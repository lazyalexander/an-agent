//! Static routing requests bind exact instances; routing never grants execution.
use super::*;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    source: PathBuf,
    output: String,
    target: PathBuf,
    result: String,
    wall_seconds: u64,
}
pub fn subscribe(path: &Path, name: &str, request: &Path, approved: Option<&str>) -> Result<Value> {
    native::bundle::identifier(name)?;
    let request: Request = serde_json::from_slice(&regular(request, 65536)?)?;
    if request.schema != "an-native-artifact-route-v1" || !(1..=300).contains(&request.wall_seconds)
    {
        return Err("unsupported native artifact route".into());
    }
    let source = native::location(&request.source)?;
    let target = native::location(&request.target)?;
    separate(&source, &target)?;
    let space = Space::open(path)?;
    separate(&space.root, &source)?;
    separate(&space.root, &target)?;
    let _target = store::lock(&target)?;
    let (instance, target_bundle) = native::active(&target)?;
    let (_, source_bundle) = native::state(&source)?;
    native::event_contract::permits(&source_bundle.manifest, false)?;
    native::event_contract::permits(&target_bundle.manifest, true)?;
    native::event_contract::permits(&target_bundle.manifest, false)?;
    if !source_bundle.manifest.outputs.contains(&request.output)
        || !target_bundle.manifest.outputs.contains(&request.result)
    {
        return Err("route must select declared source and result outputs".into());
    }
    let route = json!({"schema":"an-native-event-subscription-v1","workspace":space.id,"name":name,
        "source":identity(&source)?,"target":identity(&target)?,"output":request.output,"result":request.result,
        "kind":native::event_contract::ARTIFACT,"wall_seconds":request.wall_seconds,
        "target_generation":native::round_control::current(&target,&instance)?["generation"]});
    let _lock = store::lock(&space.root)?;
    let plan = json!({"operation":"subscribe","route":route,"execution_granted":false});
    let (digest, execute) = native::approval(&plan, approved)?;
    let existing =
        space.facts.log()?.into_iter().any(|r| {
            r.kind == "config" && r.name.as_deref() == Some(&format!("native.events.{name}"))
        });
    if existing {
        if space.route(name)? != route {
            return Err("static subscription name already used".into());
        }
        return Ok(
            json!({"status":"succeeded","subscription":hash(&route)?,"executed":false,"already_recorded":true}),
        );
    }
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    if space.facts.log()?.len() >= 512 {
        return Err("native event workspace is full".into());
    }
    if space
        .facts
        .log()?
        .iter()
        .filter(|r| r.kind == "config")
        .count()
        >= 16
    {
        return Err("static subscription limit reached".into());
    }
    space.facts.put_config(
        &format!("native.events.{name}"),
        &serde_json::to_vec(&route)?,
    )?;
    Ok(
        json!({"status":"succeeded","subscription":hash(&route)?,"executed":true,"execution_granted":false}),
    )
}
pub fn revoke(path: &Path, name: &str, approved: Option<&str>) -> Result<Value> {
    let space = Space::open(path)?;
    let _lock = store::lock(&space.root)?;
    let route = space.route(name)?;
    let plan = json!({"operation":"revoke","workspace":space.id,"subscription":hash(&route)?,
        "running_execution_cancelled":false});
    let target = space
        .root
        .join("revocations")
        .join(format!("{}.json", hash(&route)?));
    let (digest, execute) = native::approval(&plan, approved)?;
    if target.exists() {
        let recorded: Value = serde_json::from_slice(&regular(&target, 65536)?)?;
        if recorded != plan {
            return Err("revocation evidence changed".into());
        }
        return Ok(json!({"status":"succeeded","executed":false,"already_revoked":true}));
    }
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    write_json(&target, &plan)?;
    Ok(json!({"status":"succeeded","executed":true,"running_execution_cancelled":false}))
}
