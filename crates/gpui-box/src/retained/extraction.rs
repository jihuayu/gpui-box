//! Shared full/subtree extraction from persistent layout and visual properties.
use super::*;

impl UiTree {
    /// Property-only updates retain owned local paint/text inputs. Resolve the
    /// same ordered f32 operations as full extraction, without consulting Taffy,
    /// re-preparing glyph chunks, or looking up absent sparse property entries.
    pub(super) fn resolve_property_subtree(
        &self,
        before: &FramePacket,
        range: std::ops::Range<usize>,
        origin: [f32; 2],
        clip: Option<Rect>,
        state: PropertyState,
        edits: &mut properties::PropertyEdits,
    ) -> Result<Vec<PaintItem>, Error> {
        let mut result: Vec<PaintItem> = Vec::with_capacity(range.len());
        // Preorder permits a stack of ancestors instead of a NodeId lookup for
        // every resolved parent. One context per depth, including the boundary.
        let mut stack = vec![(None, origin, clip, state)];
        for index in range {
            let previous = &before.items[index];
            let node = self.node(previous.node)?;
            while stack.len() > 1 && stack.last().unwrap().0 != node.parent {
                stack.pop();
            }
            let (_, origin, inherited_clip, mut state) = *stack.last().unwrap();
            let mut item = previous.clone();
            let spatial = &mut item.spatial;
            spatial.translation = node.translation;
            spatial.clip = node.clip;
            // No layout input changed, so the published clamped scroll extent
            // is still valid; the requested offset itself may have changed.
            let layout = &node.published_layout;
            spatial.scroll = [
                node.scroll[0].min(layout.scroll_width()),
                node.scroll[1].min(layout.scroll_height()),
            ];
            let x = origin[0] + spatial.layout_origin[0] + spatial.translation[0];
            let y = origin[1] + spatial.layout_origin[1] + spatial.translation[1];
            let transform = (node.translation != [0.0; 2]).then_some(PropertyNode {
                parent: state.transform,
                value: node.translation,
            });
            let own_clip = node.clip.map(|value| PropertyNode {
                parent: state.clip,
                value,
            });
            let effect = (node.opacity != 1.0).then_some(PropertyNode {
                parent: state.effect,
                value: node.opacity,
            });
            if transform.is_some() || previous.properties.transform == Some(item.node) {
                edits.transforms.push((item.node, transform));
            }
            if own_clip.is_some() || previous.properties.clip == Some(item.node) {
                edits.clips.push((item.node, own_clip));
            }
            if effect.is_some() || previous.properties.effect == Some(item.node) {
                edits.effects.push((item.node, effect));
            }
            if node.translation != [0.0; 2] {
                state.transform = Some(item.node);
            }
            if node.clip.is_some() {
                state.clip = Some(item.node);
            }
            if node.opacity != 1.0 {
                state.effect = Some(item.node);
            }
            item.properties = state;
            if node.scroll != [0.0; 2] {
                edits.scrolls.push((
                    item.node,
                    Some(PropertyNode {
                        parent: state.scroll,
                        value: spatial.scroll,
                    }),
                ));
                state.scroll = Some(item.node);
            } else if before.properties.scrolls.contains_key(&item.node) {
                edits.scrolls.push((item.node, None));
            }
            let own_clip = node.clip.map(|clip| clip.translated([x, y]));
            item.clip = match (inherited_clip, own_clip) {
                (Some(a), Some(b)) => Some(a.intersect(b)),
                (a, b) => a.or(b),
            };
            item.bounds.x = x;
            item.bounds.y = y;
            item.content_bounds.x = (x + item.content_offsets[0]) + item.content_offsets[2];
            item.content_bounds.y = (y + item.content_offsets[1]) + item.content_offsets[3];
            stack.push((
                Some(item.node),
                [x - spatial.scroll[0], y - spatial.scroll[1]],
                item.clip,
                state,
            ));
            result.push(item);
        }
        Ok(result)
    }

    pub(super) fn extract_subtree(
        &mut self,
        root: NodeId,
        origin: [f32; 2],
        inherited_clip: Option<Rect>,
        state: PropertyState,
        properties: &mut impl properties::PropertyOutput,
        recorded_chunks: &mut u64,
    ) -> Result<Vec<PaintItem>, Error> {
        self.extract(
            root,
            (origin, inherited_clip, state),
            properties,
            recorded_chunks,
            true,
        )
    }

    pub(super) fn extract_node(
        &mut self,
        root: NodeId,
        context: ([f32; 2], Option<Rect>, PropertyState),
        properties: &mut impl properties::PropertyOutput,
        recorded_chunks: &mut u64,
    ) -> Result<PaintItem, Error> {
        Ok(self
            .extract(root, context, properties, recorded_chunks, false)?
            .pop()
            .expect("visible indexed node"))
    }

    fn extract(
        &mut self,
        root: NodeId,
        (origin, inherited_clip, state): ([f32; 2], Option<Rect>, PropertyState),
        properties: &mut impl properties::PropertyOutput,
        recorded_chunks: &mut u64,
        descendants: bool,
    ) -> Result<Vec<PaintItem>, Error> {
        let mut items = vec![];
        let mut stack = vec![(root, origin, inherited_clip, state)];
        while let Some((id, origin, inherited_clip, mut state)) = stack.pop() {
            let node = self.node(id)?;
            if node.style.display == taffy::Display::None {
                continue;
            }
            let layout = *self.layout.layout(node.layout)?;
            let x = origin[0] + layout.location.x + node.translation[0];
            let y = origin[1] + layout.location.y + node.translation[1];
            let transform_property = (node.translation != [0.0; 2]).then_some(PropertyNode {
                parent: state.transform,
                value: node.translation,
            });
            if transform_property.is_some() {
                state.transform = Some(id);
            }
            let clip = node.clip.map(|clip| clip.translated([x, y]));
            let clip_property = node.clip.map(|value| PropertyNode {
                parent: state.clip,
                value,
            });
            if clip_property.is_some() {
                state.clip = Some(id);
            }
            let clip = match (inherited_clip, clip) {
                (Some(a), Some(b)) => Some(a.intersect(b)),
                (a, b) => a.or(b),
            };
            let effect_property = (node.opacity != 1.0).then_some(PropertyNode {
                parent: state.effect,
                value: node.opacity,
            });
            if effect_property.is_some() {
                state.effect = Some(id);
            }
            let paint_state = state;
            let scroll = [
                node.scroll[0].min(layout.scroll_width()),
                node.scroll[1].min(layout.scroll_height()),
            ];
            let scroll_property = (node.scroll != [0.0; 2]).then_some(PropertyNode {
                parent: state.scroll,
                value: scroll,
            });
            if scroll_property.is_some() {
                state.scroll = Some(id);
            }
            properties.record(
                id,
                transform_property,
                clip_property,
                scroll_property,
                effect_property,
            );
            if descendants {
                let children = self.ordered_children_for(id, &node.children)?;
                for child in children.iter().rev() {
                    stack.push((*child, [x - scroll[0], y - scroll[1]], clip, state));
                }
            }
            let spatial = SpatialState {
                parent: node.parent,
                layout_origin: [layout.location.x, layout.location.y],
                translation: node.translation,
                scroll,
                clip: node.clip,
                root_clip: node.parent.is_none().then_some(inherited_clip).flatten(),
            };
            let node = self.node_mut(id);
            let commands = node
                .chunk
                .get_or_insert_with(|| {
                    *recorded_chunks += 1;
                    Arc::from([if node.decoration == QuadDecoration::default() {
                        DisplayCommand::Fill(node.color)
                    } else {
                        DisplayCommand::DecoratedFill {
                            color: node.color,
                            decoration: node.decoration,
                        }
                    }])
                })
                .clone();
            #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
            let glyphs = node
                .text
                .as_ref()
                .map(|text| {
                    text.resource.chunk(
                        (layout.size.width
                            - layout.border.left
                            - layout.border.right
                            - layout.padding.left
                            - layout.padding.right)
                            .max(0.0),
                    )
                })
                .transpose()?;
            items.push(PaintItem {
                node: id,
                bounds: Rect {
                    x,
                    y,
                    width: layout.size.width,
                    height: layout.size.height,
                },
                commands,
                fill_color: node.color,
                shadows: node.shadows.clone(),
                image: node.image.clone(),
                #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                text: node.text.clone(),
                #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                glyphs,
                content_offsets: [
                    layout.border.left,
                    layout.border.top,
                    layout.padding.left,
                    layout.padding.top,
                ],
                content_bounds: Rect {
                    x: x + layout.border.left + layout.padding.left,
                    y: y + layout.border.top + layout.padding.top,
                    width: (layout.size.width
                        - layout.border.left
                        - layout.border.right
                        - layout.padding.left
                        - layout.padding.right)
                        .max(0.0),
                    height: (layout.size.height
                        - layout.border.top
                        - layout.border.bottom
                        - layout.padding.top
                        - layout.padding.bottom)
                        .max(0.0),
                },
                clip,
                properties: paint_state,
                spatial,
            });
        }
        Ok(items)
    }

    pub(super) fn index_paint_items(&mut self, items: &PaintItems) {
        self.paint_indices.clear();
        self.paint_subtree_ends.clear();
        let mut stack = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let parent = self.node(item.node).expect("extracted live node").parent;
            while stack.last().copied() != parent {
                let id = stack.pop().expect("visible preorder parent");
                self.paint_subtree_ends.insert(id, index);
            }
            self.paint_indices.insert(item.node, index);
            stack.push(item.node);
        }
        for id in stack {
            self.paint_subtree_ends.insert(id, items.len());
        }
    }
}
