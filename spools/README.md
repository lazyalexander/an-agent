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
- `consumes` / `produces` are required (use `[]`): the spool's claim about
  workspace events. They are matched against the workspace register at
  mount (`AgentControl::mount_spool`), in both directions — consumed
  events must be registered and routed to the spool, produced events must
  be registered, and a route naming the spool must be consumed by it.
- Scripts see a tape projection (clip ids + previews) and mount config
  (`config`); they yield continuations (`halt / utter / invoke_model /
  invoke_tool / approve`); they never hold full text, handles, or
  secrets.
- `sort` (optional, default `beat`) names the kind of mounted body:
  - **beat** — one workspace event in, a string reply out
    (`an_agent_spool::beat`). Beats are compiled at mount: a syntax error
    fails the mount, never the first event. Authoring gotcha: rhai's
    `trim()` mutates in place and returns unit — don't use it in
    expressions.
  - **policy** — an agent body: tape projection in, continuation out
    (`an_agent_spool::policy`), driven step by step by core; every model
    call is admitted against the thread's card and taped as its own
    anchored action + observation. On the delivery path `approve` is
    refused (it belongs to the thread loop) and `invoke_tool` is not
    admitted yet.
  - **gate** — hook-chain machinery, built by the host's hook wiring
    (`HostHooks`); refused at mount.

Current entries:

- `agent_loop/` — the thin loop as a policy: model proposes (bounded to
  the requires whitelist), the policy approves by reference.
- `echo/` — the trivial host-constructor spool, the library's hello world.
- `chat_npc/` — a discord character (rhai beat): answers one
  `discord.message` in persona, replies become `discord.post`. One body,
  many mounts — the persona is mount config.
- `discord/` — the channel envelope (host constructor): consumes
  `discord.post` to send, produces inbound `discord.message`, net
  egress. The scripted parley case (`an-agent-spool/tests/
  discord_parley.rs`) stands in for the real bridge.
