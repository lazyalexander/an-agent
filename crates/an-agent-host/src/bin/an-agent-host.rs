//! Explicit operator entry point; package tooling is not a runtime dependency.
use an_agent_host::execution::{self, Result};
use std::{path::Path, process::ExitCode};

fn supervised_run(
    store: &str,
    release: &str,
    input: &str,
    execute: bool,
) -> Result<serde_json::Value> {
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    #[cfg(unix)]
    let signals = [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM]
        .map(|signal| signal_hook::flag::register(signal, std::sync::Arc::clone(&cancelled)))
        .into_iter()
        .collect::<std::io::Result<Vec<_>>>()?;
    let result = execution::run_cancellable(
        Path::new(store),
        release,
        Path::new(input),
        execute,
        &cancelled,
    );
    #[cfg(unix)]
    for id in signals {
        signal_hook::low_level::unregister(id);
    }
    result
}

fn run(args: Vec<String>) -> Result<()> {
    let result = match args.as_slice() {
        [cmd, profile, packet, output] if cmd == "model" => an_agent_host::model_run::run(Path::new(profile), Path::new(packet), Path::new(output), None)?,
        [cmd, profile, packet, output, flag, digest] if cmd == "model" && flag == "--approve" => an_agent_host::model_run::run(Path::new(profile), Path::new(packet), Path::new(output), Some(digest))?,
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
        _ => return Err("usage: an-agent-host check DEPLOYMENT | register DEPLOYMENT STORE | run STORE RELEASE INPUT [--execute] | context-check THREAD PACKET | model PROFILE PACKET NEW_OUTPUT [--approve PLAN_SHA256]".into()),
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
