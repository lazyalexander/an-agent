//! OpenShell translates its qualified lifecycle evidence; it does not publish.
use super::*;
use serde_json::json;

pub(in crate::execution) struct OpenShell<'a>(pub &'a crate::openshell::Profile);

impl ExecutionBackend for OpenShell<'_> {
    fn validate(&self, file: bool) -> Result<()> {
        if !file {
            return Err("OpenShell adapter requires a file-kit-v1 deployment".into());
        }
        Ok(())
    }

    fn plan(
        &self,
        _executable: &Path,
        _directory: &Path,
        _file: bool,
        plan: &mut Value,
    ) -> Result<()> {
        merge_plan(plan, self.0.plan())?;
        plan["context"]["kind"] = json!("explicit-file-snapshot");
        plan["limits"]["wall_seconds"] = plan["wall_seconds"].clone();
        plan["files"] = json!({"declaration_slot":"/work","guest_directory":"/sandbox",
            "input":"/sandbox/input.txt","candidate":"/sandbox/output.txt","output_limit":65536,
            "guest_scratch":"/tmp","host_mounts":false,"extra_guest_files":"never imported"});
        Ok(())
    }

    fn execute(&self, request: Request<'_>, _file: bool) -> Result<Outcome> {
        let result = crate::openshell::execute(
            self.0,
            request.executable,
            request.directory,
            request.input,
            request.cancelled,
        )?;
        Ok(Outcome {
            operation: request.operation.clone(),
            execution: result.execution,
            environment: result.environment.map(environment),
            settled_environment: result.settled_environment.map(environment),
            termination_confirmed: result.termination_confirmed,
            requires_environment: true,
            candidate: result.artifact.map_or(Candidate::None, Candidate::Bytes),
            evidence: result.evidence,
        })
    }

    fn cancel_before_acceptance(&self) -> bool {
        true
    }
    fn decorate_receipt(&self, receipt: &mut Value, mut evidence: Value, accepted: bool) {
        evidence["artifact_accepted"] = json!(accepted);
        receipt["openshell"] = evidence;
        receipt["process"]["scope"] =
            json!("local-openshell-cli-only; remote termination is recorded separately");
        receipt["container"] = Value::Null;
    }
}

fn environment(identity: crate::openshell::RuntimeIdentity) -> Environment {
    Environment {
        id: identity.id,
        generation: identity.generation,
    }
}
