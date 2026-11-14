//! Finite product controls over existing authoritative task/session operations.
use crate::{daemon, execution::Result};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn view(service: &Path, id: &str) -> Result<Value> {
    let task = daemon::request(service, &json!({"op":"task","id":id}))?;
    let session = if task["kind"] == "harness" {
        daemon::request(service, &json!({"op":"harness-connect","id":id}))?
    } else {
        Value::Null
    };
    Ok(json!({"task":task,"session":session}))
}
pub(super) fn submit(service: &Path, value: &Value) -> Result<Value> {
    let id = value["id"].as_str().ok_or("task identity required")?;
    let current = view(service, id)?;
    let mut request = match value["control"].as_str() {
        Some("cancel") => json!({"op":"cancel","id":id}),
        Some("supplement") => {
            let session = &current["session"];
            if session.is_null() {
                return Err("task has no interactive session".into());
            }
            if value["execution"] != session["execution"] {
                return Err("task execution changed".into());
            }
            let instruction = value["instruction"]
                .as_str()
                .ok_or("instruction required")?;
            let command = value["command"]
                .as_str()
                .ok_or("command identity required")?;
            if session["turns"]["active"].is_null() {
                json!({"op":"harness-continue","id":id,"execution":value["execution"],
                    "after_turn":value["after_turn"],"instruction":instruction,"command":command})
            } else {
                json!({"op":"harness-steer","id":id,"execution":value["execution"],
                    "instruction":instruction,"command":command})
            }
        }
        _ => return Err("unsupported product task control".into()),
    };
    if let Some(approval) = value.get("approve") {
        request["approve"] = approval.clone();
    }
    daemon::request(service, &request)
}
