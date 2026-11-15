// The native Android app (Kotlin, Jetpack Compose). Built only in CI (`android-native` job in .github/workflows/ci.yml):
// the Rust bridge (`rust/`, libanixops_net.so) is built first with cargo-ndk into app/src/main/jniLibs, then Gradle.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "anixops-net-android"
include(":core", ":app")
