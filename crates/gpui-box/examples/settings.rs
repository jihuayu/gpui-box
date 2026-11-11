use gpui_box::{
    app_data::{AppData, DataScope, DirectoryRoots},
    system::{Completion, OperationContext},
};
use std::error::Error;
fn main() -> Result<(), Box<dyn Error>> {
    // Explicit scratch roots keep this runnable demonstration out of real app data.
    let root =
        std::env::temp_dir().join(format!("gpui-box-settings-example-{}", std::process::id()));
    let app = AppData::open(DirectoryRoots::under(root))?;
    app.capabilities()
        .into_iter()
        .for_each(|capability| assert!(capability.check().is_ok()));
    let context = OperationContext::default();
    let v1 = app.settings(DataScope::Application, "preferences", 1, 4096)?;
    v1.save(&42_u32, &context)?;
    let v2 = app.settings(DataScope::Application, "preferences", 2, 4096)?;
    v2.migrate(&context, |_, old| Ok(old))?;
    if let Completion::Completed(Some(value)) = v2.load::<u32>(&context)? {
        println!("Saved, migrated and reloaded preference: {value}");
    }
    Ok(())
}
