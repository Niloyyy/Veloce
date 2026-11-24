plugins {
    id("com.android.application")
    id("kotlin-android")
    id("dev.flutter.flutter-gradle-plugin")
}

val releaseKeystorePath = System.getenv("VELOCE_KEYSTORE_PATH")
val releaseKeystorePassword = System.getenv("VELOCE_KEYSTORE_PASSWORD")
val releaseKeyAlias = System.getenv("VELOCE_KEY_ALIAS")
val releaseKeyPassword = System.getenv("VELOCE_KEY_PASSWORD")
val hasReleaseKeystore = !releaseKeystorePath.isNullOrBlank() &&
    !releaseKeystorePassword.isNullOrBlank() &&
    !releaseKeyAlias.isNullOrBlank() &&
    !releaseKeyPassword.isNullOrBlank() &&
    file(releaseKeystorePath).exists()

android {
    namespace = "dev.veloce.p2p"
    compileSdk = maxOf(36, flutter.compileSdkVersion)

    defaultConfig {
        applicationId = "dev.veloce.p2p"
        minSdk = 24
        targetSdk = 35
        versionCode = 2
        versionName = "0.1.1"

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

    signingConfigs {
        if (hasReleaseKeystore) {
            create("release") {
                storeFile = file(releaseKeystorePath!!)
                storePassword = releaseKeystorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        release {
            // Use CI/local release keystore when configured; otherwise fall back to debug
            // signing so CI preview APKs are immediately sideloadable on Android devices.
            signingConfig = if (hasReleaseKeystore) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
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
