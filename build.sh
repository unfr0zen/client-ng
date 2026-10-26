#!/bin/sh
# Builds client-ng for the gen 3 and the gen 2 (ARMv7, hard float,
# against the gen 2's glibc 2.19, which the gen 3's 2.31 also runs; the gen
# 3's Vivante GPU libraries are glibc shared objects) and stages it with its
# skins and menus in build/client-ng/, as deployed.
#
#     client-ng/build.sh            # macOS: in the unfr0zen-build Docker image;
#                                # Linux: natively, with that image's tools
#     CLIENT_NATIVE=1 client-ng/build.sh   # with this host's cargo-zigbuild and zig
#
# Skins run on LuaJIT, whose build runs tools of the target's 32-bit width
# on the build host: $HOST_CC compiles them (in Docker, zig's 32-bit ARM,
# which the image runs; natively, set it to a 32-bit compiler such as
# "gcc -m32" on x86 Linux).
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
target=armv7-unknown-linux-gnueabihf
glibc=2.19
stage=build/client-ng

build="cargo zigbuild --locked --profile client -p client-ng --target $target.$glibc --target-dir .work/client-target"
# For the Cortex-A8 (the gen 2's; the gen 3's A9 runs it too), with the NEON
# the CPU painter's SIMD uses.
# A fixed-address executable, bound lazily: the gen 2's glibc 2.19 loader
# crashes binding a Rust binary now, and its 2.6.37 kernel maps a PIE among
# the libraries, where the loader crashes now and then; the gen 3 runs it
# as well.
# LuaJIT's own program, linked on the way to its library, needs an
# unwinder, and it and the client need the integer-to-float helpers
# LuaJIT calls by their generic names (client-ng/luajit/floatdi.c).
# The sources' paths the binary keeps for panics are named from the crates'
# folder, Rust's own and this repository, never the build host's.
luajit='zig=$(dirname "$(python3 -c "import ziglang; print(ziglang.__file__)")")/zig
    mkdir -p .work/luajit
    $zig cc -target arm-linux-gnueabihf.'"$glibc"' -O2 -c client-ng/luajit/floatdi.c -o .work/luajit/floatdi.o
    export HOST_CC="$zig cc -target arm-linux-musleabihf" TARGET_XLIBS="-lunwind $root/.work/luajit/floatdi.o -lm"
    export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_RUSTFLAGS="-C link-arg=$root/.work/luajit/floatdi.o -C link-arg=-Wl,-z,lazy -C relocation-model=static -C link-arg=-no-pie -C target-cpu=cortex-a8 --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}/registry/src=crates --remap-path-prefix=$(rustc --print sysroot)=rust --remap-path-prefix=$root=."'
if [ "${CLIENT_NATIVE:-}" = 1 ]; then
    sh -c "$build"
elif [ "$(uname -s)" != Darwin ]; then
    root="$root" sh -c "$luajit; $build"
else
    image=unfr0zen-build
    docker image inspect "$image" > /dev/null 2>&1 || docker build -q -t "$image" - < Dockerfile > /dev/null
    docker run --rm -v "$root:$root" -w "$root" --user "$(id -u):$(id -g)" \
        -e HOME=/tmp -e CARGO_HOME="$root/.work/cargo" -e root="$root" "$image" sh -c "$luajit; $build"
fi

rm -rf "$stage"
mkdir -p "$stage/config"
cp ".work/client-target/$target/client/client-ng" "$stage/"
cp -R client-ng/skins "$stage/"
cp client-ng/config/menus.lua "$stage/config/"
rm -rf "$stage/config/plugins" "$stage/config/locales"
cp -R client-ng/config/plugins client-ng/config/locales "$stage/config/"
# Doom's title graphic, from the shareware WAD the firmware build fetches.
if [ -f build/doom/doom1.wad ]; then
    python3 client-ng/tools/doom-logo.py build/doom/doom1.wad "$stage/config/plugins/doom-logo.png" > /dev/null
fi
echo "staged $stage ($(du -sh "$stage" | cut -f1))"
