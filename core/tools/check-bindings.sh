#!/usr/bin/env bash
# Generates the Kotlin and Swift bindings of ristle-ffi and runs a smoke
# program in each against the real library (ADR 0001).
#
#   tools/check-bindings.sh generate   # library mode, from an unstripped debug build
#   tools/check-bindings.sh kotlin     # compile with kotlinc, run on the JVM
#   tools/check-bindings.sh swift      # Swift 5 module + a consumer module, run
#
# Linux, from any directory. `kotlin` needs kotlinc, java and curl; `swift`
# needs swiftc. Output goes to core/target/bindings-check.
set -euo pipefail

core="$(cd "$(dirname "$0")/.." && pwd)"
out="$core/target/bindings-check"
generated="$out/generated"
lib_dir="$core/target/debug"

# Pinned runtime dependencies of the generated Kotlin.
jna_version=5.17.0
jna_sha256=b3a9408e7c51e08ef0e3bfcc08f443f6ec0f6191ba8cd7c18d53d2b22e5bdbc0
coroutines_version=1.10.2
coroutines_sha256=5ca175b38df331fd64155b35cd8cae1251fa9ee369709b36d42e0a288ccce3fd

generate() {
    cd "$core"
    # Debug builds keep symbols, which library mode reads its metadata from.
    cargo build --locked -p ristle-ffi
    rm -rf "$generated"
    cargo run --locked -q -p uniffi-bindgen -- generate \
        --library "$lib_dir/libristle_core.so" \
        --language kotlin --language swift \
        --no-format --out-dir "$generated"
    wc -l "$generated/uniffi/ristle_core/ristle_core.kt" "$generated/ristle_core.swift"
}

fetch() { # url sha256 file
    if [ ! -f "$3" ] || ! echo "$2  $3" | sha256sum --check --status; then
        curl --fail --silent --show-error --location --output "$3" "$1"
        echo "$2  $3" | sha256sum --check
    fi
}

kotlin() {
    local dir="$out/kotlin" maven=https://repo1.maven.org/maven2
    mkdir -p "$dir"
    fetch "$maven/net/java/dev/jna/jna/$jna_version/jna-$jna_version.jar" \
        "$jna_sha256" "$dir/jna.jar"
    fetch "$maven/org/jetbrains/kotlinx/kotlinx-coroutines-core-jvm/$coroutines_version/kotlinx-coroutines-core-jvm-$coroutines_version.jar" \
        "$coroutines_sha256" "$dir/coroutines.jar"
    local classpath="$dir/jna.jar:$dir/coroutines.jar"
    kotlinc -version
    kotlinc -Werror -include-runtime -classpath "$classpath" -d "$dir/smoke.jar" \
        "$generated/uniffi/ristle_core/ristle_core.kt" "$core/crates/ffi/smoke/Smoke.kt"
    java -Djna.library.path="$lib_dir" -classpath "$dir/smoke.jar:$classpath" SmokeKt
}

swift() {
    local dir="$out/swift"
    rm -rf "$dir"
    mkdir -p "$dir/ffi"
    # The generated module map also names Darwin modules, which Linux lacks.
    cp "$generated/ristle_coreFFI.h" "$dir/ffi/"
    printf 'module ristle_coreFFI {\n    header "ristle_coreFFI.h"\n    export *\n}\n' \
        >"$dir/ffi/module.modulemap"
    swiftc --version
    # The bindings in their own Swift 5 language-mode module (ADR 0001).
    swiftc -swift-version 5 -parse-as-library -module-name ristle_core \
        -emit-library -emit-module -emit-module-path "$dir/ristle_core.swiftmodule" \
        -I "$dir/ffi" -L "$lib_dir" -lristle_core \
        "$generated/ristle_core.swift" -o "$dir/libristle_core_swift.so"
    # A consumer in another module.
    swiftc -swift-version 5 -I "$dir" -I "$dir/ffi" \
        -L "$dir" -lristle_core_swift -L "$lib_dir" -lristle_core \
        "$core/crates/ffi/smoke/swift/main.swift" -o "$dir/smoke"
    LD_LIBRARY_PATH="$dir:$lib_dir" "$dir/smoke"
}

case "${1:-}" in
    generate | kotlin | swift) "$1" ;;
    *)
        echo "usage: $0 generate|kotlin|swift" >&2
        exit 2
        ;;
esac
