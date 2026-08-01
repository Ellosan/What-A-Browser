import java.io.File
import java.util.Properties

plugins {
    id("com.android.application")
}

// Gradle needs the NDK to strip the shared library. Without it the debug APK
// ships every symbol the Rust build produced, which is hundreds of megabytes.
//
// `ndkVersion` has to agree with whatever `ndkPath` points at. When they
// disagree the plugin does not fail — it warns and packages the library
// unstripped, which is a size regression that hides easily. So the version is
// read out of the NDK rather than written down a second time here.
//
// This is computed outside `android { }` because in there `java` resolves to the
// Java extension rather than the package, and `java.io.File` will not compile.
val ndkDir: String? = (System.getenv("ANDROID_NDK_HOME") ?: System.getenv("ANDROID_NDK_ROOT"))
    ?.takeIf { File(it, "source.properties").isFile }
val ndkRevision: String? = ndkDir?.let { dir ->
    val properties = Properties()
    File(dir, "source.properties").inputStream().use(properties::load)
    properties.getProperty("Pkg.Revision")
}

android {
    namespace = "com.whatabrowser.wat"
    compileSdk = 34

    ndkDir?.let { ndkPath = it }
    ndkRevision?.let { ndkVersion = it }

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
