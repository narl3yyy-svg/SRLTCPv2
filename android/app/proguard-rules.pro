# SRLTCP ProGuard / R8 rules

# UniFFI Kotlin bindings + native peers
-keep class uniffi.** { *; }
-keep class com.srltcp.v2.** { *; }
-keep class com.srltcp.v2.service.SrltcpForegroundService { *; }

# JNA — required by UniFFI. R8 must not rename Pointer/Structure fields or
# native code fails with: "Can't obtain peer field ID for class com.sun.jna.Pointer"
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.** { public *; }
-keepclassmembers class com.sun.jna.** {
    public <fields>;
    public <methods>;
    *;
}
-dontwarn java.awt.**
-dontwarn com.sun.jna.**

# Tink / EncryptedSharedPreferences optional annotations
-dontwarn com.google.errorprone.annotations.**
-dontwarn com.google.crypto.tink.**
-dontwarn javax.annotation.**