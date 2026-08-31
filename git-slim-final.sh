#!/bin/bash
# git-slim-final.sh — Final aggressive cleanup
# WARNING: This rewrites history. All collaborators must re-clone.

set -e

echo "=== Current repo size ==="
du -sh .git
git count-objects -vH

echo ""
echo "=== Removing all large files from history ==="
# Remove all files > 1MB from history
git filter-repo --strip-blobs-bigger-than 1M --force

echo ""
echo "=== Cleaning up ==="
git reflog expire --expire=now --all
git gc --prune=now --aggressive

echo ""
echo "=== New repo size ==="
du -sh .git
git count-objects -vH

echo ""
echo "=== Verifying large blobs removed ==="
git verify-pack -v .git/objects/pack/*.idx 2>/dev/null | grep blob | sort -k 3 -n -r | head -5

echo ""
echo "=== Done! ==="
echo "All collaborators must re-clone this repository."
