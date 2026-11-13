//! Binary staging through bounded guest messages. Staging never accepts a product.
use super::*;
const CHUNK: usize = 8192;
const LIMIT: usize = 4 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    direction: String,
    path: String,
    attempt: String,
    sha256: String,
    bytes: usize,
    #[serde(default)]
    source: Option<PathBuf>,
}

fn decode(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid Cocoon binary chunk".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).map_err(Into::into))
        .collect()
}

/// The caller must have authorized the complete plan and any selected input.
pub(crate) fn run(
    directory: &Path,
    profile: &Profile,
    id: &str,
    plan: Value,
    task: &str,
    active: impl Fn() -> Result<()>,
) -> Result<Value> {
    uuid::Uuid::parse_str(id)?;
    let parsed: Plan = serde_json::from_value(plan.clone())?;
    uuid::Uuid::parse_str(&parsed.attempt)?;
    if parsed.bytes > LIMIT
        || !store::valid_digest(&parsed.sha256)
        || !matches!(parsed.direction.as_str(), "upload" | "download")
        || !parsed.path.starts_with("/workspace/")
        || Path::new(&parsed.path)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("invalid bounded Cocoon transfer plan".into());
    }
    active()?;
    let input = if parsed.direction == "upload" {
        let bytes = store::read(
            parsed.source.as_deref().ok_or("transfer input required")?,
            LIMIT,
        )?;
        if bytes.len() != parsed.bytes || store::digest(&bytes) != parsed.sha256 {
            return Err("Cocoon input changed".into());
        }
        bytes
    } else {
        if parsed.source.is_some() {
            return Err("download cannot select a host path".into());
        }
        Vec::new()
    };
    store::directory(directory)?;
    let root = directory.join(id);
    store::directory(&root)?;
    let binding = json!({"plan_sha256":store::digest(&serde_json::to_vec(&plan)?),
        "vm":profile.vm,"generation":profile.generation,"caller":profile.caller,"task":task});
    let intent = root.join("intent.json");
    if intent.exists() {
        if serde_json::from_slice::<Value>(&store::read(&intent, 32768)?)? != binding {
            return Err("Cocoon transfer identity already bound".into());
        }
    } else {
        persist(&intent, &binding)?;
    }
    let mut output = Vec::new();
    let chunks = parsed.bytes.max(1).div_ceil(CHUNK);
    let operations = root.join("operations");
    for index in 0..chunks {
        active()?;
        let offset = index * CHUNK;
        let size = (parsed.bytes - offset).min(CHUNK);
        let identity = json!({"vm":profile.vm,"caller":profile.caller,"task":task,
            "attempt":parsed.attempt,"generation":profile.generation,"request":0});
        let payload = if parsed.direction == "upload" {
            json!({"kind":"write_bytes","path":parsed.path,"offset":offset,"sync":index+1==chunks,
                "content_hex":input[offset..offset+size].iter().map(|b|format!("{b:02x}")).collect::<String>()})
        } else {
            json!({"kind":"read_chunk","path":parsed.path,"offset":offset,"max_bytes":size.max(1)})
        };
        let operation =
            json!({"operation":"guest","request":{"identity":identity,"operation":payload}});
        let chunk_id = store::digest(format!("{id}:{index}").as_bytes())[..32].to_string();
        let mut receipt = submit(&operations, profile, &chunk_id, operation)?;
        if receipt["state"] == "unknown" {
            active()?;
            // A query may recover evidence; never resubmit the guest operation.
            if let Ok(recovered) = reconcile(&operations, profile, &chunk_id) {
                receipt = recovered;
            }
        }
        if receipt["state"] != "observed" {
            return Ok(
                json!({"transfer":id,"state":"unknown","pending_operation":chunk_id,"artifact_accepted":false}),
            );
        }
        let response = &receipt["result"];
        if response["identity"] != identity {
            return Err("Cocoon transfer response identity mismatch".into());
        }
        let result = response["result"]
            .get("Ok")
            .ok_or("guest transfer failed")?;
        if parsed.direction == "upload" {
            if result["kind"] != "written" {
                return Err("guest write unconfirmed".into());
            }
        } else {
            if result["kind"] != "chunk" {
                return Err("guest read unconfirmed".into());
            }
            let bytes = decode(result["content_hex"].as_str().ok_or("guest binary chunk")?)?;
            if bytes.len() != size
                || result["next_offset"] != offset + size
                || result["eof"] != (index + 1 == chunks)
            {
                return Err("Cocoon output size or cursor mismatch".into());
            }
            output.extend(bytes);
        }
    }
    active()?;
    if parsed.direction == "download" {
        if output.len() != parsed.bytes || store::digest(&output) != parsed.sha256 {
            return Err("Cocoon output digest mismatch".into());
        }
        let candidate = root.join("candidate.bin");
        if candidate.exists() {
            if store::read(&candidate, LIMIT)? != output {
                return Err("candidate changed".into());
            }
        } else {
            let pending = root.join("candidate.pending");
            store::write_new(&pending, &output)?;
            fs::File::open(&pending)?.sync_all()?;
            fs::hard_link(&pending, &candidate)?;
            fs::remove_file(pending)?;
            fs::File::open(&root)?.sync_all()?;
        }
    }
    Ok(
        json!({"transfer":id,"state":"staged","direction":parsed.direction,
        "sha256":parsed.sha256,"bytes":parsed.bytes,"artifact_accepted":false}),
    )
}

#[cfg(test)]
mod tests;
