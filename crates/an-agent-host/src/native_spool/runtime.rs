//! Operator-selected Python dependencies, separate from portable release requests.
use super::*;
use serde::{Deserialize, Serialize};

mod inventory;
pub(super) use crate::execution::sandbox::workspace::PYTHON_RUNTIME as PYTHON;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Runtime {
    schema: String,
    executable: PathBuf,
    executable_sha256: String,
    stdlib: PathBuf,
    site: PathBuf,
    site_sha256: String,
    dependencies: Vec<String>,
    #[serde(default)]
    readonly: Vec<PathBuf>,
    #[serde(default)]
    inputs: Vec<PathBuf>,
    #[serde(default)]
    graphics: Option<String>,
}

fn python(bundle: &bundle::Bundle) -> bool {
    bundle
        .manifest
        .requirements
        .as_ref()
        .is_some_and(|r| r.runtime == PYTHON)
}

pub(super) fn prepare(
    bundle: &bundle::Bundle,
    selected: Option<&Path>,
    root: &Path,
    package: &Path,
) -> Result<Option<Vec<u8>>> {
    match (python(bundle), selected) {
        (false, None) => Ok(None),
        (true, Some(path)) => {
            let bytes = regular(path, 65536)?;
            let runtime: Runtime = serde_json::from_slice(&bytes)?;
            runtime.check(bundle)?;
            for path in [&runtime.stdlib, &runtime.site, &runtime.executable]
                .into_iter()
                .chain(runtime.readonly.iter())
                .chain(runtime.inputs.iter())
            {
                let package = package.canonicalize()?;
                if root.starts_with(path)
                    || path.starts_with(root)
                    || package.starts_with(path)
                    || path.starts_with(&package)
                {
                    return Err(
                        "runtime must be separate from instance and release material".into(),
                    );
                }
            }
            Ok(Some(bytes))
        }
        _ => Err(
            "Python releases require an explicit operator runtime; shell releases do not".into(),
        ),
    }
}

/// Retained evidence remains readable when the external environment is unavailable.
pub(super) fn binding(
    root: &Path,
    instance: &Value,
    bundle: &bundle::Bundle,
) -> Result<Option<Runtime>> {
    if !python(bundle) {
        if instance.get("runtime_sha256").is_some() || root.join("runtime.json").exists() {
            return Err("unexpected native runtime binding".into());
        }
        return Ok(None);
    }
    let bytes = regular(&root.join("runtime.json"), 65536)?;
    if instance["runtime_sha256"] != store::digest(&bytes) {
        return Err("instance runtime binding changed".into());
    }
    Ok(Some(serde_json::from_slice(&bytes)?))
}

pub(super) fn verify(
    root: &Path,
    instance: &Value,
    bundle: &bundle::Bundle,
) -> Result<Option<Runtime>> {
    let runtime = binding(root, instance, bundle)?;
    if let Some(runtime) = &runtime {
        runtime.check(bundle)?;
    }
    Ok(runtime)
}

impl Runtime {
    fn check(&self, bundle: &bundle::Bundle) -> Result<()> {
        let requested = &bundle
            .manifest
            .requirements
            .as_ref()
            .ok_or("requirements")?
            .dependencies;
        let actual: std::collections::BTreeSet<_> = self.dependencies.iter().collect();
        let expected: std::collections::BTreeSet<_> = requested.iter().collect();
        if self.schema != "an-native-python-runtime-v1"
            || actual != expected
            || actual.len() != self.dependencies.len()
            || !store::valid_digest(&self.site_sha256)
            || !store::valid_digest(&self.executable_sha256)
        {
            return Err(
                "runtime schema, dependency labels or digests do not match the release".into(),
            );
        }
        if bundle
            .manifest
            .requirements
            .as_ref()
            .is_some_and(|r| r.gpu == "metal")
            && self.graphics.as_deref() != Some("macos-agx-metal-v1")
        {
            return Err("Metal release requires explicit operator graphics selection".into());
        }
        if self.readonly.len() > 6
            || self.inputs.len() > 8
            || self
                .graphics
                .as_deref()
                .is_some_and(|g| g != "macos-agx-metal-v1")
        {
            return Err("unsupported Python runtime extensions".into());
        }
        for path in [&self.executable, &self.stdlib, &self.site]
            .into_iter()
            .chain(self.readonly.iter())
            .chain(self.inputs.iter())
        {
            store::no_links(path)?;
            if !path.is_absolute() || *path != path.canonicalize()? || path == Path::new("/") {
                return Err("runtime paths must be canonical absolute selections".into());
            }
        }
        if !self.stdlib.is_dir()
            || !self.site.is_dir()
            || self.site.starts_with(&self.stdlib)
            || self.stdlib.starts_with(&self.site)
            || self.executable.starts_with(&self.site)
            || store::digest(&regular(&self.executable, 20 * 1024 * 1024)?)
                != self.executable_sha256
            || inventory::digest(&self.site)? != self.site_sha256
        {
            return Err("selected Python executable or dependency inventory changed".into());
        }
        Ok(())
    }
}

pub(super) fn profile(root: &Path, bundle: &bundle::Bundle, profile: &mut Value) -> Result<()> {
    let instance: Value = serde_json::from_slice(&regular(&root.join("instance.json"), 65536)?)?;
    if let Some(runtime) = verify(root, &instance, bundle)? {
        profile["executable"] = json!(runtime.executable);
        let entry = profile["args"][0].clone();
        // No site startup, user packages, .pth processing or bytecode writes.
        profile["args"] = json!(["-S", "-B", entry]);
        let mut roots = vec![runtime.stdlib.clone(), runtime.site.clone()];
        roots.extend(runtime.readonly);
        profile["readonly"] = json!(roots);
        if let Some(graphics) = runtime.graphics {
            profile["graphics"] = json!(graphics);
        }
        for (index, path) in runtime.inputs.iter().enumerate() {
            let slot = profile["inputs"].as_array().ok_or("profile inputs")?.len();
            profile["inputs"]
                .as_array_mut()
                .ok_or("profile inputs")?
                .push(json!(path));
            profile["input_layout"]
                .as_array_mut()
                .ok_or("Python input layout")?
                .push(json!(format!(
                    "aux-{index}-{}",
                    path.file_name()
                        .and_then(|p| p.to_str())
                        .ok_or("auxiliary basename")?
                )));
            profile["environment"][format!("AN_SPOOL_AUX_{index}")] =
                json!(format!("{{input:{slot}}}"));
        }
        profile["environment"]["PYTHONPATH"] =
            json!(format!("{{inputs}}:{}", runtime.site.display()));
        profile["environment"]["PYTHONNOUSERSITE"] = json!("1");
        profile["environment"]["PYTHONDONTWRITEBYTECODE"] = json!("1");
        profile["environment"]["HOME"] = json!("{work}");
        profile["environment"]["AN_SPOOL_RUNTIME_SHA256"] = instance["runtime_sha256"].clone();
    }
    Ok(())
}

/// Operator graphics selection alone permits the existing longer workspace bound.
pub(super) fn maximum_seconds(root: &Path, bundle: &bundle::Bundle) -> Result<u64> {
    let instance: Value = serde_json::from_slice(&regular(&root.join("instance.json"), 65536)?)?;
    Ok(
        if binding(root, &instance, bundle)?
            .is_some_and(|r| r.graphics.as_deref() == Some("macos-agx-metal-v1"))
        {
            900
        } else {
            300
        },
    )
}
