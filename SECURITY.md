# Security policy

## Scope

Report security issues in fsproof that could expose data outside the requested
path, execute unintended commands, or corrupt diagnostic output.

Do not include secrets, private filesystem contents, or sensitive path data in
an issue. Use a redacted reproducer and the smallest affected tree.

## Supported version

Only the latest main branch and the latest published release receive
security fixes.

## Design boundary

fsproof reads filesystem metadata and mount information. It does not claim
that a notification stream is complete, does not write to the watched tree,
and does not provide a security boundary. Run it with the least filesystem
access needed for the requested path.
