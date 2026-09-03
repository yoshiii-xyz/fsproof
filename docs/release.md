# Release

Run the local release gates from the repository root:

~~~
git diff --check
cargo fmt --all -- --check
cargo check --all-targets --locked
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --locked
RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --locked
cargo package --locked
cargo publish --dry-run --locked
cargo audit
cargo audit --file fuzz/Cargo.lock
~~~

Run the parser fuzz target with a hard wall clock and a libFuzzer budget:

~~~
timeout --foreground 60s env RUSTUP_TOOLCHAIN=nightly cargo fuzz run duration -- -max_total_time=10 -verbosity=0 -print_final_stats=1
~~~

The timeout is the outer termination predicate. The libFuzzer time budget is
the inner termination predicate. Record exit status, executions, and any
crash artifacts in private QA evidence. Do not commit fuzz corpus or target
build output.

## Publication order

1. Complete the local gates and inspect the package contents.
2. Review the diff and commit the release candidate.
3. Push main and wait for CI, Security, and CodeQL.
4. Publish the crate from the reviewed commit.
5. Verify the crates.io API and docs.rs pages.
6. Push an annotated v0.1.0 tag and wait for the tag package workflow.
7. Create the GitHub release with the changelog.
8. Download the published crate independently, compare its contents, install
   it in a fresh Cargo home, and run inspect, verify, and watch smoke tests.
9. Record exact results, commit, URLs, and validation limits in private QA.

Hosted actions are pinned to reviewed immutable commits. Branch protection,
security settings, hosted run results, and release artifacts must be checked
before calling the release complete.

## Completeness boundary

No release statement may claim complete filesystem event delivery. The MVP
guarantee is best effort, with explicit diagnostics for rescan, fallback,
unconfirmed events, unsupported classifications, and resource limits.
