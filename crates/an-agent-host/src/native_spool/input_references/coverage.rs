//! An explicit domain attestation, checked against the original attempt under its instance lock.
use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Coverage {
    schema: String,
    checkpoint: String,
    declaration_sha256: String,
    inputs: Vec<Decision>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    catalog: PathBuf,
    revision: String,
    disposition: String,
}

// The qualified profile restricts direct dependencies to the two managed input roles.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema: String,
    state_output: String,
    data: String,
    parent_state: String,
}
fn committed_policy(accepted: &Value, state_output: &Value) -> Result<(Policy, Value)> {
    let output = accepted["outputs"]
        .as_array()
        .ok_or("accepted outputs")?
        .iter()
        .find(|o| o["path"] == "checkpoint-input-policy.json")
        .ok_or("release requires a dependency policy accepted with this state")?;
    let revision = output["binding"]["revision"]
        .as_str()
        .ok_or("policy revision")?;
    let catalog = Path::new(accepted["catalog"].as_str().ok_or("policy catalog")?);
    let (bytes, binding) = crate::artifact_versions::read(catalog, revision)?;
    let policy: Policy = serde_json::from_slice(&bytes)?;
    if policy.schema != "an-native-checkpoint-managed-input-policy-v1"
        || *state_output != policy.state_output
        || output["path"] == *state_output
        || binding.get("encoding").is_some()
        || bytes.len() > 65536
    {
        return Err("invalid committed checkpoint dependency policy".into());
    }
    Ok((policy, output.clone()))
}

pub(super) fn verify(
    root: &Path,
    name: &str,
    start: &Value,
    result: &Value,
    accepted: &Value,
    file: &Path,
) -> Result<Value> {
    let coverage: Coverage = serde_json::from_slice(&regular(file, 65536)?)?;
    if coverage.schema != "an-checkpoint-exhaustive-input-coverage-v1" {
        return Err("explicit exhaustive checkpoint dependency attestation required".into());
    }
    let checkpoint = checkpoint::selection(root, &coverage.checkpoint)?;
    let binding = &checkpoint["binding"];
    let source = &binding["source"];
    let parent = start["state_input"]["binding"]["revision"]
        .as_str()
        .ok_or("exact input state required")?;
    if binding["parent"] != parent
        || source["kind"] != "native-spool-state-commit"
        || source["run"] != name
        || source["attempt"] != result["workspace"]["attempt"]
        || source["instance"] != result["instance"]
        || source["release"] != result["release"]
    {
        return Err("checkpoint does not commit this attempt and parent".into());
    }
    let commit_hash = source["approval_sha256"]
        .as_str()
        .filter(|h| store::valid_digest(h))
        .ok_or("commit approval")?;
    let commit = read(
        &root
            .join("checkpoints")
            .join(format!("commit-{commit_hash}.json")),
    )?;
    if digest(&commit)? != commit_hash
        || commit["schema"] != "an-native-spool-state-commit-v1"
        || commit["artifact"] != source["artifact"]
        || commit["expected"] != parent
        || commit["run"] != name
        || commit["attempt"] != source["attempt"]
    {
        return Err("checkpoint commit evidence mismatch".into());
    }
    let output = accepted["outputs"]
        .as_array()
        .ok_or("accepted outputs")?
        .iter()
        .find(|o| o["path"] == checkpoint["output"])
        .ok_or("accepted checkpoint output")?;
    if commit["instance"] != source["instance"]
        || commit["release"] != source["release"]
        || commit["output"] != checkpoint["output"]
        || source["output"] != checkpoint["output"]
        || output["binding"]["revision"] != commit["artifact"]["revision"]
        || output["binding"]["content_sha256"] != binding["content_sha256"]
    {
        return Err("acknowledged output does not match committed checkpoint".into());
    }
    let dependencies = checkpoint::registered_dependencies(root, &coverage.checkpoint)?;
    if dependencies["status"] != "registered"
        || dependencies["approval_sha256"] != coverage.declaration_sha256
    {
        return Err("coverage must bind a confirmed checkpoint declaration".into());
    }
    let pins = dependencies["claims"].as_array().ok_or("checkpoint pins")?;
    let inputs = start["workspace"]["artifact_inputs"]
        .as_array()
        .ok_or("managed inputs")?;
    if inputs.len() != 2 || inputs.len() != coverage.inputs.len() {
        return Err("coverage must classify both original data/state inputs".into());
    }
    if inputs[0]["binding"] != start["data_input"]["binding"]
        || inputs[0]["catalog"] != start["data_input"]["catalog"]
        || inputs[1]["binding"] != start["state_input"]["binding"]
        || inputs[1]["catalog"] != start["state_input"]["catalog"]
        || pins.iter().any(|pin| {
            !inputs.iter().any(|input| {
                pin["reference"]["catalog"] == input["catalog"]
                    && pin["reference"]["binding"] == input["binding"]
            })
        })
    {
        return Err("checkpoint policy scope must equal the original managed input roles".into());
    }
    let (policy, policy_output) = committed_policy(accepted, &checkpoint["output"])?;
    let mut decisions = Vec::new();
    for ((input, decision), declared) in inputs
        .iter()
        .zip(coverage.inputs)
        .zip([policy.data, policy.parent_state])
    {
        if input["catalog"] != json!(decision.catalog)
            || input["binding"]["revision"] != decision.revision
        {
            return Err("coverage input order or version mismatch".into());
        }
        let retained = pins.iter().any(|pin| {
            pin["reference"]["catalog"] == input["catalog"]
                && pin["reference"]["binding"] == input["binding"]
        });
        let expected = if retained {
            "checkpoint-pin"
        } else {
            "not-required"
        };
        if decision.disposition != declared {
            return Err("operator disposition conflicts with accepted spool policy".into());
        }
        if decision.disposition != expected {
            return Err("input disposition conflicts with checkpoint pins".into());
        }
        decisions.push(json!({"catalog":decision.catalog,"revision":decision.revision,"disposition":decision.disposition}));
    }
    Ok(
        json!({"schema":coverage.schema,"checkpoint":coverage.checkpoint,"commit_sha256":commit_hash,
        "declaration_sha256":coverage.declaration_sha256,"policy_output":policy_output,"inputs":decisions}),
    )
}
