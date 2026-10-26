/* 64-bit integer to float conversions under their generic names, which
 * LuaJIT calls: on ARM, zig's runtime and Rust's compiler-builtins provide
 * them only under their EABI names. client-ng/build.sh links this into LuaJIT's
 * own `luajit` program, built on the way to its library, and the client. */
double __floatdidf(long long a) { return (double)a; }
double __floatundidf(unsigned long long a) { return (double)a; }
float __floatdisf(long long a) { return (float)a; }
float __floatundisf(unsigned long long a) { return (float)a; }
