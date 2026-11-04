//! Retain interrupted delivery progress and fence uncertain body replay.
use super::{AgentControl, ControlError, DeliveryOutcome};
use crate::{agent::Agent, memstream::Kind, workspace::WorkspaceRecord};
use serde_json::{Value, json};

impl AgentControl {
    pub(super) fn deliver_batch(
        &self,
        agent: &Agent,
        event: &WorkspaceRecord,
        selected: Option<&str>,
    ) -> Result<Vec<DeliveryOutcome>, ControlError> {
        let instance = self
            .inner
            .supervision
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .instance(agent.id())?;
        if self
            .inner
            .agent_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&agent.id())
            .is_some_and(|r| r.failed_deliveries.contains(&event.id))
        {
            return Err(ControlError::DeliveryUnsettled(event.id.clone()));
        }
        let previous = agent
            .session()
            .tape()
            .read_all()?
            .into_iter()
            .rev()
            .find(|note| {
                note.tags.iter().any(|t| t == "audit")
                    && note.tags.iter().any(|t| t == "delivery_batch")
                    && serde_json::from_str::<Value>(&note.content)
                        .is_ok_and(|v| v["event"] == event.id)
            });
        if previous.is_some_and(|note| {
            serde_json::from_str::<Value>(&note.content).map_or(true, |v| {
                v["status"] != "settled" || v["instance"] != json!(instance)
            })
        }) {
            return Err(ControlError::DeliveryUnsettled(event.id.clone()));
        }
        self.hooks_before(agent, event)?;
        let (registration, route) = if let Some(name) = selected {
            (self.current_registration_snapshot()?, vec![name.to_owned()])
        } else {
            self.route_for(event.name.as_deref().unwrap_or_default())?
        };
        self.batch_note(agent, event, instance, "started")?;
        let mut ready = Vec::new();
        let mut withheld = None;
        let result = (|| {
            for name in route {
                match self.invoke(agent, &name, event, &registration) {
                    Ok(Ok(outcome)) => {
                        if let DeliveryOutcome::Reply(reply) = &outcome
                            && let Err(error) = self
                                .hooks_after(agent, reply)
                                .and_then(|()| self.validate_reply(agent.id(), reply))
                        {
                            if selected.is_none()
                                && matches!(error, ControlError::HookDenied { .. })
                            {
                                continue;
                            }
                            withheld = Some(Box::new(outcome));
                            return Err(error);
                        }
                        ready.push(outcome);
                    }
                    Ok(Err(reason)) if selected.is_some() => {
                        return Err(ControlError::SpoolFailed { name, reason });
                    }
                    Ok(Err(_)) => {}
                    Err(ControlError::NotMounted(_)) if selected.is_none() => {
                        self.note_spool(agent, &name, &event.id, Err("missing"))?;
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                if let Err(error) = self.batch_note(agent, event, instance, "settled") {
                    return Err(interrupted(self, agent, event, ready, withheld, error));
                }
                Ok(ready)
            }
            Err(error)
                if ready.is_empty()
                    && withheld.is_none()
                    && matches!(
                        error,
                        ControlError::HookDenied { .. }
                            | ControlError::SpoolFailed { .. }
                            | ControlError::NotMounted(_)
                    ) =>
            {
                if let Err(write) = self.batch_note(agent, event, instance, "settled") {
                    return Err(interrupted(self, agent, event, ready, withheld, write));
                }
                Err(error)
            }
            Err(error) => Err(interrupted(self, agent, event, ready, withheld, error)),
        }
    }

    fn batch_note(
        &self,
        agent: &Agent,
        event: &WorkspaceRecord,
        instance: super::InstanceRef,
        status: &str,
    ) -> Result<(), ControlError> {
        self.append_audit_event(
            agent,
            Kind::Action,
            "delivery_batch",
            json!({"event":event.id,"instance":instance,"status":status}).to_string(),
            vec![],
        )?;
        Ok(())
    }
}

fn interrupted(
    control: &AgentControl,
    agent: &Agent,
    event: &WorkspaceRecord,
    ready: Vec<DeliveryOutcome>,
    withheld: Option<Box<DeliveryOutcome>>,
    source: ControlError,
) -> ControlError {
    // A failed sync can leave a complete settlement line visible. Keep an
    // in-process fence too; resumed incarnations cannot replay any old batch.
    if let Some(run) = control
        .inner
        .agent_runs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_mut(&agent.id())
    {
        run.failed_deliveries.insert(event.id.clone());
    }
    // Ready means admitted and hook-checked, not applied. The host must still
    // validate before any explicit reconciliation; withheld is never executable.
    ControlError::DeliveryInterrupted {
        event: event.id.clone(),
        ready,
        withheld,
        source: Box::new(source),
    }
}
