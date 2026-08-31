#!/usr/bin/env bash
set -euo pipefail

dir="assets/nori/app-icons/files/本机/下载"

for pdf in "$dir"/*.pdf; do
  [ -e "$pdf" ] || continue
  name="$(basename "$pdf" .pdf)"
  out="$dir/${name}.pdf.png"

  pages="$(pdfinfo "$pdf" 2>/dev/null | awk '/^Pages:/{print $2}')"
  if [ -z "$pages" ]; then
    echo "skip (broken): $(basename "$pdf")"
    continue
  fi

  width="$(awk -v p="$pages" 'BEGIN{w=int(8000/(1.414*p)); if(w>800)w=800; if(w<320)w=320; print w}')"

  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  pdftoppm -png -scale-to-x "$width" -scale-to-y -1 "$pdf" "$tmp/p"

  mapfile -t pages_imgs < <(ls -v "$tmp"/p-*.png)
  magick "${pages_imgs[@]}" -append "$out"
  rm -rf "$tmp"
  trap - EXIT

  h="$(magick identify -format '%h' "$out")"
  echo "ok: $(basename "$out") (${pages} pages, ${width}x${h})"
done
