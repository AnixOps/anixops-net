# The Rust side calls these by name through JNI.
-keep class io.anixops.net.core.NativeCore { *; }
-keep interface io.anixops.net.core.PlatformBridge { *; }
-keep class * implements io.anixops.net.core.PlatformBridge { public java.lang.String call(java.lang.String, java.lang.String); }
