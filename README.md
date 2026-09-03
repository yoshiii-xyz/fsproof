# fsproof

Filesystem event correctness layer.

Status: pre-release validation in progress. The target MVP release is
v0.1.0.

## Install

From a checkout:

~~~
cargo install --path . --locked
~~~

## Quick start

Inspect a tree and report the bounded snapshot plus backend information:

~~~
fsproof inspect PATH
fsproof inspect PATH --format json
~~~

Verify a finite interval using native notifications or polling:

~~~
fsproof verify PATH --duration 10s
fsproof verify PATH --duration 10s --backend poll --format json
~~~

Stream ready records and canonical changes as JSON Lines:

~~~
fsproof watch PATH --format jsonl
fsproof watch PATH --backend poll --poll-interval 250ms --format jsonl
~~~

## What it solves

Filesystem notifications are useful, but a native event stream does not by
itself prove that every change was observed. fsproof combines a notification
backend with bounded snapshots and reconciliation. It reports a canonical
stream of Created, Removed, Modified, and Renamed changes while keeping the
completeness guarantee explicit.

The MVP is a Rust library and CLI for local technical workflows. It does not
run a service, send telemetry, or retain a durable event queue.

## How it works

fsproof records bounded metadata for a file or directory tree without
following symlinks. A native notify backend is preferred. The polling backend
uses periodic snapshots. When native notifications report a rescan condition,
or when an event cannot be confirmed by snapshots, the output records that
degraded condition instead of silently claiming completeness.

Snapshot cost is bounded by maximum entries and maximum bytes. The default
guarantee is always best effort. Network, pseudo, virtual, overlay, and other
filesystem conditions are reported when the host exposes enough information
to classify them.

## Commands and library API

The CLI implements inspect, verify, and watch. The library exposes the same
operations through inspect, verify, and the generic watch writer function,
along with snapshot and rendering types.

## Output and exit codes

Text output is intended for a terminal. JSON output has schema version 1.
watch --format jsonl begins with a ready record and then emits canonical event
or diagnostic records as they become available.

Exit code 0 means the requested operation completed with status ok. Exit code
3 means the operation completed with a degraded or failed status. Invalid
arguments, inaccessible paths, and exceeded snapshot limits also use exit
code 3. fsproof does not convert an unknown completeness state into success.

## Safety, privacy, and data handling

fsproof reads metadata, directory entries, symlink targets, and filesystem
mount information needed for classification. It does not read regular-file
contents, upload data, or write to the watched tree. Output contains paths and
metadata needed to explain changes, so treat captured output as local
diagnostic material.

## Limits and non-goals

See docs/limits.md. The MVP does not promise complete behavior on NFS, remote
filesystems, WSL, overlayfs, pseudo filesystems, or virtual filesystems
unless the specific environment has been tested and recorded. It does not
provide a durable queue, a cross-host protocol, a hosted service, or a
file-restart application.

## Testing and development

The local validation commands and release evidence are documented in
docs/release.md. Contribution rules are in CONTRIBUTING.md.

## Release and support status

This checkout is in pre-release validation. Release status changes only after
the local gates, hosted gates, artifact checks, and independent smoke audit
are recorded in private QA evidence.

## Contributing

See CONTRIBUTING.md.

## License

See LICENSE.
