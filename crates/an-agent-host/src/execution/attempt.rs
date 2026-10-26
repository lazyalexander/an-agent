//! One admitted attempt: input selection, backend execution and settled evidence.
use super::{Result, backend, publication, sandbox, store};
use an_agent_core::det_seam::Entropy;
use an_agent_spool::spool::Registry;
use serde_json::{Value, json};
use std::{fs, path::Path};
use store::{digest, directory, load, lock, no_links, read, valid_digest, write_new};

pub(super) enum Input<'a> {
    File(&'a Path),
    Revision(&'a Path, &'a str),
}

pub(super) fn run_selected(
    root: &Path,
    release: &str,
    selected: Input<'_>,
    execute: bool,
    cancelled: &std::sync::atomic::AtomicBool,
    backend: &dyn backend::ExecutionBackend,
    emit_plan: bool,
) -> Result<Value> {
    if !valid_digest(release) {
        return Err("release must be a full SHA-256 digest".into());
    }
    no_links(root)?;
    let root = root.canonicalize()?;
    let allocation_lock = lock(&root)?;
    no_links(&root.join("registry"))?;
    let deployment = load(&root.join("releases").join(release))?;
    let spec = deployment.spec;
    let file = spec.config.get("profile").and_then(|v| v.as_str()) == Some("file-kit-v1");
    backend.validate(file)?;
    if spec.sha256 != release {
        return Err("release identity mismatch".into());
    }
    if Registry::open(root.join("registry"))?
        .recover(&spec.name, &spec.version)?
        .sha256
        != release
    {
        return Err("registered descriptor mismatch".into());
    }
    let (input, binding) = match selected {
        Input::File(path) => (read(path, sandbox::INPUT_LIMIT)?, Value::Null),
        Input::Revision(catalog, revision) => crate::artifact_versions::read(catalog, revision)?,
    };
    if binding["encoding"] == "bytes" {
        return Err("text execution profile cannot consume binary artifacts".into());
    }
    if input.len() > sandbox::INPUT_LIMIT {
        return Err("selected revision exceeds execution input limit".into());
    }
    let text = std::str::from_utf8(&input)?;
    let run_id = Entropy::os().uuid_v4();
    let run_dir = root.join("runs").join(run_id.to_string());
    let executable = run_dir.join("program");
    let mut plan = json!({"release":release,"cwd":run_dir,"environment":{},"context":{"kind":"explicit-stdin","sha256":digest(&input),"bytes":input.len(),"trust":"caller-supplied-data"},"limits":{"input_bytes":8192,"output_bytes_per_stream":65536,"wall_seconds":5},"artifact_boundary":run_dir,"execute":execute});
    plan["executable_publication"] = publication::plan();
    if !binding.is_null() {
        plan["input_revision"] = binding.clone();
    }
    backend.plan(&executable, &run_dir, file, &mut plan)?;
    if !execute {
        return Ok(plan);
    }
    // Also show the actual resolved command before any process starts.
    if emit_plan {
        println!("{plan}");
    }
    directory(&root.join("runs"))?;
    fs::create_dir(&run_dir)?;
    // Shared metadata is resolved and the attempt directory is exclusively owned.
    // Do not serialize independent execution, cancellation or settled-run cleanup.
    drop(allocation_lock);
    write_new(
        &run_dir.join("plan.json"),
        &serde_json::to_vec_pretty(&plan)?,
    )?;
    write_new(&run_dir.join("input.txt"), &input)?;
    let admitted = super::admission::admit(&run_dir, &spec, run_id, text)?;
    let operation = backend::Operation {
        run_id: run_id.to_string(),
        operation_id: Entropy::os().uuid_v4().to_string(),
    };
    // Intent is recorded before publication or submission. A missing receipt is
    // an unsettled operation, never implicit permission to retry.
    write_new(
        &run_dir.join("operation.json"),
        &serde_json::to_vec_pretty(&operation)?,
    )?;
    let mut publication = Value::Null;
    let mut evidence = Value::Null;
    let mut observation = Value::Null;
    let mut candidate = backend::Candidate::None;
    let execution = (|| -> Result<sandbox::Execution> {
        let published = publication::publish(&executable, &deployment.program, cancelled)?;
        publication = published.evidence;
        if let Some(stopped) = published.stopped {
            return Ok(stopped);
        }
        let mut result = backend.execute(
            backend::Request {
                operation: &operation,
                executable: &executable,
                directory: &run_dir,
                input: &input,
                cancelled,
            },
            file,
        )?;
        evidence = result.evidence.clone();
        observation = json!({"operation":result.operation,"status":result.execution.status,
            "exit_code":result.execution.code,"environment":result.environment,
            "settled_environment":result.settled_environment,
            "termination_confirmed":result.termination_confirmed});
        if let Err(error) = result.reconcile(&operation) {
            observation["reconciliation_error"] = json!(error.to_string());
            result.execution.status = "refused";
            result
                .execution
                .stderr
                .extend_from_slice(format!("\nresult rejected: {error}").as_bytes());
            result.candidate = backend::Candidate::None;
        }
        candidate = result.candidate;
        Ok(result.execution)
    })();
    if let Err(error) = &execution
        && let Some(error) = error.downcast_ref::<publication::PublicationError>()
    {
        publication = error.evidence.clone();
    }
    let refusal = execution
        .as_ref()
        .err()
        .map(|error| sandbox::refusal_evidence(error.as_ref()));
    let (mut status, code, stdout, mut stderr, pid, reaped) = match execution {
        Ok(result) => (
            result.status,
            result.code,
            result.stdout,
            result.stderr,
            result.pid,
            result.reaped,
        ),
        Err(error) => (
            "refused",
            None,
            vec![],
            error.to_string().into_bytes(),
            None,
            false,
        ),
    };
    let mut artifact = Value::Null;
    if backend.cancel_before_acceptance()
        && status == "succeeded"
        && cancelled.load(std::sync::atomic::Ordering::Relaxed)
    {
        status = "cancelled";
    }
    if file && status == "succeeded" {
        let accepted = if code != Some(0) || !reaped {
            Err("file output requires a reaped successful process".into())
        } else {
            backend::accept(candidate, &run_dir)
        };
        match accepted {
            Ok(bytes) => {
                write_new(&run_dir.join("artifact.txt"), &bytes)?;
                artifact =
                    json!({"path":"artifact.txt","sha256":digest(&bytes),"bytes":bytes.len()});
            }
            Err(error) => {
                status = "failed";
                stderr.extend_from_slice(format!("\nartifact rejected: {error}").as_bytes());
            }
        }
    }
    write_new(&run_dir.join("stdout.txt"), &stdout)?;
    write_new(&run_dir.join("stderr.txt"), &stderr)?;
    let receipt = json!({"schema_version":1,"run_id":run_id,"release":release,"request_event":admitted.request_event,"execution_event":admitted.execution_event,"reply_tape_id":admitted.reply_tape_id(),"status":status,"exit_code":code,"process":{"pid":pid,"reaped":reaped,"scope":"direct-child"},"context":{"path":"input.txt","sha256":digest(&input),"trust":"caller-supplied-data"},"stdout":{"path":"stdout.txt","sha256":digest(&stdout)},"stderr":{"path":"stderr.txt","sha256":digest(&stderr)},"plan_sha256":digest(&serde_json::to_vec_pretty(&plan)?),"retention":"manual; no automatic cleanup","backend":plan["backend"],"container":false});
    let mut receipt = receipt;
    if !binding.is_null() {
        receipt["input_revision"] = binding;
    }
    receipt["publication"] = publication;
    receipt["operation"] = json!({"intent":operation,"observation":observation,
        "cancel_requested":cancelled.load(std::sync::atomic::Ordering::Relaxed),
        "automatic_retries":0});
    backend.decorate_receipt(&mut receipt, evidence, !artifact.is_null());
    if let Some(error) = refusal {
        receipt["error"] = error;
    }
    if file {
        receipt["artifact"] = artifact;
    }
    write_new(
        &run_dir.join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    admitted.finish(status, &receipt)?;
    Ok(
        json!({"status":status,"receipt":run_dir.join("receipt.json"),"stdout":run_dir.join("stdout.txt")}),
    )
}
