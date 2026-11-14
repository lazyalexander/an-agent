//! Numeric observations over original provider events and authorized tool receipts.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const TOKEN_FIELDS: [&str; 5] = [
    "input_tokens",
    "cached_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
];

fn valid_snapshot(usage: &Value, previous: Option<&Value>) -> bool {
    for (subset, total) in [
        ("cached_input_tokens", "input_tokens"),
        ("reasoning_output_tokens", "output_tokens"),
    ] {
        if usage[subset]
            .as_u64()
            .zip(usage[total].as_u64())
            .is_some_and(|(a, b)| a > b)
        {
            return false;
        }
    }
    if let (Some(input), Some(output), Some(total)) = (
        usage["input_tokens"].as_u64(),
        usage["output_tokens"].as_u64(),
        usage["total_tokens"].as_u64(),
    ) && input.checked_add(output) != Some(total)
    {
        return false;
    }
    previous.is_none_or(|old| {
        TOKEN_FIELDS.iter().all(|key| {
            old[key]
                .as_u64()
                .zip(usage[key].as_u64())
                .is_none_or(|(a, b)| b >= a)
        })
    })
}

pub(super) fn now_ms() -> u64 {
    an_agent_core::det_seam::Clock::wall().now_ms()
}

/// A resource disclosure is measured after authorization and successful reading.
pub(super) fn disclosure(arguments: &Value, response: &Value) -> Option<Value> {
    let request: Value = serde_json::from_str(arguments["request"].as_str()?).ok()?;
    if !matches!(request["op"].as_str(), Some("read-input" | "artifact-read")) {
        return None;
    }
    let (digest, count) = if let Some(text) = response["text"].as_str() {
        (store::digest(text.as_bytes()), text.len())
    } else {
        let bytes: Option<Vec<u8>> = response["bytes"]
            .as_array()?
            .iter()
            .map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
            .collect();
        let bytes = bytes?;
        (store::digest(&bytes), bytes.len())
    };
    Some(json!({"content_sha256":digest,"content_bytes":count,
        "resource_sha256":store::digest(&serde_json::to_vec(&request).ok()?),
        "response_bytes":serde_json::to_vec(response).ok()?.len(),
        "source":"authorized-host-read","delivered_to_model":null}))
}

pub(super) fn view(dir: &Path, task: &harness::Task) -> Result<Value> {
    let adapter = super::harness_adapter::selected(task)?;
    let mut snapshots = BTreeMap::new();
    let mut updates = 0;
    let mut invalid = 0;
    let session = if dir.join("session.json").exists() {
        read(&dir.join("session.json"))?
    } else {
        Value::Null
    };
    let native = dir.join("native-events.jsonl");
    if native.exists() {
        for raw in store::read(&native, 2 * 1024 * 1024)?.split_inclusive(|b| *b == b'\n') {
            if raw.last() != Some(&b'\n') {
                continue;
            }
            let Ok(event) = serde_json::from_slice::<Value>(raw) else {
                invalid += 1;
                continue;
            };
            let normalized = adapter.event(&event);
            let usage = &normalized["usage"];
            let Some(id) = usage["session"].as_str() else {
                continue;
            };
            if super::harness_adapter::interactive(task)? && usage["session"] != session["thread"] {
                invalid += 1;
                continue;
            }
            updates += 1;
            if !valid_snapshot(usage, snapshots.get(id)) {
                invalid += 1;
                continue;
            }
            snapshots.insert(id.to_owned(), usage.clone());
        }
    }
    let mut usage = json!({"available":!snapshots.is_empty(),"updates":updates,"invalid_records":invalid,
        "sessions":snapshots.len(),"aggregation":"latest cumulative snapshot per session",
        "source":"provider-reported","cache_is_input_subset":true,"reasoning_is_output_subset":true});
    for key in TOKEN_FIELDS {
        let sum = snapshots
            .values()
            .try_fold(0u64, |sum, item| sum.checked_add(item[key].as_u64()?));
        usage[key] = sum
            .filter(|_| !snapshots.is_empty())
            .map_or(Value::Null, |n| json!(n));
    }
    let path = dir.join("tool-receipts.jsonl");
    let mut disclosures = Vec::new();
    let mut unique = BTreeSet::new();
    let mut total_bytes = 0;
    let mut unique_bytes = 0;
    let mut tool_us = 0u64;
    let mut tool_count = 0;
    let mut timed_tools = 0;
    let mut receipt_invalid = 0;
    if path.exists() {
        for raw in store::read(&path, 2 * 1024 * 1024)?.split_inclusive(|b| *b == b'\n') {
            if raw.last() != Some(&b'\n') {
                continue;
            }
            let Ok(record) = serde_json::from_slice::<Value>(raw) else {
                receipt_invalid += 1;
                continue;
            };
            tool_count += 1;
            let elapsed = record["elapsed_us"].as_u64().or_else(|| {
                record["elapsed_ms"]
                    .as_u64()
                    .and_then(|ms| ms.checked_mul(1000))
            });
            if let Some(elapsed) = elapsed {
                tool_us = tool_us.checked_add(elapsed).ok_or("tool timing overflow")?;
                timed_tools += 1;
            }
            let data = &record["disclosure"];
            if let (Some(hash), Some(bytes)) = (
                data["content_sha256"].as_str(),
                data["content_bytes"].as_u64(),
            ) {
                total_bytes += bytes;
                if unique.insert(hash.to_owned()) {
                    unique_bytes += bytes;
                }
                disclosures.push(json!({"content_sha256":hash,"content_bytes":bytes,
                    "response_bytes":data["response_bytes"],"resource_sha256":data["resource_sha256"],
                    "observed_ms":record["observed_ms"]}));
            }
        }
    }
    Ok(json!({"schema":"an-harness-metrics-v1","usage":usage,
        "disclosure":{"available":path.exists(),"reads":disclosures.len(),
            "total_bytes":total_bytes,"unique_bytes":unique_bytes,"repeated_bytes":total_bytes-unique_bytes,
            "invalid_records":receipt_invalid,"resources":disclosures,
            "scope":"authorized host reads; classify knowledge using release-bound content hashes",
            "model_consumption":"unknown; host response receipt only"},
        "host_tool_elapsed_ms":(tool_count == timed_tools).then_some(tool_us as f64 / 1000.0),
        "host_tool_timing":{"receipts":tool_count,"timed_receipts":timed_tools}}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cumulative_usage_rejects_regression_and_invalid_subsets() {
        let old =
            json!({"input_tokens":10,"cached_input_tokens":4,"output_tokens":2,"total_tokens":12});
        assert!(valid_snapshot(&old, Some(&old)));
        assert!(!valid_snapshot(&json!({"input_tokens":9}), Some(&old)));
        assert!(!valid_snapshot(
            &json!({"input_tokens":3,"cached_input_tokens":4}),
            None
        ));
        assert!(!valid_snapshot(
            &json!({"input_tokens":3,"output_tokens":2,"total_tokens":6}),
            None
        ));
        assert!(valid_snapshot(&json!({"input_tokens":11}), Some(&old)));
    }

    #[test]
    fn numeric_usage_excludes_private_content_and_preserves_missing_values() {
        let value =
            super::super::codex::usage(&json!({"method":"thread/tokenUsage/updated","params":{
            "threadId":"thread","turnId":"turn","secret":"hidden",
            "tokenUsage":{"total":{"inputTokens":10,"cachedInputTokens":5,"outputTokens":3}}}}))
            .unwrap();
        assert_eq!(value["total_tokens"], 13);
        assert_eq!(value["cached_input_tokens"], 5);
        assert!(value["reasoning_output_tokens"].is_null());
        assert!(!value.to_string().contains("hidden"));
        assert!(super::super::codex::usage(&json!({"type":"turn.started"})).is_none());
    }

    #[test]
    fn disclosure_counts_utf8_bytes_without_retaining_text() {
        let arguments = json!({"request":r#"{"op":"read-input","path":"private"}"#});
        let receipt = disclosure(&arguments, &json!({"text":"知识"})).unwrap();
        assert_eq!(receipt["content_bytes"], 6);
        assert!(!receipt.to_string().contains("知识"));
        assert!(disclosure(&arguments, &json!({"error":"denied"})).is_none());
    }
}
