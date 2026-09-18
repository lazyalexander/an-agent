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

fn run_script(
    script: &str,
    allow_net: bool,
    args: serde_json::Value,
    allowed_hosts: Vec<String>,
) -> Result<String, String> {
    let mut engine = rhai::Engine::new();
    // Probe scripts run untrusted-ish registry content: hard resource
    // bounds, not just an operation budget.
    engine.set_max_operations(100_000);
    engine.set_max_string_size(1 << 20);
    engine.set_max_array_size(10_000);
    engine.set_max_map_size(10_000);
    engine.set_max_expr_depths(64, 64);
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
}
