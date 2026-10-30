//! Declared interfaces are not subscriptions or execution grants.
use super::{Result, bundle};
use serde::{Deserialize, Serialize};

pub(super) const ARTIFACT: &str = "artifact.accepted";
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Interface {
    pub consumes: Vec<String>,
    pub produces: Vec<String>,
}

pub(super) fn validate(m: &bundle::Manifest) -> Result<()> {
    match (m.schema.as_str(), &m.events) {
        ("an-native-spool-v1" | "an-native-spool-v2", None) => Ok(()),
        ("an-native-spool-v3", Some(events)) => {
            for names in [&events.consumes, &events.produces] {
                if names.len() > 1 || names.iter().any(|name| name != ARTIFACT) {
                    return Err("unsupported or duplicate native event interface".into());
                }
            }
            Ok(())
        }
        _ => Err("native schema and event interface must agree".into()),
    }
}

pub(super) fn permits(m: &bundle::Manifest, consume: bool) -> Result<()> {
    let e = m
        .events
        .as_ref()
        .ok_or("release has no native event interface")?;
    let names = if consume { &e.consumes } else { &e.produces };
    if !names.iter().any(|name| name == ARTIFACT) {
        return Err("event is outside the declared native interface".into());
    }
    Ok(())
}
