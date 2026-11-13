//! Bounded next turns on the original provider thread and supervised owner.
use super::*;

pub(super) fn send_pending(
    dir: &Path,
    input: &mut ChildStdin,
    thread: &str,
    completed: u8,
    schema: &Value,
    authority: &Binding,
    cancelled: &AtomicBool,
) -> Result<()> {
    let session = read(&dir.join("session.json"))?;
    for path in storage::entries(dir, "commands", 33)? {
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let command = read(&path)?;
        if command["plan"]["operation"] != "continue-harness"
            || command["plan"]["after_turn"] != completed
        {
            continue;
        }
        let name = text(&command["plan"], "command")?;
        let marker = dir.join(format!("sent-{name}.json"));
        if marker.exists() {
            return Ok(());
        }
        if command["plan"]["execution"] != session["execution"] {
            return Err("continuation execution changed".into());
        }
        if crate::task_permissions::active(authority).is_err() || dir.join("cancel.json").exists() {
            cancelled.store(true, Ordering::Release);
            return Ok(());
        }
        write(
            &marker,
            &json!({"command":name,"execution":session["execution"],"after_turn":completed}),
        )?;
        send(
            input,
            json!({"id":format!("continue-{name}"),"method":"turn/start","params":{
            "threadId":thread,"input":[{"type":"text","text":command["instruction"]}],"outputSchema":schema}}),
        )?;
        return Ok(());
    }
    Ok(())
}

pub(super) fn acknowledge(dir: &Path, message: &Value, completed: u8) -> Result<Option<String>> {
    let Some(name) = message["id"]
        .as_str()
        .and_then(|id| id.strip_prefix("continue-"))
    else {
        return Ok(None);
    };
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || read(&dir.join(format!("sent-{name}.json")))?["after_turn"] != completed
    {
        return Err("unbound continuation response".into());
    }
    if message.get("error").is_some() {
        super::super::task_session::publish(
            &dir.join(format!("ack-{name}.json")),
            &json!({"command":name,"state":"rejected"}),
        )?;
        return Err("provider refused continuation on original session".into());
    }
    let turn = text(&message["result"]["turn"], "id")?.to_owned();
    super::super::task_session::publish(
        &dir.join(format!("ack-{name}.json")),
        &json!({"command":name,"state":"acknowledged","turn":turn}),
    )?;
    Ok(Some(turn))
}
