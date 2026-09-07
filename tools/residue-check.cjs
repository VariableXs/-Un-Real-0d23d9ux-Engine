#!/usr/bin/env node
//! E-4（规格 16.4 / 附录 1121 行）：零残留验收工具。
//!
//! 用法：
//!   node tools/residue-check.cjs                 # 报告模式：列出发现的残留
//!   node tools/residue-check.cjs --expect-clean  # 断言模式：有残留则以退出码 1 失败
//!   node tools/residue-check.cjs --json          # 机器可读输出（归档用）
//!   node tools/residue-check.cjs --out <file>    # 同时把报告写入文件（证据归档）
//!
//! 扫描面（与引擎 E-2 残留扫描器同口径）：
//!   - %USERPROFILE% 顶层 *variable* 命名的文件/目录
//!   - %APPDATA% / %LOCALAPPDATA% 顶层 *variable* 命名的目录
//!   - %LOCALAPPDATA%\Temp 顶层 *variable* 命名的文件
//!   - HKCU\Software 下 *variable* 命名的键（reg query，只读）
//!   - %APPDATA%\Microsoft\Windows\Recent 指向 Variable 的 .lnk
//!
//! 白名单：已知无害项（thumbcache / iconcache 等）不报。退出码：
//!   0 = 干净（或仅白名单项）；1 = 有残留；2 = 运行环境异常。

"use strict";
const fs = require("fs");
const os = require("os");
const path = require("path");
const { execFileSync } = require("child_process");

const args = process.argv.slice(2);
const expectClean = args.includes("--expect-clean");
const asJson = args.includes("--json");
const outIdx = args.indexOf("--out");
const outFile = outIdx >= 0 ? args[outIdx + 1] : null;

const WHITELIST = ["thumbcache", "iconcache", "recentcustomitems", "usbstor"];

function whitelisted(name) {
  const lower = name.toLowerCase();
  return WHITELIST.some((w) => lower.includes(w));
}

const findings = [];

function add(kind, where, detail) {
  findings.push({ kind, where, detail });
}

function scanDirTop(dir, label, recursive = false, depth = 0) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return; // 目录不存在 = 无残留
  }
  for (const e of entries) {
    if (!/variable/i.test(e.name)) continue;
    if (whitelisted(e.name)) continue;
    add("file", label, path.join(dir, e.name) + (e.isDirectory() ? "/" : ""));
    if (recursive && e.isDirectory() && depth < 1) {
      scanDirTop(path.join(dir, e.name), label, recursive, depth + 1);
    }
  }
}

function scanRegistry() {
  let out = "";
  try {
    out = execFileSync("reg", ["query", "HKCU\\Software"], { encoding: "utf8" });
  } catch (e) {
    add("error", "registry", `reg query 失败: ${e.message}`);
    return;
  }
  for (const line of out.split(/\r?\n/)) {
    const m = line.match(/HKCU\\Software\\(\S+)/i);
    if (!m) continue;
    const sub = m[1];
    // 只看顶层键（无更多反斜杠）
    if (sub.includes("\\")) continue;
    if (!/variable/i.test(sub)) continue;
    if (whitelisted(sub)) continue;
    add("reg", "HKCU\\Software", `HKCU\\Software\\${sub}`);
  }
}

function scanRecent() {
  const recent = path.join(process.env.APPDATA || "", "Microsoft", "Windows", "Recent");
  let entries;
  try {
    entries = fs.readdirSync(recent);
  } catch {
    return;
  }
  for (const name of entries) {
    if (/variable/i.test(name) && name.toLowerCase().endsWith(".lnk")) {
      add("file", "Recent", path.join(recent, name));
    }
  }
}

// ---- 执行扫描 ----
const home = os.homedir();
const appdata = process.env.APPDATA || path.join(home, "AppData", "Roaming");
const local = process.env.LOCALAPPDATA || path.join(home, "AppData", "Local");

scanDirTop(home, "USERPROFILE-top");
scanDirTop(appdata, "APPDATA-top");
scanDirTop(local, "LOCALAPPDATA-top");
scanDirTop(path.join(local, "Temp"), "LOCALAPPDATA-Temp");
scanRegistry();
scanRecent();

const errors = findings.filter((f) => f.kind === "error");
const residues = findings.filter((f) => f.kind !== "error");

// ---- 输出 ----
function reportText() {
  const lines = [];
  lines.push(`# residue-check 报告  ${new Date().toISOString()}`);
  lines.push(`宿主: ${os.hostname()}  用户: ${os.userInfo().username}`);
  lines.push("");
  if (errors.length) {
    lines.push("[运行异常]");
    for (const e of errors) lines.push(`  ${e.where}: ${e.detail}`);
    lines.push("");
  }
  if (residues.length === 0) {
    lines.push("零残留 ✅（宿主观测面未发现 Variable 痕迹）");
  } else {
    lines.push(`发现 ${residues.length} 项残留:`);
    for (const r of residues) lines.push(`  [${r.kind}] ${r.where}: ${r.detail}`);
  }
  return lines.join("\n");
}

const text = reportText();
if (asJson) {
  console.log(JSON.stringify({ clean: residues.length === 0, residues, errors }, null, 2));
} else {
  console.log(text);
}
if (outFile) {
  fs.mkdirSync(path.dirname(path.resolve(outFile)), { recursive: true });
  fs.writeFileSync(outFile, text + "\n", "utf8");
}

// ---- 退出码 ----
if (errors.length && !residues.length) process.exit(2);
if (residues.length > 0) {
  process.exit(expectClean ? 1 : 0);
}
process.exit(0);
