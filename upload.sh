#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────
#  upload.sh — publish silver to your GitHub repository
#
#  usage:
#    ./upload.sh https://github.com/<you>/<repo>.git
#    ./upload.sh https://github.com/<you>/<repo>.git "my commit message"
#
#  Run it again any time you change the code — it commits and
#  pushes whatever is new.
# ─────────────────────────────────────────────────────────────
set -e
cd "$(dirname "$0")"

REPO_URL="$1"
MSG="${2:-silver ide — update}"

if [ -z "$REPO_URL" ]; then
    printf "GitHub repo URL (e.g. https://github.com/you/silver.git): "
    read -r REPO_URL
fi
if [ -z "$REPO_URL" ]; then
    echo "no repo URL given — nothing to do"
    exit 1
fi

# owner/repo from the URL (works for https and ssh forms)
SLUG=$(echo "$REPO_URL" | sed -E 's#^(git@github.com:|https://github.com/)##; s#\.git$##')

# Fill the real repo address into the README install commands.
if grep -q "YOUR-USERNAME/silver" README.md 2>/dev/null; then
    if sed --version >/dev/null 2>&1; then
        sed -i "s#YOUR-USERNAME/silver#${SLUG}#g" README.md          # linux
    else
        sed -i '' "s#YOUR-USERNAME/silver#${SLUG}#g" README.md       # macOS
    fi
    echo "→ README install commands now point at ${SLUG}"
fi

# First time: turn this folder into a git repository.
if [ ! -d .git ]; then
    git init
    echo "→ git repository created"
fi
git branch -M main

git add -A
git commit -m "$MSG" || echo "→ nothing new to commit"

# Point 'origin' at your repo (create or update it).
if git remote | grep -q '^origin$'; then
    git remote set-url origin "$REPO_URL"
else
    git remote add origin "$REPO_URL"
fi

echo "→ pushing to $REPO_URL ..."
git push -u origin main

echo ""
echo "✓ done! your IDE is live at: https://github.com/${SLUG}"
echo "  anyone can now install it with:"
echo "    cargo install --git https://github.com/${SLUG}"
