plugins {
    id("com.android.application")
}

android {
    namespace = "com.whatabrowser.wat"
    compileSdk = 34

    // Gradle needs the NDK to strip the shared library. Without it the debug
    // APK ships every symbol the Rust build produced, which is hundreds of
    // megabytes.
    System.getenv("ANDROID_NDK_HOME")?.let { ndkPath = it }
        ?: System.getenv("ANDROID_NDK_ROOT")?.let { ndkPath = it }

    defaultConfig {
        applicationId = "com.whatabrowser.wat"
        // The browser draws its own interface with no platform widgets, so the
        // only real floor is what the NDK and winit's native-activity backend
        // need.
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    // `cargo ndk` writes the shared libraries here, one directory per ABI.
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    buildTypes {
        release {
            // The Rust side is already optimised by cargo, and there is no Java
            // to shrink.
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
        debug {
            isJniDebuggable = true
        }
    }

    // A missing ABI should fail the build rather than ship an APK that crashes
    // on the device it is missing for.
    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    lint {
        abortOnError = false
    }
}
