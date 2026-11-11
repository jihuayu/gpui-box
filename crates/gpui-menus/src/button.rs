//! Component button/toolbar command projections.

use crate::{CommandId, CommandPayload, ContextExpr};

/// A button or toolbar action backed by a registered command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonCommandBinding {
    pub command_id: CommandId,
    pub payload: CommandPayload,
    pub label: Option<String>,
    pub visible_when: ContextExpr,
    pub enabled_when: ContextExpr,
}

impl ButtonCommandBinding {
    pub fn new(command_id: CommandId) -> Self {
        Self {
            command_id,
            payload: CommandPayload::Unit,
            label: None,
            visible_when: ContextExpr::Always,
            enabled_when: ContextExpr::Always,
        }
    }

    pub fn with_payload(mut self, payload: CommandPayload) -> Self {
        self.payload = payload;
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn visible_when(mut self, expression: ContextExpr) -> Self {
        self.visible_when = expression;
        self
    }

    pub fn enabled_when(mut self, expression: ContextExpr) -> Self {
        self.enabled_when = expression;
        self
    }
}

/// Projected component state shared by buttons and toolbar actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonCommandState {
    pub label: String,
    pub summary: String,
    pub accessible_name: String,
    pub accessible_checked: Option<bool>,
    pub visible: bool,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub shortcut_hint: Option<String>,
}
