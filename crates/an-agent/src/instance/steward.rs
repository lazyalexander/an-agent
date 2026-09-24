//! The one user-facing agent in an instance. Spawn, mail, and link are
//! verbs here — not tools. World-facing grants on its card must be Deny.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::act::{ActSentence, Audience, Permit};
use crate::memstream::{AppendEvent, FromKind, Kind};
use crate::principal::card::AgentCard;

use super::agent::Agent;
use super::pool::{Pool, PoolError, Turn};
use super::tree::Seat;
use super::wp::{CloseOut, DepEdge, Wp, WpError};
use super::{InstanceError, spawn};

#[derive(Debug, Error)]
pub enum StewardError {
    #[error("steward grant {0} is not deny")]
    GrantNotDenied(String),
    #[error("worker grant {0} is outside the spawn bound")]
    OutsideBound(String),
    #[error("signal {0} is outside the charter")]
    SignalDenied(&'static str),
    #[error("lite charter must not grant file access")]
    LiteFile,
    #[error("spawn bound is wider than the parent charter")]
    WiderThanParent,
    #[error("worker {0} has no sub-workplace")]
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
    Instance(#[from] InstanceError),
    #[error(transparent)]
    Pool(#[from] PoolError),
    #[error(transparent)]
    Tree(#[from] super::tree::TreeError),
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
    /// Held on the steward until the pool is idle.
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
    ) -> Result<Self, StewardError> {
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

pub struct Steward {
    agent: Arc<Agent>,
    pool: Pool,
    seats: Mutex<Vec<Seat>>,
    queue: Mutex<VecDeque<Intent>>,
    root: PathBuf,
    wp: Wp,
    bounds: Mutex<HashMap<Uuid, ActSentence>>,
    flags: Mutex<HashMap<Uuid, Arc<AtomicBool>>>,
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

impl Steward {
    /// `config` and `context` are projection snapshots (config text, clip-id
    /// list). They are written beside the tape and also recorded as events
    /// so `recover` can rebuild the files. The prompt projection is the card
    /// hash, not a second copy of the prompt text.
    pub fn open(
        card: &AgentCard,
        sessions_root: impl AsRef<Path>,
        config: &str,
        context: &str,
    ) -> Result<Self, StewardError> {
        deny_world_grants(card)?;
        let root = sessions_root.as_ref().to_path_buf();
        let agent = Arc::new(spawn(card, &root)?);
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
        let steward_id = agent.id();
        Ok(Self {
            agent,
            pool,
            seats: Mutex::new(vec![seat]),
            queue: Mutex::new(VecDeque::new()),
            root,
            wp,
            bounds: Mutex::new(HashMap::new()),
            flags: Mutex::new(HashMap::from([(
                steward_id,
                Arc::new(AtomicBool::new(false)),
            )])),
        })
    }

    pub fn id(&self) -> Uuid {
        self.agent.id()
    }

    pub fn agent(&self) -> &Agent {
        &self.agent
    }

    pub fn workplace(&self) -> &Wp {
        &self.wp
    }

    /// `bound` is the parent's charter for this child. It is written on the
    /// steward tape. Every tool grant on `card` must fit inside it.
    /// Registers one sub-workplace for the child.
    pub fn spawn_worker(
        &self,
        card: &AgentCard,
        bound: &ActSentence,
    ) -> Result<Arc<Agent>, StewardError> {
        self.spawn_child(self.agent.id(), card, bound, true)
    }

    /// Spawn under an existing child. The new bound must fit in that child's charter.
    pub fn spawn_under(
        &self,
        parent: Uuid,
        card: &AgentCard,
        bound: &ActSentence,
    ) -> Result<Arc<Agent>, StewardError> {
        self.spawn_child(parent, card, bound, true)
    }

    /// A child with a seat and no sub-workplace. It cannot write paths.
    /// The charter's file face must be `none`. Finishing is `complete` only.
    pub fn spawn_lite(
        &self,
        card: &AgentCard,
        bound: &ActSentence,
    ) -> Result<Arc<Agent>, StewardError> {
        if !bound_is_fileless(bound) {
            return Err(StewardError::LiteFile);
        }
        self.spawn_child(self.agent.id(), card, bound, false)
    }

    /// Drop this seat and every descendant. Live sub-workplaces are abandoned
    /// with no view. Already published views stay.
    pub fn release(&self, id: Uuid) -> Result<(), StewardError> {
        let ids = self.pool.tree().descendants(id)?;
        self.append(
            &self.agent,
            "release",
            json!({ "id": id.to_string() }).to_string(),
            vec![],
        )?;
        for worker in &ids {
            self.wp.abandon_worker(*worker)?;
            self.bounds
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(worker);
            self.flags
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(worker);
        }
        let mut seats = self.seats.lock().unwrap_or_else(|e| e.into_inner());
        let mut keep = Vec::new();
        let mut dropping = Vec::new();
        for seat in seats.drain(..) {
            if ids.contains(&seat.id()) {
                dropping.push(seat);
            } else {
                keep.push(seat);
            }
        }
        *seats = keep;
        drop(seats);
        for seat in dropping {
            let _ = seat.unmount();
        }
        Ok(())
    }

    /// Parent cancels a direct child. Recorded on both tapes. A child cannot
    /// cancel its parent or a sibling.
    pub fn cancel(&self, parent: Uuid, child: Uuid) -> Result<String, StewardError> {
        let actual = self.pool.tree().parent(child)?;
        if actual != Some(parent) {
            return Err(StewardError::SignalDenied("cancel"));
        }
        let child_agent = self
            .pool
            .tree()
            .get(child)
            .ok_or(StewardError::NotMounted(child))?;
        let parent_agent = self
            .pool
            .tree()
            .get(parent)
            .ok_or(StewardError::NotMounted(parent))?;
        self.record_both(&parent_agent, &child_agent, "cancel", "cancel")
    }

    /// Set this seat's cancel flag and record cancel down every child edge.
    /// The flag is the only thing a [`Lease`] carries.
    pub fn propagate_cancel(&self, id: Uuid) -> Result<(), StewardError> {
        if self
            .flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .is_none()
            && id != self.agent.id()
        {
            return Err(StewardError::NotMounted(id));
        }
        self.mark_cancelled(id);
        let children = self.pool.tree().children(id).unwrap_or_default();
        for child in children {
            self.propagate_cancel(child)?;
        }
        if id != self.agent.id() {
            let parent = self
                .pool
                .tree()
                .parent(id)?
                .ok_or(StewardError::NotMounted(id))?;
            self.cancel(parent, id)?;
        }
        Ok(())
    }

    pub fn lease(&self, id: Uuid) -> Result<Lease, StewardError> {
        let flags_map = self.flags.lock().unwrap_or_else(|e| e.into_inner());
        let mut flags = Vec::new();
        let mut cursor = id;
        loop {
            let flag = flags_map
                .get(&cursor)
                .cloned()
                .ok_or(StewardError::NotMounted(cursor))?;
            flags.push(flag);
            if cursor == self.agent.id() {
                break;
            }
            cursor = self
                .pool
                .tree()
                .parent(cursor)?
                .ok_or(StewardError::NotMounted(cursor))?;
        }
        Ok(Lease { id, flags })
    }

    /// Child finishes and hands a result to its parent.
    /// Denied when the charter's complete face is Deny.
    pub fn complete(&self, child: Uuid, text: &str) -> Result<String, StewardError> {
        let parent = self
            .pool
            .tree()
            .parent(child)?
            .ok_or(StewardError::NotMounted(child))?;
        let bound = self.charter(child)?;
        if bound.signal().complete == Permit::Deny {
            return Err(StewardError::SignalDenied("complete"));
        }
        let child_agent = self
            .pool
            .tree()
            .get(child)
            .ok_or(StewardError::NotMounted(child))?;
        let parent_agent = self
            .pool
            .tree()
            .get(parent)
            .ok_or(StewardError::NotMounted(parent))?;
        self.record_both(&child_agent, &parent_agent, "complete", text)
    }

    /// Child sends a message. The parent is always allowed.
    /// Anyone else requires `audience: any`.
    pub fn send(&self, from: Uuid, to: Uuid, text: &str) -> Result<String, StewardError> {
        let parent = self
            .pool
            .tree()
            .parent(from)?
            .ok_or(StewardError::NotMounted(from))?;
        let bound = self.charter(from)?;
        if to != parent && bound.signal().audience != Audience::Any {
            return Err(StewardError::SignalDenied("send"));
        }
        let from_agent = self
            .pool
            .tree()
            .get(from)
            .ok_or(StewardError::NotMounted(from))?;
        let to_agent = self
            .pool
            .tree()
            .get(to)
            .ok_or(StewardError::NotMounted(to))?;
        self.record_both(&from_agent, &to_agent, "send", text)
    }

    pub fn submit(&self, worker: Uuid, text: &str) -> Result<Arrival, StewardError> {
        if self.pool.is_running(worker) {
            self.mail(worker, text)?;
            return Ok(Arrival::Steered);
        }
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(Intent {
                worker,
                text: text.to_string(),
            });
        Ok(Arrival::Queued)
    }

    pub fn pop(&self) -> Result<Intent, StewardError> {
        if self.pool.running() > 0 {
            return Err(StewardError::Busy);
        }
        self.queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
            .ok_or(StewardError::Empty)
    }

    pub fn begin_turn(&self, worker: Uuid) -> Result<Turn<'_>, StewardError> {
        if self.pool.tree().get(worker).is_none() {
            return Err(StewardError::NotMounted(worker));
        }
        Ok(self.pool.begin_turn(worker)?)
    }

    pub fn mail(&self, worker: Uuid, text: &str) -> Result<String, StewardError> {
        let child = self
            .pool
            .tree()
            .get(worker)
            .ok_or(StewardError::NotMounted(worker))?;
        let body = json!({
            "to_session": child.session().id_str(),
            "text": text,
        })
        .to_string();
        let parent = self.append(&self.agent, "send", body, vec![])?;
        let back = json!({
            "to_session": self.agent.session().id_str(),
            "text": text,
        })
        .to_string();
        self.append(&child, "send", back, vec![parent.clone()])?;
        Ok(parent)
    }

    /// Publish the worker's sub-workplace if it wrote anything.
    /// Steward does not write file bytes.
    pub fn close_worker(&self, worker: Uuid, extra: &[DepEdge]) -> Result<CloseOut, StewardError> {
        let sub = self
            .wp
            .subwp_of(worker)
            .ok_or(StewardError::NoSubwp(worker))?;
        Ok(self.wp.close(&sub, extra)?)
    }

    /// Collect: record pointers only. Does not run tools.
    pub fn collect(&self, product: &ProductPtr) -> Result<String, StewardError> {
        let _kind = TurnKind::Collect;
        self.link(product)
    }

    pub fn link(&self, product: &ProductPtr) -> Result<String, StewardError> {
        let body = json!({
            "child_session": product.child_session,
            "tape_sha256": product.tape_sha256,
            "md_sha256": product.md_sha256,
            "tree_id": product.tree_id,
            "subwp_sha256": product.subwp_sha256,
        })
        .to_string();
        self.append(&self.agent, "link", body, vec![])
    }

    pub fn define_view(&self, name: &str, link_ids: &[String]) -> Result<String, StewardError> {
        let body = json!({ "name": name, "links": link_ids }).to_string();
        self.append(&self.agent, "view", body, vec![])
    }

    /// Latest definition of `name` on the steward tape. Read-only.
    pub fn view(&self, name: &str) -> Result<Vec<String>, StewardError> {
        let events = self.agent.session().tape().read_all()?;
        let found = events.iter().rev().find(|e| {
            e.tags.iter().any(|t| t == "view") && view_name(&e.content).as_deref() == Some(name)
        });
        let Some(event) = found else {
            return Err(StewardError::Empty);
        };
        let v: Value = serde_json::from_str(&event.content)?;
        let links = v["links"]
            .as_array()
            .ok_or(StewardError::Empty)?
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
        Ok(links)
    }

    fn spawn_child(
        &self,
        parent: Uuid,
        card: &AgentCard,
        bound: &ActSentence,
        with_subwp: bool,
    ) -> Result<Arc<Agent>, StewardError> {
        if self.pool.tree().get(parent).is_none() {
            return Err(StewardError::NotMounted(parent));
        }
        if parent != self.agent.id() {
            let charter = self.charter(parent)?;
            if !bound.within(&charter) {
                return Err(StewardError::WiderThanParent);
            }
        }
        for grant in &card.tools {
            if !bound.allows(&grant.tag) {
                return Err(StewardError::OutsideBound(grant.name.clone()));
            }
        }
        let child = Arc::new(spawn(card, &self.root)?);
        let seat = self.pool.tree().mount(Arc::clone(&child), Some(parent))?;
        self.seats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seat);
        let sub = if with_subwp {
            Some(self.wp.register(child.id(), child.session().root())?)
        } else {
            None
        };
        self.bounds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(child.id(), bound.clone());
        self.flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(child.id(), Arc::new(AtomicBool::new(false)));
        self.append(
            &self.agent,
            "spawn",
            json!({
                "worker": child.id_str(),
                "session": child.session().id_str(),
                "parent": parent.to_string(),
                "subwp": sub,
                "seat": if with_subwp { "worker" } else { "lite" },
                "bound": bound,
            })
            .to_string(),
            vec![],
        )?;
        Ok(child)
    }

    fn mark_cancelled(&self, id: Uuid) {
        if let Some(flag) = self
            .flags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn charter(&self, worker: Uuid) -> Result<ActSentence, StewardError> {
        self.bounds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&worker)
            .cloned()
            .ok_or(StewardError::NotMounted(worker))
    }

    fn record_both(
        &self,
        from: &Agent,
        to: &Agent,
        tag: &str,
        text: &str,
    ) -> Result<String, StewardError> {
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
    ) -> Result<String, StewardError> {
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

fn hash_file(path: &Path) -> Result<String, StewardError> {
    let bytes = fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn restore_seats(session_dir: &Path) -> Result<Vec<RestoredSeat>, StewardError> {
    use crate::memstream::JsonlStore;
    let events = JsonlStore::open(session_dir.join("memory.jsonl"))?.read_all()?;
    let mut nodes: HashMap<Uuid, RestoredSeat> = HashMap::new();
    let mut steward_id = None;
    for event in &events {
        let Ok(steward) = event.from.parse::<Uuid>() else {
            continue;
        };
        if event.tags.iter().any(|tag| tag == "spawn") {
            steward_id = Some(steward);
            let value: Value = serde_json::from_str(&event.content)?;
            let id = value["worker"]
                .as_str()
                .unwrap_or_default()
                .parse::<Uuid>()
                .map_err(|_| StewardError::NotMounted(steward))?;
            let parent = value["parent"]
                .as_str()
                .and_then(|text| text.parse().ok())
                .unwrap_or(steward);
            nodes.insert(
                id,
                RestoredSeat {
                    id,
                    parent,
                    lite: value["seat"].as_str() == Some("lite"),
                    subwp: value["subwp"].as_str().map(str::to_string),
                },
            );
        }
        if event.tags.iter().any(|tag| tag == "release") {
            let value: Value = serde_json::from_str(&event.content)?;
            if let Some(id) = value["id"].as_str().and_then(|text| text.parse().ok()) {
                remove_subtree(&mut nodes, id);
            }
        }
        if event.tags.iter().any(|tag| tag == "cancel") {
            let value: Value = serde_json::from_str(&event.content)?;
            if let Some(id) = value["to"].as_str().and_then(|text| text.parse().ok()) {
                remove_subtree(&mut nodes, id);
            }
        }
    }
    let Some(steward_id) = steward_id else {
        return Ok(Vec::new());
    };
    let live: Vec<Uuid> = nodes
        .keys()
        .copied()
        .filter(|id| parent_chain_reaches(*id, &nodes, steward_id))
        .collect();
    nodes.retain(|id, _| live.contains(id));
    let mut restored: Vec<_> = nodes.into_values().collect();
    restored.sort_by_key(|seat| seat.id);
    Ok(restored)
}

fn remove_subtree(nodes: &mut HashMap<Uuid, RestoredSeat>, root: Uuid) {
    let mut drop_ids = vec![root];
    let mut i = 0;
    while i < drop_ids.len() {
        let id = drop_ids[i];
        for (child, seat) in nodes.iter() {
            if seat.parent == id {
                drop_ids.push(*child);
            }
        }
        i += 1;
    }
    for id in drop_ids {
        nodes.remove(&id);
    }
}

fn parent_chain_reaches(id: Uuid, nodes: &HashMap<Uuid, RestoredSeat>, steward: Uuid) -> bool {
    let mut cursor = id;
    for _ in 0..=nodes.len() {
        let Some(seat) = nodes.get(&cursor) else {
            return false;
        };
        if seat.parent == steward {
            return true;
        }
        cursor = seat.parent;
    }
    false
}

fn bound_is_fileless(bound: &ActSentence) -> bool {
    matches!(
        bound,
        ActSentence::Bare {
            file: crate::act::BareFile::None,
            ..
        } | ActSentence::Forget { .. }
    )
}

fn deny_world_grants(card: &AgentCard) -> Result<(), StewardError> {
    for grant in &card.tools {
        if grant.tag.permit != Permit::Deny {
            return Err(StewardError::GrantNotDenied(grant.name.clone()));
        }
    }
    Ok(())
}

fn note_projection(agent: &Agent, tag: &str, content: &str) -> Result<(), StewardError> {
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

fn write_if_absent(path: &Path, content: &str) -> Result<(), StewardError> {
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
mod tests {
    use super::*;
    use crate::act::{
        ActSentence, Audience, BareFile, FileFacet, Ingest, MemoryFacet, Signal, ToolTag,
    };
    use crate::principal::card::{ModelSpec, ToolGrant, Topology};
    use crate::testkit::TempDir;

    fn bare(id: &str, permit: Permit) -> AgentCard {
        AgentCard {
            v: 1,
            id: Uuid::parse_str(id).unwrap(),
            model: ModelSpec {
                base_url: "https://example.com".into(),
                model: "m".into(),
                extra_body: None,
            },
            prompt: "keep-this-prompt".into(),
            tools: vec![ToolGrant {
                name: "bash".into(),
                tag: ToolTag {
                    file: FileFacet::None,
                    permit,
                    memory: MemoryFacet::Ignore,
                },
            }],
            topology: Topology::Leaf,
            kernel: "0.1.0".into(),
            supersedes: None,
        }
    }

    #[test]
    fn go_grant_cannot_open_a_steward() {
        let tmp = TempDir::new("steward-deny");
        let err = Steward::open(
            &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Go),
            tmp.path(),
            "{}",
            "[]",
        );
        assert!(matches!(err, Err(StewardError::GrantNotDenied(_))));
    }

    #[test]
    fn only_the_steward_spawns_and_queue_steers() {
        let tmp = TempDir::new("steward-queue");
        let steward = Steward::open(
            &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{\"limit\":1}",
            "[\"c0\"]",
        )
        .unwrap();
        let worker = steward
            .spawn_worker(
                &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
                &ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore),
            )
            .unwrap();
        assert_eq!(
            steward.pool.tree().parent(worker.id()).unwrap(),
            Some(steward.id())
        );
        assert_eq!(
            steward.submit(worker.id(), "later").unwrap(),
            Arrival::Queued
        );
        let turn = steward.begin_turn(worker.id()).unwrap();
        assert_eq!(
            steward.submit(worker.id(), "also check tests").unwrap(),
            Arrival::Steered
        );
        assert!(matches!(steward.pop(), Err(StewardError::Busy)));
        let mailed = worker.session().tape().read_all().unwrap();
        assert!(
            mailed
                .iter()
                .any(|e| e.tags.iter().any(|t| t == "send")
                    && e.content.contains("also check tests"))
        );
        drop(turn);
        let intent = steward.pop().unwrap();
        assert_eq!(intent.text, "later");
        let spawned = steward.agent().session().tape().read_all().unwrap();
        assert!(
            spawned.iter().any(|e| {
                e.tags.iter().any(|t| t == "spawn") && e.content.contains("\"bound\"")
            })
        );
    }

    #[test]
    fn spawn_rejects_a_grant_wider_than_the_bound() {
        let tmp = TempDir::new("steward-bound");
        let steward = Steward::open(
            &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{}",
            "[]",
        )
        .unwrap();
        let err = steward.spawn_worker(
            &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
            &ActSentence::bare(Permit::Deny, BareFile::None, Ingest::Ignore),
        );
        assert!(matches!(err, Err(StewardError::OutsideBound(_))));
        let id = Uuid::parse_str("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb").unwrap();
        assert!(steward.workplace().subwp_of(id).is_none());
    }

    #[test]
    fn lite_has_a_seat_no_subwp_and_finishes_by_complete() {
        let tmp = TempDir::new("steward-lite");
        let steward = Steward::open(
            &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{}",
            "[]",
        )
        .unwrap();
        let wide = ActSentence::bare(Permit::Go, BareFile::Unbounded, Ingest::Ignore);
        assert!(matches!(
            steward.spawn_lite(
                &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
                &wide
            ),
            Err(StewardError::LiteFile)
        ));
        let bound = ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore);
        let lite = steward
            .spawn_lite(
                &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
                &bound,
            )
            .unwrap();
        assert_eq!(
            steward.pool.tree().parent(lite.id()).unwrap(),
            Some(steward.id())
        );
        assert!(steward.workplace().subwp_of(lite.id()).is_none());
        assert!(matches!(
            steward.close_worker(lite.id(), &[]),
            Err(StewardError::NoSubwp(_))
        ));
        steward.complete(lite.id(), "result").unwrap();
        assert!(tagged(&lite, "complete"));
        assert!(tagged(steward.agent(), "complete"));
        assert!(steward.workplace().current_paths().is_empty());
    }

    #[test]
    fn release_drops_the_subtree_without_publishing_and_child_bound_cannot_widen() {
        let tmp = TempDir::new("steward-release");
        let steward = Steward::open(
            &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{}",
            "[]",
        )
        .unwrap();
        let parent_bound = ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore);
        let parent = steward
            .spawn_worker(
                &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
                &parent_bound,
            )
            .unwrap();
        let wider = parent_bound.clone().with_signal(Signal {
            complete: Permit::Go,
            audience: Audience::Any,
        });
        assert!(matches!(
            steward.spawn_under(
                parent.id(),
                &bare("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                &wider,
            ),
            Err(StewardError::WiderThanParent)
        ));
        let child = steward
            .spawn_under(
                parent.id(),
                &bare("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                &parent_bound,
            )
            .unwrap();
        let sub = steward.workplace().subwp_of(parent.id()).unwrap();
        steward
            .workplace()
            .write(
                &sub,
                "src/a.txt",
                b"draft",
                crate::instance::WriteMode::Create,
            )
            .unwrap();
        steward.release(parent.id()).unwrap();
        assert!(steward.pool.tree().get(parent.id()).is_none());
        assert!(steward.pool.tree().get(child.id()).is_none());
        assert!(steward.workplace().subwp_of(parent.id()).is_none());
        assert!(steward.workplace().current_paths().is_empty());
    }

    fn tagged(agent: &Agent, tag: &str) -> bool {
        agent
            .session()
            .tape()
            .read_all()
            .unwrap()
            .iter()
            .any(|e| e.tags.iter().any(|t| t == tag))
    }

    #[test]
    fn cancel_propagates_down_and_restore_skips_broken_chains() {
        let tmp = TempDir::new("steward-lease");
        let steward = Steward::open(
            &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{}",
            "[]",
        )
        .unwrap();
        let bound = ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore);
        let kept = steward
            .spawn_lite(
                &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
                &bound,
            )
            .unwrap();
        let parent = steward
            .spawn_worker(
                &bare("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                &bound,
            )
            .unwrap();
        let child = steward
            .spawn_under(
                parent.id(),
                &bare("dddddddd-dddd-dddd-dddd-dddddddddddd", Permit::Go),
                &bound,
            )
            .unwrap();
        let parent_lease = steward.lease(parent.id()).unwrap();
        let child_lease = steward.lease(child.id()).unwrap();
        let kept_lease = steward.lease(kept.id()).unwrap();
        steward.propagate_cancel(parent.id()).unwrap();
        assert!(parent_lease.is_cancelled());
        assert!(child_lease.is_cancelled());
        assert!(!kept_lease.is_cancelled());
        assert!(tagged(&child, "cancel"));
        let restored = restore_seats(steward.agent().session().root()).unwrap();
        assert!(restored.iter().any(|seat| seat.id == kept.id()));
        assert!(
            restored
                .iter()
                .all(|seat| seat.id != parent.id() && seat.id != child.id())
        );
    }

    #[test]
    fn signals_follow_the_charter_and_only_parent_cancels() {
        let tmp = TempDir::new("steward-signal");
        let steward = Steward::open(
            &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{}",
            "[]",
        )
        .unwrap();
        let bound =
            ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore).with_signal(Signal {
                complete: Permit::Deny,
                audience: Audience::Parent,
            });
        let worker = steward
            .spawn_worker(
                &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Go),
                &bound,
            )
            .unwrap();
        let sibling = steward
            .spawn_worker(
                &bare("cccccccc-cccc-cccc-cccc-cccccccccccc", Permit::Go),
                &ActSentence::bare(Permit::Go, BareFile::None, Ingest::Ignore),
            )
            .unwrap();
        assert!(matches!(
            steward.complete(worker.id(), "done"),
            Err(StewardError::SignalDenied("complete"))
        ));
        assert!(matches!(
            steward.send(worker.id(), sibling.id(), "hi"),
            Err(StewardError::SignalDenied("send"))
        ));
        assert!(matches!(
            steward.cancel(worker.id(), steward.id()),
            Err(StewardError::SignalDenied("cancel"))
        ));
        let id = steward.cancel(steward.id(), worker.id()).unwrap();
        assert!(tagged(steward.agent(), "cancel"));
        assert!(tagged(&worker, "cancel"));
        assert!(!tagged(&sibling, "cancel"));
        let back = worker.session().tape().read_all().unwrap();
        assert!(
            back.iter()
                .any(|e| e.refs.first().map(String::as_str) == Some(id.as_str()))
        );
        steward.send(worker.id(), steward.id(), "ping").unwrap();
        assert!(tagged(&worker, "send"));
    }

    #[test]
    fn collect_records_pointers_not_bodies() {
        let tmp = TempDir::new("steward-collect");
        let steward = Steward::open(
            &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{}",
            "[]",
        )
        .unwrap();
        let worker = steward
            .spawn_worker(
                &bare("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", Permit::Deny),
                &ActSentence::bare(Permit::Deny, BareFile::None, Ingest::Ignore),
            )
            .unwrap();
        let marker = "BODY_SHOULD_NOT_APPEAR";
        worker
            .session()
            .tape()
            .append(AppendEvent {
                from: worker.id_str().into(),
                from_kind: FromKind::Agent,
                kind: Kind::Utterance,
                session: worker.session().id_str().into(),
                content: marker.into(),
                tags: vec![],
                refs: vec![],
                act: None,
                card: None,
            })
            .unwrap();
        let product = ProductPtr::from_tape_file(
            worker.session().id_str(),
            &worker.session().tape_path(),
            None,
            None,
            None,
        )
        .unwrap();
        let digest = product.tape_sha256.clone();
        let id = steward.collect(&product).unwrap();
        let events = steward.agent().session().tape().read_all().unwrap();
        let link = events.iter().find(|e| e.id == id).unwrap();
        assert!(link.tags.iter().any(|t| t == "link"));
        assert!(!link.content.contains(marker));
        assert!(link.content.contains(&digest));
    }

    #[test]
    fn view_reads_linked_ids_only() {
        let tmp = TempDir::new("steward-view");
        let steward = Steward::open(
            &bare("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", Permit::Deny),
            tmp.path(),
            "{}",
            "[]",
        )
        .unwrap();
        let a = steward
            .link(&ProductPtr {
                child_session: "s1".into(),
                tape_sha256: "aa".into(),
                md_sha256: None,
                tree_id: None,
                subwp_sha256: None,
            })
            .unwrap();
        let b = steward
            .link(&ProductPtr {
                child_session: "s2".into(),
                tape_sha256: "bb".into(),
                md_sha256: Some("cc".into()),
                tree_id: Some("tree-1".into()),
                subwp_sha256: None,
            })
            .unwrap();
        steward
            .define_view("main", &[a.clone(), b.clone()])
            .unwrap();
        assert_eq!(steward.view("main").unwrap(), vec![a, b]);
    }
}
