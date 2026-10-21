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
echo "$classes"
echo "$classes" | grep -qx "Lcom/withshalom/cad/MainActivity;" \
  || { echo "dexdump output not understood"; exit 1; }
if echo "$classes" | grep -v "^Lcom/withshalom/cad/"; then
  echo "classes from outside the app (listed above)"
  exit 1
fi
