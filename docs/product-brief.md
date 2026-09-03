# Product brief

## Mission

Produce a canonical change stream while explicitly reporting when native
filesystem notifications cannot guarantee completeness.

## MVP commands

~~~
fsproof inspect PATH
fsproof verify PATH --duration 10s
fsproof watch PATH --format jsonl
~~~

The CLI also accepts JSON output for inspect and verify, a polling backend,
poll interval, and bounded snapshot limits.

## Core design

The library combines:

1. A native filesystem event stream when the selected platform provides one.
2. A periodic snapshot of bounded metadata.
3. State reconciliation between observations.
4. A canonical Created, Removed, Modified, or Renamed event stream.

The polling backend uses the same bounded snapshot and reconciliation model
without a native event source.

## Required behavior

- Report backend capabilities and limitations.
- Detect likely network or virtual filesystem conditions when the host exposes
  enough mount information to classify them.
- Never claim completeness without stating the guarantee.
- Reconcile missed and duplicate events.
- Bound snapshot cost by entry count and metadata byte budget.
- Expose terminal text and JSON Lines output.
- Preserve diagnostic evidence for rescan conditions and unconfirmed events.

## Automated validation boundary

The local test suite covers regular local trees, temporary filesystems when
available, rename and duplicate-event traces, permission and symlink metadata,
polling, bounded snapshots, and deterministic replay. The test suite records
which optional mounts exist on the host. It does not claim NFS, WSL, or remote
behavior without a matching environment.

The release gate also runs a bounded parser fuzz target and the independent
artifact smoke checks described in docs/release.md.

## Non-goals

fsproof is not a file-restart application, a hosted service, a cloud backend,
a durable event queue, or a cross-host protocol. It does not promise
completeness on every filesystem and does not add a frontend.
