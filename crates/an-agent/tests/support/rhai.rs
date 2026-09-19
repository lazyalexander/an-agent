//! Rhai-backed tool: the descriptor's effect IS the host-function wiring
//! (contract T8) — `net: egress` registers `http_get_json`; with `proc: none`
//! no `exec` exists. Probe-grade; kernel integration lands with the YAML
//! registry slice.

use an_agent::act::{Permit, Tool, ToolCtx, ToolTag};
use an_agent::tools::descriptor::{Constructor, Descriptor, Net};

/// Responses are byte-capped at the host fn: a script that can name any
/// URL must not also get unbounded memory via the response body.
const MAX_RESPONSE_BYTES: u64 = 1 << 20;

pub struct RhaiTool {
    desc: Descriptor,
    script: String,
    // Descriptor v1 has no schema field — parameters come from the caller
    // (normally the constructor's implementation) until the format grows one.
    params: serde_json::Value,
    allowed_hosts: Vec<String>,
}

impl RhaiTool {
    /// `allowed_hosts` bounds the egress face from outside until the
    /// descriptor can carry it (`net: egress(to: [...])` is a documented
    /// future loosening); today the declaration alone cannot say *where*.
    pub fn from_descriptor(
        desc: Descriptor,
        params: serde_json::Value,
        allowed_hosts: &[&str],
    ) -> Result<Self, String> {
        // The probe wires only the net face: rhai gets no FS host fns and
        // no exec. A descriptor declaring a face the constructor cannot
        // honor must fail here, not at first call — a file r/w/rw tag would
        // die in admission (MissingResource) and proc: spawn has no effector.
        use an_agent::act::FileFacet;
        use an_agent::tools::descriptor::Proc;
        if matches!(
            desc.effect.file,
            FileFacet::Read { .. } | FileFacet::Write { .. } | FileFacet::ReadWrite { .. }
        ) {
            return Err(format!(
                "rhai constructor cannot wire file face {:?}",
                desc.effect.file
            ));
        }
        if desc.effect.proc_ == Proc::Spawn {
            return Err("rhai constructor cannot wire proc: spawn".into());
        }
        let script = match &desc.constructor {
            Constructor::Rhai { script } => script.clone(),
            _ => return Err("not a rhai descriptor".into()),
        };
        Ok(Self {
            desc,
            script,
            params,
            allowed_hosts: allowed_hosts.iter().map(|s| s.to_string()).collect(),
        })
    }
}

fn eval_err(e: impl std::fmt::Display) -> Box<rhai::EvalAltResult> {
    Box::new(rhai::EvalAltResult::ErrorSystem(
        "host".into(),
        e.to_string().into(),
    ))
}

/// SSRF gate: scripts name raw URLs, so the host fn only honors https URLs
/// to allowlisted hosts. Anything else comes back as a normal `#{ error }`.
pub fn check_url(url: &str, allowed_hosts: &[String]) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() != "https" {
        return Err(format!("only https egress is allowed: {url}"));
    }
    let host = parsed.host_str().unwrap_or("");
    if !allowed_hosts.iter().any(|h| h == host) {
        return Err(format!("host not in egress allowlist: {host}"));
    }
    Ok(parsed)
}

fn fetch(
    client: &reqwest::blocking::Client,
    url: &str,
    params: rhai::Map,
    allowed_hosts: &[String],
) -> Result<serde_json::Value, String> {
    use std::io::Read;

    let url = check_url(url, allowed_hosts)?;
    let query: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let resp = client
        .get(url)
        .query(&query)
        .send()
        .map_err(|e| e.to_string())?;
    // Redirect targets are re-checked: an allowlisted host must not vouch
    // for where it points (e.g. a 302 to a link-local metadata endpoint).
    let final_url = resp.url();
    let final_host = final_url.host_str().unwrap_or("");
    if final_url.scheme() != "https" || !allowed_hosts.iter().any(|h| h == final_host) {
        return Err(format!("redirect left the allowlist: {final_url}"));
    }
    let mut body = resp.take(MAX_RESPONSE_BYTES + 1);
    let mut buf = Vec::new();
    body.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(format!("response exceeds {MAX_RESPONSE_BYTES} bytes"));
    }
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

/// Probe scripts run untrusted-ish registry content: hard resource
/// bounds, not just an operation budget.
fn bounded_engine() -> rhai::Engine {
    let mut engine = rhai::Engine::new();
    engine.set_max_operations(100_000);
    engine.set_max_string_size(1 << 20);
    engine.set_max_array_size(10_000);
    engine.set_max_map_size(10_000);
    engine.set_max_expr_depths(64, 64);
    engine
}

fn run_script(
    script: &str,
    allow_net: bool,
    args: serde_json::Value,
    allowed_hosts: Vec<String>,
) -> Result<String, String> {
    let mut engine = bounded_engine();
    if allow_net {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .user_agent("an-agent-rhai-probe/0.1")
            .build()
            .map_err(|e| e.to_string())?;
        // Network failure is a normal outcome, not an exception: rhai's
        // try/catch does not intercept host-fn errors, so failures come back
        // as #{ error: msg } maps the script can branch on (and the tape sees).
        engine.register_fn(
            "http_get_json",
            move |url: &str,
                  params: rhai::Map|
                  -> Result<rhai::Dynamic, Box<rhai::EvalAltResult>> {
                match fetch(&client, url, params, &allowed_hosts) {
                    Ok(json) => rhai::serde::to_dynamic(json).map_err(eval_err),
                    Err(msg) => {
                        let mut m = rhai::Map::new();
                        m.insert("error".into(), msg.into());
                        Ok(m.into())
                    }
                }
            },
        );
    }
    let mut scope = rhai::Scope::new();
    scope.push(
        "params",
        rhai::serde::to_dynamic(args).map_err(|e| e.to_string())?,
    );
    engine
        .eval_with_scope::<rhai::Dynamic>(&mut scope, script)
        .map(|d| d.to_string())
        .map_err(|e| e.to_string())
}

/// Resolves once the signal fires; pends forever without one.
async fn wait_cancel(signal: Option<tokio::sync::watch::Receiver<bool>>) {
    let Some(mut rx) = signal else {
        return std::future::pending::<()>().await;
    };
    loop {
        if *rx.borrow() {
            return;
        }
        if rx.changed().await.is_err() {
            return std::future::pending::<()>().await;
        }
    }
}

// --- policy sort: tape projection in, continuation out ---

/// The next step a policy script yields (effect-as-data): the host admits
/// and executes it; the script never calls a model or a tool itself.
/// Malformed output is a hard error, never silently reinterpreted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Continuation {
    Halt,
    Utter {
        text: String,
    },
    InvokeModel {
        clips: Vec<String>,
    },
    InvokeTool {
        name: String,
        args: serde_json::Value,
    },
}

impl Continuation {
    pub fn from_value(v: &serde_json::Value) -> Result<Self, String> {
        let kind = v
            .get("kind")
            .and_then(|k| k.as_str())
            .ok_or("continuation requires a string kind")?;
        match kind {
            "halt" => Ok(Self::Halt),
            "utter" => Ok(Self::Utter {
                text: v
                    .get("text")
                    .and_then(|t| t.as_str())
                    .ok_or("utter requires a string text")?
                    .to_string(),
            }),
            "invoke_model" => {
                let clips = v
                    .get("clips")
                    .and_then(|c| c.as_array())
                    .ok_or("invoke_model requires a clips array")?
                    .iter()
                    .map(|c| {
                        c.as_str()
                            .map(str::to_string)
                            .ok_or_else(|| "clips entries must be strings".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::InvokeModel { clips })
            }
            "invoke_tool" => {
                let name = v
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or("invoke_tool requires a string name")?
                    .to_string();
                let args = v
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                if !args.is_object() {
                    return Err("invoke_tool args must be an object".into());
                }
                Ok(Self::InvokeTool { name, args })
            }
            other => Err(format!("unknown continuation kind: {other}")),
        }
    }

    /// Canonical JSON form — what lands on tape as the decision's effect.
    pub fn to_value(&self) -> serde_json::Value {
        match self {
            Self::Halt => serde_json::json!({ "kind": "halt" }),
            Self::Utter { text } => serde_json::json!({ "kind": "utter", "text": text }),
            Self::InvokeModel { clips } => {
                serde_json::json!({ "kind": "invoke_model", "clips": clips })
            }
            Self::InvokeTool { name, args } => {
                serde_json::json!({ "kind": "invoke_tool", "name": name, "args": args })
            }
        }
    }
}

/// Policy-sort rhai tool: pure computation over a host-injected tape
/// projection. The declared effect must be empty of effectors — a policy
/// that declares file/net/proc faces is rejected at construction, the
/// same fail-fast gate as RhaiTool.
pub struct RhaiPolicy {
    desc: Descriptor,
    script: String,
}

impl RhaiPolicy {
    pub fn from_descriptor(desc: Descriptor) -> Result<Self, String> {
        use an_agent::act::FileFacet;
        use an_agent::tools::descriptor::{Net, Proc};
        if desc.effect.file != FileFacet::None {
            return Err(format!(
                "policy declares a file face it cannot use: {:?}",
                desc.effect.file
            ));
        }
        if desc.effect.net != Net::None {
            return Err("policy declares net it cannot use".into());
        }
        if desc.effect.proc_ != Proc::None {
            return Err("policy declares proc it cannot use".into());
        }
        let script = match &desc.constructor {
            Constructor::Rhai { script } => script.clone(),
            _ => return Err("not a rhai descriptor".into()),
        };
        Ok(Self { desc, script })
    }

    /// Names this policy may yield in `invoke_tool` — the descriptor's
    /// requires list, enforced by the host driver.
    pub fn whitelist(&self) -> Vec<String> {
        self.desc.requires.iter().map(|r| r.name.clone()).collect()
    }
}

fn run_policy_script(script: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
    let engine = bounded_engine();
    let mut scope = rhai::Scope::new();
    scope.push(
        "params",
        rhai::serde::to_dynamic(args).map_err(|e| e.to_string())?,
    );
    let out = engine
        .eval_with_scope::<rhai::Dynamic>(&mut scope, script)
        .map_err(|e| e.to_string())?;
    rhai::serde::from_dynamic::<serde_json::Value>(&out).map_err(|e| e.to_string())
}

#[async_trait::async_trait]
impl Tool for RhaiPolicy {
    fn name(&self) -> &str {
        &self.desc.name
    }

    fn description(&self) -> &str {
        &self.desc.summary
    }

    fn parameters(&self) -> serde_json::Value {
        // Not model-facing: the host injects the tape projection as args.
        serde_json::json!({
            "type": "object",
            "properties": {
                "clips": {
                    "type": "array",
                    "description": "recent tape projection, newest first: id/kind/from/tags/preview per clip"
                },
                "last_observation": { "type": "string" }
            }
        })
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        Some(ToolTag {
            file: an_agent::act::FileFacet::None,
            permit: Permit::Go,
            memory: self.desc.effect.memory.clone(),
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> Result<String, String> {
        let script = self.script.clone();
        let mut task = tokio::task::spawn_blocking(move || run_policy_script(&script, args));
        let value = tokio::select! {
            res = &mut task => res.map_err(|e| e.to_string())??,
            _ = wait_cancel(ctx.signal.clone()) => return Err("cancelled".into()),
        };
        // Validate at the boundary: the taped effect is always a
        // well-formed continuation or an error, never garbage.
        let cont = Continuation::from_value(&value)?;
        Ok(cont.to_value().to_string())
    }
}

#[async_trait::async_trait]
impl Tool for RhaiTool {
    fn name(&self) -> &str {
        &self.desc.name
    }

    fn description(&self) -> &str {
        &self.desc.summary
    }

    fn parameters(&self) -> serde_json::Value {
        self.params.clone()
    }

    fn tag_seed(&self) -> Option<ToolTag> {
        // The declared effect derives the admission seed directly.
        Some(ToolTag {
            file: self.desc.effect.file.clone(),
            permit: Permit::Go,
            memory: self.desc.effect.memory.clone(),
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolCtx) -> Result<String, String> {
        let script = self.script.clone();
        let allow_net = self.desc.effect.net == Net::Egress;
        let hosts = self.allowed_hosts.clone();
        let mut task =
            tokio::task::spawn_blocking(move || run_script(&script, allow_net, args, hosts));
        // A cancelled probe stops waiting here. The blocking request itself
        // cannot be killed; it finishes under its own 30s timeout.
        tokio::select! {
            res = &mut task => res.map_err(|e| e.to_string())?,
            _ = wait_cancel(ctx.signal.clone()) => Ok("cancelled".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts() -> Vec<String> {
        vec!["api.duckduckgo.com".to_string()]
    }

    #[test]
    fn url_gate_allows_only_https_allowlisted_hosts() {
        assert!(check_url("https://api.duckduckgo.com/?q=x", &hosts()).is_ok());
        // Plain http is refused even for an allowlisted host.
        assert!(
            check_url("http://api.duckduckgo.com/", &hosts())
                .unwrap_err()
                .contains("https")
        );
        // Off-list hosts are refused — including link-local metadata.
        assert!(
            check_url("https://169.254.169.254/latest/meta-data", &hosts())
                .unwrap_err()
                .contains("allowlist")
        );
        assert!(check_url("https://evil.example.com/", &hosts()).is_err());
    }

    fn desc(effect: &str) -> Descriptor {
        let yaml = format!(
            "v: 1\nname: probe_tool\nversion: 0.1.0\nconstructor: rhai\nscript: |\n  1\nsummary: probe\neffect:\n{effect}\nrequires: []\n"
        );
        an_agent::tools::descriptor::parse(&yaml).unwrap()
    }

    #[test]
    fn unwireable_faces_fail_at_construction() {
        let spawn =
            desc("  net: none\n  file: { op: none }\n  proc: spawn\n  memory: { op: ignore }");
        assert!(
            RhaiTool::from_descriptor(spawn, serde_json::json!({}), &[])
                .err()
                .unwrap()
                .contains("proc: spawn")
        );
        let read = desc(
            "  net: none\n  file: { op: r, path: \"/srv\" }\n  proc: none\n  memory: { op: ignore }",
        );
        assert!(
            RhaiTool::from_descriptor(read, serde_json::json!({}), &[])
                .err()
                .unwrap()
                .contains("file face")
        );
        // The faces the probe does wire still construct.
        let ok = desc(
            "  net: egress\n  file: { op: unbounded }\n  proc: none\n  memory: { op: ignore }",
        );
        assert!(RhaiTool::from_descriptor(ok, serde_json::json!({}), &[]).is_ok());
    }

    #[test]
    fn continuation_parsing_is_strict() {
        use serde_json::json;
        assert_eq!(
            Continuation::from_value(&json!({"kind": "halt"})).unwrap(),
            Continuation::Halt
        );
        assert_eq!(
            Continuation::from_value(&json!({"kind": "utter", "text": "hi"})).unwrap(),
            Continuation::Utter { text: "hi".into() }
        );
        assert_eq!(
            Continuation::from_value(&json!({"kind": "invoke_model", "clips": ["a", "b"]}))
                .unwrap(),
            Continuation::InvokeModel {
                clips: vec!["a".into(), "b".into()]
            }
        );
        assert_eq!(
            Continuation::from_value(
                &json!({"kind": "invoke_tool", "name": "echo", "args": {"text": "x"}})
            )
            .unwrap(),
            Continuation::InvokeTool {
                name: "echo".into(),
                args: json!({"text": "x"})
            }
        );
        // Garbage is a hard error, never reinterpreted.
        assert!(Continuation::from_value(&json!({"kind": "nope"})).is_err());
        assert!(Continuation::from_value(&json!({"kind": "utter"})).is_err());
        assert!(Continuation::from_value(&json!({"clips": []})).is_err());
        assert!(Continuation::from_value(&json!({"kind": "invoke_model", "clips": [1]})).is_err());
        assert!(
            Continuation::from_value(&json!({"kind": "invoke_tool", "name": "x", "args": 1}))
                .is_err()
        );
        // Canonical form round-trips through the parser.
        let cont = Continuation::InvokeTool {
            name: "echo".into(),
            args: json!({}),
        };
        assert_eq!(Continuation::from_value(&cont.to_value()).unwrap(), cont);
    }

    #[test]
    fn policy_rejects_effector_faces_and_keeps_requires_whitelist() {
        let net =
            desc("  net: egress\n  file: { op: none }\n  proc: none\n  memory: { op: ignore }");
        assert!(
            RhaiPolicy::from_descriptor(net)
                .err()
                .unwrap()
                .contains("net")
        );
        let file = desc(
            "  net: none\n  file: { op: r, path: \"/srv\" }\n  proc: none\n  memory: { op: ignore }",
        );
        assert!(
            RhaiPolicy::from_descriptor(file)
                .err()
                .unwrap()
                .contains("file face")
        );
        let spawn =
            desc("  net: none\n  file: { op: none }\n  proc: spawn\n  memory: { op: ignore }");
        assert!(
            RhaiPolicy::from_descriptor(spawn)
                .err()
                .unwrap()
                .contains("proc")
        );
        // A pure policy constructs; requires become the invoke whitelist.
        let yaml = "v: 1\nname: pol\nversion: 0.1.0\nconstructor: rhai\nscript: |\n  #{ kind: \"halt\" }\nsummary: p\neffect:\n  net: none\n  file: { op: none }\n  proc: none\n  memory: { op: remember, aspect: ctx }\nrequires:\n  - { name: echo, version: 1.0.0 }\n";
        let policy =
            RhaiPolicy::from_descriptor(an_agent::tools::descriptor::parse(yaml).unwrap()).unwrap();
        assert_eq!(policy.whitelist(), vec!["echo".to_string()]);
    }
}
