//! One receiver-local handoff of an already delivered peer packet.
use super::*;
use crate::context_exchange::{self, ContextPacket};
use an_agent_core::{control::AgentControl, principal::card::AgentCard};
use std::collections::BTreeSet;
use uuid::Uuid;

const ROSTER: &str = "silk/agents.json";

fn select(bundle: &bundle::Bundle, receiver: Uuid, id: Uuid) -> Result<AgentCard> {
    let bytes = bundle
        .payload
        .get(ROSTER)
        .ok_or("receiver has no agent roster")?;
    if bytes.len() > 32768 {
        return Err("internal agent roster exceeds 32 KiB".into());
    }
    let cards: Vec<AgentCard> = serde_json::from_slice(bytes)?;
    let mut seen = BTreeSet::new();
    if cards.is_empty()
        || cards.len() > 8
        || cards.iter().any(|c| {
            c.v != 1
                || !c.tools.is_empty()
                || c.id.is_nil()
                || c.id == receiver
                || !seen.insert(c.id)
        })
    {
        return Err("expected one to eight distinct tool-free internal agent cards".into());
    }
    cards
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "agent is not declared by this receiver".into())
}

pub fn deliver_agent(path: &Path, name: &str, id: Uuid, approved: Option<&str>) -> Result<Value> {
    let (dir, record) = open(path, name)?;
    let peer = &record["plan"];
    if peer["schema"] != CONTEXT_REQUEST {
        return Err("agent delivery requires a bounded peer context".into());
    }
    let source = Path::new(text(&peer["source"], "root")?);
    let target = Path::new(text(&peer["target"], "root")?);
    let _source = store::lock(source)?;
    let _target = store::lock(target)?;
    let _request = store::lock(&dir)?;
    available(&dir, &record)?;
    let settled = result(&dir, &record, "request")?;
    if settled["status"] != "succeeded" {
        return Err("agent delivery requires a successful receiver phase".into());
    }
    let data = artifact(
        source,
        text(peer, "source_run")?,
        text(peer, "source_output")?,
    )?;
    let (mut packet, evidence) = selected_packet(&data, &peer["target"])?;
    if data != peer["data"] || evidence != peer["context_packet"] {
        return Err("peer context selection changed".into());
    }
    let (_, bundle) = active(target)?;
    let card = select(&bundle, packet.audience, id)?;
    let request = text(&record, "approval_sha256")?;
    let parent = target.join("agent-handoffs");
    let delivery = parent.join(request);
    store::no_links(&delivery)?;
    if delivery.try_exists()? {
        return Err("agent handoff already admitted or uncertain; inspect without replay".into());
    }
    let plan = json!({"schema":"an-native-agent-handoff-v1","request":request,
        "receiver":peer["target"],"run":settled["run"],"input":evidence,
        "agent":id,"card_sha256":card.hash()?,"model_disclosure":false});
    let (digest, execute) = approval(&plan, approved)?;
    packet.audience = id;
    packet.source = format!("peer:{digest}");
    let bytes = serde_json::to_vec(&packet)?;
    ContextPacket::parse(&bytes, id)?;
    if !execute {
        return Ok(json!({"plan":plan,"approval_sha256":digest,"executed":false}));
    }
    store::directory(&parent)?;
    fs::create_dir(&delivery)?;
    sync(&parent)?;
    sync(target)?;
    write_json(
        &delivery.join("intent.json"),
        &json!({"plan":plan,"approval_sha256":digest}),
    )?;
    sync(&delivery)?;
    // A fresh local engagement has no tools, model client or shared conversation.
    let harness = AgentControl::open(delivery.join("harness"), &[])?;
    let recipient = harness.open_agent(&card)?;
    let clip = context_exchange::import(&harness, recipient, &bytes)?;
    let session = harness.directory(recipient)?;
    harness.finish(recipient, "Local context received")?;
    let export = session.join("files/context.json");
    store::write_new(&export, &bytes)?;
    sync(export.parent().ok_or("missing participant files")?)?;
    let receipt = json!({"status":"succeeded","approval_sha256":digest,"agent":recipient,
        "clip":clip,"session":session,"packet":export,"packet_sha256":store::digest(&bytes)});
    write_json(&delivery.join("receipt.json"), &receipt)?;
    sync(&delivery)?;
    Ok(receipt)
}
