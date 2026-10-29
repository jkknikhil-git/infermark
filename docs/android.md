# Android

So, the short version: InferMark runs on Android as a normal app, with llama.cpp compiled into
it for the phone's CPU. There's no Play Store build yet. You sideload a debug APK, either one you
build yourself or the one CI keeps for a week after every push to `main`.

The phone we're testing on is a motorola edge 60 pro (Dimensity 8350, 8 GB RAM, Android 17).
That's one phone, so treat anything here as "works on that one" until more people try theirs.

## What we measured

So, first of all, it works: the Standard profile ran start to finish on that phone, plugged in,
in about 6 minutes. With Qwen3 0.6B (Q4_K_M) on the CPU, a 493-token prompt and 128 tokens out:

| Test | Result |
|---|---|
| Memory bandwidth (copy / triad) | 24.7–27.1 / 27.7–27.9 GB/s |
| Generation, 4 threads (fast cores) | 21.8 tok/s |
| Generation, 8 threads (all cores) | 16.4 tok/s |
| Prompt processing, 4 / 8 threads | 140 / 142 tok/s |
| Time to first token | 3.6 s |
| Load time | 1.3–1.45 s |
| Endurance, 5 minutes (8 threads, shorter prompts) | peak 25.3, last quarter 20.5 tok/s (81% stability) |
| Battery temperature during the endurance run | 33.8 → 37.4 °C, thermal status "nominal" throughout |

Here's the thing about threads: generating on all eight cores was a quarter slower than on the
four fast ones, because the 2.2 GHz little cores hold the rest back. Reading the prompt didn't
care either way. So on phones InferMark now defaults to the fast cores, and a profile's
`run.threads` still overrides it. With that default the Quick profile went from 16.7 to
19.2 tok/s on the same phone.

Prompt length matters more than you might think on a phone, too. With a 128-token prompt the
same sweep measured 33.5 tok/s on 4 threads, against 21.8 with 493 tokens. So compare results
from the same profile, not across profiles. The endurance row ran before the thread change, so
take its absolute numbers loosely. The 19% drop over 5 minutes is the part worth keeping in mind.

## What's different on a phone

Here's the thing: a phone app can do far less than a desktop program, and InferMark has to work
within that.

- **The engine is built in.** Android won't run executables from app storage, so instead of
  `llama-server`, the app links llama.cpp (the `llama-inproc` feature) built for
  `armv8.2-a+dotprod` (see `.cargo/config.toml`). It runs on the CPU only for now. Vulkan and
  OpenCL on Adreno or Mali GPUs come later.
- **Sensors come from Android itself.** Apps can't read most of `/sys` and `/proc`, so CPU load
  and CPU temperature show up as "not available on this device". What you do get, through a small
  Kotlin plugin (`gen/android/.../DevicePlugin.kt`), is battery charge, drain in watts (current ×
  voltage while unplugged), battery temperature, Android's thermal status and battery saver.
- **Memory is tighter.** The gate only uses 55% of RAM on phones, against 85% on desktops,
  because Android closes a big app well before memory is actually full.
- **The screen stays on** during a run. Android dims, then throttles, a phone that thinks nobody
  is looking.
- **Storage is the app's own folder**, so models and results are removed when you uninstall.

## Install a build

1. On the phone, turn on Developer options and USB debugging.
2. Download the `infermark-android-debug-apk` artifact from a CI run, or build one (below).
3. With the phone plugged in:

   ```bash
   adb install -r app-arm64-debug.apk
   ```

The APK is about 170 MB because debug builds keep their symbols in. Android will warn that it's
from an unknown developer. That's expected for a debug build, and you decide whether to trust it.

## Build it yourself

You need JDK 17, the Android SDK with NDK 27, Node 22, Rust with the `aarch64-linux-android`
target, and libclang (bindgen uses it to read llama.cpp's headers). Point `JAVA_HOME`,
`ANDROID_HOME`, `NDK_HOME` and `LIBCLANG_PATH` at them, then:

```bash
cd apps/desktop
npm ci
npx tauri android build --apk --target aarch64 --debug
```

The APK lands in `src-tauri/gen/android/app/build/outputs/apk/arm64/debug/`. The first build
compiles llama.cpp for arm64, which is the slow part. After that, on the i5-11320H this was built
on, a rebuild took about 6 minutes for the Rust side and 10 for the first Gradle run, which also
downloads Gradle's own dependencies.

A couple of Windows notes, learned the hard way:

- **Use the Ninja generator** (`CMAKE_GENERATOR=Ninja`, with the SDK's CMake on `PATH`). The
  default Visual Studio generator can't cross-compile for Android.
- **Symlinks.** Tauri links the built library into `jniLibs` with a symlink, which Windows only
  allows with Developer Mode on. If you'd rather not turn that on, let the Tauri build fail at that
  step, then copy the library over and run Gradle directly:

  ```bash
  cp target/aarch64-linux-android/debug/libinfermark_desktop_lib.so apps/desktop/src-tauri/gen/android/app/src/main/jniLibs/arm64-v8a/
  cd apps/desktop/src-tauri/gen/android
  ./gradlew assembleArm64Debug -x rustBuildArm64Debug -PabiList=arm64-v8a -ParchList=arm64 -PtargetList=aarch64
  ```

## Practical advice

Run it plugged in for speed numbers and unplugged for the battery test, and close other apps
first, the same as you would on a laptop. And look, if your phone isn't the one above, a result
from it is genuinely useful, even if something comes out as "not available on this device".
