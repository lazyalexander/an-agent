"""Eligibility for the existing real-host acceptance profile, not runtime authority.

This check selects a qualified test environment. The host must still install its
actual policy and confirm termination; OS identity alone proves neither.
"""
import platform

NATIVE_WORKSPACE_REASON = "Real macOS native workspace enforcement required; no fallback"


def native_workspace_available():
    return platform.system() == "Darwin"


def require_native_workspace():
    if not native_workspace_available():
        raise SystemExit(NATIVE_WORKSPACE_REASON)
