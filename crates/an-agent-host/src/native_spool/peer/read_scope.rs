//! Release-bound ceilings for a local, read-only peer context request.
use super::*;
use serde::Deserialize;

const DECLARATION: &str = "silk/peer-read.json";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadRequest {
    pub task: String,
    pub material_revision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadScope {
    schema: String,
    task: String,
    worker_release: String,
    material_output: String,
    reply_output: String,
    max_text_bytes: u64,
    max_wall_seconds: u64,
}

impl ReadScope {
    pub(super) fn select(
        bundle: &bundle::Bundle,
        request: Option<&ReadRequest>,
    ) -> Result<Option<Self>> {
        match (bundle.payload.get(DECLARATION), request) {
            (None, None) => Ok(None),
            (Some(bytes), Some(_)) if bytes.len() <= 4096 => {
                let scope: Self = serde_json::from_slice(bytes)?;
                bundle::identifier(&scope.task)?;
                bundle::identifier(&scope.material_output)?;
                bundle::identifier(&scope.reply_output)?;
                if scope.schema != "an-native-peer-read-v1"
                    || !store::valid_digest(&scope.worker_release)
                    || !(1..=crate::context_exchange::MAX_PACKET_BYTES as u64)
                        .contains(&scope.max_text_bytes)
                    || !(1..=300).contains(&scope.max_wall_seconds)
                    || !bundle.manifest.outputs.contains(&scope.material_output)
                    || bundle
                        .manifest
                        .binary_outputs
                        .contains(&scope.material_output)
                {
                    return Err("invalid release read declaration".into());
                }
                Ok(Some(scope))
            }
            _ => Err("read request and a bounded release declaration must both be present".into()),
        }
    }

    pub(super) fn check(
        &self,
        request: &ReadRequest,
        target: &Value,
        output: &str,
        reply_output: &str,
        wall_seconds: u64,
    ) -> Result<()> {
        if self.task != request.task
            || self.worker_release != target["release"]
            || self.material_output != output
            || self.reply_output != reply_output
            || wall_seconds > self.max_wall_seconds
            || !store::valid_digest(&request.material_revision)
        {
            return Err("peer read exceeds the release declaration".into());
        }
        Ok(())
    }

    pub(super) fn evidence(
        &self,
        request: &ReadRequest,
        data: &Value,
        packet: &Value,
    ) -> Result<Value> {
        if data["binding"]["revision"] != request.material_revision
            || packet["text_bytes"]
                .as_u64()
                .ok_or("missing text budget evidence")?
                > self.max_text_bytes
        {
            return Err("peer read material revision or text budget mismatch".into());
        }
        Ok(
            json!({"task":self.task,"access":"read","disclosure":"local-only",
            "material_revision":request.material_revision,"max_text_bytes":self.max_text_bytes,
            "max_wall_seconds":self.max_wall_seconds}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_rejects_wrong_worker_revision_and_budget() -> Result<()> {
        let scope: ReadScope = serde_json::from_value(json!({
            "schema":"an-native-peer-read-v1","task":"verify-grip-finish",
            "worker_release":"a".repeat(64),"material_output":"material.txt",
            "reply_output":"proof.txt","max_text_bytes":128,"max_wall_seconds":15
        }))?;
        let read = ReadRequest {
            task: "verify-grip-finish".into(),
            material_revision: "b".repeat(64),
        };
        let target = json!({"release":"a".repeat(64)});
        let accepted = json!({"binding":{"revision":read.material_revision}});
        scope.check(&read, &target, "material.txt", "proof.txt", 15)?;
        assert!(
            scope
                .check(&read, &target, "material.txt", "proof.txt", 16)
                .is_err()
        );
        assert!(
            scope
                .check(
                    &read,
                    &json!({"release":"c".repeat(64)}),
                    "material.txt",
                    "proof.txt",
                    15
                )
                .is_err()
        );
        assert!(
            scope
                .evidence(&read, &accepted, &json!({"text_bytes":129}))
                .is_err()
        );
        assert!(
            scope
                .evidence(
                    &read,
                    &json!({"binding":{"revision":"c".repeat(64)}}),
                    &json!({"text_bytes":1})
                )
                .is_err()
        );
        assert_eq!(
            scope.evidence(&read, &accepted, &json!({"text_bytes":128}))?["disclosure"],
            "local-only"
        );
        Ok(())
    }
}
