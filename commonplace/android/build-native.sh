#!/bin/sh
# SPDX-License-Identifier: MPL-2.0-no-copyleft-exception
set -eu
android_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$android_dir/../.." && pwd)
manifest=$repo_dir/commonplace/Cargo.toml
output=$android_dir/app/build/generated
: "${ANDROID_HOME:?Set ANDROID_HOME to an Android SDK with platform 36 and NDK 28.2.13676358}"
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/28.2.13676358}
if [ ! -d "$ANDROID_NDK_HOME/toolchains" ]; then
  echo "Install NDK 28.2.13676358 with sdkmanager before building." >&2
  exit 1
fi
cargo build --locked --manifest-path "$manifest" -p commonplace-core --features bindgen
cd "$repo_dir/commonplace"
cargo run --locked --manifest-path "$manifest" -p commonplace-core --features bindgen \
  --bin commonplace-bindgen -- generate \
  --library "$repo_dir/commonplace/target/debug/libcommonplace_core.so" \
  --metadata-no-deps --language kotlin --config "$repo_dir/.config/android/uniffi.toml" \
  --out-dir "$output/kotlin" --no-format
cargo ndk -t arm64-v8a -t x86_64 --platform 26 -o "$output/jniLibs" \
  build --locked --manifest-path "$manifest" -p commonplace-core --features mobile --release
