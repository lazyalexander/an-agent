//! Requires closure: the joined ceiling, and whether the mount can unwind.

use an_agent_core::act::{FileFacet, MemoryFacet, ModelFacet};

use crate::descriptor::{Net, Proc};

use super::{Faces, Flow, Inverse, Registry, SpoolError};

// --- transitive folds over the requires closure ---
//
// Some properties of a spool are inherited from its dependencies; they
// must be folded over the whole closure at admission time, never read off
// the root's own faces alone: a spool that can yield a net-egress member
// has egress reach. Publish order is topological (a requires target must
// already be published), so the closure is acyclic and the walk always
// terminates; the visited set only dedups diamonds.

/// The transitive fold of one spool's requires closure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closure {
    /// Join of every member's effect faces, root included.
    pub ceiling: Faces,
    /// False when any member declares itself irreversible.
    pub reversible: bool,
    /// Every (name, version) in the closure, root included.
    pub members: Vec<(String, String)>,
}

fn bottom_faces() -> Faces {
    Faces {
        file: FileFacet::None,
        memory: MemoryFacet::Ignore,
        net: Net::None,
        proc_: Proc::None,
        flow: Flow::None,
        model: ModelFacet::None,
    }
}

fn join_faces(a: &Faces, b: &Faces) -> Faces {
    Faces {
        file: file_join(&a.file, &b.file),
        memory: memory_join(&a.memory, &b.memory),
        net: net_join(a.net, b.net),
        proc_: proc_join(a.proc_, b.proc_),
        flow: flow_join(a.flow, b.flow),
        model: model_join(a.model, b.model),
    }
}

/// One member with model reach puts the model in the system's ceiling.
fn model_join(a: ModelFacet, b: ModelFacet) -> ModelFacet {
    if a == ModelFacet::Complete || b == ModelFacet::Complete {
        ModelFacet::Complete
    } else {
        ModelFacet::None
    }
}

/// Two different rooted paths cannot be expressed as one facet, so the
/// join over-approximates to Unbounded: a ceiling may be loose, never low.
fn file_join(a: &FileFacet, b: &FileFacet) -> FileFacet {
    use FileFacet as F;
    fn parts(f: &FileFacet) -> (&String, bool, u8) {
        match f {
            F::Read { path, recursive } => (path, *recursive, 1),
            F::Write { path, recursive } => (path, *recursive, 2),
            F::ReadWrite { path, recursive } => (path, *recursive, 3),
            F::None | F::Unbounded => unreachable!(),
        }
    }
    match (a, b) {
        (F::None, x) | (x, F::None) => x.clone(),
        (F::Unbounded, _) | (_, F::Unbounded) => F::Unbounded,
        _ => {
            let (pa, ra, ka) = parts(a);
            let (pb, rb, kb) = parts(b);
            if pa != pb {
                return F::Unbounded;
            }
            let path = pa.clone();
            let recursive = ra || rb;
            match ka | kb {
                1 => F::Read { path, recursive },
                2 => F::Write { path, recursive },
                _ => F::ReadWrite { path, recursive },
            }
        }
    }
}

/// Remember without an aspect covers any aspect, so joining two different
/// aspects widens to None; Forget never appears in an effect.
fn memory_join(a: &MemoryFacet, b: &MemoryFacet) -> MemoryFacet {
    use MemoryFacet as M;
    match (a, b) {
        (M::Ignore, x) | (x, M::Ignore) => x.clone(),
        (M::Remember { aspect: None }, _) | (_, M::Remember { aspect: None }) => {
            M::Remember { aspect: None }
        }
        (M::Remember { aspect: x }, M::Remember { aspect: y }) => M::Remember {
            aspect: if x == y { x.clone() } else { None },
        },
        (M::Forget { .. }, _) | (_, M::Forget { .. }) => unreachable!(),
    }
}

fn net_join(a: Net, b: Net) -> Net {
    if a == Net::Egress || b == Net::Egress {
        Net::Egress
    } else {
        Net::None
    }
}

fn proc_join(a: Proc, b: Proc) -> Proc {
    if a == Proc::Spawn || b == Proc::Spawn {
        Proc::Spawn
    } else {
        Proc::None
    }
}

fn flow_join(a: Flow, b: Flow) -> Flow {
    match (a, b) {
        (Flow::None, x) | (x, Flow::None) => x,
        (Flow::Both, _) | (_, Flow::Both) => Flow::Both,
        (Flow::In, Flow::In) => Flow::In,
        (Flow::Out, Flow::Out) => Flow::Out,
        _ => Flow::Both,
    }
}

impl Registry {
    /// Fold the requires closure of a published spool: the ceiling its
    /// whole system can reach, and whether the mount can be unwound.
    pub fn closure(&self, name: &str, version: &str) -> Result<Closure, SpoolError> {
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![self.recover(name, version)?];
        let mut ceiling = bottom_faces();
        let mut reversible = true;
        let mut members = Vec::new();
        while let Some(spec) = stack.pop() {
            if !seen.insert((spec.name.clone(), spec.version.clone())) {
                continue;
            }
            ceiling = join_faces(&ceiling, &spec.effect);
            reversible &= !matches!(spec.inverse, Inverse::Irreversible);
            members.push((spec.name.clone(), spec.version.clone()));
            for req in &spec.requires {
                stack.push(self.recover(&req.name, &req.version)?);
            }
        }
        Ok(Closure {
            ceiling,
            reversible,
            members,
        })
    }
}
