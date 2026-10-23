//! Thin fail-closed project gate over the pinned SPACE checker.
mod base;
mod policy;
use pplx_space_archcheck::{
    Context, Mode, check,
    report::{Format, render},
};
use std::{error::Error, path::Path};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn run() -> Result<()> {
    let mut root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--root") && args.len() >= 2 {
        root = Path::new(&args[1]).canonicalize()?;
        args.drain(..2);
    }
    let base = match args.as_slice() {
        [] => "origin/dev-advance",
        [flag, value] if flag == "--base" => value,
        _ => return Err("usage: cargo arch [--root PATH] [--base COMMIT]".into()),
    };
    let policy = policy::load(&root)?;
    // Upstream permits absent baselines; this project's gate does not.
    if !root.join("architecture-baseline.toml").is_file() {
        return Err("missing architecture-baseline.toml".into());
    }
    let mut ctx = Context::load(&root)?;
    let scratch = tempfile::tempdir()?;
    base::attach(
        &root,
        base,
        &policy.bootstrap_base,
        scratch.path(),
        &mut ctx,
    )?;
    let findings = check(&ctx);
    print!("{}", render(Format::Human, &findings, ctx.files.len()));
    let skipped = pplx_space_archcheck::skipped(&ctx);
    if !skipped.is_empty() {
        return Err(format!("incomplete comparison: {}", skipped.join("; ")).into());
    }
    policy::inspect(&root, &policy, &ctx)?;
    if findings.iter().any(|f| f.mode == Mode::Required) {
        return Err("required architecture findings".into());
    }
    Ok(())
}
fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("architecture gate: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
