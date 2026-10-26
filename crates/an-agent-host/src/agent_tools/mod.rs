//! Finite model-facing capabilities; not a sandbox for a third-party executable.
mod publication;
mod transport;
use crate::{
    execution::{Result, store},
    file_job::regular,
};
use publication::Call;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    schema: String,
    store: PathBuf,
    session: PathBuf,
    source: PathBuf,
    kits: BTreeMap<String, String>,
    max_calls: usize,
}

pub struct Bridge {
    grant: Grant,
    source: Vec<u8>,
    closing: AtomicBool,
    active: Mutex<Option<Arc<Call>>>,
}

pub fn preview(path: &Path) -> Result<Value> {
    let (grant, source, plan) = load(path)?;
    let _ = (grant, source);
    Ok(json!({"plan":plan,"approval_sha256":store::digest(&serde_json::to_vec(&plan)?)}))
}

fn load(path: &Path) -> Result<(Grant, Vec<u8>, Value)> {
    let bytes = regular(path, 65536)?;
    let mut g: Grant = serde_json::from_slice(&bytes)?;
    if g.schema != "an-agent-tools-v1"
        || !(1..=32).contains(&g.max_calls)
        || g.kits.is_empty()
        || g.kits.len() > 8
    {
        return Err("unsupported capability grant".into());
    }
    store::no_links(&g.store)?;
    g.store = g.store.canonicalize()?;
    let source = regular(&g.source, 65536)?;
    std::str::from_utf8(&source)?;
    store::no_links(&g.session)?;
    if g.session.exists() {
        return Err("tool session must be new; no implicit resumption".into());
    }
    g.session = g
        .session
        .parent()
        .ok_or("missing session parent")?
        .canonicalize()?
        .join(g.session.file_name().ok_or("missing session name")?);
    if g.session.starts_with(&g.store) || path.canonicalize()?.starts_with(&g.session) {
        return Err("capability session must exclude the execution store and grant".into());
    }
    let mut kits = BTreeMap::new();
    for (name, release) in &g.kits {
        identifier(name)?;
        if !store::valid_digest(release) {
            return Err("kit grant must pin a full release digest".into());
        }
        let check = crate::execution::check(&g.store.join("releases").join(release))?;
        if check["release"] != *release {
            return Err("granted release mismatch".into());
        }
        kits.insert(name, check);
    }
    let plan = json!({"schema":"an-agent-tools-plan-v1","grant_sha256":store::digest(&bytes),
        "store":g.store,"session":g.session,"source_sha256":store::digest(&source),"kits":kits,
        "max_calls":g.max_calls,"native_shell":false,"arbitrary_paths":false,"automatic_retries":0});
    Ok((g, source, plan))
}

fn identifier(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err("invalid opaque invocation or capability identifier".into());
    }
    Ok(())
}

impl Bridge {
    pub fn open(path: &Path, approved: &str) -> Result<Arc<Self>> {
        let (grant, source, plan) = load(path)?;
        if store::digest(&serde_json::to_vec(&plan)?) != approved {
            return Err("capability approval mismatch".into());
        }
        store::directory(&grant.session)?;
        store::directory(&grant.session.join("calls"))?;
        store::directory(&grant.session.join("denials"))?;
        store::write_new(
            &grant.session.join("grant.json"),
            &serde_json::to_vec_pretty(&plan)?,
        )?;
        Ok(Arc::new(Self {
            grant,
            source,
            closing: AtomicBool::new(false),
            active: Mutex::new(None),
        }))
    }

    pub fn shutdown(&self) {
        self.closing.store(true, Ordering::SeqCst);
        if let Ok(active) = self.active.lock()
            && let Some(stop) = active.as_ref()
        {
            stop.cancel();
        }
    }

    pub async fn serve(self: Arc<Self>) -> Result<()> {
        transport::serve(self).await
    }

    fn invoke(&self, name: &str, arguments: Value, stop: Arc<Call>) -> Result<Value> {
        let encoded = serde_json::to_vec(&arguments)?;
        if name.len() > 64 || encoded.len() > 512 * 1024 {
            return Err("capability envelope exceeds limit".into());
        }
        let hash = store::digest(&encoded);
        let result = self.dispatch(name, arguments, stop);
        if let Err(error) = &result {
            let denials = self.grant.session.join("denials");
            if std::fs::read_dir(&denials)?.count() < 32 {
                let id = an_agent_core::det_seam::Entropy::os().uuid_v4();
                store::write_new(
                    &denials.join(format!("{id}.json")),
                    &serde_json::to_vec(&json!({
                    "tool":name,"request_sha256":hash,"granted":false,"error":error.to_string()}))?,
                )?;
            }
        }
        result
    }

    fn dispatch(&self, name: &str, arguments: Value, stop: Arc<Call>) -> Result<Value> {
        let mut active = self
            .active
            .try_lock()
            .map_err(|_| "another capability operation is active")?;
        if active.is_some() || self.closing.load(Ordering::SeqCst) {
            return Err("capability session is busy or closing".into());
        }
        *active = Some(stop.clone());
        drop(active);
        struct Clear<'a>(&'a Bridge);
        impl Drop for Clear<'_> {
            fn drop(&mut self) {
                if let Ok(mut a) = self.0.active.lock() {
                    *a = None;
                }
            }
        }
        let _clear = Clear(self);
        let request: Request = serde_json::from_value(arguments)?;
        identifier(&request.invocation)?;
        let calls = self.grant.session.join("calls");
        if std::fs::read_dir(&calls)?.count() >= self.grant.max_calls {
            return Err("capability quota exhausted".into());
        }
        let dir = calls.join(&request.invocation);
        // Every mutation is fenced before dispatch; duplicate/uncertain calls do not retry.
        if dir.exists() {
            return Err("invocation already submitted; inspect existing evidence".into());
        }
        store::directory(&dir)?;
        store::write_new(
            &dir.join("intent.json"),
            &serde_json::to_vec(&json!({"tool":name,"request":request}))?,
        )?;
        let result = (|| -> Result<Value> {
            if stop.stop().load(Ordering::SeqCst) {
                return Err("request cancelled before dispatch".into());
            }
            match name {
                "read_source" if request.kit.is_none() && request.text.is_none() => Ok(
                    json!({"source":std::str::from_utf8(&self.source)?,"sha256":store::digest(&self.source)}),
                ),
                "propose_source" if request.kit.is_none() => {
                    let text = request.text.as_ref().ok_or("missing source proposal")?;
                    if text.len() > 65536 {
                        return Err("source proposal exceeds 64 KiB".into());
                    }
                    store::write_new(&dir.join("candidate.rs"), text.as_bytes())?;
                    Ok(
                        json!({"candidate":request.invocation,"sha256":store::digest(text.as_bytes()),"bytes":text.len(),"accepted":false,"executed":false}),
                    )
                }
                "run_kit" => {
                    let release = self
                        .grant
                        .kits
                        .get(request.kit.as_deref().ok_or("missing kit handle")?)
                        .ok_or("kit capability is not granted")?;
                    let text = request.text.as_ref().ok_or("missing kit input")?;
                    if text.len() > 8192 {
                        return Err("kit input exceeds 8 KiB".into());
                    }
                    store::write_new(&dir.join("input.txt"), text.as_bytes())?;
                    let result = crate::execution::run_for_tool(
                        &self.grant.store,
                        release,
                        &dir.join("input.txt"),
                        stop.stop(),
                    )?;
                    let receipt =
                        Path::new(result["receipt"].as_str().ok_or("missing run receipt")?)
                            .parent()
                            .ok_or("missing run directory")?;
                    let bytes = regular(&receipt.join("receipt.json"), 65536)?;
                    let observed: Value = serde_json::from_slice(&bytes)?;
                    let mut response = json!({"status":observed["status"],"execution_status":observed["status"],"run_id":observed["run_id"],"receipt_sha256":store::digest(&bytes),"reaped":observed["process"]["reaped"],"publication":{"status":"not-attempted"}});
                    if observed["status"] == "succeeded" {
                        let text = regular(&receipt.join("artifact.txt"), 65536)?;
                        let text = std::str::from_utf8(&text)?;
                        stop.publish(&dir, &mut response, || {
                            crate::artifact_versions::import(
                                &self.grant.session.join("artifacts"),
                                receipt,
                            )
                        })?;
                        if response.get("artifact").is_some() {
                            response["text"] = json!(text);
                        }
                    }
                    Ok(response)
                }
                _ => Err("capability or request fields are not allowed".into()),
            }
        })();
        let mut value = match result {
            Ok(value) => value,
            Err(error) => {
                json!({"status":"refused-or-uncertain","error":error.to_string(),"automatic_retries":0})
            }
        };
        if value["status"] == "refused-or-uncertain" && dir.join("publication-intent.json").exists()
        {
            value["publication"] =
                json!({"status":"uncertain", "intent":"publication-intent.json"});
        }
        store::write_new(
            &dir.join("result.pending"),
            &serde_json::to_vec_pretty(&value)?,
        )?;
        // A visible result is also the end of capability ownership. Do not
        // expose a half-written receipt while the next request still sees busy.
        drop(_clear);
        std::fs::rename(dir.join("result.pending"), dir.join("result.json"))?;
        Ok(value)
    }
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    invocation: String,
    #[serde(default)]
    kit: Option<String>,
    #[serde(default)]
    text: Option<String>,
}
