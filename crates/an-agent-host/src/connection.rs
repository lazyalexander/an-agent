//! Operator-owned connection policy; spool cards cannot redirect credentials.
use crate::HttpModelClient;
use an_agent_core::control::{Completion, ModelClient, ModelMessage};
use an_agent_core::principal::card::ModelSpec;

/// One fixed endpoint, model and provider-options binding. Construct outside
/// untrusted workloads and install through AgentControl::set_model_client.
/// This is application-level egress admission, not an OS network sandbox.
pub struct ModelConnection {
    spec: ModelSpec,
    client: HttpModelClient,
}

impl ModelConnection {
    /// Credentials are supplied by the operator's secret resolver, never YAML.
    /// Plain HTTP is supported only for literal loopback development endpoints.
    pub fn new(spec: ModelSpec, bearer: Option<String>) -> Result<Self, String> {
        let url = reqwest::Url::parse(&spec.base_url).map_err(|_| "invalid model endpoint")?;
        let local = url
            .host_str()
            .and_then(|s| s.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !(url.scheme() == "https" || url.scheme() == "http" && local)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || spec.model.trim().is_empty()
            || spec.extra_body.as_ref().is_some_and(|v| !v.is_object())
        {
            return Err("invalid connection policy".into());
        }
        let client = match bearer {
            Some(key) => HttpModelClient::bearer(key)?,
            None => HttpModelClient::new()?,
        };
        Ok(Self { spec, client })
    }

    /// Use the resolved identity on the card so admission and tape see reality.
    pub fn spec(&self) -> &ModelSpec {
        &self.spec
    }
}

impl ModelClient for ModelConnection {
    fn complete(
        &self,
        spec: &ModelSpec,
        messages: Vec<ModelMessage>,
    ) -> Result<Completion, String> {
        if spec.base_url != self.spec.base_url
            || spec.model != self.spec.model
            || spec.extra_body != self.spec.extra_body
        {
            return Err("model connection policy mismatch".into());
        }
        if messages.len() > 64 || messages.iter().map(|m| m.content.len()).sum::<usize>() > 65536 {
            return Err("model request exceeds limit".into());
        }
        self.client.complete(spec, messages)
    }
}
