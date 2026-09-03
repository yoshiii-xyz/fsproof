# Limits

## Notification completeness

Native filesystem notifications are not a proof of completeness. Linux
inotify can report queue overflow, and rename notifications do not provide a
portable, durable transaction across all failure modes. notify exposes a
rescan condition for events that may have been missed. fsproof reports those
conditions and reconciles with a bounded snapshot.

## Filesystem classes

Network filesystems, WSL mounts, overlayfs, pseudo filesystems, virtual
filesystems, and unusual bind mount arrangements can have different event
semantics. fsproof reports a classification when the host provides enough
mount information, but the MVP does not promise complete behavior for those
classes. The release evidence lists the classes actually exercised.

## Polling and rename ambiguity

Polling can miss a short-lived state between snapshots. A snapshot pair can
identify a likely rename when one removed and one created fingerprint match,
but it cannot prove identity for equal files or simultaneous changes. Native
rename events also depend on the backend's available path information.

Metadata-only snapshots can miss content changes that preserve every recorded
metadata field. fsproof does not read regular-file contents.

## Resource bounds

Entry count and metadata byte limits bound memory and traversal cost. There is
no durable queue, no unbounded event history, and no guarantee that a tree
will fit within the configured limits.

## Validation boundary

Tests run only against filesystems available on the validation host. Any
unavailable mount class is recorded as unverified. No NFS, remote host, or WSL
claim is made from a local Linux run.
