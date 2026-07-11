# SRLTCP ProGuard / R8 rules
-keep class uniffi.** { *; }
-keep class com.srltcp.v2.service.SrltcpForegroundService { *; }

# Tink / EncryptedSharedPreferences optional annotations
-dontwarn com.google.errorprone.annotations.**
-dontwarn com.google.crypto.tink.**
-dontwarn javax.annotation.**