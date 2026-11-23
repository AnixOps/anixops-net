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
        // The Tauri app's id, which this app replaced (docs/architecture/android-native-compose.md): it installs over
        // it with the same key and keeps its data directory.
        applicationId = "io.anixops.net.android"
        minSdk = 26
        // Google Play has required API 36 for new apps and updates since 2026-08-31 (extensions ended 2026-11-01).
        targetSdk = 36
        // Above the Tauri app's last code (3000: Tauri's major × 1000000 + minor × 1000 + patch for 0.3.0), so
        // Android takes this as an upgrade. Raise it with each release: release.yml refuses a tag whose versionCode
        // is not above the previous release's, and Google Play refuses a code it has already seen (3001 went to
        // internal testing on 2026-11-16).
        versionCode = 3007
        // release.yml and play-bundle.yml pass the tag without its "v" (-Panixops.versionName=0.3.0-beta.7); local
        // and CI builds show the plain version. The default is a copy of Cargo.toml's workspace version, kept equal
        // by .github/scripts/version.py (set it with `version.py set X.Y.Z`).
        versionName = findProperty("anixops.versionName") as String? ?: "0.3.0"
        // arm64 only in the APK; CI's emulator smoke test builds x86_64 instead (-Panixops.abis=x86_64).
        ndk {
            abiFilters += (findProperty("anixops.abis") as String? ?: "arm64-v8a").split(",")
        }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // The launcher name.
        manifestPlaceholders["appLabel"] = "AnixOps Net"
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

    bundle {
        // English is the default language now and Chinese a language split: keep every language in the base, so the
        // system's per-app language page can switch to one the device itself does not use (a few hundred KB).
        language {
            enableSplit = false
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
            // arm64 only, as the Tauri app: ChromeOS runs it through its ARM translation; an x86_64 library would
            // double the Rust build for a platform we do not target.
            "ChromeOsAbiSupport",
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
    // QR import: CameraX for the preview, ZXing core (Apache-2.0) to decode; no Google Play services.
    implementation(libs.androidx.camera.camera2)
    implementation(libs.androidx.camera.lifecycle)
    implementation(libs.androidx.camera.view)
    implementation(libs.zxing.core)
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.robolectric)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    testImplementation(libs.roborazzi)
    testImplementation(libs.roborazzi.compose)

    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
}

/** Packs the user agreement (TERMS.md at the repository root) into the assets, so the app shows the one it was built with. */
abstract class TermsAsset : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val source: RegularFileProperty

    @get:OutputDirectory
    abstract val output: DirectoryProperty

    @TaskAction
    fun copy() {
        source.get().asFile.copyTo(output.get().file("TERMS.md").asFile, overwrite = true)
    }
}

val termsAsset = tasks.register<TermsAsset>("termsAsset") {
    source.set(rootProject.layout.projectDirectory.file("../../TERMS.md"))
}

androidComponents {
    onVariants { variant ->
        variant.sources.assets?.addGeneratedSourceDirectory(termsAsset, TermsAsset::output)
    }
}
