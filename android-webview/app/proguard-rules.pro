# Nothing here is reached by reflection or from JavaScript: there is no
# @JavascriptInterface anywhere in this app, deliberately. So the defaults are
# enough and this file exists only to say that on purpose.

# kmp-tor's process helper reaches for java.lang.management to find its own PID
# when it is running on a desktop JVM. Android has no such package, and the code
# path is guarded — but R8 sees the reference and stops. Nothing here executes
# it, so the reference is allowed to dangle.
-dontwarn java.lang.management.**
