plugins {
    id("com.android.application")
    id("kotlin-android")
    id("dev.flutter.flutter-gradle-plugin")
}

val releaseKeystorePath = System.getenv("VELOCE_KEYSTORE_PATH")
val releaseKeystorePassword = System.getenv("VELOCE_KEYSTORE_PASSWORD")
val releaseKeyAlias = System.getenv("VELOCE_KEY_ALIAS")
val releaseKeyPassword = System.getenv("VELOCE_KEY_PASSWORD")
val hasAnyReleaseSigningEnv = listOf(
    releaseKeystorePath,
    releaseKeystorePassword,
    releaseKeyAlias,
    releaseKeyPassword,
).any { !it.isNullOrBlank() }
val hasReleaseKeystore = !releaseKeystorePath.isNullOrBlank() &&
    !releaseKeystorePassword.isNullOrBlank() &&
    !releaseKeyAlias.isNullOrBlank() &&
    !releaseKeyPassword.isNullOrBlank() &&
    file(releaseKeystorePath).exists()

if (hasAnyReleaseSigningEnv && !hasReleaseKeystore) {
    throw GradleException(
        "Incomplete Android release signing configuration: ensure VELOCE_KEYSTORE_PATH (existing file), " +
            "VELOCE_KEYSTORE_PASSWORD, VELOCE_KEY_ALIAS, and VELOCE_KEY_PASSWORD are all set."
    )
}

android {
    namespace = "dev.veloce.p2p"
    compileSdk = maxOf(36, flutter.compileSdkVersion)

    defaultConfig {
        applicationId = "dev.veloce.p2p"
        minSdk = 24
        targetSdk = 35
        versionCode = 3
        versionName = "0.1.2"
    }

    androidResources {
        localeFilters += listOf("en")
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    // Compress native .so libraries inside the APK (cuts download size by ~60% while
    // ELF segments remain 16 KB page-aligned via -Wl,-z,max-page-size=16384).
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
        resources {
            excludes += setOf(
                "META-INF/*.kotlin_module",
                "META-INF/DEPENDENCIES",
                "META-INF/LICENSE*",
                "META-INF/NOTICE*",
                "META-INF/AL2.0",
                "META-INF/LGPL2.1",
                "DebugProbesKt.bin",
                "kotlin-tooling-metadata.json",
            )
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
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
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
