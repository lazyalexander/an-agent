//! Receipt-checked, deterministic views of retained output. Never model authority.
use crate::{
    context_exchange::ContextPacket,
    execution::{Result, store},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{fs, path::Path};
use uuid::Uuid;

#[derive(Serialize)]
struct ByteRange {
    start: usize,
    end: usize,
}

#[derive(Serialize)]
struct Excerpt {
    text: String,
    ranges: Vec<ByteRange>,
    omitted_bytes: usize,
}

// A pure view: retain exact UTF-8 source ranges, never summarize or repair bytes.
fn excerpt(text: &str, budget: usize) -> Result<Excerpt> {
    if !(128..=2048).contains(&budget) {
        return Err("text byte budget must be between 128 and 2048".into());
    }
    if text.len() <= budget {
        return Ok(Excerpt {
            text: text.into(),
            ranges: vec![ByteRange {
                start: 0,
                end: text.len(),
            }],
            omitted_bytes: 0,
        });
    }
    // Reserve space for the omission marker; bytes are not a token estimate.
    let retained_budget = budget - 64;
    let mut head = retained_budget / 2;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - (retained_budget - retained_budget / 2);
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    let omitted = tail - head;
    Ok(Excerpt {
        text: format!(
            "{}\n[... {omitted} source bytes omitted ...]\n{}",
            &text[..head],
            &text[tail..]
        ),
        ranges: vec![
            ByteRange {
                start: 0,
                end: head,
            },
            ByteRange {
                start: tail,
                end: text.len(),
            },
        ],
        omitted_bytes: omitted,
    })
}

/// Export one explicit retained output into the existing context packet ABI.
/// No scan, replay, model request, thread import, or source modification occurs.
/// The operator authorizes reading this private run and selecting its audience.
pub fn export(
    run: &Path,
    stream: &str,
    audience: Uuid,
    budget: usize,
    output: &Path,
) -> Result<Value> {
    store::no_links(run)?;
    let run = run.canonicalize()?;
    let receipt_bytes = store::read(&run.join("receipt.json"), 65536)?;
    let receipt: Value = serde_json::from_slice(&receipt_bytes)?;
    let status = receipt["status"].as_str().ok_or("missing run status")?;
    let (filename, expected, limit) = match stream {
        "stdout" | "stderr"
            if receipt["schema_version"] == 1
                && [
                    "succeeded",
                    "failed",
                    "refused",
                    "cancelled",
                    "timed_out",
                    "output_limit",
                ]
                .contains(&status) =>
        {
            let filename = if stream == "stdout" {
                "stdout.txt"
            } else {
                "stderr.txt"
            };
            if receipt[stream]["path"] != filename {
                return Err("unexpected artifact path".into());
            }
            (filename, &receipt[stream]["sha256"], 65536)
        }
        "model" if receipt["schema"] == "an-model-receipt-v1" && status == "succeeded" => (
            "output.txt",
            &receipt["result"]["response_sha256"],
            1024 * 1024,
        ),
        _ => {
            return Err(
                "unsupported receipt, outcome or stream; use stdout, stderr or model".into(),
            );
        }
    };
    let bytes = store::read(&run.join(filename), limit)?;
    let digest = store::digest(&bytes);
    if expected.as_str() != Some(&digest) {
        return Err("artifact digest mismatch".into());
    }
    let view = excerpt(std::str::from_utf8(&bytes)?, budget)?;
    let receipt_digest = store::digest(&receipt_bytes);
    let packet = ContextPacket {
        schema: "an-context-v1".into(),
        audience,
        source: format!("host-artifact:{status}:{receipt_digest}:{stream}"),
        sha256: store::digest(view.text.as_bytes()),
        text: view.text.clone(),
    };
    let packet_bytes = serde_json::to_vec(&packet)?;
    // JSON escaping counts too. Fail before persistence rather than exceed the ABI.
    ContextPacket::parse(&packet_bytes, audience)
        .map_err(|_| "serialized context exceeds packet limit; choose a smaller byte budget")?;
    store::no_links(output)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let output = parent
        .canonicalize()?
        .join(output.file_name().ok_or("missing output name")?);
    if output.starts_with(&run) {
        return Err("context export must not modify the source run".into());
    }
    let selection = json!({
        "schema":"an-artifact-context-selection-v1", "source_run":run,
        "receipt_sha256":receipt_digest, "artifact":filename, "artifact_sha256":digest,
        "artifact_bytes":bytes.len(), "source_status":status, "audience":audience,
        "strategy":"utf8-head-tail-v1", "text_byte_budget":budget,
        "ranges":view.ranges, "omitted_bytes":view.omitted_bytes,
        "selected_text_bytes":view.text.len(), "packet_bytes":packet_bytes.len(),
        "packet_sha256":store::digest(&packet_bytes),
        "source_scope":"retained artifact only; not a claim that process output was complete",
        "trust":"untrusted data; digests establish byte identity, not authorship or permission"
    });
    // Exclusive directory creation: never overwrite previous selections. Partial
    // persistence remains inspectable on I/O failure, never reported as success.
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&output)?;
    store::write_new(
        &output.join("selection.json"),
        &serde_json::to_vec_pretty(&selection)?,
    )?;
    store::write_new(&output.join("context.json"), &packet_bytes)?;
    Ok(
        json!({"status":"succeeded", "operation":"context-export", "context":output.join("context.json"),
        "selection":output.join("selection.json"), "packet_bytes":packet_bytes.len(),
        "omitted_bytes":view.omitted_bytes, "model_called":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_short_text_and_bounded_unicode_views() -> Result<()> {
        let short = excerpt("完整\n", 128)?;
        assert_eq!(short.text, "完整\n");
        assert_eq!(short.omitted_bytes, 0);
        for budget in 128..=2048 {
            let source = "齿轮⚙️abc\n".repeat(500);
            let view = excerpt(&source, budget)?;
            assert!(view.text.len() <= budget);
            assert!(view.text.contains("source bytes omitted"));
            let retained: usize = view
                .ranges
                .iter()
                .map(|r| {
                    assert!(source.get(r.start..r.end).is_some());
                    r.end - r.start
                })
                .sum();
            assert_eq!(retained + view.omitted_bytes, source.len());
        }
        assert!(excerpt("x", 127).is_err());
        assert!(excerpt("x", 2049).is_err());
        Ok(())
    }
}
