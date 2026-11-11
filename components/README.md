# Components

Distribution-owned reusable UI crates belong here. The first planned package is
`gpui-box-components`, containing shared behavior, themes, controls, and adaptive
presentation for desktop and mobile. Add real packages to the root workspace
when their implementation begins.

Depend on the coordinated `gpui-box` facade. Keep native host integration in
`platforms` and upstream engine/backend work in `vendor/gpui`.
