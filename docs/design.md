# Design

## Input boundary

Each operation accepts one existing path. A directory is traversed
recursively. A regular file is represented as one entry. Metadata reads use
symlink metadata, so symlink targets are recorded without following the link.
Directory entries are sorted before traversal to make snapshots deterministic.

## Bounded snapshots

Every snapshot has a maximum entry count and a maximum metadata byte budget.
The defaults are 100,000 entries and 256 MiB. A limit error stops the
operation before an unbounded tree can consume memory. The snapshot includes
relative path, entry kind, file size, modified timestamp, permissions, and
symlink target where available.

## Fingerprints and reconciliation

The snapshot index is a sorted map keyed by relative path. A fingerprint
change yields Modified. A path present only in the after snapshot yields
Created. A path present only in the before snapshot yields Removed.

When one removed entry and one created entry have the same fingerprint, the
pair is represented as Renamed. If a trace contains repeated native records,
records are keyed and deduplicated. Native paths are matched against the
snapshot diff. An event that cannot be confirmed is retained as a diagnostic
and prevents an unconditional ok status.

## Notification backends

The native backend uses the notify crate's recommended watcher on the target
platform. On Linux this is normally inotify. Native events are watched
recursively and normalized into the internal event record. notify rescan
markers are treated as evidence that the native stream may have missed
events.

The polling backend uses notify's PollWatcher and the configured interval.
When native watcher setup or watch registration fails, fsproof attempts the
polling backend and records a degraded diagnostic. A polling fallback is
never presented as a native completeness guarantee.

Polling notifications can wake the reconciliation loop, but periodic
snapshots are the authority in polling mode. PollWatcher records are not
counted as native events, which avoids turning normal notification ordering
into an unconfirmed duplicate after a snapshot has already observed a change.

## Filesystem classification

On Linux, fsproof reads process mount information and selects the longest
matching mount point for the requested path. It reports a best-effort class
such as local, network, overlay, pseudo, virtual, or unknown. Other platforms
report the available platform label and an unknown classification when the
host does not expose a portable mount source.

Classification is descriptive. It does not prove that a filesystem is local
or that notifications are complete.

## Output and status

Inspect returns snapshot and backend metadata. Verify returns before and
after snapshots, observed normalized events, reconciliation events, and
diagnostics. Watch emits a JSON Lines ready record followed by event or
diagnostic records and returns a summary to the caller.

Schema version 1 is included in machine-readable records. Status ok is
reserved for an operation that completed without a degraded diagnostic or
unconfirmed native event. The guarantee field remains best effort in all MVP
reports.
