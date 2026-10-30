# rust_to_e

it installs Rust (MSVC toolchain) + Build Tools on whatever drive you pick. Windows only

## how it works

you type a drive letter. Enter = C, and then it's just a normal install
Any other letter, and it makes junctions (`mklink /J`) from the usual places on C: to folders on that drive, so Windows and the installers think everything is still on C:. Stuff like `.cargo`, `.rustup`, `Windows Kits`, `Package Cache` and so on. Then it downloads and installs Build Tools and rustup.
If one of those folders already exists on C: and isn't empty, it asks what to do: move it to the other drive (default), delete it (you have to type YES), or skip it

## usage

1. grab `rust_to_e.exe` from [Releases](../../releases/latest)
2. run it **as administrator** (right click -> Run as administrator)
3. pick a drive, wait. Build Tools take a while, it's a few GB
4. restart your terminal when it's done

Or build it yourself:

```
cargo build --release
```

## things to know

- the junctions point to the other drive. If that drive is disconnected or changes its letter, Rust and MSVC stop working
- "Delete" really deletes. If you have a full Visual Studio installed, don't pick it for `Microsoft Visual Studio`
- some small stuff still ends up on C: (shared components, registry, system runtimes), but it's tens of MB, not GBs
- VS version (2022) and Windows SDK (22621) are hardcoded, if Microsoft changes something it may break
- the exe isn't signed, so SmartScreen will complain. More info -> Run anyway

no warranty, use at your own risk