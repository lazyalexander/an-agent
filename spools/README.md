# spools — the spool library

The library of published spool bodies, one directory per spool. A spool is
a YAML grant envelope plus the script it wraps: authors edit them as
separate files, and the body names its script with `script: "@file.rhai"`.

Rules of the shelf:

- The published body is self-contained: loading inlines the script
  (`an_agent_spool::library::load`), so the registry hashes one document
  and never chases files. Separation is for authors, not for the registry.
- Script refs are clean relative paths — no absolutes, no `..`, no `~`.
- Names are lowercase snake/kebab, versions are exact `x.y.z`, and a
  published (name, version) never changes: a fix is a new version.
- `requires` entries must already be published — publish order is
  topological.
- Scripts see a tape projection (clip ids + previews) and mount config
  (`config`); they yield continuations (`halt / utter / invoke_model /
  invoke_tool / approve / silk`); they never hold full text, handles, or
  secrets.

Current entries:

- `agent_loop/` — the thin loop as a policy: model proposes (bounded to
  the requires whitelist), the policy approves by reference.
- `echo/` — the trivial host-constructor spool, the library's hello world.
