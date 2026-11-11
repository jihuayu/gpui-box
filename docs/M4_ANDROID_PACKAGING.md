# M4 application packaging

## Android release workflow

Generate an application through the existing source-checkout CLI and provision
its pinned mise environment. Android minimum/native/target API is 35 and
compileSdk is 36. Release native output uses Cargo's release profile, separate
from debug JNI output. The application retains its configured development
channel until M5 supplies distribution-channel policy.

```sh
gpui-box generate
gpui-box doctor
gpui-box package android
gpui-box package android --release
```

The release command requires all four environment variables below. Use a secret
manager or a process-local environment in CI. `doctor` prints their presence
only. Passwords are passed to Gradle through environment providers and to
keytool through its environment-password options.

| Variable | Meaning |
| --- | --- |
| `GPUI_BOX_ANDROID_KEYSTORE` | Absolute path to an existing private-key keystore |
| `GPUI_BOX_ANDROID_STORE_PASSWORD` | Keystore password |
| `GPUI_BOX_ANDROID_KEY_ALIAS` | Private-key alias |
| `GPUI_BOX_ANDROID_KEY_PASSWORD` | Key password |
| `GPUI_BOX_BUNDLETOOL` | Absolute path to the verified bundletool JAR below |

The keystore must contain a private key. The CLI rejects the standard debug
alias/certificate, missing inputs and signer mismatches between the configured
identity, APK and AAB. Actual publishing credentials remain application-owned.
Generated gitignore rules exclude dist output, environment files and keystores.

### Verified build tools

CLI Android workflows verify the managed Wrapper JAR and the complete pinned
properties file, then invoke `GradleWrapperMain` through mise's Java directly.
This supports Windows paths containing spaces and avoids shell-specific scripts.
Fresh starter mise Android tasks also invoke the Wrapper. Existing app-owned
mise files remain subject to their application's maintenance policy.

| Artifact | SHA-256 |
| --- | --- |
| Gradle 8.13 binary distribution | `20f1b1176237254a6fc204d8434196fa11a4cfb387567519c61556e8710aed78` |
| Gradle 8.13 Wrapper JAR | `81a82aaea5abcc8ff68b3dfcb58b3c3c429378efd98e7433460610fecd7ae45f` |
| bundletool 1.18.3 all JAR | `a099cfa1543f55593bc2ed16a70a7c67fe54b1747bb7301f37fdfd6d91028e29` |

Obtain bundletool from its official release, verify the digest and set
`GPUI_BOX_BUNDLETOOL` to the local file. The CLI performs the digest check again.
The JDK, cargo-ndk, SDK build tools and NDK retain their existing mise/Gradle pins.
[bundletool release](https://github.com/google/bundletool/releases/tag/1.18.3)
[Gradle checksums](https://gradle.org/release-checksums/)

### Inspection and publication

The CLI builds `assembleRelease`, `bundleRelease` and the resolved Maven
inventory. It verifies APK signatures, application ID/version/API, both required
ABIs, every packaged ELF library's ABI and 16 KB PT_LOAD alignment, and APK ZIP
alignment with `zipalign -c -P 16 4`. AAB verification includes bundletool
validation, PAGE_ALIGNMENT_16K configuration, JAR signature/certificate, parsed
manifest and byte-identical native library inventory compared with the APK.

Manifest inspection rejects debuggable/testOnly flags and unexpected exported
components, checks the exported launcher, disabled backup, uncompressed native
libraries, requested permissions and custom schemes. The packaged service asset
must equal the typed application configuration. Custom exported providers need a
future explicit policy extension and runtime qualification in their owning issues.

Successful output is staged and renamed to
`dist/android/<version>-<version_code>`. Existing releases are preserved and a
new versionCode is required for subsequent output. A create-new release lock
serializes packaging commands. After interruption, confirm ownership before
removing a stale `.gpui-box/release.lock`. Failed staging output remains available
for diagnosis. Other build processes must coordinate access to the same project.

```text
app.apk / app.aab             Signed application artifacts
release.json                 Identity, tools, signer, native hashes and review gates
AndroidManifest.xml          Parsed bundle manifest used by inspection
gpui-box-services.json       Inspected typed service declarations
dependency-inventory.json    Cargo resolved graph and license provenance
android-dependencies.json    Selected Gradle release runtime coordinates
maven-poms/                  Matching Maven POM license declarations
licenses/                    Available original and supplemental texts
license-review.json          Missing texts and publication review gate
LICENSE / GPUI-* / FONT-*    Distribution, upstream and bundled font provenance
THIRD-PARTY-NOTICES.txt       Inventory scope and review guidance
SHA256SUMS                   Every output file except this checksum index
```

Cargo inventory deliberately includes build and non-target packages. Original
license/notice texts are collected from package roots and declared license files.
Local distribution/application packages use their owning workspace's Apache
license. Supplemental texts use commit-pinned provenance under `tooling/licenses`.
Missing texts remain listed explicitly. Current unresolved packages include
pathfinder_geometry, seahash and simd_helpers. Maven POMs supply declared terms.
Publication review must resolve applicable missing texts and complete native,
font and Maven redistribution obligations.

## Windows portable developer bundle

On a Windows host with the desktop target enabled, run the following command.

```sh
gpui-box package desktop --portable
```

The command uses the existing development profile to build `box-desktop`, copies
the runnable executable and application asset roots, collects host-filtered Cargo
provenance/licenses, and publishes a ZIP, extracted bundle and outer digest under
`dist/desktop/<version>-<version_code>-<arch>-windows-dev`. `bundle.json` records
the debug profile and runtime prerequisites. Asset paths overlapping build output
or bundle metadata and asset symlinks are rejected. Nested files receive checksums.

The current executable imports Windows GPU/system libraries and VCRUNTIME140.
Use a supported Windows version, compatible graphics drivers and the Microsoft
Visual C++ runtime. Full SDK relocation remains #62. Desktop installer, signing
and update acceptance remains M7/#8.

## Local qualification

Validation used Rust 1.98.1, JDK 17.0.21+9, Gradle 8.13, cargo-ndk 4.1.2,
NDK 30.0.16248370, SDK build-tools 36.0.0 and bundletool 1.18.3 on Windows x86_64.
The GPUI baseline remains `f8cfc7ed9e57503def9479138894f3139f5400cc`.

The generated `target/Release QA App` uses a space-containing source/application
path. A throwaway local signing identity built version 0.1.1/code 4 of
`dev.gpui.box.releaseqa` with camera/notification declarations and a custom scheme.
Release APK installation and cold start succeeded on Android 15/API 35/x86_64.
bundletool `build-apks` generated the signed split set, `install-apks` installed
it and a second cold start succeeded. The final code 4 build repeats the complete
inspection and split generation with the updated manifest/native consistency checks. Earlier code 3 UI automation observed Count 0 to Count 1
and Activation events 0 to 1 after the custom-scheme Intent. APK/AAB and both ABI
splits share the inspected signer. Split ZIP alignment passes. ARM64 ELF LOAD
alignment is 0x4000. The emulator uses 4096-byte pages, so 16 KB runtime acceptance
remains pending.

The final code 4 APK installs and reports a successful cold start. Its split set
also installs and exposes a live process and the expected Count 0/Activation 0
semantic tree. Two `am start -W` waits after split installation timed out at about
10.6 seconds on the loaded emulator. Preserve this launch-wait qualification gap
alongside the earlier successful code 3 cold-start evidence. AAB jarsigner reports
the expected local self-signed/short-lived QA certificate warnings and a
JarFile/JarInputStream ordering warning from the AGP-produced bundle. Bundletool
validation, certificate matching and native-content checks pass. These results
retain the store/channel and publication review gates.

The earlier Windows developer ZIP was extracted into a path containing spaces and
Chinese characters. Every recursive checksum passed. Import inspection recorded
VCRUNTIME140 and Windows system/GPU libraries. The process launches, but the
desktop was locked during window capture, leaving this iteration's interactive
portable-app acceptance pending. The final ZIP contains 559 files and passes
558 recursive checksums plus the outer ZIP digest. The final Android output
passes 673 recursive checksums.

Integrated `mise run check` passes 143 host tests and six compile-fail doctests,
plus two nested child-process reports. CLI tests pass 17/17. Components pass
five model and two real GPUI-context tests. Strict CLI/shared Clippy and formatting
pass. Lint reports zero errors and eight existing warnings.

The complete API 35 instrumentation suite passes 14/14 after resetting the test
application. Earlier retries recorded input-dispatch timeouts and retained
permission/page state. The notification Settings test now returns with
CLEAR_TOP/SINGLE_TOP to its original Activity, awaits real foreground focus and
clears intervening Settings UI so the next test can launch. No manual host-state
refresh is used to satisfy reconciliation. Preserve failed logs when diagnosing
emulator contention, and reset application state before rerunning first-prompt
cases. Doctor still identifies this host's missing SDK license record.

Physical devices, Play test-track delivery, actual publishing credentials,
clean-machine provisioning, 16 KB runtime, non-Windows host qualification and
complete publication/license review retain separate acceptance gates. M4 remains
open until its full criteria are evidenced.
