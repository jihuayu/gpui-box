#!/usr/bin/env bash
set -Eeuo pipefail

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repository"
report=${GPUI_BOX_EMULATOR_REPORT_DIR:-$repository/target/ci/emulator}
serial=${GPUI_BOX_EMULATOR_SERIAL:-emulator-5554}
[[ "$serial" =~ ^emulator-[0-9]+$ ]] || { echo 'Select an emulator serial' >&2; exit 1; }
mkdir -p -- "$(dirname -- "$report")"
mkdir -- "$report"
adb=("${ANDROID_HOME:?Android SDK is required}/platform-tools/adb" -s "$serial")

collect() {
    "${adb[@]}" logcat -d -v threadtime > "$report/logcat.txt" 2>&1 || true
    "${adb[@]}" shell dumpsys SurfaceFlinger > "$report/surfaceflinger.txt" 2>&1 || true
    "${adb[@]}" exec-out screencap -p > "$report/final.png" 2>/dev/null || true
}
trap collect EXIT
{
    git rev-parse HEAD
    python3 --version
    "$ANDROID_HOME/emulator/emulator" -version
    "${adb[@]}" get-state
    "${adb[@]}" shell getprop
    "${adb[@]}" shell getconf PAGE_SIZE
    "${adb[@]}" shell wm size
    "${adb[@]}" shell wm density
    cat "$ANDROID_HOME/system-images/android-35/google_apis/x86_64/source.properties"
} > "$report/environment.txt" 2>&1
[[ $("${adb[@]}" shell getprop ro.build.version.sdk | tr -d '\r') == 35 ]]
[[ $("${adb[@]}" shell getprop ro.product.cpu.abi | tr -d '\r') == x86_64 ]]
sha256sum platforms/android/app/build/outputs/apk/debug/app-debug.apk \
    platforms/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk \
    > "$report/apk-sha256.txt"
"${adb[@]}" install -r platforms/android/app/build/outputs/apk/debug/app-debug.apk > "$report/install.txt"
"${adb[@]}" install -r platforms/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk >> "$report/install.txt"
"${adb[@]}" shell pm clear dev.gpui.box.example | tee "$report/reset.txt"
tr -d '\r' < "$report/reset.txt" | grep -Fx 'Success'
"${adb[@]}" logcat -c
"${adb[@]}" shell input keyevent 224
"${adb[@]}" shell wm dismiss-keyguard
timeout 12m "${adb[@]}" shell am instrument -w -r \
    dev.gpui.box.example.test/androidx.test.runner.AndroidJUnitRunner \
    2>&1 | tee "$report/instrumentation.txt"
python3 tooling/android/verify_instrumentation.py "$report/instrumentation.txt" "$report/receipt.json"
