# gpui-box application

Edit the shared view in `app/src/lib.rs`. Desktop and Android use the same model.
Run `mise run dev:desktop` or `mise -E android run build:android`.
Set ANDROID_HOME to the Android SDK. Use the gpui-box source CLI for doctor,
generate, build, install, devices and logs. API 35 is required.

`gpui-box.json` owns project configuration. `gpui-box generate` updates managed
manifests after checking their recorded hashes. Keep Android customizations in
`android-custom.gradle.kts` and application source under `app/src`. Existing
application files are preserved. Commit the generated Cargo.lock after the first
build. Release signing and application updates follow M4/M5.

CLI build/dev/install/package regenerates managed files before invoking tools.
Run `gpui-box generate` before direct mise tasks after changing configuration.
If a generator is killed, verify no other generator is running before removing
`.gpui-box/generate.lock`, then rerun generation to recover its pending journal.
