#!/data/data/com.termux/files/usr/bin/bash
# Attaches a Termux .deb (built via scripts/termux-package.sh) to the
# GitHub Release that the "Release" workflow already created for this
# version. Must run on-device, after the tag has been pushed.
set -euo pipefail

command -v gh >/dev/null || { echo "Install GitHub CLI first: pkg install gh"; exit 1; }

DEB=$(ls -t bmake_*.deb 2>/dev/null | head -n1)
if [ -z "${DEB:-}" ]; then
  echo "No bmake_*.deb found — run scripts/termux-package.sh first."
  exit 1
fi

TAG="v$(grep -m1 '^version' Cargo.toml | cut -d '"' -f2)"
echo "Uploading ${DEB} to release ${TAG}..."
gh release upload "$TAG" "$DEB" --clobber
echo "Done — ${DEB} attached to ${TAG}."