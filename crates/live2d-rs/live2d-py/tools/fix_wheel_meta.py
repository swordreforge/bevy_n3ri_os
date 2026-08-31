#!/usr/bin/env python3
"""Strip PEP 639 metadata fields (License-Expression, license-file) from a wheel's METADATA."""
import sys, zipfile, os

whl = sys.argv[1]
tmp = whl + ".tmp"

with zipfile.ZipFile(whl, "r") as zin:
    with zipfile.ZipFile(tmp, "w", zipfile.ZIP_DEFLATED) as zout:
        for item in zin.infolist():
            data = zin.read(item.filename)
            if item.filename.endswith("METADATA"):
                lines = [l for l in data.decode().split("\n")
                         if not l.startswith("License-Expression:")
                         and not l.startswith("license-file:")]
                data = "\n".join(lines).encode()
            zout.writestr(item, data)
os.replace(tmp, whl)
print(f"Fixed: {os.path.basename(whl)}")
