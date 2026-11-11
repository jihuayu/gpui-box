use crate::layout::{FieldDraft, FocusTarget, RouteIdentity, SceneState, SelectionRange};
use gpui_system::{ErrorCode, SystemError};
use std::collections::BTreeMap;

const MAGIC: &[u8] = b"GPL1";

/// Upper bound on migration hops attempted for one restore.
const MAX_MIGRATION_HOPS: usize = 32;

/// Version of the persisted snapshot schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion(pub u32);

/// The result of a restore or save attempt.
///
/// Every variant is recoverable: no malformed host input may panic the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreOutcome {
    /// The stored schema matched and the snapshot was restored.
    Restored(VersionedSnapshot),
    /// A registered migration produced a current-version snapshot.
    Migrated {
        /// Schema version found in the store.
        from: SchemaVersion,
        /// Migrated snapshot.
        snapshot: VersionedSnapshot,
    },
    /// No migration was registered, or the registered migration failed.
    SchemaMismatch {
        /// Schema version found in the store.
        found: SchemaVersion,
        /// Schema version this coordinator expects.
        expected: SchemaVersion,
    },
    /// The stored bytes were not a valid snapshot.
    CorruptSnapshot,
    /// No snapshot was stored under this key.
    AbsentSnapshot,
    /// The store rejected the write.
    SaveFailed,
    /// The restore callback outlived the host generation that requested it.
    StaleGeneration {
        /// Generation carried by the restore request.
        found: u64,
        /// Current live host generation.
        current: u64,
    },
}

/// Durable storage for encoded lifecycle snapshots.
///
/// Issue #22 supplies persistent platform storage. The core crate ships only
/// the in-memory test implementation.
pub trait SnapshotStore {
    /// Loads raw bytes for a key.
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, SystemError>;
    /// Stores raw bytes for a key.
    fn save(&mut self, key: &str, payload: &[u8]) -> Result<(), SystemError>;
}

/// A deterministic in-memory store for tests and examples.
#[derive(Debug, Clone, Default)]
pub struct InMemorySnapshotStore {
    entries: BTreeMap<String, Vec<u8>>,
    fail_saves: bool,
}

impl InMemorySnapshotStore {
    /// An empty store that accepts writes.
    pub fn new() -> Self {
        Self::default()
    }

    /// A store that rejects every write, for failure-path tests.
    pub fn failing_saves() -> Self {
        let mut store = Self::new();
        store.fail_saves = true;
        store
    }

    /// Inserts raw bytes, for corrupt-payload tests.
    pub fn insert_raw(&mut self, key: impl Into<String>, payload: impl Into<Vec<u8>>) {
        self.entries.insert(key.into(), payload.into());
    }

    /// Raw bytes currently stored under a key.
    pub fn raw(&self, key: &str) -> Option<&[u8]> {
        self.entries.get(key).map(Vec::as_slice)
    }

    /// Whether a key is present.
    pub fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// Removes and returns raw bytes.
    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.entries.remove(key)
    }
}

impl SnapshotStore for InMemorySnapshotStore {
    fn load(&self, key: &str) -> Result<Option<Vec<u8>>, SystemError> {
        Ok(self.entries.get(key).cloned())
    }

    fn save(&mut self, key: &str, payload: &[u8]) -> Result<(), SystemError> {
        if self.fail_saves {
            return Err(SystemError::new(
                ErrorCode::PlatformFailure,
                "Snapshot store rejected the write",
            ));
        }
        self.entries.insert(key.to_string(), payload.to_vec());
        Ok(())
    }
}

/// A migration hook from one schema version to the next.
type MigrationFn = dyn Fn(SchemaVersion, &[u8]) -> Result<Vec<u8>, SystemError>;

/// A registered migration from one schema version to the next.
pub struct SchemaMigration {
    from: SchemaVersion,
    to: SchemaVersion,
    migrate: Box<MigrationFn>,
}

impl SchemaMigration {
    /// Registers a migration hook from `from` to `to`.
    pub fn new(
        from: SchemaVersion,
        to: SchemaVersion,
        migrate: impl Fn(SchemaVersion, &[u8]) -> Result<Vec<u8>, SystemError> + 'static,
    ) -> Self {
        Self {
            from,
            to,
            migrate: Box::new(migrate),
        }
    }

    /// The source schema version.
    pub fn from_version(&self) -> SchemaVersion {
        self.from
    }

    /// The schema version produced by a successful hop.
    pub fn to_version(&self) -> SchemaVersion {
        self.to
    }

    /// Runs the migration, returning bytes for [`Self::to_version`].
    pub fn apply(&self, payload: &[u8]) -> Result<Vec<u8>, SystemError> {
        (self.migrate)(self.from, payload)
    }
}

impl std::fmt::Debug for SchemaMigration {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SchemaMigration")
            .field("from", &self.from)
            .field("to", &self.to)
            .finish_non_exhaustive()
    }
}

/// Route, focus and draft state persisted with an explicit schema version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionedSnapshot {
    /// Schema version of this payload.
    pub schema: SchemaVersion,
    /// Host generation observed when the snapshot was produced.
    pub host_generation: u64,
    /// Restored route, including parameters.
    pub route: RouteIdentity,
    /// Restored focused controller.
    pub focus: Option<FocusTarget>,
    /// Restored field drafts, including selection and composition.
    pub drafts: BTreeMap<String, FieldDraft>,
}

impl VersionedSnapshot {
    /// Builds an empty snapshot for a route.
    pub fn new(schema: SchemaVersion, host_generation: u64, route: RouteIdentity) -> Self {
        Self {
            schema,
            host_generation,
            route,
            focus: None,
            drafts: BTreeMap::new(),
        }
    }

    /// Encodes the snapshot as deterministic bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        push_u32(&mut out, self.schema.0);
        push_u64(&mut out, self.host_generation);
        push_str(&mut out, &self.route.name);
        push_u32(&mut out, self.route.params.len() as u32);
        for (key, value) in &self.route.params {
            push_str(&mut out, key);
            push_str(&mut out, value);
        }
        match &self.focus {
            Some(focus) => {
                out.push(1);
                push_str(&mut out, &focus.0);
            }
            None => out.push(0),
        }
        push_u32(&mut out, self.drafts.len() as u32);
        for draft in self.drafts.values() {
            push_str(&mut out, &draft.field);
            push_str(&mut out, &draft.committed);
            push_str(&mut out, &draft.draft);
            push_u32(&mut out, draft.selection_utf16.anchor);
            push_u32(&mut out, draft.selection_utf16.focus);
            push_u32(&mut out, draft.selection_bytes.anchor);
            push_u32(&mut out, draft.selection_bytes.focus);
            out.push(u8::from(draft.composing));
        }
        out
    }

    /// Decodes and validates a snapshot payload.
    pub fn decode(bytes: &[u8]) -> Result<Self, SystemError> {
        let mut reader = Reader::new(bytes);
        if reader.take(MAGIC.len())? != MAGIC {
            return Err(corrupt());
        }
        let schema = SchemaVersion(reader.u32()?);
        let host_generation = reader.u64()?;
        let name = reader.string()?;
        let parameter_count = reader.u32()? as usize;
        let mut params = BTreeMap::new();
        for _ in 0..parameter_count {
            let key = reader.string()?;
            let value = reader.string()?;
            params.insert(key, value);
        }
        let focus = match reader.u8()? {
            0 => None,
            1 => Some(FocusTarget(reader.string()?)),
            _ => return Err(corrupt()),
        };
        let draft_count = reader.u32()? as usize;
        let mut drafts = BTreeMap::new();
        for _ in 0..draft_count {
            let field = reader.string()?;
            let committed = reader.string()?;
            let draft = reader.string()?;
            let selection_utf16 = SelectionRange {
                anchor: reader.u32()?,
                focus: reader.u32()?,
            };
            let selection_bytes = SelectionRange {
                anchor: reader.u32()?,
                focus: reader.u32()?,
            };
            let composing = match reader.u8()? {
                0 => false,
                1 => true,
                _ => return Err(corrupt()),
            };
            let value = FieldDraft {
                field: field.clone(),
                committed,
                draft,
                selection_utf16,
                selection_bytes,
                composing,
            };
            drafts.insert(field, value);
        }
        if !reader.finished() {
            return Err(corrupt());
        }
        Ok(Self {
            schema,
            host_generation,
            route: RouteIdentity { name, params },
            focus,
            drafts,
        })
    }

    /// Reads only the schema version header.
    pub fn peek_schema(bytes: &[u8]) -> Result<SchemaVersion, SystemError> {
        let mut reader = Reader::new(bytes);
        if reader.take(MAGIC.len())? != MAGIC {
            return Err(corrupt());
        }
        Ok(SchemaVersion(reader.u32()?))
    }
}

fn corrupt() -> SystemError {
    SystemError::new(ErrorCode::IntegrityFailure, "Snapshot payload is corrupt")
}

fn schema_mismatch(found: SchemaVersion, expected: SchemaVersion) -> RestoreOutcome {
    RestoreOutcome::SchemaMismatch { found, expected }
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_str(out: &mut Vec<u8>, value: &str) {
    push_u32(out, value.len() as u32);
    out.extend_from_slice(value.as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], SystemError> {
        let end = self.offset.checked_add(len).ok_or_else(corrupt)?;
        let slice = self.bytes.get(self.offset..end).ok_or_else(corrupt)?;
        self.offset = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, SystemError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, SystemError> {
        let bytes = self.take(4)?;
        let array = <[u8; 4]>::try_from(bytes).map_err(|_| corrupt())?;
        Ok(u32::from_le_bytes(array))
    }

    fn u64(&mut self) -> Result<u64, SystemError> {
        let bytes = self.take(8)?;
        let array = <[u8; 8]>::try_from(bytes).map_err(|_| corrupt())?;
        Ok(u64::from_le_bytes(array))
    }

    fn string(&mut self) -> Result<String, SystemError> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| corrupt())
    }

    fn finished(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

/// Coordinates versioned saves, migration and generation-checked restores.
#[derive(Debug)]
pub struct RestorationCoordinator<S = InMemorySnapshotStore> {
    store: S,
    schema: SchemaVersion,
    host_generation: u64,
    migrations: BTreeMap<SchemaVersion, SchemaMigration>,
    last_outcome: Option<RestoreOutcome>,
}

impl<S: SnapshotStore> RestorationCoordinator<S> {
    /// Builds a coordinator for the current schema and a store.
    pub fn new(schema: SchemaVersion, store: S) -> Self {
        Self {
            store,
            schema,
            host_generation: 0,
            migrations: BTreeMap::new(),
            last_outcome: None,
        }
    }

    /// The current schema version.
    pub fn schema(&self) -> SchemaVersion {
        self.schema
    }

    /// The underlying store.
    pub fn store(&self) -> &S {
        &self.store
    }

    /// Mutable access to the underlying store.
    pub fn store_mut(&mut self) -> &mut S {
        &mut self.store
    }

    /// The live host generation used to reject late restores.
    pub fn host_generation(&self) -> u64 {
        self.host_generation
    }

    /// Raises the live host generation; older values are ignored.
    pub fn set_host_generation(&mut self, generation: u64) {
        self.host_generation = self.host_generation.max(generation);
    }

    /// Advances the live host generation by one.
    pub fn advance_host_generation(&mut self) -> u64 {
        self.host_generation = self.host_generation.saturating_add(1);
        self.host_generation
    }

    /// Registers a migration, replacing one for the same source version.
    pub fn register_migration(&mut self, migration: SchemaMigration) -> Option<SchemaMigration> {
        self.migrations.insert(migration.from_version(), migration)
    }

    /// Registers a migration hop from a closure.
    pub fn with_migration(
        mut self,
        from: SchemaVersion,
        to: SchemaVersion,
        migrate: impl Fn(SchemaVersion, &[u8]) -> Result<Vec<u8>, SystemError> + 'static,
    ) -> Self {
        let migration = SchemaMigration::new(from, to, migrate);
        self.register_migration(migration);
        self
    }

    /// The most recent restore or save outcome.
    pub fn last_outcome(&self) -> Option<&RestoreOutcome> {
        self.last_outcome.as_ref()
    }

    /// Encodes and stores a snapshot.
    ///
    /// A schema mismatch or store rejection is recorded as a recoverable
    /// outcome; neither case panics.
    pub fn save(&mut self, key: &str, snapshot: &VersionedSnapshot) -> Result<(), SystemError> {
        if snapshot.schema != self.schema {
            let found = snapshot.schema;
            let expected = self.schema;
            self.last_outcome = Some(schema_mismatch(found, expected));
            return Err(SystemError::new(
                ErrorCode::InvalidConfiguration,
                "Snapshot schema is not current",
            ));
        }
        let payload = snapshot.encode();
        match self.store.save(key, &payload) {
            Ok(()) => {
                self.last_outcome = None;
                Ok(())
            }
            Err(error) => {
                self.last_outcome = Some(RestoreOutcome::SaveFailed);
                Err(error)
            }
        }
    }

    /// Loads, migrates and validates a snapshot for a restore request.
    pub fn restore(&mut self, key: &str, request_generation: u64) -> RestoreOutcome {
        let outcome = self.restore_inner(key, request_generation);
        self.last_outcome = Some(outcome.clone());
        outcome
    }

    /// Restores a snapshot into scene state only when the callback is current.
    pub fn restore_into(
        &mut self,
        key: &str,
        request_generation: u64,
        state: &mut SceneState,
    ) -> RestoreOutcome {
        let outcome = self.restore(key, request_generation);
        match &outcome {
            RestoreOutcome::Restored(snapshot) => state.apply_snapshot(snapshot),
            RestoreOutcome::Migrated { snapshot, .. } => state.apply_snapshot(snapshot),
            _ => {}
        }
        outcome
    }

    fn restore_inner(&mut self, key: &str, request_generation: u64) -> RestoreOutcome {
        if request_generation < self.host_generation {
            let found = request_generation;
            let current = self.host_generation;
            return RestoreOutcome::StaleGeneration { found, current };
        }
        let mut payload = match self.store.load(key) {
            Ok(Some(payload)) => payload,
            Ok(None) => return RestoreOutcome::AbsentSnapshot,
            Err(_) => return RestoreOutcome::CorruptSnapshot,
        };
        let initial = match VersionedSnapshot::peek_schema(&payload) {
            Ok(initial) => initial,
            Err(_) => return RestoreOutcome::CorruptSnapshot,
        };
        if initial == self.schema {
            return match VersionedSnapshot::decode(&payload) {
                Ok(snapshot) => RestoreOutcome::Restored(snapshot),
                Err(_) => RestoreOutcome::CorruptSnapshot,
            };
        }
        if initial > self.schema {
            return schema_mismatch(initial, self.schema);
        }
        let mut current = initial;
        for _ in 0..MAX_MIGRATION_HOPS {
            if current == self.schema {
                break;
            }
            let Some(migration) = self.migrations.get(&current) else {
                return schema_mismatch(initial, self.schema);
            };
            let next = migration.to_version();
            if next <= current || next > self.schema {
                return schema_mismatch(initial, self.schema);
            }
            payload = match migration.apply(&payload) {
                Ok(payload) => payload,
                Err(_) => return schema_mismatch(initial, self.schema),
            };
            let encoded = match VersionedSnapshot::peek_schema(&payload) {
                Ok(encoded) => encoded,
                Err(_) => return RestoreOutcome::CorruptSnapshot,
            };
            if encoded != next {
                return schema_mismatch(initial, self.schema);
            }
            current = next;
        }
        if current != self.schema {
            return schema_mismatch(initial, self.schema);
        }
        match VersionedSnapshot::decode(&payload) {
            Ok(snapshot) => RestoreOutcome::Migrated {
                from: initial,
                snapshot,
            },
            Err(_) => RestoreOutcome::CorruptSnapshot,
        }
    }
}
