#!/usr/bin/env python3
"""Build and install Luminal and its desktop launcher for the current user."""

import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile


def atomic_install(source: Path, destination: Path, mode: int) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f".{destination.name}-", dir=destination.parent)
    os.close(fd)
    try:
        shutil.copyfile(source, temporary)
        os.chmod(temporary, mode)
        os.replace(temporary, destination)
    finally:
        Path(temporary).unlink(missing_ok=True)


def desktop_string(value: str) -> str:
    return value.replace("\\", "\\\\").replace("\n", "\\n").replace("\r", "\\r").replace("\t", "\\t")


def desktop_exec(path: Path) -> str:
    # Exec has its own quoting rules, in addition to desktop-entry escaping.
    value = str(path).replace("%", "%%")
    for character in ('\\', '"', '`', '$'):
        value = value.replace(character, '\\' + character)
    return desktop_string('"' + value + '"')


def main() -> None:
    if os.geteuid() == 0:
        raise SystemExit("Run this installer as your desktop user, without sudo.")
    if not shutil.which("cargo"):
        raise SystemExit("Cargo is missing. See docs/OMARCHY.md for prerequisites.")
    repo = Path(__file__).resolve().parent.parent
    data = Path(os.environ.get("XDG_DATA_HOME") or Path.home() / ".local/share").expanduser()
    if not data.is_absolute():
        raise SystemExit("XDG_DATA_HOME must be an absolute path.")
    install_dir = data / "luminal"
    launcher = Path.home() / ".local/bin/luminal"
    desktop = data / "applications/luminal.desktop"
    icon = install_dir / "luminal.png"

    subprocess.run(["cargo", "build", "--locked", "--release", "-p", "luminal-app"], cwd=repo, check=True)
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=repo, text=True))
    binary = install_dir / "bin/luminal-app"
    atomic_install(Path(metadata["target_directory"]) / "release/luminal-app", binary, 0o755)
    atomic_install(repo / "assets/icons/luminal.png", icon, 0o644)

    # Launch independently of the checkout, with writable per-user game logs.
    with tempfile.TemporaryDirectory(prefix="luminal-install-") as directory:
        staging = Path(directory)
        wrapper = staging / "luminal"
        wrapper.write_text(
            '#!/bin/sh\nset -eu\n'
            'state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/luminal"\n'
            'mkdir -p "$state_dir"\ncd "$state_dir"\n'
            f'set -- {shlex.quote(str(binary))} "$@"\n'
            'for name in DISPLAY WAYLAND_DISPLAY XDG_RUNTIME_DIR LUMINAL_SEED; do\n'
            '  if printenv "$name" >/dev/null; then set -- "--setenv=$name" "$@"; fi\n'
            'done\n'
            'exec systemd-run --user --collect --quiet --property=Type=exec \\\n'
            '  --description=Luminal --working-directory="$state_dir" \\\n'
            '  --property="StandardOutput=append:$state_dir/launcher.log" \\\n'
            '  --property="StandardError=append:$state_dir/launcher.log" "$@"\n')
        atomic_install(wrapper, launcher, 0o755)
        entry = staging / "luminal.desktop"
        entry.write_text(
            "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Luminal\n"
            "Comment=Command a frigate in light-delayed space combat\n"
            f"Exec={desktop_exec(launcher)}\nIcon={desktop_string(str(icon))}\n"
            "Terminal=false\nCategories=Game;StrategyGame;\n"
            "Keywords=space;combat;strategy;frigate;\nStartupWMClass=luminal\n")
        if shutil.which("desktop-file-validate"):
            subprocess.run(["desktop-file-validate", str(entry)], check=True)
        atomic_install(entry, desktop, 0o644)
    if shutil.which("update-desktop-database"):
        subprocess.run(["update-desktop-database", str(desktop.parent)], check=True)
    print(f"Installed {binary}\nLauncher: {desktop}\nIcon: {icon}")
    print("Open the Omarchy application launcher and search for Luminal.")
    print(f"Or run {launcher}. Re-run this installer after pulling updates.")


if __name__ == "__main__":
    main()
