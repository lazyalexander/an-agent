//! Strict admission for the first explicit runtime-requirements contract.
use super::{Result, bundle};
use crate::execution::sandbox::workspace;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Requirements {
    pub runtime: String,
    os: String,
    arch: String,
    interaction: String,
    gpu: String,
    pub inputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_input: Option<String>,
    pub editable: Vec<String>,
    capabilities: Vec<String>,
    pub dependencies: Vec<String>,
}
pub(super) fn schema(m: &bundle::Manifest) -> Result<()> {
    match (m.schema.as_str(), &m.requirements) {
        ("an-native-spool-v1", None) | ("an-native-spool-v2" | "an-native-spool-v3", Some(_)) => {
            state_slot(m)
        }
        _ => Err("native schema and requirements must agree".into()),
    }
}

fn state_slot(m: &bundle::Manifest) -> Result<()> {
    let Some(r) = &m.requirements else {
        return Ok(());
    };
    let Some(slot) = &r.state_input else {
        return Ok(());
    };
    bundle::identifier(slot)?;
    if let Some(data) = r.inputs.first() {
        bundle::identifier(data)?;
    }
    if m.schema != "an-native-spool-v3"
        || r.inputs.len() != 1
        || matches!(slot.as_str(), "kit" | "tmp")
        || r.inputs
            .first()
            .is_some_and(|p| matches!(p.as_str(), "kit" | "tmp"))
        || r.inputs.contains(slot)
        || m.files.keys().any(|p| {
            p.rsplit('/').next() == Some(slot.as_str())
                || p.rsplit('/').next() == r.inputs.first().map(String::as_str)
        })
    {
        return Err("state input requires distinct v3 data, state and code slots".into());
    }
    Ok(())
}
pub(super) fn validate(m: &bundle::Manifest) -> Result<()> {
    schema(m)?;
    let Some(r) = &m.requirements else {
        return Ok(());
    };
    let python = r.runtime == super::runtime::PYTHON && m.schema == "an-native-spool-v3";
    if (!python && r.runtime != workspace::SHELL_RUNTIME)
        || r.os != workspace::TARGET_OS
        || !workspace::supported()
        || r.arch != "portable-source"
        || r.interaction != "headless"
        || r.gpu != "none"
    {
        return Err(
            "unsupported native runtime, OS, architecture, interaction or GPU requirement".into(),
        );
    }
    if !python && !r.dependencies.is_empty() {
        return Err("unresolved native dependencies".into());
    }
    if python {
        let mut seen = BTreeSet::new();
        if r.dependencies.len() > 16 {
            return Err("too many Python dependency labels".into());
        }
        for name in &r.dependencies {
            bundle::identifier(name)?;
            if !seen.insert(name) {
                return Err("duplicate Python dependency label".into());
            }
        }
    }
    if r.inputs.len() != 1 {
        return Err("native profile requires one file input slot".into());
    }
    bundle::identifier(&r.inputs[0])?;
    let requested: BTreeSet<_> = r.capabilities.iter().map(String::as_str).collect();
    if requested.len() != r.capabilities.len()
        || requested != BTreeSet::from(["read-input", "write-declared-outputs"])
    {
        return Err("unsupported capability set for fixed native profile".into());
    }
    let mut seen = BTreeSet::new();
    for path in &r.editable {
        if !path.starts_with("kit/") || !m.files.contains_key(path) || !seen.insert(path) {
            return Err("editable path must be a unique selected kit file".into());
        }
    }
    Ok(())
}

pub(super) fn editable(m: &bundle::Manifest, path: &str) -> Result<()> {
    if m.requirements
        .as_ref()
        .is_some_and(|r| !r.editable.iter().any(|p| p == path))
    {
        return Err("kit path is not editable in this release".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "requirements_tests.rs"]
mod tests;
