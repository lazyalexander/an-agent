use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    File,
    Tree,
    Agent,
    Net,
    Cmd,
    Code,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Tree => "tree",
            Self::Agent => "agent",
            Self::Net => "net",
            Self::Cmd => "cmd",
            Self::Code => "code",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "file" => Self::File,
            "tree" => Self::Tree,
            "agent" => Self::Agent,
            "net" => Self::Net,
            "cmd" => Self::Cmd,
            "code" => Self::Code,
            _ => return None,
        })
    }
}

/// Mnemonic for an object inside a workplace. Content identity is ObjectId, not this.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Resource {
    pub workplace: Uuid,
    pub kind: ResourceKind,
    pub path: Vec<String>,
}

impl Resource {
    pub fn new(
        workplace: Uuid,
        kind: ResourceKind,
        path: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            workplace,
            kind,
            path: path.into_iter().map(Into::into).collect(),
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        let mut parts = s.split('/');
        let wp = parts
            .next()
            .ok_or("empty resource")?
            .parse::<Uuid>()
            .map_err(|_| "workplace id must be a uuid")?;
        let kind = ResourceKind::parse(parts.next().ok_or("missing kind")?)
            .ok_or("unknown resource kind")?;
        let path: Vec<String> = parts
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect();
        Ok(Self {
            workplace: wp,
            kind,
            path,
        })
    }
}

impl std::fmt::Display for Resource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.workplace, self.kind.as_str())?;
        for p in &self.path {
            write!(f, "/{p}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonic_round_trips() {
        let wp = Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
        let r = Resource::new(wp, ResourceKind::File, ["src", "main.rs"]);
        let s = r.to_string();
        assert_eq!(s, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa/file/src/main.rs");
        assert_eq!(Resource::parse(&s).unwrap(), r);
    }
}
