//! Explicit operator grant preview or finite MCP stdio capability service.
use an_agent_host::{agent_tools, execution::Result};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [profile] => println!("{}", agent_tools::preview(Path::new(profile))?),
        [profile, flag, approval] if flag == "--approve" => {
            agent_tools::Bridge::open(Path::new(profile), approval)?
                .serve()
                .await?
        }
        _ => return Err("usage: an-agent-tools PROFILE [--approve SHA256]".into()),
    }
    Ok(())
}
