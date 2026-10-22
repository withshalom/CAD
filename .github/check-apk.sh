#!/usr/bin/env bash
# Usage: check-apk.sh APK
#
# Native libraries aligned for 16 KB pages, and no code but the app's own:
# no third-party libraries slipped in.
set -euo pipefail
apk=$1
bt="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -n 1)"
unzip -l "$apk"
"$bt/zipalign" -c -P 16 4 "$apk"
classes="$("$bt/dexdump" "$apk" | grep "Class descriptor" | sed "s/.*'\(.*\)'.*/\1/" | sort -u)"
echo "Classes defined in the APK, by package:"
echo "$classes" | sed -E 's|^L(([^/;]+/){0,2}).*|\1|' | sort | uniq -c
# A here-string, not `echo | grep -q`: grep -q stops at the first match, and
# with pipefail the echo it cut off fails the check (it did, at random).
grep -qx "Lcom/withshalom/cad/MainActivity;" <<<"$classes" \
  || { echo "dexdump output not understood"; exit 1; }
# D8 also defines stubs of platform classes newer than the app's minimum
# API level (android.*, dalvik.*: its API modelling), so that older
# devices verify the app, and its own desugaring helpers
# (com.android.tools.r8.*). They come with the toolchain, not a library.
other="$(echo "$classes" | grep -v -e '^Lcom/withshalom/cad/' -e '^Landroid/' -e '^Ldalvik/' \
  -e '^Lcom/android/tools/r8/' || true)"
if [ -n "$other" ]; then
  echo "Classes from outside the app, the platform and the toolchain:"
  echo "$other"
  exit 1
fi
