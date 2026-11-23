plugins {
    id("com.android.application")
    id("kotlin-android")
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "dev.veloce.p2p"
    compileSdk = maxOf(36, flutter.compileSdkVersion)

    defaultConfig {
        applicationId = "dev.veloce.p2p"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"

        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    // Required for Android 15+ 16 KB ELF page-size alignment compliance.
    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}

// Optional Gradle task to compile Rust core-mobile-ffi via cargo-ndk with 16 KB page alignment.
val buildRustAndroid by tasks.registering(Exec::class) {
    workingDir = file("../..")
    commandLine("bash", "scripts/build-android-rust.sh")
}
