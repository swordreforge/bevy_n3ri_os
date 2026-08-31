#!/bin/bash
# git-slim.sh — Rewrite history to remove large files
# WARNING: This rewrites history. All collaborators must re-clone.

set -e

echo "=== Current repo size ==="
du -sh .git
git count-objects -vH

echo ""
echo "=== Files to remove from history ==="
echo "1. n3ri-minimal (compiled binary, ~50MB)"
echo "2. *.wav files (old audio format, converted to ogg)"
echo "3. *.m4a files (old audio format, converted to ogg)"
echo "4. *.mp3 files (old audio format, converted to ogg)"
echo ""

# Create paths file for git-filter-repo
cat > /tmp/git-filter-paths.txt << 'EOF'
n3ri-minimal
*.wav
*.m4a
*.mp3
EOF

echo "=== Running git-filter-repo ==="
git filter-repo --invert-paths --paths-from-file /tmp/git-filter-paths.txt --force

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
