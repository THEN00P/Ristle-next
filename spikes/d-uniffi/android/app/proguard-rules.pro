# JNA reflects over Structure fields and registers native methods by name.
-keep class com.sun.jna.** { *; }
-keep class * extends com.sun.jna.** { *; }
-dontwarn java.awt.**
# Generated UniFFI bindings: JNA binds the native symbols to these method names.
-keep class uniffi.** { *; }
