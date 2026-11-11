//! Search consumes document projections through the contract.
use crate::contracts::{DocumentSummary, Documents};
use gpui_box::menus::{
    CommandDescriptor, CommandHandle, CommandId, CommandScope, ContributionScope,
    MenuCommandService, RegistrationLease, SystemError, UiOwner,
};
use std::rc::Rc;

pub struct SearchModule {
    pub query: CommandHandle<String, Vec<DocumentSummary>>,
    _scope: ContributionScope,
    _lease: RegistrationLease,
}

impl SearchModule {
    pub fn activate(
        commands: &MenuCommandService,
        owner: &UiOwner,
        documents: Rc<dyn Documents>,
    ) -> Result<Self, SystemError> {
        let scope = commands.create_scope(CommandScope::UiOwner(owner.clone()))?;
        // This provider contains three in-memory records. A production search
        // service should move expensive work into register_background.
        let query = commands.register_ui_in(
            &scope,
            CommandDescriptor::ui(CommandId::new("search.query")?, "Search documents")?,
            move |query: String, _| {
                let query = query.to_lowercase();
                Ok(documents
                    .list()
                    .into_iter()
                    .filter(|(_, title)| title.to_lowercase().contains(&query))
                    .collect::<Vec<_>>())
            },
        )?;
        let (query, lease) = query.into_parts();
        Ok(Self {
            query,
            _scope: scope,
            _lease: lease,
        })
    }
}
