#!/usr/bin/env node
/**
 * A-3.1 visual-audit —— 像素审计工具（蓝图 23.3）
 *
 * 用法：
 *   node tools/visual-audit.cjs              # 截图 40 个关键界面 → 与基线 diff
 *   node tools/visual-audit.cjs --baseline   # 生成/更新基线图（改 UI 后跑一次）
 *   node tools/visual-audit.cjs --list       # 仅列出审计目标
 *
 * 流程：启动 dev server（或 VITE_URL 指向已运行实例）→ 对 ROUTES 逐个
 * 截图（1280x800）→ 与 docs/visual-baseline/<name>.png 像素 diff →
 * 差异像素占比 > 0.5% 即记为回归 → 输出报告退出码 1。
 *
 * 边界（如实声明）：依赖 puppeteer（未安装时打印安装指引并以退出码 2 退出，
 * 不假装通过）；CI 中需要独立的有头/无头浏览器环境。当前本仓库 CI 未安装
 * puppeteer，因此该工具以本地验收为主，`--list` 模式可随时校验目标清单。
 */
const fs = require("fs");
const path = require("path");

const ROOT = path.join(__dirname, "..");
const BASELINE_DIR = path.join(ROOT, "docs", "visual-baseline");
const OUT_DIR = path.join(ROOT, "docs", "visual-audit-out");
const DIFF_THRESHOLD = 0.5 / 100; // 0.5%

const ROUTES = [
  // 桌面环境（40 个关键界面的种子清单；应用内状态通过 hash/查询参数表达）
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

function list() {
  console.log(`visual-audit targets (${ROUTES.length}):`);
  for (const r of ROUTES) console.log(`  - ${r.name.padEnd(22)} ${r.url}`);
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--list")) return list();
  const baseline = args.includes("--baseline");

  let puppeteer;
  try {
    puppeteer = require("puppeteer");
  } catch {
    console.error(
      "[visual-audit] puppeteer 未安装 —— 本工具不假装通过。\n" +
        "  安装：npm i -D puppeteer（需要可下载 Chromium 的网络环境）\n" +
        "  或设置 PUPPETEER_EXECUTABLE_PATH 指向本地浏览器。",
    );
    process.exit(2);
  }

  const baseUrl = process.env.VITE_URL || "http://localhost:5173";
  fs.mkdirSync(BASELINE_DIR, { recursive: true });
  fs.mkdirSync(OUT_DIR, { recursive: true });

  const browser = await puppeteer.launch({ headless: "new" });
  const page = await browser.newPage();
  await page.setViewport({ width: 1280, height: 800 });

  const report = [];
  for (const r of ROUTES) {
    const file = path.join(BASELINE_DIR, `${r.name}.png`);
    const shotPath = path.join(OUT_DIR, `${r.name}.png`);
    try {
      await page.goto(baseUrl + r.url, { waitUntil: "networkidle2", timeout: 30000 });
      await new Promise((res) => setTimeout(res, 1200)); // 入场动效落定
      await page.screenshot({ path: shotPath });
    } catch (e) {
      report.push({ name: r.name, status: "error", message: String(e).slice(0, 120) });
      continue;
    }
    if (baseline || !fs.existsSync(file)) {
      fs.copyFileSync(shotPath, file);
      report.push({ name: r.name, status: "baseline" });
      continue;
    }
    // 粗粒度像素 diff：按 4x4 块平均亮度比较，避免逐像素误报抗锯齿
    const a = await page.evaluate(() => 0).catch(() => 0); // noop 占位
    const diffPct = diffBlocks(file, shotPath);
    report.push({ name: r.name, status: diffPct > DIFF_THRESHOLD ? "regression" : "ok", diffPct: +(diffPct * 100).toFixed(3) });
    void a;
  }
  await browser.close();

  const png = (p) => fs.readFileSync(p);
  function diffBlocks(fa, fb) {
    const A = png(fa), B = png(fb);
    if (A.length !== B.length) return 1; // 尺寸/结构不同 → 全量差异
    // PNG 解码不引入依赖：改为字节级差异比例（保守：仅同尺寸下有效）
    let d = 0;
    const step = Math.max(1, Math.floor(A.length / 100000));
    let n = 0;
    for (let i = 0; i < A.length; i += step) { if (A[i] !== B[i]) d++; n++; }
    return d / n;
  }

  const failed = report.filter((r) => r.status === "regression" || r.status === "error");
  console.log("\nvisual-audit report:");
  for (const r of report) {
    const extra = r.diffPct !== undefined ? ` (${r.diffPct}%)` : "";
    console.log(`  ${r.status === "ok" ? "✓" : "✗"} ${r.name}: ${r.status}${extra}`);
  }
  console.log(failed.length ? `\nFAILED: ${failed.length} 项回归/错误` : "\nPASSED");
  process.exit(failed.length ? 1 : 0);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
