//! User-facing agent for the file workspace. Not the process. Spawn, mail, and link are
//! verbs here — not tools. World-facing grants on its card must be Deny.
//! The recorder is the tape-keeping identity: every verb is a tape append,
//! and its state (seats, bounds, flags) is a projection rebuildable from
//! the tape — see `restore_seats`. With world grants all Deny, the tape is
//! its only effect channel.

mod product;
mod recover;
mod restore;
mod signal;
mod spawn;
mod subwp;
mod wp;

pub use recover::{RecoverError, recover};
pub use restore::restore_seats;
pub use subwp::{SubWp, SubWpError};
pub use wp::{CloseOut, DepEdge, Resolve, VerId, Wp, WpError, WriteMode};

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::act::{Audience, Charter, Permit};
use crate::memstream::{AppendEvent, FromKind, Kind};
use crate::principal::card::AgentCard;

use crate::agent::{Agent, SpawnError, spawn_with};
use crate::seat::{Pool, PoolError, Seat, TreeError, Turn};

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("recorder grant {0} is not deny")]
    GrantNotDenied(String),
    #[error("worker grant {0} is outside the spawn bound")]
    OutsideBound(String),
    #[error("signal {0} is outside the charter")]
    SignalDenied(&'static str),
    #[error("lite charter must not grant file access")]
    LiteFile,
    #[error("spawn bound is wider than the parent charter")]
    WiderThanParent,
    #[error("worker {0} has no sub-workspace")]
    NoSubwp(Uuid),
    #[error("no turn is running")]
    Idle,
    #[error("a turn is still running")]
    Busy,
    #[error("queue is empty")]
    Empty,
    #[error("worker is not mounted: {0}")]
    NotMounted(Uuid),
    #[error(transparent)]
    Spawn(#[from] SpawnError),
    #[error(transparent)]
    Pool(#[from] PoolError),
    #[error(transparent)]
    Tree(#[from] TreeError),
    #[error(transparent)]
    Wp(#[from] WpError),
    #[error("store: {0}")]
    Store(#[from] crate::memstream::StoreError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrival {
    /// Mail to the worker that already holds the turn.
    Steered,
    /// Held on the recorder until the pool is idle.
    Queued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    Work,
    Collect,
}

/// Pointers to a child product. Bodies stay in the child session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductPtr {
    pub child_session: String,
    pub tape_sha256: String,
    pub md_sha256: Option<String>,
    pub tree_id: Option<String>,
    pub subwp_sha256: Option<String>,
}

impl ProductPtr {
    pub fn from_tape_file(
        child_session: impl Into<String>,
        tape: &Path,
        md_sha256: Option<String>,
        tree_id: Option<String>,
        subwp_sha256: Option<String>,
    ) -> Result<Self, RecorderError> {
        Ok(Self {
            child_session: child_session.into(),
            tape_sha256: hash_file(tape)?,
            md_sha256,
            tree_id,
            subwp_sha256,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub worker: Uuid,
    pub text: String,
}

pub struct Recorder {
    agent: Arc<Agent>,
    pool: Pool,
    seats: Mutex<Vec<Seat>>,
    queue: Mutex<VecDeque<Intent>>,
    root: PathBuf,
    wp: Wp,
    bounds: Mutex<HashMap<Uuid, Charter>>,
    flags: Mutex<HashMap<Uuid, Arc<AtomicBool>>>,
    registry: Vec<(String, crate::principal::factory::ToolCtor)>,
}

/// In-process cancel handle. It only observes flags down the parent chain.
/// Rights and message text stay on the tape.
pub struct Lease {
    id: Uuid,
    flags: Vec<Arc<AtomicBool>>,
}

impl Lease {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn is_cancelled(&self) -> bool {
        self.flags.iter().any(|flag| flag.load(Ordering::Relaxed))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredSeat {
    pub id: Uuid,
    pub parent: Uuid,
    pub lite: bool,
    pub subwp: Option<String>,
}

impl Recorder {
    /// `config` and `context` are projection snapshots (config text, clip-id
    /// list). They are written beside the tape and also recorded as events
    /// so `recover` can rebuild the files. The prompt projection is the card
    /// hash, not a second copy of the prompt text. `registry` is the tool
    /// constructors for this recorder and every child it spawns.
    pub fn open(
        card: &AgentCard,
        sessions_root: impl AsRef<Path>,
        config: &str,
        context: &str,
        registry: &[(&str, crate::principal::factory::ToolCtor)],
    ) -> Result<Self, RecorderError> {
        deny_world_grants(card)?;
        let root = sessions_root.as_ref().to_path_buf();
        let registry: Vec<(String, crate::principal::factory::ToolCtor)> = registry
            .iter()
            .map(|(name, ctor)| ((*name).to_string(), *ctor))
            .collect();
        let agent = {
            let listed: Vec<(&str, crate::principal::factory::ToolCtor)> = registry
                .iter()
                .map(|(name, ctor)| (name.as_str(), *ctor))
                .collect();
            Arc::new(spawn_with(card, &root, &listed)?)
        };
        let pool = Pool::new(1)?;
        let seat = pool.tree().mount(Arc::clone(&agent), None)?;
        note_projection(&agent, "prompt", agent.card_hash())?;
        note_projection(&agent, "config", config)?;
        note_projection(&agent, "context", context)?;
        write_if_absent(
            &projection_path(agent.session().root(), "prompt"),
            agent.card_hash(),
        )?;
        write_if_absent(&projection_path(agent.session().root(), "config"), config)?;
        write_if_absent(&projection_path(agent.session().root(), "context"), context)?;
        let wp = Wp::open(&root)?;
        let recorder_id = agent.id();
        Ok(Self {
            agent,
            pool,
            seats: Mutex::new(vec![seat]),
            queue: Mutex::new(VecDeque::new()),
            root,
            wp,
            bounds: Mutex::new(HashMap::new()),
            flags: Mutex::new(HashMap::from([(
                recorder_id,
                Arc::new(AtomicBool::new(false)),
            )])),
            registry,
        })
    }

    fn listed_registry(&self) -> Vec<(&str, crate::principal::factory::ToolCtor)> {
        self.registry
            .iter()
            .map(|(name, ctor)| (name.as_str(), *ctor))
            .collect()
    }

    pub fn id(&self) -> Uuid {
        self.agent.id()
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    pub fn workspace(&self) -> &Wp {
        &self.wp
    }

    fn charter(&self, worker: Uuid) -> Result<Charter, RecorderError> {
        self.bounds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&worker)
            .cloned()
            .ok_or(RecorderError::NotMounted(worker))
    }

    fn record_both(
        &self,
        from: &Agent,
        to: &Agent,
        tag: &str,
        text: &str,
    ) -> Result<String, RecorderError> {
        let body = json!({
            "to": to.id_str(),
            "text": text,
        })
        .to_string();
        let origin = self.append(from, tag, body, vec![])?;
        let back = json!({
            "from": from.id_str(),
            "text": text,
        })
        .to_string();
        self.append(to, tag, back, vec![origin.clone()])?;
        Ok(origin)
    }

    fn append(
        &self,
        agent: &Agent,
        tag: &str,
        content: String,
        refs: Vec<String>,
    ) -> Result<String, RecorderError> {
        let event = agent.session().tape().append(AppendEvent {
            from: agent.id_str().to_string(),
            from_kind: FromKind::Agent,
            kind: Kind::Action,
            session: agent.session().id_str().to_string(),
            content,
            tags: vec![tag.to_string()],
            refs,
            act: None,
            card: Some(agent.card_hash().to_string()),
        })?;
        Ok(event.id)
    }
}

fn hash_file(path: &Path) -> Result<String, RecorderError> {
    let bytes = fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn deny_world_grants(card: &AgentCard) -> Result<(), RecorderError> {
    for grant in &card.tools {
        if grant.tag.permit != Permit::Deny {
            return Err(RecorderError::GrantNotDenied(grant.name.clone()));
        }
    }
    Ok(())
}

fn note_projection(agent: &Agent, tag: &str, content: &str) -> Result<(), RecorderError> {
    agent.session().tape().append(AppendEvent {
        from: agent.id_str().to_string(),
        from_kind: FromKind::Agent,
        kind: Kind::Utterance,
        session: agent.session().id_str().to_string(),
        content: content.to_string(),
        tags: vec![tag.to_string()],
        refs: vec![],
        act: None,
        card: Some(agent.card_hash().to_string()),
    })?;
    Ok(())
}

pub(crate) fn projection_path(root: &Path, name: &str) -> PathBuf {
    root.join(name)
}

fn write_if_absent(path: &Path, content: &str) -> Result<(), RecorderError> {
    if path.exists() {
        return Ok(());
    }
    fs::write(path, content)?;
    Ok(())
}

fn view_name(content: &str) -> Option<String> {
    let v: Value = serde_json::from_str(content).ok()?;
    v["name"].as_str().map(str::to_string)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
