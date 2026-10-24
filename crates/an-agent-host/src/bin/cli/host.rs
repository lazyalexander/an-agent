use super::{Path, Result, Value};
pub(super) fn dispatch(command: &str, args: &[String]) -> Result<Value> {
    match (command, args) {
        ("check", [folder]) => Ok(an_agent_host::execution::check(Path::new(folder))?),
        ("register", [folder, store]) => Ok(an_agent_host::execution::register(
            Path::new(folder),
            Path::new(store),
        )?),
        ("run", [store, release, input]) => super::with_cancellation(|cancelled| {
            an_agent_host::execution::run_cancellable(
                Path::new(store),
                release,
                Path::new(input),
                false,
                &cancelled,
            )
        }),
        ("run", [store, release, input, flag]) if flag == "--execute" => {
            super::with_cancellation(|cancelled| {
                an_agent_host::execution::run_cancellable(
                    Path::new(store),
                    release,
                    Path::new(input),
                    true,
                    &cancelled,
                )
            })
        }
        ("run-version", [store, release, catalog, revision]) => {
            super::with_cancellation(|cancelled| {
                an_agent_host::execution::run_revision(
                    Path::new(store),
                    release,
                    Path::new(catalog),
                    revision,
                    false,
                    &cancelled,
                )
            })
        }
        ("run-version", [store, release, catalog, revision, flag]) if flag == "--execute" => {
            super::with_cancellation(|cancelled| {
                an_agent_host::execution::run_revision(
                    Path::new(store),
                    release,
                    Path::new(catalog),
                    revision,
                    true,
                    &cancelled,
                )
            })
        }
        ("run-openshell", [profile, store, release, input]) => {
            super::with_cancellation(|cancelled| {
                an_agent_host::execution::run_openshell(
                    Path::new(profile),
                    Path::new(store),
                    release,
                    Path::new(input),
                    false,
                    &cancelled,
                )
            })
        }
        ("run-openshell", [profile, store, release, input, flag]) if flag == "--execute" => {
            super::with_cancellation(|cancelled| {
                an_agent_host::execution::run_openshell(
                    Path::new(profile),
                    Path::new(store),
                    release,
                    Path::new(input),
                    true,
                    &cancelled,
                )
            })
        }
        ("capability-read", [store, run, catalog, release]) => {
            super::with_cancellation(|cancelled| {
                an_agent_host::capability::artifact_read(
                    Path::new(store),
                    run,
                    Path::new(catalog),
                    release,
                    None,
                    &cancelled,
                )
            })
        }
        ("capability-read", [store, run, catalog, release, flag, digest])
            if flag == "--approve" =>
        {
            super::with_cancellation(|cancelled| {
                an_agent_host::capability::artifact_read(
                    Path::new(store),
                    run,
                    Path::new(catalog),
                    release,
                    Some(digest),
                    &cancelled,
                )
            })
        }
        ("build-kit", [source, toolchain, output]) => super::with_cancellation(|cancelled| {
            an_agent_host::file_job::build(
                Path::new(source),
                Path::new(toolchain),
                Path::new(output),
                None,
                &cancelled,
            )
        }),
        ("build-kit", [source, toolchain, output, flag, digest]) if flag == "--approve" => {
            super::with_cancellation(|cancelled| {
                an_agent_host::file_job::build(
                    Path::new(source),
                    Path::new(toolchain),
                    Path::new(output),
                    Some(digest),
                    &cancelled,
                )
            })
        }
        _ => Err(format!("invalid arguments for {command}").into()),
    }
}
