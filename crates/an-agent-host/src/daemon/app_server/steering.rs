//! Codex steering mailbox consumed once by the original dialogue owner.
use super::*;

pub(super) fn send_pending(
    dir: &Path,
    authority: &Binding,
    cancelled: &AtomicBool,
    input: &mut ChildStdin,
    thread: &str,
    turn: &str,
) -> Result<()> {
    if dir.join("cancel.json").exists() || !dir.join("session.json").exists() {
        return Ok(());
    }
    if crate::task_permissions::active(authority).is_err() {
        cancelled.store(true, Ordering::Release);
        return Ok(());
    }
    let session = read(&dir.join("session.json"))?;
    let mut commands = Vec::new();
    for path in storage::entries(dir, "commands", 33)? {
        if path.extension().is_some_and(|ext| ext == "json") {
            commands.push(read(&path)?);
        }
    }
    commands.sort_by_key(|command| command["sequence"].as_u64());
    for command in commands {
        if command["plan"]["operation"] != "steer-harness" {
            continue;
        }
        let name = text(&command["plan"], "command")?;
        let marker = dir.join(format!("sent-{name}.json"));
        if marker.exists() {
            continue;
        }
        if command["plan"]["execution"] != session["execution"] {
            return Err("stale steering command".into());
        }
        if command["plan"]
            .get("turn")
            .is_some_and(|selected| selected != turn)
        {
            let receipt = dir.join(format!("ack-{name}.json"));
            if !receipt.exists() {
                super::super::harness_adapter::publish(
                    &receipt,
                    &json!({"command":name,"state":"rejected","reason":"selected turn ended"}),
                )?;
            }
            continue;
        }
        if turn.is_empty() {
            continue;
        }
        if crate::task_permissions::active(authority).is_err() {
            cancelled.store(true, Ordering::Release);
            return Ok(());
        }
        // Persist before transport. A crash in this window is unknown, never resent.
        write(
            &marker,
            &json!({"command":name,"execution":session["execution"]}),
        )?;
        send(
            input,
            json!({"id":format!("steer-{name}"),"method":"turn/steer","params":{
            "threadId":thread,"expectedTurnId":turn,"input":[{"type":"text","text":command["instruction"]}]}}),
        )?;
    }
    Ok(())
}

pub(super) fn acknowledge(dir: &Path, message: &Value, turn: &str) -> Result<bool> {
    let Some(name) = message["id"]
        .as_str()
        .and_then(|v| v.strip_prefix("steer-"))
    else {
        return Ok(false);
    };
    if name.is_empty()
        || name.len() > 64
        || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || !dir.join(format!("sent-{name}.json")).is_file()
    {
        return Err("unbound steering response".into());
    }
    let command = read(&dir.join("commands").join(format!("{name}.json")))?;
    let selected = command["plan"]["turn"].as_str().unwrap_or(turn);
    let state = if message.get("error").is_some() {
        "rejected"
    } else if message["result"]["turnId"] == selected {
        "acknowledged"
    } else {
        return Err("steering response has foreign turn".into());
    };
    super::super::harness_adapter::publish(
        &dir.join(format!("ack-{name}.json")),
        &json!({"command":name,"state":state}),
    )?;
    Ok(true)
}

/// Poll the original pipe directly, keeping all I/O inside the supervised dialogue.
pub(super) struct Messages {
    output: ChildStdout,
    pending: Vec<u8>,
}
impl Messages {
    pub(super) fn new(output: ChildStdout) -> Self {
        Self {
            output,
            pending: Vec::new(),
        }
    }
    pub(super) fn next(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        if let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            if end >= 131072 {
                return Err(std::io::Error::other("app-server oversized message"));
            }
            return Ok(Some(self.pending.drain(..=end).collect()));
        }
        if self.pending.len() > 131072 {
            return Err(std::io::Error::other("app-server oversized message"));
        }
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let mut descriptor = libc::pollfd {
                fd: self.output.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut descriptor, 1, 50) };
            if ready == 0 {
                return Ok(None);
            }
            if ready < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted {
                    return Ok(None);
                }
                return Err(error);
            }
        }
        let mut bytes = [0; 4096];
        let count = self.output.read(&mut bytes)?;
        if count == 0 {
            return Err(std::io::Error::other(
                "app-server EOF before completion; never replay",
            ));
        }
        self.pending.extend_from_slice(&bytes[..count]);
        Ok(None)
    }
}
