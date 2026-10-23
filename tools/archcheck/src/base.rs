//! Base selection is explicit; no missing or malformed comparison is skipped.
use crate::Result;
use pplx_space_archcheck::{Baseline, Context, Renames, config::CrateNamesPolicy};
use std::{fs, path::Path, process::Command};

// One reviewed migration for main's first receipt of the architecture gate.
// Compare against committed rules, never the feature's proposed baseline.
const PRE_GATE_MAIN: &str = "a3ff193c7d9732f2c272125e9b9c925bc9096567";
const FIRST_POLICY: &str = "8c89829b9de7506925a5d1d107d04df500ddd54f";

fn policy_revision(revision: &str) -> &str {
    if revision == PRE_GATE_MAIN {
        FIRST_POLICY
    } else {
        revision
    }
}

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(format!(
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(output.stdout)
}
pub fn attach(
    root: &Path,
    reference: &str,
    bootstrap: &str,
    scratch: &Path,
    ctx: &mut Context,
) -> Result<()> {
    if bootstrap != "f54cfa71e463a7b2902a16a26494dc44fce2485a" {
        return Err("bootstrap anchor cannot be changed by policy configuration".into());
    }
    let revision = String::from_utf8(git(
        root,
        &["rev-parse", "--verify", &format!("{reference}^{{commit}}")],
    )?)?
    .trim()
    .to_owned();
    let bootstrap_mode = revision == bootstrap;
    let policy_base = policy_revision(&revision);
    if policy_base != revision {
        // Missing history or an unrelated checkout must fail, not seed policy.
        git(root, &["merge-base", "--is-ancestor", FIRST_POLICY, "HEAD"])?;
        eprintln!("first main promotion: compare {revision} against reviewed policy {policy_base}");
    }
    for file in [
        "ARCHITECTURE.toml",
        "architecture-baseline.toml",
        "architecture-policy.toml",
    ] {
        let target = scratch.join(file);
        if bootstrap_mode {
            // Only this exact pre-policy commit can seed a baseline.
            fs::copy(root.join(file), &target)?;
            eprintln!("bootstrap review against {revision}: {file}");
        } else {
            fs::write(
                &target,
                git(root, &["show", &format!("{policy_base}:{file}")])?,
            )?;
        }
    }
    // Parse strictly before upstream's intentionally lenient base loader.
    Baseline::load(&scratch.join("architecture-baseline.toml"))?;
    for file in ["ARCHITECTURE.toml", "architecture-policy.toml"] {
        let before: toml::Value = toml::from_str(&fs::read_to_string(scratch.join(file))?)?;
        let after: toml::Value = toml::from_str(&fs::read_to_string(root.join(file))?)?;
        if before != after {
            return Err(format!(
                "{file} policy changed: requires a separate reviewed gate migration"
            )
            .into());
        }
    }
    ctx.base_baseline = Baseline::load_base(&scratch.join("architecture-baseline.toml"))?;
    ctx.base_architecture = CrateNamesPolicy::load_base(&scratch.join("ARCHITECTURE.toml"))?;
    let renamed = scratch.join("renames");
    // Include uncommitted moves, not only HEAD.
    fs::write(
        &renamed,
        git(
            root,
            &["diff", "-z", "--name-status", "--find-renames", &revision],
        )?,
    )?;
    ctx.renames = Some(Renames::load(&renamed)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_is_pinned_to_one_base_and_never_selects_a_moving_branch() {
        assert_eq!(policy_revision(PRE_GATE_MAIN), FIRST_POLICY);
        for other in [
            "main",
            "origin/main",
            "HEAD",
            FIRST_POLICY,
            "f54cfa71e463a7b2902a16a26494dc44fce2485a",
            "0000000000000000000000000000000000000000",
        ] {
            assert_eq!(policy_revision(other), other);
        }
    }
}
