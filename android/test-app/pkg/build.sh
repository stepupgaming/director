set -e
SDK=${ANDROID_SDK:-/c/Users/Steve/AppData/Local/Android/Sdk}; BT=$SDK/build-tools/36.0.0; JBR="/c/Program Files/Android/Android Studio/jbr/bin"
TC=$SDK/ndk/29.0.14206865/toolchains/llvm/prebuilt/windows-x86_64/bin
ABI=${1:-arm64-v8a}; TRIPLE=${2:-aarch64-linux-android}
cd "$(dirname "$0")"
rm -rf stage unsigned.apk aligned.apk director-test-$ABI.apk; mkdir -p stage/lib/$ABI
$TC/llvm-strip.exe -o stage/lib/$ABI/libmain.so ${DIRECTOR_ANDROID_TARGET:-G:/director-android-target}/$TRIPLE/release/libmain.so
$BT/aapt.exe package -f -M AndroidManifest.xml -I $SDK/platforms/android-35/android.jar -F unsigned.apk
(cd stage && $BT/aapt.exe add ../unsigned.apk lib/$ABI/libmain.so >/dev/null)
$BT/zipalign.exe -f -p 4 unsigned.apk aligned.apk
if [ ! -f test.keystore ]; then
  head -c 24 /dev/urandom | base64 | tr -dc A-Za-z0-9 > ks.pass
  "$JBR/keytool.exe" -genkeypair -keystore test.keystore -storepass "$(cat ks.pass)" -alias test -keyalg RSA -keysize 2048 -validity 3650 -dname "CN=Director test" >/dev/null 2>&1
fi
PATH="$JBR:$PATH" $BT/apksigner.bat sign --ks test.keystore --ks-pass file:ks.pass --out director-test-$ABI.apk aligned.apk
ls -la director-test-$ABI.apk
