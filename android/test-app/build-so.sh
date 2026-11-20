export NDK_HOME="${NDK_HOME:-${ANDROID_SDK:-/c/Users/Steve/AppData/Local/Android/Sdk}/ndk/29.0.14206865}"; export ANDROID_NDK="$NDK_HOME"; TC="$NDK_HOME/toolchains/llvm/prebuilt/windows-x86_64/bin"
export CC_aarch64_linux_android="$TC/aarch64-linux-android28-clang.cmd" CXX_aarch64_linux_android="$TC/aarch64-linux-android28-clang++.cmd" AR_aarch64_linux_android="$TC/llvm-ar.exe" CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TC/aarch64-linux-android28-clang.cmd"
export CC_x86_64_linux_android="$TC/x86_64-linux-android28-clang.cmd" CXX_x86_64_linux_android="$TC/x86_64-linux-android28-clang++.cmd" AR_x86_64_linux_android="$TC/llvm-ar.exe" CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$TC/x86_64-linux-android28-clang.cmd"
cd "$(dirname "$0")"
cargo build --release --target ${1:-aarch64-linux-android} --target-dir "${DIRECTOR_ANDROID_TARGET:-G:/director-android-target}" 2>&1 | grep -E "^error|^warning: unused|-->.*(dirapk|android_media)|Finished|could not" | head -30
