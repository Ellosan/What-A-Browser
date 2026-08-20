plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.whatabrowser.wat.webview"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.whatabrowser.wat.webview"
        // WebView's mixed-content control and modern settings need 21; 24 keeps
        // this in step with the other Android app.
        minSdk = 24
        targetSdk = 34
        versionCode = 3
        versionName = "0.1.3"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        release {
            // There is very little code here, but shrinking still strips the
            // unused half of the WebKit compatibility library.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
        debug {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    // Off by default in AGP 8; the WebView's remote-debugging switch is keyed
    // on it, and getting that wrong ships a debuggable browser.
    buildFeatures {
        buildConfig = true
    }

    // Tor is a 8-9 MB native library per architecture, and nobody should
    // download four of them to use one. Each device gets its own APK; the
    // universal one exists for the checks in CI and for anyone who would rather
    // have a single file.
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
            isUniversalApk = true
        }
    }

    packaging {
        jniLibs {
            // Compresses the tor library in the APK. Uncompressed is the modern
            // default because it saves disk after install, but this app is
            // sideloaded rather than delivered by a store, and halving what has
            // to be downloaded is worth more here than the copy on disk.
            useLegacyPackaging = true
        }
    }

    lint {
        abortOnError = false
    }
}

// Deliberately thin. The browser is the system WebView and about a thousand
// lines of interface; every dependency added here is paid for at every cold
// start, and cold start is the whole point of this build.
dependencies {
    // Backports the modern WebView settings — Safe Browsing in particular — to
    // devices whose framework predates them.
    implementation("androidx.webkit:webkit:1.11.0")
    implementation("io.matthewnelson.kmp-tor:runtime:2.6.0")
    implementation("io.matthewnelson.kmp-tor:resource-noexec-tor:409.5.0")
    testImplementation("junit:junit:4.13.2")
}
