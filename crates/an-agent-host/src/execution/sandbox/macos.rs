//! macOS-specific Seatbelt policy and launch command; no Linux translation.
use super::Result;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;

pub(crate) fn quoted(path: &Path) -> Result<String> {
    let text = path.to_str().ok_or("sandbox path must be UTF-8")?;
    // SBPL string syntax must not be injectable through user directory names.
    if text.chars().any(|c| c.is_control()) {
        return Err("control characters in sandbox path".into());
    }
    Ok(format!(
        "\"{}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

pub fn profile(executable: &Path) -> Result<String> {
    Ok(format!(
        r#"(version 1)
(deny default)
(allow process-exec (literal {executable}))
; dyld opens the root vnode. This is not recursive root read access.
(allow file-read* (literal "/"))
(allow file-read* (literal {executable}))
(allow file-read* (subpath "/System/Library") (subpath "/usr/lib")
    (literal "/dev/urandom") (literal "/dev/random"))
(allow sysctl-read)
"#,
        executable = quoted(executable)?
    ))
}

#[cfg(target_os = "macos")]
pub fn command(executable: &Path) -> Result<Command> {
    let policy = profile(executable)?;
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command.args(["-p", &policy]).arg(executable);
    Ok(command)
}
