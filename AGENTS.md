# an-agent — working agreements

This file is the discipline every agent working in this repo follows. It
is shorter than the design docs on purpose: it covers how code moves,
not what the code means. For the design contracts see the docs repo
(`~/docs/an-agent/`), especially `workspace.md` and `constraints.md`.

## Branch lines

- `main` — stable. Receives only batch merges from `dev-advance`, opened
  as a PR and merged by the user.
- `dev-advance` — the sole development authority. Every feature PR
  targets it. Never branch new work off `main`.
- `dev-*` — feature branches. Local-first; push only when a PR needs a
  head.

Rules:

- The user clicks merge. Always. Open the PR, state what it carries,
  stop.
- PR base is `dev-advance`, not another feature branch. If a follow-up
  depends on an unmerged PR, note "stacks on #N" in the body — and the
  moment #N merges, retarget the base (`gh pr edit M --base
  dev-advance`). Merging a stacked PR into its stacked base strands the
  work one branch off the authority line; this accident has happened
  twice (#42, #44) and each time cost a sync PR.
- Remote hygiene: the remote holds `main`, `dev-advance`, and in-flight
  PR heads only. Delete a head when its PR merges
  (`git push origin --delete <branch>`). Local branches may be kept
  freely.
- After any merge you expected to land on `dev-advance`, verify with
  `git log origin/dev-advance --oneline -5` that it actually did. Do
  not trust the PR page.

## The asset test (what may travel toward main)

A case earns its place on the main line by what it leaves behind when
it dies:

- **Asset** — leaves runtime machinery, published spool bodies, and
  standard `cargo test` coverage that exercises the real kernel path
  (AgentControl, register, mount match, dispatch, tape). Assets ride
  feature PRs into `dev-advance` and later `main`. Example:
  `an-agent-spool/tests/discord_parley.rs` — its scripted outbox can be
  swapped for a real bridge without touching the chain it proves.
- **Probe** — leaves only scaffolding: a parallel harness, a duplicate
  mount path, one-off glue. Probes must never reach the main line.
  Example: the retired `an-agent-probes` crate — it ran beside the
  kernel instead of through it, and deleting it cost nothing.

When unsure: if deleting it tomorrow would cost the repo a capability
the kernel honors, it is an asset; if it would only cost a way to
watch, it belongs to the lab.

## The lab (experiments and observation tooling)

- Experiments and observation/auxiliary tooling live in dedicated
  packages under the top-level `lab/` directory, which is gitignored —
  they never merge to `main`.
- Lab crates path-depend on workspace crates; workspace crates never
  depend on lab crates. One-way dependencies, so the lab can always be
  deleted without touching the product.
- Lab code may be dirty. It is not reviewed to product standard and not
  pushed to remote by default — pushing lab work is a deliberate,
  stated decision.
- The lab is not a workspace member, so a `dev-advance → main` merge
  can never carry it.
- Promotion is re-implementation, not merge: rewrite the proven idea
  clean in the proper crate with tests to product standard, then delete
  the lab original.

## Settling a batch into main

1. All feature PRs of the batch merged to `dev-advance`; the line runs
   clean (see verification below).
2. Open one PR `dev-advance → main` stating the batch contents. The
   user merges.
3. After merge, confirm `origin/main` and `origin/dev-advance` agree,
   and prune the batch's remote heads.

## Verification before every commit

1. `cargo test --workspace` — green.
2. `cargo clippy --workspace --all-targets` — zero warnings.
3. `cargo fmt --all` — applied.

Plus the discipline-specific checks:

- Does this change belong to the product, a case asset, or the lab? Put
  it in exactly one of the three.
- Is the PR base `dev-advance`?
- Module splits follow the split discipline (see the `split-modules`
  skill): a split commit is a pure move, public paths stay, tests are
  moved not lost.
