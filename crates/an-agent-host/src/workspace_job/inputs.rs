//! Pinned artifact references stay on the host; workloads receive selected copies.
use super::profile::ArtifactInput;
use crate::{
    artifact_versions,
    execution::{Result, store},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};

pub(super) fn bind(
    requests: &[ArtifactInput],
    inputs: &[Value],
    roots: &[std::path::PathBuf],
    work: &Path,
    reader: Option<NativeInput<'_>>,
) -> Result<Vec<Value>> {
    if requests.len() > 16 {
        return Err("too many artifact inputs".into());
    }
    let mut paths = BTreeSet::new();
    let mut result = Vec::new();
    for request in requests {
        store::no_links(&request.path)?;
        let path = request.path.canonicalize()?;
        if !paths.insert(path.clone()) {
            return Err("duplicate artifact input".into());
        }
        let selected = inputs
            .iter()
            .find(|i| i["path"] == json!(path))
            .ok_or("artifact input must be a selected read-only file")?;
        let catalog = request.catalog.canonicalize()?;
        if catalog.starts_with(work)
            || roots
                .iter()
                .any(|r| catalog.starts_with(r) || r.starts_with(&catalog))
        {
            return Err(
                "artifact catalog must remain outside workload read and write roots".into(),
            );
        }
        let binding = if let Some(reader) = reader.filter(|r| r.matches(&path)) {
            if reader.data["catalog"] != json!(catalog)
                || reader.data["binding"]["revision"] != request.revision
            {
                return Err("registered input selector mismatch".into());
            }
            reader.data["binding"].clone()
        } else {
            artifact_versions::admission::read_input(&catalog, &request.revision)?.1
        };
        if selected["sha256"] != binding["content_sha256"] {
            return Err("selected file does not match pinned artifact revision".into());
        }
        result.push(json!({"path":path,"catalog":catalog,"binding":binding}));
    }
    Ok(result)
}

/// Only native admission constructs this handle. Workspace profiles cannot deserialize it.
#[derive(Clone, Copy)]
pub(crate) struct NativeInput<'a> {
    data: &'a Value,
    reference: Option<&'a Value>,
}

impl<'a> NativeInput<'a> {
    pub(crate) fn preview(data: &'a Value) -> Self {
        Self {
            data,
            reference: None,
        }
    }

    pub(crate) fn admitted(data: &'a Value, claims: &'a [Value]) -> Result<Self> {
        let claim = claims
            .first()
            .ok_or("missing registered business input claim")?;
        if claim["catalog"] != data["catalog"]
            || claim["binding"] != data["binding"]
            || claim["slot"] != 0
        {
            return Err("registered business input claim mismatch".into());
        }
        Ok(Self {
            data,
            reference: Some(claim),
        })
    }

    pub(super) fn matches(self, path: &Path) -> bool {
        self.data["path"]
            .as_str()
            .is_some_and(|p| Path::new(p) == path)
    }

    pub(super) fn identity(self) -> Result<(usize, String)> {
        let binding = &self.data["binding"];
        let catalog = Path::new(self.data["catalog"].as_str().ok_or("registered catalog")?);
        if let Some(claim) = self.reference {
            artifact_versions::registered::check_pin(catalog, claim)?;
        } else {
            artifact_versions::registered::admission_open(catalog, binding)?;
        }
        Ok((
            usize::try_from(binding["bytes"].as_u64().ok_or("registered length")?)?,
            binding["content_sha256"]
                .as_str()
                .ok_or("registered content hash")?
                .into(),
        ))
    }

    pub(super) fn snapshot(self, item: &Value) -> Result<Vec<u8>> {
        if item["path"] != self.data["path"]
            || item["catalog"] != self.data["catalog"]
            || item["binding"] != self.data["binding"]
        {
            return Err("registered snapshot selection differs from admission".into());
        }
        artifact_versions::registered::snapshot(
            self.reference
                .ok_or("preview cannot read registered input")?,
        )
    }
}
