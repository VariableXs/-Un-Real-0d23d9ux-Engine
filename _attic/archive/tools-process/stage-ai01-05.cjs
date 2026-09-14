#!/usr/bin/env node
/**
 * AURORA-10000：AI-01~AI-05 批次，勿删。
 * stage-ai01-05.cjs — 并行会话安全暂存：
 * 工作区含其他 AI 会话未提交 WIP；共享文件（App.tsx / SettingsModal.tsx）按
 * 「HEAD 基线 + 仅本批次改动」重建 blob 后写入暂存区，工作区文件不动。
 * 其余本批次独占文件整文件暂存。他人 WIP 一律不入暂存区。
 */
const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const ROOT = path.resolve(__dirname, "..");
const W = (f, s) => fs.writeFileSync(path.join(ROOT, f), s, "utf8");
const git = (c) => execSync(c, { cwd: ROOT, encoding: "utf8" }).trim();
const head = (f) => git(`git show HEAD:${f}`);
const stageBlob = (f, content) => {
  W(f + ".__stage__.tmp", content);
  const sha = git(`git hash-object -w "${f}.__stage__.tmp"`);
  fs.unlinkSync(path.join(ROOT, f + ".__stage__.tmp"));
  git(`git update-index --add --cacheinfo 100644,${sha},${f}`);
  console.log(`staged(blob) ${f} @${sha.slice(0, 10)}`);
};
const stageFile = (f) => { git(`git add -- "${f}"`); console.log(`staged(file) ${f}`); };

/* ---- App.tsx：HEAD + 本批次两处改动 ---- */
let app = head("src/App.tsx");
const importAnchor = `import { OobeGate } from "./features/oobe/OobeWizard";`;
const importAdd = `${importAnchor}
// AURORA-10000：AI-01~AI-05 批次，勿删（族0020 睡眠唤醒剧场触发器）
import { showWakeCeremony } from "./system/boot/theater/ceremonyFx";`;
if (!app.includes(importAnchor) || app.includes("showWakeCeremony")) throw new Error("App import anchor");
app = app.replace(importAnchor, importAdd);
const demoAnchor = `    installDemoModeExitHook();
    void recoverDemoModeOnBoot();
  }, []);`;
if (!app.includes(demoAnchor)) throw new Error("App demo anchor");
const wakeBlock = `${demoAnchor}

  // AURORA-10000：AI-01~AI-05 批次，勿删 —— 族0020 睡眠唤醒剧场：
  // 屏幕从休眠/后台回到前台时按所选档位播放一次唤醒仪式（默认关 = 零行为变化）。
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let restore: (() => void) | null = null;
    const onVis = (): void => {
      if (document.visibilityState !== "visible" || restore) return;
      void loadSettings().then((s) => {
        if (disposed) return;
        const id = s.bootTheater?.wake;
        if (id) restore = showWakeCeremony(id);
      }).catch(() => { /* 浏览器 dev 或读取失败：静默 */ });
    };
    document.addEventListener("visibilitychange", onVis);
    return () => {
      disposed = true;
      document.removeEventListener("visibilitychange", onVis);
      restore?.();
    };
  }, []);`;
app = app.replace(demoAnchor, wakeBlock);
stageBlob("src/App.tsx", app);

/* ---- SettingsModal.tsx：HEAD + 本批次三处改动 ---- */
let sm = head("src/features/settings/SettingsModal.tsx");
const smImportAnchor = `import { SoundNotifyTab } from "./SoundNotifyTab";`;
sm = sm.replace(smImportAnchor, `${smImportAnchor}
// AURORA-10000：AI-01~AI-05 批次，勿删
import { BootTheaterTab } from "./BootTheaterTab";`);
const smTabsAnchor = `    { id: "about", label: t("aboutVariable") },
  ];`;
if (!sm.includes(smTabsAnchor)) throw new Error("SM tabs anchor");
sm = sm.replace(smTabsAnchor, `    { id: "about", label: t("aboutVariable") },
    // AURORA-10000：AI-01~AI-05 批次，勿删（启动与品牌剧场设置页）
    { id: "bootTheater", label: "启动剧场" },
  ];`);
const smRenderAnchor = `{tab === "sndnotify" && <SoundNotifyTab settings={props.settings} onPatch={props.onChange} />}`;
if (!sm.includes(smRenderAnchor)) throw new Error("SM render anchor");
sm = sm.replace(smRenderAnchor, `${smRenderAnchor}
          {/* AURORA-10000：AI-01~AI-05 批次，勿删（领域01 启动与品牌剧场 F00001~F00625 设置页） */}
          {tab === "bootTheater" && <BootTheaterTab settings={props.settings} onPatch={props.onChange} />}`);
stageBlob("src/features/settings/SettingsModal.tsx", sm);

/* ---- 本批次独占文件：整文件暂存 ---- */
[
  "src/system/boot/theater/registry.ts",
  "src/system/boot/theater/params.ts",
  "src/system/boot/theater/BootTheater.tsx",
  "src/system/boot/theater/theaterSound.ts",
  "src/system/boot/theater/ceremonyFx.ts",
  "src/system/boot/theater/__tests__/theater.test.ts",
  "src/features/settings/BootTheaterTab.tsx",
  "src/styles/boot-theater.css",
  "src/lib/settings.ts",
  "src/system/boot/BootScreen.tsx",
  "docs/AURORA-10000-功能全景图.md",
  "docs/AURORA-10000-AI分工完成图.md",
  "project_memory.md",
  "tools/gen-boot-theater.cjs",
  "tools/mark-done-ai01-05.cjs",
].forEach(stageFile);

console.log("---- staged diff stat ----");
console.log(git("git diff --cached --stat"));
