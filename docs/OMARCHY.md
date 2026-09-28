# Install on Omarchy

Luminal runs natively on Omarchy's Wayland desktop and starts fullscreen.
The installer builds a release executable and installs it for your user, with
a generated icon and a searchable **Luminal** application launcher in the Games
category. It does not change Hyprland settings.

Install the build dependencies from a terminal:

```sh
sudo pacman -S --needed git base-devel rust pkgconf python alsa-lib libx11 libxi \
  libxcursor libxrandr libxinerama libxkbcommon wayland mesa desktop-file-utils
```

Use Rust 1.98 or newer. If you already manage Rust with rustup, keep that
installation instead of installing Arch's `rust` package, and select a compatible
toolchain before running the installer.

```sh
git clone https://github.com/gersham/luminal-game.git
cd luminal-game
./scripts/install-omarchy.py
```

Run the installer as your normal desktop user, without `sudo`. The first build
downloads dependencies and can take a few minutes. No image-generation service
or credentials are needed: the generated icon is included in the repository.

Open Omarchy's Apps menu (**Super+Alt+Space** with the current default bindings),
search for **Luminal**, and launch it. You can also run `~/.local/bin/luminal`.
Close the window with your normal window-close binding. The running game keeps
its current build until you close and reopen it.

To update an existing installation:

```sh
git pull --ff-only
./scripts/install-omarchy.py
```

The installer replaces its own files and can be run repeatedly. It copies the
executable, so launching the installed game does not depend on the checkout or
its `target` directory. Audio and the window icon are embedded in the executable.

With default XDG paths, installed files are:

- `~/.local/bin/luminal`: launch command.
- `~/.local/share/luminal/`: executable and launcher icon.
- `~/.local/share/applications/luminal.desktop`: application entry.
- `~/.local/state/luminal/logs/latest.log`: runtime diagnostics; replaced each launch.

`XDG_DATA_HOME` and `XDG_STATE_HOME` are respected when set. The launcher command
always lives in `~/.local/bin`. To uninstall, remove the command, desktop entry,
and `luminal` installation directory above; remove the state directory if you
also want to discard logs.
