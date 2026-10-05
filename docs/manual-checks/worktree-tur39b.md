# Manual checks: tur39b

TUR-39b: release.yml Linux legs with PipeWire. Both legs (deb and AppImage on
ubuntu-24.04) were built by a temporary CI proof job, run
https://github.com/shantanujumde/meet-ai-app/actions/runs/37271867700; the
AppImage had no libpipewire/libspa inside. Nothing was run on a Linux desktop.

## Run by hand

1. On a clean Ubuntu 24.04 desktop, `sudo apt install ./meet-ai-X.Y.Z-linux-amd64.deb`
   from a real release, launch meet-ai and record a short call.
   Expect: apt pulls `libpipewire-0.3-0t64`; the app starts and records.
   Why skipped: needs a real release run and a Linux desktop with audio.
2. On the same machine, `chmod +x` the AppImage and run it; record again.
   Then `ldd` on the extracted `usr/bin/meet-ai` (`--appimage-extract`).
   Expect: libpipewire resolves to `/usr/lib/x86_64-linux-gnu/`, recording works.
   Why skipped: same; whether the system PipeWire works with this build must be
   measured, not assumed.
3. On Ubuntu 22.04, try to install the .deb.
   Expect: apt refuses (glibc/libpipewire too old); release notes say 24.04 or later.
   Why skipped: needs a 22.04 machine.
