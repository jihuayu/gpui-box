//! Reactive context keys, snapshots, stores and composable expressions.
//!
//! Context values are logical application state, never native handles. Every
//! trigger surface projects `visible`/`enabled`/`checked` from the same live
//! store and re-evaluates at invocation time.

use gpui_system::{ErrorCode, OperationContext, SurfaceId, SystemError, UiContext};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

/// Maximum accepted context key length in bytes.
pub const MAX_CONTEXT_KEY_BYTES: usize = 128;

fn context_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CONTEXT_KEY_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

/// Stable logical context key such as `editor.focused`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContextKey(String);

impl ContextKey {
    pub fn new(value: impl Into<String>) -> Result<Self, SystemError> {
        let value = value.into();
        if !context_key(&value) {
            return Err(SystemError::new(
                ErrorCode::InvalidInput,
                "Context key must be non-empty ASCII alphanumeric plus . _ - : and at most 128 bytes",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ContextKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("ContextKey").field(&self.0).finish()
    }
}

impl fmt::Display for ContextKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for ContextKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::str::FromStr for ContextKey {
    type Err = SystemError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl TryFrom<&str> for ContextKey {
    type Error = SystemError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for ContextKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ContextKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

/// A logical context value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextValue {
    Bool(bool),
    Int(i64),
    Text(String),
    StringSet(BTreeSet<String>),
}

impl ContextValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    pub fn set(values: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::StringSet(values.into_iter().map(Into::into).collect())
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::Int(value) => Some(*value != 0),
            Self::Text(value) => match value.as_str() {
                "true" | "1" => Some(true),
                "false" | "0" => Some(false),
                _ => None,
            },
            Self::StringSet(_) => None,
        }
    }

    pub fn contains(&self, needle: &str) -> bool {
        match self {
            Self::Text(value) => value.contains(needle),
            Self::StringSet(values) => values.contains(needle),
            Self::Bool(_) | Self::Int(_) => false,
        }
    }
}

/// A point-in-time logical context view. Revision is the store observation
/// sequence, not a durable authorization grant.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSnapshot {
    pub revision: u64,
    pub values: BTreeMap<ContextKey, ContextValue>,
}

impl ContextSnapshot {
    pub fn new(revision: u64, values: BTreeMap<ContextKey, ContextValue>) -> Self {
        Self { revision, values }
    }

    pub fn get(&self, key: &ContextKey) -> Option<&ContextValue> {
        self.values.get(key)
    }

    pub fn contains_key(&self, key: &ContextKey) -> bool {
        self.values.contains_key(key)
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ContextKey, &ContextValue)> {
        self.values.iter()
    }

    /// Later values replace earlier values.
    pub fn merged_with(&self, overlay: &Self) -> Self {
        let mut values = self.values.clone();
        values.extend(overlay.values.clone());
        Self {
            revision: self.revision.max(overlay.revision),
            values,
        }
    }
}

/// Global, per-surface and per-scope context storage with revision tracking.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContextStore {
    global: BTreeMap<ContextKey, ContextValue>,
    surfaces: BTreeMap<u64, BTreeMap<ContextKey, ContextValue>>,
    scopes: BTreeMap<u64, BTreeMap<ContextKey, ContextValue>>,
    revision: u64,
}

impl ContextStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    fn changed(&mut self) -> u64 {
        self.revision = self.revision.saturating_add(1);
        self.revision
    }

    pub fn set_global(&mut self, key: ContextKey, value: ContextValue) -> u64 {
        self.global.insert(key, value);
        self.changed()
    }

    pub fn remove_global(&mut self, key: &ContextKey) -> bool {
        let removed = self.global.remove(key).is_some();
        if removed {
            self.changed();
        }
        removed
    }

    pub fn set_surface(&mut self, surface: SurfaceId, key: ContextKey, value: ContextValue) -> u64 {
        self.surfaces
            .entry(surface.0)
            .or_default()
            .insert(key, value);
        self.changed()
    }

    pub fn remove_surface(&mut self, surface: SurfaceId, key: &ContextKey) -> bool {
        let removed = self
            .surfaces
            .get_mut(&surface.0)
            .is_some_and(|values| values.remove(key).is_some());
        if removed {
            self.changed();
        }
        removed
    }

    pub fn set_scope(&mut self, scope: u64, key: ContextKey, value: ContextValue) -> u64 {
        self.scopes.entry(scope).or_default().insert(key, value);
        self.changed()
    }

    pub fn remove_scope(&mut self, scope: u64, key: &ContextKey) -> bool {
        let removed = self
            .scopes
            .get_mut(&scope)
            .is_some_and(|values| values.remove(key).is_some());
        if removed {
            self.changed();
        }
        removed
    }

    pub fn clear_surface(&mut self, surface: SurfaceId) -> bool {
        let removed = self.surfaces.remove(&surface.0).is_some();
        if removed {
            self.changed();
        }
        removed
    }

    pub fn clear_scope(&mut self, scope: u64) -> bool {
        let removed = self.scopes.remove(&scope).is_some();
        if removed {
            self.changed();
        }
        removed
    }

    pub fn snapshot(&self) -> ContextSnapshot {
        self.snapshot_for(None, None)
    }

    /// Merge in lookup order: global, surface, scope. The returned revision is
    /// the live store revision.
    pub fn snapshot_for(&self, surface: Option<SurfaceId>, scope: Option<u64>) -> ContextSnapshot {
        let mut values = self.global.clone();
        if let Some(surface) = surface
            && let Some(local) = self.surfaces.get(&surface.0)
        {
            values.extend(local.clone());
        }
        if let Some(scope) = scope
            && let Some(local) = self.scopes.get(&scope)
        {
            values.extend(local.clone());
        }
        ContextSnapshot {
            revision: self.revision,
            values,
        }
    }

    pub fn get(
        &self,
        key: &ContextKey,
        surface: Option<SurfaceId>,
        scope: Option<u64>,
        overlay: Option<&ContextSnapshot>,
    ) -> Option<ContextValue> {
        overlay
            .and_then(|overlay| overlay.get(key))
            .or_else(|| {
                scope
                    .and_then(|scope| self.scopes.get(&scope))
                    .and_then(|values| values.get(key))
            })
            .or_else(|| {
                surface
                    .and_then(|surface| self.surfaces.get(&surface.0))
                    .and_then(|values| values.get(key))
            })
            .or_else(|| self.global.get(key))
            .cloned()
    }

    pub fn evaluate(
        &self,
        expression: &ContextExpr,
        surface: Option<SurfaceId>,
        scope: Option<u64>,
        overlay: Option<&ContextSnapshot>,
    ) -> bool {
        match expression {
            ContextExpr::Always => true,
            ContextExpr::Never => false,
            ContextExpr::HasKey(key) => self.get(key, surface, scope, overlay).is_some(),
            ContextExpr::Equals(key, expected) => {
                self.get(key, surface, scope, overlay).as_ref() == Some(expected)
            }
            ContextExpr::NotEquals(key, expected) => {
                self.get(key, surface, scope, overlay).as_ref() != Some(expected)
            }
            ContextExpr::Contains(key, needle) => self
                .get(key, surface, scope, overlay)
                .is_some_and(|value| value.contains(needle)),
            ContextExpr::Not(inner) => !self.evaluate(inner, surface, scope, overlay),
            ContextExpr::All(expressions) => expressions
                .iter()
                .all(|expression| self.evaluate(expression, surface, scope, overlay)),
            ContextExpr::Any(expressions) => expressions
                .iter()
                .any(|expression| self.evaluate(expression, surface, scope, overlay)),
        }
    }
}

/// Composable reactive context predicate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextExpr {
    #[default]
    Always,
    Never,
    HasKey(ContextKey),
    Equals(ContextKey, ContextValue),
    NotEquals(ContextKey, ContextValue),
    Contains(ContextKey, String),
    Not(Box<ContextExpr>),
    All(Vec<ContextExpr>),
    Any(Vec<ContextExpr>),
}

impl ContextExpr {
    pub const fn always() -> Self {
        Self::Always
    }

    pub const fn never() -> Self {
        Self::Never
    }

    pub fn has_key(key: ContextKey) -> Self {
        Self::HasKey(key)
    }

    pub fn equals(key: ContextKey, value: ContextValue) -> Self {
        Self::Equals(key, value)
    }

    pub fn not_equals(key: ContextKey, value: ContextValue) -> Self {
        Self::NotEquals(key, value)
    }

    pub fn contains(key: ContextKey, needle: impl Into<String>) -> Self {
        Self::Contains(key, needle.into())
    }

    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Self {
        Self::Not(Box::new(self))
    }

    pub fn and(self, other: Self) -> Self {
        Self::All(vec![self, other])
    }

    pub fn or(self, other: Self) -> Self {
        Self::Any(vec![self, other])
    }

    /// Project the expression's boolean value from a value lookup.
    pub fn evaluate_with(&self, lookup: impl Fn(&ContextKey) -> Option<ContextValue>) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::HasKey(key) => lookup(key).is_some(),
            Self::Equals(key, expected) => lookup(key).as_ref() == Some(expected),
            Self::NotEquals(key, expected) => lookup(key).as_ref() != Some(expected),
            Self::Contains(key, needle) => lookup(key).is_some_and(|value| value.contains(needle)),
            Self::Not(inner) => !inner.evaluate_with(lookup),
            Self::All(expressions) => expressions
                .iter()
                .all(|expression| expression.evaluate_with(&lookup)),
            Self::Any(expressions) => expressions
                .iter()
                .any(|expression| expression.evaluate_with(&lookup)),
        }
    }
}

impl std::ops::Not for ContextExpr {
    type Output = Self;

    fn not(self) -> Self::Output {
        Self::Not(Box::new(self))
    }
}

/// The user-visible origin of one invocation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    #[default]
    Programmatic,
    Menu,
    ContextMenu,
    Keybinding,
    Palette,
    Button,
    SystemAction,
    Activation,
}

impl TriggerSource {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Programmatic => "programmatic",
            Self::Menu => "menu",
            Self::ContextMenu => "context-menu",
            Self::Keybinding => "keybinding",
            Self::Palette => "palette",
            Self::Button => "button",
            Self::SystemAction => "system-action",
            Self::Activation => "activation",
        }
    }
}

/// Invocation metadata shared by every trigger surface.
#[derive(Clone, Default)]
pub struct InvocationContext {
    pub operation: OperationContext,
    pub trigger: TriggerSource,
    pub surface: Option<SurfaceId>,
    pub scope: Option<crate::ContributionScopeId>,
    pub overlay: Option<ContextSnapshot>,
}

impl InvocationContext {
    pub fn new(operation: OperationContext) -> Self {
        Self {
            operation,
            ..Self::default()
        }
    }

    pub fn with_trigger(mut self, trigger: TriggerSource) -> Self {
        self.trigger = trigger;
        self
    }

    pub fn with_surface(mut self, surface: SurfaceId) -> Self {
        self.surface = Some(surface);
        self
    }

    pub fn with_scope(mut self, scope: crate::ContributionScopeId) -> Self {
        self.scope = Some(scope);
        self
    }

    pub fn with_overlay(mut self, overlay: ContextSnapshot) -> Self {
        self.overlay = Some(overlay);
        self
    }

    pub fn with_context(mut self, key: ContextKey, value: ContextValue) -> Self {
        let mut values = self
            .overlay
            .take()
            .map(|snapshot| snapshot.values)
            .unwrap_or_default();
        values.insert(key, value);
        self.overlay = Some(ContextSnapshot {
            revision: 0,
            values,
        });
        self
    }

    pub fn ui(&self) -> Option<&UiContext> {
        self.operation.ui.as_ref()
    }

    /// Effective surface: an explicit target wins, otherwise the live UI
    /// attachment.
    pub fn effective_surface(&self) -> Option<SurfaceId> {
        self.surface.or_else(|| self.ui().map(|ui| ui.surface))
    }

    pub fn effective_generation(&self) -> Option<u64> {
        self.ui().map(|ui| ui.generation)
    }
}

impl fmt::Debug for InvocationContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InvocationContext")
            .field("trigger", &self.trigger)
            .field("surface", &self.surface)
            .field("scope", &self.scope)
            .field("has_ui", &self.operation.ui.is_some())
            .field("has_overlay", &self.overlay.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> ContextKey {
        ContextKey::new(value).expect("key")
    }

    #[test]
    fn context_lookup_precedence_is_overlay_scope_surface_global() {
        let mut store = ContextStore::new();
        store.set_global(key("editor.mode"), ContextValue::text("global"));
        store.set_surface(
            SurfaceId(1),
            key("editor.mode"),
            ContextValue::text("surface"),
        );
        store.set_scope(7, key("editor.mode"), ContextValue::text("scope"));
        let overlay = ContextSnapshot::new(
            99,
            BTreeMap::from([(key("editor.mode"), ContextValue::text("overlay"))]),
        );
        assert_eq!(
            store.get(
                &key("editor.mode"),
                Some(SurfaceId(1)),
                Some(7),
                Some(&overlay)
            ),
            Some(ContextValue::text("overlay"))
        );
        assert_eq!(
            store.get(&key("editor.mode"), Some(SurfaceId(1)), Some(7), None),
            Some(ContextValue::text("scope"))
        );
        assert_eq!(
            store.get(&key("editor.mode"), Some(SurfaceId(1)), None, None),
            Some(ContextValue::text("surface"))
        );
        assert_eq!(
            store.get(&key("editor.mode"), None, None, None),
            Some(ContextValue::text("global"))
        );
    }

    #[test]
    fn expressions_compose_and_missing_keys_are_distinct() {
        let mut store = ContextStore::new();
        store.set_global(key("editor.focused"), ContextValue::Bool(true));
        store.set_global(key("editor.language"), ContextValue::text("rust"));
        store.set_global(
            key("editor.capabilities"),
            ContextValue::set(["format", "rename"]),
        );
        let expr = ContextExpr::has_key(key("editor.focused"))
            .and(ContextExpr::equals(
                key("editor.focused"),
                ContextValue::Bool(true),
            ))
            .and(ContextExpr::contains(key("editor.capabilities"), "format"))
            .or(ContextExpr::never());
        assert!(store.evaluate(&expr, None, None, None));
        assert!(!store.evaluate(
            &ContextExpr::equals(key("editor.focused"), ContextValue::Bool(false)),
            None,
            None,
            None
        ));
        assert!(!store.evaluate(&ContextExpr::has_key(key("missing")), None, None, None));
        assert!(store.evaluate(
            &ContextExpr::not_equals(key("missing"), ContextValue::Bool(true)),
            None,
            None,
            None
        ));
        assert!(store.evaluate(
            &ContextExpr::Not(Box::new(ContextExpr::Never)),
            None,
            None,
            None
        ));
    }

    #[test]
    fn revision_only_tracks_mutations() {
        let mut store = ContextStore::new();
        assert_eq!(store.revision(), 0);
        store.set_global(key("a"), ContextValue::Bool(true));
        let first = store.revision();
        store.remove_global(&key("missing"));
        assert_eq!(store.revision(), first);
        store.clear_scope(1);
        assert_eq!(store.revision(), first);
        store.set_surface(SurfaceId(1), key("b"), ContextValue::Int(1));
        assert!(store.revision() > first);
        assert_eq!(store.snapshot_for(Some(SurfaceId(1)), None).len(), 2);
    }

    #[test]
    fn context_keys_reject_unsafe_shapes() {
        for value in ["", "with space", "slash/value", "é"] {
            assert!(ContextKey::new(value).is_err());
        }
        assert!(ContextKey::new("editor.focused").is_ok());
    }
}
