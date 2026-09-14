/**
 * Win11 设置窗口静态预览生成器（非功能性自检工具，归档 _attic/）。
 *
 * 目的：把 src/styles/win11-settings.css 直接套在一份等价 DOM 上，用无头 Edge 截图，
 * 自检「尺寸/布局/控件」是否真的按实测规格渲染（jsdom 不算布局，只有真渲染才算数）。
 * 图标取自 node_modules/lucide-react（与运行时的同一份路径数据）。
 */
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../..");
const iconDir = resolve(root, "node_modules/lucide-react/dist/esm/icons");

const ATTR = { d: "d", cx: "cx", cy: "cy", r: "r", x: "x", y: "y", width: "width", height: "height", rx: "rx", x1: "x1", y1: "y1", x2: "x2", y2: "y2", points: "points" };

function icon(name) {
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
  return `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${children.join("")}</svg>`;
}

const T = (s, size = 16) => icon(s).replace(/width="16" height="16"/, `width="${size}" height="${size}"`);

const NAV = [
  ["home", "主页"], ["palette", "外观"], ["file-text", "编辑器"], ["git-branch", "思维导图"],
  ["sliders-horizontal", "通用"], ["globe", "环境"], ["layout-template", "浏览器"], ["code", "编码"],
  ["layers", "生态"], ["network", "网络"], ["shield", "安全工作台"], ["keyboard", "快捷键"],
  ["mouse-pointer-2", "输入手感"], ["app-window", "氛围"], ["gauge", "窗口手感"], ["rocket", "性能与维护"],
];
const SYSNAV = [["monitor", "显示"], ["volume-2", "声音"], ["network", "网络和 Internet"], ["user", "账户"], ["clock", "时间和语言"]];

const navItem = ([ic, label], on = false) =>
  `<button type="button" class="w11-navitem${on ? " on" : ""}">${T(ic)}<span class="w11-lbl">${label}</span></button>`;

const row = (title, sub, ctl) =>
  `<div class="w11-row"><span class="w11-rowtext"><span class="w11-rowtitle">${title}</span>${sub ? `<span class="w11-rowsub">${sub}</span>` : ""}</span>${ctl ? `<span class="w11-rowctl">${ctl}</span>` : ""}</div>`;

const select = (val, opts, wide = false) =>
  `<span class="w11-select${wide ? " w11-select-wide" : ""}"><select>${opts.map((o) => `<option${o === val ? " selected" : ""}>${o}</option>`).join("")}</select>${T("chevron-down", 14).replace('class=', 'class="w11-chev" ')}</span>`;

const sw = (on = false) => `<button type="button" role="switch" class="w11-switch" aria-checked="${on}"><span class="knob"></span></button>`;
const slider = (pct, label) =>
  `<span class="w11-rowtext" style="flex:none"><span class="w11-rowsub">${label}</span></span><input type="range" min="0" max="100" value="${pct}" style="--w11-fill: calc(${pct}% + ${(10 - pct * 0.2).toFixed(2)}px)">`;

const card = (title, desc, body) =>
  `<section class="w11-card"><div class="w11-cardhead"><h3>${title}</h3>${desc ? `<p>${desc}</p>` : ""}</div>${body}</section>`;

const linkRow = (ic, title, sub) =>
  `<div class="w11-row w11-row-click"><span class="w11-rowicon">${T(ic, 20)}</span>${T("chevron-right", 12).replace('class=', 'class="w11-chev" ')}</div>`;

function window_(inner, cls = "") {
  return `<div class="modal-overlay preview-static preview-viewport"><div class="modal modal-w11 ${cls}"><div class="modal-body">${inner}</div></div></div>`;
}

const APPEARANCE = `
  <div class="settings-layout w11-shell">
    <div class="w11-titlebar">
      <button type="button" class="w11-back">${T("arrow-left")}</button>
      <span class="w11-apptitle">设置</span>
      <div class="w11-search">${T("search", 14)}<input type="text" placeholder="搜索设置"></div>
      <div class="w11-caption">
        <button type="button" class="w11-cap"><span class="w11-glyph w11-glyph-min"></span></button>
        <button type="button" class="w11-cap"><span class="w11-glyph w11-glyph-max"></span></button>
        <button type="button" class="w11-cap w11-cap-close"><span class="w11-glyph w11-glyph-close"></span></button>
      </div>
    </div>
    <div class="w11-main">
      <nav class="settings-nav w11-nav">
        <div class="w11-account"><span class="w11-avatar">V</span><span class="w11-who"><b>Variable 引擎</b><span>本地账户 · 离线优先</span></span></div>
        ${NAV.map((n, i) => navItem(n, i === 1)).join("")}
        <div class="nav-group w11-navgroup">系统与环境</div>
        ${SYSNAV.map((n) => navItem(n)).join("")}
      </nav>
      <div class="settings-body w11-page">
        <div class="w11-content">
          <nav class="w11-crumb"><button type="button" class="w11-crumb-lv">设置</button><span class="w11-crumb-sep">›</span><span class="w11-crumb-cur">外观</span></nav>
          ${card("桌面壁纸", "", row("桌面壁纸", "", select("视频壁纸", ["引力场", "纯色", "图片", "动态", "视频壁纸"])) + row("桌面图标大小", "", select("中 · 48", ["小 · 32", "中 · 48", "大 · 64"])))}
          ${card("窗口与任务栏", "", row("窗口控制按钮位置", "同步作用于桌面红绿灯与各软件窗口", select("Mac 风格（右上角圆点）", ["Mac 风格（右上角圆点）", "Windows 风格（右上角按钮）"], true)) + row("任务栏位置", "", select("底部（默认）", ["底部（默认）", "左侧", "右侧", "顶部"])) + row("运行指示样式", "", select("Win11 圆点（默认）", ["Win11 圆点（默认）", "下划线", "胶囊"])))}
          ${card("个性化", "", row("媒体呼吸", "播放时钟旁微幅呼吸", sw(true)) + row("主题", "", select("深空", ["深空", "纸张", "极简黑", "高对比", "自定义"])) + row("纯色背景", "", '<input type="color" value="#7ba7d8">'))}
          ${card("画面调节", "", `<div class="w11-row w11-row-stack">${slider(62, "亮度 62%")}</div><div class="w11-row w11-row-stack">${slider(18, "模糊 18px")}</div><div class="w11-row w11-row-stack">${slider(0, "暗角 0%")}</div>`)}
          ${card("组织与订阅", "", `<div class="w11-cardbody"><div class="row gap8" style="display:flex;gap:8px"><input class="w11-input flex-1" value="Asia/Shanghai" style="flex:1"><button type="button" class="w11-btn">添加时区</button><button type="button" class="w11-btn w11-btn-accent">免费试用</button></div><label class="check-line" style="display:inline-flex;align-items:center;gap:10px;margin-top:12px;font-size:14px"><input type="checkbox" checked> 每日自动更换壁纸</label></div>`)}
          ${card("更多设置", "", linkRow("monitor", "屏幕", "") + linkRow("app-window", "默认应用", "") + linkRow("search", "搜索", ""))}
        </div>
      </div>
    </div>
  </div>`;

const LEGACY = `
  <div class="settings-layout w11-shell">
    <div class="w11-titlebar">
      <button type="button" class="w11-back">${T("arrow-left")}</button>
      <span class="w11-apptitle">设置</span>
      <div class="w11-search">${T("search", 14)}<input type="text" placeholder="搜索设置"></div>
      <div class="w11-caption">
        <button type="button" class="w11-cap"><span class="w11-glyph w11-glyph-min"></span></button>
        <button type="button" class="w11-cap"><span class="w11-glyph w11-glyph-restore"></span></button>
        <button type="button" class="w11-cap w11-cap-close"><span class="w11-glyph w11-glyph-close"></span></button>
      </div>
    </div>
    <div class="w11-main">
      <nav class="settings-nav w11-nav">
        <div class="w11-account"><span class="w11-avatar">V</span><span class="w11-who"><b>Variable 引擎</b><span>本地账户 · 离线优先</span></span></div>
        ${NAV.map((n, i) => navItem(n, i === 12)).join("")}
        <div class="nav-group w11-navgroup">系统与环境</div>
        ${SYSNAV.map((n) => navItem(n)).join("")}
      </nav>
      <div class="settings-body w11-page">
        <div class="w11-content">
          <nav class="w11-crumb"><button type="button" class="w11-crumb-lv">设置</button><span class="w11-crumb-sep">›</span><span class="w11-crumb-cur">输入手感</span></nav>
          <div class="w11-legacy">
          <label class="field"><span class="field-label">指针加速度</span><select><option>跟随系统</option><option>关闭</option></select></label>
          <label class="field"><span class="field-label">双击间隔（ms）</span><input class="text-input" value="500"></label>
          <label class="field"><span class="field-label">滚轮行数</span><input type="range" min="0" max="100" value="35" style="--w11-fill: calc(35% + 3.00px)"></label>
          <label class="field"><span class="field-label">键盘重复延迟</span><div class="col gap4"><select><option>短</option><option>中</option></select><span class="dim small">仅对硬件键盘生效</span></div></label>
          <label class="field"><span class="field-label">启用平滑滚动</span><label class="check-line"><input type="checkbox" checked> 开启</label></label>
          <h4>进阶</h4>
          <label class="field"><span class="field-label">动效强度</span><span class="row gap8"><button type="button" class="btn ghost">恢复默认</button><button type="button" class="btn ghost">导出</button></span></label>
          </div>
        </div>
      </div>
    </div>
  </div>`;

const html = `<!doctype html>
<html lang="zh-CN" data-theme="deep-space">
<head>
<meta charset="utf-8">
<title>Win11 设置窗口 · 静态自检预览</title>
<!-- 直接引用仓库真实样式：global.css 内部 @import 顺序与运行时一致（… → overlays → win11-settings） -->
<link rel="stylesheet" href="../../src/styles/global.css">
<style>
  html, body { margin: 0; padding: 0; background: #0b1122; }
  body.preview { display: flex; flex-direction: column; align-items: center; gap: 28px; padding: 28px; }
  .preview-label { color: #7f8ea3; font: 12px/1.6 "Segoe UI", system-ui, sans-serif; align-self: flex-start; }
  /* 预览专用：把固定定位的遮罩改为静态排布，便于同页并排截图 */
  .modal-overlay.preview-static { position: static; inset: auto; display: block; background: none; backdrop-filter: none; }
  /* 预览专用：两个窗口统一 1080×720，便于同页并排对比（最大化变体不在此自检范围） */
  /* 预览专用：在固定视口容器内展示全屏设置面板（100% 相对该容器） */
  .preview-static.preview-viewport { width: 1360px; height: 820px; overflow: hidden; }
  .preview-static.preview-windowed .modal.modal-w11 { width: min(94vw, 1080px); height: min(88vh, 720px); }
</style>
</head>
<body class="preview">
  <div class="preview-label">① 已迁移页（外观）· 全屏面板 · 标题栏 48 / 导航 300 / 卡片内边距 24 / 行高 68 / 面包屑页头 28</div>
  ${window_(APPEARANCE)}
  <div class="preview-label">② 未迁移页（输入手感）· 同一外壳，靠 .w11-legacy 桥接既有 .field 版式</div>
  ${window_(LEGACY, "modal-w11-max")}
</body>
</html>
`;

mkdirSync(resolve(here, "../preview"), { recursive: true });
const out = resolve(here, "../preview/win11-settings-preview.html");
writeFileSync(out, html, "utf8");
console.log("WROTE", out, html.length, "bytes");
