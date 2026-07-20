#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
#  release.sh — publish a downloadable version of silver
#
#  usage:
#    ./release.sh v0.2.0
#    ./release.sh v0.2.0 "what changed in this version"
#
#  It commits and pushes your code, then pushes a version tag.
#  GitHub Actions picks the tag up and builds the macOS, Linux,
#  and Windows downloads onto the Releases page automatically.
# ─────────────────────────────────────────────────────────────
set -e
cd "$(dirname "$0")"

VERSION="$1"
MSG="${2:-silver $VERSION}"

if [ -z "$VERSION" ]; then
    printf "version to publish (e.g. v0.2.0): "
    read -r VERSION
fi
if [ -z "$VERSION" ]; then
    echo "no version given — nothing to do"
    exit 1
fi
# Allow "0.2.0" as well as "v0.2.0".
case "$VERSION" in
    v*) ;;
    *) VERSION="v$VERSION" ;;
esac

# 1. Everything committed and on GitHub first.
git add -A
git commit -m "$MSG" || echo "→ nothing new to commit"
git push -u origin main

# 2. The tag is what makes GitHub build the downloads.
git tag -f "$VERSION"
git push -f origin "$VERSION"

echo ""
echo "✓ $VERSION is on its way!"
echo "  GitHub is now building the macOS, Linux, and Windows apps (~10 min)."
echo "  progress:  https://github.com/karanleo-coder/Silver_IDE/actions"
echo "  downloads: https://github.com/karanleo-coder/Silver_IDE/releases"

# Bonus: if the GitHub CLI is here, watch the build live.
if command -v gh >/dev/null 2>&1; then
    echo ""
    echo "watching the build (ctrl+c is safe — it keeps building without you):"
    sleep 5
    RUN_ID=$(gh run list --limit 1 --json databaseId --jq '.[0].databaseId' 2>/dev/null || true)
    if [ -n "$RUN_ID" ]; then
        gh run watch "$RUN_ID" --exit-status || {
            echo "✗ the build hit a problem — see the log:"
            echo "  gh run view $RUN_ID --log-failed"
            exit 1
        }
        echo "✓ downloads are live: https://github.com/karanleo-coder/Silver_IDE/releases"
    fi
fi
