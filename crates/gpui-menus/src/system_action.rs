//! System-action and validated activation-route command bindings.

use crate::{ActivationTarget, CommandId, CommandPayload, ContextExpr, ErrorCode, SystemError};
use gpui_activation::RouteRegistry;
use serde_json::Value;
use std::collections::BTreeMap;

/// The external identity a system action binding responds to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemActionTarget {
    /// A validated external/system action identifier.
    ActionId(String),
    /// A validated durable activation route and version.
    Activation(ActivationTarget),
}

impl SystemActionTarget {
    pub fn activation(target: ActivationTarget) -> Self {
        Self::Activation(target)
    }

    pub fn action_id(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !valid_action_id(&value) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "System action identity is invalid",
            ));
        }
        Ok(Self::ActionId(value))
    }

    pub fn route(&self) -> Option<&str> {
        match self {
            Self::Activation(target) => Some(target.route()),
            Self::ActionId(_) => None,
        }
    }
}

fn valid_action_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

/// A validated mapping from an external action or activation route to a
/// registered command. `parameter_mapping` maps a payload object field to an
/// activation parameter name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemActionBinding {
    pub target: SystemActionTarget,
    pub command_id: CommandId,
    pub payload: CommandPayload,
    pub parameter_mapping: BTreeMap<String, String>,
    pub when: ContextExpr,
}

impl SystemActionBinding {
    pub fn new(target: SystemActionTarget, command: CommandId) -> Self {
        Self {
            target,
            command_id: command,
            payload: CommandPayload::Unit,
            parameter_mapping: BTreeMap::new(),
            when: ContextExpr::Always,
        }
    }

    pub fn action(action_id: impl Into<String>, command: CommandId) -> Result<Self, SystemError> {
        Ok(Self::new(
            SystemActionTarget::action_id(action_id)?,
            command,
        ))
    }

    pub fn activation(target: ActivationTarget, command: CommandId) -> Self {
        Self::new(SystemActionTarget::Activation(target), command)
    }

    pub fn with_payload(mut self, payload: CommandPayload) -> Self {
        self.payload = payload;
        self
    }

    pub fn parameter(
        mut self,
        payload_field: impl Into<String>,
        parameter: impl Into<String>,
    ) -> Self {
        self.parameter_mapping
            .insert(payload_field.into(), parameter.into());
        self
    }

    pub fn when(mut self, expression: ContextExpr) -> Self {
        self.when = expression;
        self
    }

    pub(crate) fn validate(&self, routes: &RouteRegistry) -> Result<(), SystemError> {
        match &self.target {
            SystemActionTarget::ActionId(value) => {
                if !valid_action_id(value) {
                    return Err(SystemError::new(
                        ErrorCode::InvalidInput,
                        "System action identity is invalid",
                    ));
                }
            }
            SystemActionTarget::Activation(target) => routes.validate(target)?,
        }
        if self.parameter_mapping.iter().any(|(field, parameter)| {
            field.is_empty()
                || field.len() > 128
                || !valid_action_id(field)
                || parameter.is_empty()
                || parameter.len() > 128
        }) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "System action parameter mapping is invalid",
            ));
        }
        Ok(())
    }

    pub(crate) fn apply_parameters(
        &self,
        parameters: &BTreeMap<String, String>,
    ) -> Result<CommandPayload, SystemError> {
        if self.parameter_mapping.is_empty() {
            return Ok(self.payload.clone());
        }
        let mut object = match self.payload.clone() {
            CommandPayload::Unit => serde_json::Map::new(),
            CommandPayload::Json(Value::Object(object)) => object,
            CommandPayload::Json(_) => {
                return Err(SystemError::new(
                    ErrorCode::InvalidInput,
                    "Mapped system action payload must be a JSON object",
                ));
            }
        };
        for (field, parameter) in &self.parameter_mapping {
            let value = parameters.get(parameter).ok_or_else(|| {
                SystemError::new(
                    ErrorCode::InvalidInput,
                    "Required activation parameter is missing",
                )
            })?;
            object.insert(field.clone(), Value::String(value.clone()));
        }
        Ok(CommandPayload::from_value(Value::Object(object)))
    }
}
