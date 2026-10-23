//! Native adapter delegates enforcement and process ownership to the supervisor.
use super::*;
use crate::execution::store::{directory, write_new};
use serde_json::json;
use std::time::Duration;

pub(in crate::execution) struct Native;

impl ExecutionBackend for Native {
    fn validate(&self, _file: bool) -> Result<()> {
        Ok(())
    }

    fn plan(&self, executable: &Path, run: &Path, file: bool, plan: &mut Value) -> Result<()> {
        merge_plan(
            plan,
            if file {
                crate::file_job::file_plan(executable, &run.join("work"))?
            } else {
                sandbox::plan(executable)?
            },
        )?;
        if file {
            plan["cwd"] = json!(run.join("work"));
            plan["context"]["kind"] = json!("explicit-file-snapshot");
            plan["files"] = json!({"declaration_slot":"/work","bound_directory":run.join("work"),"read":"work/input.txt","write":"work/output.txt","output_limit":65536,"other_writes":false});
        }
        Ok(())
    }

    fn execute(&self, request: Request<'_>, file: bool) -> Result<Outcome> {
        let (execution, candidate) = if file {
            let work = request.directory.join("work");
            directory(&work)?;
            write_new(&work.join("input.txt"), request.input)?;
            let slot = crate::file_job::prepare_output(&work)?;
            (
                crate::file_job::execute_file(request.executable, &work, request.cancelled)?,
                Candidate::NativeFile(slot),
            )
        } else {
            (
                sandbox::execute(
                    request.executable,
                    request.directory,
                    request.input,
                    Duration::from_secs(5),
                    request.cancelled,
                )?,
                Candidate::None,
            )
        };
        Ok(Outcome {
            execution,
            candidate,
            evidence: Value::Null,
        })
    }

    fn cancel_before_acceptance(&self) -> bool {
        false
    }
    fn decorate_receipt(&self, _receipt: &mut Value, _evidence: Value, _accepted: bool) {}
}
