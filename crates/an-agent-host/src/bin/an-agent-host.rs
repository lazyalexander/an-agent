//! Explicit operator entry point; package tooling is not a runtime dependency.
use an_agent_host::execution::{self, Result};
use std::{path::Path, process::ExitCode};

fn run(args: Vec<String>) -> Result<()> {
    let result = match args.as_slice() {
        [cmd, audience, file] if cmd == "context-check" => {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(file)?.take(4097).read_to_end(&mut bytes)?;
            let packet = an_agent_host::context_exchange::ContextPacket::parse(&bytes, uuid::Uuid::parse_str(audience)?)?;
            serde_json::json!({"schema": packet.schema, "audience": packet.audience, "source": packet.source, "sha256": packet.sha256, "validated": true})
        }
        [cmd, folder] if cmd == "check" => execution::check(Path::new(folder))?,
        [cmd, folder, store] if cmd == "register" => execution::register(Path::new(folder), Path::new(store))?,
        [cmd, store, release, input] if cmd == "run" => execution::run(Path::new(store), release, Path::new(input), false)?,
        [cmd, store, release, input, flag] if cmd == "run" && flag == "--execute" => execution::run(Path::new(store), release, Path::new(input), true)?,
        _ => return Err("usage: an-agent-host check DEPLOYMENT | register DEPLOYMENT STORE | run STORE RELEASE INPUT [--execute] | context-check THREAD PACKET".into()),
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
