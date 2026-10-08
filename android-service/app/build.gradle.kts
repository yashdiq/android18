import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
    id("org.jetbrains.kotlin.plugin.compose")
    id("com.google.devtools.ksp")
    id("com.google.dagger.hilt.android")
}

android {
    namespace = "com.android18.service"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.android18.service"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
    }

    // Release signing: `keystore.properties` (gitignored) or ANDROID18_KS_* env
    // vars. Without either, release falls back to the debug key so the APK is
    // always installable (never an "unsigned" artifact).
    val ksProps = Properties().apply {
        val file = rootProject.file("keystore.properties")
        if (file.exists()) file.inputStream().use { load(it) }
    }
    fun ks(key: String, env: String): String? =
        ksProps.getProperty(key) ?: System.getenv(env)
    val ksFile = ks("storeFile", "ANDROID18_KS_FILE")
    val hasReleaseKey = ksFile != null && file(ksFile).let { it.exists() || rootProject.file(ksFile).exists() }

    signingConfigs {
        if (hasReleaseKey) {
            create("release") {
                val f = file(ksFile!!)
                storeFile = if (f.exists()) f else rootProject.file(ksFile)
                storePassword = ks("storePassword", "ANDROID18_KS_PASS")
                keyAlias = ks("keyAlias", "ANDROID18_KEY_ALIAS")
                keyPassword = ks("keyPassword", "ANDROID18_KEY_PASS") ?: ks("storePassword", "ANDROID18_KS_PASS")
                enableV1Signing = true
                enableV2Signing = true
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = if (hasReleaseKey) {
                signingConfigs.getByName("release")
            } else {
                logger.warn("WARNING: no release keystore configured; signing release with the debug key")
                signingConfigs.getByName("debug")
            }
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
    buildFeatures {
        compose = true
    }
    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
}

dependencies {
    // Wire + transport.
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")
    implementation("io.ktor:ktor-server-core:3.0.1")
    implementation("io.ktor:ktor-server-cio:3.0.1")
    implementation("org.jmdns:jmdns:3.5.9")

    // Compose (BOM keeps artifact versions in sync; compiler ships with Kotlin 2.0.20).
    val composeBom = platform("androidx.compose:compose-bom:2024.09.03")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.6")
    implementation("androidx.navigation:navigation-compose:2.8.1")

    // DI.
    implementation("com.google.dagger:hilt-android:2.52")
    ksp("com.google.dagger:hilt-compiler:2.52")
    implementation("androidx.hilt:hilt-navigation-compose:1.2.0")

    // Pairing QR (generate) + scan-decode (CameraX feeds zxing YUV frames).
    // CameraX 1.4.0+ realigns libimage_processing_util_jni.so to 16 KB page
    // sizes — required for Play uploads targeting Android 15+ (Nov 2025).
    implementation("com.google.zxing:core:3.5.3")
    val camerax = "1.4.2"
    implementation("androidx.camera:camera-core:$camerax")
    implementation("androidx.camera:camera-camera2:$camerax")
    implementation("androidx.camera:camera-lifecycle:$camerax")
    implementation("androidx.camera:camera-view:$camerax")

    testImplementation("junit:junit:4.13.2")
}

