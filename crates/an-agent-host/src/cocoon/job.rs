//! Fixed operator programs with bounded proposal data, independent of model protocols.
use super::*;
use std::collections::BTreeMap;

mod lifecycle;
mod spec;
pub(crate) use lifecycle::release;
pub(crate) use spec::{Input, load};

#[derive(Debug)]
struct Unknown;
impl std::fmt::Display for Unknown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cocoon job outcome unknown; inspect original operation")
    }
}
impl std::error::Error for Unknown {}

fn step(root: &Path, profile: &Profile, id: &str, name: &str, operation: Value) -> Result<Value> {
    let key = store::digest(format!("{id}:{name}").as_bytes())[..32].to_string();
    let mut observed = submit(&root.join("operations"), profile, &key, operation)?;
    if observed["state"] == "unknown"
        && let Ok(recovered) = reconcile(&root.join("operations"), profile, &key)
    {
        observed = recovered;
    }
    if observed["state"] != "observed" {
        return Err(Unknown.into());
    }
    Ok(observed["result"].clone())
}

fn transfer(
    root: &Path,
    profile: &Profile,
    id: &str,
    name: &str,
    plan: Value,
    task: &str,
    active: &impl Fn() -> Result<()>,
) -> Result<(String, Value)> {
    let hash = store::digest(format!("{id}:{name}").as_bytes());
    let key = uuid::Uuid::parse_str(&hash[..32])?.to_string();
    let result = super::transfer::run(
        &root.join("cocoon-transfers"),
        profile,
        &key,
        plan,
        task,
        active,
    )?;
    if result["state"] != "staged" {
        return Err(Unknown.into());
    }
    Ok((key, result))
}

#[allow(
    clippy::too_many_arguments,
    reason = "explicit task, program and data bindings"
)]
pub(crate) fn run(
    session: &Path,
    spec_path: &Path,
    spec_sha256: &str,
    id: &str,
    task: &str,
    data: Value,
    inputs: Vec<Input>,
    active: impl Fn() -> Result<()>,
) -> Result<Value> {
    if uuid::Uuid::parse_str(id)?.to_string() != id
        || inputs.len() > 4
        || serde_json::to_vec(&data)?.len() > 65536
        || inputs.iter().map(|i| i.bytes.len()).sum::<usize>() > 4 * 1024 * 1024
    {
        return Err("Cocoon job input limit".into());
    }
    active()?;
    let spec = load(spec_path, spec_sha256)?;
    let profile = super::load(&spec.transport, &spec.transport_sha256)?;
    let program = store::read(&spec.program, 262144)?;
    if store::digest(&program) != spec.program_sha256 {
        return Err("Cocoon job program changed".into());
    }
    let context = exchange(
        &profile,
        &an_agent_core::det_seam::Entropy::os().uuid_v4().to_string(),
        json!({"operation":"context","vm":profile.vm}),
    )?;
    let attempt = context["attempt"]
        .as_str()
        .ok_or("Cocoon job needs an active attempt")?;
    uuid::Uuid::parse_str(attempt)?;
    if context["vm"] != profile.vm
        || context["generation"] != profile.generation
        || context["state"] != "running"
        || context["work"]["guest_ready"] != true
    {
        return Err("Cocoon job environment is not ready".into());
    }
    for action in ["inspect", "guest", "stop"] {
        if !context["work"]["actions"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == action))
        {
            return Err("Cocoon job requires inspect, guest and stop authority".into());
        }
    }
    lifecycle::claim(session, task, &profile, attempt, &spec)?;
    store::directory(&session.join("cocoon-jobs"))?;
    let root = session.join("cocoon-jobs").join(id);
    store::directory(&root)?;
    let approved_spec = store::read(spec_path, 32768)?;
    if store::digest(&approved_spec) != spec_sha256 {
        return Err("Cocoon job specification changed before submission".into());
    }
    let retained_spec = root.join("spec.json");
    if retained_spec.exists() {
        if store::digest(&store::read(&retained_spec, 32768)?) != spec_sha256 {
            return Err("retained Cocoon specification changed".into());
        }
    } else {
        store::write_new(&retained_spec, &approved_spec)?;
    }
    let binding = json!({"program_sha256":spec.program_sha256,"spec":spec_path,"spec_sha256":spec_sha256,"task":task,
        "request_sha256":store::digest(&serde_json::to_vec(&data)?),"vm":profile.vm,"generation":profile.generation,"attempt":attempt,
        "inputs":inputs.iter().map(|i|json!({"sha256":store::digest(&i.bytes),"bytes":i.bytes.len(),"source":i.evidence})).collect::<Vec<_>>()});
    let intent = root.join("intent.json");
    if intent.exists() {
        if serde_json::from_slice::<Value>(&store::read(&intent, 32768)?)? != binding {
            return Err("Cocoon job ID already bound".into());
        }
    } else {
        persist(&intent, &binding)?;
    }
    let finished = root.join("result.json");
    if finished.exists() {
        return serde_json::from_slice(&store::read(&finished, 262144)?).map_err(Into::into);
    }
    let guest = format!("/workspace/an-{id}");
    let identity = json!({"vm":profile.vm,"generation":profile.generation,"attempt":attempt,"caller":profile.caller,"task":task,"request":0});
    let call = |name: &str, operation: Value| -> Result<Value> {
        active()?;
        let result = step(
            &root,
            &profile,
            id,
            name,
            json!({"operation":"guest","request":{"identity":identity,"operation":operation}}),
        )?;
        if result["identity"] != identity {
            return Err("Cocoon job response identity mismatch".into());
        }
        result["result"]
            .get("Ok")
            .cloned()
            .ok_or_else(|| "Cocoon guest rejected job step".into())
    };
    let work = (|| -> Result<Value> {
        let mkdir = call(
            "mkdir",
            json!({"kind":"exec","argv":["/bin/mkdir","-p",guest],"cwd":"/workspace"}),
        )?;
        if mkdir["kind"] != "exited" || mkdir["code"] != 0 || mkdir["timed_out"] != false {
            return Err("Cocoon job directory creation failed".into());
        }
        let input_names = (0..inputs.len())
            .map(|i| format!("{guest}/input-{i}"))
            .collect::<Vec<_>>();
        let payload = serde_json::to_vec(&json!({"data":data,"inputs":input_names}))?;
        let mut uploads = vec![
            ("program".to_owned(), program),
            ("request.json".to_owned(), payload),
        ];
        uploads.extend(
            inputs
                .into_iter()
                .enumerate()
                .map(|(i, v)| (format!("input-{i}"), v.bytes)),
        );
        for (name, bytes) in uploads {
            active()?;
            let local = root.join(&name);
            if local.exists() {
                if store::read(&local, 4 * 1024 * 1024)? != bytes {
                    return Err("Cocoon job staged input changed".into());
                }
            } else {
                store::write_new(&local, &bytes)?;
            }
            transfer(
                session,
                &profile,
                id,
                &name,
                json!({"direction":"upload","path":format!("{guest}/{name}"),"attempt":attempt,
                "sha256":store::digest(&bytes),"bytes":bytes.len(),"source":local}),
                task,
                &active,
            )?;
        }
        let mut argv = vec![spec.interpreter.clone()];
        argv.extend(spec.arguments.iter().map(|a| {
            a.replace("{program}", &format!("{guest}/program"))
                .replace("{request}", &format!("{guest}/request.json"))
                .replace("{output}", &guest)
        }));
        let executed = call("execute", json!({"kind":"exec","argv":argv,"cwd":guest}))?;
        if executed["kind"] != "exited" || executed["code"] != 0 || executed["timed_out"] != false {
            return Err("Cocoon domain program failed or timed out".into());
        }
        let manifest = call(
            "manifest",
            json!({"kind":"read_chunk","path":format!("{guest}/manifest.json"),"offset":0,"max_bytes":8192}),
        )?;
        if manifest["kind"] != "chunk" || manifest["eof"] != true {
            return Err("Cocoon output manifest exceeds bound".into());
        }
        let hex = manifest["content_hex"]
            .as_str()
            .ok_or("Cocoon output manifest missing")?;
        let bytes = super::transfer::decode(hex)?;
        let files: BTreeMap<String, Value> = serde_json::from_slice(&bytes)?;
        if files.len() != spec.outputs.len() || files.keys().ne(spec.outputs.keys()) {
            return Err("Cocoon output declaration mismatch".into());
        }
        let mut outputs = serde_json::Map::new();
        for (name, declared) in &files {
            active()?;
            let (transfer, result) = transfer(
                session,
                &profile,
                id,
                &format!("output:{name}"),
                json!({"direction":"download","path":format!("{guest}/{name}"),
                "attempt":attempt,"sha256":declared["sha256"],"bytes":declared["bytes"]}),
                task,
                &active,
            )?;
            let (path, _) = super::transfer::candidate(
                &session.join("cocoon-transfers"),
                &transfer,
                task,
                result["sha256"].as_str().ok_or("output digest")?,
            )?;
            let mut output = json!({"reference":{"session":session.file_name().and_then(|v|v.to_str()),"operation":id,"output":name},"transfer":transfer,"sha256":result["sha256"],"bytes":result["bytes"],"artifact_accepted":false});
            if spec.outputs[name] == "utf-8" {
                output["text"] = json!(String::from_utf8(store::read(&path, 65536)?)?);
            }
            outputs.insert(name.clone(), output);
        }
        active()?;
        if serde_json::to_vec(&outputs)?.len() > 131072 {
            return Err("Cocoon text disclosure limit".into());
        }
        Ok(
            json!({"operation":id,"state":"proposed","outputs":outputs,"artifact_accepted":false,"execution":executed}),
        )
    })();
    match work {
        Ok(value) => {
            persist(&finished, &value)?;
            Ok(value)
        }
        Err(e) if e.downcast_ref::<Unknown>().is_some() => {
            Ok(json!({"operation":id,"state":"unknown","artifact_accepted":false}))
        }
        Err(e) => Err(e),
    }
}

pub(crate) fn output(session: &Path, id: &str, name: &str, task: &str) -> Result<(PathBuf, Value)> {
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        return Err("invalid Cocoon job reference".into());
    }
    let root = session.join("cocoon-jobs").join(id);
    let intent: Value = serde_json::from_slice(&store::read(&root.join("intent.json"), 32768)?)?;
    let result: Value = serde_json::from_slice(&store::read(&root.join("result.json"), 262144)?)?;
    if intent["task"] != task
        || result["operation"] != id
        || result["state"] != "proposed"
        || result["artifact_accepted"] != false
    {
        return Err("Cocoon job output is not available".into());
    }
    let output = result["outputs"]
        .get(name)
        .ok_or("undeclared Cocoon job output")?;
    let (path, mut evidence) = super::transfer::candidate(
        &session.join("cocoon-transfers"),
        output["transfer"].as_str().ok_or("output transfer")?,
        task,
        output["sha256"].as_str().ok_or("output digest")?,
    )?;
    evidence["job"] = json!(id);
    evidence["output"] = json!(name);
    evidence["spec_sha256"] = intent["spec_sha256"].clone();
    Ok((path, evidence))
}

#[cfg(test)]
mod tests;

/// Retained observation never opens a transport or restarts a program.
pub(crate) fn status(session: &Path, id: &str) -> Result<Value> {
    if uuid::Uuid::parse_str(id)?.to_string() != id {
        return Err("invalid Cocoon job identity".into());
    }
    let root = session.join("cocoon-jobs").join(id);
    store::read(&root.join("intent.json"), 32768)?;
    let result = root.join("result.json");
    if result.exists() {
        serde_json::from_slice(&store::read(&result, 262144)?).map_err(Into::into)
    } else {
        Ok(
            json!({"operation":id,"state":"running-or-unknown","automatic_retries":0,"artifact_accepted":false}),
        )
    }
}

pub(crate) fn summaries(session: &Path) -> Result<Value> {
    let directory = session.join("cocoon-jobs");
    if !directory.exists() {
        return Ok(json!([]));
    }
    let mut results = Vec::new();
    for (count, entry) in fs::read_dir(directory)?.enumerate() {
        if count >= 64 {
            return Err("Cocoon job receipt limit".into());
        }
        let path = entry?.path();
        if !path.join("intent.json").exists() {
            continue;
        }
        let id = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("Cocoon job identity")?;
        let value = status(session, id)?;
        let mut outputs = value["outputs"].clone();
        if let Some(values) = outputs.as_object_mut() {
            for output in values.values_mut() {
                if let Some(object) = output.as_object_mut() {
                    object.remove("text");
                }
            }
        }
        results.push(json!({"operation":id,"state":value["state"],"outputs":outputs,"artifact_accepted":false}));
    }
    results.sort_by(|a, b| a["operation"].as_str().cmp(&b["operation"].as_str()));
    Ok(json!(results))
}
