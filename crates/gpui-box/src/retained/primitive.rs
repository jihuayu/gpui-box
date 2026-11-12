//! Primitive data shared by CPU commands, damage and backend instance conversion.
use super::*;

/// Uniform circular corners and an inside solid border, in logical pixels.
/// Large radii/widths are clamped to half the allocated box's shortest edge.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuadDecoration {
    pub radius: f32,
    pub border_width: f32,
    pub border_color: u32,
}
impl QuadDecoration {
    pub(super) fn validate(self) -> Result<(), Error> {
        if [self.radius, self.border_width]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
        {
            return Err(Error::InvalidPrimitive);
        }
        Ok(())
    }
    pub fn resolved(self, bounds: Rect) -> Self {
        let limit = (bounds.width.min(bounds.height) / 2.0).max(0.0);
        Self {
            radius: self.radius.min(limit),
            border_width: self.border_width.min(limit),
            ..self
        }
    }
}

/// GPUI-compatible rounded box shadow in logical pixels. Drop shadows paint
/// before the fill, inset shadows after it; list order is preserved per phase.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoxShadow {
    pub color: u32,
    pub offset: [f32; 2],
    pub blur_radius: f32,
    pub spread_radius: f32,
    pub inset: bool,
}
impl BoxShadow {
    pub(super) fn validate(self) -> Result<(), Error> {
        if [
            self.offset[0],
            self.offset[1],
            self.blur_radius * 6.0,
            self.spread_radius * 2.0,
        ]
        .iter()
        .any(|v| !v.is_finite())
            || self.blur_radius < 0.0
        {
            return Err(Error::InvalidPrimitive);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowGeometry {
    pub bounds: Rect,
    pub radius: f32,
    pub element_bounds: Rect,
    pub element_radius: f32,
    pub ink: Rect,
}
impl PaintItem {
    pub fn shadow_geometry(&self, shadow: BoxShadow) -> ShadowGeometry {
        let element_radius = self
            .commands
            .first()
            .map_or(0.0, |command| command.quad().1.resolved(self.bounds).radius);
        let spread = if shadow.inset {
            -shadow.spread_radius
        } else {
            shadow.spread_radius
        };
        let bounds = Rect {
            x: self.bounds.x + shadow.offset[0] - spread,
            y: self.bounds.y + shadow.offset[1] - spread,
            width: (self.bounds.width + spread * 2.0).max(0.0),
            height: (self.bounds.height + spread * 2.0).max(0.0),
        };
        let margin = 3.0 * shadow.blur_radius;
        ShadowGeometry {
            bounds,
            element_bounds: self.bounds,
            element_radius,
            radius: if shadow.inset {
                (element_radius - shadow.spread_radius).max(0.0)
            } else {
                element_radius
            },
            ink: if shadow.inset {
                self.bounds
            } else {
                Rect {
                    x: bounds.x - margin,
                    y: bounds.y - margin,
                    width: bounds.width + margin * 2.0,
                    height: bounds.height + margin * 2.0,
                }
            },
        }
    }
}

impl DisplayCommand {
    pub fn quad(&self) -> (u32, QuadDecoration) {
        match self {
            Self::Fill(color) => (*color, QuadDecoration::default()),
            Self::DecoratedFill { color, decoration } => (*color, *decoration),
        }
    }
}

impl PaintItem {
    /// Resolve current material against a reusable geometry command. Consumers
    /// must use this rather than the command's recorded seed color.
    pub fn resolved_quad(&self, command: &DisplayCommand) -> (u32, QuadDecoration) {
        (self.fill_color, command.quad().1)
    }
    pub(super) fn visible(&self, command: &DisplayCommand) -> bool {
        let (color, decoration) = self.resolved_quad(command);
        color & 255 != 0 || decoration.border_width > 0.0 && decoration.border_color & 255 != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shadow_draft_construction_publishes_atomically_and_invalid_values_do_not_leak_nodes() {
        let mut tree = UiTree::new(Style::default(), 0).unwrap();
        let root = tree.root();
        let shadow = BoxShadow {
            color: 0x112233ff,
            blur_radius: 2.0,
            ..Default::default()
        };
        let inserted = tree
            .edit(BackendCapabilities::default(), |edit| {
                let node = edit.append(
                    root,
                    Style {
                        size: Size {
                            width: length(20.0),
                            height: length(10.0),
                        },
                        ..Default::default()
                    },
                    0,
                )?;
                edit.properties().shadows(node, vec![shadow]);
                Ok(node)
            })
            .unwrap()
            .value;
        let before = tree.frame([80.0, 40.0]).unwrap();
        assert_eq!(before.items[1].node, inserted);
        assert_eq!(before.items[1].shadows.as_ref(), [shadow]);
        let failed = tree.edit(BackendCapabilities::default(), |edit| {
            let node = edit.append(root, Style::default(), 0)?;
            edit.properties().shadows(
                node,
                vec![BoxShadow {
                    blur_radius: -1.0,
                    ..shadow
                }],
            );
            Ok(node)
        });
        assert!(matches!(failed, Err(Error::InvalidPrimitive)));
        let after = tree.frame([80.0, 40.0]).unwrap();
        assert_eq!(after.revision, before.revision);
        assert!(Arc::ptr_eq(&after.items, &before.items));
    }

    #[test]
    fn shadows_are_atomic_paint_inputs_with_overflow_damage_and_immutable_packets() {
        let mut tree = UiTree::new(
            Style {
                size: Size {
                    width: length(100.0),
                    height: length(100.0),
                },
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let child = tree
            .append(
                tree.root(),
                Style {
                    position: taffy::Position::Absolute,
                    inset: taffy::Rect {
                        left: length(10.0),
                        top: length(10.0),
                        right: taffy::LengthPercentageAuto::auto(),
                        bottom: taffy::LengthPercentageAuto::auto(),
                    },
                    size: Size {
                        width: length(20.0),
                        height: length(10.0),
                    },
                    ..Default::default()
                },
                0,
            )
            .unwrap();
        let before = tree.frame([100.0; 2]).unwrap();
        let value = BoxShadow {
            color: 0x000000ff,
            offset: [-4.0, 5.0],
            blur_radius: 2.0,
            spread_radius: 1.0,
            inset: false,
        };
        let mut tx = Transaction::default();
        tx.shadows(child, vec![value]);
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default())
                .unwrap()
                .routes,
            vec![(child, UpdatePath::RepaintChunks)]
        );
        let after = tree.frame([100.0; 2]).unwrap();
        assert_eq!((after.layout_runs, after.recorded_chunks), (0, 1));
        assert_eq!(
            after.items[1].shadow_geometry(value).ink,
            Rect {
                x: -1.0,
                y: 8.0,
                width: 34.0,
                height: 24.0
            }
        );
        let expected = Rect {
            x: 0.0,
            y: 8.0,
            width: 33.0,
            height: 24.0,
        };
        assert_eq!(after.items[1].ink_bounds(), Some(expected));
        assert_eq!(after.damage.bounds, Some(expected));
        assert_ne!(after.hit_test(6.0, 20.0), Some(child));
        assert!(before.items[1].shadows.is_empty());
        let mut tx = Transaction::default();
        tx.color(child, 0xff0000ff);
        tx.shadows(
            child,
            vec![BoxShadow {
                blur_radius: f32::NAN,
                ..value
            }],
        );
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidPrimitive
        );
        assert_eq!(tree.frame([100.0; 2]).unwrap().revision, after.revision);
        let mut tx = Transaction::default();
        tx.shadows(child, vec![]);
        tx.shadows(child, vec![value]);
        assert!(
            !tree
                .commit(tx, BackendCapabilities::default())
                .unwrap()
                .request_frame
        );
        let mut tx = Transaction::default();
        tx.shadows(child, vec![]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let removed = tree.frame([100.0; 2]).unwrap();
        assert_eq!(removed.damage.bounds, Some(expected));
        assert!(removed.items[1].ink_bounds().is_none());
        assert_eq!(after.items[1].shadows.as_ref(), [value]);
    }
    #[test]
    fn shadow_geometry_handles_inset_and_negative_spread_and_validates_draft_sinks() {
        let mut tree = UiTree::new(
            Style {
                size: Size {
                    width: length(40.0),
                    height: length(20.0),
                },
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let node = tree.root();
        let mut tx = Transaction::default();
        tx.decoration(
            node,
            QuadDecoration {
                radius: 8.0,
                ..Default::default()
            },
        );
        let shadow = BoxShadow {
            color: 0x000000ff,
            offset: [2.0, -1.0],
            spread_radius: 3.0,
            inset: true,
            ..Default::default()
        };
        tx.shadows(node, vec![shadow]);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([40.0, 20.0]).unwrap();
        let g = frame.items[0].shadow_geometry(shadow);
        assert_eq!(
            g.bounds,
            Rect {
                x: 5.0,
                y: 2.0,
                width: 34.0,
                height: 14.0
            }
        );
        assert_eq!((g.radius, g.element_radius), (5.0, 8.0));
        assert_eq!(g.ink, frame.items[0].bounds);
        let g = frame.items[0].shadow_geometry(BoxShadow {
            inset: false,
            spread_radius: -3.0,
            ..shadow
        });
        assert_eq!(
            g.bounds,
            Rect {
                x: 5.0,
                y: 2.0,
                width: 34.0,
                height: 14.0
            }
        );
        assert_eq!(g.radius, 8.0);
        assert!(
            BoxShadow {
                blur_radius: -1.0,
                ..shadow
            }
            .validate()
            .is_err()
        );
        assert!(
            BoxShadow {
                blur_radius: f32::MAX / 4.0,
                ..shadow
            }
            .validate()
            .is_err()
        );
        let mut ui = reactive::ReactiveUi::new(tree, BackendCapabilities::default()).unwrap();
        let source = ui.signal(Arc::<[BoxShadow]>::from([shadow]));
        ui.bind(source, reactive::Property::shadows(node)).unwrap();
        let result = ui
            .transaction(|tx| {
                tx.set(source, Arc::from([]))?;
                tx.set(source, Arc::from([shadow]))?;
                Ok(())
            })
            .unwrap();
        assert!(!result.commit.request_frame);
        assert_eq!(result.sink_writes, 0);
        let next: Arc<[BoxShadow]> = Arc::from([BoxShadow {
            color: 0xff8844ff,
            ..shadow
        }]);
        ui.transaction(|tx| {
            tx.set(source, next.clone())?;
            Ok(())
        })
        .unwrap();
        let changed = ui.frame([40.0, 20.0]).unwrap();
        assert!(Arc::ptr_eq(&changed.items[0].shadows, &next));
    }

    #[test]
    fn decoration_is_paint_only_atomic_and_updates_ink_with_transparent_fill() {
        let mut tree = UiTree::new(
            Style {
                size: Size {
                    width: length(80.0),
                    height: length(40.0),
                },
                ..Default::default()
            },
            0,
        )
        .unwrap();
        let before = tree.frame([80.0, 40.0]).unwrap();
        assert!(before.items[0].ink_bounds().is_none());
        let decoration = QuadDecoration {
            radius: 8.0,
            border_width: 2.0,
            border_color: 0xff8844ff,
        };
        let mut tx = Transaction::default();
        tx.decoration(tree.root(), decoration);
        let result = tree.commit(tx, BackendCapabilities::default()).unwrap();
        assert_eq!(
            result.routes,
            vec![(tree.root(), UpdatePath::RepaintChunks)]
        );
        assert_eq!(
            result.decisions[0].reason,
            RouteReason::PrimitivePaintChanged
        );
        let after = tree.frame([80.0, 40.0]).unwrap();
        assert_eq!((after.layout_runs, after.recorded_chunks), (0, 1));
        assert!(after.damage.bounds.is_some());
        assert!(after.items[0].ink_bounds().is_some());
        assert_eq!(after.items[0].commands[0].quad(), (0, decoration));
        let mut tx = Transaction::default();
        tx.color(tree.root(), 0xffffffff);
        tx.decoration(
            tree.root(),
            QuadDecoration {
                radius: f32::NAN,
                ..decoration
            },
        );
        assert_eq!(
            tree.commit(tx, BackendCapabilities::default()).unwrap_err(),
            Error::InvalidPrimitive
        );
        assert_eq!(tree.frame([80.0, 40.0]).unwrap().revision, after.revision);
        let mut tx = Transaction::default();
        tx.decoration(tree.root(), QuadDecoration::default());
        tx.decoration(tree.root(), decoration);
        assert!(
            !tree
                .commit(tx, BackendCapabilities::default())
                .unwrap()
                .request_frame
        );
        let (color, resolved) = after.items[0].commands[0].quad();
        assert_eq!(color, 0);
        assert_eq!(
            QuadDecoration {
                radius: 100.0,
                border_width: 100.0,
                ..resolved
            }
            .resolved(after.items[0].bounds)
            .radius,
            20.0
        );
    }

    #[test]
    fn instance_material_changes_reuse_commands_and_damage_old_and_new_alpha() {
        let mut tree = UiTree::new(
            Style {
                size: Size {
                    width: length(80.0),
                    height: length(40.0),
                },
                ..Default::default()
            },
            0xff8844ff,
        )
        .unwrap();
        let before = tree.frame([80.0, 40.0]).unwrap();
        let mut tx = Transaction::default();
        tx.color(tree.root(), 0);
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )
        .unwrap();
        let hidden = tree.frame([80.0, 40.0]).unwrap();
        assert_eq!(hidden.recorded_chunks, 0);
        assert!(Arc::ptr_eq(
            &before.items[0].commands,
            &hidden.items[0].commands
        ));
        assert!(hidden.items[0].ink_bounds().is_none());
        assert!(hidden.damage.bounds.is_some());
        assert_eq!(before.items[0].fill_color, 0xff8844ff);
        let mut tx = Transaction::default();
        tx.color(tree.root(), 0x44bb88ff);
        tree.commit(
            tx,
            BackendCapabilities {
                solid_color_patch: true,
            },
        )
        .unwrap();
        let restored = tree.frame([80.0, 40.0]).unwrap();
        assert_eq!(restored.recorded_chunks, 0);
        assert!(restored.items[0].ink_bounds().is_some());
        assert_eq!(restored.updates[0].fill_color, 0x44bb88ff);
        assert!(restored.damage.bounds.is_some());
        // Backend fallback re-records a canonical seed without changing how a
        // published material is resolved by either CPU or GPU consumers.
        let mut tx = Transaction::default();
        tx.color(tree.root(), 0x112233ff);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let fallback = tree.frame([80.0, 40.0]).unwrap();
        assert_eq!(fallback.recorded_chunks, 1);
        assert_eq!(
            fallback.items[0].commands[0],
            DisplayCommand::Fill(0x112233ff)
        );
    }
}
