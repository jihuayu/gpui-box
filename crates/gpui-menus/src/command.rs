//! Typed command identity, descriptors, payload bridging and task snapshots.
//!
//! Command identity is deliberately narrow: a stable identifier is not a
//! display string, a path, or user content. A descriptor declares the
//! execution domain, concurrency policy, capability/permission preconditions
//! and reactive visibility policy used by every trigger surface.

use crate::{ContextExpr, ContextKey, MenusOperation, PermissionKind};
use gpui_system::{ErrorCode, SystemError};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
use serde_json::Value;
use std::{fmt, hash::Hash, marker::PhantomData};

/// Maximum accepted command identifier length in bytes.
pub const MAX_COMMAND_ID_BYTES: usize = 128;

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_COMMAND_ID_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

/// Stable command identity such as `file.save` or `editor.format`.
///
/// The value is validated on construction and on deserialization. It is never
/// derived from a title, a path or user content.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandId(String);

impl CommandId {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !identifier(&value) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Command identity must be non-empty ASCII alphanumeric plus . _ - : and at most 128 bytes",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_valid(value: &str) -> bool {
        identifier(value)
    }
}

impl fmt::Debug for CommandId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("CommandId").field(&self.0).finish()
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for CommandId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for CommandId {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for CommandId {
    type Error = SystemError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for CommandId {
    type Error = SystemError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for CommandId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CommandId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// Where a command handler is allowed to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionDomain {
    /// Executes on the owning UI thread with a current UI context.
    Ui,
    /// Executes on the service's bounded background worker pool.
    Background,
}

/// Per-command execution concurrency policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum ConcurrencyPolicy {
    /// At most `max_in_flight` concurrent invocations.
    AllowParallel { max_in_flight: usize },
    /// Reject while any invocation is running.
    #[default]
    SingleFlightReject,
    /// Cancel the superseded in-flight invocation before starting the next.
    ReplaceInFlight,
}

impl ConcurrencyPolicy {
    /// Validate a policy. `AllowParallel` rejects a zero limit.
    pub fn validate(&self) -> Result<(), SystemError> {
        match self {
            Self::AllowParallel { max_in_flight } if *max_in_flight == 0 => Err(SystemError::new(
                ErrorCode::InvalidInput,
                "AllowParallel requires max_in_flight greater than zero",
            )),
            _ => Ok(()),
        }
    }

    /// Fallible constructor for the parallel variant.
    pub fn allow_parallel(max_in_flight: usize) -> Result<Self, SystemError> {
        let policy = Self::AllowParallel { max_in_flight };
        policy.validate()?;
        Ok(policy)
    }

    pub const fn is_parallel(&self) -> bool {
        matches!(self, Self::AllowParallel { .. })
    }
}

/// A typed command declaration shared by every trigger surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDescriptor {
    pub id: CommandId,
    pub title: String,
    pub category: Option<String>,
    pub summary: Option<String>,
    pub domain: ExecutionDomain,
    pub concurrency: ConcurrencyPolicy,
    pub required_permissions: Vec<PermissionKind>,
    pub required_capabilities: Vec<MenusOperation>,
    pub enabled_when: ContextExpr,
    pub visible_when: ContextExpr,
    pub checked_key: Option<ContextKey>,
}

impl CommandDescriptor {
    pub fn new(
        id: CommandId,
        title: impl Into<String>,
        domain: ExecutionDomain,
        concurrency: ConcurrencyPolicy,
    ) -> Result<Self, SystemError> {
        let descriptor = Self {
            id,
            title: title.into(),
            category: None,
            summary: None,
            domain,
            concurrency,
            required_permissions: Vec::new(),
            required_capabilities: Vec::new(),
            enabled_when: ContextExpr::Always,
            visible_when: ContextExpr::Always,
            checked_key: None,
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    /// Convenience constructor for a UI command using single-flight rejection.
    pub fn ui(id: CommandId, title: impl Into<String>) -> Result<Self, SystemError> {
        Self::new(
            id,
            title,
            ExecutionDomain::Ui,
            ConcurrencyPolicy::SingleFlightReject,
        )
    }

    /// Convenience constructor for a background command using single-flight
    /// rejection.
    pub fn background(id: CommandId, title: impl Into<String>) -> Result<Self, SystemError> {
        Self::new(
            id,
            title,
            ExecutionDomain::Background,
            ConcurrencyPolicy::SingleFlightReject,
        )
    }

    pub fn validate(&self) -> Result<(), SystemError> {
        if self.title.trim().is_empty() || self.title.len() > 512 {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Command title must be non-empty and at most 512 bytes",
            ));
        }
        self.concurrency.validate()
    }

    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    pub fn require_permission(mut self, permission: PermissionKind) -> Self {
        self.required_permissions.push(permission);
        self
    }

    pub fn require_permissions(
        mut self,
        permissions: impl IntoIterator<Item = PermissionKind>,
    ) -> Self {
        self.required_permissions.extend(permissions);
        self
    }

    pub fn require_capability(mut self, capability: MenusOperation) -> Self {
        self.required_capabilities.push(capability);
        self
    }

    pub fn require_capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = MenusOperation>,
    ) -> Self {
        self.required_capabilities.extend(capabilities);
        self
    }

    pub fn enabled_when(mut self, expression: ContextExpr) -> Self {
        self.enabled_when = expression;
        self
    }

    pub fn visible_when(mut self, expression: ContextExpr) -> Self {
        self.visible_when = expression;
        self
    }

    pub fn checked_when(mut self, key: ContextKey) -> Self {
        self.checked_key = Some(key);
        self
    }
}

/// A typed handle binding one input/output pair to a registered command.
///
/// `PhantomData<fn(I) -> O>` keeps the handle `Send + Sync` and cloneable even
/// when the handler input or output is not. The handle is only a typed lookup
/// key; it grants no execution authority.
pub struct CommandHandle<I, O> {
    id: CommandId,
    domain: ExecutionDomain,
    marker: PhantomData<fn(I) -> O>,
}

impl<I, O> CommandHandle<I, O> {
    pub fn new(id: CommandId, domain: ExecutionDomain) -> Self {
        Self {
            id,
            domain,
            marker: PhantomData,
        }
    }

    pub fn id(&self) -> &CommandId {
        &self.id
    }

    pub const fn domain(&self) -> ExecutionDomain {
        self.domain
    }
}

impl<I, O> Clone for CommandHandle<I, O> {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            domain: self.domain,
            marker: PhantomData,
        }
    }
}

impl<I, O> PartialEq for CommandHandle<I, O> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.domain == other.domain
    }
}

impl<I, O> Eq for CommandHandle<I, O> {}

impl<I, O> fmt::Debug for CommandHandle<I, O> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CommandHandle")
            .field("id", &self.id)
            .field("domain", &self.domain)
            .finish()
    }
}

/// Dynamic command input or output used between typed handlers and surfaces.
///
/// `Unit` and `Value::Null` are interchangeable, so `()` round-trips without a
/// special case at the menu, keybinding, palette or activation boundary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CommandPayload {
    #[default]
    Unit,
    Json(Value),
}

impl CommandPayload {
    pub const fn unit() -> Self {
        Self::Unit
    }

    pub fn json(value: impl Into<Value>) -> Self {
        Self::from_value(value.into())
    }

    pub fn from_json(value: Value) -> Self {
        Self::from_value(value)
    }

    pub fn from_value(value: Value) -> Self {
        if value.is_null() {
            Self::Unit
        } else {
            Self::Json(value)
        }
    }

    pub fn as_value(&self) -> Value {
        match self {
            Self::Unit => Value::Null,
            Self::Json(value) => value.clone(),
        }
    }

    pub fn is_unit(&self) -> bool {
        matches!(self, Self::Unit | Self::Json(Value::Null))
    }

    /// Serialize a typed value into the dynamic bridge. `()` becomes `Unit`.
    pub fn from_typed<T: Serialize>(value: &T) -> Result<Self, SystemError> {
        let value = serde_json::to_value(value).map_err(|_| {
            SystemError::new(
                ErrorCode::InvalidInput,
                "Command payload cannot be serialized",
            )
        })?;
        Ok(Self::from_value(value))
    }

    /// Deserialize into the command's typed input or output.
    pub fn into_typed<T: DeserializeOwned>(self) -> Result<T, SystemError> {
        serde_json::from_value(self.as_value()).map_err(|_| {
            SystemError::new(
                ErrorCode::InvalidInput,
                "Command payload does not match the typed command",
            )
        })
    }
}

impl From<()> for CommandPayload {
    fn from((): ()) -> Self {
        Self::Unit
    }
}

impl From<Value> for CommandPayload {
    fn from(value: Value) -> Self {
        Self::from_value(value)
    }
}

impl From<CommandPayload> for Value {
    fn from(payload: CommandPayload) -> Self {
        payload.as_value()
    }
}

impl Serialize for CommandPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.as_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CommandPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::from_value(Value::deserialize(deserializer)?))
    }
}

/// Identity of one accepted invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InvocationId(pub u64);

/// Terminal-or-running state of one command invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTaskState {
    Running,
    Completed,
    Cancelled(gpui_system::CancelReason),
    Failed(ErrorCode),
}

impl CommandTaskState {
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

/// Bounded public snapshot of one invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandTaskSnapshot {
    pub id: InvocationId,
    pub command_id: CommandId,
    pub domain: ExecutionDomain,
    pub trigger: crate::TriggerSource,
    pub scope: Option<crate::ContributionScopeId>,
    pub surface: Option<gpui_system::SurfaceId>,
    pub state: CommandTaskState,
}

/// Diagnostic emitted when two contributions claim the same command identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateCommandDiagnostic {
    pub command_id: CommandId,
    pub existing_scope: crate::ContributionScopeId,
    pub conflicting_scope: crate::ContributionScopeId,
    pub domain: ExecutionDomain,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextValue;

    #[test]
    fn command_identity_is_narrow_and_validated() {
        for value in ["file.save", "editor.format", "app:window.new", "a_b-c.1"] {
            assert_eq!(CommandId::new(value).expect("valid").as_str(), value);
        }
        for value in ["", "with space", "slash/value", "é", "trailing\n"] {
            assert!(CommandId::new(value).is_err(), "{value:?} must be rejected");
        }
        assert!(CommandId::new("a".repeat(129)).is_err());
        assert!(CommandId::new("a".repeat(128)).is_ok());
    }

    #[test]
    fn payload_bridges_unit_null_and_typed_values() {
        assert_eq!(
            CommandPayload::from_typed(&()).expect("unit"),
            CommandPayload::Unit
        );
        assert_eq!(
            CommandPayload::from_value(Value::Null),
            CommandPayload::Unit
        );
        assert_eq!(
            CommandPayload::Unit
                .into_typed::<()>()
                .expect("unit output"),
            ()
        );
        let value = CommandPayload::from_typed(&vec![1_u32, 2, 3]).expect("serialized");
        assert_eq!(
            value.into_typed::<Vec<u32>>().expect("typed output"),
            vec![1, 2, 3]
        );
        assert_eq!(
            CommandPayload::from_value(Value::Null)
                .into_typed::<Option<u8>>()
                .expect("option"),
            None
        );
        assert_eq!(
            CommandPayload::Unit
                .into_typed::<u8>()
                .expect_err("mismatched payload")
                .code,
            ErrorCode::InvalidInput
        );
    }

    #[test]
    fn handles_are_cloneable_and_thread_safe() {
        fn assert_send_sync<T: Send + Sync>() {}
        fn assert_clone<T: Clone>() {}
        assert_send_sync::<CommandHandle<(), ()>>();
        assert_send_sync::<CommandHandle<std::rc::Rc<()>, std::rc::Rc<()>>>();
        assert_clone::<CommandHandle<(), ()>>();
        let handle = CommandHandle::<(), ()>::new(
            CommandId::new("file.save").expect("id"),
            ExecutionDomain::Ui,
        );
        assert_eq!(handle, handle.clone());
        assert_eq!(handle.id().as_str(), "file.save");
    }

    #[test]
    fn descriptors_validate_concurrency_and_context() {
        let id = CommandId::new("file.save").expect("id");
        assert!(
            CommandDescriptor::new(
                id,
                "Save",
                ExecutionDomain::Ui,
                ConcurrencyPolicy::AllowParallel { max_in_flight: 0 },
            )
            .is_err()
        );
        assert!(ConcurrencyPolicy::allow_parallel(0).is_err());
        let descriptor = CommandDescriptor::ui(CommandId::new("file.save").expect("id"), "Save")
            .expect("descriptor")
            .enabled_when(ContextExpr::equals(
                ContextKey::new("editor.focused").expect("key"),
                ContextValue::Bool(true),
            ))
            .with_summary("Write the current document");
        assert_eq!(descriptor.title, "Save");
        assert_eq!(
            descriptor.summary.as_deref(),
            Some("Write the current document")
        );
    }
}
