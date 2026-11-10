# 杨三海解压 · Windows 一键出包
#
# 用法（在仓库根目录，或在任意目录用全路径）：
#   pwsh -File scripts\build-win.ps1
#   pwsh -File scripts\build-win.ps1 -SkipBuild      # 只重新打包（产物已在）
#
# 产物（dist-win\）：
#   yshjy-<版本>-win-x64\        绿色版目录
#   yshjy-<版本>-win-x64-cli\    命令行版目录
#   yshjy-<版本>-win-x64.zip     绿色版压缩包
#   yshjy-<版本>-win-x64-cli.zip 命令行压缩包
#   yshjy-<版本>-setup.exe       安装包（Inno Setup）
#   SHA256SUMS.txt               校验清单
#
# 为什么把 VC++ 运行库塞进包里：干净 Windows 没装过 VS，双击会报
# 「丢失 MSVCP140.dll」。见 docs/WIN-PACKAGING-FIX.md 第 1 条。

param(
  # 引擎 target 目录（默认仓库里的 target；嫌慢可以指到别的盘）
  [string]$TargetDir = "",
  [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# ── 版本号以 Cargo.toml 为准，别在脚本里写死 ───────────────────────────
$cargoToml = Get-Content (Join-Path $root 'Cargo.toml') -Raw
if ($cargoToml -notmatch '(?ms)^\[package\].*?^version\s*=\s*"([^"]+)"') {
  throw 'Cargo.toml 里没找到 package.version'
}
$version = $Matches[1]
Write-Host "== 版本 $version ==" -ForegroundColor Cyan

if ($TargetDir) { $env:CARGO_TARGET_DIR = $TargetDir }
$targetRoot = if ($TargetDir) { $TargetDir } else { Join-Path $root 'target' }

# ── 1. 编引擎（常用版 + 全能版）─────────────────────────────────────
if (-not $SkipBuild) {
  $env:CARGO_HTTP_PROXY = ''
  $env:HTTP_PROXY = ''
  $env:HTTPS_PROXY = ''

  Write-Host '== 编引擎（常用版）==' -ForegroundColor Cyan
  & cargo build --release
  if ($LASTEXITCODE -ne 0) { throw 'cargo build（常用版）失败' }
  $builtExe = Join-Path $targetRoot 'release\yshjy.exe'
  if (-not (Test-Path $builtExe)) { throw "找不到 $builtExe" }

  # 全能版会覆盖同一个 target/release/yshjy.exe：先把常用版挪到临时文件，
  # 不然后面两个包都会变成全能版（11-05 那版就是这么丢的）。
  $commonExe = Join-Path $env:TEMP "yshjy-common-$PID.exe"
  Copy-Item $builtExe $commonExe -Force

  Write-Host '== 编引擎（全能版）==' -ForegroundColor Cyan
  & cargo build --release --features full
  if ($LASTEXITCODE -ne 0) { throw 'cargo build（全能版）失败' }
  $fullExe = Join-Path $targetRoot 'release\yshjy.exe'
} else {
  $commonExe = Join-Path $targetRoot 'release\yshjy.exe'
  $fullExe = Join-Path $targetRoot 'release\yshjy-full.exe'
  if (-not (Test-Path $fullExe)) { $fullExe = $commonExe }
}

# ── 2. 编界面 ────────────────────────────────────────────────────────
$guiRel = Join-Path $root 'gui\build\windows\x64\runner\Release'
if (-not $SkipBuild) {
  Write-Host '== 编界面（Flutter Windows release）==' -ForegroundColor Cyan
  Push-Location (Join-Path $root 'gui')
  try {
    & flutter build windows --release
    if ($LASTEXITCODE -ne 0) { throw 'flutter build windows 失败' }
  } finally { Pop-Location }
}
if (-not (Test-Path (Join-Path $guiRel 'yshjy_gui.exe'))) {
  throw "找不到界面产物：$guiRel\yshjy_gui.exe"
}

# ── 3. 摊目录 ────────────────────────────────────────────────────────
$dist = Join-Path $root 'dist-win'
$stage = Join-Path $dist "yshjy-$version-win-x64"
$cliStage = Join-Path $dist "yshjy-$version-win-x64-cli"

foreach ($dir in @($stage, $cliStage)) {
  if (Test-Path $dir) {
    # 只删 dist-win 下的生成目录（路径写死在本脚本里，不接受外部拼接）
    $resolved = (Resolve-Path $dir).Path
    if ($resolved -notlike (Join-Path $dist '*')) {
      throw "拒绝删除非 dist-win 目录：$resolved"
    }
    Remove-Item -LiteralPath $resolved -Recurse -Force
  }
  New-Item -ItemType Directory -Path $dir -Force | Out-Null
}

Write-Host '== 摊绿色版目录 ==' -ForegroundColor Cyan
Copy-Item (Join-Path $guiRel '*') $stage -Recurse -Force
Copy-Item $commonExe (Join-Path $stage 'yshjy.exe') -Force
if ($fullExe -ne $commonExe) {
  Copy-Item $fullExe (Join-Path $stage 'yshjy-full.exe') -Force
}

# VC++ 运行库：必须是 64 位（本进程是 64 位时 System32 就是 64 位）
$runtimeDlls = @('msvcp140.dll', 'vcruntime140.dll', 'vcruntime140_1.dll')
foreach ($dll in $runtimeDlls) {
  $src = Join-Path $env:WINDIR "System32\$dll"
  if (-not (Test-Path $src)) { throw "找不到运行库：$src" }
  Copy-Item $src (Join-Path $stage $dll) -Force
}

# 文档
$docs = @('README.md', 'CHANGELOG.md', 'LICENSE')
foreach ($d in $docs) {
  $p = Join-Path $root $d
  if (Test-Path $p) { Copy-Item $p $stage -Force }
}
Copy-Item (Join-Path $root 'docs\Windows-使用说明.txt') $stage -Force

Write-Host '== 摊命令行版目录 ==' -ForegroundColor Cyan
Copy-Item $commonExe (Join-Path $cliStage 'yshjy.exe') -Force
if ($fullExe -ne $commonExe) {
  Copy-Item $fullExe (Join-Path $cliStage 'yshjy-full.exe') -Force
}
Copy-Item (Join-Path $env:WINDIR 'System32\vcruntime140.dll') $cliStage -Force
foreach ($d in $docs) {
  $p = Join-Path $root $d
  if (Test-Path $p) { Copy-Item $p $cliStage -Force }
}
Copy-Item (Join-Path $root 'docs\Windows-使用说明.txt') $cliStage -Force

# ── 4. 打 zip ───────────────────────────────────────────────────────
Write-Host '== 打 zip ==' -ForegroundColor Cyan
$zipGui = Join-Path $dist "yshjy-$version-win-x64.zip"
$zipCli = Join-Path $dist "yshjy-$version-win-x64-cli.zip"
foreach ($z in @($zipGui, $zipCli)) { if (Test-Path $z) { Remove-Item -LiteralPath $z -Force } }

$sevenZip = Get-Command 7z -ErrorAction SilentlyContinue
if ($sevenZip) {
  Push-Location $dist
  try {
    & $sevenZip.Source a -tzip -mx=9 -y $zipGui "yshjy-$version-win-x64\*" | Out-Null
    & $sevenZip.Source a -tzip -mx=9 -y $zipCli "yshjy-$version-win-x64-cli\*" | Out-Null
  } finally { Pop-Location }
} else {
  Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zipGui -CompressionLevel Optimal
  Compress-Archive -Path (Join-Path $cliStage '*') -DestinationPath $zipCli -CompressionLevel Optimal
}

# ── 5. 安装包 ───────────────────────────────────────────────────────
Write-Host '== 打安装包（Inno Setup）==' -ForegroundColor Cyan
$iscc = Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'
if (-not (Test-Path $iscc)) { throw "找不到 Inno Setup：$iscc" }
$setup = Join-Path $dist "yshjy-$version-setup.exe"
& $iscc "/DAppVersion=$version" (Join-Path $root 'scripts\yshjy-setup.iss')
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $setup)) {
  # 仓库在移动硬盘（exFAT）上时，Inno 最后一步「更新 Setup.exe 的资源」会报
  # Resource update error: EndUpdateResource failed (110) —— 文件打不开，
  # 杀软实时扫描 / 移动盘都可能触发。这不是源码问题，退一步就好：
  # 先编到本机临时目录，成功后再把 exe 拷回 dist-win。
  Write-Host '  直接输出失败，改在本机临时目录编译后拷回…' -ForegroundColor Yellow
  $localOut = Join-Path $env:TEMP "yshjy-iscc-$PID"
  New-Item -ItemType Directory -Path $localOut -Force | Out-Null
  & $iscc "/DAppVersion=$version" "/O$localOut" (Join-Path $root 'scripts\yshjy-setup.iss')
  if ($LASTEXITCODE -ne 0) { throw 'Inno Setup 编译失败（本机临时目录同样失败）' }
  Copy-Item (Join-Path $localOut "yshjy-$version-setup.exe") $setup -Force
  Remove-Item -LiteralPath $localOut -Recurse -Force -ErrorAction SilentlyContinue
}

# ── 6. 校验清单 ─────────────────────────────────────────────────────
Write-Host '== 生成 SHA256SUMS.txt ==' -ForegroundColor Cyan
$setup = Join-Path $dist "yshjy-$version-setup.exe"
$files = @($setup, $zipGui, $zipCli) | Where-Object { Test-Path $_ }
$lines = foreach ($f in $files) {
  $h = (Get-FileHash -Algorithm SHA256 -LiteralPath $f).Hash.ToLower()
  "$h  $(Split-Path -Leaf $f)"
}
$lines | Set-Content -Path (Join-Path $dist 'SHA256SUMS.txt') -Encoding ascii

# ── 7. 汇总 ─────────────────────────────────────────────────────────
if (-not $SkipBuild -and (Test-Path $commonExe) -and
    $commonExe.StartsWith($env:TEMP, [System.StringComparison]::OrdinalIgnoreCase)) {
  Remove-Item -LiteralPath $commonExe -Force -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host "== 完成（$version）==" -ForegroundColor Green
Get-ChildItem $dist -File | Where-Object { $_.Name -match "^yshjy-$([regex]::Escape($version))" -or $_.Name -eq 'SHA256SUMS.txt' } |
  Select-Object Name, @{n = '大小(MB)'; e = { [math]::Round($_.Length / 1MB, 1) } } |
  Format-Table -AutoSize
