//! Opt-in CLI timing diagnostics; never authority, domain data or a second tape.
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

pub(crate) struct Span {
    start: Option<Instant>,
    stage: &'static str,
    path: PathBuf,
    bytes: usize,
}
impl Span {
    #[allow(
        clippy::disallowed_methods,
        reason = "opt-in host performance observation"
    )]
    pub fn new(stage: &'static str, path: &Path, bytes: usize) -> Self {
        let enabled = std::env::var_os("AN_AGENT_HOST_TIMING").is_some_and(|v| v == "1");
        Self {
            start: enabled.then(Instant::now),
            stage,
            path: if enabled { path.into() } else { PathBuf::new() },
            bytes,
        }
    }
    pub fn finish(self, outcome: &str) {
        if let Some(start) = self.start {
            let _ = writeln!(
                std::io::stderr().lock(),
                "AN_AGENT_TIMING={}",
                serde_json::json!({"schema":"an-host-timing-v1",
                "pid":std::process::id(),"stage":self.stage,"path":self.path,"bytes":self.bytes,
                "elapsed_us":start.elapsed().as_micros(),"outcome":outcome})
            );
        }
    }
}
