//! Product entry points over original host services.
use super::{Path, Result, Value};
use an_agent_host::application::environment;
pub(super) fn dispatch(args: &[String]) -> Result<Value> {
    match args {
  [op,root] if op=="serve"=>super::with_cancellation(|cancel|an_agent_host::application::server::serve(Path::new(root),&cancel)),
  [op,archive] if op=="check-environment"=>environment::preview(Path::new(archive)),
  [op,root,archive,flag,digest] if op=="import-environment" && flag=="--approve"=>environment::import(Path::new(root),Path::new(archive),digest),
  _=>Err("usage: app check-environment ARCHIVE | import-environment PRIVATE_ROOT ARCHIVE --approve DIGEST".into())
 }
}
