//! Run manually: cargo run -p gpui-sharing --features gpui-example --example windows_share
//! A click invokes the OS share sheet; the status describes invocation only.
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod app {
    use gpui::*;
    use gpui_sharing::{ShareItem, ShareRequest, Sharing, windows::WindowsSharing};
    use gpui_system::{SurfaceId, UiOwner};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: main initialized this STA successfully and owns the guard.
            unsafe { CoUninitialize() };
        }
    }
    struct ShareView {
        owner: UiOwner,
        sharing: Option<Sharing>,
        status: String,
    }
    impl Render for ShareView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .p_6()
                .flex()
                .flex_col()
                .gap_4()
                .child("Native Windows sharing — text and web links")
                .child(
                    div()
                        .id("share-text")
                        .p_4()
                        .bg(rgb(0x4477bb))
                        .cursor_pointer()
                        .child("Share text")
                        .on_click(cx.listener(|this, _, window, cx| {
                            let result = (|| {
                                if this.sharing.is_none() {
                                    let handle = HasWindowHandle::window_handle(window)
                                        .map_err(|_| "Window handle unavailable".to_owned())?;
                                    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                                        return Err("Expected Win32 window".into());
                                    };
                                    this.sharing = Some(
                                        WindowsSharing::attach(
                                            handle.hwnd.get(),
                                            this.owner.context(None).map_err(|e| e.to_string())?,
                                        )
                                        .map_err(|e| e.to_string())?,
                                    );
                                }
                                let sharing = this.sharing.as_ref().unwrap();
                                sharing
                                    .share(
                                        ShareRequest::native(ShareItem::Text(
                                            "你好，GPUI 😀".into(),
                                        )),
                                        sharing.user_initiated().map_err(|e| e.to_string())?,
                                    )
                                    .map(|v| {
                                        format!("{v:?} — display and target delivery unobserved")
                                    })
                                    .map_err(|e| e.to_string())
                            })();
                            this.status = result.unwrap_or_else(|error| error);
                            cx.notify();
                        })),
                )
                .child(self.status.clone())
        }
    }
    pub fn run() {
        // SAFETY: this is the application's UI thread before GPUI starts.
        // Successful S_OK/S_FALSE is balanced by the one guard after run ends.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
            .expect("A Windows STA UI thread is required");
        let _apartment = Apartment;
        gpui_platform::application().run(|cx| {
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.open_window(WindowOptions::default(), |_, cx| {
                cx.new(|_| ShareView {
                    owner: UiOwner::new(SurfaceId(1)),
                    sharing: None,
                    status: "Click Share text to invoke the native sheet".into(),
                })
            })
            .unwrap();
            cx.activate(true);
        });
    }
}
#[cfg(target_os = "windows")]
fn main() {
    app::run();
}
#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This example requires Windows");
}
