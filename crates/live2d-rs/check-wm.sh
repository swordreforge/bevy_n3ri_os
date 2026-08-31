#!/bin/bash

# 方法1：检查合成器特有环境变量（最可靠）
if [ -n "$HYPRLAND_INSTANCE_SIGNATURE" ]; then
    echo "Hyprland"
elif [ -n "$SWAYSOCK" ]; then
    echo "Sway"
elif [ -n "$NIRI_SOCKET" ] || command -v niri &>/dev/null && niri msg version &>/dev/null 2>&1; then
    echo "Niri"
elif [ "$XDG_CURRENT_DESKTOP" = "GNOME" ] || pgrep -x gnome-shell &>/dev/null; then
    echo "GNOME"
elif [ "$XDG_CURRENT_DESKTOP" = "KDE" ] || [ "$XDG_CURRENT_DESKTOP" = "KDE Plasma" ]; then
    echo "KDE Plasma"
else
    echo "未知: XDG_CURRENT_DESKTOP=$XDG_CURRENT_DESKTOP"
fi
