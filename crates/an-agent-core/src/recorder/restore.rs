//! Rebuild live seats from the tape. A broken parent chain is dropped.

use super::*;

pub fn restore_seats(session_dir: &Path) -> Result<Vec<RestoredSeat>, RecorderError> {
    use crate::memstream::JsonlStore;
    let events = JsonlStore::open(session_dir.join("memory.jsonl"))?.read_all()?;
    let mut nodes: HashMap<Uuid, RestoredSeat> = HashMap::new();
    let mut recorder_id = None;
    for event in &events {
        let Ok(recorder) = event.from.parse::<Uuid>() else {
            continue;
        };
        if event.tags.iter().any(|tag| tag == "spawn") {
            recorder_id = Some(recorder);
            let value: Value = serde_json::from_str(&event.content)?;
            let id = value["worker"]
                .as_str()
                .unwrap_or_default()
                .parse::<Uuid>()
                .map_err(|_| RecorderError::NotMounted(recorder))?;
            let parent = value["parent"]
                .as_str()
                .and_then(|text| text.parse().ok())
                .unwrap_or(recorder);
            nodes.insert(
                id,
                RestoredSeat {
                    id,
                    parent,
                    lite: value["seat"].as_str() == Some("lite"),
                    subwp: value["subwp"].as_str().map(str::to_string),
                },
            );
        }
        if event.tags.iter().any(|tag| tag == "release") {
            let value: Value = serde_json::from_str(&event.content)?;
            if let Some(id) = value["id"].as_str().and_then(|text| text.parse().ok()) {
                remove_subtree(&mut nodes, id);
            }
        }
        if event.tags.iter().any(|tag| tag == "cancel") {
            let value: Value = serde_json::from_str(&event.content)?;
            if let Some(id) = value["to"].as_str().and_then(|text| text.parse().ok()) {
                remove_subtree(&mut nodes, id);
            }
        }
    }
    let Some(recorder_id) = recorder_id else {
        return Ok(Vec::new());
    };
    let live: Vec<Uuid> = nodes
        .keys()
        .copied()
        .filter(|id| parent_chain_reaches(*id, &nodes, recorder_id))
        .collect();
    nodes.retain(|id, _| live.contains(id));
    let mut restored: Vec<_> = nodes.into_values().collect();
    restored.sort_by_key(|seat| seat.id);
    Ok(restored)
}

fn remove_subtree(nodes: &mut HashMap<Uuid, RestoredSeat>, root: Uuid) {
    let mut drop_ids = vec![root];
    let mut i = 0;
    while i < drop_ids.len() {
        let id = drop_ids[i];
        for (child, seat) in nodes.iter() {
            if seat.parent == id {
                drop_ids.push(*child);
            }
        }
        i += 1;
    }
    for id in drop_ids {
        nodes.remove(&id);
    }
}

fn parent_chain_reaches(id: Uuid, nodes: &HashMap<Uuid, RestoredSeat>, recorder: Uuid) -> bool {
    let mut cursor = id;
    for _ in 0..=nodes.len() {
        let Some(seat) = nodes.get(&cursor) else {
            return false;
        };
        if seat.parent == recorder {
            return true;
        }
        cursor = seat.parent;
    }
    false
}
