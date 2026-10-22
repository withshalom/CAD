#!/usr/bin/env bash
# Usage: publish-pages.sh SITE_DIR
#
# Replaces the gh-pages branch with one commit holding SITE_DIR (the web app
# and BlackCAD.html, from the web job), plus .nojekyll (serve the files as
# they are) and CNAME when PAGES_DOMAIN is set. Run by the workflow's pages
# job with GH_TOKEN, GITHUB_REPOSITORY and GITHUB_SHA set.
set -euo pipefail
site=$1
for f in index.html core.wasm sw.js manifest.webmanifest BlackCAD.html; do
  test -f "$site/$f" || { echo "the site has no $f"; exit 1; }
done
touch "$site/.nojekyll"
if [ -n "${PAGES_DOMAIN:-}" ]; then
  echo "$PAGES_DOMAIN" > "$site/CNAME"
fi
cd "$site"
git init -q -b gh-pages
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git add -A
git commit -q -m "Web app built from ${GITHUB_SHA}"
git push -q -f "https://x-access-token:${GH_TOKEN}@github.com/${GITHUB_REPOSITORY}.git" gh-pages
git log --stat --format='Published %H: %s' -1 | cat
