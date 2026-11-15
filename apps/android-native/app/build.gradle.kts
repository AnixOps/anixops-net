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
        // arm64 only in the APK; CI's emulator smoke test builds x86_64 instead (-Panixops.abis=x86_64).
        ndk {
            abiFilters += (findProperty("anixops.abis") as String? ?: "arm64-v8a").split(",")
        }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
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
