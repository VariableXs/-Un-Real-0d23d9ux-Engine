/* ═══════════════════════════════════════════════════════════════════════════
 * v4 · 装配层：功能区（F315~F320）+ 三界面切换（F301~F303）+ 极简交互（F326）
 *   把画布视图 / 流程图视图 / 界面管理器接到同一棵 IR 树上。
 *   「三界面共享同一个画布容器和同一棵 AST 树：切换时形状/位置/连线完全不变。」
 *   v4 新增：可折叠目录树（chevron + 类型徽章）、卡片化详情面板（指标网格 +
 *   调用关系 chips）、Ctrl+K 命令面板（全节点跳转 + 指令直达）、动态图例、
 *   状态栏 FPS/边数、视图切换过渡、主题联动流程图。
 * ═══════════════════════════════════════════════════════════════════════════ */
(function (root) {
  "use strict";
  var CA = root.CA;
  var U = CA.util;
  var $ = function (id) { return document.getElementById(id); };

  var KIND_ZH = ["项目", "模块", "子系统", "文件", "类", "函数", "行"];

  var ir = CA.buildIR(CA.DEMO_SPEC);
  var iface = new CA.InterfaceManager();
  iface.level = 2; iface.selected = -1; iface.expanded = {};

  var state = {
    view: "canvas",
    theme: "dark",
    split: 0.5,
    bookmarks: [],
    understood: {},
    search: "",
    pin: false,
    hash: ir.structureHash,
    expanded: {}
  };

  var active = { canvas: true, flow: false };
  var canvasEl = $("scene");
  /* 状态栏脏检查缓存 —— 必须先于视图构造（setLevel 回调会同步触发 updateStatus） */
  var statusCache = {};

  var canvasView = new CA.CanvasView(canvasEl, {
    minimap: $("minimap"),
    isActive: function () { return active.canvas; },
    onSelect: onSelect,
    onHover: onHover,
    onStatus: updateStatus
  });
  canvasView.setIR(ir);
  canvasView.setLevel(2);

  var flowView = new CA.FlowView(canvasEl, {
    minimap: $("minimap"),
    isActive: function () { return active.flow; },
    funcId: function () { return currentFuncId(); },
    onSelect: function (id, node) {
      if (node) showStory(node.label, node.note || "处理一个步骤", "后面的步骤需要它的结果");
    },
    onEdit: function () { verifyStructure(); }
  });
  flowView.setIR(ir);

  canvasView.start();
  flowView.start();

  /* ── F315 载入真实工程 IR（?ir=<url> / data/ir.json / 选择文件 / 拖拽） ── */
  function toast(msg) {
    var t = $("toast");
    if (!t) return;
    t.textContent = msg;
    t.classList.add("on");
    clearTimeout(toast._t);
    toast._t = setTimeout(function () { t.classList.remove("on"); }, 2800);
  }
  function applyIR(next, label) {
    if (!next || !next.nodes || !next.nodes.length) { toast("IR 数据为空或结构不符"); return false; }
    ir = next;
    state.hash = ir.structureHash;
    state.understood = {};
    state.bookmarks = [];
    state.search = "";
    state.expanded = {};
    if ($("search")) $("search").value = "";
    iface.selected = -1; iface.expanded = {};
    canvasView.setIR(ir);
    flowView.setIR(ir);
    canvasView.setLevel(iface.level);
    canvasView.setMode(iface.mode);
    $("brand-sub").textContent = ir.name;
    $("s-hash").style.color = "";
    closeDetail();
    renderTree();
    renderLegend();
    updateProgress();
    setView(state.view);
    if (label) toast("已载入 " + label + " · " + ir.nodes.length + " 节点 / " + ir.edges.length + " 边");
    root.CA_APP.ir = ir;
    return true;
  }
  function loadIRText(text, label) {
    var json;
    try { json = JSON.parse(text); }
    catch (e) { toast("IR JSON 解析失败：" + e.message); return false; }
    return applyIR(CA.irFromJSON(json), label);
  }
  function readFile(f, label) {
    if (!f) return;
    var r = new FileReader();
    r.onload = function () { loadIRText(String(r.result), label || f.name); };
    r.readAsText(f);
  }
  function loadIRURL(url) {
    if (typeof fetch !== "function") return;
    fetch(url, { cache: "no-store" })
      .then(function (r) { if (!r.ok) throw new Error(r.status); return r.json(); })
      .then(function (j) { applyIR(CA.irFromJSON(j), url); })
      .catch(function () { /* 没有外部数据就用内置演示 IR */ });
  }

  /* ── 当前函数上下文（流程图用） ─────────────────────────────────────── */
  function currentFuncId() {
    var sel = canvasView.getSelected();
    if (sel >= 0 && ir.nodes[sel].kind === 5) return sel;
    var fns = CA.nodesAtLevel(ir, 6);
    return fns.length ? fns[0].id : (ir.byName.placeOrder || 0);
  }

  /* ── F320 状态栏（带脏检查：渲染循环每帧回调，不变不写 DOM） ────────── */
  var lastStatus = null;
  function put(id, v) {
    if (statusCache[id] === v) return;
    statusCache[id] = v;
    var el = $(id);
    if (el) el.textContent = v;
  }
  function updateStatus(st) {
    if (st && st.zoom != null) lastStatus = st;
    var lv = iface.level;
    put("s-level", "L" + lv);
    put("s-nodes", st && st.nodes != null ? st.nodes : CA.nodesAtLevel(ir, lv).length);
    put("s-files", ir.meta.fileCount);
    put("s-loc", ir.meta.loc);
    put("s-edges", ir.edges.length);
    put("s-lod", lastStatus ? lastStatus.lod : "-");
    put("s-fps", lastStatus && lastStatus.fps != null ? lastStatus.fps : "—");
    put("s-hash", state.hash.toString(16).slice(0, 8));
    put("zoomval", Math.round((lastStatus ? lastStatus.zoom : 1) * 100) + "%");
  }

  /* ── F315 目录树（v4：折叠 chevron + 类型徽章 + 引导线层级） ────────── */
  function defaultExpanded(depth) { return depth <= 2; }   /* 项目/模块/文件默认展开 */
  function renderTree() {
    var host = $("tree");
    host.innerHTML = "";
    var q = state.search.trim().toLowerCase();
    var hits = 0;
    (function walk(id, depth) {
      var n = ir.nodes[id];
      var match = q && (n.name.toLowerCase().indexOf(q) >= 0 || (n.plain || "").toLowerCase().indexOf(q) >= 0);
      if (match) hits++;
      var isOpen = q ? true : (state.expanded[id] != null ? state.expanded[id] : defaultExpanded(depth));
      var hasKids = n.children.length > 0 && n.kind < 6;
      var row = document.createElement("div");
      row.className = "tree-row" + (match ? " match" : "") + (canvasView.getSelected() === id ? " sel" : "");
      row.style.paddingLeft = 4 + depth * 11 + "px";
      row.innerHTML =
        '<i class="chev' + (hasKids ? (isOpen ? "" : " closed") : " leaf") + '" data-chev="' + id + '">▶</i>' +
        '<i class="dot" style="background:' + (n.domain ? CA.semanticColor(n.domain) : "#8E8E93") + '"></i>' +
        '<span class="nm">' + esc(n.name) + "</span>" +
        (n.kind >= 3 && n.kind <= 5 ? '<span class="kd">' + KIND_ZH[n.kind] + "</span>" : "") +
        '<span class="star' + (state.bookmarks.indexOf(id) >= 0 ? " on" : "") + '" data-star="' + id + '">★</span>';
      row.title = n.plain || n.name;
      row.addEventListener("click", function (e) {
        var t = e.target;
        if (t.dataset && t.dataset.star != null) { toggleBookmark(+t.dataset.star); return; }
        if (t.dataset && t.dataset.chev != null && hasKids) {
          state.expanded[id] = !isOpen;
          renderTree();
          return;
        }
        jumpTo(id);
      });
      host.appendChild(row);
      if (hasKids && isOpen) n.children.forEach(function (c) { walk(c, depth + 1); });
    })(ir.root, 0);
    put2("search-hit", q ? hits + " 处" : "");
    renderBookmarks();
  }
  function put2(id, v) { var el = $(id); if (el && el.textContent !== v) el.textContent = v; }
  function esc(s) {
    return String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }
  function jumpTo(id) {
    var n = ir.nodes[id];
    iface.selected = id;
    if (n.kind + 1 !== iface.level) { iface.level = U.clamp(n.kind + 1, 1, 7); $("level").value = iface.level; syncLevelUI(); }
    if (state.view === "canvas") {
      canvasView.setLevel(iface.level);
      canvasView.setMode(iface.mode === "compare" ? "compare" : iface.mode);
      canvasView.select(id);
      canvasView.focus(id);
    } else if (n.kind === 5 || n.kind === 6) {
      var fid = n.kind === 6 ? n.parent : id;
      flowView.setChart(CA.buildFlow(ir, fid));
      selectNode(fid);
    }
    renderTree();
  }
  function syncLevelUI() {
    var lv = $("level");
    if (!lv) return;
    lv.value = iface.level;
    if (typeof lv.style.setProperty === "function") {
      lv.style.setProperty("--lv-pct", ((iface.level - 1) / 6 * 100).toFixed(1) + "%");
    }
    put2("level-tag", CA.LEVEL_LABEL[iface.level - 1]);
  }

  /* ── 书签 ───────────────────────────────────────────────────────────── */
  function toggleBookmark(id) {
    var i = state.bookmarks.indexOf(id);
    if (i >= 0) state.bookmarks.splice(i, 1); else state.bookmarks.push(id);
    renderTree();
  }
  function renderBookmarks() {
    var host = $("bookmarks");
    host.innerHTML = "";
    if (!state.bookmarks.length) {
      var s = document.createElement("span");
      s.className = "canvas-tip";
      s.textContent = "点节点旁的 ★ 收藏";
      host.appendChild(s);
      return;
    }
    state.bookmarks.forEach(function (id) {
      var b = document.createElement("button");
      b.className = "chip";
      b.textContent = ir.nodes[id].name;
      b.addEventListener("click", function () { jumpTo(id); });
      host.appendChild(b);
    });
  }

  /* ── 动态图例：域色取自真实 IR（模块级 domain），状态色固定 ─────────── */
  function renderLegend() {
    var host = $("legend-body");
    if (!host) return;
    var seen = {}, rows = [];
    ir.nodes.forEach(function (n) {
      if (n.kind !== 1 || !n.domain || seen[n.domain]) return;
      seen[n.domain] = 1;
      rows.push('<div class="li"><i class="sw" style="background:' + CA.semanticColor(n.domain) + '"></i>' + esc(n.domain) + "模块</div>");
    });
    if (!rows.length) {
      seen = {};
      ir.nodes.forEach(function (n) {
        if (n.kind !== 3 || !n.domain || seen[n.domain]) return;
        seen[n.domain] = 1;
        rows.push('<div class="li"><i class="sw" style="background:' + CA.semanticColor(n.domain) + '"></i>' + esc(n.domain) + "</div>");
      });
    }
    rows.push('<div class="li"><i class="sw" style="background:#FF3B30"></i>bug / 红</div>');
    rows.push('<div class="li"><i class="sw" style="background:#8E8E93"></i>死代码 / 灰</div>');
    rows.push('<div class="li"><i class="lb"></i>线宽 = 调用频率</div>');
    host.innerHTML = rows.join("");
  }

  /* ── F317 详情区（v4：卡片化指标网格 + 调用关系 chips） ─────────────── */
  function onSelect(id) {
    selectNode(id);
    renderTree();
  }
  function selectNode(id) {
    iface.selected = id;
    if (id < 0) { closeDetail(); return; }
    var n = ir.nodes[id];
    state.understood[id] = 1;
    var host = $("detail-body");
    var statusText = { bug: "bug（红）", warn: "警告（黄）", dead: "死代码（灰）", "": "正常（绿）" }[n.status || ""];
    var dictSec = CA.Dict ? CA.Dict.explainNode(n) : "";
    var sigText = n.sig && CA.Dict ? CA.Dict.applySyntax(n.sig, ir.lang) : (n.sig || "");
    var outs = ir.edges.filter(function (e) { return e.from === id; });
    var ins = ir.edges.filter(function (e) { return e.to === id; });
    function relChips(list, isOut) {
      if (!list.length) return "";
      var chips = list.slice(0, 6).map(function (e) {
        var other = ir.nodes[isOut ? e.to : e.from];
        return '<button class="chip" data-jump="' + other.id + '">' + esc(other.name) + "</button>";
      }).join("");
      var more = list.length > 6 ? '<span class="canvas-tip">+' + (list.length - 6) + "</span>" : "";
      return '<div class="rl">' + (isOut ? "调用 →" : "← 被调") + " " + list.length + "</div>" +
        '<div class="chips">' + chips + "</div>" + more;
    }
    host.innerHTML =
      "<h3>" + (n.icon || "") + " " + esc(n.name) + "</h3>" +
      '<div class="plain">' + esc(n.plain || n.name) + "</div>" +
      dictSec +
      (sigText ? '<div class="sig">' + esc(sigText) + "</div>" : "") +
      '<div class="metrics">' +
      mcard("层级", KIND_ZH[n.kind] || n.kindName) +
      mcard("代码行", n.loc) +
      mcard("圈复杂度", n.cc || "—") +
      mcard("语言", CA.Dict ? CA.Dict.detectLang(n.name, n.sig || n.plain || "") : (ir.lang || "—")) +
      mcard("覆盖率", Math.round((n.cov || 0) * 100) + "%") +
      mcard("调用热度", Math.round((n.hot || 0) * 100) + "%") +
      "</div>" +
      bar((n.cov || 0), "#5B9DFF") +
      bar((n.hot || 0), "#FF9500") +
      kv("所属", n.domain ? n.domain + " · " + CA.lifeAnalogy(n.domain) : "—") +
      kv("状态", statusText) +
      kv("调用出边", outs.length) +
      kv("调用入边", ins.length) +
      '<div class="rel">' + relChips(outs, true) + relChips(ins, false) + "</div>" +
      '<div class="acts">' +
      '<button class="chip primary" id="d-story">讲故事</button>' +
      '<button class="chip" id="d-down">下一级</button>' +
      '<button class="chip" id="d-focus">定位</button>' +
      '<button class="chip" id="d-mark">书签</button>' +
      "</div>";
    host.querySelectorAll("[data-jump]").forEach(function (b) {
      b.addEventListener("click", function () { jumpTo(+b.getAttribute("data-jump")); });
    });
    $("app").classList.remove("detail-closed");
    $("a-detail").setAttribute("aria-pressed", "true");
    $("d-story").onclick = function () {
      showStory(n.name, n.plain || "处理一个步骤", "后面的步骤需要它的结果");
    };
    $("d-down").onclick = function () { jumpTo(n.children.length ? n.children[0] : id); };
    $("d-focus").onclick = function () { canvasView.focus(id); };
    $("d-mark").onclick = function () { toggleBookmark(id); };
    /* F308 选中即讲故事 */
    showStory(n.name, n.plain || "处理一个步骤", "后面的步骤需要它的结果");
    updateProgress();
  }
  function mcard(k, v) {
    return '<div class="m"><span>' + k + "</span><b>" + esc(String(v)) + "</b></div>";
  }
  function kv(k, v) {
    return '<div class="kv"><span>' + k + "</span><b>" + esc(String(v)) + "</b></div>";
  }
  function bar(v, color) {
    return '<div class="bar"><i style="width:' + Math.round(v * 100) + "%;background:" + color + ';color:' + color + '"></i></div>';
  }
  function closeDetail() {
    $("app").classList.add("detail-closed");
    if ($("a-detail")) $("a-detail").setAttribute("aria-pressed", "false");
    $("detail-body").innerHTML = '<div class="empty"><i></i>点画布上的节点，这里会滑出它的大白话解释、指标和操作。</div>';
  }

  /* ── F308 点击讲故事 / F311 一键总结 / F310 进度条 ──────────────────── */
  function showStory(name, what, why) {
    var s = CA.tellStory(name, what, why);
    $("story-1").textContent = s[0];
    $("story-2").textContent = s[1];
    $("story-3").textContent = s[2];
    $("story").classList.add("on");
  }
  function updateProgress() {
    var total = CA.nodesAtLevel(ir, iface.level).length || 1;
    var got = Object.keys(state.understood).filter(function (k) { return ir.nodes[k] && ir.nodes[k].kind === iface.level - 1; }).length;
    $("progress").firstElementChild.style.width = (CA.progressFill(got, total) * 100).toFixed(1) + "%";
  }

  /* ── 悬停：F313 Hoare 三元组（专业界面）+ 通用浮层 ──────────────────── */
  function onHover(id, pos) {
    var tip = $("tip"), hoare = $("hoare");
    if (id < 0) { tip.classList.remove("on"); hoare.classList.remove("on"); return; }
    var n = ir.nodes[id];
    tip.innerHTML = "<b>" + esc(n.name) + "</b><br/><span class='k'>" + esc(n.plain || "") + "</span>" +
      (n.cc ? "<br/><span class='k'>CC " + n.cc + " · 覆盖 " + Math.round((n.cov || 0) * 100) + "% · 热度 " + Math.round((n.hot || 0) * 100) + "%</span>" : "");
    tip.style.left = Math.min(pos.x + 14, (canvasEl.clientWidth || 800) - 270) + "px";
    tip.style.top = pos.y + 14 + "px";
    tip.classList.add("on");
    if (iface.mode !== "plain" && n.kind >= 5) {
      hoare.textContent = CA.hoare("req.valid", n.name + "()", "reply.ok");
      hoare.style.left = Math.min(pos.x + 14, (canvasEl.clientWidth || 800) - 270) + "px";
      hoare.style.top = pos.y + 52 + "px";
      hoare.classList.add("on");
    } else hoare.classList.remove("on");
  }

  /* ── F301~F303 三界面切换 ───────────────────────────────────────────── */
  function setMode(m) {
    iface.switch(m);
    state.split = 0.5;
    var isCompare = m === "compare";
    canvasView.setMode(m);
    canvasView.setCompare(isCompare, 0.5);
    $("split").style.display = isCompare ? "block" : "none";
    $("split").style.left = "50%";
    $("tag-left").textContent = "通俗界面";
    $("tag-right").style.display = isCompare ? "block" : "none";
    $("tag-right").style.left = "52%";
    Array.prototype.forEach.call($("iface-chips").children, function (b) {
      b.setAttribute("aria-pressed", String(b.dataset.mode === m));
    });
    renderTree();
  }

  /* ── 视图切换（画布 / 流程图 / 瀑布 / 蛛网 / 架构）+ 交叉淡入过渡 ──── */
  var VIEW_ZH = { canvas: "全景画布", flow: "流程图", waterfall: "调用瀑布", spider: "变量蛛网", arch: "模块架构" };
  function setView(v) {
    state.view = v;
    active.canvas = v === "canvas";
    active.flow = v !== "canvas";
    put2("s-view", VIEW_ZH[v] || v);
    Array.prototype.forEach.call($("view-chips").children, function (b) {
      b.setAttribute("aria-pressed", String(b.dataset.view === v));
    });
    if (active.flow) {
      if (v === "flow") flowView.setChart(CA.buildFlow(ir, currentFuncId()));
      flowView.setMode(v);
    } else {
      canvasView.setLevel(iface.level);
    }
    var stage = $("stage");
    if (stage && stage.animate) {
      try { stage.animate([{ opacity: 0.35 }, { opacity: 1 }], { duration: 280, easing: "cubic-bezier(0.16,1,0.3,1)" }); } catch (e) { /* 老宿主忽略 */ }
    }
    updateStatus(null);
  }

  /* ── F336 结构哈希校验：切换前后校验 AST ────────────────────────────── */
  function verifyStructure() {
    var h = CA.structureHash(ir);
    if (h !== state.hash) {
      state.hash = h;
      $("s-hash").style.color = "#FF5D5D";
    } else {
      $("s-hash").style.color = "";
    }
    updateStatus(null);
  }

  /* ── v3 一键总结 + 重复内容检测（应用到任何载入的项目 IR） ──────────── */
  function summaryText() {
    var s = CA.summarize(ir.name, ir.meta.funcCount, ir.lang);
    var dups = CA.Dict ? CA.Dict.dedupeNodes(ir) : [];
    if (dups.length) {
      s += "\n\n检测到重复内容 " + dups.length + " 组（同层级同名，可考虑合并）：";
      dups.slice(0, 6).forEach(function (d) {
        s += "\n· " + d.name + " ×" + d.count + "（" + d.levelName + " 级）";
      });
      if (dups.length > 6) s += "\n· …等共 " + dups.length + " 组";
    } else {
      s += "\n\n未检测到同层级同名重复。";
    }
    var langs = {};
    ir.nodes.forEach(function (n) {
      if (n.kind == null || !n.name) return;
      var l = CA.Dict.detectLang(n.name, "");
      if (l !== "未知") langs[l] = (langs[l] || 0) + 1;
    });
    var langList = Object.keys(langs).sort(function (a, b) { return langs[b] - langs[a]; }).slice(0, 4);
    if (langList.length) s += "\n\n语言构成：" + langList.map(function (l) { return l + "×" + langs[l]; }).join("、") + "。";
    return s;
  }

  /* ═══ v4 命令面板（Ctrl+K）：全节点跳转 + 指令直达 ═══════════════════ */
  var cmdkOpen = false, cmdkSel = 0, cmdkItems = [];
  var CMDS = [
    { icon: "🌐", label: "视图：全景画布", sub: "切换", run: function () { setView("canvas"); } },
    { icon: "🔀", label: "视图：流程图", sub: "切换", run: function () { setView("flow"); } },
    { icon: "📊", label: "视图：调用瀑布", sub: "切换", run: function () { setView("waterfall"); } },
    { icon: "🕸", label: "视图：变量蛛网", sub: "切换", run: function () { setView("spider"); } },
    { icon: "🧩", label: "视图：模块架构", sub: "切换", run: function () { setView("arch"); } },
    { icon: "🎯", label: "适应窗口", sub: "视图", run: function () { active.canvas ? canvasView.fit() : flowView.fit(); } },
    { icon: "📝", label: "一键总结", sub: "分析", run: function () { $("summary-body").textContent = summaryText(); $("summary").classList.add("on"); } },
    { icon: "📖", label: "词典面板", sub: "数据", run: function () { var d = $("dicts"); if (d) { d.classList.add("on"); renderDictsPanel(); } } },
    { icon: "📸", label: "截图壁纸 2x", sub: "导出", run: function () { var c = canvasView.screenshot(2); download(c.toDataURL("image/png"), "code-analysis-wallpaper.png"); } },
    { icon: "🖼", label: "导出 PNG / SVG", sub: "导出", run: function () { download(flowView.exportPNG(2).toDataURL("image/png"), "flowchart.png"); download("data:image/svg+xml;charset=utf-8," + encodeURIComponent(flowView.exportSVG()), "flowchart.svg"); } },
    { icon: "🌔", label: "浅色 / 深色主题", sub: "外观", run: function () { setTheme(); } },
    { icon: "🖥", label: "全屏", sub: "视图", run: function () { if (!document.fullscreenElement) document.documentElement.requestFullscreen(); else document.exitFullscreen(); } },
    { icon: "📂", label: "打开 IR JSON", sub: "载入", run: function () { $("ir-file").click(); } }
  ];
  function buildCmdk(q) {
    var items = [];
    q = (q || "").trim().toLowerCase();
    if (q) {
      var count = 0;
      for (var i = 0; i < ir.nodes.length && count < 8; i++) {
        var n = ir.nodes[i];
        if (n.name.toLowerCase().indexOf(q) >= 0 || (n.plain || "").toLowerCase().indexOf(q) >= 0) {
          items.push({
            icon: n.icon || CA.metaphorIcon(n.name), label: n.name,
            sub: KIND_ZH[n.kind] || "", run: (function (id) { return function () { jumpTo(id); }; })(n.id)
          });
          count++;
        }
      }
    }
    CMDS.forEach(function (c) {
      if (!q || c.label.toLowerCase().indexOf(q) >= 0 || c.sub.indexOf(q) >= 0) items.push(c);
    });
    if (q) {
      for (var lv = 1; lv <= 7; lv++) {
        if (("L" + lv).indexOf(q) === 0 || CA.LEVEL_LABEL[lv - 1].toLowerCase().indexOf(q) >= 0) {
          items.push({ icon: "🔍", label: "下钻到 " + CA.LEVEL_LABEL[lv - 1], sub: "级别", run: (function (l) { return function () { iface.level = l; syncLevelUI(); canvasView.setLevel(l); updateProgress(); }; })(lv) });
        }
      }
    }
    return items;
  }
  function renderCmdk() {
    var list = $("cmdk-list");
    if (!list) return;
    if (!cmdkItems.length) { list.innerHTML = '<div class="cmdk-empty">没有匹配的节点或命令</div>'; return; }
    list.innerHTML = cmdkItems.map(function (it, i) {
      return '<div class="cmdk-item' + (i === cmdkSel ? " act" : "") + '" data-i="' + i + '">' +
        '<span class="ic">' + (it.icon || "•") + "</span>" +
        '<span class="lb">' + esc(it.label) + "</span>" +
        (it.sub ? '<span class="sub">' + esc(it.sub) + "</span>" : "") +
        "</div>";
    }).join("");
    list.querySelectorAll(".cmdk-item").forEach(function (el) {
      el.addEventListener("click", function () { runCmdk(+el.getAttribute("data-i")); });
    });
    var act = list.querySelector(".cmdk-item.act");
    if (act && act.scrollIntoView) act.scrollIntoView({ block: "nearest" });
  }
  function runCmdk(i) {
    var it = cmdkItems[i];
    if (!it) return;
    closeCmdk();
    it.run();
  }
  function openCmdk() {
    var k = $("cmdk");
    if (!k) return;
    cmdkOpen = true; cmdkSel = 0;
    cmdkItems = buildCmdk("");
    renderCmdk();
    k.classList.add("on");
    var inp = $("cmdk-input");
    if (inp) { inp.value = ""; setTimeout(function () { inp.focus(); }, 30); }
  }
  function closeCmdk() {
    var k = $("cmdk");
    if (!k) return;
    cmdkOpen = false;
    k.classList.remove("on");
  }
  if ($("cmdk-open")) tool("cmdk-open", openCmdk);
  if ($("t-cmdk")) tool("t-cmdk", openCmdk);
  if ($("cmdk")) {
    $("cmdk").addEventListener("mousedown", function (e) {
      if (e.target === $("cmdk")) closeCmdk();
    });
  }
  if ($("cmdk-input")) {
    $("cmdk-input").addEventListener("input", function (e) {
      cmdkSel = 0;
      cmdkItems = buildCmdk(e.target.value);
      renderCmdk();
    });
  }
  document.addEventListener("keydown", function (e) {
    if ((e.ctrlKey || e.metaKey) && (e.key === "k" || e.key === "K")) {
      e.preventDefault();
      cmdkOpen ? closeCmdk() : openCmdk();
      return;
    }
    if (!cmdkOpen) return;
    if (e.key === "Escape") { closeCmdk(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); cmdkSel = Math.min(cmdkSel + 1, cmdkItems.length - 1); renderCmdk(); }
    else if (e.key === "ArrowUp") { e.preventDefault(); cmdkSel = Math.max(cmdkSel - 1, 0); renderCmdk(); }
    else if (e.key === "Enter") { e.preventDefault(); runCmdk(cmdkSel); }
  });

  /* ── v3 词典面板 + 专属文件夹载入 ───────────────────────────────────── */
  function renderDictsPanel() {
    var host = $("dicts-body");
    if (!host || !CA.Dict) return;
    var st = CA.Dict.stats();
    var rows = st.packs.map(function (p) {
      return '<div class="dict-row"><b>' + esc(p.name) + "</b><span>" + p.terms +
        " 词条" + (p.skipped ? " · 去重 " + p.skipped : "") +
        (p.syntax ? " · 语法 " + p.syntax : "") + "</span></div>";
    });
    rows.push('<div class="dict-row"><b>我的补充</b><span>' + st.user + " 条（设置里导入后生效）</span></div>");
    rows.push('<div class="dict-row total"><b>当前生效</b><span>' + st.terms + " 词条 · 全项目通用</span></div>");
    host.innerHTML = rows.join("");
  }
  function loadUserDict() {
    try { return JSON.parse(root.localStorage.getItem("ca-user-dict") || "{}"); }
    catch (e) { return {}; }
  }
  if (CA.Dict) {
    CA.Dict.loadUser(loadUserDict());
    if (typeof fetch === "function" && String((root.location && root.location.protocol) || "").indexOf("http") === 0) {
      fetch("/api/dicts", { cache: "no-store" })
        .then(function (r) { return r.ok ? r.json() : []; })
        .then(function (list) {
          (list || []).forEach(function (f) {
            fetch("/dicts/" + f, { cache: "no-store" })
              .then(function (r) { return r.ok ? r.json() : null; })
              .then(function (j) { if (j) { CA.Dict.load(j, f); renderDictsPanel(); } })
              .catch(function () { /* 单个词典包损坏不影响其他 */ });
          });
        })
        .catch(function () { /* 静态伺服模式无 /api/dicts */ });
    }
  }
  if ($("a-dict")) {
    tool("a-dict", function () {
      var d = $("dicts");
      if (!d) return;
      d.classList.toggle("on");
      renderDictsPanel();
    });
  }
  if ($("dicts-close")) {
    $("dicts-close").addEventListener("click", function () { $("dicts").classList.remove("on"); });
  }
  if ($("dict-import")) {
    $("dict-import").addEventListener("click", function () { $("dict-file").click(); });
  }
  if ($("dict-file")) {
    $("dict-file").addEventListener("change", function (e) {
      var f = e.target.files && e.target.files[0];
      e.target.value = "";
      if (!f) return;
      var r = new FileReader();
      r.onload = function () {
        var j;
        try { j = JSON.parse(String(r.result)); }
        catch (err) { toast("词典 JSON 解析失败：" + err.message); return; }
        var p = CA.Dict.load(j, f.name);
        toast("词典已载入：" + f.name + " · " + p.count + " 词条" + (p.skipped ? " · 去重 " + p.skipped : ""));
        renderDictsPanel();
      };
      r.readAsText(f);
    });
  }

  /* ── 事件绑定 ───────────────────────────────────────────────────────── */
  $("level").addEventListener("input", function (e) {
    var lv = +e.target.value;
    /* 空层级防护：此项目没有该层级节点时，留在原层级并如实说明。 */
    if (CA.nodesAtLevel(ir, lv).length === 0) {
      syncLevelUI();
      toast("L" + lv + "（" + CA.LEVEL_LABEL[lv - 1] + "）此项目没有节点，已保持在 L" + iface.level);
      return;
    }
    iface.level = lv;
    syncLevelUI();
    canvasView.setLevel(lv);
    updateProgress();
    verifyStructure();
  });
  $("search").addEventListener("input", function (e) {
    state.search = e.target.value;
    var set = new Set();
    if (state.search.trim()) {
      var q = state.search.trim().toLowerCase();
      ir.nodes.forEach(function (n) {
        if (n.name.toLowerCase().indexOf(q) >= 0 || (n.plain || "").toLowerCase().indexOf(q) >= 0) set.add(n.id);
      });
    }
    canvasView.setSearch(set.size ? set : null);
    renderTree();
  });
  $("iface-chips").addEventListener("click", function (e) {
    var b = e.target.closest("button"); if (b) setMode(b.dataset.mode);
  });
  $("view-chips").addEventListener("click", function (e) {
    var b = e.target.closest("button"); if (b) setView(b.dataset.view);
  });
  $("legend-fold").addEventListener("click", function () {
    var l = $("legend"); l.classList.toggle("folded");
    $("legend-fold").textContent = l.classList.contains("folded") ? "＋" : "－";
  });
  $("summary-close").addEventListener("click", function () { $("summary").classList.remove("on"); });
  if ($("story-close")) {
    $("story-close").addEventListener("click", function () { $("story").classList.remove("on"); });
  }
  document.addEventListener("keydown", function (e) {
    var ae = document.activeElement;
    if (ae && (ae.tagName === "INPUT" || ae.tagName === "TEXTAREA" || ae.isContentEditable)) return;
    if (e.key === "Escape") { closeDetail(); $("story").classList.remove("on"); $("summary").classList.remove("on"); }
    if (e.key === "f" || e.key === "F") {
      var id = canvasView.getSelected();
      if (id >= 0) canvasView.focus(id);
      put2("s-fkey", "开");
      var dot = $("s-dot");
      if (dot) dot.classList.remove("off");
    }
  });

  /* ── F318 工具栏 ────────────────────────────────────────────────────── */
  function tool(id, fn) {
    var el = $(id);
    if (el) el.addEventListener("click", fn);
  }
  tool("t-zoomin", function () { active.canvas ? canvasView.zoomBy(220) : flowView.zoomBy(220); });
  tool("t-zoomout", function () { active.canvas ? canvasView.zoomBy(-220) : flowView.zoomBy(-220); });
  tool("t-fit", function () { active.canvas ? canvasView.fit() : flowView.fit(); });
  function toggleTool(btn, key, apply) {
    btn.addEventListener("click", function () {
      var on = btn.getAttribute("aria-pressed") !== "true";
      btn.setAttribute("aria-pressed", String(on));
      apply(on);
    });
  }
  toggleTool($("t-particles"), "particles", function (on) { canvasView.setFlags({ particles: on }); });
  toggleTool($("t-bloom"), "bloom", function (on) { canvasView.setFlags({ bloom: on }); });
  toggleTool($("t-heat"), "heat", function (on) { canvasView.setFlags({ heat: on }); });
  toggleTool($("t-rain"), "rain", function (on) { canvasView.setFlags({ rain: on }); });
  toggleTool($("t-spectrum"), "spectrum", function (on) { canvasView.setFlags({ spectrum: on }); });
  toggleTool($("t-dataflow"), "dataflow", function (on) { flowView.setDataFlow(on); });
  tool("t-layout", function () {
    var fid = currentFuncId();
    flowView.setChart(CA.buildFlow(ir, fid));
  });
  tool("t-play", function () {
    var on = flowView.play();
    $("t-play").setAttribute("aria-pressed", String(on));
  });
  tool("t-compare", function () {
    var on = !flowView.isCompare();
    var old = CA.buildFlow(ir, currentFuncId());
    old.nodes.splice(3, 1); // 旧版少一个节点 → 走 diff 逻辑
    flowView.setCompare(on, old);
    $("t-compare").setAttribute("aria-pressed", String(on));
  });
  /* F274 截图壁纸（2x 超采样） */
  tool("t-shot", function () {
    var c = canvasView.screenshot(2);
    download(c.toDataURL("image/png"), "code-analysis-wallpaper.png");
  });
  /* F289 导出 PNG / SVG */
  tool("t-export", function () {
    var png = flowView.exportPNG(2);
    download(png.toDataURL("image/png"), "flowchart.png");
    var svg = flowView.exportSVG();
    download("data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg), "flowchart.svg");
  });
  /* F290 打印模式：A4 分页，每页标题+页码 */
  tool("t-print", function () {
    var pages = flowView.printPages();
    var svg = flowView.exportSVG();
    var w = window.open("", "_blank");
    if (!w) return;
    w.document.write(
      "<!doctype html><html><head><title>流程图打印</title><style>" +
      "@page{size:A4;margin:14mm}body{font-family:'Segoe UI',sans-serif;margin:0}" +
      ".page{page-break-after:always;padding:0 0 10mm}.page:last-child{page-break-after:auto}" +
      "h4{font-weight:600;margin:0 0 6mm;font-size:13px}.foot{position:fixed;bottom:6mm;right:14mm;color:#86868B;font-size:10px}" +
      "svg{max-width:100%;height:auto}</style></head><body>"
    );
    pages.forEach(function (p, i) {
      w.document.write('<div class="page"><h4>流程图 · 第 ' + (i + 1) + " / " + pages.length + " 页</h4>" + svg +
        '<div class="foot">' + (i + 1) + " / " + pages.length + "</div></div>");
    });
    w.document.write("</body></html>");
    w.document.close();
    setTimeout(function () { w.print(); }, 300);
  });
  tool("t-full", function () {
    if (!document.fullscreenElement) document.documentElement.requestFullscreen();
    else document.exitFullscreen();
  });
  tool("t-summary", function () {
    $("summary-body").textContent = summaryText();
    $("summary").classList.add("on");
  });
  tool("t-load", function () { $("ir-file").click(); });

  /* ── AI-09 活动栏 + 标题栏色板（壳无关：三壳共用同一装配层） ──────────── */
  function toggleNav() {
    var app = $("app");
    app.classList.toggle("nav-closed");
    $("a-nav").setAttribute("aria-pressed", String(!app.classList.contains("nav-closed")));
  }
  function toggleDetailPanel() {
    var app = $("app");
    app.classList.toggle("detail-closed");
    $("a-detail").setAttribute("aria-pressed", String(!app.classList.contains("detail-closed")));
  }
  if ($("a-nav")) tool("a-nav", toggleNav);
  if ($("a-detail")) tool("a-detail", toggleDetailPanel);
  if ($("a-style")) {
    tool("a-style", function () {
      if (CA.Shell) CA.Shell.cycleStyle();
    });
  }
  if ($("a-theme")) tool("a-theme", function () { setTheme(); });
  if ($("a-summary")) {
    tool("a-summary", function () {
      $("summary-body").textContent = summaryText();
      $("summary").classList.add("on");
    });
  }
  /* 标题栏 8 风格色板：点击直达对应风格 */
  if (CA.Shell && document.querySelectorAll) {
    Array.prototype.forEach.call(document.querySelectorAll(".tl-styles button"), function (b) {
      b.addEventListener("click", function () { CA.Shell.applyStyle(+b.dataset.styleI); });
    });
  }
  /* UI-025 快捷键：Ctrl+B 导航面板 / Ctrl+Alt+B 详情面板（三壳同表） */
  document.addEventListener("keydown", function (e) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey === undefined) return;
    var k = (e.key || "").toLowerCase();
    if (k === "b" && !e.altKey) { e.preventDefault(); toggleNav(); }
    else if (k === "b" && e.altKey) { e.preventDefault(); toggleDetailPanel(); }
  });
  if ($("ir-file")) {
    $("ir-file").addEventListener("change", function (e) {
      var f = e.target.files && e.target.files[0];
      readFile(f);
      e.target.value = "";
    });
  }
  /* 直接把 .json 拖到画布上载入 */
  (function () {
    var stage = $("stage");
    if (!stage) return;
    stage.addEventListener("dragover", function (e) { e.preventDefault(); stage.classList.add("dropping"); });
    stage.addEventListener("dragleave", function () { stage.classList.remove("dropping"); });
    stage.addEventListener("drop", function (e) {
      e.preventDefault();
      stage.classList.remove("dropping");
      var f = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
      readFile(f);
    });
  })();
  /* 主题切换（工具栏与活动栏共用；t-theme/a-theme 双入口） */
  function setTheme() {
    state.theme = state.theme === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", state.theme);
    canvasView.setTheme(state.theme);
    flowView.setTheme(state.theme);
  }
  tool("t-theme", setTheme);
  function download(url, name) {
    var a = document.createElement("a");
    a.href = url; a.download = name;
    document.body.appendChild(a); a.click(); a.remove();
  }

  /* ── F326 隐藏非必要 UI：鼠标移到屏幕边缘 10px 内才浮现 ─────────────── */
  document.addEventListener("mousemove", function (e) {
    if (state.pin) return;
    var b = document.body;
    var w = root.innerWidth, h = root.innerHeight;
    b.classList.toggle("reveal-left", CA.edgeReveal(e.clientX, w) || e.clientX < 0);
    b.classList.toggle("reveal-right", e.clientX >= w - CA.EDGE_REVEAL_PX || e.clientX < 0);
    b.classList.toggle("reveal-bottom", e.clientY >= h - CA.EDGE_REVEAL_PX);
  });
  $("s-pin").addEventListener("click", function () {
    state.pin = !state.pin;
    $("s-pin").textContent = state.pin ? "取消锁定" : "锁定面板";
    var b = document.body;
    if (state.pin) {
      b.classList.remove("auto-ui");
      b.classList.add("reveal-left", "reveal-right", "reveal-bottom");
    } else {
      b.classList.add("auto-ui");
      b.classList.remove("reveal-left", "reveal-right", "reveal-bottom");
    }
  });

  /* ── F303 对照界面分割线拖拽 ────────────────────────────────────────── */
  (function () {
    var sp = $("split"), dragging = false;
    sp.addEventListener("mousedown", function () { dragging = true; });
    root.addEventListener("mousemove", function (e) {
      if (!dragging) return;
      var r = $("stage").getBoundingClientRect();
      state.split = U.clamp((e.clientX - r.left) / r.width, 0.15, 0.85);
      sp.style.left = (state.split * 100).toFixed(2) + "%";
      $("tag-right").style.left = (state.split * 100 + 2).toFixed(2) + "%";
      canvasView.setCompare(true, state.split);
    });
    root.addEventListener("mouseup", function () { dragging = false; });
  })();

  /* ── 启动 ───────────────────────────────────────────────────────────── */
  root.addEventListener("resize", function () { canvasView.resize(); flowView.resize(); });
  $("brand-sub").textContent = ir.name;
  syncLevelUI();
  closeDetail();
  renderTree();
  renderLegend();
  updateProgress();
  updateStatus(null);
  setMode("plain");
  setView("canvas");

  /* 启动后 ?ir=<url> 显式载入；默认数据由 shell.js 统一调度
   * （壳A 服务器 → /api/ir 的 core 统一 IR；静态伺服 → data/ir.json 兜底）。 */
  (function bootLoad() {
    var loc = root.location || {};
    var url = "";
    try { url = new URLSearchParams(String(loc.search || "")).get("ir") || ""; } catch (e) { url = ""; }
    if (url) loadIRURL(url);
  })();

  /* 供控制台/验收脚本调用 */
  root.CA_APP = { ir: ir, canvas: canvasView, flow: flowView, iface: iface, state: state,
    loadIRJSON: function (json, label) { return applyIR(CA.irFromJSON(json), label || "IR JSON"); },
    loadIRText: loadIRText, loadIRURL: loadIRURL,
    openCmdk: openCmdk, closeCmdk: closeCmdk };
})(typeof globalThis !== "undefined" ? globalThis : this);
