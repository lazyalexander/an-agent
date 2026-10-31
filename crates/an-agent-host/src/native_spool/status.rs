//! Evidence-derived observation, never permission to execute or reclaim.
use super::*;

pub fn status(path: &Path, name: &str) -> Result<Value> {
    bundle::identifier(name)?;
    let root = location(path)?;
    let (instance, _) = state(&root)?;
    let dir = root.join("runs").join(name);
    store::no_links(&dir)?;
    if !dir.is_dir() {
        return Err("unknown spool run".into());
    }
    let mut value = json!({"schema":"an-native-spool-run-status-v1","instance":instance["instance"],
        "release":instance["release"],"run":name,"phase":"prepared",
        "termination_confirmed":false,"execution_status":Value::Null,
        "artifact_status":"not-accepted","outputs":[],"authority":"observation only; custody commands revalidate"});
    if !dir.join("job").exists() {
        if dir.join("start.json").exists() || dir.join("result.json").exists() {
            value["phase"] = json!("unconfirmed");
        }
        return Ok(value);
    }
    // No instance lock: execution holds it. Missing or racing evidence stays uncertain.
    let settled = match run::settled(&root, name) {
        Ok(record) => record,
        Err(error) => {
            value["phase"] = json!("running-or-unconfirmed");
            value["diagnostic"] = json!(error.to_string());
            return Ok(value);
        }
    };
    value["termination_confirmed"] = json!(true);
    value["execution_status"] = settled["status"].clone();
    value["phase"] = json!("settled");
    if settled["status"] == "succeeded" {
        value["artifact_status"] = json!("awaiting-acceptance");
        if !dir.join("accepted.json").exists()
            && dir
                .join("job/attempts")
                .join(settled["workspace"]["attempt"].as_str().ok_or("attempt")?)
                .join("accepted.json")
                .exists()
        {
            value["artifact_status"] = json!("unconfirmed");
            value["diagnostic"] = json!(
                "workspace acceptance exists without instance-bound acceptance; reconcile before further custody"
            );
        }
        if dir.join("accepted.json").exists() {
            match artifacts::verified(&root, name, true) {
                Ok((artifacts, _)) => {
                    value["artifact_status"] = json!("accepted");
                    value["outputs"] = artifacts["outputs"].clone();
                    value["catalog"] = artifacts["catalog"].clone();
                    value["phase"] = json!("accepted");
                }
                Err(error) => {
                    value["artifact_status"] = json!("unconfirmed");
                    value["diagnostic"] = json!(error.to_string());
                }
            }
        }
    }
    if dir.join("job/reclaimed.json").exists() {
        value["phase"] = json!("reclaimed");
    }
    Ok(value)
}
