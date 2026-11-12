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
    timeout -k 5s 15s "${adb[@]}" logcat -d -v threadtime > "$report/logcat.txt" 2>&1 || true
    timeout -k 5s 15s "${adb[@]}" shell dumpsys SurfaceFlinger > "$report/surfaceflinger.txt" 2>&1 || true
    timeout -k 5s 15s "${adb[@]}" exec-out screencap -p > "$report/final.png" 2>/dev/null || true
}
trap collect EXIT
{
    git rev-parse HEAD
    python3 --version
    cat "$ANDROID_HOME/emulator/source.properties"
    sha256sum "$ANDROID_HOME/emulator/emulator" \
        "$ANDROID_HOME/emulator/qemu/linux-x86_64/qemu-system-x86_64"
    timeout -k 5s 30s "${adb[@]}" get-state
    timeout -k 5s 30s "${adb[@]}" shell getprop
    timeout -k 5s 30s "${adb[@]}" shell getconf PAGE_SIZE
    timeout -k 5s 30s "${adb[@]}" shell wm size
    timeout -k 5s 30s "${adb[@]}" shell wm density
    cat "$ANDROID_HOME/system-images/android-35/google_apis/x86_64/source.properties"
} > "$report/environment.txt" 2>&1
[[ $(timeout -k 5s 30s "${adb[@]}" shell getprop ro.build.version.sdk | tr -d '\r') == 35 ]]
[[ $(timeout -k 5s 30s "${adb[@]}" shell getprop ro.product.cpu.abi | tr -d '\r') == x86_64 ]]
sha256sum platforms/android/app/build/outputs/apk/debug/app-debug.apk \
    platforms/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk \
    > "$report/apk-sha256.txt"
timeout -k 5s 120s "${adb[@]}" install -r platforms/android/app/build/outputs/apk/debug/app-debug.apk > "$report/install.txt"
timeout -k 5s 120s "${adb[@]}" install -r platforms/android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk >> "$report/install.txt"
timeout -k 5s 30s "${adb[@]}" shell pm clear dev.gpui.box.example | tee "$report/reset.txt"
tr -d '\r' < "$report/reset.txt" | grep -Fx 'Success'
timeout -k 5s 30s "${adb[@]}" logcat -c
timeout -k 5s 30s "${adb[@]}" shell input keyevent 224
timeout -k 5s 30s "${adb[@]}" shell wm dismiss-keyguard
timeout -k 5s 12m "${adb[@]}" shell am instrument -w -r \
    dev.gpui.box.example.test/androidx.test.runner.AndroidJUnitRunner \
    2>&1 | tee "$report/instrumentation.txt"
python3 tooling/android/verify_instrumentation.py "$report/instrumentation.txt" "$report/receipt.json"
