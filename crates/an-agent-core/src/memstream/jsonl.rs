use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use thiserror::Error;

use super::event::{AppendEvent, Memevent};
use crate::det_seam::{Clock, Entropy};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("tape belongs to process {owner}; inherited handle is invalid in process {current}")]
    InheritedProcess { owner: u32, current: u32 },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("tape already has a writer: {0}")]
    WriterBusy(PathBuf),
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
    #[cfg(test)]
    fail_append_after: Option<usize>,
    #[cfg(test)]
    fail_io_at: Option<&'static str>,
    file: File,
    next_seq: Option<u64>,
    /// The tape's last line is valid but lacks a trailing newline; the next
    /// append must start with one or the two lines fuse.
    needs_newline: bool,
}

pub struct JsonlStore {
    owner_pid: u32,
    pub(super) path: PathBuf,
    state: Mutex<State>,
    clock: Clock,
    entropy: Mutex<Entropy>,
}

impl Drop for JsonlStore {
    fn drop(&mut self) {
        // An inherited open file description shares its lock with the parent.
        // Child destruction must close its descriptor without unlocking that lock.
        if self.owner_pid != std::process::id() {
            return;
        }
        // A concurrent fork may briefly inherit the open file description before
        // exec closes it. Close alone would keep the parent's lock alive there.
        // Only this store owns writing; inherited descriptors confer no ownership.
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        let _ = state.file.unlock();
    }
}

impl JsonlStore {
    /// Reject inherited handles before touching potentially inherited mutexes or I/O.
    pub(super) fn check_owner(&self) -> Result<(), StoreError> {
        let current = std::process::id();
        if self.owner_pid != current {
            return Err(StoreError::InheritedProcess {
                owner: self.owner_pid,
                current,
            });
        }
        Ok(())
    }

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
        // Lifetime ownership precedes every scan, repair and sequence reservation.
        // Lock the tape itself so symlink/hard-link aliases share the same lock.
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;
        match file.try_lock() {
            Ok(()) => (),
            Err(fs::TryLockError::WouldBlock) => return Err(StoreError::WriterBusy(path)),
            Err(fs::TryLockError::Error(error)) => return Err(error.into()),
        }
        sync_parent_dir(path.parent())?;
        Ok(Self {
            owner_pid: std::process::id(),
            path,
            state: Mutex::new(State {
                #[cfg(test)]
                fail_append_after: None,
                #[cfg(test)]
                fail_io_at: None,
                file,
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
        self.check_owner()?;
        let mut st = self.state.lock().map_err(|_| StoreError::Poisoned)?;
        // Failed repair can leave a tail in place. Never retain a cached
        // sequence that would let the next append bypass reconciliation.
        st.next_seq = None;
        self.scan_and_heal(&mut st)
    }

    /// Appends one event. The write lock spans seq reservation, the single
    /// write, and fsync: concurrent appends can neither duplicate nor skip
    /// a seq. A write/sync error invalidates the cached sequence; a retained
    /// complete event is consumed on rescan, not silently appended again.
    pub fn append(&self, input: AppendEvent) -> Result<Memevent, StoreError> {
        self.check_owner()?;
        let mut st = self.state.lock().map_err(|_| StoreError::Poisoned)?;
        #[cfg(test)]
        if let Some(remaining) = st.fail_append_after {
            st.fail_append_after = remaining.checked_sub(1);
            if remaining == 0 {
                return Err(StoreError::Io(std::io::Error::other(
                    "injected append failure",
                )));
            }
        }
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
        let written = (|| -> std::io::Result<()> {
            #[cfg(test)]
            if st.fail_io_at == Some("partial_write") {
                st.file.write_all(&line.as_bytes()[..line.len() / 2])?;
                io_checkpoint(&mut st, "partial_write")?;
            }
            st.file.write_all(line.as_bytes())?;
            #[cfg(test)]
            io_checkpoint(&mut st, "append_sync")?;
            st.file.sync_data()
        })()
        .map_err(StoreError::Io);
        if let Err(e) = written {
            // The tape may hold a torn line now. Force the next append to
            // rescan (and heal) instead of piling onto it. A complete retained
            // line consumes its seq even when its sync reported an error.
            st.next_seq = None;
            return Err(e);
        }
        st.needs_newline = false;
        st.next_seq = Some(seq + 1);
        Ok(event)
    }

    #[cfg(test)]
    pub(crate) fn fail_next_append(&self) {
        self.fail_append_after(0);
    }

    /// Test-only fault injection after skipping this many append attempts.
    #[cfg(test)]
    pub(crate) fn fail_append_after(&self, skipped: usize) {
        self.state
            .lock()
            .expect("test store lock")
            .fail_append_after = Some(skipped);
    }

    fn scan_and_heal(&self, st: &mut State) -> Result<Vec<Memevent>, StoreError> {
        self.check_owner()?;
        st.needs_newline = false;
        st.file.rewind()?;
        let mut bytes = Vec::new();
        st.file.read_to_end(&mut bytes)?;
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
        self.check_owner()?;
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
        #[cfg(test)]
        io_checkpoint(st, "before_truncate")?;
        st.file.set_len(offset)?;
        #[cfg(test)]
        io_checkpoint(st, "truncate_sync")?;
        st.file.sync_data()?;
        Ok(())
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

    pub(super) fn tmp() -> (TempDir, PathBuf) {
        let dir = TempDir::new("mem");
        let path = dir.path().join("memory.jsonl");
        (dir, path)
    }

    pub(super) fn base(content: &str) -> AppendEvent {
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

#[cfg(test)]
fn io_checkpoint(st: &mut State, stage: &'static str) -> std::io::Result<()> {
    if st.fail_io_at == Some(stage) {
        st.fail_io_at = None;
        return Err(std::io::Error::other(format!(
            "injected tape failure: {stage}"
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "writer_tests.rs"]
mod writer_tests;

#[cfg(test)]
#[path = "fork_owner_tests.rs"]
mod fork_owner_tests;
