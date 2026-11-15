//! A restored disk never grants access to prior products. Bind explicitly selected host custody.
use super::*;

pub(super) fn context(service: &Path, request: &Value, image: &Value) -> Result<Value> {
    let Some(id) = request["resume_task"].as_str() else {
        return Ok(Value::Null);
    };
    let root = directory(service, id)?;
    let prior: Value = serde_json::from_slice(&store::read(&root.join("plan.json"), 131072)?)?;
    let setup: Value = serde_json::from_slice(&store::read(&root.join("setup.json"), 65536)?)?;
    let source = view(service, id)?;
    if prior["environment_digest"] != request["environment"]
        || image["source_vm"] != setup["vm"]
        || source["state"]["state"] != "succeeded"
        || source["state"]["termination_confirmed"] != true
    {
        return Err("project image and accepted source task do not match".into());
    }
    let mut products = serde_json::Map::new();
    for stage in ["intermediate", "final"] {
        let accepted = artifact(service, id, stage)?;
        products.insert(
            stage.into(),
            json!({"reference":source["state"]["products"][stage],
            "revision":accepted["revision"],"content_sha256":accepted["content_sha256"]}),
        );
    }
    Ok(json!({"source_task":id,"products":products}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restored_bytes_cannot_authorize_unaccepted_or_wrong_source_products() -> Result<()> {
        let (_temporary, service, id, root) = super::super::tests::fixture()?;
        let environment = "a".repeat(64);
        store::write_new(
            &root.join("plan.json"),
            &serde_json::to_vec(&json!({"environment_digest":environment}))?,
        )?;
        store::write_new(&root.join("setup.json"), br#"{"vm":"source"}"#)?;
        store::write_new(&root.join("state.json"), br#"{"state":"running"}"#)?;
        let request = json!({"resume_task":id,"environment":environment});
        assert!(context(&service, &request, &json!({"source_vm":"other"})).is_err());
        assert!(context(&service, &request, &json!({"source_vm":"source"})).is_err());
        assert_eq!(context(&service, &json!({}), &Value::Null)?, Value::Null);
        Ok(())
    }
}
