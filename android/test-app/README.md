# Director on-device test (Android)

A NativeActivity APK that runs Director on a phone or emulator with no FFmpeg: MediaCodec decodes
and encodes video, MediaMuxer writes the MP4, and audio goes through MediaCodec AAC or Symphonia.
It runs five steps and writes `log.txt` in the app's files folder
(`/storage/emulated/0/Android/data/com.nofacetube.directortest/files`):

| Step | What it checks |
| --- | --- |
| decode source | hardware decode of a 1080p24 H.264 file onto a 30 fps grid, AAC decode |
| colour tags | untagged, BT.709-tagged and full-range BT.601 files decode to the same colours as FFmpeg |
| encode speed | 150 frames of 1080p H.264 plus an AAC tone |
| html scene | compiles an HTML scene (two videos, a grade, two audio tracks) on the device and exports it |
| reel | exports `scenes/reel.json` (28.6 s, 1080p motion graphics) |

Results on a Galaxy S25 Ultra (2026-11-20, screen on, app in front): decode 180 frames in 1.5 s,
encode 161 fps, HTML scene 6 s in 2.3 s, reel in 23 s. The Android 36 x86_64 emulator passes all
five with the software `c2.android` codecs.

## Build and run

Needs the Android NDK 29, build-tools 36, platform android-35 and the Rust targets
`aarch64-linux-android` / `x86_64-linux-android`. Set `ANDROID_SDK` and `DIRECTOR_ANDROID_TARGET`
if yours differ from the defaults in the scripts. Run from Git Bash:

```
bash build-so.sh                                          # phone (arm64)
bash pkg/build.sh                                         # -> pkg/director-test-arm64-v8a.apk
bash build-so.sh x86_64-linux-android && bash pkg/build.sh x86_64 x86_64-linux-android   # emulator
```

`pkg/build.sh` signs with a throwaway debug keystore it creates on first use (`pkg/test.keystore`,
password in `pkg/ks.pass`, both git-ignored). It is for testing only.

The reel needs its images and music next to it. Push them once, then start the app:

```
MSYS_NO_PATHCONV=1 adb push ../../examples/html/reel/assets /storage/emulated/0/Android/data/com.nofacetube.directortest/files/
adb install -r pkg/director-test-arm64-v8a.apk
adb shell am start -n com.nofacetube.directortest/android.app.NativeActivity
MSYS_NO_PATHCONV=1 adb shell cat /storage/emulated/0/Android/data/com.nofacetube.directortest/files/log.txt
```

Create an empty file named `quick` in that folder to run only the two decode steps. Keep the
screen on while it runs: a sleeping phone made the reel 10x slower in one run.

`scenes/reel.json` is `examples/html/reel/reel.html` compiled on the desktop at 1920x1080 with
`compile-reel` (`cargo run --release --manifest-path compile-reel/Cargo.toml -- <html> <out.json> 1920 1080`).
