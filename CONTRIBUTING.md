# Contributing

Keep changes inside the product brief. This repository is a focused Rust
library and CLI. Do not add a frontend, hosted service, cloud backend,
telemetry, or unrelated compatibility promise.

## Local checks

Run these commands before proposing a release:

~~~
cargo fmt --all -- --check
cargo check --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --locked
RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --locked
cargo package --locked
~~~

Run the audit and bounded fuzz commands in docs/release.md when changing
dependency or parser behavior.

## Change process

Explain the failure mode or user need, make a narrow change, and add a
regression test when practical. Read back changed files, inspect the diff, and
record exact validation output in the private QA directory for release work.

Do not commit credentials, private audit output, generated targets, or
unredacted filesystem traces.
