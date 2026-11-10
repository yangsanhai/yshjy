# 杨三海解压 · 安卓一键出包
#
# 用法（在仓库根目录）：
#   pwsh -File scripts\build-android.ps1                  # 出 arm64 的 release APK
#   pwsh -File scripts\build-android.ps1 -Debug           # 快一点，调界面时用
#   pwsh -File scripts\build-android.ps1 -SkipEngine      # 引擎没改，只重打 APK
#
# 依赖（批 1 装过，见 docs/ANDROID-整体方案-2026-11-10.md §8）：
#   Android SDK + ndk;28.2.13676358 + rustup 目标 aarch64-linux-android
#   **不需要 cargo-ndk** —— 它只是帮忙设 CC/AR/linker 环境变量的封装，这里直接设。
#
# 产物：
#   gui\android\app\src\main\jniLibs\<abi>\libyshjy.so   引擎（从 ffi crate 编出来）
#   gui\build\app\outputs\flutter-apk\app-*.apk          安装包

param(
  [string]$Abi = 'arm64-v8a',
  [switch]$SkipEngine,
  [switch]$Debug
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# ABI → (rust 目标, clang 前缀)
$abiMap = @{
  'arm64-v8a'   = @{ Rust = 'aarch64-linux-android'; Clang = 'aarch64-linux-android'; Env = 'aarch64_linux_android' }
  'armeabi-v7a' = @{ Rust = 'armv7-linux-androideabi'; Clang = 'armv7a-linux-androideabi'; Env = 'armv7_linux_androideabi' }
  'x86_64'      = @{ Rust = 'x86_64-linux-android'; Clang = 'x86_64-linux-android'; Env = 'x86_64_linux_android' }
}
if (-not $abiMap.ContainsKey($Abi)) { throw "不认识的 ABI：$Abi（可选 arm64-v8a / armeabi-v7a / x86_64）" }
$m = $abiMap[$Abi]

# Android 最低 API（与 build.gradle.kts 的 flutter.minSdkVersion 一致 = 24）
$apiLevel = 24

# ── 0. 找 SDK / NDK ─────────────────────────────────────────────────
$sdk = if ($env:ANDROID_HOME) { $env:ANDROID_HOME } else { Join-Path $env:LOCALAPPDATA 'Android\Sdk' }
if (-not (Test-Path $sdk)) { throw "找不到 Android SDK：$sdk（先跑批 1 的安装脚本）" }

$ndkDir = Get-ChildItem (Join-Path $sdk 'ndk') -Directory -ErrorAction SilentlyContinue |
  Sort-Object Name -Descending | Select-Object -First 1
if (-not $ndkDir) { throw "SDK 里没有 NDK：$sdk\ndk" }
$env:ANDROID_NDK_HOME = $ndkDir.FullName
$env:ANDROID_NDK_ROOT = $ndkDir.FullName
$env:ANDROID_HOME = $sdk
$env:ANDROID_SDK_ROOT = $sdk
Write-Host "== NDK $($ndkDir.Name) / ABI $Abi ==" -ForegroundColor Cyan

$llvmBin = Join-Path $ndkDir.FullName 'toolchains\llvm\prebuilt\windows-x86_64\bin'
$clang = Join-Path $llvmBin "$($m.Clang)$apiLevel-clang.cmd"
if (-not (Test-Path $clang)) { throw "找不到 NDK clang：$clang" }

$jniLibs = Join-Path $root 'gui\android\app\src\main\jniLibs'

# ── 1. 编引擎（ffi crate → libyshjy.so）────────────────────────────
if (-not $SkipEngine) {
  Write-Host "== 编引擎（$($m.Rust)）==" -ForegroundColor Cyan
  # Rust 交叉编译靠这几个环境变量找 NDK 工具链（cargo-ndk 干的就是这件事）
  Set-Item -Path "env:CC_$($m.Env)" -Value $clang
  Set-Item -Path "env:CXX_$($m.Env)" -Value (Join-Path $llvmBin "$($m.Clang)$apiLevel-clang++.cmd")
  Set-Item -Path "env:AR_$($m.Env)" -Value (Join-Path $llvmBin 'llvm-ar.exe')
  Set-Item -Path "env:CARGO_TARGET_$($m.Env.ToUpper())_LINKER" -Value $clang

  & cargo build --manifest-path ffi\Cargo.toml --release --target $m.Rust
  if ($LASTEXITCODE -ne 0) { throw 'cargo 交叉编译失败' }
}
$built = Join-Path $root "ffi\target\$($m.Rust)\release"
$soSrc = Get-ChildItem $built -Filter 'libyshjy_ffi.so' -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $soSrc) { $soSrc = Get-ChildItem $built -Filter '*.so' -ErrorAction SilentlyContinue | Select-Object -First 1 }
if (-not $soSrc) { throw "找不到编出来的 .so：$built" }

$abiDir = Join-Path $jniLibs $Abi
New-Item -ItemType Directory -Path $abiDir -Force | Out-Null
$so = Join-Path $abiDir 'libyshjy.so'
Copy-Item $soSrc.FullName $so -Force
$before = (Get-Item $so).Length

# 去符号表缩小体积（导出符号保留 —— 少了它 Dart 那边就找不到函数了）
$strip = Join-Path $llvmBin 'llvm-strip.exe'
if (Test-Path $strip) { & $strip --strip-unneeded $so }
Write-Host ("   libyshjy.so {0:N2} MB → {1:N2} MB" -f ($before / 1MB), ((Get-Item $so).Length / 1MB))

# ── 2. 校验：9 个 ABI 符号 + 16 KB 对齐 ─────────────────────────────
$nm = Join-Path $llvmBin 'llvm-nm.exe'
$readelf = Join-Path $llvmBin 'llvm-readelf.exe'
if ((Test-Path $nm) -and (Test-Path $readelf)) {
  $syms = & $nm -D --defined-only $so | Select-String 'yshjy_'
  Write-Host "   导出符号 $($syms.Count)/9" -ForegroundColor $(if ($syms.Count -eq 9) { 'Green' } else { 'Red' })
  if ($syms.Count -ne 9) { throw "ABI 符号不全（$($syms.Count)/9），装到手机上会 dlopen 失败" }

  $loads = & $readelf -l $so | Select-String 'LOAD'
  $bad = $loads | Where-Object { $_ -notmatch '0x4000|0x10000' }
  if ($bad) {
    Write-Host '  ⚠ 有 LOAD 段不是 16 KB 对齐（Android 15+ 会拒装）' -ForegroundColor Yellow
    $loads | ForEach-Object { "     $_" }
  } else {
    Write-Host '   16 KB 页对齐：OK' -ForegroundColor Green
  }
}

# ── 3. 打 APK ───────────────────────────────────────────────────────
Write-Host '== 打 APK ==' -ForegroundColor Cyan
Push-Location (Join-Path $root 'gui')
try {
  $apkArgs = @('build', 'apk')
  if ($Debug) { $apkArgs += '--debug' } else { $apkArgs += '--release' }
  & flutter @apkArgs
  if ($LASTEXITCODE -ne 0) { throw 'flutter build apk 失败' }
} finally { Pop-Location }

# ── 4. 汇总 ─────────────────────────────────────────────────────────
$apkDir = Join-Path $root 'gui\build\app\outputs\flutter-apk'
Write-Host ''
Write-Host '== 完成 ==' -ForegroundColor Green
Get-ChildItem $apkDir -Filter *.apk -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending |
  Select-Object Name, @{n = '大小(MB)'; e = { [math]::Round($_.Length / 1MB, 1) } }, LastWriteTime |
  Format-Table -AutoSize
