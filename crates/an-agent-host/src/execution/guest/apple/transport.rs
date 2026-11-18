//! Kit: bounded host CLI calls; a CLI exit alone does not prove guest termination.
use super::Profile;
use crate::execution::{Result, sandbox};

pub(super) fn call(profile: &Profile, args: &[&str], seconds: u64) -> Result<Vec<u8>> {
    call_with_input(profile, args, b"", seconds)
}

pub(super) fn call_with_input(
    profile: &Profile,
    args: &[&str],
    input: &[u8],
    seconds: u64,
) -> Result<Vec<u8>> {
    profile.validate()?;
    if input.len() > 32768 {
        return Err("Apple Container request frame limit".into());
    }
    let result =
        sandbox::apple_container::call(&profile.cli, &profile.ledger, args, input, seconds)?;
    if result.status != "succeeded" || result.code != Some(0) || !result.reaped {
        return Err(
            "Apple Container transport failed; inspect original operation, never replay".into(),
        );
    }
    Ok(result.stdout)
}
