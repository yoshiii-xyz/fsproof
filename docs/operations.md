# Operations

## Inspect

~~~
fsproof inspect PATH
fsproof inspect PATH --format json
~~~

Inspect reports the bounded snapshot, filesystem classification, and backend
capabilities. Use max entries and max bytes to tune the resource boundary.

## Verify

~~~
fsproof verify PATH --duration 10s
fsproof verify PATH --duration 10s --backend poll --poll-interval 100ms
fsproof verify PATH --duration 10s --format json
~~~

Verify takes a before snapshot, observes the requested interval, takes an
after snapshot, and reconciles the observations. The duration must have an
ms, s, or m suffix. Use poll for deterministic local testing or when native
watching is unavailable.

## Watch

~~~
fsproof watch PATH --format jsonl
fsproof watch PATH --backend poll --poll-interval 250ms --format jsonl
~~~

Watch writes one JSON object per line. The first record is ready metadata.
Later records are canonical events or diagnostics. A finite duration may be
used for tests. Without a duration, the process runs until interrupted.

## Status and failure

Exit code 0 means status ok. Exit code 3 means an invalid argument, failed
path or metadata operation, exceeded snapshot limit, degraded backend, or
unconfirmed event. The JSON status and diagnostics are the source of detail.

## Privacy

Output can contain absolute and relative paths, file sizes, timestamps,
permissions, and symlink targets. Keep output local and redact paths before
sharing. fsproof does not read regular-file contents or send network
telemetry.
