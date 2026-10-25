//! Operator-owned trusted-runtime plan; package metadata cannot supply authority.
use super::inventory;
use crate::{
    execution::{Result, store},
    file_job::regular,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    schema: String,
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub readonly: Vec<PathBuf>,
    pub inputs: Vec<PathBuf>,
    pub outputs: Vec<String>,
    #[serde(default)]
    binary_outputs: Vec<String>,
    #[serde(default)]
    artifact_inputs: Vec<ArtifactInput>,
    pub wall_seconds: u64,
    trusted_process_group: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ArtifactInput {
    pub path: PathBuf,
    pub catalog: PathBuf,
    pub revision: String,
}

impl Profile {
    pub fn plan(bytes: &[u8], root: &Path, predecessor: &Value) -> Result<(Self, Value)> {
        let p: Self = serde_json::from_slice(bytes)?;
        if p.schema != "an-workspace-job-v1"
            || !p.trusted_process_group
            || !(1..=300).contains(&p.wall_seconds)
            || p.args.len() > 64
            || p.inputs.is_empty()
            || p.inputs.len() > 16
            || p.readonly.len() > 8
            || p.outputs.is_empty()
            || p.outputs.len() > 16
            || p.environment.len() > 32
        {
            return Err("unsupported bounded trusted-runtime profile".into());
        }
        let work = root.join("work");
        let exe = p.executable.canonicalize()?;
        let executable_hash = store::digest(&regular(&exe, 20 * 1024 * 1024)?);
        let mut roots = Vec::new();
        for path in &p.readonly {
            let path = path.canonicalize()?;
            if path == Path::new("/") || root.starts_with(&path) || path.starts_with(root) {
                return Err("runtime roots must exclude the host job and its ancestors".into());
            }
            roots.push(path);
        }
        let mut inputs = Vec::new();
        let mut total = 0usize;
        for file in &p.inputs {
            let path = file.canonicalize()?;
            if !roots.iter().any(|r| path.starts_with(r)) {
                return Err("input outside selected read roots".into());
            }
            let bytes = regular(&path, 64 * 1024 * 1024)?;
            total += bytes.len();
            if total > 100 * 1024 * 1024 {
                return Err("selected inputs exceed 100 MiB".into());
            }
            inputs.push(json!({"path":path,"sha256":store::digest(&bytes)}));
        }
        let mut outputs = p.outputs.clone();
        outputs.sort();
        outputs.dedup();
        if outputs.len() != p.outputs.len() {
            return Err("duplicate output".into());
        }
        for path in &outputs {
            inventory::relative(path)?;
        }
        let mut binary = p.binary_outputs.clone();
        binary.sort();
        binary.dedup();
        if binary.len() != p.binary_outputs.len()
            || binary.iter().any(|name| !outputs.contains(name))
        {
            return Err("binary outputs must be unique declared outputs".into());
        }
        let artifacts = super::inputs::bind(&p.artifact_inputs, &inputs, &roots, &work)?;
        let resolve = |s: &str| s.replace("{work}", &work.to_string_lossy());
        let args: Vec<_> = p.args.iter().map(|a| resolve(a)).collect();
        let mut env = BTreeMap::from([
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("TMPDIR".to_owned(), work.join("tmp").display().to_string()),
            ("LANG".to_owned(), "C".to_owned()),
        ]);
        for (key, value) in &p.environment {
            if env.contains_key(key)
                || key.is_empty()
                || key.len() > 128
                || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            {
                return Err("invalid or reserved environment key".into());
            }
            env.insert(key.clone(), resolve(value));
        }
        if args
            .iter()
            .chain(env.values())
            .any(|s| s.len() > 8192 || s.contains('\0'))
        {
            return Err("argument or environment exceeds limits".into());
        }
        let policy = crate::execution::sandbox::workspace::policy(&exe, &work, &roots)?;
        let mut plan = json!({"schema":"an-workspace-plan-v1","backend":"macos-trusted-workspace-v1",
            "supported":cfg!(target_os="macos"),"profile_sha256":store::digest(bytes),
            "executable":exe,"executable_sha256":executable_hash,"args":args,"environment":env,
            "inputs":inputs,"readonly":roots,"outputs":outputs,"policy":policy,"root":root,
            "predecessor":predecessor,"scratch_before":if work.exists(){inventory::scan(&work)?}else{json!({})},
            "wall_seconds":p.wall_seconds,"network":false,"automatic_retries":0,
            "scope":"reviewed local tools without daemonization; not hostile process-tree containment"});
        if !binary.is_empty() {
            plan["binary_outputs"] = json!(binary);
        }
        if !artifacts.is_empty() {
            plan["artifact_inputs"] = json!(artifacts);
        }
        Ok((p, plan))
    }
}

/// Reconcile selected immutable inputs before promoting a runtime's candidates.
pub(super) fn verify_inputs(plan: &Value) -> Result<()> {
    let exe = Path::new(plan["executable"].as_str().ok_or("missing executable")?);
    if store::digest(&regular(exe, 20 * 1024 * 1024)?) != plan["executable_sha256"] {
        return Err("selected runtime executable changed".into());
    }
    for input in plan["inputs"].as_array().ok_or("missing selected inputs")? {
        let path = Path::new(input["path"].as_str().ok_or("invalid input path")?);
        if store::digest(&regular(path, 64 * 1024 * 1024)?) != input["sha256"] {
            return Err("selected workflow input changed during execution".into());
        }
    }
    super::inputs::verify(plan)?;
    Ok(())
}
