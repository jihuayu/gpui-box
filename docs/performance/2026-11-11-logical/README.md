# Logical scope validation evidence

This freezes the [logical scope stage](../../RETAINED_LOGICAL_SCOPES.md).
`validation.json` contains all 50 successful hardware pixel scenarios.
`metadata.json` records the source hashes and build, and `adapter.json` identifies
the GTX 1650 Ti/Vulkan device. All recorded source hashes were checked against
the final measured source before copying these files.

The six `logical-*.png` files are the new logical-scope scenarios. Existing
scenario images are retained in the earlier evidence directories. The complete
suite also reran damage and layer-cache timing controls; those results remain
in the local run directory and do not establish a keyed-reconciliation speedup.
This stage validates identity, lifecycle and pixels, not native presentation.
