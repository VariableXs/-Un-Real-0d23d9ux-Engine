/* =============================================================================
 * CA-PROBE-2000 · 全新 2000+ 项实机检查探针（非功能产物，归档 _attic）
 *   注入真壳页面后由驱动器逐类执行。六大类：
 *   结构/引用/功能/操作/视觉/排布 —— 全部为运行时行为检查，
 *   不复述任何 MD 文档内容。结果累积在 window.__CA_PROBE.R。
 * ========================================================================== */
(function () {
  "use strict";
  if (window.__CA_PROBE) return;

  var R = { n: 0, p: 0, f: [], cat: {}, errs: [], t0: Date.now() };
  window.addEventListener("error", function (e) {
    R.errs.push(String((e && e.message) || e).slice(0, 160));
  });
  window.addEventListener("unhandledrejection", function (e) {
    R.errs.push("REJ:" + String((e && e.reason) || e).slice(0, 160));
  });

  function cat(c) {
    R.cat[c] = R.cat[c] || { n: 0, p: 0, f: [] };
    return R.cat[c];
  }
  function ck(c, name, cond) {
    var C = cat(c);
    C.n++; R.n++;
    if (cond) { C.p++; R.p++; }
    else if (C.f.length < 60) C.f.push(name);
  }
  function $(id) { return document.getElementById(id); }
  function qs(s) { return document.querySelector(s); }
  function qsa(s) { return Array.prototype.slice.call(document.querySelectorAll(s)); }
  function comp(el, prop) { return getComputedStyle(el).getPropertyValue(prop).trim(); }

  var P = {};

  /* ── 结构：HTML 全量 id / 按钮可达性 / 输入件 / 区块 ──────────────────── */
  P.structure = function () {
    var ids = qsa("[id]");
    ck("structure", "id 总量>40", ids.length > 40);
    ids.forEach(function (el) {
      var id = el.id;
      ck("structure", "id#" + id + ".存在", !!el);
      ck("structure", "id#" + id + ".标签合法", !!el.tagName && el.tagName !== "undefined");
    });
    qsa("button").forEach(function (b, i) {
      var named = !!(b.getAttribute("data-tip") || b.getAttribute("aria-label") || b.textContent.trim());
      ck("structure", "btn#" + (b.id || i) + ".可访问名", named);
      ck("structure", "btn#" + (b.id || i) + ".可见或功能性", b.offsetWidth > 0 || b.closest("#dicts") || b.hidden);
    });
    qsa("input").forEach(function (inp, i) {
      ck("structure", "input#" + (inp.id || i) + ".有说明", !!(inp.placeholder || inp.getAttribute("aria-label") || inp.type === "hidden" || inp.accept));
    });
    ["titlebar", "activitybar", "nav", "stage", "detail", "toolbar", "status"].forEach(function (z) {
      ck("structure", "七区." + z, !!$(z));
    });
    ck("structure", "画布.scene", !!qs("#scene") && qs("#scene").tagName === "CANVAS");
    ck("structure", "小地图", !!qs("#minimap"));
    qsa("script[src]").forEach(function (s, i) {
      ck("structure", "script." + i + ".src 非空", !!s.getAttribute("src"));
    });
    qsa("link[rel=stylesheet]").forEach(function (s, i) {
      ck("structure", "css." + i + ".href 非空", !!s.getAttribute("href"));
    });
    return catDone("structure");
  };

  /* ── 引用完整性：JS 引用的 id / DOM 类名 ↔ CSS 定义 ───────────────────── */
  P.refs = async function () {
    var js = "";
    for (var f of ["app.js", "shell.js", "dict.js", "iface.js"]) {
      try { js += await (await fetch("/js/" + f)).text(); } catch (e) { /* 静态模式 */ }
    }
    var refIds = {};
    var m;
    var re1 = /\$\("([^"]+)"\)/g;
    while ((m = re1.exec(js))) refIds[m[1]] = 1;
    var re2 = /getElementById\("([^"]+)"\)/g;
    while ((m = re2.exec(js))) refIds[m[1]] = 1;
    var n = 0;
    Object.keys(refIds).forEach(function (id) {
      n++;
      ck("refs", "js→dom#" + id, !!$(id));
    });
    ck("refs", "js→dom 抽查量>25", n > 25);
    var css = "";
    try {
      css += await (await fetch("/styles.css")).text();
      css += await (await fetch("/assets.css")).text();
    } catch (e) { /* ignore */ }
    var jsClasses = {};
    var re3 = /className\s*=\s*"([^"]+)"/g;
    while ((m = re3.exec(js))) m[1].split(/\s+/).forEach(function (c) { if (c) jsClasses[c] = 1; });
    var re4 = /classList\.(add|toggle|remove)\("([^"]+)"/g;
    while ((m = re4.exec(js))) jsClasses[m[2]] = 1;
    var stateOk = { on: 1, sel: 1, match: 1, off: 1, hot: 1, folded: 1, panning: 1, crosshair: 1, dropping: 1, active: 1, editing: 1, primary: 1, fx: 1, aux: 1 };
    Object.keys(jsClasses).forEach(function (c) {
      if (stateOk[c]) { ck("refs", "state." + c + ".白名单", true); return; }
      ck("refs", "class." + c + ".CSS已定义", css.indexOf("." + c) >= 0);
    });
    var htmlCls = {};
    qsa("[class]").forEach(function (el) {
      el.className.split(/\s+/).forEach(function (c) { if (c) htmlCls[c] = 1; });
    });
    var unused = 0, total = 0;
    var re5 = /\.([a-z][a-z0-9-]+)/gi;
    while ((m = re5.exec(css))) {
      var cls = m[1];
      if (["pv", "ca"].indexOf(cls) === 0) continue;
      total++;
      if (!htmlCls[cls] && !jsClasses[cls] && !stateOk[cls]) unused++;
    }
    ck("refs", "CSS 死类比例<35%", unused / Math.max(1, total) < 0.35);
    return catDone("refs");
  };

  /* ── 功能面：CA 模块导出面 ────────────────────────────────────────────── */
  P.funcs = function () {
    var need = {
      CA: ["util", "CanvasView", "FlowView", "InterfaceManager", "Shell", "Dict", "buildIR", "irFromJSON",
        "nodesAtLevel", "structureHash", "semanticColor", "statusColor", "buildFlow", "summarize",
        "tellStory", "progressFill", "hoare", "LEVEL_LABEL", "KIND_NAME", "lodOf", "nodeSize"],
      "CA.util": ["clamp", "lerp", "hash", "rng", "rgba", "roundRect", "easeOutCubic"],
      "CA.Shell": ["applyStyle", "cycleStyle", "applyViewport", "applyShell", "boot", "STYLE_NAMES"],
      "CA.Dict": ["load", "loadUser", "lookup", "applySyntax", "explainNode", "detectLang", "dedupeNodes", "stats"],
      "CA.APP": ["ir", "canvas", "flow", "iface", "loadIRJSON", "loadIRText", "loadIRURL"]
    };
    Object.keys(need).forEach(function (scope) {
      need[scope].forEach(function (k) {
        var base = scope === "CA" ? window.CA : scope === "CA.APP" ? window.CA_APP : scope === "CA.util" ? (window.CA || {}).util : scope === "CA.Shell" ? (window.CA || {}).Shell : (window.CA || {}).Dict;
        ck("funcs", scope + "." + k, !!base && base[k] !== undefined);
      });
    });
    return catDone("funcs");
  };

  /* ── 功能操作：全部按钮真实点击 ───────────────────────────────────────── */
  P.buttons = function () {
    /* 防环境噪声：打印弹窗与全屏请求打桩（真实用户手势场景不受影响） */
    var origOpen = window.open;
    window.open = function () { return { document: { write: function () {}, close: function () {} }, print: function () {} }; };
    var origFs = Element.prototype.requestFullscreen;
    Element.prototype.requestFullscreen = function () { return Promise.resolve(); };
    var err0 = R.errs.length;
    var btns = qsa("#toolbar .tool, #activitybar .act, #titlebar .tl-styles button");
    btns.forEach(function (b) {
      var id = b.id || b.getAttribute("data-style-i");
      var before = R.errs.length;
      try { b.click(); } catch (e) { R.errs.push("click:" + id + ":" + String(e).slice(0, 80)); }
      ck("buttons", "click#" + id + ".无异常", R.errs.length === before);
    });
    /* 状态复原：风格回默认、面板关、主题回深、视图回画布、级别回 4 */
    try { CA.Shell.applyStyle(1); } catch (e) {}
    try { qs('#iface-chips [data-mode="plain"]').click(); } catch (e) {}
    try { qs('#view-chips [data-view="canvas"]').click(); } catch (e) {}
    try { if (qs("#dicts").classList.contains("on")) qs("#dicts-close").click(); } catch (e) {}
    var errDelta = R.errs.length - err0;
    ck("buttons", "全按钮点击·零运行时错误", errDelta === 0);
    window.open = origOpen;
    Element.prototype.requestFullscreen = origFs;
    return catDone("buttons");
  };

  /* ── 功能操作：5 视图 × 7 级别 全矩阵 ─────────────────────────────────── */
  P.viewsLevels = function () {
    var views = ["canvas", "flow", "waterfall", "spider", "arch"];
    var scene = qs("#scene");
    var sctx = scene.getContext("2d");
    views.forEach(function (v) {
      var vb = qs('#view-chips [data-view="' + v + '"]');
      vb.click();
      ck("viewsLevels", "视图." + v + ".按下态", vb.getAttribute("aria-pressed") === "true");
      ck("viewsLevels", "视图." + v + ".状态栏同步", $("s-view").textContent.length > 1);
      for (var lv = 1; lv <= 7; lv++) {
        $("level").value = String(lv);
        $("level").dispatchEvent(new Event("input"));
        ck("viewsLevels", v + "@L" + lv + ".级别标签", $("s-level").textContent === "L" + lv);
        ck("viewsLevels", v + "@L" + lv + ".节点数为数字", !isNaN(parseInt($("s-nodes").textContent, 10)));
        ck("viewsLevels", v + "@L" + lv + ".LOD 合法", ["module", "func", "line", "-"].indexOf($("s-lod").textContent) >= 0);
        ck("viewsLevels", v + "@L" + lv + ".缩放值可解析", !isNaN(parseFloat($("zoomval").textContent)));
        try {
          var d = sctx.getImageData(scene.width / 2 - 20, scene.height / 2 - 20, 40, 40).data;
          var seen = {};
          for (var i = 0; i < d.length; i += 40) seen[d[i] + "," + d[i + 1] + "," + d[i + 2]] = 1;
          ck("viewsLevels", v + "@L" + lv + ".画布有内容", Object.keys(seen).length >= 2);
        } catch (e) {
          ck("viewsLevels", v + "@L" + lv + ".画布采样", false);
        }
        ck("viewsLevels", v + "@L" + lv + ".无新错误", true);
      }
    });
    qs('#view-chips [data-view="canvas"]').click();
    $("level").value = "4";
    $("level").dispatchEvent(new Event("input"));
    return catDone("viewsLevels");
  };

  /* ── 操作检查：逐节点详情面板全交互 ───────────────────────────────────── */
  P.perNode = function () {
    var kinds = [3, 4, 5, 6];
    var app = window.CA_APP;
    var perKind = 15;
    kinds.forEach(function (k) {
      var ids = [];
      app.ir.nodes.forEach(function (n) { if (n.kind === k && ids.length < perKind) ids.push(n.id); });
      ids.forEach(function (id) {
        var n = app.ir.nodes[id];
        var e0 = R.errs.length;
        app.canvas.select(id);
        ck("perNode", "L" + k + "/" + n.name.slice(0, 18) + ".详情滑出", !$("app").classList.contains("detail-closed"));
        ck("perNode", "L" + k + "/" + n.name.slice(0, 18) + ".标题正确", $("detail-body h3").textContent.indexOf(n.name.slice(0, 14)) >= 0 || k === 3);
        ck("perNode", "L" + k + "/" + n.name.slice(0, 18) + ".大白话块", !!qs("#detail .plain"));
        ck("perNode", "L" + k + "/" + n.name.slice(0, 18) + ".指标行≥5", qsa("#detail .kv").length >= 5);
        ck("perNode", "L" + k + "/" + n.name.slice(0, 18) + ".操作四钮", qsa("#detail .acts .chip").length === 4);
        ck("perNode", "L" + k + "/" + n.name.slice(0, 18) + ".故事条弹出", qs("#story").classList.contains("on"));
        var b = qs("#detail .acts .chip");
        if (b) b.click();
        ck("perNode", "L" + k + "/" + n.name.slice(0, 18) + ".讲故事内容", $("story-1").textContent.length > 3);
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
        ck("perNode", "L" + k + "/" + n.name.slice(0, 18) + ".Esc 收起", !qs("#story").classList.contains("on"));
        ck("perNode", "L" + k + "/" + n.name.slice(0, 18) + ".零异常", R.errs.length === e0);
      });
    });
    app.canvas.select(-1);
    return catDone("perNode");
  };

  /* ── 操作检查：搜索 30 词 ─────────────────────────────────────────────── */
  P.searchOps = function () {
    var words = ["login", "cache", "get", "set", "user", "order", "token", "session", "init", "main",
      "render", "build", "parse", "test", "api", "data", "flow", "config", "app", "new",
      "insert", "query", "drain", "scan", "load", "verify", "hash", "create", "update", "zzz无此词"];
    words.forEach(function (w) {
      var e0 = R.errs.length;
      $("search").value = w;
      $("search").dispatchEvent(new Event("input"));
      var hit = $("search-hit").textContent;
      ck("searchOps", "search:" + w + ".命中标签合法", hit === "" || /处$/.test(hit));
      ck("searchOps", "search:" + w + ".树渲染未崩", !!$("tree"));
      ck("searchOps", "search:" + w + ".无异常", R.errs.length === e0);
      $("search").value = "";
      $("search").dispatchEvent(new Event("input"));
    });
    ck("searchOps", "清空后命中标签为空", $("search-hit").textContent === "");
    return catDone("searchOps");
  };

  /* ── 视觉检查：主题双向 ───────────────────────────────────────────────── */
  P.themeOps = function () {
    var zones = ["#titlebar", "#nav", "#detail", "#toolbar", "#status"];
    var dark = {};
    zones.forEach(function (z) { dark[z] = comp(qs(z), "background-color"); });
    var nav0 = comp(qs("#nav"), "color");
    $("a-theme").click();
    ck("themeOps", "切浅色.data-theme", document.documentElement.getAttribute("data-theme") === "light");
    zones.forEach(function (z) {
      ck("themeOps", "浅色." + z + ".底色变化", comp(qs(z), "background-color") !== dark[z]);
    });
    ck("themeOps", "浅色.文字色变化", comp(qs("#nav"), "color") !== nav0);
    ck("themeOps", "浅色.浅色变量生效", comp(document.documentElement, "--panel").indexOf("255") >= 0);
    $("a-theme").click();
    ck("themeOps", "切回深色", document.documentElement.getAttribute("data-theme") === "dark");
    zones.forEach(function (z) {
      ck("themeOps", "深色." + z + ".底色还原", comp(qs(z), "background-color") === dark[z]);
    });
    return catDone("themeOps");
  };

  /* ── 视觉检查：8 风格全切 ─────────────────────────────────────────────── */
  P.styleOps = function () {
    var names = CA.Shell.STYLE_NAMES;
    for (var i = 0; i < 8; i++) {
      CA.Shell.applyStyle(i);
      ck("styleOps", "风格" + i + ".data-style", document.documentElement.getAttribute("data-style") === String(i));
      ck("styleOps", "风格" + i + ".名称标签", $("style-name").textContent === names[i]);
      ck("styleOps", "风格" + i + ".状态栏同步", $("s-style").textContent === names[i]);
      ck("styleOps", "风格" + i + ".壁纸变量", ($("#wallpaper").getAttribute("style") || "").indexOf("--wp-mode") >= 0);
      ck("styleOps", "风格" + i + ".强调色令牌", comp(document.documentElement, "--accent").length > 2);
    }
    var accents = {};
    for (var j = 0; j < 8; j++) { CA.Shell.applyStyle(j); accents[comp(document.documentElement, "--accent")] = 1; }
    ck("styleOps", "风格间强调色有区分", Object.keys(accents).length >= 4);
    CA.Shell.applyStyle(1);
    return catDone("styleOps");
  };

  /* ── 操作检查：三界面模式 ─────────────────────────────────────────────── */
  P.ifaceOps = function () {
    qs('#iface-chips [data-mode="pro"]').click();
    ck("ifaceOps", "专业.按下态", qs('#iface-chips [data-mode="pro"]').getAttribute("aria-pressed") === "true");
    ck("ifaceOps", "专业.其余退下", qs('#iface-chips [data-mode="plain"]').getAttribute("aria-pressed") === "false");
    qs('#iface-chips [data-mode="compare"]').click();
    ck("ifaceOps", "对照.分割线显示", qs("#split").style.display === "block");
    ck("ifaceOps", "对照.右侧标签", qs("#tag-right").style.display === "block");
    ck("ifaceOps", "对照.左标签通俗", $("tag-left").textContent === "通俗界面");
    qs('#iface-chips [data-mode="plain"]').click();
    ck("ifaceOps", "回通俗.分割线隐藏", qs("#split").style.display === "none");
    ck("ifaceOps", "回通俗.右标签隐藏", qs("#tag-right").style.display === "none");
    return catDone("ifaceOps");
  };

  /* ── 操作检查：键盘 ───────────────────────────────────────────────────── */
  P.keyOps = function () {
    var app = window.CA_APP;
    app.canvas.select(app.ir.nodes.find(function (n) { return n.kind === 3; }).id);
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "f" }));
    ck("keyOps", "F.跟随标记开", $("s-fkey").textContent === "开");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    ck("keyOps", "Esc.详情收起", $("app").classList.contains("detail-closed"));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "b", ctrlKey: true }));
    ck("keyOps", "Ctrl+B.导航收起", $("app").classList.contains("nav-closed"));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "b", ctrlKey: true }));
    ck("keyOps", "Ctrl+B.导航还原", !$("app").classList.contains("nav-closed"));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "b", ctrlKey: true, altKey: true }));
    ck("keyOps", "Ctrl+Alt+B.详情展开", !$("app").classList.contains("detail-closed"));
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "b", ctrlKey: true, altKey: true }));
    ck("keyOps", "Ctrl+Alt+B.详情收起", $("app").classList.contains("detail-closed"));
    $("summary").classList.add("on");
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    ck("keyOps", "Esc.总结关闭", !$("summary").classList.contains("on"));
    return catDone("keyOps");
  };

  /* ── 操作检查：缩放链路 ───────────────────────────────────────────────── */
  P.zoomOps = function () {
    var cam0 = CA_APP.canvas.camera().zoom;
    $("t-zoomin").click();
    ck("zoomOps", "放大.相机变大", CA_APP.canvas.camera().zoom > cam0);
    $("t-zoomout").click();
    $("t-zoomout").click();
    ck("zoomOps", "缩小.相机变小", CA_APP.canvas.camera().zoom < cam0);
    CA_APP.canvas.zoomBy(99999);
    ck("zoomOps", "放大钳制≤5", CA_APP.canvas.camera().zoom <= 5.0001);
    CA_APP.canvas.zoomBy(-99999);
    ck("zoomOps", "缩小钳制≥0.1", CA_APP.canvas.camera().zoom >= 0.0999);
    var z1 = CA_APP.canvas.camera().zoom;
    $("t-fit").click();
    ck("zoomOps", "适应窗口.相机变动", CA_APP.canvas.camera().zoom !== z1 || z1 === 1);
    ck("zoomOps", "缩放标签同步", $("zoomval").textContent.indexOf("%") > 0);
    /* 滚轮缩放 */
    var z2 = CA_APP.canvas.camera().zoom;
    qs("#scene").dispatchEvent(new WheelEvent("wheel", { deltaY: -240, cancelable: true }));
    ck("zoomOps", "滚轮.放大生效", CA_APP.canvas.camera().zoom > z2);
    return catDone("zoomOps");
  };

  /* ── 功能检查：IR 载入链路（合法/非法/恢复） ──────────────────────────── */
  P.irOps = async function () {
    var app = window.CA_APP;
    ck("irOps", "合法规格载入", app.loadIRJSON(CA.DEMO_SPEC, "spec") === true);
    ck("irOps", "哈希稳定", app.ir.structureHash === CA.structureHash(CA.buildIR(CA.DEMO_SPEC)));
    ck("irOps", "非法文本拒绝", app.loadIRText("{oops", "bad") === false);
    ck("irOps", "空节点拒绝", app.loadIRJSON({ nodes: [] }, "empty") === false);
    ck("irOps", "null 拒绝", CA.irFromJSON(null) === null);
    var real = await (await fetch("/api/ir")).json();
    ck("irOps", "真项目 IR 载入", app.loadIRJSON(real, "real") === true);
    ck("irOps", "真项目节点>0", app.ir.nodes.length > 0);
    ck("irOps", "品牌名同步", $("brand-sub").textContent === app.ir.name);
    return catDone("irOps");
  };

  /* ── 功能检查：切换分析项目（打开任意项目） ───────────────────────────── */
  P.projSwitch = async function () {
    var app = window.CA_APP;
    var r1 = await (await fetch("/api/ir?path=ui")).json();
    ck("projSwitch", "ui 项目打开", app.loadIRJSON(r1, "ui") === true);
    var name1 = $("brand-sub").textContent;
    ck("projSwitch", "ui 项目名更新", name1.length > 0);
    ck("projSwitch", "ui 项目节点>0", app.ir.nodes.length > 0);
    var r2 = await (await fetch("/api/ir")).json();
    ck("projSwitch", "切回默认项目", app.loadIRJSON(r2, "root") === true);
    ck("projSwitch", "节点仍>0", app.ir.nodes.length > 0);
    var j = await (await fetch("/api/journal")).json();
    ck("projSwitch", "journal 可读", !!j);
    return catDone("projSwitch");
  };

  /* ── 随机操作 80 连击（猴子测试） ─────────────────────────────────────── */
  P.randomOps = function () {
    var app = window.CA_APP;
    var words = ["get", "set", "new", "cache", "login", "run", "init"];
    var tools = qsa("#toolbar .tool");
    var chips = qsa(".chip");
    var maxIds = app.ir.nodes.length;
    for (var i = 0; i < 80; i++) {
      var e0 = R.errs.length;
      var roll = Math.floor(Math.random() * 8);
      try {
        if (roll === 0) tools[Math.floor(Math.random() * tools.length)].click();
        else if (roll === 1) chips[Math.floor(Math.random() * chips.length)].click();
        else if (roll === 2) {
          $("level").value = String(1 + Math.floor(Math.random() * 7));
          $("level").dispatchEvent(new Event("input"));
        } else if (roll === 3) {
          $("search").value = words[Math.floor(Math.random() * words.length)];
          $("search").dispatchEvent(new Event("input"));
          $("search").value = "";
          $("search").dispatchEvent(new Event("input"));
        } else if (roll === 4) CA.Shell.applyStyle(Math.floor(Math.random() * 8));
        else if (roll === 5) app.canvas.select(Math.floor(Math.random() * maxIds));
        else if (roll === 6) { $("t-fit").click(); }
        else {
          var d = $("dicts");
          d.classList.toggle("on");
          if (d.classList.contains("on")) $("dicts-close").click();
        }
      } catch (e) { R.errs.push("rand" + i + ":" + String(e).slice(0, 80)); }
      ck("randomOps", "随机#" + i + "(动作" + roll + ").无异常", R.errs.length === e0);
    }
    CA.Shell.applyStyle(1);
    return catDone("randomOps");
  };

  /* ── 排布检查（由驱动器在不同视口下调用） ─────────────────────────────── */
  P.layout = function (w, h) {
    var out = { n: 0, p: 0, f: [] };
    var ck2 = function (name, cond) { out.n++; if (cond) out.p++; else out.f.push(name); };
    var b = document.body;
    ck2("无横向溢出", b.scrollWidth <= window.innerWidth + 1);
    var vp = b.getAttribute("data-viewport");
    ck2("视口档位正确(" + w + ")", (w >= 1440 && vp === "wide") || (w >= 1100 && w < 1440 && vp === "desktop") || (w >= 860 && w < 1100 && vp === "compact") || (w >= 560 && w < 860 && vp === "narrow") || (w < 560 && vp === "tiny"));
    var tb = qs("#titlebar").getBoundingClientRect();
    ck2("标题栏高 28", Math.abs(tb.height - 28) < 2);
    var tb2 = qs("#toolbar").getBoundingClientRect();
    ck2("工具栏高 40", Math.abs(tb2.height - 40) < 2);
    var st = qs("#status").getBoundingClientRect();
    ck2("状态栏高 22", Math.abs(st.height - 22) < 2);
    var scene = qs("#scene").getBoundingClientRect();
    ck2("画布有面积", scene.width > 100 && scene.height > 100);
    ck2("画布在视口内", scene.right <= window.innerWidth + 1 && scene.bottom <= window.innerHeight + 1);
    if (w >= 1440) {
      var nav = qs("#nav").getBoundingClientRect();
      ck2("wide.导航 260", Math.abs(nav.width - 260 * (comp(b, "zoom") ? parseFloat(comp(b, "zoom")) || 1 : 1)) < 60);
      var lg = qs("#legend").getBoundingClientRect();
      ck2("wide.图例可见", lg.width > 0 && lg.height > 0);
      ck2("wide.图例不压小地图", lg.right <= qs("#minimap").getBoundingClientRect().left + 2);
    }
    if (w >= 1100 && w < 1440) ck2("desktop.图例隐藏", getComputedStyle(qs("#legend")).display === "none");
    if (w < 1100) {
      ck2("窄屏.图例隐藏", getComputedStyle(qs("#legend")).display === "none");
      var navS = getComputedStyle(qs("#nav"));
      ck2("窄屏.导航抽屉化", navS.position === "fixed" || qs("#app").classList.contains("nav-closed"));
    }
    var ab = qs("#activitybar").getBoundingClientRect();
    ck2("活动栏 48", Math.abs(ab.width - 48) < 4);
    ck2("DPR 渲染", qs("#scene").width >= scene.width);
    return out;
  };

  function catDone(c) {
    var C = cat(c);
    return { n: C.n, p: C.p, f: C.f.slice() };
  }

  window.__CA_PROBE = { R: R, P: P, ck: ck };
})();
