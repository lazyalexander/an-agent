use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{TimeZone, Utc};
use thiserror::Error;
use ulid::Ulid;

use super::event::{AppendEvent, Memevent};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("corrupt memevent: {0}")]
    Corrupt(String),
}

pub type Clock = Box<dyn Fn() -> u64 + Send + Sync>;

pub struct JsonlStore {
    path: PathBuf,
    next_seq: Mutex<Option<u64>>,
    now_ms: Clock,
}

impl JsonlStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_clock(path, Box::new(|| Utc::now().timestamp_millis() as u64))
    }

    pub fn open_with_clock(path: impl AsRef<Path>, now_ms: Clock) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        Ok(Self {
            path,
            next_seq: Mutex::new(None),
            now_ms,
        })
    }

    pub fn read_all(&self) -> Result<Vec<Memevent>, StoreError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let file = File::open(&self.path)?;
        let mut events = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let event: Memevent = serde_json::from_str(&line)?;
            event.validate().map_err(StoreError::Corrupt)?;
            events.push(event);
        }
        Ok(events)
    }

    pub fn append(&self, input: AppendEvent) -> Result<Memevent, StoreError> {
        let seq = self.peek_next_seq()?;
        let ts = iso_ms((self.now_ms)());
        let event = Memevent {
            v: 1,
            id: Ulid::new().to_string(),
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
        };
        event.validate().map_err(StoreError::Corrupt)?;
        let mut file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(&event)?)?;
        *self.next_seq.lock().expect("seq lock") = Some(seq + 1);
        Ok(event)
    }

    fn peek_next_seq(&self) -> Result<u64, StoreError> {
        let mut slot = self.next_seq.lock().expect("seq lock");
        if let Some(n) = *slot {
            return Ok(n);
        }
        let n = self.read_all()?.last().map(|e| e.seq + 1).unwrap_or(1);
        *slot = Some(n);
        Ok(n)
    }
}

fn iso_ms(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let nsec = ((ms % 1000) * 1_000_000) as u32;
    Utc.timestamp_opt(secs, nsec)
        .single()
        .unwrap_or(Utc.timestamp_opt(0, 0).single().expect("epoch"))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memstream::{FromKind, Kind};

    fn tmp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("an-agent-mem-{}", Ulid::new()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("memory.jsonl")
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
        }
    }

    #[test]
    fn seq_increases_and_clock_is_injected() {
        let path = tmp();
        let store = JsonlStore::open_with_clock(&path, Box::new(|| 0)).unwrap();
        let first = store.append(base("hi")).unwrap();
        let second = store.append(base("there")).unwrap();
        assert_eq!(first.seq, 1);
        assert_eq!(second.seq, 2);
        assert_eq!(first.ts, "1970-01-01T00:00:00.000Z");
        assert_eq!(store.read_all().unwrap().len(), 2);
    }

    #[test]
    fn continues_seq_from_existing_file() {
        let path = tmp();
        fs::write(
            &path,
            r#"{"v":1,"id":"01TEST","seq":7,"ts":"2026-01-01T00:00:00.000Z","from":"a","from_kind":"agent","kind":"utterance","session":"s","content":"prior","tags":[],"refs":[]}
"#,
        )
        .unwrap();
        let store = JsonlStore::open_with_clock(&path, Box::new(|| 0)).unwrap();
        assert_eq!(store.append(base("after")).unwrap().seq, 8);
    }

    #[test]
    fn reads_legacy_line_without_act() {
        let path = tmp();
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
        let path = tmp();
        fs::write(&path, "not-json\n").unwrap();
        let store = JsonlStore::open(&path).unwrap();
        assert!(store.read_all().is_err());
    }
}
