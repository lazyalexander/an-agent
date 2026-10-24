use super::{Path, Result, Value};
use std::io::Read;

pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("context-check", [audience, file]) => {
            let mut bytes = Vec::new();
            std::fs::File::open(file)?
                .take(4097)
                .read_to_end(&mut bytes)?;
            let packet = an_agent_host::context_exchange::ContextPacket::parse(
                &bytes,
                uuid::Uuid::parse_str(audience)?,
            )?;
            Ok(
                serde_json::json!({"schema":packet.schema,"audience":packet.audience,"source":packet.source,"sha256":packet.sha256,"validated":true}),
            )
        }
        ("context-export", [run, stream, audience, budget, output]) => {
            Ok(an_agent_host::artifact_context::export(
                Path::new(run),
                stream,
                uuid::Uuid::parse_str(audience)?,
                budget.parse()?,
                Path::new(output),
            )?)
        }
        _ => Err(format!("invalid arguments for {command}").into()),
    }
}
