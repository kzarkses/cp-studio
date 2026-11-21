# CP Studio

A native REAPER extension, written in Rust.

- `reaper_cp_studio.dll` is a small, fixed loader that REAPER loads at startup.
- `cp_module.dll` holds the actual code and is hot-reloaded: rebuild it and
  REAPER picks it up in about a second, without restarting.

## Build

Requires Rust (stable, MSVC toolchain) on Windows.

    cargo xtask check     # format, clippy, tests
    cargo xtask install   # build and install the loader (REAPER closed)
    cargo xtask module    # rebuild the module; REAPER reloads it

In REAPER: `CP Studio: Show status`, `CP Studio: Reload module now`,
`CP Studio: Open hot-reload test window`.
