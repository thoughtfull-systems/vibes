#!/bin/sh
# SPDX-License-Identifier: MPL-2.0-no-copyleft-exception
set -eu
android_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
properties=$android_dir/../../.config/android/gradle.properties
if [ "$#" -eq 0 ]; then
  set -- assembleDebug lintDebug testDebugUnitTest
fi
# Gradle reads gradle.properties only from the project root, so pass the file's entries as flags.
while IFS= read -r line || [ -n "$line" ]; do
  case "$line" in
    *[![:space:]]*) ;;
    *) continue ;;
  esac
  line=${line#"${line%%[![:space:]]*}"}
  case "$line" in
    '#'* | '!'*) continue ;;
    *=*) ;;
    *)
      echo "build.sh: $properties needs key=value lines: $line" >&2
      exit 2
      ;;
  esac
  key=${line%%=*}
  key=${key%"${key##*[![:space:]]}"}
  value=${line#*=}
  value=${value#"${value%%[![:space:]]*}"}
  value=${value%"${value##*[![:space:]]}"}
  case "$key" in
    org.gradle.*) ;;
    *) key=org.gradle.project.$key ;;
  esac
  set -- "-D$key=$value" "$@"
done <"$properties"
exec gradle -p "$android_dir" "$@"
