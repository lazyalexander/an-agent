//! Explicit operator entry point; package tooling is not a runtime dependency.
use an_agent_host::execution::{self, Result};
use std::{path::Path, process::ExitCode};

#[cfg(unix)]
#[derive(Default)]
struct SignalRegistrations(Vec<signal_hook::SigId>);

#[cfg(unix)]
impl Drop for SignalRegistrations {
    fn drop(&mut self) {
        for id in self.0.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

fn supervised_run(
    store: &str,
    release: &str,
    input: &str,
    execute: bool,
) -> Result<serde_json::Value> {
    with_cancellation(|cancelled| {
        execution::run_cancellable(
            Path::new(store),
            release,
            Path::new(input),
            execute,
            &cancelled,
        )
    })
}

fn with_cancellation(
    run: impl FnOnce(std::sync::Arc<std::sync::atomic::AtomicBool>) -> Result<serde_json::Value>,
) -> Result<serde_json::Value> {
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(unix)]
    let mut signals = SignalRegistrations::default();
    #[cfg(unix)]
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        // Earlier registrations are also released if a later registration fails.
        signals.0.push(signal_hook::flag::register(
            signal,
            std::sync::Arc::clone(&cancelled),
        )?);
    }
    run(cancelled)
}

fn run(args: Vec<String>) -> Result<()> {
    let result = match args.as_slice() {
        [cmd, store, id, release, state] if cmd == "instance-init" => an_agent_host::instance::init(Path::new(store), id, release, Path::new(state), None)?,
        [cmd, store, id, release, state, flag, digest] if cmd == "instance-init" && flag == "--approve" => an_agent_host::instance::init(Path::new(store), id, release, Path::new(state), Some(digest))?,
        [cmd, store, id] if cmd == "instance-show" => an_agent_host::instance::inspect(Path::new(store), id, None)?,
        [cmd, store, id, revision] if cmd == "instance-show" => an_agent_host::instance::inspect(Path::new(store), id, Some(revision))?,
        [cmd, store, id, revision] if cmd == "instance-run" => with_cancellation(|cancelled| an_agent_host::instance::run(Path::new(store), id, revision, false, &cancelled))?,
        [cmd, store, id, revision, flag] if cmd == "instance-run" && flag == "--execute" => with_cancellation(|cancelled| an_agent_host::instance::run(Path::new(store), id, revision, true, &cancelled))?,
        [cmd, store, id, expected, run] if cmd == "instance-commit" => an_agent_host::instance::commit(Path::new(store), id, expected, run, None)?,
        [cmd, store, id, expected, run, flag, digest] if cmd == "instance-commit" && flag == "--approve" => an_agent_host::instance::commit(Path::new(store), id, expected, run, Some(digest))?,
        [cmd, store, run] if cmd == "work-reclaim" => an_agent_host::retention::reclaim(Path::new(store), run, None)?,
        [cmd, store, run, flag, digest] if cmd == "work-reclaim" && flag == "--approve" => an_agent_host::retention::reclaim(Path::new(store), run, Some(digest))?,
        [cmd, store, run, catalog, release] if cmd == "capability-read" => with_cancellation(|cancelled| an_agent_host::capability::artifact_read(Path::new(store), run, Path::new(catalog), release, None, &cancelled))?,
        [cmd, store, run, catalog, release, flag, digest] if cmd == "capability-read" && flag == "--approve" => with_cancellation(|cancelled| an_agent_host::capability::artifact_read(Path::new(store), run, Path::new(catalog), release, Some(digest), &cancelled))?,
        [cmd, catalog, run] if cmd == "artifact-import" => an_agent_host::artifact_versions::import(Path::new(catalog), Path::new(run))?,
        [cmd, catalog, revision, output] if cmd == "artifact-checkout" => an_agent_host::artifact_versions::checkout(Path::new(catalog), revision, Path::new(output))?,
        [cmd, catalog, parent, file, approved] if cmd == "artifact-edit" => an_agent_host::artifact_versions::edit(Path::new(catalog), parent, Path::new(file), approved)?,
        [cmd, store, release, catalog, revision] if cmd == "run-version" => with_cancellation(|cancelled| execution::run_revision(Path::new(store), release, Path::new(catalog), revision, false, &cancelled))?,
        [cmd, store, release, catalog, revision, flag] if cmd == "run-version" && flag == "--execute" => with_cancellation(|cancelled| execution::run_revision(Path::new(store), release, Path::new(catalog), revision, true, &cancelled))?,
        [cmd, profile, store, release, input] if cmd == "run-openshell" => with_cancellation(|cancelled| execution::run_openshell(Path::new(profile), Path::new(store), release, Path::new(input), false, &cancelled))?,
        [cmd, profile, store, release, input, flag] if cmd == "run-openshell" && flag == "--execute" => with_cancellation(|cancelled| execution::run_openshell(Path::new(profile), Path::new(store), release, Path::new(input), true, &cancelled))?,
        [cmd, source, toolchain, output] if cmd == "build-kit" => with_cancellation(|cancelled| an_agent_host::file_job::build(Path::new(source), Path::new(toolchain), Path::new(output), None, &cancelled))?,
        [cmd, source, toolchain, output, flag, digest] if cmd == "build-kit" && flag == "--approve" => with_cancellation(|cancelled| an_agent_host::file_job::build(Path::new(source), Path::new(toolchain), Path::new(output), Some(digest), &cancelled))?,
        [cmd, run, stream, audience, budget, output] if cmd == "context-export" =>
            an_agent_host::artifact_context::export(Path::new(run), stream, uuid::Uuid::parse_str(audience)?, budget.parse()?, Path::new(output))?,
        [cmd, profile, packet, output] if cmd == "model" => with_cancellation(|cancelled| an_agent_host::model_run::run_cancellable(Path::new(profile), Path::new(packet), Path::new(output), None, cancelled))?,
        [cmd, profile, packet, output, flag, digest] if cmd == "model" && flag == "--approve" => with_cancellation(|cancelled| an_agent_host::model_run::run_cancellable(Path::new(profile), Path::new(packet), Path::new(output), Some(digest), cancelled))?,
        [cmd, audience, file] if cmd == "context-check" => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(file)?.take(4097).read_to_end(&mut bytes)?;
            let packet = an_agent_host::context_exchange::ContextPacket::parse(&bytes, uuid::Uuid::parse_str(audience)?)?;
            serde_json::json!({"schema": packet.schema, "audience": packet.audience, "source": packet.source, "sha256": packet.sha256, "validated": true})
        }
        [cmd, folder] if cmd == "check" => execution::check(Path::new(folder))?,
        [cmd, folder, store] if cmd == "register" => execution::register(Path::new(folder), Path::new(store))?,
        [cmd, store, release, input] if cmd == "run" => supervised_run(store, release, input, false)?,
        [cmd, store, release, input, flag] if cmd == "run" && flag == "--execute" => supervised_run(store, release, input, true)?,
        _ => return Err("usage: an-agent-host instance-init STORE INSTANCE RELEASE STATE_FILE [--approve PLAN_SHA256] | instance-show STORE INSTANCE [REVISION] | instance-run STORE INSTANCE REVISION [--execute] | instance-commit STORE INSTANCE EXPECTED_REVISION RUN_ID [--approve PLAN_SHA256] | work-reclaim STORE RUN_ID [--approve PLAN_SHA256] | capability-read STORE REQUEST_RUN_ID CATALOG RELEASE [--approve PLAN_SHA256] | build-kit SOURCE TOOLCHAIN NEW_OUTPUT [--approve PLAN_SHA256] | check DEPLOYMENT | register DEPLOYMENT STORE | run STORE RELEASE INPUT [--execute] | run-openshell PROFILE STORE RELEASE INPUT [--execute] | artifact-import CATALOG RUN | artifact-checkout CATALOG REVISION NEW_FILE | artifact-edit CATALOG PARENT FILE APPROVED_SHA256 | run-version STORE RELEASE CATALOG REVISION [--execute] | context-check THREAD PACKET | context-export RUN artifact|stdout|stderr|model THREAD TEXT_BYTE_BUDGET NEW_OUTPUT | model PROFILE PACKET NEW_OUTPUT [--approve PLAN_SHA256]".into()),
    };
    println!("{}", serde_json::to_string(&result)?);
    if result
        .get("status")
        .and_then(|v| v.as_str())
        .is_some_and(|v| v != "succeeded")
    {
        return Err("execution did not succeed; inspect the retained receipt".into());
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
