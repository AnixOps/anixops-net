// The app: Compose UI, the VPN service and what the Rust client asks of Android. libanixops_net.so comes from
// CI's cargo-ndk step (src/main/jniLibs, not committed).
plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
    alias(libs.plugins.roborazzi)
}

android {
    namespace = "io.anixops.net"
    compileSdk = 36

    defaultConfig {
        // The Tauri app's id: the cutover (docs/architecture/android-native-compose.md) installs over it and keeps
        // its data directory. Until then the preview build carries the ".next" suffix and installs beside it.
        applicationId = "io.anixops.net.android"
        applicationIdSuffix = ".next"
        minSdk = 26
        targetSdk = 35
        versionCode = 300
        versionName = "0.3.0"
        ndk {
            abiFilters += setOf("arm64-v8a")
        }
        // The launcher name; "（预览）" until the cutover, so the two apps are told apart.
        manifestPlaceholders["appLabel"] = "AnixOps Net（预览）"
    }

    buildTypes {
        release {
            // Off until the release build has an emulator smoke test; the JNI entry points are kept by
            // core/consumer-rules.pro for when it is turned on.
            isMinifyEnabled = false
            // CI signs the APK (zipalign + apksigner, .github/actions/sign-apk).
            signingConfig = null
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    testOptions {
        unitTests {
            isIncludeAndroidResources = true
        }
    }

    lint {
        warningsAsErrors = true
        abortOnError = true
        disable += setOf(
            // Version bumps are a deliberate PR of their own (gradle/libs.versions.toml), not a lint failure.
            "GradleDependency",
            "NewerVersionAvailable",
            "AndroidGradlePluginVersion",
            "OldTargetApi",
            // One launcher bitmap until the icon set is generated from AppIcon.svg (cutover checklist).
            "IconDensities",
            "IconMissingDensityFolder",
            "IconLauncherShape",
        )
    }
}

kotlin {
    jvmToolchain(17)
}

dependencies {
    implementation(project(":core"))
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material3.navigation.suite)
    implementation(libs.compose.material.icons.core)
    implementation(libs.kotlinx.coroutines.android)
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    testImplementation(libs.roborazzi)
    testImplementation(libs.roborazzi.compose)
}
