use super::*;

#[derive(Clone, Debug, PartialEq)]
enum Resource {
    Image(ImageResource),
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    Text(text::TextResource),
}

/// Host-owned immutable resource bindings. Rebinding an ID to different bytes
/// or another resource kind rejects; use a new ID for new resource identity.
pub struct UiResources {
    identity: Arc<()>,
    values: BTreeMap<UiResourceId, Resource>,
}
impl Default for UiResources {
    fn default() -> Self {
        Self {
            identity: Arc::new(()),
            values: BTreeMap::new(),
        }
    }
}
impl UiResources {
    pub fn new() -> Self {
        Self::default()
    }
    fn insert(&mut self, id: UiResourceId, value: Resource) -> Result<(), UiError> {
        if self.values.get(&id).is_some_and(|old| *old != value) {
            return Err(UiError::ResourceConflict(id));
        }
        self.values.insert(id, value);
        Ok(())
    }
    pub fn image(&mut self, id: UiResourceId, image: ImageResource) -> Result<(), UiError> {
        self.insert(id, Resource::Image(image))
    }
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    pub fn text(&mut self, id: UiResourceId, text: text::TextResource) -> Result<(), UiError> {
        self.insert(id, Resource::Text(text))
    }
}

/// Complete immutable output plus its external input identity. The frame is
/// applied/CPU-published evidence, not proof of GPU submission or display.
pub struct UiPublication {
    pub status: UiApplyStatus,
    pub runtime: u64,
    pub epoch: u64,
    pub revision: u64,
    pub next_sequence: u64,
    pub commit: Commit,
    pub frame: local::LocalFrame,
    identities: Arc<BTreeMap<NodeId, UiNodeId>>,
}
impl UiPublication {
    /// Uses this publication's geometry and identity mapping, even after the
    /// live document changes, recovers, reuses a slot or is dropped.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<UiNodeId> {
        self.identities.get(&self.frame.hit_test(x, y)?).copied()
    }
}

/// Single-owner bridge from owned public source records to the persistent
/// retained layout/render tree. Keep this owner on its UI thread; the messages
/// themselves contain no engine handles, callbacks or platform borrows.
pub struct UiDocument {
    runtime: u64,
    epoch: u64,
    revision: u64,
    next_sequence: u64,
    root: UiNodeId,
    nodes: BTreeMap<UiNodeId, UiNode>,
    resolved_nodes: BTreeMap<UiNodeId, UiNode>,
    style_environment: LayoutEnvironment,
    generations: BTreeMap<u64, u64>,
    mapping: BTreeMap<UiNodeId, NodeId>,
    resources: BTreeMap<UiResourceId, Resource>,
    resource_table: Arc<()>,
    identities: Arc<BTreeMap<NodeId, UiNodeId>>,
    tree: UiTree,
    limits: UiLimits,
    last_batch: Option<Arc<UiBatch>>,
}

impl UiDocument {
    pub fn from_snapshot(
        snapshot: UiSnapshot,
        resources: &UiResources,
        limits: UiLimits,
        capabilities: BackendCapabilities,
        constraints: LayoutConstraints,
        viewport: [f32; 2],
    ) -> Result<(Self, UiPublication), UiError> {
        validate_header(snapshot.schema, snapshot.runtime, snapshot.epoch)?;
        snapshot.style_environment.validate()?;
        if snapshot.next_sequence == 0 {
            return Err(UiError::InvalidRevision);
        }
        if snapshot.nodes.len() > limits.max_nodes
            || snapshot.generations.len() > limits.max_generation_slots
        {
            return Err(UiError::LimitExceeded);
        }
        let mut nodes = BTreeMap::new();
        for node in &snapshot.nodes {
            validate_payload_bounds(node, limits)?;
        }
        // Inactive fields must also be reflexive for exact source/replay equality.
        if !PartialEq::eq(&snapshot, &snapshot) {
            return Err(UiError::NonReflexiveValue);
        }
        for node in snapshot.nodes {
            let id = node.id;
            if nodes.insert(id, node).is_some() {
                return Err(UiError::DuplicateSlot(id.slot));
            }
        }
        let mut generations = BTreeMap::new();
        for (slot, generation) in snapshot.generations {
            if generation == 0 || generations.insert(slot, generation).is_some() {
                return Err(UiError::DuplicateSlot(slot));
            }
        }
        for id in nodes.keys() {
            if generations.get(&id.slot) != Some(&id.generation) {
                return Err(UiError::InvalidIdentity(*id));
            }
        }
        validate_graph(snapshot.root, &nodes, limits)?;
        let resolved_nodes = resolve_styles(&nodes, &snapshot.style_environment, limits)?;
        validate_graph(snapshot.root, &resolved_nodes, limits)?;
        let pinned = pin_resources(&nodes, resources, &BTreeMap::new())?;
        let root_node = &resolved_nodes[&snapshot.root];
        let mut tree = UiTree::new_layout(root_node.layout.clone(), root_node.properties.color)?;
        let mut mapping = BTreeMap::from([(snapshot.root, tree.root())]);
        let empty = BTreeMap::new();
        let result = stage(
            &mut tree,
            &empty,
            &resolved_nodes,
            &mut mapping,
            &pinned,
            capabilities,
            (constraints, viewport),
        )?;
        let frame = tree
            .published_local_frame()
            .expect("validated local publication")
            .clone();
        let identities = identities(&mapping);
        let publication = UiPublication {
            status: UiApplyStatus::Applied,
            runtime: snapshot.runtime,
            epoch: snapshot.epoch,
            revision: snapshot.revision,
            next_sequence: snapshot.next_sequence,
            commit: result,
            frame,
            identities: identities.clone(),
        };
        Ok((
            Self {
                runtime: snapshot.runtime,
                epoch: snapshot.epoch,
                revision: snapshot.revision,
                next_sequence: snapshot.next_sequence,
                root: snapshot.root,
                nodes,
                resolved_nodes,
                style_environment: snapshot.style_environment,
                generations,
                mapping,
                resources: pinned,
                resource_table: resources.identity.clone(),
                identities,
                tree,
                limits,
                last_batch: None,
            },
            publication,
        ))
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }
    pub fn node(&self, id: UiNodeId) -> Option<&UiNode> {
        self.nodes.get(&id)
    }
    /// Selected layout/visual values after ordered style composition. Authored
    /// declarations and state remain available through node()/snapshot().
    pub fn resolved_node(&self, id: UiNodeId) -> Option<&UiNode> {
        self.resolved_nodes.get(&id)
    }
    pub fn style_environment(&self) -> &LayoutEnvironment {
        &self.style_environment
    }
    pub fn layout_result(&self, id: UiNodeId) -> Result<LayoutResult, UiError> {
        let id = self.mapping.get(&id).ok_or(UiError::InvalidIdentity(id))?;
        Ok(self.tree.layout_result(*id)?)
    }
    pub fn published_frame(&self) -> local::LocalFrame {
        self.tree
            .published_local_frame()
            .expect("document always publishes a complete frame")
            .clone()
    }
    /// Map the current published hit back to its source identity. Old frame hit
    /// results must be interpreted using that frame's own mapping/receipt.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<UiNodeId> {
        let node = self.tree.published_local_frame()?.hit_test(x, y)?;
        self.mapping
            .iter()
            .find_map(|(id, internal)| (*internal == node).then_some(*id))
    }
    pub fn snapshot(&self) -> UiSnapshot {
        UiSnapshot {
            schema: UI_SCHEMA_VERSION,
            runtime: self.runtime,
            epoch: self.epoch,
            revision: self.revision,
            next_sequence: self.next_sequence,
            root: self.root,
            nodes: self.nodes.values().cloned().collect(),
            generations: self
                .generations
                .iter()
                .map(|(&slot, &generation)| (slot, generation))
                .collect(),
            style_environment: self.style_environment.clone(),
        }
    }

    /// Publish the complete final batch and its frame atomically. A replay of
    /// the last exactly equal message returns its original frame without
    /// reapplying operations or adopting different host inputs.
    pub fn apply(
        &mut self,
        batch: UiBatch,
        resources: &UiResources,
        capabilities: BackendCapabilities,
        constraints: LayoutConstraints,
        viewport: [f32; 2],
    ) -> Result<UiPublication, UiError> {
        validate_header(batch.schema, batch.runtime, batch.epoch)?;
        if batch.runtime != self.runtime {
            return Err(UiError::WrongRuntime);
        }
        if batch.epoch != self.epoch {
            return Err(UiError::WrongEpoch);
        }
        if !Arc::ptr_eq(&self.resource_table, &resources.identity) {
            return Err(UiError::WrongResourceTable);
        }
        if batch.operations.len() > self.limits.max_operations {
            return Err(UiError::LimitExceeded);
        }
        for operation in &batch.operations {
            match operation {
                UiOperation::Create(node) => validate_payload_bounds(node, self.limits)?,
                UiOperation::Layout { value, .. } => validate_style_bounds(value, self.limits)?,
                UiOperation::Properties { value, .. }
                    if value.shadows.len() > self.limits.max_shadows_per_node =>
                {
                    return Err(UiError::LimitExceeded);
                }
                UiOperation::Children { value, .. }
                    if value.len() > self.limits.max_children_per_node =>
                {
                    return Err(UiError::LimitExceeded);
                }
                UiOperation::Styles { value, .. } => validate_style_refs(value, self.limits)?,
                UiOperation::StyleEnvironment(value) => value.validate()?,
                _ => {}
            }
        }
        if !PartialEq::eq(&batch, &batch) {
            return Err(UiError::NonReflexiveValue);
        }
        if self
            .last_batch
            .as_ref()
            .is_some_and(|last| last.sequence == batch.sequence)
        {
            if self.last_batch.as_deref() != Some(&batch) {
                return Err(UiError::ReplayConflict);
            }
            let frame = self.published_frame();
            return Ok(UiPublication {
                status: UiApplyStatus::Replayed,
                runtime: self.runtime,
                epoch: self.epoch,
                revision: self.revision,
                next_sequence: self.next_sequence,
                commit: Commit {
                    revision: frame.revision(),
                    ..Commit::default()
                },
                frame,
                identities: self.identities.clone(),
            });
        }
        if batch.base_revision != self.revision || batch.sequence != self.next_sequence {
            return Err(UiError::ResyncRequired {
                expected_revision: self.revision,
                expected_sequence: self.next_sequence,
            });
        }
        if self.revision.checked_add(1) != Some(batch.revision) {
            return Err(UiError::InvalidRevision);
        }
        let next_sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or(UiError::InvalidRevision)?;
        if batch.operations.len() > self.limits.max_operations {
            return Err(UiError::LimitExceeded);
        }
        let mut nodes = self.nodes.clone();
        let mut style_environment = self.style_environment.clone();
        let mut generations = self.generations.clone();
        let mut live_slots: BTreeSet<_> = nodes.keys().map(|id| id.slot).collect();
        let mut removed = BTreeSet::new();
        for operation in &batch.operations {
            match operation {
                UiOperation::Create(node) => {
                    validate_payload_bounds(node, self.limits)?;
                    if node.id.generation == 0
                        || generations
                            .get(&node.id.slot)
                            .is_some_and(|old| node.id.generation <= *old)
                        || !live_slots.insert(node.id.slot)
                    {
                        return Err(UiError::InvalidIdentity(node.id));
                    }
                    generations.insert(node.id.slot, node.id.generation);
                    nodes.insert(node.id, node.clone());
                    if nodes.len() > self.limits.max_nodes
                        || generations.len() > self.limits.max_generation_slots
                    {
                        return Err(UiError::LimitExceeded);
                    }
                }
                UiOperation::Layout { node, value } => {
                    validate_style_bounds(value, self.limits)?;
                    value.validate()?;
                    get_mut(&mut nodes, *node)?.layout = value.clone()
                }
                UiOperation::Properties { node, value } => {
                    if value.shadows.len() > self.limits.max_shadows_per_node {
                        return Err(UiError::LimitExceeded);
                    }
                    validate_properties(value)?;
                    get_mut(&mut nodes, *node)?.properties = value.clone()
                }
                UiOperation::Content { node, value } => {
                    validate_content(value)?;
                    get_mut(&mut nodes, *node)?.content = value.clone()
                }
                UiOperation::Children { node, value } => {
                    if value.len() > self.limits.max_children_per_node {
                        return Err(UiError::LimitExceeded);
                    }
                    get_mut(&mut nodes, *node)?.children = value.clone()
                }
                UiOperation::Styles { node, value } => {
                    get_mut(&mut nodes, *node)?.styles = value.clone()
                }
                UiOperation::StyleState { node, value } => {
                    get_mut(&mut nodes, *node)?.style_state = *value
                }
                UiOperation::StyleEnvironment(value) => style_environment = value.clone(),
                UiOperation::RemoveSubtree(id) => {
                    if *id == self.root || !nodes.contains_key(id) {
                        return Err(UiError::InvalidIdentity(*id));
                    }
                    removed.insert(*id);
                }
            }
        }
        let mut stack: Vec<_> = removed.iter().copied().collect();
        let mut dead = BTreeSet::new();
        while let Some(id) = stack.pop() {
            if dead.insert(id) {
                let node = nodes.get(&id).ok_or(UiError::InvalidIdentity(id))?;
                stack.extend(node.children.iter().copied());
            }
        }
        if dead.contains(&self.root) {
            return Err(UiError::InvalidTopology);
        }
        nodes.retain(|id, _| !dead.contains(id));
        for node in nodes.values_mut() {
            node.children.retain(|id| !dead.contains(id));
        }
        validate_graph(self.root, &nodes, self.limits)?;
        let resolved_nodes = resolve_styles(&nodes, &style_environment, self.limits)?;
        validate_graph(self.root, &resolved_nodes, self.limits)?;
        let pinned = pin_resources(&nodes, resources, &self.resources)?;
        let mut mapping = self.mapping.clone();
        let commit = stage(
            &mut self.tree,
            &self.resolved_nodes,
            &resolved_nodes,
            &mut mapping,
            &pinned,
            capabilities,
            (constraints, viewport),
        )?;
        self.nodes = nodes;
        self.resolved_nodes = resolved_nodes;
        self.style_environment = style_environment;
        self.generations = generations;
        self.mapping = mapping;
        self.identities = identities(&self.mapping);
        self.resources = pinned;
        self.revision = batch.revision;
        self.next_sequence = next_sequence;
        self.last_batch = Some(Arc::new(batch));
        Ok(UiPublication {
            status: UiApplyStatus::Applied,
            runtime: self.runtime,
            epoch: self.epoch,
            revision: self.revision,
            next_sequence,
            commit,
            frame: self.published_frame(),
            identities: self.identities.clone(),
        })
    }

    /// Adopt a newer complete state of the same runtime/epoch. Recovery cannot
    /// regress accepted revision/sequence or forget generation tombstones.
    /// A new epoch is a new document; the host explicitly retires this owner.
    pub fn recover(
        &mut self,
        snapshot: UiSnapshot,
        resources: &UiResources,
        capabilities: BackendCapabilities,
        constraints: LayoutConstraints,
        viewport: [f32; 2],
    ) -> Result<UiPublication, UiError> {
        validate_header(snapshot.schema, snapshot.runtime, snapshot.epoch)?;
        if snapshot.runtime != self.runtime {
            return Err(UiError::WrongRuntime);
        }
        if snapshot.epoch != self.epoch {
            return Err(UiError::WrongEpoch);
        }
        if !Arc::ptr_eq(&self.resource_table, &resources.identity) {
            return Err(UiError::WrongResourceTable);
        }
        if snapshot.revision <= self.revision || snapshot.next_sequence <= self.next_sequence {
            return Err(UiError::ResyncRequired {
                expected_revision: self.revision,
                expected_sequence: self.next_sequence,
            });
        }
        let (next, publication) = Self::from_snapshot(
            snapshot,
            resources,
            self.limits,
            capabilities,
            constraints,
            viewport,
        )?;
        if self.generations.iter().any(|(slot, generation)| {
            next.generations
                .get(slot)
                .is_none_or(|next| next < generation)
        }) {
            return Err(UiError::InvalidTopology);
        }
        for id in next.nodes.keys() {
            if !self.nodes.contains_key(id)
                && self
                    .generations
                    .get(&id.slot)
                    .is_some_and(|old| id.generation <= *old)
            {
                return Err(UiError::InvalidIdentity(*id));
            }
        }
        for (id, resource) in &next.resources {
            if self.resources.get(id).is_some_and(|old| old != resource) {
                return Err(UiError::ResourceConflict(*id));
            }
        }
        *self = next;
        Ok(publication)
    }
}

fn get_mut(nodes: &mut BTreeMap<UiNodeId, UiNode>, id: UiNodeId) -> Result<&mut UiNode, UiError> {
    nodes.get_mut(&id).ok_or(UiError::InvalidIdentity(id))
}

fn identities(mapping: &BTreeMap<UiNodeId, NodeId>) -> Arc<BTreeMap<NodeId, UiNodeId>> {
    Arc::new(
        mapping
            .iter()
            .map(|(&id, &internal)| (internal, id))
            .collect(),
    )
}

fn pin_resources(
    nodes: &BTreeMap<UiNodeId, UiNode>,
    input: &UiResources,
    previous: &BTreeMap<UiResourceId, Resource>,
) -> Result<BTreeMap<UiResourceId, Resource>, UiError> {
    let mut pinned = BTreeMap::new();
    for node in nodes.values() {
        #[cfg(not(all(feature = "retained-text", not(target_family = "wasm"))))]
        if matches!(node.content, UiContent::Text { .. }) {
            return Err(UiError::UnsupportedContent);
        }
        let id = match node.content {
            UiContent::Image(id) | UiContent::Text { resource: id, .. } => id,
            _ => continue,
        };
        let resource = input
            .values
            .get(&id)
            .or_else(|| previous.get(&id))
            .ok_or(UiError::MissingResource(id))?;
        if previous.get(&id).is_some_and(|old| old != resource) {
            return Err(UiError::ResourceConflict(id));
        }
        match (&node.content, resource) {
            (UiContent::Image(_), Resource::Image(_)) => {}
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            (UiContent::Text { .. }, Resource::Text(_)) => {}
            _ => return Err(UiError::UnsupportedContent),
        }
        pinned.insert(id, resource.clone());
    }
    Ok(pinned)
}

fn stage(
    tree: &mut UiTree,
    previous: &BTreeMap<UiNodeId, UiNode>,
    nodes: &BTreeMap<UiNodeId, UiNode>,
    mapping: &mut BTreeMap<UiNodeId, NodeId>,
    resources: &BTreeMap<UiResourceId, Resource>,
    capabilities: BackendCapabilities,
    host: (LayoutConstraints, [f32; 2]),
) -> Result<Commit, UiError> {
    let (constraints, viewport) = host;
    let root = tree.root();
    let creates_nodes = nodes.keys().any(|id| !mapping.contains_key(id));
    let result = tree.edit_validated(
        capabilities,
        |edit| {
            for (&id, node) in nodes {
                if let std::collections::btree_map::Entry::Vacant(entry) = mapping.entry(id) {
                    let internal =
                        edit.append_layout(root, node.layout.clone(), node.properties.color)?;
                    entry.insert(internal);
                }
            }
            // Assign every final list before removing dead nodes. This permits
            // rescuing a survivor from a retiring host, independent of op order.
            for (&id, node) in nodes {
                let internal = mapping[&id];
                if (internal == root && creates_nodes)
                    || previous
                        .get(&id)
                        .is_none_or(|old| old.children != node.children)
                {
                    edit.set_children(
                        internal,
                        node.children.iter().map(|id| mapping[id]).collect(),
                    )?;
                }
            }
            for (&id, &internal) in mapping.iter() {
                if !nodes.contains_key(&id) {
                    edit.set_children(internal, vec![])?;
                    edit.remove(internal)?;
                }
            }
            for (&id, node) in nodes {
                let old = previous.get(&id);
                let internal = mapping[&id];
                let tx = edit.properties();
                if old.is_none_or(|old| old.layout != node.layout) {
                    tx.layout_style(internal, node.layout.clone());
                }
                if old.is_none_or(|old| old.properties != node.properties) {
                    let p = &node.properties;
                    tx.color(internal, p.color);
                    tx.translation(internal, p.translation);
                    tx.clip(internal, p.clip);
                    tx.scroll(internal, p.scroll);
                    tx.opacity(internal, p.opacity);
                    tx.decoration(internal, p.decoration);
                    tx.shadows(internal, p.shadows.to_vec());
                }
                if old.is_none_or(|old| old.content != node.content) {
                    write_content(tx, internal, &node.content, resources);
                }
            }
            Ok(())
        },
        |draft| {
            draft
                .local_frame_in_viewport(constraints, viewport)?
                .validate_geometry()?;
            Ok(())
        },
    )?;
    mapping.retain(|id, _| nodes.contains_key(id));
    Ok(result.commit)
}

fn write_content(
    tx: &mut Transaction,
    node: NodeId,
    content: &UiContent,
    resources: &BTreeMap<UiResourceId, Resource>,
) {
    // Resource transitions must clear old intrinsic owners before the final
    // replacement is validated. Box/Fixed alone explicitly author measurement.
    tx.image(node, None);
    #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
    tx.text(node, None);
    match content {
        UiContent::Box => tx.measure(node, None),
        UiContent::Fixed {
            size,
            first_baseline,
            last_baseline,
        } => tx.measure(
            node,
            Some(MeasureSnapshot::FixedBaselines(BaselineMeasureSnapshot {
                width: size[0],
                height: size[1],
                first_baseline: *first_baseline,
                last_baseline: *last_baseline,
            })),
        ),
        UiContent::Image(id) => {
            let image = match &resources[id] {
                Resource::Image(image) => image,
                #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                _ => unreachable!("validated image resource"),
            };
            tx.image(node, Some(image.clone()));
        }
        UiContent::Text { resource, color } => {
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            {
                let Resource::Text(text) = &resources[resource] else {
                    unreachable!("validated text resource")
                };
                tx.text(
                    node,
                    Some(text::TextPaint {
                        resource: text.clone(),
                        color: *color,
                    }),
                );
            }
            #[cfg(not(all(feature = "retained-text", not(target_family = "wasm"))))]
            {
                let _ = (resource, color);
                unreachable!("unsupported text rejected before staging")
            }
        }
    }
}
