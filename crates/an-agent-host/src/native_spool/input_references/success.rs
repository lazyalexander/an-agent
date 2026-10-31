//! Success is insufficient: state stays pinned and delivery must be confirmed.
use super::*;

pub(super) fn evidence(
    root: &Path,
    name: &str,
    result: &Value,
    workspace: &Path,
    subscription: &str,
) -> Result<Value> {
    if result["status"] != "succeeded" || result["workspace"]["termination_confirmed"] != true {
        return Err("successful input release requires confirmed whole-attempt success".into());
    }
    let start = read(&root.join("runs").join(name).join("start.json"))?;
    state_free(root, &start)?;
    let completion =
        events::confirmed_input_consumption(workspace, subscription, root, name, &start)?;
    Ok(
        json!({"workspace":location(workspace)?,"subscription":subscription,
        "receipt":completion,"state_policy":"stateless-only; opaque state dependencies remain retained"}),
    )
}

fn state_free(root: &Path, start: &Value) -> Result<()> {
    for name in ["checkpoints", "checkpoints.pending"] {
        let path = root.join(name);
        store::no_links(&path)?;
        if path.try_exists()? {
            return Err(
                "successful stateful inputs stay retained pending explicit checkpoint dependencies"
                    .into(),
            );
        }
    }
    if start.get("state_input").is_some()
        || start.get("data_input").is_some()
        || start["workspace"]["artifact_inputs"]
            .as_array()
            .is_none_or(|v| v.len() != 1)
    {
        return Err("successful input release requires one stateless managed event input".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use an_agent_core::testkit::TempDir;

    #[test]
    fn state_and_partial_initialization_cannot_be_treated_as_absent() -> Result<()> {
        let tmp = TempDir::new("success-input-state");
        let root = fs::canonicalize(tmp.path())?;
        let mut start = json!({"workspace":{"artifact_inputs":[{}]}});
        state_free(&root, &start)?;
        start["state_input"] = Value::Null;
        assert!(state_free(&root, &start).is_err());
        start.as_object_mut().unwrap().remove("state_input");
        for name in ["checkpoints.pending", "checkpoints"] {
            fs::create_dir(root.join(name))?;
            assert!(state_free(&root, &start).is_err());
            fs::remove_dir(root.join(name))?;
        }
        start["workspace"]["artifact_inputs"] = json!([]);
        assert!(state_free(&root, &start).is_err());
        Ok(())
    }
}
