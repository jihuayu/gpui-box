//! Document contributions depend on public contracts and command APIs.
use crate::contracts::Documents;
use gpui_box::menus::{
    CommandDescriptor, CommandHandle, CommandId, CommandScope, ContributionScope, ErrorCode,
    MenuCommandService, RegistrationLease, SystemError, UiOwner,
};
use std::rc::Rc;

pub struct DocumentModule {
    pub open: CommandHandle<String, String>,
    pub save: CommandHandle<(String, String), String>,
    _scope: ContributionScope,
    _leases: Vec<RegistrationLease>,
}

fn failure(_message: String) -> SystemError {
    SystemError::new(
        ErrorCode::InvalidInput,
        "The document provider rejected the request",
    )
}

impl DocumentModule {
    pub fn activate(
        commands: &MenuCommandService,
        owner: &UiOwner,
        documents: Rc<dyn Documents>,
    ) -> Result<Self, SystemError> {
        let scope = commands.create_scope(CommandScope::UiOwner(owner.clone()))?;
        let reader = documents.clone();
        let open = commands.register_ui_in(
            &scope,
            CommandDescriptor::ui(CommandId::new("documents.open")?, "Open document")?,
            move |id: String, _| reader.read(&id).map_err(failure),
        )?;
        let save = commands.register_ui_in(
            &scope,
            CommandDescriptor::ui(CommandId::new("documents.save")?, "Save document")?,
            move |(id, text): (String, String), _| {
                documents.save(&id, &text).map_err(failure)?;
                Ok(id)
            },
        )?;
        let (open, open_lease) = open.into_parts();
        let (save, save_lease) = save.into_parts();
        Ok(Self {
            open,
            save,
            _scope: scope,
            _leases: vec![open_lease, save_lease],
        })
    }
}
