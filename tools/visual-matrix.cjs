#!/usr/bin/env node
/**
 * M-82 视觉回归多主题矩阵（Visual Matrix Regression）— AI-20 质量门禁与收官组
 *
 * 基线结构升级（承 A-3 visual-audit.cjs，不替代）：
 *   docs/visual-matrix/<route>/<theme>/<scale>.png   ← {route}/{theme}/{scale} 两维分层
 *   主题：dark / light / hc  ｜ 缩放：100% / 125% / 150%
 *   15 路由 × 3 主题 × 3 缩放 = 135 张基线
 *
 * 阈值按主题标定（HC 对比强、色阶锐利，误报率低但噪声也低，故收紧）：
 *   dark 0.50% ｜ light 0.55% ｜ hc 0.35%
 *
 * 用法：
 *   node tools/visual-matrix.cjs --list        # 仅列出 135 个目标（不装 puppeteer 也可跑）
 *   node tools/visual-matrix.cjs --plan        # 输出基线文件清单与当前覆盖率（不渲染）
 *   node tools/visual-matrix.cjs               # 跑批 diff（需 puppeteer）
 *   node tools/visual-matrix.cjs --baseline    # 生成/更新基线（PR 必须附 diff 截图）
 *   node tools/visual-matrix.cjs --routes desktop-shell,start-menu
 *   node tools/visual-matrix.cjs --theme hc --scale 125
 *
 * 退出码：0 通过 / 1 有回归或错误 / 2 依赖缺失（不假装通过）
 *
 * 缩放实现边界（如实声明）：页面缩放走 CDP Page.setPageZoomFactor；CDP 会话不可用时
 * 降级为 100% 并标记 scaleDegraded，报告明确提示该组结果不可作为门禁依据。
 */
const fs = require("fs");
const path = require("path");

const ROOT = path.join(__dirname, "..");
const BASELINE_DIR = path.join(ROOT, "docs", "visual-matrix");
const OUT_DIR = path.join(ROOT, "docs", "visual-matrix-out");

const ROUTES = [
  { name: "desktop-shell", url: "/" },
  { name: "start-menu", url: "/?shot=start" },
  { name: "quick-panel", url: "/?shot=quick" },
  { name: "settings-appearance", url: "/?shot=settings&tab=appearance" },
  { name: "settings-editor", url: "/?shot=settings&tab=editor" },
  { name: "settings-data", url: "/?shot=settings&tab=data" },
  { name: "settings-about", url: "/?shot=settings&tab=about" },
  { name: "taskman-processes", url: "/?shot=taskman" },
  { name: "search-overlay", url: "/?shot=search" },
  { name: "write-app", url: "/?shot=write" },
  { name: "mindmap-app", url: "/?shot=mind" },
  { name: "code-app", url: "/?shot=code" },
  { name: "fate-app", url: "/?shot=fate" },
  { name: "explorer", url: "/?shot=explorer" },
  { name: "welcome-wizard", url: "/?shot=wizard" },
];

const THEMES = ["dark", "light", "hc"];
const SCALES = [100, 125, 150];

const THRESHOLDS = { dark: 0.5, light: 0.55, hc: 0.35 }; // 百分比
const VIEWPORT = { width: 1280, height: 800 };

function targets() {
  const out = [];
  for (const r of ROUTES) for (const t of THEMES) for (const s of SCALES) out.push({ route: r.name, theme: t, scale: s });
  return out;
}

function themeStyle(theme) {
  return {
    dark: "background-color:#1a1a1a;color-scheme:dark;",
    light: "background-color:#f3f3f3;color-scheme:light;",
    hc: "background-color:#000000;color-scheme:light;filter:contrast(1.65) saturate(1.35);",
  }[theme];
}

function list() {
  console.log(`visual-matrix targets (${ROUTES.length} routes x ${THEMES.length} themes x ${SCALES.length} scales = ${ROUTES.length * THEMES.length * SCALES.length}):`);
  for (const r of ROUTES) {
    console.log(`  ${r.name.padEnd(22)} ${r.url}`);
    for (const t of THEMES) console.log(`      ├─ ${t.padEnd(6)} thresholds: ${THRESHOLDS[t]}%`);
    for (const s of SCALES) console.log(`      ├─ ${s}%`);
  }
  console.log(`\nbaseline root: ${path.relative(ROOT, BASELINE_DIR)}/<route>/<theme>/<scale>.png`);
}

function plan() {
  const all = targets();
  const have = all.filter((t) => fs.existsSync(path.join(BASELINE_DIR, t.route, t.theme, `${t.scale}.png`)));
  console.log(`visual-matrix plan: ${have.length}/${all.length} 基线就位`);
  for (const t of THEMES) {
    const n = have.filter((x) => x.theme === t).length;
    console.log(`  ${t.padEnd(6)} ${String(n).padStart(3)}/${ROUTES.length * SCALES.length}  (阈值 ${THRESHOLDS[t]}%)`);
  }
  const missing = all.filter((t) => !have.includes(t));
  if (missing.length) {
    console.log(`\n缺失基线（示例前 12 条，共 ${missing.length}）：`);
    for (const t of missing.slice(0, 12)) console.log(`  - ${t.route}/${t.theme}/${t.scale}.png`);
  }
  if (missing.length) {
    console.log(`\nNEXT: node tools/visual-matrix.cjs --baseline  （PR 必须含 diff 截图，见 docs/acceptance/ai20-质量门禁验收.md）`);
  }
}

function diffBlocks(fa, fb) {
  const A = fs.readFileSync(fa);
  const B = fs.readFileSync(fb);
  if (A.length !== B.length) return 1; // 结构/尺寸不同 → 全量差异
  let d = 0;
  const step = Math.max(1, Math.floor(A.length / 100000));
  let n = 0;
  for (let i = 0; i < A.length; i += step) {
    if (A[i] !== B[i]) d++;
    n++;
  }
  return n ? d / n : 0;
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--list")) return list();
  if (args.includes("--plan")) return plan();
  const baseline = args.includes("--baseline");

  const routeFilter = argVal("--routes");
  const themeFilter = argVal("--theme");
  const scaleFilter = parseInt(argVal("--scale"), 10);

  let puppeteer;
  try {
    puppeteer = require("puppeteer");
  } catch {
    console.error(
      "[visual-matrix] puppeteer 未安装 —— 本工具不假装通过。\n" +
        "  安装：npm i -D puppeteer（需要可下载 Chromium 的网络环境）\n" +
        "  或设置 PUPPETEER_EXECUTABLE_PATH 指向本地浏览器。\n" +
        "  无渲染环境时可用 --list / --plan 校验矩阵结构与基线覆盖率。",
    );
    process.exit(2);
  }

  const baseUrl = process.env.VITE_URL || "http://localhost:1420";
  fs.mkdirSync(BASELINE_DIR, { recursive: true });
  fs.mkdirSync(OUT_DIR, { recursive: true });

  const browser = await puppeteer.launch({ headless: "new" });
  const page = await browser.newPage();

  // 页面缩放：CDP 优先，失败降级为 100%（降级组结果不作为门禁依据）
  let cdp = null;
  let scaleDegraded = false;
  try {
    const client = await page.target().createCDPSession();
    cdp = { client, set: async (factor) => client.send("Page.setPageZoomFactor", { zoomFactor: factor }) };
  } catch {
    scaleDegraded = true;
  }

  async function apply(theme, scale) {
    await page.setViewport({ ...VIEWPORT });
    if (cdp) {
      await cdp.set(scale / 100);
    } else {
      scaleDegraded = true;
    }
    // 主题注入：导航前写入，供入口读取；同时直接改根元素背景与 color-scheme
    await page.evaluateOnNewDocument((style) => {
      try {
        const root = document.documentElement;
        root.dataset.auditTheme = root.dataset.auditTheme || "";
        const ins = document.createElement("style");
        ins.id = "__visual_matrix_theme__";
        ins.textContent = `html{${style}}`;
        (document.head || document.documentElement).appendChild(ins);
      } catch {
        /* 无 DOM 时忽略 */
      }
    }, themeStyle(theme));
  }

  let list2 = targets();
  if (routeFilter) list2 = list2.filter((t) => routeFilter.includes(t.route));
  if (themeFilter && THEMES.includes(themeFilter)) list2 = list2.filter((t) => t.theme === themeFilter);
  if (Number.isFinite(scaleFilter) && SCALES.includes(scaleFilter)) list2 = list2.filter((t) => t.scale === scaleFilter);

  const urlOf = (name) => {
    const r = ROUTES.find((x) => x.name === name);
    return r ? r.url : "/";
  };

  const report = [];
  for (const t of list2) {
    const rel = `${t.route}/${t.theme}/${t.scale}.png`;
    const file = path.join(BASELINE_DIR, rel);
    const shotPath = path.join(OUT_DIR, rel);
    fs.mkdirSync(path.dirname(shotPath), { recursive: true });
    try {
      await apply(t.theme, scaleDegraded ? 100 : t.scale);
      await page.goto(baseUrl + urlOf(t.route), { waitUntil: "networkidle2", timeout: 30000 });
      await new Promise((res) => setTimeout(res, 1200)); // 入场动效落定
      await page.screenshot({ path: shotPath });
    } catch (e) {
      report.push({ ...t, status: "error", message: String(e).slice(0, 110) });
      continue;
    }
    if (baseline || !fs.existsSync(file)) {
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.copyFileSync(shotPath, file);
      report.push({ ...t, status: "baseline" });
      continue;
    }
    const limit = THRESHOLDS[t.theme];
    let diffPct = 0;
    try {
      diffPct = +(diffBlocks(file, shotPath) * 100).toFixed(3);
    } catch (e) {
      report.push({ ...t, status: "error", message: `diff 失败：${String(e).slice(0, 90)}` });
      continue;
    }
    const degraded = scaleDegraded && t.scale !== 100;
    report.push({
      ...t,
      status: diffPct > limit ? "regression" : "ok",
      diffPct,
      threshold: limit,
      ...(degraded ? { scaleDegraded: true } : {}),
    });
  }
  await browser.close();

  const failed = report.filter((r) => r.status === "regression" || r.status === "error");
  console.log("\nvisual-matrix report:");
  for (const r of report) {
    const flag = r.status === "ok" ? "✓" : r.status === "baseline" ? "＋" : "✗";
    const extra = r.diffPct !== undefined ? ` (${r.diffPct}% / 阈值 ${r.threshold}%)` : "";
    const dg = r.scaleDegraded ? " [缩放降级→100%]" : "";
    const msg = r.message ? ` — ${r.message}` : "";
    console.log(`  ${flag} ${r.route}/${r.theme}/${r.scale}${extra}${dg}${msg}`);
  }
  if (scaleDegraded) {
    console.log("\n⚠️ CDP 缩放不可用：非 100% 组实际以 100% 渲染，该组 diff 结果不作为门禁依据。");
  }
  console.log(failed.length ? `\nFAILED: ${failed.length} 项回归/错误` : `\nPASSED (${report.length} 组)`);
  process.exit(failed.length ? 1 : 0);
}

function argVal(flag) {
  const i = process.argv.indexOf(flag);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
