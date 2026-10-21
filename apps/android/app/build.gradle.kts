plugins {
    id("com.android.application")
}

android {
    // Placeholder identity until the product is named. The application ID
    // cannot change once an app is published on Google Play.
    namespace = "com.withshalom.cad"
    compileSdk = 36

    // The app is Java only. Without this, AGP's built-in Kotlin support
    // would add the Kotlin standard library to the APK.
    enableKotlin = false

    defaultConfig {
        applicationId = "com.withshalom.cad"
        // Keep equal to the API level the native library is built for
        // (`cargo xtask android --api`, default 26).
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

// libcad_ffi.so comes from `cargo xtask android`, which writes it to
// src/main/jniLibs. Stop with a clear message if it is not there, rather
// than building an app that fails when it starts.
val jniLibsDir = layout.projectDirectory.dir("src/main/jniLibs").asFile
val checkNativeLibrary = tasks.register("checkNativeLibrary") {
    val dir = jniLibsDir
    doLast {
        if (!dir.walkTopDown().any { it.name == "libcad_ffi.so" }) {
            throw GradleException(
                "No libcad_ffi.so in $dir. Build it first, from the repository root: " +
                    "cargo xtask android"
            )
        }
    }
}
tasks.configureEach {
    if (name == "preBuild") {
        dependsOn(checkNativeLibrary)
    }
}
