// The Kotlin side of the bridge to the Rust client (libanixops_net.so): typed commands over `NativeCore`, and the
// envelope the Rust side calls back through (`PlatformBridge`). No UI; its tests run on the JVM.
plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.serialization)
}

android {
    namespace = "io.anixops.net.core"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    lint {
        warningsAsErrors = true
        abortOnError = true
        // Version bumps are a deliberate PR of their own (gradle/libs.versions.toml), not a lint failure.
        disable += setOf("GradleDependency", "NewerVersionAvailable", "AndroidGradlePluginVersion")
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation(libs.kotlinx.coroutines.android)
    api(libs.kotlinx.serialization.json)
    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
}

// The shared request and answer samples (crates/client-core/tests/fixtures) the Rust tests also read.
tasks.withType<Test>().configureEach {
    systemProperty("fixtures", rootDir.resolve("../../crates/client-core/tests/fixtures").absolutePath)
    // The page models' samples (crates/client-view), which the Rust tests check against the view commands.
    systemProperty("viewCases", rootDir.resolve("../../crates/client-view/fixtures/cases.json").absolutePath)
}
