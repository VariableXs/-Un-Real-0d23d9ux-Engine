#!/usr/bin/env node
/**
 * X-6：uxpack 打包工具 —— 把含 `.uxpack.json` 的目录打成 `.uxpack` 分发容器。
 *
 * 用法：
 *   node tools/uxpack.cjs pack <ext-dir> [-o out.uxpack]
 *   node tools/uxpack.cjs verify <pack.uxpack>
 *
 * 容器格式（与 Rust 侧 shell/extensions.rs parse_envelope 对齐）：
 *   { "uxpack": 1, "manifest": {...}, "files": { "<相对路径>": "<base64>" } }
 * V1 边界：未压缩；signature 字段随包透传，验签执行属后续批。
 */

"use strict";

const fs = require("fs");
const path = require("path");

const UXPACK_VERSION = 1;
const ALLOWED_ROOTS = [
  "widget", "window", "events", "storage", "vault",
  "net", "notify", "layout", "hardware", "theme", "aihub",
];

function validateManifest(m) {
  const errs = [];
  if (!m || typeof m !== "object") return ["manifest 缺失"];
  if (!m.id || !/^[a-z0-9-]+$/.test(m.id)) errs.push("id 只允许小写字母/数字/连字符");
  if (!m.name) errs.push("name 不能为空");
  if (!m.version) errs.push("version 不能为空");
  if (!["web", "plugin", "external"].includes(m.type)) errs.push("type 必须是 web / plugin / external");
  if (!m.entry) errs.push("entry 不能为空");
  for (const p of m.permissions ?? []) {
    if (!ALLOWED_ROOTS.includes(String(p).split(":")[0])) errs.push(`未知权限: ${p}`);
  }
  return errs;
}

/** 递归收集文件；跳过 key.bin / *.bin 加密存储等运行时产物。 */
function collectFiles(dir, base, out) {
  for (const name of fs.readdirSync(dir)) {
    const abs = path.join(dir, name);
    const rel = base ? `${base}/${name}` : name;
    const st = fs.statSync(abs);
    if (st.isDirectory()) {
      collectFiles(abs, rel, out);
      continue;
    }
    if (name === "key.bin" || name.endsWith(".bin")) continue; // 运行时加密存储不随包分发
    out.push({ rel, abs });
  }
}

/** 读 JSON（容忍 UTF-8 BOM）。 */
function readJson(p) {
  const raw = fs.readFileSync(p, "utf8").replace(/^\uFEFF/, "");
  return JSON.parse(raw);
}

function pack(extDir, outPath) {
  const manifestPath = path.join(extDir, ".uxpack.json");
  if (!fs.existsSync(manifestPath)) {
    console.error(`错误: ${extDir} 内没有 .uxpack.json`);
    process.exit(1);
  }
  let manifest;
  try {
    manifest = readJson(manifestPath);
  } catch (e) {
    console.error(`错误: .uxpack.json 解析失败: ${e.message}`);
    process.exit(1);
  }
  const errs = validateManifest(manifest);
  if (errs.length) {
    console.error("manifest 校验失败:\n  - " + errs.join("\n  - "));
    process.exit(1);
  }
  const files = {};
  const collected = [];
  collectFiles(extDir, "", collected);
  if (!collected.some((f) => f.rel === manifest.entry)) {
    console.error(`错误: entry 不在包内: ${manifest.entry}`);
    process.exit(1);
  }
  for (const f of collected) {
    files[f.rel] = fs.readFileSync(f.abs).toString("base64");
  }
  const envelope = JSON.stringify({ uxpack: UXPACK_VERSION, manifest, files });
  const out = outPath || `${manifest.id}-${manifest.version}.uxpack`;
  fs.writeFileSync(out, envelope);
  console.log(`已打包: ${out} (${Object.keys(files).length} 个文件, ${Buffer.byteLength(envelope)} 字节)`);
}

function verify(packPath) {
  let env;
  try {
    env = readJson(packPath);
  } catch (e) {
    console.error(`无效: 解析失败: ${e.message}`);
    process.exit(1);
  }
  const errs = [];
  if (env.uxpack !== UXPACK_VERSION) errs.push(`容器版本不符: ${env.uxpack}`);
  errs.push(...validateManifest(env.manifest));
  if (!env.files || !Object.keys(env.files).length) errs.push("包内没有文件");
  for (const name of Object.keys(env.files ?? {})) {
    if (!name || name.includes("..") || name.includes("\\") || name.startsWith("/")) {
      errs.push(`非法包内路径: ${name}`);
    } else {
      try { Buffer.from(env.files[name], "base64"); } catch { errs.push(`base64 损坏: ${name}`); }
    }
  }
  if (env.manifest && env.manifest.entry && !(env.files ?? {})[env.manifest.entry]) {
    errs.push(`entry 不在包内: ${env.manifest.entry}`);
  }
  if (errs.length) {
    console.error("无效包:\n  - " + errs.join("\n  - "));
    process.exit(1);
  }
  console.log(`有效: ${env.manifest.id} v${env.manifest.version} (${env.manifest.type}), ${Object.keys(env.files).length} 个文件`);
}

const [cmd, target, outFlag, outVal] = process.argv.slice(2);
if (cmd === "pack" && target) pack(target, outFlag === "-o" ? outVal : undefined);
else if (cmd === "verify" && target) verify(target);
else {
  console.error("用法: node tools/uxpack.cjs pack <ext-dir> [-o out.uxpack] | verify <pack.uxpack>");
  process.exit(1);
}
