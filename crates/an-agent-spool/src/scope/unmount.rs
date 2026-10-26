//! Pure removal planning: validate the whole children-first topology before effects.
use super::*;

impl ScopeTree {
    pub(super) fn unmount_order(&self, root: ScopeId) -> Result<Vec<ScopeId>, ScopeError> {
        let mut order = Vec::new();
        self.append_unmount_order(root, &mut order)?;
        // Simulate the known wrapper inverse without running user disposables.
        // Internal child wrappers can leave before their parent target; external
        // wrappers and incompatible sibling ordering still block the entire plan.
        let mut wrappers = self.wrappers.clone();
        for &id in &order {
            self.check_unpinned_in(id, &wrappers)?;
            for chain in wrappers.values_mut() {
                chain.retain(|(owner, _)| *owner != id);
            }
        }
        Ok(order)
    }

    fn append_unmount_order(
        &self,
        id: ScopeId,
        order: &mut Vec<ScopeId>,
    ) -> Result<(), ScopeError> {
        for &child in self.node(id)?.children.iter().rev() {
            self.append_unmount_order(child, order)?;
        }
        order.push(id);
        Ok(())
    }
}
