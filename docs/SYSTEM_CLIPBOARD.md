# Foreground text clipboard

This is the clipboard slice of [#25](https://github.com/jihuayu/gpui-box/issues/25).
Enable the facade's `clipboard` feature and import `gpui_box::clipboard`, or use
`gpui-clipboard` directly. Native providers are explicitly attached by the host;
no provider is installed just by enabling the feature.

## Host and operation ownership

`Clipboard` is UI-local (`Rc`) and binds the supplied live `UiContext` for its
entire lifetime. Create the `UiOwner` on the actual native UI thread. Invalidate
or advance the owner before destroying or recreating its native attachment, then
attach a new clipboard service. The service retains a weak UI attachment; it
does not keep a destroyed native window alive. Existing service clones remain
bound to the same generation.

Call `user_initiated()` directly from a foreground Copy/Paste or explicitly
selected Copy fallback action. Its non-cloneable `ClipboardInteraction` is
consumed by exactly one read or write and cannot be used with a different service.
`with_cancellation` and `with_deadline` use the common `gpui-system` operation
context. This is a trusted host attestation of a user action, not an unforgeable
OS gesture or a permission grant. Providers must recheck native foreground and
access conditions at execution.

Windows hosts attach a borrowed HWND on its owning thread:

```rust,ignore
use gpui_box::clipboard::{ClipboardText, WindowsClipboard};
use gpui_box::system::Completion;
use std::num::NonZeroIsize;

// The host obtained hwnd from its own live native window. ui belongs to that
// window's UiOwner; the host invalidates it before native recreation.
let clipboard = WindowsClipboard::attach(NonZeroIsize::new(hwnd).unwrap(), ui)?;
// Inside the Copy action:
let receipt = clipboard.write_text("中文 🧑🏽‍💻", clipboard.user_initiated()?)?;
// Inside the Paste action:
if let Completion::Completed(ClipboardText::Text(text)) =
    clipboard.read_text(clipboard.user_initiated()?)?
{
    // Pass text into the existing editor/controller's paste transaction.
    // Do not synthesize a second IME composition or keyboard event stream.
}
```

The dependency-free executable `cargo +1.98.1 run --locked -p gpui-clipboard
--example text` demonstrates injected host wiring and a Unicode round trip. It
uses a headless provider, never the OS clipboard. There is no GPUI void-write
adapter: its unobservable error behavior cannot provide this service's receipt.
The native host/gallery wiring remains under #2/#3/#8 and is not modified here.

## Outcomes and native behavior

- `ClipboardText::Empty`: no formats exist; `NoText`: formats exist without a
  plain-text representation; `Text("")`: an explicit empty text representation.
- Permission denial, provider unavailability, lock contention and native failures
  use typed errors. Reading capabilities never reads clipboard content.
- `ClipboardReceipt` means the OS accepted publication. Its optional sequence is
  an observation, not permission to clear later, proof of paste or synchronization.
- Text is limited to one MiB of UTF-8; embedded NUL is rejected rather than silently
  truncated. Native UTF-16 is bounded and strictly decoded. CJK, emoji, combining
  sequences and line endings pass unchanged into the existing input controller.
- Cooperative cancellation and elapsed deadlines are checked before execution.
  Completed effects retain their actual receipt even if cancellation is requested
  during a synchronous provider call. Cancellation cannot restore old clipboard data.

The Windows provider validates HWND existence, current process, current native
thread and actual foreground window on every operation. `OpenClipboard` contention
is reported as `Busy` when a competing open owner is observable. Other access
denials remain `PermissionDenied`. It allocates/populates a movable UTF-16 block
before replacing the clipboard. After successful `SetClipboardData`, Windows
owns that allocation. `EmptyClipboard` and publication are not atomic: a failure
after clearing reports `PlatformFailure` with that partial effect stated in the
public-safe message. Existing clipboard formats cannot be guaranteed restored.

The provider writes plain text only, replacing other formats as an explicit copy
operation. It makes no sensitive-content expiry, ownership-clear, cloud-sync
suppression, rich-format, file clipboard or drag/drop claims. These need the later
transfer-data and resource lease work from the design. No clipboard payload is
logged or included in native error messages.

## Platform evidence and remaining gates

Collected on Windows x86_64, November 11, 2026: rustc `1.98.1`, custom host cargo
`1.101.0`, workspace selector `+1.98.1`.

| Platform | Implementation | Evidence and remaining gate |
| --- | --- | --- |
| Windows | Native HWND-bound text read/write with typed native errors | Compiled; real message-only-window ownership/foreground denial and invalid-HWND tests passed without opening or modifying the clipboard. Successful interactive publication/paste remains unqualified under #8. |
| Android arm64/x86_64 | Common contract; no native ClipboardManager provider installed | Both ABI facade cross-checks passed. #2 must wire foreground Activity, user interaction and OS access policy; emulator/device paste remains pending. |
| macOS | Common contract; explicit unavailable provider available | NSPasteboard adapter and #8 runtime qualification pending. |
| Linux | Common contract; explicit unavailable provider available | X11/Wayland focus, selection lifetime and clipboard-manager behavior pending under #8. |
| iOS | Common contract; explicit unavailable provider available | UIPasteboard privacy/user-interaction policy and #9 device qualification pending. |

Capabilities conservatively retain `Evidence::Compiled` on the Windows read/write
provider. The non-mutating native tests are not evidence of successful clipboard
publication or cross-application paste. Tests never save only text and restore it
over unknown existing formats; no automated test mutates the user's clipboard.

Validation:

```sh
cargo +1.98.1 fmt --all -- --check
cargo +1.98.1 test --locked -p gpui-clipboard
cargo +1.98.1 clippy --locked -p gpui-clipboard -p gpui-box --all-targets --features clipboard -- -D warnings
cargo +1.98.1 run --locked -p gpui-clipboard --example text
cargo +1.98.1 check --locked -p gpui-box --no-default-features --features clipboard --target aarch64-linux-android
cargo +1.98.1 check --locked -p gpui-box --no-default-features --features clipboard --target x86_64-linux-android
```
