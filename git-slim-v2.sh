#!/bin/bash
# git-slim-v2.sh — Aggressive cleanup: remove assets_dev/ and large binaries
# WARNING: This rewrites history. All collaborators must re-clone.

set -e

echo "=== Current repo size ==="
du -sh .git
git count-objects -vH

echo ""
echo "=== Large files in assets_dev/ ==="
find assets_dev/ -type f -size +1M -exec ls -lh {} \; 2>/dev/null | awk '{print $5, $9}'

echo ""
echo "=== Removing assets_dev/ from git tracking ==="
git rm -r --cached assets_dev/ 2>/dev/null || true

echo ""
echo "=== Adding to .gitignore ==="
if ! grep -q "^assets_dev/" .gitignore; then
    echo "assets_dev/" >> .gitignore
fi

echo ""
echo "=== Committing removal ==="
git add .gitignore
git commit -m "chore: remove assets_dev/ from tracking (large binaries, textures, audio)

These files are development-only resources (161MB total) that don't belong in git:
- velvet-v8.1.1-x86_64-avx2 (50MB binary)
- cosmicweb.min.glb (28MB 3D model)
- Live2D texture files (20MB+)
- Audio files (14MB+)

Use the local assets_dev/ directory for development."

echo ""
echo "=== Rewriting history to remove assets_dev/ ==="
git filter-repo --invert-paths --path-glob 'assets_dev/*' --force

echo ""
echo "=== Cleaning up ==="
git reflog expire --expire=now --all
git gc --prune=now --aggressive

echo ""
echo "=== New repo size ==="
du -sh .git
git count-objects -vH

echo ""
echo "=== Done! ==="
echo "All collaborators must re-clone this repository."
