/* =============================================================================
 * AI-04 · 三界面系统（F301~F314）+ 功能区（F315~F320）
 *        + 极简界面设计（F321~F330）+ 结构稳定性保证（F331~F336）
 *   与 core::iface.rs 同契约：切换 400ms 交叉淡入、树的形状/位置/连线完全不变、
 *   每个界面独立保存级别/高亮/展开折叠状态。
 * ========================================================================== */
(function (root) {
  "use strict";
  var CA = (root.CA = root.CA || {});
  var U = CA.util;

  /* ── F301~F303 三界面 ───────────────────────────────────────────────── */
  var MODES = { PLAIN: "plain", PRO: "pro", COMPARE: "compare" };
  var FADE_MS = 400;
  CA.MODES = MODES;

  function InterfaceManager() {
    this.mode = MODES.PLAIN;
    this.states = {};
    this.fadeFrom = 0;
  }
  /** F301~303 切换：旧层 1→0，新层 0→1（400ms 交叉），形状/位置/连线不变。 */
  InterfaceManager.prototype.switch = function (to) {
    if (to === this.mode) return false;
    this.saveState({ level: this.level, selected: this.selected, expanded: this.expanded });
    this.mode = to;
    this.fadeFrom = performance.now();
    var st = this.states[to];
    if (st) { this.level = st.level; this.selected = st.selected; this.expanded = st.expanded; }
    return true;
  };
  InterfaceManager.prototype.saveState = function (s) { this.states[this.mode] = s; };
  /** 淡入淡出进度：旧层 / 新层不透明度。 */
  InterfaceManager.prototype.fade = function (now) {
    var p = U.clamp((now - this.fadeFrom) / FADE_MS, 0, 1);
    return [1 - p, p];
  };
  InterfaceManager.prototype.isCompare = function () { return this.mode === MODES.COMPARE; };

  /* ── F304 故事树：树干=项目名，主根=用户故事，侧根=情节 ──────────────── */
  CA.storyTree = function (project, stories, plots) {
    var out = [];
    stories.forEach(function (s, i) {
      out.push([project, s, (plots[i] || []).join(" / ")]);
    });
    return out;
  };

  /** F305 大白话标签 / F312 类型标注。 */
  CA.plainLabel = function (n) { return n.plain || n.name; };
  CA.typeLabel = function (fn, params, ret) {
    return "fn " + fn + "(" + params.map(function (p) { return p[0] + ":" + p[1]; }).join(",") + ")->" + ret;
  };

  /** F308 点击讲故事：3 句大白话，300ms 滑入。 */
  CA.tellStory = function (name, what, why) {
    return [
      "这一步叫「" + name + "」。",
      "它在做：" + (what || "处理一件事") + "。",
      "为什么要做：因为" + (why || "后面的步骤需要它") + "。"
    ];
  };

  /** F310 进度条：绿色填充 = 已了解百分比。 */
  CA.progressFill = function (understood, total) {
    return U.clamp(total ? understood / total : 0, 0, 1);
  };

  /** F311 一键总结。 */
  CA.summarize = function (project, features, lang) {
    return "这个项目叫「" + project + "」，用 " + lang + " 写成，一共 " + features +
      " 个功能。它像一家餐厅：前台接待用户，点餐台记订单，收银台管收钱，仓库存数据。";
  };

  /** F313 形式化断言：Hoare 三元组（悬停显示）。 */
  CA.hoare = function (pre, body, post) {
    return "{" + pre + "} " + body + " {" + post + "}";
  };

  /** F314 SSA 变量流：Def-Use 链 → 细线连接定义点与使用点。 */
  CA.ssaLinks = function (defUse) {
    var out = [];
    (defUse || []).forEach(function (d) { d[1].forEach(function (u) { out.push([d[0], u]); }); });
    return out;
  };

  /* ── F315~F320 功能区布局契约 ───────────────────────────────────────── */
  var FAINT = "1px solid rgba(128,128,128,0.12)";
  var DASHED = "1px dashed rgba(128,128,128,0.10)";
  var ALMOST = "1px solid rgba(128,128,128,0.06)";
  CA.PANEL_BORDER = { faint: FAINT, dashed: DASHED, almost: ALMOST };
  CA.defaultPanels = function (w, h) {
    return [
      { id: "Nav", x: 0, y: 0, w: 260, h: h, border: FAINT },
      { id: "Canvas", x: 260, y: 0, w: Math.max(0, w - 540), h: h - 62, border: "none" },
      { id: "Detail", x: w - 280, y: 0, w: 280, h: h, border: FAINT },
      { id: "Toolbar", x: 260, y: h - 62, w: Math.max(0, w - 540), h: 40, border: FAINT },
      { id: "Legend", x: w - 460, y: h - 102, w: 180, h: 120, border: DASHED },
      { id: "Status", x: 0, y: h - 22, w: w, h: 22, border: ALMOST }
    ];
  };

  /* ── F321~F330 极简界面设计 ─────────────────────────────────────────── */
  var MIN_COLOR = {
    "背景": { light: "#FFFFFF", dark: "#0A0A0F" },
    "面板": { light: "#F5F5F7", dark: "#1A1A2E" },
    "主文字": { light: "#1D1D1F", dark: "#F5F5F7" },
    "次文字": { light: "#86868B", dark: "#86868B" },
    "淡文字": { light: "#C7C7CC", dark: "#3A3A4A" },
    "主色": { light: "#0071E3", dark: "#2997FF" }
  };
  CA.minimalColor = function (elem, dark) {
    var m = MIN_COLOR[elem];
    if (!m) return "#8E8E93";
    return dark ? m.dark : m.light;
  };
  CA.LINE_DEFAULT = { color: "#C7C7CC", w: 1 };
  CA.NODE_FILL = "rgba(0,113,227,0.06)";
  CA.MIN_NODE_GAP = 48;
  CA.whitespaceOk = function (nodeArea, canvasArea) { return nodeArea / Math.max(1, canvasArea) <= 0.4; };
  CA.FONT_THIN = { family: "Inter", weight: 300 };
  CA.EDGE_REVEAL_PX = 10;
  /** F326 隐藏非必要 UI：鼠标距边缘 10px 内才浮现。 */
  CA.edgeReveal = function (v, max) { return v <= CA.EDGE_REVEAL_PX || v >= max - CA.EDGE_REVEAL_PX; };
  CA.PANEL_GLASS = "backdrop-filter: blur(20px) saturate(180%); background: rgba(255,255,255,0.7)";
  CA.MOTION_MS = 300;
  CA.MOTION_EASING = "ease-in-out";
  CA.RADIUS_PX = 8;
  CA.ICON_STYLE = "1px-line";

  /* ── F331~F336 结构稳定性保证 ───────────────────────────────────────── */
  /**
   * 固定布局引擎：首次计算全局坐标 → 不可变坐标表；
   * 手动布局快照（F335）优先于自动坐标。
   */
  function LayoutStore(coords) {
    this.coords = coords || {};
    this.manual = null;
    this.placeholders = {};
  }
  LayoutStore.prototype.coord = function (id) {
    if (this.manual && this.manual[id]) return this.manual[id];
    return this.coords[id];
  };
  /** F332 弹性节点尺寸：大小随级别变，中心点不变。 */
  LayoutStore.prototype.elasticSize = function (id, oldS, newS) {
    var c = this.coord(id) || [0, 0];
    return [[c[0] - oldS[0] / 2, c[1] - oldS[1] / 2], [c[0] - newS[0] / 2, c[1] - newS[1] / 2]];
  };
  /** F333 连线锚点锁定：锚定节点中心，尺寸变化时自动伸缩。 */
  LayoutStore.prototype.anchors = function (a, b) {
    var ca = this.coord(a), cb = this.coord(b);
    if (!ca || !cb) return null;
    return [ca, cb];
  };
  /** F334 占位符机制：折叠节点保留空间，其他节点不移位。 */
  LayoutStore.prototype.collapsePlaceholder = function (id, size) {
    this.placeholders[id] = size;
    return this.coord(id) || [0, 0];
  };
  /** F335 布局快照：手动调整后保存，切级别不覆盖。 */
  LayoutStore.prototype.saveManual = function (map) { this.manual = map; };
  LayoutStore.prototype.clearManual = function () { this.manual = null; };
  CA.LayoutStore = LayoutStore;
  CA.InterfaceManager = InterfaceManager;
})(typeof globalThis !== "undefined" ? globalThis : this);
