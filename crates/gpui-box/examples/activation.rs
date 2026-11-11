use gpui_box::{
    activation::*,
    app_data::{AppData, DirectoryRoots},
    system::Completion,
};
use std::{error::Error, time::SystemTime};
fn value<T>(result: ServiceResult<T>) -> Result<T, SystemError> {
    match result? {
        Completion::Completed(value) => Ok(value),
        Completion::Cancelled(_) => {
            Err(SystemError::new(ErrorCode::Unavailable, "Sample cancelled"))
        }
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!(
        "gpui-box-activation-example-{}",
        std::process::id()
    ));
    let app = AppData::open(DirectoryRoots::under(root))?;
    let context = OperationContext::default();
    let inbox = value(ActivationInbox::open(
        &app,
        "sample",
        InboxConfig::default(),
        UrlPolicy::new(["https"])?,
        RouteRegistry::default(),
        &context,
    ))?;
    let event = ActivationEvent::new(
        ActivationId::new("launch-1")?,
        ActivationSource::ColdLaunch,
        ActivationPayload::Activate,
        SystemTime::now(),
    )?;
    value(inbox.enqueue(event, SystemTime::now(), &context))?;
    inbox.set_ready(true);
    let events = value(inbox.pending(SystemTime::now(), &context))?;
    for event in &events {
        value(inbox.acknowledge(event.id(), SystemTime::now(), &context))?;
    }
    println!(
        "Durably routed and acknowledged {} activation(s)",
        events.len()
    );
    Ok(())
}
