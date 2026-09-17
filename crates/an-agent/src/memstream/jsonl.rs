use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use thiserror::Error;

use super::event::{AppendEvent, Memevent};
use crate::det_seam::{Clock, Entropy};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("store lock poisoned")]
    Poisoned,
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("corrupt memevent: {0}")]
    Corrupt(String),
    #[error("corrupt tape: line {0} is not a valid event; refusing the store")]
    CorruptLine(usize),
}

struct State {
    file: Option<File>,
    next_seq: Option<u64>,
    /// The tape's last line is valid but lacks a trailing newline; the next
    /// append must start with one or the two lines fuse.
    needs_newline: bool,
}

pub struct JsonlStore {
    path: PathBuf,
    state: Mutex<State>,
    clock: Clock,
    entropy: Mutex<Entropy>,
}

impl JsonlStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with(path, Clock::wall(), Entropy::os())
    }

    pub fn open_with_clock(path: impl AsRef<Path>, clock: Clock) -> Result<Self, StoreError> {
        Self::open_with(path, clock, Entropy::os())
    }

    pub fn open_with(
        path: impl AsRef<Path>,
        clock: Clock,
        entropy: Entropy,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        Ok(Self {
            path,
            state: Mutex::new(State {
                file: None,
                next_seq: None,
                needs_newline: false,
            }),
            clock,
            entropy: Mutex::new(entropy),
        })
    }

    /// Reads every event. Corrupt bytes are never returned as events: a
    /// crash-torn final line is quarantined (see `quarantine_tail`);
    /// corruption anywhere else fails the whole store.
    pub fn read_all(&self) -> Result<Vec<Memevent>, StoreError> {
        let mut st = self.state.lock().map_err(|_| StoreError::Poisoned)?;
        self.scan_and_heal(&mut st)
    }

    /// Appends one event. The write lock spans seq reservation, the single
    /// write, and fsync: concurrent appends can neither duplicate nor skip
    /// a seq, and seq advances only after the durable write succeeds.
    pub fn append(&self, input: AppendEvent) -> Result<Memevent, StoreError> {
        let mut st = self.state.lock().map_err(|_| StoreError::Poisoned)?;
        let seq = match st.next_seq {
            Some(n) => n,
            None => {
                let events = self.scan_and_heal(&mut st)?;
                let n = events.last().map(|e| e.seq + 1).unwrap_or(1);
                st.next_seq = Some(n);
                n
            }
        };
        let now = self.clock.now_ms();
        let ts = Clock::format_ms(now);
        let id = self
            .entropy
            .lock()
            .map_err(|_| StoreError::Poisoned)?
            .ulid(now)
            .to_string();
        let event = Memevent {
            v: 1,
            id,
            seq,
            ts,
            from: input.from,
            from_kind: input.from_kind,
            kind: input.kind,
            session: Some(input.session),
            content: input.content,
            tags: input.tags,
            refs: input.refs,
            act: input.act,
            card: input.card,
        };
        event.validate().map_err(StoreError::Corrupt)?;
        let mut line = serde_json::to_string(&event)?;
        if st.needs_newline {
            line.insert(0, '\n');
        }
        line.push('\n');
        let written = match self.append_file(&mut st) {
            Ok(file) => file
                .write_all(line.as_bytes())
                .and_then(|()| file.sync_data())
                .map_err(StoreError::Io),
            Err(e) => Err(e),
        };
        if let Err(e) = written {
            // The tape may hold a torn line now. Force the next append to
            // rescan (and heal) instead of piling onto it; seq is unconsumed.
            st.next_seq = None;
            return Err(e);
        }
        st.needs_newline = false;
        st.next_seq = Some(seq + 1);
        Ok(event)
    }

    fn scan_and_heal(&self, st: &mut State) -> Result<Vec<Memevent>, StoreError> {
        st.needs_newline = false;
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(&self.path)?;
        let mut events = Vec::new();
        let mut start = 0usize;
        let mut line_no = 0usize;
        let mut healed = false;
        while start < bytes.len() {
            let (end, terminated) = match bytes[start..].iter().position(|b| *b == b'\n') {
                Some(p) => (start + p + 1, true),
                None => (bytes.len(), false),
            };
            line_no += 1;
            let chunk = &bytes[start..end];
            if chunk.iter().any(|b| !b.is_ascii_whitespace()) {
                match serde_json::from_slice::<Memevent>(chunk) {
                    Ok(event) => {
                        event.validate().map_err(StoreError::Corrupt)?;
                        events.push(event);
                    }
                    // An unterminated final line that fails to parse is a
                    // crash-torn tail: quarantine it, never serve it.
                    Err(_) if !terminated => {
                        self.quarantine_tail(st, start as u64, chunk)?;
                        healed = true;
                        break;
                    }
                    Err(_) => return Err(StoreError::CorruptLine(line_no)),
                }
            }
            start = end;
        }
        st.needs_newline = !healed && bytes.last().is_some_and(|b| *b != b'\n');
        Ok(events)
    }

    /// Moves a crash-torn tail out of the tape: appended to
    /// `<tape>.corrupt` with a header (evidence kept), then truncated from
    /// the tape. The side file is written before the truncation, so a
    /// crash in between duplicates the bytes rather than losing them.
    fn quarantine_tail(&self, st: &mut State, offset: u64, torn: &[u8]) -> Result<(), StoreError> {
        let mut side = self.path.clone().into_os_string();
        side.push(".corrupt");
        let side = PathBuf::from(side);
        let existed = side.exists();
        let mut f = OpenOptions::new().create(true).append(true).open(&side)?;
        let header = format!(
            "# quarantined {} ({} bytes)\n",
            Clock::format_ms(self.clock.now_ms()),
            torn.len()
        );
        f.write_all(header.as_bytes())?;
        f.write_all(torn)?;
        if !torn.ends_with(b"\n") {
            f.write_all(b"\n")?;
        }
        f.sync_data()?;
        if !existed {
            sync_parent_dir(side.parent())?;
        }
        let file = self.append_file(st)?;
        file.set_len(offset)?;
        file.sync_data()?;
        Ok(())
    }

    fn append_file<'a>(&self, st: &'a mut State) -> Result<&'a mut File, StoreError> {
        if st.file.is_none() {
            let existed = self.path.exists();
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            if !existed {
                sync_parent_dir(self.path.parent())?;
            }
            st.file = Some(file);
        }
        match st.file.as_mut() {
            Some(file) => Ok(file),
            None => Err(StoreError::Io(std::io::Error::other(
                "append fd unexpectedly closed",
            ))),
        }
    }
}

/// Fsync the parent directory so a just-created file survives a crash.
/// Directory handles open read-only on unix; elsewhere this is a no-op.
#[cfg(unix)]
fn sync_parent_dir(dir: Option<&Path>) -> Result<(), StoreError> {
    if let Some(dir) = dir {
        File::open(dir)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_dir(_dir: Option<&Path>) -> Result<(), StoreError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::det_seam::{Clock, Entropy};
    use crate::memstream::{FromKind, Kind};
    use crate::testkit::TempDir;

    fn tmp() -> (TempDir, PathBuf) {
        let dir = TempDir::new("mem");
        let path = dir.path().join("memory.jsonl");
        (dir, path)
    }

    fn base(content: &str) -> AppendEvent {
        AppendEvent {
            from: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".into(),
            from_kind: FromKind::Unknown,
            kind: Kind::Utterance,
            session: "11111111-1111-1111-1111-111111111111".into(),
            content: content.into(),
            tags: vec![],
            refs: vec![],
            act: None,
            card: None,
        }
    }

    const GOOD_LINE: &str = r#"{"v":1,"id":"01TEST","seq":7,"ts":"2026-01-01T00:00:00.000Z","from":"a","from_kind":"agent","kind":"utterance","session":"s","content":"prior","tags":[],"refs":[]}"#;

    #[test]
    fn seq_increases_and_clock_is_injected() {
        let (_guard, path) = tmp();
        let store = JsonlStore::open_with_clock(&path, Clock::frozen(0)).unwrap();
        let first = store.append(base("hi")).unwrap();
        let second = store.append(base("there")).unwrap();
        assert_eq!(first.seq, 1);
        assert_eq!(second.seq, 2);
        assert_eq!(first.ts, "1970-01-01T00:00:00.000Z");
        assert_eq!(store.read_all().unwrap().len(), 2);
    }

    #[test]
    fn continues_seq_from_existing_file() {
        let (_guard, path) = tmp();
        fs::write(&path, format!("{GOOD_LINE}\n")).unwrap();
        let store = JsonlStore::open_with_clock(&path, Clock::frozen(0)).unwrap();
        assert_eq!(store.append(base("after")).unwrap().seq, 8);
    }

    #[test]
    fn seeded_entropy_reproduces_ids() {
        let seed = [3u8; 32];
        let (_ga, pa) = tmp();
        let (_gb, pb) = tmp();
        let a = JsonlStore::open_with(pa, Clock::frozen(0), Entropy::seeded(seed)).unwrap();
        let b = JsonlStore::open_with(pb, Clock::frozen(0), Entropy::seeded(seed)).unwrap();
        assert_eq!(
            a.append(base("hi")).unwrap().id,
            b.append(base("hi")).unwrap().id
        );
    }

    #[test]
    fn reads_legacy_line_without_act() {
        let (_guard, path) = tmp();
        let line = r#"{"v":1,"id":"01TEST","seq":1,"ts":"2026-01-01T00:00:00.000Z","from":"a","from_kind":"human","kind":"utterance","content":"legacy","tags":["utterance"],"refs":[]}"#;
        fs::write(&path, format!("{line}\n")).unwrap();
        let store = JsonlStore::open(&path).unwrap();
        let all = store.read_all().unwrap();
        assert_eq!(all[0].content, "legacy");
        assert!(all[0].session.is_none());
        assert!(all[0].act.is_none());
    }

    #[test]
    fn corrupt_line_fails() {
        let (_guard, path) = tmp();
        fs::write(&path, "not-json\n").unwrap();
        let store = JsonlStore::open(&path).unwrap();
        assert!(matches!(store.read_all(), Err(StoreError::CorruptLine(1))));
    }

    #[test]
    fn mid_tape_corruption_fails_the_store() {
        let (_guard, path) = tmp();
        fs::write(&path, format!("{GOOD_LINE}\nnot-json\n{GOOD_LINE}\n")).unwrap();
        let store = JsonlStore::open(&path).unwrap();
        assert!(matches!(store.read_all(), Err(StoreError::CorruptLine(2))));
    }

    #[test]
    fn torn_tail_is_quarantined_and_append_continues() {
        let (_guard, path) = tmp();
        let torn = r#"{"v":1,"id":"01TE"#;
        fs::write(&path, format!("{GOOD_LINE}\n{torn}")).unwrap();
        let store = JsonlStore::open(&path).unwrap();

        let events = store.read_all().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].seq, 7);

        // Tape truncated to the last good line; torn bytes kept aside.
        let raw = fs::read_to_string(&path).unwrap();
        assert_eq!(raw, format!("{GOOD_LINE}\n"));
        let side = fs::read_to_string(dir_corrupt(&path)).unwrap();
        assert!(side.contains(torn));

        let ev = store.append(base("after")).unwrap();
        assert_eq!(ev.seq, 8);
        assert_eq!(store.read_all().unwrap().len(), 2);
    }

    #[test]
    fn unterminated_valid_tail_is_separated_from_next_append() {
        let (_guard, path) = tmp();
        fs::write(&path, GOOD_LINE).unwrap();
        let store = JsonlStore::open_with_clock(&path, Clock::frozen(0)).unwrap();
        assert_eq!(store.read_all().unwrap().len(), 1);
        let ev = store.append(base("after")).unwrap();
        assert_eq!(ev.seq, 8);
        let raw = fs::read_to_string(&path).unwrap();
        assert_eq!(raw.lines().count(), 2);
        assert_eq!(store.read_all().unwrap().len(), 2);
    }

    #[test]
    fn concurrent_appends_never_share_a_seq() {
        use std::collections::HashSet;
        use std::sync::Arc;
        use std::thread;

        let (_guard, path) = tmp();
        let store = Arc::new(JsonlStore::open_with_clock(&path, Clock::frozen(0)).unwrap());
        let mut handles = Vec::new();
        for _ in 0..4 {
            let s = Arc::clone(&store);
            handles.push(thread::spawn(move || {
                for _ in 0..25 {
                    s.append(base("x")).unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let events = store.read_all().unwrap();
        assert_eq!(events.len(), 100);
        let seqs: HashSet<u64> = events.iter().map(|e| e.seq).collect();
        assert_eq!(seqs.len(), 100);
        assert!(seqs.contains(&1) && seqs.contains(&100));
    }

    fn dir_corrupt(path: &Path) -> PathBuf {
        let mut side = path.to_path_buf().into_os_string();
        side.push(".corrupt");
        PathBuf::from(side)
    }
}
