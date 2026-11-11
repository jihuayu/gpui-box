//! Headless host/provider wiring; this example never accesses the OS clipboard.
use gpui_clipboard::{Clipboard, ClipboardProvider, ClipboardReceipt, ClipboardText};
use gpui_system::{
    Capability, Completion, OperationContext, ServiceResult, SurfaceId, SystemError, UiOwner,
};
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct HostClipboard(RefCell<String>);
impl ClipboardProvider for HostClipboard {
    fn capabilities(&self) -> Vec<Capability<gpui_clipboard::ClipboardOperation>> {
        Vec::new()
    }
    fn read_text(&self, _: &OperationContext) -> ServiceResult<ClipboardText> {
        Ok(Completion::Completed(ClipboardText::Text(
            self.0.borrow().clone(),
        )))
    }
    fn write_text(&self, text: &str, _: &OperationContext) -> ServiceResult<ClipboardReceipt> {
        *self.0.borrow_mut() = text.to_owned();
        Ok(Completion::Completed(ClipboardReceipt { sequence: None }))
    }
}
fn main() -> Result<(), SystemError> {
    let owner = UiOwner::new(SurfaceId(1));
    let clipboard = Clipboard::new(Rc::new(HostClipboard::default()), owner.context(None)?)?;
    // A real host invokes these inside its Copy and Paste action handlers.
    clipboard.write_text("共享文本 🦀", clipboard.user_initiated()?)?;
    let result = clipboard.read_text(clipboard.user_initiated()?)?;
    assert_eq!(
        result,
        Completion::Completed(ClipboardText::Text("共享文本 🦀".to_owned()))
    );
    println!("Headless host clipboard round trip passed; OS clipboard untouched");
    Ok(())
}
