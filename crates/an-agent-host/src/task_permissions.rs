//! Task-scoped capabilities retained by the host and shared with finite owners.
//! Bearer credentials stay at the application boundary; workers retain bindings.
mod scope;
pub(crate) use scope::Permission;

use crate::execution::{Result, store};
use an_agent_core::det_seam::{Clock, Entropy};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub root: PathBuf,
    pub id: String,
    pub sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Credential {
    pub id: String,
    pub secret: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Scope {
    pub task: String,
    pub subject: String,
    pub expires_ms: u64,
    pub delegate: bool,
    pub permissions: Vec<Permission>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Grant {
    pub schema: String,
    pub scope: Scope,
    pub parent: Option<Binding>,
    secret_sha256: String,
}

fn path(root: &Path, id: &str) -> Result<PathBuf> {
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        return Err("invalid task grant ID".into());
    }
    let path = root.join("grants").join(id);
    store::no_links(&path)?;
    Ok(path)
}

fn persist(path: &Path, value: &impl Serialize) -> Result<()> {
    store::write_new(path, &serde_json::to_vec(value)?)?;
    fs::File::open(path.parent().ok_or("grant parent")?)?.sync_all()?;
    Ok(())
}

pub(crate) fn load(binding: &Binding) -> Result<Grant> {
    let bytes = store::read(
        &path(&binding.root, &binding.id)?.join("grant.json"),
        131072,
    )?;
    let grant: Grant = serde_json::from_slice(&bytes)?;
    if store::digest(&bytes) != binding.sha256 || grant.schema != "an-task-grant-v1" {
        return Err("task grant binding changed".into());
    }
    Ok(grant)
}

pub(crate) fn binding(root: &Path, id: &str) -> Result<Binding> {
    let bytes = store::read(&path(root, id)?.join("grant.json"), 131072)?;
    Ok(Binding {
        root: root.to_owned(),
        id: id.into(),
        sha256: store::digest(&bytes),
    })
}

pub(crate) fn authenticate(root: &Path, credential: &Credential) -> Result<Binding> {
    let binding = binding(root, &credential.id)?;
    let grant = load(&binding)?;
    if credential.secret.len() != 64
        || store::digest(credential.secret.as_bytes()) != grant.secret_sha256
    {
        return Err("invalid task grant credential".into());
    }
    Ok(binding)
}

/// Validate every ancestor, including immutable scope and parent bindings.
pub(crate) fn lineage(binding: &Binding, require_active: bool) -> Result<Vec<Binding>> {
    let mut chain = Vec::new();
    let mut current = Some(binding.clone());
    while let Some(selected) = current {
        if selected.root != binding.root || chain.len() >= 16 || chain.contains(&selected) {
            return Err("invalid or overlong task grant lineage".into());
        }
        let grant = load(&selected)?;
        let revoked = path(&selected.root, &selected.id)?.join("revoked.json");
        store::no_links(&revoked)?;
        if require_active
            && (grant.scope.expires_ms <= Clock::wall().now_ms() || revoked.try_exists()?)
        {
            return Err("task permission expired or revoked".into());
        }
        if let Some(parent) = &grant.parent {
            grant.scope.narrower_than(&load(parent)?.scope)?;
        }
        chain.push(selected);
        current = grant.parent;
    }
    Ok(chain)
}

pub(crate) fn active(binding: &Binding) -> Result<()> {
    lineage(binding, true).map(|_| ())
}

pub(crate) fn owns(parent: &Binding, child: &Binding) -> Result<bool> {
    Ok(lineage(child, false)?.contains(parent))
}

pub(crate) fn validate(scope: &Scope, parent: Option<&Binding>) -> Result<()> {
    if scope.task.is_empty()
        || scope.task.len() > 120
        || scope.subject.is_empty()
        || scope.subject.len() > 120
        || scope.expires_ms <= Clock::wall().now_ms()
        || scope.permissions.len() > 64
    {
        return Err("invalid task permission scope".into());
    }
    for (index, permission) in scope.permissions.iter().enumerate() {
        permission.validate()?;
        if scope.permissions[..index].contains(permission) {
            return Err("duplicate task permission".into());
        }
    }
    if let Some(parent) = parent {
        if lineage(parent, true)?.len() >= 16 {
            return Err("task permission delegation depth exceeded".into());
        }
        scope.narrower_than(&load(parent)?.scope)?;
    }
    Ok(())
}

pub(crate) fn issue(
    root: &Path,
    scope: Scope,
    parent: Option<Binding>,
) -> Result<(Binding, Credential)> {
    validate(&scope, parent.as_ref())?;
    let directory = root.join("grants");
    store::directory(&directory)?;
    if fs::read_dir(&directory)?.count() >= 256 {
        return Err("task grant registry full".into());
    }
    let mut entropy = Entropy::os();
    let id = entropy.uuid_v4().to_string();
    let secret = format!(
        "{}{}",
        entropy.uuid_v4().simple(),
        entropy.uuid_v4().simple()
    );
    let directory = path(root, &id)?;
    fs::create_dir(&directory)?;
    let grant = Grant {
        schema: "an-task-grant-v1".into(),
        scope,
        parent,
        secret_sha256: store::digest(secret.as_bytes()),
    };
    persist(&directory.join("grant.json"), &grant)?;
    fs::File::open(root.join("grants"))?.sync_all()?;
    Ok((binding(root, &id)?, Credential { id, secret }))
}

pub(crate) fn revoke(binding: &Binding) -> Result<()> {
    load(binding)?;
    let path = path(&binding.root, &binding.id)?.join("revoked.json");
    if !path.try_exists()? {
        persist(&path, &serde_json::json!({"grant":binding}))?;
    }
    Ok(())
}

pub(crate) fn require(binding: &Binding, requested: &Permission) -> Result<()> {
    active(binding)?;
    if !load(binding)?
        .scope
        .permissions
        .iter()
        .any(|p| requested.within(p))
    {
        return Err("operation exceeds task permission".into());
    }
    Ok(())
}
