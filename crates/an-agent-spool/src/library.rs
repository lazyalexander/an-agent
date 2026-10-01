//! The spool library on disk: the authoring layout. A spool is a YAML
//! body plus the script it wraps — authors edit them as separate files,
//! and the body names its script with `script: "@file.rhai"` (relative to
//! the body's directory). Loading inlines the script, so the published
//! body stays self-contained and content-addressed: the separation exists
//! for authors, never for the registry.

use std::path::{Path, PathBuf};

use crate::spool::{Registry, SpoolError, SpoolSpec};

fn invalid(msg: impl Into<String>) -> SpoolError {
    SpoolError::Invalid(msg.into())
}

/// A script reference is exactly one line: `script: "@relative/path"`
/// (quotes optional). The marker keeps authoring separate while the body
/// stays one document.
fn script_ref(yaml: &str) -> Option<&str> {
    yaml.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("script:")?.trim();
        let rest = rest
            .strip_prefix('"')
            .and_then(|r| r.strip_suffix('"'))
            .or_else(|| rest.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')))
            .unwrap_or(rest);
        rest.strip_prefix('@').map(str::trim)
    })
}

fn check_ref_path(path: &str) -> Result<(), SpoolError> {
    let p = Path::new(path);
    if p.is_absolute()
        || path.contains('~')
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid(format!(
            "script ref must be a clean relative path: {path}"
        )));
    }
    Ok(())
}

/// Load a library body: read the YAML at `path`, inline its script ref if
/// any. Host constructors (no script) pass through unchanged.
pub fn load(path: &Path) -> Result<String, SpoolError> {
    let yaml = std::fs::read_to_string(path)?;
    let Some(reference) = script_ref(&yaml) else {
        return Ok(yaml);
    };
    check_ref_path(reference)?;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let script_path: PathBuf = dir.join(reference);
    let script = std::fs::read_to_string(&script_path)
        .map_err(|e| invalid(format!("script ref {reference} unreadable: {e}")))?;
    let mut inlined = String::with_capacity(yaml.len() + script.len() + 16);
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("script:") && trimmed.contains(reference) {
            inlined.push_str("script: |\n");
            for script_line in script.lines() {
                inlined.push_str("  ");
                inlined.push_str(script_line);
                inlined.push('\n');
            }
        } else {
            inlined.push_str(line);
            inlined.push('\n');
        }
    }
    if inlined.len() > crate::descriptor::MAX_DESCRIPTOR_BYTES {
        return Err(invalid(format!(
            "inlined body too large: {} bytes",
            inlined.len()
        )));
    }
    Ok(inlined)
}

/// Load and publish a library entry in one step.
pub fn publish(registry: &Registry, path: &Path) -> Result<SpoolSpec, SpoolError> {
    registry.publish(&load(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use an_agent_core::testkit::TempDir;

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    const BODY: &str = r#"v: 1
kind: spool
name: lib_entry
version: 1.0.0
summary: library entry with a wrapped script
constructor: rhai
script: "@entry.rhai"
effect:
  file: { op: none }
  memory: { op: ignore }
  net: none
  proc: none
  flow: none
inverse: none
requires: []
"#;

    #[test]
    fn load_inlines_the_script_and_the_body_stays_publishable() {
        let tmp = TempDir::new("spool-lib");
        let yaml = write(tmp.path(), "entry.yaml", BODY);
        write(
            tmp.path(),
            "entry.rhai",
            "let x = 1;\n#{ kind: \"halt\" }\n",
        );
        let body = load(&yaml).unwrap();
        assert!(body.contains("script: |"));
        assert!(body.contains("  let x = 1;"));
        assert!(!body.contains('@'));
        let spec = crate::spool::parse(&body).unwrap();
        assert!(matches!(
            spec.constructor,
            crate::spool::Constructor::Rhai { .. }
        ));
        let reg = Registry::open(tmp.path().join("registry")).unwrap();
        let published = publish(&reg, &yaml).unwrap();
        assert_eq!(published.name, "lib_entry");
    }

    #[test]
    fn dirty_refs_and_missing_files_are_refused() {
        let tmp = TempDir::new("spool-lib-dirty");
        let bad = write(
            tmp.path(),
            "bad.yaml",
            &BODY.replace("@entry.rhai", "@../escape.rhai"),
        );
        assert!(load(&bad).unwrap_err().to_string().contains("relative"));
        let missing = write(tmp.path(), "missing.yaml", BODY);
        assert!(
            load(&missing)
                .unwrap_err()
                .to_string()
                .contains("unreadable")
        );
    }

    #[test]
    fn host_bodies_pass_through() {
        let tmp = TempDir::new("spool-lib-host");
        let body = BODY
            .replace(
                "constructor: rhai\nscript: \"@entry.rhai\"",
                "constructor: host\nhost: echo",
            )
            .replace("name: lib_entry", "name: host_entry");
        let yaml = write(tmp.path(), "host.yaml", &body);
        let loaded = load(&yaml).unwrap();
        assert_eq!(loaded, body);
        assert!(crate::spool::parse(&loaded).is_ok());
    }
}
