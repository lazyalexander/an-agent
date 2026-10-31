//! Reuse the kernel's workspace log and immutable configuration snapshots.
use super::*;

pub(super) struct Space {
    pub root: PathBuf,
    pub id: String,
    pub version: u8,
    pub facts: Workspace,
}
impl Space {
    pub fn open(path: &Path) -> Result<Self> {
        let root = native::location(path)?;
        let meta: Value =
            serde_json::from_slice(&regular(&root.join("native-events.json"), 65536)?)?;
        let version = match meta["schema"].as_str() {
            Some("an-native-event-workspace-v1") => 1,
            Some("an-native-event-workspace-v2") => 2,
            Some("an-native-event-workspace-v3") => 3,
            _ => return Err("unsupported native event workspace".into()),
        };
        let id = text(&meta, "workspace")?.to_owned();
        uuid::Uuid::parse_str(&id)?;
        for name in [
            "facts",
            "facts/config",
            "facts/env",
            "facts/register",
            "facts/env.md",
            "deliveries",
            "revocations",
            "dispatches",
        ] {
            store::no_links(&root.join(name))?;
        }
        regular(&root.join("facts/log.jsonl"), 8 * 1024 * 1024)?;
        let facts = Workspace::open(root.join("facts"))?;
        if facts.log()?.len() > 512 {
            return Err("native event workspace record limit exceeded".into());
        }
        Ok(Self {
            root,
            id,
            version,
            facts,
        })
    }
    /// Metadata upgrades are monotonic and require the workspace operation lock.
    pub fn upgrade(&self, version: u8) -> Result<()> {
        if !(2..=3).contains(&version) {
            return Err("unsupported workspace upgrade".into());
        }
        let metadata = self.root.join("native-events.json");
        let mut value: Value = serde_json::from_slice(&regular(&metadata, 65536)?)?;
        let current = match value["schema"].as_str() {
            Some("an-native-event-workspace-v1") => 1,
            Some("an-native-event-workspace-v2") => 2,
            Some("an-native-event-workspace-v3") => 3,
            _ => return Err("unsupported native event workspace".into()),
        };
        if value["workspace"] != self.id {
            return Err("workspace identity changed before upgrade".into());
        }
        if current >= version {
            return Ok(());
        }
        let pending = self.root.join(format!("native-events-v{version}.pending"));
        store::no_links(&pending)?;
        value["schema"] = json!(format!("an-native-event-workspace-v{version}"));
        let bytes = serde_json::to_vec_pretty(&value)?;
        if pending.exists() {
            if regular(&pending, 65536)? != bytes {
                return Err("workspace schema upgrade staging conflicts".into());
            }
        } else {
            store::write_new(&pending, &bytes)?;
        }
        fs::rename(pending, metadata)?;
        fs::File::open(&self.root)?.sync_all()?;
        Ok(())
    }
    pub fn event(&self, id: &str) -> Result<Value> {
        self.find_event(id)?
            .ok_or_else(|| "unknown workspace event".into())
    }
    pub fn find_event(&self, id: &str) -> Result<Option<Value>> {
        if !store::valid_digest(id) {
            return Err("event must be an exact digest".into());
        }
        let mut found = None;
        for record in self.facts.log()? {
            if record.kind != "event"
                || record.name.as_deref() != Some(native::event_contract::ARTIFACT)
            {
                continue;
            }
            let event: Value =
                serde_json::from_str(record.body.as_deref().ok_or("missing event body")?)?;
            if event["id"] == id {
                if found.is_some()
                    || hash(&event["event"])? != id
                    || event["event"]["workspace"] != self.id
                {
                    return Err("duplicate or changed native event identity".into());
                }
                found = Some(event);
            }
        }
        Ok(found)
    }
    pub fn append(&self, event: &Value) -> Result<Value> {
        let id = text(event, "id")?;
        if hash(&event["event"])? != id || event["event"]["workspace"] != self.id {
            return Err("event does not bind this workspace".into());
        }
        if let Some(prior) = self.find_event(id)? {
            if prior != *event {
                return Err("event content conflicts".into());
            }
            fs::File::open(self.root.join("facts/log.jsonl"))?.sync_data()?;
            return Ok(prior);
        }
        // A malformed/torn log must never be treated as an absent event.
        let records = self.facts.log()?;
        if records.len() >= 512 {
            return Err("native event workspace is full".into());
        }
        let body = serde_json::to_string(event)?;
        if body.len() > 65536 {
            return Err("native event envelope too large".into());
        }
        self.facts
            .push_event(native::event_contract::ARTIFACT, &body)?;
        Ok(event.clone())
    }
    pub fn route(&self, name: &str) -> Result<Value> {
        let route = self.raw_route(name)?;
        replacement::validate(self, &route)?;
        Ok(route)
    }
    pub fn raw_route(&self, name: &str) -> Result<Value> {
        native::bundle::identifier(name)?;
        let records: Vec<_> = self
            .facts
            .log()?
            .into_iter()
            .filter(|r| {
                r.kind == "config" && r.name.as_deref() == Some(&format!("native.events.{name}"))
            })
            .collect();
        if records.len() != 1 {
            return Err("missing or ambiguous static subscription".into());
        }
        let digest = records[0].sha256.as_deref().ok_or("subscription digest")?;
        if !store::valid_digest(digest) {
            return Err("invalid subscription digest".into());
        }
        let bytes = regular(&self.root.join("facts/config").join(digest), 65536)?;
        let route: Value = serde_json::from_slice(&bytes)?;
        if store::digest(&bytes) != digest
            || route["workspace"] != self.id
            || route["name"] != name
            || !matches!(
                route["schema"].as_str(),
                Some("an-native-event-subscription-v1" | "an-native-event-subscription-v2")
            )
        {
            return Err("subscription binding mismatch".into());
        }
        Ok(route)
    }
    pub fn check_active(&self, route: &Value) -> Result<()> {
        self.not_revoked(route)?;
        replacement::not_retired(self, route)?;
        if control::current(self, route)?["mode"] != "active" {
            return Err("subscription admission is paused".into());
        }
        self.check_target(route)
    }
    pub fn check_target(&self, route: &Value) -> Result<()> {
        let root = Path::new(text(&route["target"], "root")?);
        let (instance, _) = native::active(root)?;
        if identity(root)? != route["target"]
            || native::round_control::current(root, &instance)?["generation"]
                != route["target_generation"]
        {
            return Err("subscription target identity or control generation changed".into());
        }
        Ok(())
    }
    pub fn not_revoked(&self, route: &Value) -> Result<()> {
        let id = hash(route)?;
        let revoked = self.root.join("revocations").join(format!("{id}.json"));
        store::no_links(&revoked)?;
        if revoked.exists() {
            return Err("subscription is revoked".into());
        }
        Ok(())
    }
}

pub fn init(path: &Path) -> Result<Value> {
    store::no_links(path)?;
    if path.exists() {
        return Err("event workspace requires a new directory".into());
    }
    let root = path
        .parent()
        .ok_or("workspace parent")?
        .canonicalize()?
        .join(path.file_name().ok_or("workspace name")?);
    store::directory(&root)?;
    Workspace::open(root.join("facts"))?;
    for dir in ["deliveries", "revocations"] {
        store::directory(&root.join(dir))?;
    }
    write_json(
        &root.join("native-events.json"),
        &json!({"schema":"an-native-event-workspace-v1",
        "workspace":an_agent_core::det_seam::Entropy::os().uuid_v4().to_string()}),
    )?;
    inspect(&root)
}
pub fn inspect(path: &Path) -> Result<Value> {
    let space = Space::open(path)?;
    let _lock = store::lock(&space.root)?;
    let records = space.facts.log()?;
    let mut subscriptions = Vec::new();
    for record in &records {
        if record.kind == "config"
            && let Some(name) = record
                .name
                .as_deref()
                .and_then(|n| n.strip_prefix("native.events."))
        {
            let route = space.route(name)?;
            subscriptions.push(json!({"name":name,"subscription":hash(&route)?,
                "control":control::current(&space,&route)?,
                "retired_by":replacement::retired_by(&space,&route)?,"cutover":route.get("cutover"),
                "admission_refusal":space.check_active(&route).err().map(|e|e.to_string())}));
        }
    }
    let mut deliveries = Vec::new();
    for entry in fs::read_dir(space.root.join("deliveries"))?.take(129) {
        if deliveries.len() == 128 {
            return Err("delivery observation limit exceeded".into());
        }
        let entry = entry?;
        store::no_links(&entry.path())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "invalid delivery name")?;
        if !store::valid_digest(&name) || !entry.file_type()?.is_dir() {
            return Err("unexpected delivery entry".into());
        }
        deliveries.push(
            json!({"delivery":name,"intent_present":entry.path().join("intent.json").exists(),
            "ack_present":entry.path().join("ack.json").exists()}),
        );
    }
    deliveries.sort_by_key(|v| v["delivery"].as_str().unwrap_or_default().to_owned());
    Ok(
        json!({"schema":"an-native-events-observation-v1","workspace":space.id,
        "records":records,"subscriptions":subscriptions,"deliveries":deliveries,
        "authority":"observations and marker presence are not execution or cleanup permission",
        "execution_granted":false,"background_dispatch":false}),
    )
}
