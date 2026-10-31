# an-agent-host

Host-side authorization, execution supervision, model/context exchange, committed
state and artifact custody. Domain rules belong in spool releases; Loom owns
building and packaging. Backends enforce each attempt boundary.

```sh
cargo install --path crates/an-agent-host --locked
```

Start with the [documentation index](../../doc/README.md) and
[current support matrix](../../doc/platforms.md). Package checking is not execution
authority. Unknown termination never authorizes replay, acceptance or cleanup.

| Task | Contract |
| --- | --- |
| Load, edit, run and control a native spool | [Native runtime](../../doc/native-spool.md) |
| Select state or review releases | [Native state](../../doc/native-state.md) |
| Route fixed products and dispatch finite batches | [Events](../../doc/native-events.md) |
| Inspect ownership and collect private material | [Storage](../../doc/native-storage.md) |
| Register a bounded stdio executable | [Deployment](../../doc/native-deployment.md) |
| Build or run reviewed operator workflows | [Execution profiles](../../doc/workspace-jobs.md) |
| Version products and mediate external reads | [Artifacts](../../doc/artifacts.md) |
| Commit instance state and track recipients | [Checkpoints](../../doc/checkpoints.md) |
| Use model/context services or finite coding tools | [Services](../../doc/model-services.md) |
| Use the optional OpenShell backend | [OpenShell](../../doc/openshell.md) |

`execution/` owns admission, backend coordination and settlement; `native_spool/`
owns the release-bound facade. `workspace_job/`, `artifact_versions/` and `instance/`
keep scratch, accepted products and explicit state distinct. `Host::pump` retains
its embedded event semantics; a subprocess is not run inside an infallible sink.

Run the repository gates in [AGENTS.md](../../AGENTS.md). Real CLI acceptance
entry points and fixture ownership are listed in [tests/cli](tests/cli/README.md).
