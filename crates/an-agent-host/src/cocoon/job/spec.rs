use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Spec {
    pub transport: PathBuf,
    pub transport_sha256: String,
    pub(super) program: PathBuf,
    pub(super) program_sha256: String,
    pub(super) interpreter: String,
    pub(super) arguments: Vec<String>,
    pub(super) outputs: BTreeMap<String, String>,
}

pub(crate) struct Input {
    pub bytes: Vec<u8>,
    pub evidence: Value,
}

pub(crate) fn load(path: &Path, digest: &str) -> Result<Spec> {
    let bytes = store::read(path, 32768)?;
    if store::digest(&bytes) != digest {
        return Err("Cocoon job specification changed".into());
    }
    let spec: Spec = serde_json::from_slice(&bytes)?;
    if !spec.transport.is_absolute()
        || !spec.program.is_absolute()
        || !spec.arguments.iter().any(|a| a == "{program}")
        || spec.interpreter.is_empty()
        || !spec.interpreter.starts_with('/')
        || spec.interpreter.contains('\0')
        || spec.arguments.len() > 8
        || spec
            .arguments
            .iter()
            .any(|a| a.len() > 1024 || a.contains('\0'))
        || spec.outputs.is_empty()
        || spec.outputs.len() > 4
        || !store::valid_digest(&spec.program_sha256)
        || spec.outputs.iter().any(|(name, encoding)| {
            !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                || name.is_empty()
                || name == "."
                || name == ".."
                || matches!(name.as_str(), "manifest.json" | "program" | "request.json")
                || name.starts_with("input-")
                || name.len() > 128
                || !matches!(encoding.as_str(), "utf-8" | "bytes")
        })
    {
        return Err("invalid bounded Cocoon job specification".into());
    }
    Ok(spec)
}
