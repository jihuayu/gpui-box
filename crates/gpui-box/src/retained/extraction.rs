//! Shared full/subtree extraction from persistent layout and visual properties.
use super::*;

impl UiTree {
    pub(super) fn extract_subtree(
        &mut self,
        root: NodeId,
        origin: [f32; 2],
        inherited_clip: Option<Rect>,
        state: PropertyState,
        properties: &mut PropertyTrees,
        recorded_chunks: &mut u64,
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
            if node.translation != [0.0; 2] {
                properties.transforms.insert(
                    id,
                    PropertyNode {
                        parent: state.transform,
                        value: node.translation,
                    },
                );
                state.transform = Some(id);
            }
            let clip = node.clip.map(|clip| clip.translated([x, y]));
            if let Some(local_clip) = node.clip {
                properties.clips.insert(
                    id,
                    PropertyNode {
                        parent: state.clip,
                        value: local_clip,
                    },
                );
                state.clip = Some(id);
            }
            let clip = match (inherited_clip, clip) {
                (Some(a), Some(b)) => Some(a.intersect(b)),
                (a, b) => a.or(b),
            };
            if node.opacity != 1.0 {
                properties.effects.insert(
                    id,
                    PropertyNode {
                        parent: state.effect,
                        value: node.opacity,
                    },
                );
                state.effect = Some(id);
            }
            let paint_state = state;
            let scroll = [
                node.scroll[0].min(layout.scroll_width()),
                node.scroll[1].min(layout.scroll_height()),
            ];
            if node.scroll != [0.0; 2] {
                properties.scrolls.insert(
                    id,
                    PropertyNode {
                        parent: state.scroll,
                        value: scroll,
                    },
                );
                state.scroll = Some(id);
            }
            for child in node.children.iter().rev() {
                stack.push((*child, [x - scroll[0], y - scroll[1]], clip, state));
            }
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
                image: node.image.clone(),
                #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                text: node.text.clone(),
                #[cfg(all(feature = "retained-text", not(target_family = "wasm")))]
                glyphs,
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
