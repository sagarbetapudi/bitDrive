# Bluetooth Personal Link ProGuard Rules

# Keep protobuf generated classes
-keep class com.bluetoothpersonallink.protocol.** { *; }
-keep class com.google.protobuf.** { *; }

# Keep Room database classes
-keep class com.bluetoothpersonallink.core.db.** { *; }
-keep class androidx.room.** { *; }

# Keep DataStore classes
-keep class androidx.datastore.** { *; }

# Keep WorkManager classes
-keep class androidx.work.** { *; }

# Keep ExoPlayer classes
-keep class com.google.android.exoplayer2.** { *; }

# Keep OkHttp/Okio classes
-keep class okhttp3.** { *; }
-keep class okio.** { *; }

# Keep Moshi classes
-keep class com.squareup.moshi.** { *; }

# Keep Timber
-keep class timber.log.** { *; }

# Keep Kotlin coroutines
-keep class kotlinx.coroutines.** { *; }

# Keep Kotlin serialization
-keep class kotlinx.serialization.** { *; }

# Keep protobuf-kotlin
-keep class com.google.protobuf.kotlin.** { *; }

# Prevent obfuscation of model classes
-keep class com.bluetoothpersonallink.core.db.entities.** { *; }

# Keep Bluetooth classes
-keep class android.bluetooth.** { *; }

# Keep foreground service
-keep class com.bluetoothpersonallink.foreground.BluetoothForegroundService { *; }

# Keep application class
-keep class com.bluetoothpersonallink.MainApplication { *; }

# Keep activity
-keep class com.bluetoothpersonallink.ui.MainActivity { *; }
-keep class com.bluetoothpersonallink.ui.SettingsActivity { *; }

# Keep documents provider
-keep class com.bluetoothpersonallink.provider.BluetoothDocumentsProvider { *; }

# Keep service classes
-keep class com.bluetoothpersonallink.services.** { *; }

# Don't warn about missing classes
-dontwarn kotlinx.coroutines.**
-dontwarn kotlinx.serialization.**
-dontwarn androidx.room.**
-dontwarn androidx.datastore.**
-dontwarn androidx.work.**
-dontwarn com.google.android.exoplayer2.**
-dontwarn okhttp3.**
-dontwarn okio.**
-dontwarn com.squareup.moshi.**
-dontwarn timber.log.**
-dontwarn com.google.protobuf.**
-dontwarn android.bluetooth.**