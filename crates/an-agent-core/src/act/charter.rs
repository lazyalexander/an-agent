//! Spawn ceiling. A charter is a [`ToolTag`] plus a [`Signal`]. The recorder
//! stores it and writes it on the spawn event as `bound`. It is not an act
//! annotation: an act carries `permit`, and the effect is projected from the
//! tool tag.

use serde::{Deserialize, Serialize};

use super::tag::{FileFacet, MemoryFacet, Permit, ToolTag};

/// Who an agent may address. `Any` is wider than `Parent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Audience {
    #[default]
    Parent,
    Any,
}

/// Signal face of a charter. Cancel is not granted here: a parent may always
/// cancel its child, and a child may not cancel anyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    pub complete: Permit,
    pub audience: Audience,
}

impl Default for Signal {
    fn default() -> Self {
        Self {
            complete: Permit::Go,
            audience: Audience::Parent,
        }
    }
}

/// Ceiling for one spawned worker. `tag` bounds permit, file, and memory.
/// `signal` bounds `complete` and who the worker may address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Charter {
    #[serde(flatten)]
    pub tag: ToolTag,
    #[serde(default)]
    pub signal: Signal,
}

impl Charter {
    pub fn new(tag: ToolTag) -> Self {
        Self {
            tag,
            signal: Signal::default(),
        }
    }

    pub fn with_signal(mut self, signal: Signal) -> Self {
        self.signal = signal;
        self
    }

    /// Lite spawn requires a ceiling that names no file path.
    pub fn file_is_none(&self) -> bool {
        matches!(self.tag.file, FileFacet::None)
    }

    /// Each card grant must fit this ceiling. Signal is not a grant face.
    pub fn allows(&self, tag: &ToolTag) -> bool {
        permit_fits(tag.permit, self.tag.permit)
            && file_within(&tag.file, &self.tag.file)
            && memory_allows(&self.tag.memory, &tag.memory)
    }

    /// `self` is no wider than `parent` on permit, file, memory, and signal.
    pub fn within(&self, parent: &Charter) -> bool {
        permit_fits(self.tag.permit, parent.tag.permit)
            && signal_within(self.signal, parent.signal)
            && file_within(&self.tag.file, &parent.tag.file)
            && memory_within(&self.tag.memory, &parent.tag.memory)
    }
}

/// Spawn fit only. Execution does not rank permits: `Ask` runs, and only
/// `Deny` blocks a tool.
fn permit_fits(grant: Permit, ceiling: Permit) -> bool {
    permit_rank(grant) <= permit_rank(ceiling)
}

fn permit_rank(permit: Permit) -> u8 {
    match permit {
        Permit::Deny => 0,
        Permit::Ask => 1,
        Permit::Go => 2,
    }
}

fn signal_within(child: Signal, parent: Signal) -> bool {
    permit_fits(child.complete, parent.complete)
        && (child.audience != Audience::Any || parent.audience == Audience::Any)
}

/// `child` is no wider than `parent`. `None` fits anywhere. `Unbounded`
/// fits only an unbounded parent. A path fits when it is under the parent
/// path and the op is no wider. The `recursive` flag is not part of the fit.
fn file_within(child: &FileFacet, parent: &FileFacet) -> bool {
    if matches!(child, FileFacet::None) || matches!(parent, FileFacet::Unbounded) {
        return true;
    }
    let (Some(root), Some(path)) = (facet_path(parent), facet_path(child)) else {
        return false;
    };
    path_under(root, path) && op_within(child, parent)
}

fn op_within(child: &FileFacet, parent: &FileFacet) -> bool {
    matches!(
        (parent, child),
        (
            FileFacet::ReadWrite { .. },
            FileFacet::Read { .. } | FileFacet::Write { .. } | FileFacet::ReadWrite { .. }
        ) | (FileFacet::Read { .. }, FileFacet::Read { .. })
            | (FileFacet::Write { .. }, FileFacet::Write { .. })
    )
}

fn facet_path(face: &FileFacet) -> Option<&str> {
    match face {
        FileFacet::Read { path, .. }
        | FileFacet::Write { path, .. }
        | FileFacet::ReadWrite { path, .. } => Some(path),
        FileFacet::None | FileFacet::Unbounded => None,
    }
}

fn path_under(root: &str, path: &str) -> bool {
    if root.is_empty() {
        return !path.starts_with('/');
    }
    path == root || path.starts_with(&format!("{root}/"))
}

fn memory_within(child: &MemoryFacet, parent: &MemoryFacet) -> bool {
    match (parent, child) {
        (MemoryFacet::Ignore, MemoryFacet::Ignore) => true,
        (MemoryFacet::Remember { aspect: None }, MemoryFacet::Remember { .. }) => true,
        (
            MemoryFacet::Remember {
                aspect: Some(bound),
            },
            MemoryFacet::Remember {
                aspect: Some(grant),
            },
        ) => bound == grant,
        (MemoryFacet::Forget { .. }, MemoryFacet::Forget { .. }) => true,
        _ => false,
    }
}

fn memory_allows(bound: &MemoryFacet, tag: &MemoryFacet) -> bool {
    match (bound, tag) {
        (
            MemoryFacet::Forget {
                remember_id: bound_id,
            },
            MemoryFacet::Forget { remember_id },
        ) => bound_id == remember_id,
        _ => memory_within(tag, bound),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ceiling(file: FileFacet, permit: Permit, memory: MemoryFacet) -> Charter {
        Charter::new(ToolTag {
            file,
            permit,
            memory,
        })
    }

    fn none(permit: Permit) -> Charter {
        ceiling(FileFacet::None, permit, MemoryFacet::Ignore)
    }

    fn write(path: &str, recursive: bool) -> FileFacet {
        FileFacet::Write {
            path: path.into(),
            recursive,
        }
    }

    #[test]
    fn permit_ceiling_rejects_a_wider_grant() {
        let bound = none(Permit::Deny);
        assert!(!bound.allows(&ToolTag::none_permit(Permit::Go)));
        assert!(bound.allows(&ToolTag::none_permit(Permit::Deny)));
        assert!(none(Permit::Go).allows(&ToolTag::none_permit(Permit::Ask)));
        assert!(none(Permit::Go).allows(&ToolTag::none_permit(Permit::Deny)));
        assert!(!none(Permit::Ask).allows(&ToolTag::none()));
    }

    #[test]
    fn file_none_rejects_a_path_and_unbounded_fits_any_file() {
        let tight = none(Permit::Go);
        assert!(!tight.allows(&ToolTag::read("/src/a.txt")));
        assert!(tight.allows(&ToolTag::none()));
        let wide = ceiling(FileFacet::Unbounded, Permit::Go, MemoryFacet::Ignore);
        assert!(wide.allows(&ToolTag::write("/src/a.txt")));
        assert!(tight.within(&wide));
        assert!(!wide.within(&tight));
    }

    #[test]
    fn path_fit_uses_prefix_and_op_and_ignores_recursive() {
        let bound = ceiling(write("/src", false), Permit::Go, MemoryFacet::Ignore);
        let nested = ToolTag {
            file: write("/src/a.txt", true),
            permit: Permit::Go,
            memory: MemoryFacet::Ignore,
        };
        assert!(bound.allows(&nested));
        assert!(bound.allows(&ToolTag::none()));
        let outside = ToolTag {
            file: write("/other", false),
            permit: Permit::Go,
            memory: MemoryFacet::Ignore,
        };
        assert!(!bound.allows(&outside));
        assert!(!bound.allows(&ToolTag::read("/src/a.txt")));
        let rw = ceiling(
            FileFacet::ReadWrite {
                path: "/src".into(),
                recursive: false,
            },
            Permit::Go,
            MemoryFacet::Ignore,
        );
        assert!(rw.allows(&ToolTag::read("/src/a.txt")));
        assert!(!rw.allows(&ToolTag::unbounded()));
    }

    #[test]
    fn signal_and_memory_ceilings() {
        let parent = none(Permit::Go).with_signal(Signal {
            complete: Permit::Deny,
            audience: Audience::Parent,
        });
        let louder = none(Permit::Go).with_signal(Signal {
            complete: Permit::Go,
            audience: Audience::Parent,
        });
        assert!(!louder.within(&parent));
        let any = none(Permit::Go).with_signal(Signal {
            complete: Permit::Deny,
            audience: Audience::Any,
        });
        assert!(!any.within(&parent));
        let same = parent.clone();
        assert!(same.within(&parent));

        let forget = ceiling(
            FileFacet::None,
            Permit::Go,
            MemoryFacet::Forget {
                remember_id: "rem-1".into(),
            },
        );
        let same_id = ToolTag {
            file: FileFacet::None,
            permit: Permit::Go,
            memory: MemoryFacet::Forget {
                remember_id: "rem-1".into(),
            },
        };
        let other_id = ToolTag {
            file: FileFacet::None,
            permit: Permit::Go,
            memory: MemoryFacet::Forget {
                remember_id: "rem-2".into(),
            },
        };
        assert!(forget.allows(&same_id));
        assert!(!forget.allows(&other_id));
        assert!(!forget.allows(&ToolTag::none()));
        let other = ceiling(
            FileFacet::None,
            Permit::Go,
            MemoryFacet::Forget {
                remember_id: "rem-2".into(),
            },
        );
        assert!(other.within(&forget));
        assert!(!none(Permit::Go).within(&forget));
        assert!(!forget.within(&none(Permit::Go)));
    }
}
