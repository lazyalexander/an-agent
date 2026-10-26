# Delivery authority and landing evidence

Delivery admission already binds the registration digest and generation, agent
incarnation and mounted spool instance. Publication now revalidates all four.
Changing registration and then restoring identical bytes does not restore an old
grant: its generation is different. An identical consecutive registration keeps
its generation and does not invalidate a reply.

## Publication boundary

After a beat body returns, or an agent body yields its final utterance, validation
and the spool note/intent admission share the existing supervision mutex with
registration changes, unmount and release. The check and publication are one
critical section. The same fence covers a halt note. No user body, hook, model
call or external callback executes under this critical section. Local tape I/O
remains inside it to preserve ordering.

A stale delivery returns `ReviewStale` and writes `delivery_stale` audit evidence;
it is not a body failure and does not automatically unmount, retry or reschedule
the body. Earlier valid model observations remain retained. After hooks may run
arbitrary host code, so offer/dispatch validate a reply again after those hooks.
A note published before revocation remains history; a later check cannot erase it.

## Host boundary

`SpoolReply` retains private admission facts. Hosts call `validate_reply` before
executing an intent. The included pump checks each Utter and Raise, and the CLI,
build and model-output paths check before starting their admitted action or
publishing the response artifact. The facts are in-memory authorization context,
not a signed transferable token or a new persistent replay format. Callers must
obtain replies through delivery rather than construct struct literals.

`applied` records a host claim about something already landed; it is not the API
that performs the external effect. It revalidates under the supervision lock.
Current claims keep the `applied` tag. A stale claim writes `applied_stale` with
its original facts, spool-note reference, host note and reason, then returns an
error. Evidence write failures propagate; no automatic retry or success claim is
synthesized. The pump records how many intents completed when revocation stops a
partially executed reply.

A preflight check is not an atomic transaction with an arbitrary application.
Revocation can occur after a check or during a sink callback. Already completed
effects cannot be rolled back by core: the host must preserve/report them, stop
remaining work and use its own revision checks or transaction boundary where
needed. This slice does not introduce application-specific rollback, a global
external-operation lock or cross-process workspace coordination.

## Recovery and verification

A reopened session has a new incarnation, and remounting has a new mount ID.
Retained old replies cannot be confirmed against those new identities, even if
registration bytes are unchanged. Fresh delivery is required; old tape history
and model blobs remain readable and are not rewritten into new grants.

Tests cover a blocked beat during registration replacement, replacement after
model completion, after-hook revocation, identical versus ABA registration,
missing facts, reopen/remount, failed landing-evidence writes, and a real host
pump whose sink revokes authority between an Utter and a Raise. The latter keeps
the visible effect and a partial stale claim while refusing the remaining Raise.
