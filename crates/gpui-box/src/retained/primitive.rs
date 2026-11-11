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
