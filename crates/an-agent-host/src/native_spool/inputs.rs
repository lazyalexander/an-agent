//! Select business data and opaque committed state independently; never infer HEAD.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    catalog: PathBuf,
    revision: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    data: Artifact,
    state_revision: String,
}

pub(super) struct EventInput<'a> {
    pub artifact: &'a Value,
    pub state_revision: Option<&'a str>,
}
pub(super) enum Input<'a> {
    File(&'a Path),
    State(&'a str),
    Event(EventInput<'a>),
    DataState(&'a Path),
}
pub(super) struct Selection {
    data: Value,
    state: Option<Value>,
}

pub(super) fn artifact(catalog: &Path, revision: &str) -> Result<Value> {
    store::no_links(catalog)?;
    let catalog = catalog.canonicalize()?;
    let (_, binding) = crate::artifact_versions::read(&catalog, revision)?;
    let file = if binding["encoding"] == "bytes" {
        "content.bin"
    } else {
        "content.txt"
    };
    Ok(
        json!({"path":catalog.join("revisions").join(revision).join(file),"catalog":catalog,"binding":binding}),
    )
}
fn binding(value: &Value) -> Value {
    json!({"path":value["path"],"catalog":value["catalog"],"binding":value["binding"]})
}
pub(super) fn combined(bundle: &bundle::Bundle) -> bool {
    bundle
        .manifest
        .requirements
        .as_ref()
        .is_some_and(|r| r.state_input.is_some())
}

pub(super) fn select(root: &Path, bundle: &bundle::Bundle, input: Input<'_>) -> Result<Selection> {
    let (data, revision) = match input {
        Input::File(path) => (json!({"path":path.canonicalize()?}), None),
        Input::State(revision) => (checkpoint::selection(root, revision)?, None),
        Input::Event(event) => (
            event.artifact.clone(),
            event.state_revision.map(str::to_owned),
        ),
        Input::DataState(path) => {
            let request: Request = serde_json::from_slice(&regular(path, 65536)?)?;
            if request.schema != "an-native-data-state-v1" {
                return Err("unsupported data/state selection".into());
            }
            (
                artifact(&request.data.catalog, &request.data.revision)?,
                Some(request.state_revision),
            )
        }
    };
    if combined(bundle) != revision.is_some() {
        return Err(
            "release and explicit data/state selection must agree; no implicit state".into(),
        );
    }
    let state = revision
        .map(|rev| checkpoint::selection(root, &rev))
        .transpose()?;
    Ok(Selection { data, state })
}

impl Selection {
    pub fn configure(&self, bundle: &bundle::Bundle, profile: &mut Value) -> Result<()> {
        let inputs = profile["inputs"].as_array_mut().ok_or("profile inputs")?;
        let index = inputs.len();
        inputs.push(self.data["path"].clone());
        if let Some(state) = &self.state {
            inputs.push(state["path"].clone());
        }
        profile["environment"]["AN_SPOOL_INPUT"] = json!(format!("{{input:{index}}}"));
        if self.state.is_some() {
            profile["environment"]["AN_SPOOL_STATE"] = json!(format!("{{input:{}}}", index + 1));
        }
        let mut bindings = Vec::new();
        if self.data.get("binding").is_some() {
            bindings.push(binding(&self.data));
        }
        if let Some(state) = &self.state {
            bindings.push(binding(state));
        }
        if !bindings.is_empty() {
            profile["artifact_inputs"] = json!(bindings.iter().map(|v| json!({"path":v["path"],"catalog":v["catalog"],"revision":v["binding"]["revision"]})).collect::<Vec<_>>());
        }
        let Some(r) = &bundle.manifest.requirements else {
            return Ok(());
        };
        if r.runtime == runtime::PYTHON || self.state.is_some() {
            let mut layout: Vec<_> = bundle
                .payload
                .keys()
                .map(|p| p.rsplit('/').next().unwrap_or(p).to_owned())
                .collect();
            layout.push(r.inputs[0].clone());
            if self.state.is_some() {
                layout.push(r.state_input.clone().ok_or("state slot")?);
            }
            profile["input_layout"] = json!(layout);
        }
        Ok(())
    }
    pub fn bind_plan(&self, plan: &mut Value) {
        if let Some(event) = self.data.get("event_input") {
            plan["event_input"] = event.clone();
        }
        if let Some(state) = &self.state {
            plan["data_input"] = binding(&self.data);
            plan["state_input"] = state.clone();
        } else if self.data.get("binding").is_some() && self.data.get("event_input").is_none() {
            plan["state_input"] = self.data.clone();
        }
    }
}

/// Exact role inventory for commit verification, including the fixed business input.
pub(super) fn commit_bindings(
    bundle: &bundle::Bundle,
    start: &Value,
    state: &Value,
) -> Result<Value> {
    let mut bindings = Vec::new();
    match (combined(bundle), start.get("data_input")) {
        (true, Some(data)) => {
            let actual = artifact(
                Path::new(data["catalog"].as_str().ok_or("data catalog")?),
                data["binding"]["revision"]
                    .as_str()
                    .ok_or("data revision")?,
            )?;
            if actual != *data {
                return Err("committed business data binding changed".into());
            }
            bindings.push(actual);
        }
        (false, None) => (),
        _ => return Err("state commit input roles do not match the release".into()),
    }
    bindings.push(binding(state));
    Ok(json!(bindings))
}
