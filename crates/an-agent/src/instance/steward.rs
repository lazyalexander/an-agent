//! The one user-facing agent in an instance. Spawn, mail, and link are
//! verbs here — not tools. World-facing grants on its card must be Deny.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::act::{ActSentence, MailTo, Permit};
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
        Ok(Self {
            agent,
            pool,
            seats: Mutex::new(vec![seat]),
            queue: Mutex::new(VecDeque::new()),
            root,
            wp,
            bounds: Mutex::new(HashMap::new()),
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
    pub fn spawn_worker(
        &self,
        card: &AgentCard,
        bound: &ActSentence,
    ) -> Result<Arc<Agent>, StewardError> {
        for grant in &card.tools {
            if !bound.allows(&grant.tag) {
                return Err(StewardError::OutsideBound(grant.name.clone()));
            }
        }
        let child = Arc::new(spawn(card, &self.root)?);
        let seat = self
            .pool
            .tree()
            .mount(Arc::clone(&child), Some(self.agent.id()))?;
        self.seats
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seat);
        let sub = self.wp.register(child.id(), child.session().root())?;
        self.bounds
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(child.id(), bound.clone());
        self.append(
            &self.agent,
            "spawn",
            json!({
                "worker": child.id_str(),
                "session": child.session().id_str(),
                "subwp": sub,
                "bound": bound,
            })
            .to_string(),
            vec![],
        )?;
        Ok(child)
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

    /// Child ends itself. Denied when the charter's return face is Deny.
    pub fn child_return(&self, child: Uuid, text: &str) -> Result<String, StewardError> {
        let parent = self
            .pool
            .tree()
            .parent(child)?
            .ok_or(StewardError::NotMounted(child))?;
        let bound = self.charter(child)?;
        if bound.signal().ret == Permit::Deny {
            return Err(StewardError::SignalDenied("return"));
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
        self.record_both(&child_agent, &parent_agent, "return", text)
    }

    /// Child mail. Parent is always allowed. Anyone else requires `mail: any`.
    pub fn child_mail(&self, from: Uuid, to: Uuid, text: &str) -> Result<String, StewardError> {
        let parent = self
            .pool
            .tree()
            .parent(from)?
            .ok_or(StewardError::NotMounted(from))?;
        let bound = self.charter(from)?;
        if to != parent && bound.signal().mail != MailTo::Any {
            return Err(StewardError::SignalDenied("mail"));
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
        self.record_both(&from_agent, &to_agent, "mail", text)
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
        let parent = self.append(&self.agent, "mail", body, vec![])?;
        let back = json!({
            "to_session": self.agent.session().id_str(),
            "text": text,
        })
        .to_string();
        self.append(&child, "mail", back, vec![parent.clone()])?;
        Ok(parent)
    }

    /// Publish the worker's sub-workplace if it wrote anything.
    /// Steward does not write file bytes.
    pub fn close_worker(&self, worker: Uuid, extra: &[DepEdge]) -> Result<CloseOut, StewardError> {
        let sub = self
            .wp
            .subwp_of(worker)
            .ok_or(StewardError::NotMounted(worker))?;
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
        ActSentence, BareFile, FileFacet, Ingest, MailTo, MemoryFacet, Signal, ToolTag,
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
                .any(|e| e.tags.iter().any(|t| t == "mail")
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
                ret: Permit::Deny,
                mail: MailTo::Parent,
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
            steward.child_return(worker.id(), "done"),
            Err(StewardError::SignalDenied("return"))
        ));
        assert!(matches!(
            steward.child_mail(worker.id(), sibling.id(), "hi"),
            Err(StewardError::SignalDenied("mail"))
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
        steward
            .child_mail(worker.id(), steward.id(), "ping")
            .unwrap();
        assert!(tagged(&worker, "mail"));
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
