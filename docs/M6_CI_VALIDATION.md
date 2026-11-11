# M6 continuous integration qualification

## Windows shared-code gate

`.github/workflows/windows.yml` runs on pull requests, pushes to main and manual
dispatch. The Windows 2025 hosted image provides MSVC and the Windows SDK. The job
uses immutable revisions of checkout, MSVC setup, mise setup and artifact upload.
Mise 2026.7.5 is checked against the published Windows x64 SHA-256, and `mise.toml`
selects Rust 1.98.1. Rustfmt and Clippy are installed through the existing mise
setup task. The workflow records rustc's verbose identity and Cargo's version.

The job invokes existing repository entry points in order

| Entry point | Scope |
| --- | --- |
| `mise run check` | Formatting, shared/CLI/system package tests, compile-fail doctests, strict all-target Clippy and diff checks |
| `mise run check:components` | Component model/context tests and strict component/gallery Clippy |
| `cargo test --locked -p gpui-box-example-hello` through mise | Shared gallery and typed workflow tests |
| `mise run check:desktop` | Windows desktop example compilation against the copied GPUI identity |

Repository permissions are read-only. Checkout credentials are discarded. The job
uses no release signing/deployment secrets. Superseded runs for the same workflow
and ref are cancelled. The timeout is 60 minutes. Logs and tool identities under
`target/ci` are uploaded on success or failure and retained for 14 days. An early
setup failure remains visible in the Actions job logs even if that directory has
not been created.

## Qualification boundary

Local Windows checks pass 196 host tests plus six compile-fail doctests and two
nested child reports, 16 component tests and six gallery/workflow tests. The local
API 35 x86_64 emulator suite passes 21/21 after restarting an unresponsive emulator.
Those emulator checks remain separate from this hosted Windows job.

The first hosted pull-request run passed all steps in 8 minutes 37 seconds,
using upstream Rust 1.98.1 and the hosted Windows toolchain. It passed the same
196 shared/CLI tests, six compile-fail doctests, 16 component tests, six gallery
tests, strict lints and desktop compilation. The validation-log artifact was
uploaded successfully. GitHub reported a Node 20 deprecation annotation for
the pinned mise/MSVC/artifact actions and forced those actions onto Node 24.
Their steps completed successfully in this run.

[Hosted qualification run](https://github.com/jihuayu/gpui-box/actions/runs/43033815129)

[Validation log artifact](https://github.com/jihuayu/gpui-box/actions/runs/43033815129/artifacts/13862158447)

This first job covers shared code, CLI, components and desktop compilation.
Desktop interactive/screen-reader sessions, Linux/macOS build coverage, Android
cross-build/emulator CI, feature/dependency audit matrices, release signing,
16 KB runtime pages, GPU/performance and hardware qualification retain separate
M6 gates. Physical-device testing remains deferred under the current project
scope. M6 stays open until its full release acceptance is satisfied.
