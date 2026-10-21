#!/usr/bin/env bash
# Usage: annotate-failure.sh TITLE LOGFILE
#
# Prints the end of a failed step's output as an error annotation. The
# annotation shows in the pull request's checks and can be read through the
# API (check-run annotations), where the full job log cannot always be.
set -u
title=$1
log=$2
# Workflow commands take one line: %, CR and LF are escaped.
body=$(tail -c 6000 "$log" | sed -e 's/%/%25/g' -e 's/\r/%0D/g' | awk '{ printf "%s%%0A", $0 }')
echo "::error title=${title}::${body}"
