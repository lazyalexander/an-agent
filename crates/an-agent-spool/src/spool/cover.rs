//! Face-by-face check: a ceiling covers an effect, or it does not.

use an_agent_core::act::{FileFacet, MemoryFacet};

use crate::descriptor::{Net, Proc};

use super::{Faces, Flow};

/// `ceiling` covers `effect` when every face of the effect is inside it.
/// Mount rights use this. The caller decides whether
/// a miss is Forbidden or Deny.
pub fn covers(ceiling: &Faces, effect: &Faces) -> bool {
    file_covers(&ceiling.file, &effect.file)
        && memory_covers(&ceiling.memory, &effect.memory)
        && net_covers(ceiling.net, effect.net)
        && proc_covers(ceiling.proc_, effect.proc_)
        && flow_covers(ceiling.flow, effect.flow)
}

fn file_covers(ceiling: &FileFacet, effect: &FileFacet) -> bool {
    if matches!(effect, FileFacet::None) {
        return true;
    }
    match (ceiling, effect) {
        (FileFacet::Unbounded, FileFacet::Unbounded) => true,
        (
            FileFacet::Unbounded,
            FileFacet::Read { .. } | FileFacet::Write { .. } | FileFacet::ReadWrite { .. },
        ) => true,
        (
            FileFacet::Read {
                path: root,
                recursive,
            },
            FileFacet::Read { path, .. },
        )
        | (
            FileFacet::Write {
                path: root,
                recursive,
            },
            FileFacet::Write { path, .. },
        )
        | (
            FileFacet::ReadWrite {
                path: root,
                recursive,
            },
            FileFacet::Read { path, .. }
            | FileFacet::Write { path, .. }
            | FileFacet::ReadWrite { path, .. },
        ) => path_covers(root, *recursive, path),
        _ => false,
    }
}

fn path_covers(root: &str, recursive: bool, path: &str) -> bool {
    if path == root {
        return true;
    }
    recursive && path.starts_with(&format!("{root}/"))
}

fn memory_covers(ceiling: &MemoryFacet, effect: &MemoryFacet) -> bool {
    match (ceiling, effect) {
        (_, MemoryFacet::Ignore) => true,
        (MemoryFacet::Remember { aspect: None }, MemoryFacet::Remember { .. }) => true,
        (MemoryFacet::Remember { aspect: Some(a) }, MemoryFacet::Remember { aspect: Some(b) }) => {
            a == b
        }
        _ => false,
    }
}

fn net_covers(ceiling: Net, effect: Net) -> bool {
    matches!(
        (ceiling, effect),
        (Net::None, Net::None) | (Net::Egress, Net::None | Net::Egress)
    )
}

fn proc_covers(ceiling: Proc, effect: Proc) -> bool {
    matches!(
        (ceiling, effect),
        (Proc::None, Proc::None) | (Proc::Spawn, Proc::None | Proc::Spawn)
    )
}

fn flow_covers(ceiling: Flow, effect: Flow) -> bool {
    matches!(
        (ceiling, effect),
        (Flow::None, Flow::None)
            | (Flow::In, Flow::None | Flow::In)
            | (Flow::Out, Flow::None | Flow::Out)
            | (Flow::Both, _)
    )
}
