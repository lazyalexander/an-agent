//! Documentation inventory and budgets; never reads ignored private notes.
use crate::Result;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    max_pages: usize,
    max_page_lines: usize,
    max_readme_lines: usize,
    max_agent_lines: usize,
}
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git").current_dir(root).args(args).output()?;
    if !out.status.success() {
        return Err(format!(
            "documentation inventory failed: {}",
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(out.stdout)
}
fn previous(root: &Path, base: &str) -> Result<Option<Budget>> {
    let files = git(
        root,
        &["ls-tree", "--name-only", base, "documentation-policy.toml"],
    )?;
    if files.is_empty() {
        return Ok(None);
    }
    let bytes = git(
        root,
        &["show", &format!("{base}:documentation-policy.toml")],
    )?;
    Ok(Some(toml::from_str(std::str::from_utf8(&bytes)?)?))
}

pub fn inspect(root: &Path, base: &str) -> Result<()> {
    let config = root.join("documentation-policy.toml");
    let current: Option<Budget> = if config.try_exists()? {
        Some(toml::from_str(&fs::read_to_string(config)?)?)
    } else {
        None
    };
    let budget = current.as_ref();
    let prior = previous(root, base)?;
    let Some(budget) = budget else {
        if prior.is_some() {
            return Err("documentation policy cannot be removed".into());
        }
        return Ok(()); // Historical checkouts before the documentation policy.
    };
    let caps = |b: &Budget| {
        [
            b.max_pages,
            b.max_page_lines,
            b.max_readme_lines,
            b.max_agent_lines,
        ]
    };
    if caps(budget).contains(&0) {
        return Err("documentation budgets must be positive".into());
    }
    if prior
        .as_ref()
        .is_some_and(|p| caps(budget).iter().zip(caps(p)).any(|(n, o)| *n > o))
    {
        return Err(
            "documentation budget weakening requires a separately reviewed policy change".into(),
        );
    }
    let entries = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let files: BTreeSet<PathBuf> = entries
        .split(|b| *b == 0)
        .filter(|b| !b.is_empty())
        .map(|b| std::str::from_utf8(b).map(PathBuf::from))
        .collect::<std::result::Result<_, _>>()?;
    let files: BTreeSet<_> = files
        .into_iter()
        .filter(|p| root.join(p).exists())
        .collect();
    let mut pages = 0;
    for path in &files {
        if !path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
        {
            continue;
        }
        if fs::symlink_metadata(root.join(path))?
            .file_type()
            .is_symlink()
        {
            return Err(format!("documentation symlink: {}", path.display()).into());
        }
        let limit = if path == Path::new("AGENTS.md") {
            budget.max_agent_lines
        } else if path.file_name().is_some_and(|n| n == "README.md") {
            budget.max_readme_lines
        } else if path.starts_with("doc") {
            pages += 1;
            budget.max_page_lines
        } else {
            return Err(format!(
                "documentation must live in doc/ or README.md: {}",
                path.display()
            )
            .into());
        };
        let text = fs::read_to_string(root.join(path))?;
        if text.lines().count() > limit {
            return Err(format!(
                "documentation line budget {limit} exceeded: {}",
                path.display()
            )
            .into());
        }
        for target in targets(&text) {
            if let Some(local) = local_target(path, &target)?
                && !files.contains(&local)
                && !files.iter().any(|p| p.starts_with(&local))
            {
                return Err(format!(
                    "missing published documentation link: {} -> {target}",
                    path.display()
                )
                .into());
            }
        }
    }
    if pages > budget.max_pages {
        return Err(format!(
            "documentation page budget exceeded: {pages}/{}",
            budget.max_pages
        )
        .into());
    }
    println!(
        "documentation: {pages}/{} topic pages; placement, length and local file links checked",
        budget.max_pages
    );
    Ok(())
}
fn local_target(source: &Path, target: &str) -> Result<Option<PathBuf>> {
    let target = target.trim().trim_matches(['<', '>']);
    if target.starts_with('#') || target.contains("://") || target.starts_with("mailto:") {
        return Ok(None);
    }
    let file = target.split(['#', '?']).next().unwrap_or("");
    if file.is_empty() {
        return Ok(None);
    }
    if file.starts_with('/') || file.contains(':') {
        return Err(format!("nonportable documentation link: {target}").into());
    }
    let mut path = source.parent().unwrap_or(Path::new("")).to_path_buf();
    for part in Path::new(file).components() {
        match part {
            Component::CurDir => (),
            Component::ParentDir => {
                if !path.pop() {
                    return Err(format!("documentation link escapes repository: {target}").into());
                }
            }
            Component::Normal(p) => path.push(p),
            _ => return Err("invalid documentation path".into()),
        }
    }
    Ok(Some(path))
}
// Repository documentation uses inline links and reference definitions. Fenced
// examples are not navigation. This is a file-link gate, not a Markdown renderer.
fn targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fence: Option<char> = None;
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with("```") || line.starts_with("~~~") {
            let ch = line.chars().next().unwrap();
            if fence == Some(ch) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(ch);
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let mut rest = line;
        while let Some((_, tail)) = rest.split_once("](") {
            let Some((value, next)) = tail.split_once(')') else {
                break;
            };
            out.push(value.split_whitespace().next().unwrap_or("").to_owned());
            rest = next;
        }
        if line.starts_with('[')
            && let Some((_, value)) = line.split_once("]:")
        {
            out.push(value.split_whitespace().next().unwrap_or("").to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests;
