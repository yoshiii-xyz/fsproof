# Local operating contract

This repository is a focused Rust project. The source files and CI workflows
are authoritative.

## Commands

- Build: cargo build --locked
- Test: cargo test --all-targets --locked
- Format check: cargo fmt --all -- --check
- Lint: cargo clippy --all-targets --all-features --locked -- -D warnings
- Documentation: RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --locked
- Package check: cargo package --locked
- CLI smoke test: use the commands in docs/release.md

## Scope

Keep changes inside docs/product-brief.md. Do not add frontend code, hosted
services, cloud backends, telemetry, or unrelated compatibility promises.

## Operating loop

1. Plan the change and define a measurable success condition.
2. Make only the scoped edit.
3. Read back every changed file.
4. Run the smallest relevant check, then the full gate at milestones.
5. Record exact outputs and evidence paths.
6. Review the diff before committing or publishing.

## Secrets and writes

Never store credentials, tokens, private keys, registry login files, secret
values, or unredacted filesystem traces in tracked files. Treat generated
qa/ material as private.

## Release

Follow docs/release.md. A release requires a clean working tree, passing local
and hosted gates, independently verified artifacts, and a truthful record of
validation limits.
