# Packaging

How Manfred's releases are packed for Linux, Windows and macOS. `.github/workflows/release.yml` builds every asset on a pushed `v*` tag and publishes them to the tag's GitHub Release with one `SHA256SUMS`. The workflow signs nothing: the Windows setup ships unsigned (there is no Windows certificate), and the macOS app ships unsigned for a signing step outside this repo (pci's Mac lane, for PITO MD's own releases) to sign, notarize and wrap in a dmg.

## What is here

- `launcher/`: the `manfred-desk` crate, the desktop launcher. `manfred` with no argument prints a usage error and `manfred.exe` is a console program, so neither is what a shortcut, Explorer or Finder should open. `manfred-desk <file>` runs `manfred gui <file>` from its own folder. On Windows it is a GUI-subsystem program and starts `manfred.exe` with no console window. On macOS it `exec`s `manfred` with `/opt/homebrew/bin` and `/usr/local/bin` added to the end of `PATH`, since an app opened from Finder gets only the system's `PATH` and Manfred runs optional tools found there. It drops a `-psn_…` argument old macOS versions pass. On macOS its first launch offers once, through `osascript`'s `display dialog`, to install the `manfred` command as a `/usr/local/bin/manfred` link to `Manfred.app/Contents/MacOS/manfred`, made through the standard administrator prompt; the answer is kept in `~/Library/Application Support/Manfred/cli-link`. `manfred-desk --install-cli` and `--remove-cli` do it later, as the window's Help menu does on macOS; they never replace or remove a `/usr/local/bin/manfred` that is not a link into a `Manfred.app`, and the offer is not made while the app runs from a disk image or a translocated folder.
- `windows/manfred.nsi`: the NSIS 3.13 script for `Manfred-vX.Y.Z-Setup.exe`, and `windows/manfred.ico`, its icon.
- `macos/Info.plist.in`: the template for `Manfred.app/Contents/Info.plist`, and `macos/manfred.icns`, its icon.

## The assets of a release

| Asset | Holds |
| --- | --- |
| `manfred-vX.Y.Z-linux-x86_64.tar.gz` | `manfred` at the root, `LICENSE`, `README.md`, `NOTICE`, `licenses/`, `share/` (the `.manf` file type, `manfred.desktop`, the icons) and `bin/register-filetypes` |
| `manfred-vX.Y.Z-windows-x86_64.zip` | `manfred.exe` at the root, `LICENSE`, `README.md`, `NOTICE`, `licenses/` |
| `Manfred-vX.Y.Z-Setup.exe` | the Windows setup, unsigned |
| `manfred-vX.Y.Z-macos-arm64.tar.gz` | `manfred` at the root (Apple Silicon, the linker's ad hoc signature only), `LICENSE`, `README.md`, `NOTICE`, `licenses/` |
| `manfred-vX.Y.Z-macos-arm64.app.zip` | `Manfred.app`, unsigned, zipped with `ditto -c -k --keepParent` |
| `docs-vX.Y.Z.tar.gz` | Manfred's section of docs.pitomd.com |
| `SHA256SUMS` | one `sha256sum` line for each asset above |

## The Windows setup

The setup asks for elevation and installs per machine into `C:\Program Files\Manfred` (`/D=<dir>` overrides it): `manfred.exe`, `manfred-desk.exe`, `manfred.ico`, `uninstall.exe`, and `LICENSE`, `NOTICE` and `licenses\` for the notices that ship with the binaries. It adds a Start-menu shortcut for all users and, as a checked option, a desktop one, both opening `manfred-desk.exe`. It registers `.manf` as `Manfred.Document` (its icon, opened by `manfred-desk.exe "%1"`, content type `application/x-manfred`) appends the install folder to the machine `Path` (`HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment`, `Path`, kept `REG_EXPAND_SZ`) when it is not there yet and broadcasts `WM_SETTINGCHANGE`, and lists Manfred in Apps & features (`HKLM\Software\Microsoft\Windows\CurrentVersion\Uninstall\Manfred`: DisplayName, DisplayVersion, Publisher `PITO MD`, DisplayIcon, InstallLocation, UninstallString, QuietUninstallString, URLInfoAbout `https://pitomd.com`, EstimatedSize, NoModify, NoRepair). The setup and the uninstaller take `/S` for a silent run. Uninstalling removes the files, both shortcuts, the file type (`.manf` only while it still names `Manfred.Document`), exactly that one `Path` entry and the listing; a `Path` too long for NSIS's 1,024-character strings is left as it is, with a line in the setup's details; recipes and `.manf` files stay.

To build it by hand on Windows, from a checkout with the release binaries built:

```text
makensis /V3 /INPUTCHARSET UTF8 /DVERSION=X.Y.Z /DVERSION_NUM=X.Y.Z.0 /DSRC=<folder> /DOUT=<Manfred-vX.Y.Z-Setup.exe> packaging\windows\manfred.nsi
```

where `<folder>` holds `manfred.exe`, `manfred-desk.exe`, `manfred.ico`, `LICENSE`, `NOTICE` and `licenses\`. `VERSION_NUM` is the version without any pre-release part, plus `.0`.

## The macOS app, and how it gets signed

`Manfred.app` as the release ships it:

```text
Manfred.app/Contents/Info.plist
Manfred.app/Contents/PkgInfo                    APPL????
Manfred.app/Contents/MacOS/manfred-desk         CFBundleExecutable, the launcher
Manfred.app/Contents/MacOS/manfred              the real binary, the command line
Manfred.app/Contents/Resources/manfred.icns
Manfred.app/Contents/Resources/LICENSE
Manfred.app/Contents/Resources/NOTICE
Manfred.app/Contents/Resources/licenses/
```

`Info.plist`: `CFBundleIdentifier` `com.pitomd.manfred`, `CFBundleName` Manfred, `CFBundleShortVersionString` the tag's version, `CFBundleVersion` its `X.Y.Z`, `LSMinimumSystemVersion` 13.3 (the build's `MACOSX_DEPLOYMENT_TARGET`), copyright `© PITO MD`, `CFBundleDocumentTypes` for `.manf` (Editor, Owner, `com.pitomd.manfred.document`) and `.glb` (Viewer, Alternate, `org.khronos.glb`), the exported type `com.pitomd.manfred.document` (conforms to `public.data`, extension `manf`, MIME `application/x-manfred`) and an imported `org.khronos.glb` (extension `glb`, MIME `model/gltf-binary`).

A signing lane takes the release as it is:

1. Download `manfred-vX.Y.Z-macos-arm64.app.zip`, `manfred-vX.Y.Z-macos-arm64.tar.gz` and `SHA256SUMS` from the tag's Release, and check both against `SHA256SUMS`.
2. Unpack the app with `ditto -x -k manfred-vX.Y.Z-macos-arm64.app.zip <dir>` (never `unzip`, which can drop the bundle's metadata).
3. Sign from the inside out, each with the Developer ID Application identity, `--options runtime --timestamp` and no entitlements: `Contents/MacOS/manfred`, then `Contents/MacOS/manfred-desk`, then `Manfred.app` itself, whose identifier is the bundle id `com.pitomd.manfred` from its `Info.plist`. Nothing else in the bundle is code. Never `--deep`.
4. `hdiutil create` (HFS+, UDZO) with the app beside an `Applications` link, sign the dmg, notarize it with `xcrun notarytool submit --wait`, then `xcrun stapler staple` the dmg and the app and `stapler validate` both.
5. For a bare signed command line, copy `Contents/MacOS/manfred` out of the signed app (it is the same build as the tarball's); a bare Mach-O can't be stapled, so Gatekeeper checks its notarisation online.

Without a Developer ID the app can be signed ad hoc (`codesign --sign - --force`), in the same order; Gatekeeper then asks the user to allow it in System Settings.
