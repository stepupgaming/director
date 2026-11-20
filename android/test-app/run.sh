# run.sh <serial> [abi] : install, launch, wait for DONE (max 10 min), print log
A=${ANDROID_SDK:-/c/Users/Steve/AppData/Local/Android/Sdk}/platform-tools/adb.exe; D=$1; ABI=${2:-arm64-v8a}
cd "$(dirname "$0")"
$A -s $D shell am force-stop com.nofacetube.directortest
$A -s $D install -r pkg/director-test-$ABI.apk | tail -1
$A -s $D logcat -c
$A -s $D shell am start -n com.nofacetube.directortest/android.app.NativeActivity >/dev/null
for i in $(seq 1 120); do sleep 5; if $A -s $D logcat -d -s dirapk:I | grep -q "DIRAPK DONE"; then break; fi; done
