#!/usr/bin/env python3
"""Generate the README download buttons (light and dark SVGs).

    python3 docs/buttons/make_buttons.py

Brand glyphs in icons.json come from Simple Icons (CC0); the Windows and
terminal glyphs are drawn here. Edit BUTTONS below and rerun to change them.
"""
import json
from pathlib import Path
from xml.sax.saxutils import escape

HERE = Path(__file__).resolve().parent
ICONS = json.loads((HERE / "icons.json").read_text())

# Terminal glyph (24x24): a window with a prompt.
TERMINAL = "M3 4h18a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2zm0 2v12h18V6H3zm2.3 2.7 1.4-1.4L10.4 11l-3.7 3.7-1.4-1.4 2.3-2.3-2.3-2.3zM11 13h6v2h-6v-2z"

# id: (icon, tile colour, caption, label, coming soon?)
BUTTONS = {
    "exe": ("windows", "#0078D4", "Windows 10 · 11", ".exe installer", False),
    "msi": ("windows", "#0078D4", "Windows · GPO · Intune", ".msi package", False),
    "dmg": ("apple", "#1d1d1f", "Apple silicon · Intel", ".dmg disk image", False),
    "deb": ("debian", "#A81D33", "Debian · Ubuntu · Mint", ".deb package", False),
    "rpm": ("fedora", "#294172", "Fedora · RHEL · openSUSE", ".rpm package", False),
    "arch": ("archlinux", "#1793D1", "Arch · Manjaro", ".pkg.tar.zst", False),
    "appimage": ("linux", "#333333", "Any Linux", "AppImage", False),
    "flatpak": ("flatpak", "#4A90D9", "Any Flatpak Linux", ".flatpak bundle", False),
    "snap": ("snapcraft", "#4C8B6F", "Ubuntu · Snap", ".snap package", False),
    "terminal": (None, "#24292F", "Install with", "One command", False),
    "homebrew": ("homebrew", "#B5791B", "Coming soon to", "Homebrew", True),
    "flathub": ("flathub", "#4A86CF", "Coming soon to", "Flathub", True),
    "snapstore": ("snapcraft", "#4C8B6F", "Coming soon to", "Snap Store", True),
    "aur": ("archlinux", "#1793D1", "Coming soon to", "AUR", True),
    "apt": ("ubuntu", "#E95420", "Coming soon to", "APT repository", True),
    "dnf": ("fedora", "#294172", "Coming soon to", "DNF repository", True),
}

THEMES = {
    "light": dict(bg="#ffffff", border="#d0d7de", caption="#57606a", label="#1f2328"),
    "dark": dict(bg="#161b22", border="#3d444d", caption="#9198a1", label="#f0f6fc"),
}

W, H = 248, 64
FONT = "-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif"


def button(theme: dict, icon: str | None, tile: str, caption: str, label: str, soon: bool) -> str:
    glyph = TERMINAL if icon is None else ICONS[icon]
    dash = ' stroke-dasharray="6 4"' if soon else ""
    opacity = ' opacity="0.8"' if soon else ""
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" role="img" aria-label="{escape(caption)} {escape(label)}">
  <rect x="1" y="1" width="{W - 2}" height="{H - 2}" rx="12" fill="{theme['bg']}" stroke="{theme['border']}" stroke-width="1.5"{dash}/>
  <g{opacity}>
    <rect x="14" y="13" width="38" height="38" rx="9" fill="{tile}"/>
    <g transform="translate(22 21) scale(0.9167)"><path fill="#ffffff" d="{glyph}"/></g>
  </g>
  <text x="66" y="27" font-family="{FONT}" font-size="10.5" font-weight="600" letter-spacing="0.6" fill="{theme['caption']}">{escape(caption.upper())}</text>
  <text x="66" y="48" font-family="{FONT}" font-size="19" font-weight="700" fill="{theme['label']}">{escape(label)}</text>
</svg>
"""


for name, theme in THEMES.items():
    out = HERE / name
    out.mkdir(exist_ok=True)
    for key, spec in BUTTONS.items():
        (out / f"{key}.svg").write_text(button(theme, *spec))
print(f"wrote {len(BUTTONS)} buttons x {len(THEMES)} themes to {HERE}")
