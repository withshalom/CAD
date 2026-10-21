// Android test viewer for the CAD core. See README.md in this folder.

pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    // The app has no library dependencies. The build still fetches its own
    // tools (the Android Gradle plugin, aapt2) from these.
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "cad-viewer"
include(":app")
