# Veloce Android R8 / ProGuard keep rules

# Keep Veloce Activity & Foreground Service
-keep class dev.veloce.p2p.** { *; }

# Flutter & flutter_rust_bridge JNI / FFI entrypoints
-keep class io.flutter.** { *; }
-keep class com.flutter_rust_bridge.** { *; }
-dontwarn io.flutter.embedding.**

# Google ML Kit & Play Services Barcode Scanning (mobile_scanner)
-keep class com.google.mlkit.** { *; }
-keep class com.google.android.gms.internal.mlkit_vision_barcode.** { *; }
-keep class com.google.android.gms.internal.mlkit_** { *; }
-keep class com.google.android.gms.vision.** { *; }
-keep class com.google.android.libraries.barhopper.** { *; }

# mobile_scanner & CameraX pipeline
-keep class dev.steenbakker.mobile_scanner.** { *; }
-dontwarn dev.steenbakker.mobile_scanner.**
-keep class androidx.camera.** { *; }
-dontwarn androidx.camera.**
