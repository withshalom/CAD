# CAD Viewer (Android test app)

A small app for trying the CAD core on Android. It opens a `.blc` or DWG
file and shows a short summary (format, object counts, units, extents,
problems) and model space, with pinch zoom. It views only; it does not
edit or save.

- `app/src/main/java/com/withshalom/cad/MainActivity.java`: the screen.
  It opens files through the system file picker or "Open with", and shows
  the results.
- `Native.java`: the entry points of `libcad_ffi.so`, built from
  `crates/cad-ffi`.
- Java against Android's own APIs, with no libraries (no AndroidX, no
  Kotlin), so the APK holds no third-party code. CI checks that: the build
  fails if any class from outside `com.withshalom.cad` gets into it.
- No permissions. The file picker and "Open with" grant access to the one
  file.

## How it talks to the core

Only integers cross between Java and Rust. The app opens the drawing, plus
two output files in its cache folder, and passes the file descriptors to
`Native.view(drawingFd, infoFd, svgFd)`. The library reads the drawing,
writes a text summary and an SVG of model space, and closes all three. The
app shows the summary and loads the SVG as an image in a WebView, with
JavaScript off.

`Native.ABI_VERSION` must equal `cad_ffi::ABI_VERSION`. The app checks this
at startup and says so if the app and the library come from different
builds.

## Build

See "Android test viewer" in the top-level README. In short:
`cargo xtask android`, then Run in Android Studio, or
`gradle -p apps/android assembleDebug`.

- **minSdk and `--api` go together.** The app supports Android 8.0
  (API 26) and later. If `minSdk` in `app/build.gradle.kts` changes, pass
  the same level to `cargo xtask android --api`.
- **The Gradle wrapper is not in the repository.** `gradlew` and its jar
  are third-party files. `gradle/wrapper/gradle-wrapper.properties` pins
  Gradle 9.6.1, which Android Studio downloads by itself. For the command
  line, install Gradle 9.6 or later, or run `gradle wrapper` once here to
  create `./gradlew` locally.
- **Versions:** Android Gradle plugin 9.4.0 (needs Gradle 9.6 and JDK 17 or
  later), compile and target SDK 36, NDK r28 or later.

## Install

Debug builds are signed with a debug key, which is fine for testing. Each
computer has its own debug key, and each GitHub run makes a new one, so a
build from elsewhere cannot replace the installed app: uninstall it first
(`adb uninstall com.withshalom.cad`, or long-press the app → Uninstall).

- **Over USB:** `adb install -r app-debug.apk`.
- **Opening the APK on the phone:** allow installs from the app you open it
  with when asked.
- **Google's developer verification:** from September 30, 2026 in Brazil,
  Indonesia, Singapore and Thailand, and worldwide in 2027, certified
  Android devices only install apps from developers verified with Google.
  Unverified apps otherwise go through a slower "advanced flow" on the
  phone. Installing with `adb` is exempt. A free limited-distribution
  account covers up to 20 devices.

## Before a release

This is a test build. For people outside testing:

- **Application ID.** `com.withshalom.cad` is a placeholder until the
  product has a name, and it cannot change once published on Google Play.
- **Signing.** Create a release keystore (`keytool`), sign release builds
  with it, and back it up. An update signed with a different key cannot
  replace the installed app.
- **Google Play.** Upload an Android App Bundle
  (`gradle -p apps/android bundleRelease`), not an APK, with target SDK 36
  or later. The native libraries must be aligned for 16 KB memory pages;
  `cargo xtask android` checks this.
- **Saving.** Editing needs a decision on storage, because the Android file
  picker gives the app a content link to one file, not a path to the folder
  (see `docs/ARCHITECTURE.md`).
