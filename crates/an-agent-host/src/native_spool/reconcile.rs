//! Explicit repair of a missing facade record after complete workspace acceptance.
use super::*;

pub fn reconcile_accept(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    bundle::identifier(name)?;
    let root = location(path)?;
    let _lock = store::lock(&root)?;
    let target = root.join("runs").join(name).join("accepted.json");
    let existing = match fs::symlink_metadata(&target) {
        Ok(_) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    // Reuse exact inventory, settlement, source, revision and content verification.
    let (artifacts, accepted) = artifacts::verified(&root, name, existing)?;
    let bound = json!({"status":"succeeded","instance":artifacts["instance"],"release":artifacts["release"],
        "run":name,"operation":"accept","result":accepted});
    let plan = json!({"schema":"an-native-spool-reconcile-accept-v1","operation":"reconcile-accept",
        "binding":bound,"execution_retried":false,"artifacts_republished":false});
    let (digest, execute) = approval(&plan, approved)?;
    if !execute {
        return Ok(
            json!({"plan":plan,"approval_sha256":digest,"executed":false,"already_recorded":existing}),
        );
    }
    if !existing {
        write_json(&target, &bound)?;
    }
    Ok(
        json!({"status":"succeeded","binding":bound,"already_recorded":existing,
        "execution_retried":false,"artifacts_republished":false}),
    )
}
