# Building Super OpenCode installers

Build the installer on the operating system it is intended for. These commands
produce native packages:

| Operating system | Output |
|---|---|
| macOS | `.dmg` application disk image |
| Windows x64 | `.exe` installer |
| Debian/Ubuntu Linux x64 | `.deb` package |

All builds use Rust 1.99.1 and Cargo's locked dependency versions. The project
does not need a `.env` file to build.

First clone the repository and enter it:

```sh
git clone https://github.com/desmondknunoo/super-opencode.git
cd super-opencode
```

## macOS

Prerequisites:

- macOS 12 or later
- Apple Xcode command-line tools
- [Rustup](https://rustup.rs/)
- CMake

Install CMake and the Rust toolchain:

```sh
xcode-select --install
brew install cmake
rustup toolchain install 1.99.1 --profile minimal
```

For Apple silicon:

```sh
rustup target add aarch64-apple-darwin
export MACOSX_DEPLOYMENT_TARGET=12.0
cargo +1.99.1 test --locked
cargo +1.99.1 build --locked --release --target aarch64-apple-darwin
scripts/package-macos.sh \
  target/aarch64-apple-darwin/release/super-opencode \
  dist
hdiutil create \
  -volname "Super OpenCode" \
  -srcfolder "dist/Super OpenCode.app" \
  -ov \
  -format UDZO \
  dist/Super-OpenCode-macos-arm64.dmg
```

For an Intel Mac:

```sh
rustup target add x86_64-apple-darwin
export MACOSX_DEPLOYMENT_TARGET=12.0
cargo +1.99.1 test --locked
cargo +1.99.1 build --locked --release --target x86_64-apple-darwin
scripts/package-macos.sh \
  target/x86_64-apple-darwin/release/super-opencode \
  dist
hdiutil create \
  -volname "Super OpenCode" \
  -srcfolder "dist/Super OpenCode.app" \
  -ov \
  -format UDZO \
  dist/Super-OpenCode-macos-x64.dmg
```

The script uses an ad-hoc code signature, creates
`dist/Super OpenCode.app`, and verifies the app bundle. For distribution outside
a known Apple Developer organization, users may see a Gatekeeper warning unless
you sign and notarize the app with an Apple Developer certificate.

## Windows x64

Prerequisites:

- Windows 10 or later
- Visual Studio 2022 Build Tools with the **Desktop development with C++**
  workload
- [Rustup](https://rustup.rs/)
- [CMake](https://cmake.org/download/)
- [NSIS](https://nsis.sourceforge.io/Download)

Run these commands in **PowerShell**:

```powershell
rustup toolchain install 1.99.1 --profile minimal
rustup target add x86_64-pc-windows-msvc
cargo +1.99.1 test --locked
cargo +1.99.1 build --locked --release --target x86_64-pc-windows-msvc
```

Stage the executable and required WebView2 loader, then build the installer:

```powershell
New-Item -ItemType Directory -Force dist/stage | Out-Null
Copy-Item target/x86_64-pc-windows-msvc/release/super-opencode.exe dist/stage/
Copy-Item README.md dist/stage/

$loader = Get-ChildItem "$env:USERPROFILE\.cargo\registry\src" `
  -Filter WebView2Loader.dll -Recurse |
  Where-Object { $_.FullName -match '[\\/]x64[\\/]WebView2Loader\.dll$' } |
  Select-Object -First 1

if (-not $loader) {
  throw "WebView2Loader.dll (x64) was not found in the Cargo registry"
}
Copy-Item $loader.FullName dist/stage/WebView2Loader.dll

$root = (Get-Location).Path
makensis `
  "/DSTAGE_DIR=$root\dist\stage" `
  "/DOUTFILE=$root\dist\Super-OpenCode-windows-x64.exe" `
  "/DAPP_VERSION=0.2.0" `
  scripts/installer.nsi
```

The installer is written to
`dist/Super-OpenCode-windows-x64.exe`. The installed app is placed in the
current user's `AppData\Local\Programs\Super OpenCode` directory and does not
require administrator privileges.

## Debian or Ubuntu Linux x64

Prerequisites:

- A 64-bit Debian or Ubuntu system
- Rustup
- CMake

Install system packages:

```sh
sudo apt-get update
sudo apt-get install -y \
  build-essential \
  cmake \
  pkg-config \
  libclang-dev \
  libxkbcommon-dev \
  libgl1-mesa-dev \
  libasound2-dev
```

Install Rust and build the package:

```sh
rustup toolchain install 1.99.1 --profile minimal
rustup target add x86_64-unknown-linux-gnu
cargo +1.99.1 test --locked
cargo +1.99.1 build --locked --release \
  --target x86_64-unknown-linux-gnu

scripts/package-linux-deb.sh \
  target/x86_64-unknown-linux-gnu/release/super-opencode \
  0.2.0 \
  dist/Super-OpenCode-linux-x64.deb
```

The package is written to `dist/Super-OpenCode-linux-x64.deb`. Install or update
it with:

```sh
sudo apt install ./dist/Super-OpenCode-linux-x64.deb
```

You can then launch **Super OpenCode** from the application menu or run:

```sh
super-opencode
```

## Sharing the build

Share only the package file from `dist/` and, when practical, its SHA-256
checksum:

```sh
# macOS or Linux
shasum -a 256 dist/Super-OpenCode-macos-arm64.dmg
shasum -a 256 dist/Super-OpenCode-macos-x64.dmg
shasum -a 256 dist/Super-OpenCode-linux-x64.deb

# Windows PowerShell
Get-FileHash dist/Super-OpenCode-windows-x64.exe -Algorithm SHA256
```

Do not share `target/`, `dist/stage/`, local Cargo credentials, or `.env` files.
