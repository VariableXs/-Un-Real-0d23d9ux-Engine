#!/usr/bin/env bash
# AI-J1 v3 换体提交构建器（_attic 工具，非功能材料）
# 用法：bash build-swap.sh   —— 产出 /tmp/aij1-swap/ 下三个「HEAD + 仅我方 hunk」版本
set -euo pipefail
cd "$(dirname "$0")/../.." || exit 1
mkdir -p /tmp/aij1-swap

node << 'NODE'
const { execSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const SWAP = path.join(os.tmpdir(), "aij1-swap");
fs.mkdirSync(SWAP, { recursive: true });
const head = (p) => execSync(`git show HEAD:"${p}"`, { maxBuffer: 1 << 26 }).toString("utf8");
const contains = (hay, needle) => hay.includes(needle);
const countOf = (hay, needle) => hay.split(needle).length - 1;

// 1) App.tsx: HEAD + 仅我方两处（import 替换 + 层挂载两行）
let app = head("src/App.tsx");
const impOld = 'import { J1Runtime } from "./features/mouse/J1Runtime";';
if (!contains(app, impOld)) throw new Error("App import anchor missing");
app = app.replace(impOld, 'import { J1Runtime, J1AppWindowLayer } from "./features/mouse/J1Runtime";');
const anchor = "        <RecoveryPromptHost />";
if (countOf(app, anchor) !== 1) throw new Error("App RecoveryPromptHost anchor not unique: " + countOf(app, anchor));
const inject = anchor + "\n        {/* J 鼠标域 AI-J1：软件窗口锚标/墨迹层 + headless 指针/滚轮/侧键/手势内核（v3 接线） */}\n        <J1AppWindowLayer appType={appType} />";
app = app.replace(anchor, inject);
fs.writeFileSync(path.join(SWAP, "App.tsx"), app);

// 2) SettingsModal.tsx: HEAD + 仅 data-autoscroll 一处
let sm = head("src/features/settings/SettingsModal.tsx");
const smOld = '<div className="settings-body w11-page">';
if (countOf(sm, smOld) !== 1) throw new Error("SettingsModal anchor not unique: " + countOf(sm, smOld));
sm = sm.replace(smOld, '<div className="settings-body w11-page" data-autoscroll="">');
fs.writeFileSync(path.join(SWAP, "SettingsModal.tsx"), sm);

// 3) CHANGELOG.md: HEAD + 仅我方条目（插在第一个 Unreleased 之前）
let cl = head("CHANGELOG.md");
const entry = fs.readFileSync("_attic/aij1-f601-f620/changelog-entry.md", "utf8");
const cut = cl.indexOf("## [Unreleased]");
if (cut < 0) throw new Error("CHANGELOG Unreleased anchor missing");
cl = cl.slice(0, cut) + entry + "\n" + cl.slice(cut);
fs.writeFileSync(path.join(SWAP, "CHANGELOG.md"), cl);

console.log("swap built:", fs.statSync(path.join(SWAP, "App.tsx")).size,
  fs.statSync(path.join(SWAP, "SettingsModal.tsx")).size,
  fs.statSync(path.join(SWAP, "CHANGELOG.md")).size);
console.log("SWAP_DIR=" + SWAP);
NODE
