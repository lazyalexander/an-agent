//! Rhai-backed tool: the descriptor's effect IS the host-function wiring
//! (contract T8) — `net: egress` registers `http_get_json`; with `proc: none`
//! no `exec` exists. Probe-grade; kernel integration lands with the YAML
//! registry slice.

use an_agent::act::{Permit, Tool, ToolCtx, ToolTag};
use an_agent::tools::descriptor::{Constructor, Descriptor, Net};

pub struct RhaiTool {
    desc: Descriptor,
    script: String,
    // Descriptor v1 has no schema field — parameters come from the caller
    // (normally the constructor's implementation) until the format grows one.
    params: serde_json::Value,
}

impl RhaiTool {
    pub fn from_descriptor(desc: Descriptor, params: serde_json::Value) -> Result<Self, String> {
        let script = match &desc.constructor {
            Constructor::Rhai { script } => script.clone(),
            _ => return Err("not a rhai descriptor".into()),
        };
        Ok(Self {
            desc,
            script,
            params,
        })
    }
}

fn eval_err(e: impl std::fmt::Display) -> Box<rhai::EvalAltResult> {
    Box::new(rhai::EvalAltResult::ErrorSystem(
        "host".into(),
        e.to_string().into(),
    ))
}

fn run_script(script: &str, allow_net: bool, args: serde_json::Value) -> Result<String, String> {
    let mut engine = rhai::Engine::new();
    engine.set_max_operations(100_000);
    if allow_net {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
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
                let query: Vec<(String, String)> = params
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                let fetched = client
                    .get(url)
                    .query(&query)
                    .send()
                    .and_then(|r| r.text())
                    .map_err(|e| e.to_string())
                    .and_then(|text| {
                        serde_json::from_str::<serde_json::Value>(&text).map_err(|e| e.to_string())
                    });
                match fetched {
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

    async fn execute(&self, args: serde_json::Value, _ctx: &ToolCtx) -> Result<String, String> {
        let script = self.script.clone();
        let allow_net = self.desc.effect.net == Net::Egress;
        tokio::task::spawn_blocking(move || run_script(&script, allow_net, args))
            .await
            .map_err(|e| e.to_string())?
    }
}
