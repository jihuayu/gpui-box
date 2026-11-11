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

Optional native declarations use the versioned `services` object in
`gpui-box.json`. Existing configurations may omit it.

```json
{
  "schema_version": 1,
  "permissions": [{ "kind": "camera", "purpose": "Scan a document after the user selects Scan." }],
  "url_schemes": ["my-application"]
}
```

Place this object under `services`, then run `gpui-box generate`. It generates
only the requested Android permissions and custom scheme intent filters, plus
the managed `gpui-box-services.json` asset. Supported permission kinds are
`notifications`, `camera`, `microphone`, `location_when_in_use` and `contacts`.
The purpose is application metadata for contextual permission UI. The Android
host installs `permission_service()` in GPUI SystemServices before app launch.
Use `gpui_box::android::permission_context()` for the current host attachment,
then call the shared service through the permission crate. Hold/await the local
request task. The gallery demonstrates explicit requests and settings recovery.
`doctor` reports configuration and per-operation qualification separately.
Android's system dialog uses its OS wording. Applications display their declared
purpose context before requesting. Photos, biometrics and special-access settings
need their operation-specific providers. Authorization is freshly queried.

Android applications can subscribe to custom URL delivery through
`gpui_box::android::on_activation`. Return acceptance after handing the stable
event ID to the application's router. The host retries pending delivery while
foregrounded and preserves pending IDs across Activity recreation. Its bounded
queue and process-local duplicate window provide transient delivery. Applications
requiring durable routing should use `gpui-activation`'s inbox on a worker.
Add an Android target dependency on the facade when using these host APIs.

Transient UI restoration uses `on_saved_state`, `take_saved_state` and the
application's versioned snapshot schema. Register a save callback after opening
the window, then apply consumed bytes through the launch callback's `App`.
Android retains the live Rust model during Activity recreation. New processes
consume the saved Bundle once. Snapshots are limited to 128 KiB. Finish old IME
composition during restoration and store business data through private storage.

CLI build/dev/install/package regenerates managed files before invoking tools.
Run `gpui-box generate` before direct mise tasks after changing configuration.
If a generator is killed, verify no other generator is running before removing
`.gpui-box/generate.lock`, then rerun generation to recover its pending journal.
