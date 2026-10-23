//! Project-specific dependency boundaries; no product dependency on this tool.
use crate::Result;
use pplx_space_archcheck::Context;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    version: u8,
    pub bootstrap_base: String,
    production_warning: usize,
    test_warning: usize,
    test_limit_advisory: usize,
    dependencies: Vec<Edge>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edge {
    from: String,
    forbid: Vec<String>,
}
pub fn load(root: &Path) -> Result<Policy> {
    let p: Policy = toml::from_str(&fs::read_to_string(root.join("architecture-policy.toml"))?)?;
    if p.version != 1 || p.bootstrap_base.len() != 40 {
        return Err("unsupported architecture policy".into());
    }
    Ok(p)
}
fn denied(graph: &BTreeMap<String, Vec<String>>, start: &str, banned: &[String]) -> Option<String> {
    let mut visited = BTreeSet::new();
    let mut pending = graph.get(start).cloned().unwrap_or_default();
    while let Some(name) = pending.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        if banned.contains(&name) {
            return Some(name);
        }
        pending.extend(graph.get(&name).into_iter().flatten().cloned());
    }
    None
}
pub fn inspect(root: &Path, policy: &Policy, ctx: &Context) -> Result<()> {
    let output = Command::new("cargo")
        .current_dir(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--locked",
        ])
        .output()?;
    if !output.status.success() {
        return Err("cargo metadata failed".into());
    }
    let data: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let mut graph = BTreeMap::new();
    for package in data["packages"].as_array().ok_or("missing packages")? {
        let name = package["name"]
            .as_str()
            .ok_or("missing package name")?
            .to_owned();
        let deps = package["dependencies"]
            .as_array()
            .ok_or("missing dependencies")?
            .iter()
            .filter(|d| d["kind"].as_str() != Some("dev"))
            .map(|d| {
                d["name"]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or("missing dependency name")
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        graph.insert(name, deps);
    }
    for rule in &policy.dependencies {
        if rule.from != "*" && !graph.contains_key(&rule.from) {
            return Err(format!("stale dependency owner {}", rule.from).into());
        }
        for name in graph
            .keys()
            .filter(|n| rule.from == "*" || **n == rule.from)
        {
            if let Some(dep) = denied(&graph, name, &rule.forbid) {
                return Err(format!("forbidden dependency path: {name} -> ... -> {dep}").into());
            }
        }
    }
    for file in &ctx.files {
        if file.code_lines > policy.production_warning {
            eprintln!(
                "advisory: {} has {} effective lines",
                file.path, file.code_lines
            );
        }
    }
    // Test size is advisory in this first gate; never trade coverage for a cap.
    fn tests(path: &Path, policy: &Policy) -> Result<()> {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_symlink() {
                return Err("source symlink is outside the architecture inventory".into());
            }
            let path = entry.path();
            if path.is_dir() {
                tests(&path, policy)?;
            } else if path.extension().is_some_and(|e| e == "rs") {
                let name = path.file_name().unwrap().to_string_lossy();
                if path.components().any(|c| c.as_os_str() == "tests")
                    || name.ends_with("_tests.rs")
                    || name == "tests.rs"
                {
                    let lines = fs::read_to_string(&path)?
                        .lines()
                        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with("//"))
                        .count();
                    if lines > policy.test_warning {
                        eprintln!(
                            "advisory: {} test lines: {lines} (target ceiling {})",
                            path.display(),
                            policy.test_limit_advisory
                        );
                    }
                }
            }
        }
        Ok(())
    }
    tests(&root.join("crates"), policy)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indirect_edges_and_cycles_are_checked() {
        let g = BTreeMap::from([
            ("core".into(), vec!["bridge".into()]),
            ("bridge".into(), vec!["core".into(), "host".into()]),
        ]);
        assert_eq!(denied(&g, "core", &["host".into()]), Some("host".into()));
        assert_eq!(denied(&g, "core", &["waver".into()]), None);
    }
}
