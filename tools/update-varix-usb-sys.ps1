# update-varix-usb-sys.ps1 - REAL worker, runs as SYSTEM (has ESP write access).
# v3 (2026-09-26): + forensic dump of limine.conf lineage (base64, encoding-safe).
# Survey -> dump -> backup -> update kernel ELF + limine.conf + boot-select.json on ESP.
$ErrorActionPreference = "Stop"
$repo = "d:\2\14\-Un-Real-0d23d9ux-Engine-main"
$log  = Join-Path $repo "reports\usb-update-log.txt"
$surv = Join-Path $repo "reports\usb-survey.txt"
$dump = Join-Path $repo "reports\usb-dump.txt"
"=== worker run $(Get-Date -Format s) ===" | Out-File $log -Append -Encoding utf8

try {
  $p = Get-Partition -DiskNumber 1 -PartitionNumber 1
  $esp = $null
  foreach ($a in @($p.AccessPaths)) {
    $s = [string]$a
    if ($s.Length -ge 2 -and $s[1] -eq ':') { $esp = $s.Substring(0, 2); break }
  }
  if (-not $esp) { throw "partition 1 has no drive letter path" }
  "ESP root: $esp" | Out-File $log -Append -Encoding utf8

  Get-ChildItem "$esp\" -Recurse -Force -ErrorAction SilentlyContinue |
    Select-Object FullName, Length, LastWriteTime |
    Out-File $surv -Encoding utf8
  "survey written" | Out-File $log -Append -Encoding utf8

  # ---- forensic dump (base64; preserves exact bytes) ----
  $targets = @(
    "$esp\limine.conf",
    "$esp\limine.conf.bak",
    "$esp\limine.conf.bak-20260926-014411",
    "$esp\boot-select.json"
  )
  # SHARED truth source (locate by label on same disk)
  try {
    $shr = Get-Partition -DiskNumber 1 | Where-Object {
      $v = $_ | Get-Volume -ErrorAction SilentlyContinue
      ($null -ne $v) -and ($v.FileSystemLabel -eq 'SHARED')
    } | Select-Object -First 1
    if ($shr) {
      $lp = $null
      foreach ($a in @($shr.AccessPaths)) { $s2=[string]$a; if ($s2.Length -ge 2 -and $s2[1] -eq ':') { $lp = $s2.Substring(0,2); break } }
      if ($lp) { $targets += "$lp\boot-select.json" }
    }
  } catch { "shared locate: $_" | Out-File $log -Append -Encoding utf8 }

  $lines = @("=== usb dump $(Get-Date -Format s) ===")
  foreach ($f in $targets) {
    if (Test-Path $f) {
      $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($f))
      $lines += "===== FILE: $f ($([IO.FileInfo]::new($f).Length) bytes) ====="
      for ($i = 0; $i -lt $b64.Length; $i += 76) {
        $lines += $b64.Substring($i, [Math]::Min(76, $b64.Length - $i))
      }
    } else {
      $lines += "===== FILE: $f (ABSENT) ====="
    }
  }
  [IO.File]::WriteAllLines($dump, $lines)
  "dump written ($($targets.Count) targets)" | Out-File $log -Append -Encoding utf8

  # ---- backup ----
  $kpath = Join-Path $esp "kernel\varix"
  $stamp = Get-Date -Format "yyyyMMdd-HHmmss"
  if (Test-Path $kpath) {
    Copy-Item $kpath "$kpath.bak-$stamp" -Force
    "backup: $kpath -> $kpath.bak-$stamp" | Out-File $log -Append -Encoding utf8
  } else {
    New-Item -ItemType Directory -Force -Path (Join-Path $esp "kernel") | Out-Null
    "WARN: kernel file was missing (created fresh)" | Out-File $log -Append -Encoding utf8
  }

  # ---- kernel ----
  $elf = Join-Path $repo "kernel\target\x86_64-unknown-none\release\varix"
  $srcHash = (Get-FileHash $elf -Algorithm SHA256).Hash
  Copy-Item $elf $kpath -Force
  $dstHash = (Get-FileHash $kpath -Algorithm SHA256).Hash
  if ($srcHash -ne $dstHash) { throw "kernel copy hash mismatch" }
  $ki = Get-Item $kpath
  "kernel updated+verified: $($ki.FullName) $($ki.Length) bytes" | Out-File $log -Append -Encoding utf8

  # ---- limine.conf ----
  $csrc = Join-Path $repo "limine.conf"
  $cdst = Join-Path $esp "limine.conf"
  if (Test-Path $cdst) {
    $old = (Get-FileHash $cdst -Algorithm MD5).Hash
    $new = (Get-FileHash $csrc -Algorithm MD5).Hash
    "conf old=$old new=$new" | Out-File $log -Append -Encoding utf8
    if ($old -ne $new) {
      Copy-Item $cdst "$cdst.bak-$stamp" -Force
      Copy-Item $csrc $cdst -Force
      $cpHash = (Get-FileHash $cdst -Algorithm MD5).Hash
      if ($cpHash -ne $new) { throw "conf copy hash mismatch" }
      "conf updated (backup made, hash verified)" | Out-File $log -Append -Encoding utf8
    } else {
      "conf identical - skipped" | Out-File $log -Append -Encoding utf8
    }
  } else {
    Copy-Item $csrc $cdst -Force
    "conf created (was missing)" | Out-File $log -Append -Encoding utf8
  }

  # ---- boot-select.json ----
  $bsrc = Join-Path $repo "boot-select.json"
  $bdst = Join-Path $esp "boot-select.json"
  if (Test-Path $bdst) {
    $ob = (Get-FileHash $bdst -Algorithm MD5).Hash
    $nb = (Get-FileHash $bsrc -Algorithm MD5).Hash
    if ($ob -ne $nb) {
      Copy-Item $bdst "$bdst.bak-$stamp" -Force
      Copy-Item $bsrc $bdst -Force
      "boot-select.json updated (backup made)" | Out-File $log -Append -Encoding utf8
    } else {
      "boot-select.json identical - skipped" | Out-File $log -Append -Encoding utf8
    }
  } else {
    Copy-Item $bsrc $bdst -Force
    "boot-select.json created (was missing)" | Out-File $log -Append -Encoding utf8
  }

  "=== DONE OK ===" | Out-File $log -Append -Encoding utf8
} catch {
  "ERROR: $_" | Out-File $log -Append -Encoding utf8
  "=== DONE FAIL ===" | Out-File $log -Append -Encoding utf8
}
