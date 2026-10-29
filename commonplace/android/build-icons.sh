#!/bin/sh
# SPDX-License-Identifier: MPL-2.0-no-copyleft-exception
set -eu
android_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
linux_icons=$android_dir/../linux/data/icons/hicolor
output=$android_dir/app/build/generated/icons/res
for density in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  case "$density" in
    mdpi) size=24 ;;
    hdpi) size=36 ;;
    xhdpi) size=48 ;;
    xxhdpi) size=72 ;;
    xxxhdpi) size=96 ;;
  esac
  drawables=$output/drawable-$density
  mipmaps=$output/mipmap-$density
  mkdir -p "$drawables" "$mipmaps"
  for icon in actions add all_notes back bold bullets clear_search code down drag dropdown \
    edit execute inbox indent italic link more numbers outdent plan review search sort_ascending \
    sort_descending starred strike trash up; do
    rsvg-convert --width "$size" --height "$size" \
      --output "$drawables/$icon.png" "$android_dir/vendor/adwaita/$icon.svg"
  done
  for icon in journal memo; do
    rsvg-convert --width "$size" --height "$size" --output "$drawables/$icon.png" \
      "$linux_icons/symbolic/apps/systems.thoughtfull.Commonplace-$icon-symbolic.svg"
  done
  rsvg-convert --width "$((size * 9 / 2))" --height "$((size * 9 / 2))" \
    --output "$drawables/commonplace.png" \
    "$linux_icons/scalable/apps/systems.thoughtfull.Commonplace.svg"
  rsvg-convert --width "$((size * 2))" --height "$((size * 2))" \
    --output "$mipmaps/commonplace.png" \
    "$linux_icons/scalable/apps/systems.thoughtfull.Commonplace.svg"
  rsvg-convert --width "$((size * 9 / 2))" --height "$((size * 9 / 2))" \
    --output "$drawables/commonplace_symbolic.png" \
    "$linux_icons/symbolic/apps/systems.thoughtfull.Commonplace-symbolic.svg"
done
