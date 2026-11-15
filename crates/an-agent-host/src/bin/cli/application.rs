//! Product entry points over original host services.
use super::{Path, Result, Value};
use an_agent_host::application::environment;
pub(super) fn dispatch(args: &[String]) -> Result<Value> {
    match args {
        [op,root,id] if op=="task-release-setup"=>an_agent_host::application::tasks::setup::release_original(Path::new(root),id),
        [op,root,id] if op=="task-bind-setup"=>an_agent_host::application::tasks::setup::bind(Path::new(root),id),
        [op,root,id,pid] if op=="task-runtime-owner"=>an_agent_host::application::tasks::runtime::record(Path::new(root),id,pid.parse()?),
        [op,root,id] if op=="task-worker"=>an_agent_host::application::tasks::worker(Path::new(root),id),
  [op,root] if op=="serve"=>super::with_cancellation(|cancel|an_agent_host::application::server::serve(Path::new(root),&cancel)),
  [op,archive] if op=="check-environment"=>environment::preview(Path::new(archive)),
  [op,root,archive,flag,digest] if op=="import-environment" && flag=="--approve"=>environment::import(Path::new(root),Path::new(archive),digest),
  _=>Err("usage: app check-environment ARCHIVE | import-environment PRIVATE_ROOT ARCHIVE --approve DIGEST".into())
 }
}
