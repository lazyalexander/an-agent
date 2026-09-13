use serde::{Deserialize, Serialize};

use super::sentence::{Access, ActSentence, BareFile, Ingest};
use super::tag::ToolTag;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub unbounded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workplace: Option<String>,
    pub memory: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

fn ingest_name(m: &Ingest) -> String {
    match m {
        Ingest::Ignore => "ignore".into(),
        Ingest::Remember { .. } => "remember".into(),
    }
}

fn empty_effect(workplace: Option<String>, memory: String, unbounded: bool) -> Effect {
    Effect {
        reads: vec![],
        writes: vec![],
        unbounded,
        workplace,
        memory,
        from_head: None,
        to_head: None,
        blob: None,
    }
}

pub fn effect_from_sentence(sentence: &ActSentence) -> Effect {
    match sentence {
        ActSentence::Bare {
            file, memory, ..
        } => empty_effect(
            None,
            ingest_name(memory),
            matches!(file, BareFile::Unbounded),
        ),
        ActSentence::OnResource {
            resource,
            access,
            memory,
            ..
        } => {
            let path = resource.to_string();
            let (reads, writes) = match access {
                Access::R { .. } => (vec![path], vec![]),
                Access::W { .. } => (vec![], vec![resource.to_string()]),
                Access::Rw { .. } => (vec![path.clone()], vec![path]),
            };
            Effect {
                reads,
                writes,
                unbounded: false,
                workplace: Some(resource.workplace.to_string()),
                memory: ingest_name(memory),
                from_head: None,
                to_head: None,
                blob: None,
            }
        }
        ActSentence::Forget { .. } => empty_effect(None, "forget".into(), false),
    }
}

pub fn effect_from_tag(tag: &ToolTag, workplace: Option<String>) -> Effect {
    match ActSentence::from_seed(tag, None) {
        Ok(s) => {
            let mut e = effect_from_sentence(&s);
            if e.workplace.is_none() {
                e.workplace = workplace;
            }
            e
        }
        Err(_) => empty_effect(workplace, tag.memory.op_name().into(), false),
    }
}
