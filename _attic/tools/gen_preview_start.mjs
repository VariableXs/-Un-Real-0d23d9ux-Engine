/**
 * 开始菜单 Win11 新版面板 —— 静态预览生成器（非功能性自检工具，归档 _attic/）。
 *
 * 目的：把 **仓库里真实的** src/styles/desktop.css + startmenu-board.css 套在一份等价 DOM 上，
 * 生成可直接打开的单文件预览，用来核对「尺寸是否真的按实测规格渲染」。
 * 图标取自 node_modules/lucide-react（与运行时同一份路径数据），零网络、零图片依赖。
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");
const iconDir = resolve(root, "node_modules/lucide-react/dist/esm/icons");
const outDir = resolve(root, "_attic/preview");

const ATTR = {
  d: "d", cx: "cx", cy: "cy", r: "r", x: "x", y: "y", width: "width", height: "height",
  rx: "rx", x1: "x1", y1: "y1", x2: "x2", y2: "y2", points: "points",
};

function icon(name, size = 16) {
  const src = readFileSync(resolve(iconDir, `${name}.js`), "utf8");
  const children = [];
  const re = /\[\s*"(\w+)"\s*,\s*\{([^}]*)\}\s*\]/g;
  let m;
  while ((m = re.exec(src)) !== null) {
    const [, tag, rawAttrs] = m;
    const attrs = {};
    const ar = /(\w+):\s*"([^"]*)"/g;
    let a;
    while ((a = ar.exec(rawAttrs)) !== null) {
      const k = ATTR[a[1]];
      if (k && k !== "d") attrs[k] = a[2];
    }
    const dr = /d:\s*"([^"]*)"/.exec(rawAttrs);
    const body = Object.entries(attrs).map(([k, v]) => `${k}="${v}"`).join(" ");
    children.push(`<${tag}${body ? " " + body : ""}${dr ? ` d="${dr[1]}"` : ""}/>`);
  }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${children.join("")}</svg>`;
}

const RECENT = [
  ["music", "汽水音乐", 158],
  ["file-text", "C^2 Courage (Un)Real Variable", 200],
  ["monitor", "联想电脑管家", 214],
  ["mail", "outlook", 206],
  ["code", "traecode", 262],
  ["sparkles", "workbuddy", 276],
  ["git-branch", "github", 0],
  ["palette", "cavoti", 300],
  ["app-window", "global build", 210],
  ["terminal", "zcode global build", 130],
  ["cpu", "kira ai", 340],
  ["layers", "Visual Studio Installer", 265],
];

const PINNED = [
  ["folder-open", "文件管理器", 210],
  ["pen-line", "编辑器", 200],
  ["git-branch", "思维导图", 262],
  ["trash-2", "回收站", 0],
  ["gauge", "任务管理器", 190],
  ["settings", "设置", 220],
];

const TOPAPPS = [
  ["camera", "截图工具", 200],
  ["palette", "画图", 300],
  ["folder-open", "文件资源管理器", 45],
  ["globe", "Microsoft Edge", 200],
];

const recentRow = ([ic, label, hue]) =>
  `<button type="button" class="sm-row"><span class="desktop-icon-tile sm-row-icon" style="--hue:${hue}">${icon(ic, 18)}</span><span class="sm-row-name">${label}</span></button>`;

const pinnedRow = ([ic, label, hue]) =>
  `<button type="button" class="start-app"><span class="desktop-icon-tile" style="--hue:${hue}">${icon(ic, 18)}</span><span class="start-app-name">${label}</span></button>`;

const topApp = ([ic, label, hue]) =>
  `<button type="button" class="sm-topapp"><span class="desktop-icon-tile sm-topapp-icon" style="--hue:${hue}">${icon(ic, 22)}</span><span class="sm-topapp-name">${label}</span></button>`;

const chip = (label, cls = "") =>
  `<button type="button" class="sm-chip${cls}">${label}</button>`;

const trendingChip = (label) =>
  `<button type="button" class="sm-chip sm-trend-chip">${icon("search", 13)}<span>${label}</span></button>`;

const HTML = `<!DOCTYPE html>
<html lang="zh-CN" data-theme="deep-space">
<head>
<meta charset="utf-8">
<title>开始菜单 · Win11 新版面板（静态自检预览）</title>
<style>
${readFileSync(resolve(root, "src/design/tokens.css"), "utf8")}
${readFileSync(resolve(root, "src/styles/desktop.css"), "utf8")}
${readFileSync(resolve(root, "src/styles/startmenu-board.css"), "utf8")}

/* —— 预览页专用：把面板从其原生定位里解放出来，便于整页查看 —— */
html, body { margin: 0; height: 100%; background: #0d1117; }
body {
  font: 14px/1.5 "Segoe UI", "Microsoft YaHei UI", system-ui, sans-serif;
  display: flex; align-items: flex-start; justify-content: center;
  padding: 24px 0 40px;
}
.start-overlay { position: relative; inset: auto; display: block; }
.start-overlay .start-menu { position: relative; left: auto; bottom: auto; transform: none; }
</style>
</head>
<body>
<div class="start-overlay">
  <div class="start-menu">
    <div class="start-search">${icon("search", 16)}<input placeholder="搜索应用、设置和文档" readonly></div>
    <div class="sm-body">
      <section class="sm-col sm-col-a">
        <p class="sm-head start-section dim small">最近使用</p>
        <div class="sm-recent-list">${RECENT.map(recentRow).join("")}</div>
        <p class="sm-head start-section start-group-head dim small"><span>已固定</span></p>
        <div class="start-grid">${PINNED.map(pinnedRow).join("")}</div>
      </section>

      <section class="sm-col sm-col-b">
        <div class="sm-today">
          <span class="sm-today-l">Today</span>
          <span class="sm-today-d">· 9月14日 周一</span>
          <span class="sm-flex"></span>
          <span class="sm-stat">200 ${icon("trophy", 13)}</span>
          <button type="button" class="sm-me">V</button>
          <div class="start-power-wrap">
            <button type="button" class="sm-more">${icon("more-horizontal", 16)}</button>
          </div>
        </div>
        <div class="sm-hero">
          <span class="sm-hero-art" style="--hue:126"></span>
          <span class="sm-hero-cap">
            <span class="sm-hero-kicker">每日一图</span>
            <span class="sm-hero-title">9月14日</span>
          </span>
        </div>
      </section>

      <section class="sm-col sm-col-b2">
        <div class="sm-card sm-topapps">
          <p class="sm-card-head">热门应用</p>
          <div class="sm-topapps-grid">${TOPAPPS.map(topApp).join("")}</div>
        </div>
      </section>

      <section class="sm-col sm-col-c">
        <div class="sm-card sm-quiz">
          <span class="sm-quiz-art"></span>
          <p class="sm-quiz-head">每日问答</p>
          <p class="sm-quiz-q">想把任务栏停靠到屏幕左侧，去哪一页改？</p>
          <div class="sm-quiz-opts">
            ${chip("设置 › 外观", " right")}
            ${chip("设置 › 系统 › 声音")}
            ${chip("桌面右键菜单")}
          </div>
          <p class="sm-quiz-note dim small right">答对了 · 外观页的「窗口与任务栏」卡片里可切换任务栏四向停靠。</p>
        </div>
      </section>

      <section class="sm-col sm-col-c2">
        <div class="sm-card sm-tip">
          <span class="sm-tip-art"></span>
          <p class="sm-tip-head">今日提示 · 9月14日</p>
          <p class="sm-tip-title">用字母跳转开始菜单</p>
          <p class="sm-tip-note dim small">打开开始菜单后直接敲字母（或拼音首字母），命中项会高亮，回车即可启动。</p>
        </div>
      </section>

      <section class="sm-col sm-trend">
        <p class="sm-card-head sm-trend-head">${icon("trending-up", 14)} 热门搜索</p>
        <div class="sm-trend-grid">
          ${trendingChip("外观设置在哪")}
          ${trendingChip("workbuddy")}
          ${trendingChip("文件管理器")}
          ${trendingChip("思维导图")}
          ${trendingChip("任务管理器")}
          ${trendingChip("壁纸")}
        </div>
      </section>
    </div>
  </div>
</div>
</body>
</html>
`;

mkdirSync(outDir, { recursive: true });
writeFileSync(resolve(outDir, "startmenu-board.html"), HTML);
console.log("written", resolve(outDir, "startmenu-board.html"), HTML.length, "bytes");
