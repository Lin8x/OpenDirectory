// OpenDirectory Endpoint Agent for Android.
//
// Deliberately small: no AndroidX, Compose or Google Play services, only the
// platform APIs and the Kotlin standard library. The app has no background
// service; the system JobScheduler runs a check-in about every 15 minutes
// when there's a network and the battery isn't low.
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.opendirectory.agent"
    compileSdk = 35

    defaultConfig {
        applicationId = "dev.opendirectory.agent"
        minSdk = 26
        targetSdk = 35
        // Keep in step with the workspace version (Cargo.toml); the server
        // sends each agent only the settings its version understands.
        versionCode = 13
        versionName = "0.13.0"
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Signed with the debug key unless you configure your own signing.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures { buildConfig = true }
    dependenciesInfo { includeInApk = false; includeInBundle = false }
    lint { abortOnError = true; warningsAsErrors = false }
}

kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }
