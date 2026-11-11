# Supplemental dependency license texts

Some published Cargo archives omit their repository-level license files.
`sources.json` records exact package versions, publishing Git revisions, source
URLs and SHA-256 values for the copied original texts in this directory. The
packager verifies these hashes before copying them into artifact inventories.

These files preserve their original upstream terms and notices. The broader
resolved graph includes build and non-target packages. `license-review.json`
lists packages whose texts are still missing and keeps publication review
explicit. SPDX metadata alone does not complete redistribution review.

Current unresolved Cargo texts include pathfinder_geometry 0.5.1, seahash 4.1.0
and simd_helpers 0.1.0. The first package's archived revision was unavailable at
the recorded repository during collection. The second uses a separate GitLab
repository. The third revision has no repository-root license text. Resolve
these provenance gaps and review Maven/native/font obligations before publishing
an application release.
