/* =============================================================================
 * AI-09 · 壳控制器（C10~C12 / C15 / C17 / C19）
 *   一个 core，三个壳：本层负责把「当前跑在哪个壳、哪个视口、哪个风格」
 *   落到 DOM 属性上；功能与布局不因壳/视口/风格隐藏，只换实现层。
 *
 *   - C10 壳探测：?shell= 参数 > 服务器 /api/shell > 默认壳A（对齐 core
 *     detect_shell 的「参数 > 环境 > 默认」优先级，浏览器侧无环境变量）。
 *   - C12 嵌入适配：body[data-viewport] 五档（阈值与 core Viewport::of 一致：
 *     ≥1440 wide / ≥1100 desktop / ≥860 compact / ≥560 narrow / 其余 tiny）。
 *   - C15 键位：宿主键盘事件统一走本层注册表（壳C 由事件循环映射进来）。
 *   - C17 壁纸：8 风格预设与 core wallpaper_for_style 逐位一致，
 *     --wp-* 变量契约与 core wallpaper_css 输出同名同义。
 *   - C19 大项目：壳A 服务器在场时直接消费 /api/ir（core 序列化的统一 IR）。
 * ========================================================================== */
(function (root) {
  "use strict";
  var CA = root.CA || (root.CA = {});
  var doc = root.document;
  if (!doc || !doc.body) return;

  /* ── C17 壁纸预设：style → {mode, dim, blur}（对齐 core wallpaper_for_style）── */
  var WALLPAPER_PRESETS = [
    { style: 0, mode: "scanline",  dim: 0.10, blur: 0 },
    { style: 1, mode: "gradient",  dim: 0.00, blur: 0 },
    { style: 2, mode: "waves",     dim: 0.02, blur: 0 },
    { style: 3, mode: "starfield", dim: 0.05, blur: 0 },
    { style: 4, mode: "mesh",      dim: 0.08, blur: 0 },
    { style: 5, mode: "aurora",    dim: 0.00, blur: 6 },
    { style: 6, mode: "plain",     dim: 0.00, blur: 0 },
    { style: 7, mode: "blueprint", dim: 0.06, blur: 0 }
  ];

  var STYLE_NAMES = ["像素", "现代简约", "清新", "星空", "赛博朋克", "玻璃拟态", "新拟态", "手绘"];
  var SHELL_NAMES = { A: "壳A · Windows 独立", B: "壳B · Variable 嵌入", C: "壳C · VARIX 内核" };
  var STYLE_KEY = "ca-style";

  /* ── C10 壳探测（优先级：URL 参数 > 服务器档案 > 默认壳A） ─────────────── */
  function queryParam(name) {
    try {
      return new URLSearchParams(String(root.location && root.location.search || "")).get(name) || "";
    } catch (e) { return ""; }
  }
  function detectShell() {
    var f = (queryParam("shell") || "").toUpperCase();
    if (f === "B" || f === "EMBED" || f === "VARIABLE") return "B";
    if (f === "C" || f === "VARIX" || f === "KERNEL") return "C";
    if (f === "A" || f === "STANDALONE") return "A";
    return ""; /* 交给服务器档案 / 默认 */
  }

  /* ── C12 视口分档（阈值对齐 core Viewport::of） ────────────────────────── */
  function viewportOf(w) {
    if (w >= 1440) return "wide";
    if (w >= 1100) return "desktop";
    if (w >= 860) return "compact";
    if (w >= 560) return "narrow";
    return "tiny";
  }

  /* ── C17 风格与壁纸 ────────────────────────────────────────────────────── */
  function loadStyle() {
    var q = parseInt(queryParam("style"), 10);
    if (!isNaN(q) && q >= 0 && q < WALLPAPER_PRESETS.length) return q;
    try {
      var v = parseInt(root.localStorage && root.localStorage.getItem(STYLE_KEY), 10);
      if (!isNaN(v) && v >= 0 && v < WALLPAPER_PRESETS.length) return v;
    } catch (e) { /* 嵌入态可能禁 localStorage：走默认 */ }
    return 1; /* 默认现代简约 */
  }
  function saveStyle(i) {
    try { root.localStorage && root.localStorage.setItem(STYLE_KEY, String(i)); } catch (e) { /* 同上 */ }
  }
  function wallpaperVars(preset) {
    /* 与 core wallpaper_css 同名同义：--wp-mode/opacity/blur/dim/speed */
    return "--wp-mode:" + preset.mode + ";--wp-opacity:1;--wp-blur:" + preset.blur +
      "px;--wp-dim:" + preset.dim + ";--wp-speed:1;";
  }

  var Shell = {
    kind: "A",
    styleIndex: 1,
    WALLPAPER_PRESETS: WALLPAPER_PRESETS,
    STYLE_NAMES: STYLE_NAMES,
    viewportOf: viewportOf,
    detectShell: detectShell,
    wallpaperVars: wallpaperVars,

    /* 应用风格：html[data-style] + #wallpaper 的 --wp-* 变量 + 色板选中态 */
    applyStyle: function (i) {
      var n = WALLPAPER_PRESETS.length;
      this.styleIndex = ((i % n) + n) % n;
      var p = WALLPAPER_PRESETS[this.styleIndex];
      doc.documentElement.setAttribute("data-style", String(this.styleIndex));
      var wp = doc.getElementById("wallpaper");
      if (wp) wp.setAttribute("style", wallpaperVars(p));
      if (doc.querySelectorAll) {
        var btns = doc.querySelectorAll(".tl-styles button");
        for (var k = 0; k < btns.length; k++) {
          var on = +btns[k].getAttribute("data-style-i") === this.styleIndex;
          if (on) btns[k].classList.add("on"); else btns[k].classList.remove("on");
        }
      }
      var tag = doc.getElementById("style-name");
      if (tag) tag.textContent = STYLE_NAMES[this.styleIndex];
      var vtag = doc.getElementById("s-style");
      if (vtag) vtag.textContent = STYLE_NAMES[this.styleIndex];
      saveStyle(this.styleIndex);
      return this.styleIndex;
    },
    cycleStyle: function () {
      return this.applyStyle(this.styleIndex + 1);
    },

    /* 视口：body[data-viewport] + 视口徽标 */
    applyViewport: function (w) {
      var v = viewportOf(w == null ? root.innerWidth : w);
      doc.body.setAttribute("data-viewport", v);
      var tag = doc.getElementById("vp-tag");
      if (tag) tag.textContent = v;
      return v;
    },

    /* 壳身份：body[data-shell] + 标题栏徽标 */
    applyShell: function (kind) {
      this.kind = SHELL_NAMES[kind] ? kind : "A";
      doc.body.setAttribute("data-shell", this.kind);
      var badge = doc.getElementById("shell-badge");
      if (badge) {
        badge.textContent = SHELL_NAMES[this.kind];
        badge.setAttribute("data-shell", this.kind);
      }
      return this.kind;
    },

    /* ── 启动：壳探测 → 风格 → 视口 →（壳A 服务器在场）载入统一 IR ───────── */
    boot: function () {
      var self = this;
      this.applyShell(detectShell());
      this.applyStyle(loadStyle());
      this.applyViewport(root.innerWidth);
      root.addEventListener("resize", function () { self.applyViewport(root.innerWidth); });

      /* F 键跟随等键位注册表（C15 三端同表：壳C 事件循环把内核键位事件
       * 翻译成 KeyboardEvent 派发，这里只消费标准事件，三壳等价）。 */
      this.keybinds = [];
      root.addEventListener("keydown", function (e) {
        for (var i = 0; i < self.keybinds.length; i++) {
          var b = self.keybinds[i];
          if (b.match(e)) { b.run(e); }
        }
      });

      /* 壳A 服务器在场（http 且 /api/ping 应答）→ 拉壳档案 + core 序列化 IR；
       * 纯静态伺服 → data/ir.json 兜底；file:// → 内置演示 IR。 */
      if (String(root.location && root.location.protocol || "").indexOf("http") !== 0) return;
      if (typeof fetch !== "function") return;
      fetch("/api/ping", { cache: "no-store" })
        .then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); })
        .then(function (ping) {
          if (!ping || ping.app !== "codeanalysis") throw new Error("not shell-a");
          self.applyShell(detectShell() || "A");
          fetch("/api/shell", { cache: "no-store" })
            .then(function (r) { return r.ok ? r.json() : null; })
            .then(function (profile) {
              if (!profile) return;
              var badge = doc.getElementById("shell-badge");
              if (badge && profile.detected) {
                badge.textContent = profile.detected;
                badge.setAttribute("data-shell", self.kind);
              }
            })
            .catch(function () { /* 壳档案缺失不影响功能 */ });
          /* C19：等装配层就绪后载入 core 产出的统一 IR（覆盖演示数据）。 */
          var tries = 0;
          (function pullIR() {
            if (root.CA_APP) root.CA_APP.loadIRURL("/api/ir");
            else if (tries++ < 20) setTimeout(pullIR, 250);
          })();
        })
        .catch(function () {
          /* 无壳A 服务器：静态伺服兜底本地演示数据。 */
          if (root.CA_APP) root.CA_APP.loadIRURL("data/ir.json");
        });
    }
  };

  CA.Shell = Shell;
  Shell.boot();
})(typeof globalThis !== "undefined" ? globalThis : this);
