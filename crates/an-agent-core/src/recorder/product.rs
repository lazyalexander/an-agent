//! Close, collect, and link. The recorder stores pointers, not bodies.

use super::*;

impl Recorder {
    /// Publish the worker's sub-workspace if it wrote anything.
    /// Recorder does not write file bytes.
    pub fn close_worker(&self, worker: Uuid, extra: &[DepEdge]) -> Result<CloseOut, RecorderError> {
        let sub = self
            .wp
            .subwp_of(worker)
            .ok_or(RecorderError::NoSubwp(worker))?;
        Ok(self.wp.close(&sub, extra)?)
    }

    /// Collect: record pointers only. Does not run tools.
    pub fn collect(&self, product: &ProductPtr) -> Result<String, RecorderError> {
        let _kind = TurnKind::Collect;
        self.link(product)
    }

    pub fn link(&self, product: &ProductPtr) -> Result<String, RecorderError> {
        let body = json!({
            "child_session": product.child_session,
            "tape_sha256": product.tape_sha256,
            "md_sha256": product.md_sha256,
            "tree_id": product.tree_id,
            "subwp_sha256": product.subwp_sha256,
        })
        .to_string();
        self.append(&self.agent, "link", body, vec![])
    }

    pub fn define_view(&self, name: &str, link_ids: &[String]) -> Result<String, RecorderError> {
        let body = json!({ "name": name, "links": link_ids }).to_string();
        self.append(&self.agent, "view", body, vec![])
    }

    /// Latest definition of `name` on the recorder tape. Read-only.
    pub fn view(&self, name: &str) -> Result<Vec<String>, RecorderError> {
        let events = self.agent.session().tape().read_all()?;
        let found = events.iter().rev().find(|e| {
            e.tags.iter().any(|t| t == "view")
                && super::view_name(&e.content).as_deref() == Some(name)
        });
        let Some(event) = found else {
            return Err(RecorderError::Empty);
        };
        let v: Value = serde_json::from_str(&event.content)?;
        let links = v["links"]
            .as_array()
            .ok_or(RecorderError::Empty)?
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
        Ok(links)
    }
}
