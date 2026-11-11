# Vendored official GPUI

This directory contains copied source from the official Zed repository at
`f8cfc7ed9e57503def9479138894f3139f5400cc`.

It includes the GPUI engine and platform packages, their local manifest dependency
closure including test/build dependencies, and fonts referenced by upstream
examples and tests. All selected package manifests declare Apache-2.0. Font
licenses are retained beside their assets.

`Cargo.upstream.toml` preserves the original workspace manifest. `Cargo.toml`
is adapted to contain the selected packages and inherited dependencies/lints.
`UPSTREAM.json` records the source revision, archive digest, copied files, and
per-file content hashes. License symlinks are materialized as regular files for
Windows portability. [Downstream changes](DOWNSTREAM_CHANGES.md) records the
Android target-gate patch. All other imported Rust files retain their original
content.

The distribution root uses path dependencies on this source. Root tasks scope
formatting, tests, and linting to distribution packages. Run upstream-specific
checks explicitly with `--manifest-path vendor/gpui/Cargo.toml` and the intended
package/features. Its copied upstream `.cargo` aliases refer to the original
Zed tooling and are retained for provenance. Distribution tasks run from the root.

Keep generic engine and backend fixes here. Record downstream changes and their
upstream rationale when modifying imported files. Common components and product
services belong in the distribution's `components` and `crates` directories.
