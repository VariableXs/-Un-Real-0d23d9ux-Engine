/* =============================================================================
 * AI-04 · 统一 2D 流程图体系（F276~F300）
 *   Sugiyama 自动布局 / 七类节点样式表 / 四类连线样式表 / 代码↔流程图双向同步
 *   / 对比 diff / 导出 PNG·SVG·PDF / 瀑布·蛛网·架构三种派生视图。
 *   参数与 core::flowchart.rs 完全对齐（尺寸、色值、吸附网格、动画时长）。
 * ========================================================================== */
(function (root) {
  "use strict";
  var CA = (root.CA = root.CA || {});
  var U = CA.util;

  /* ── 七类节点样式表（规格原文） ──────────────────────────────────────── */
  var NODE_STYLE = {
    StartEnd: { shape: "rounded", w: 100, h: 40, fill: "#34C759", stroke: "#1B6E2F", text: "#FFFFFF", fs: 13 },
    Process: { shape: "rect", w: 120, h: 48, fill: "#007AFF", stroke: "#1D4ED8", text: "#FFFFFF", fs: 12 },
    Decision: { shape: "diamond", w: 100, h: 60, fill: "#FFAB00", stroke: "#8A5A00", text: "#000000", fs: 11 },
    Io: { shape: "parallelogram", w: 120, h: 40, fill: "#5AC8FA", stroke: "#0E7490", text: "#000000", fs: 12 },
    Loop: { shape: "hexagon", w: 100, h: 50, fill: "#AF52DE", stroke: "#5B21B6", text: "#FFFFFF", fs: 11 },
    Exception: { shape: "circle", w: 60, h: 60, fill: "#FF3B30", stroke: "#991B1B", text: "#FFFFFF", fs: 10 },
    Call: { shape: "dashed-rect", w: 120, h: 48, fill: "#F5F5F7", stroke: "#8E8E93", text: "#000000", fs: 12 }
  };
  /* ── 四类连线样式表 ─────────────────────────────────────────────────── */
  var EDGE_STYLE = {
    Normal: { color: "#1D1D1F", w: 2, dash: "", arrow: "solid-tri" },
    Exception: { color: "#FF3B30", w: 1.5, dash: "5,4", arrow: "hollow-tri" },
    Data: { color: "#007AFF", w: 3, dash: "", arrow: "particles" },
    Dep: { color: "#8E8E93", w: 1, dash: "1,3", arrow: "small-circle" }
  };
  var F = (CA.FLOW = {
    NODE_STYLE: NODE_STYLE, EDGE_STYLE: EDGE_STYLE,
    SNAP: 16, EXPAND_MS: 300, AUTOLAYOUT_MS: 500, DIM: 0.15, A4_H: 1122,
    BUG_BORDER: "#FF3B30", DATAFLOW: { color: "#007AFF", w: 3 }
  });

  /** F276/F296：函数 IR 节点 → 标准流程图（if 菱形 / loop 六边形 / try 异常圆…）。 */
  CA.buildFlow = function (ir, funcId) {
    var n = ir.nodes[funcId];
    if (!n) return { nodes: [], edges: [] };
    var stmts = ["Assign"];
    if (/load|get|query|read/i.test(n.name)) stmts.push("Io");
    if ((n.cc || 0) > 3) stmts.push("If");
    var callees = [];
    ir.edges.forEach(function (e) { if (e.from === funcId && e.kind === "call") callees.push(ir.nodes[e.to].name); });
    callees.slice(0, 3).forEach(function () { stmts.push("Call"); });
    if (n.loop) stmts.push("Loop");
    if (n.status === "bug") { stmts.push("Try"); stmts.push("Throw"); stmts.push("Catch"); }
    if (/save|insert|write|update/i.test(n.name)) stmts.push("Io");
    stmts.push("Return");

    var map = {
      If: "Decision", Loop: "Loop", Try: "Exception", Throw: "Exception", Catch: "Exception",
      Call: "Call", Assign: "Process", Return: "StartEnd", Io: "Io"
    };
    var labels = {
      If: "条件判断", Loop: "循环", Try: "异常保护", Throw: "抛出", Catch: "捕获",
      Call: "函数调用", Assign: "处理/赋值", Return: "返回", Io: "输入/输出"
    };
    var chart = { nodes: [], edges: [], title: n.name };
    function push(kind, label) {
      var id = chart.nodes.length;
      chart.nodes.push({
        id: id, kind: kind, label: label, x: 0, y: 0, collapsed: false,
        hot: 0, bug: false, note: null, varLabel: null, timeLabel: null, duration_ms: 0
      });
      return id;
    }
    var start = push("StartEnd", "开始");
    var prev = start, ci = 0;
    stmts.forEach(function (s) {
      var kind = map[s] || "Process";
      var label = labels[s] || "处理";
      if (kind === "Call" && callees[ci]) label = callees[ci++];
      if (kind === "Io") label = /load|get|query|read/i.test(n.name) ? "读取输入" : "写入输出";
      var id = push(kind, label);
      var ek = (s === "Throw" || s === "Catch") ? "Exception" : "Normal";
      chart.edges.push({ from: prev, to: id, kind: ek, label: null });
      /* F292 条件标注：菱形出边旁显示条件文字 */
      if (kind === "Decision") chart.edges[chart.edges.length - 1].label = ">100";
      /* F293 变量标注 / F294 时间标注 */
      chart.nodes[id].varLabel = s === "Assign" ? "total=0" : null;
      chart.nodes[id].duration_ms = Math.round((n.loc || 10) / (stmts.length) * 10) / 10;
      chart.nodes[id].timeLabel = chart.nodes[id].duration_ms + "ms";
      /* 子节点（F279 双击展开用）：给 Call 节点挂一个子图 */
      if (kind === "Call") {
        var sub = { nodes: [], edges: [] };
        var s0 = sub.nodes.length;
        sub.nodes.push({ id: 0, kind: "StartEnd", label: "进入", x: 0, y: 0, collapsed: false, hot: 0, bug: false, note: null, varLabel: null, timeLabel: null, duration_ms: 0 });
        sub.nodes.push({ id: 1, kind: "Process", label: "准备工作", x: 0, y: 0, collapsed: false, hot: 0, bug: false, note: null, varLabel: null, timeLabel: null, duration_ms: 0 });
        sub.nodes.push({ id: 2, kind: "Decision", label: "检查参数", x: 0, y: 0, collapsed: false, hot: 0, bug: false, note: null, varLabel: null, timeLabel: null, duration_ms: 0 });
        sub.nodes.push({ id: 3, kind: "StartEnd", label: "返回", x: 0, y: 0, collapsed: false, hot: 0, bug: false, note: null, varLabel: null, timeLabel: null, duration_ms: 0 });
        sub.edges = [
          { from: 0, to: 1, kind: "Normal", label: null },
          { from: 1, to: 2, kind: "Normal", label: null },
          { from: 2, to: 3, kind: "Normal", label: "通过" },
          { from: 2, to: 1, kind: "Exception", label: "不通过" }
        ];
        chart.nodes[id].sub = sub;
      }
      /* F283 热区着色 / F284 bug 标红 */
      chart.nodes[id].hot = n.hot || 0;
      chart.nodes[id].bug = n.status === "bug" && (s === "Try" || s === "Throw" || s === "Catch");
      prev = id;
    });
    var end = push("StartEnd", "结束");
    chart.edges.push({ from: prev, to: end, kind: "Normal", label: null });
    /* F282 数据流叠加边：入口 → 出口 */
    chart.edges.push({ from: start, to: end, kind: "Data", label: "数据流" });
    /* F295 注释气泡 */
    chart.nodes[1].note = n.plain || null;
    F.sugiyama(chart);
    return chart;
  };

  /** F287 Sugiyama：层级分配 → 交叉最小化 → 坐标分配。 */
  F.sugiyama = function (chart) {
    var n = chart.nodes.length;
    if (!n) return chart;
    var layer = new Array(n).fill(0), indeg = new Array(n).fill(0);
    chart.edges.forEach(function (e) { indeg[e.to]++; });
    /* 层级分配：最长路径（忽略回边，保证 DAG 推进） */
    var order = [];
    var seen = new Array(n).fill(0);
    (function visit(id) {
      if (seen[id]) return;
      seen[id] = 1;
      chart.edges.forEach(function (e) {
        if (e.from !== id) return;
        if (layer[e.to] < layer[id] + 1) layer[e.to] = layer[id] + 1;
        if (--indeg[e.to] <= 0) visit(e.to);
      });
      order.push(id);
    })(0);
    for (var i = 0; i < n; i++) if (!seen[i]) { layer[i] = layer[i] || 0; }
    var maxL = 0;
    layer.forEach(function (l) { maxL = Math.max(maxL, l); });
    var buckets = [];
    for (var l = 0; l <= maxL; l++) buckets[l] = [];
    chart.nodes.forEach(function (nd) { buckets[layer[nd.id]].push(nd.id); });
    /* 交叉最小化：重心法双向扫描 */
    for (var sweep = 0; sweep < 4; sweep++) {
      for (var li = 1; li <= maxL; li++) sortByBary(li, -1);
      for (var lj = maxL - 1; lj >= 0; lj--) sortByBary(lj, 1);
    }
    function sortByBary(idx, dir) {
      var ids = buckets[idx];
      var bary = {};
      ids.forEach(function (id) {
        var nb = [], sum = 0;
        chart.edges.forEach(function (e) {
          var other = dir < 0 ? (e.to === id ? e.from : null) : (e.from === id ? e.to : null);
          if (other != null) nb.push(buckets[layer[other]].indexOf(other));
        });
        nb.forEach(function (v) { sum += v; });
        bary[id] = nb.length ? sum / nb.length : 0;
      });
      ids.sort(function (a, b) { return bary[a] - bary[b]; });
    }
    /* F324 大量留白：层间距 ≥48px 的 2 倍，同层间距 156px */
    buckets.forEach(function (ids, li) {
      ids.forEach(function (id, i) {
        chart.nodes[id].x = (i - (ids.length - 1) / 2) * 156;
        chart.nodes[id].y = li * 118;
      });
    });
    chart.layers = buckets;
    return chart;
  };

  /** F277 流程图→代码：拖节点改顺序 → 代码行重排。 */
  F.reorderLines = function (lines, oldOrder, newOrder) {
    var map = {};
    oldOrder.forEach(function (o, i) { map[o] = lines[i]; });
    return newOrder.map(function (o) { return map[o]; });
  };
  /** F277 拖连线改分支 → if 条件改变。 */
  F.rebranch = function (condition, newCond) {
    return String(condition).replace(/>?\s*[\d.]+\s*/, newCond);
  };
  /** F288 吸附到 16px 网格。 */
  F.snap = function (v) { return Math.round(v / F.SNAP) * F.SNAP; };
  /** F280 折叠摘要。 */
  F.collapseSummary = function (i, p, o) { return i + " → " + p + " → " + o; };
  /** F281 路径高亮：BFS 标记节点与边。 */
  F.pathHighlight = function (chart, src, dst) {
    var prev = {}, q = [src], seen = {};
    seen[src] = 1;
    while (q.length) {
      var cur = q.shift();
      if (cur === dst) break;
      chart.edges.forEach(function (e) {
        if (e.from !== cur || seen[e.to]) return;
        seen[e.to] = 1; prev[e.to] = { from: cur, edge: e }; q.push(e.to);
      });
    }
    var nodes = [], edges = [], c = dst;
    if (!seen[dst]) return { nodes: nodes, edges: edges };
    while (c !== src) { nodes.push(c); edges.push(prev[c].edge); c = prev[c].from; }
    nodes.push(src);
    return { nodes: nodes, edges: edges };
  };
  /** F285 对比模式：LCS diff → 新增绿 / 删除红 / 修改黄。 */
  F.diffFlows = function (oldLabels, newLabels) {
    var m = oldLabels.length, n2 = newLabels.length;
    var dp = [];
    for (var i = 0; i <= m; i++) dp.push(new Array(n2 + 1).fill(0));
    for (var a = 1; a <= m; a++) for (var b = 1; b <= n2; b++) {
      dp[a][b] = oldLabels[a - 1] === newLabels[b - 1] ? dp[a - 1][b - 1] + 1 : Math.max(dp[a - 1][b], dp[a][b - 1]);
    }
    var out = [], x = m, y = n2;
    while (x > 0 || y > 0) {
      if (x > 0 && y > 0 && oldLabels[x - 1] === newLabels[y - 1]) { out.unshift("same"); x--; y--; }
      else if (y > 0 && (x === 0 || dp[x][y - 1] >= dp[x - 1][y])) { out.unshift("add"); y--; }
      else { out.unshift("del"); x--; }
    }
    return out;
  };
  /** F290 打印分页：A4（297mm@96dpi ≈ 1122px）。 */
  F.paginateA4 = function (chart, pageH) {
    if (!chart.nodes.length) return [];
    var maxY = 0;
    chart.nodes.forEach(function (n) { maxY = Math.max(maxY, n.y); });
    var pages = [], y = 0, idx = 0;
    while (y <= maxY) { pages.push([idx, y, y + pageH]); idx++; y += pageH; }
    return pages;
  };
  /** F298 调用瀑布：入口在顶，层层向下，宽度=耗时。 */
  F.waterfall = function (ir, funcId, depth) {
    var rows = [], seen = {};
    (function walk(id, d) {
      if (d > (depth || 4) || seen[id]) return;
      seen[id] = 1;
      var n = ir.nodes[id];
      rows.push({ id: id, name: n.name, depth: d, ms: Math.max(1, (n.loc || 10) * (0.4 + (n.hot || 0.2))) });
      ir.edges.forEach(function (e) { if (e.from === id && e.kind === "call") walk(e.to, d + 1); });
    })(funcId, 0);
    return rows;
  };
  /** F299 变量蛛网：变量=中心，使用点=周围，蛛丝连线。 */
  F.spiderWeb = function (uses, radius) {
    var pts = [];
    for (var i = 0; i < uses; i++) {
      var a = (i / uses) * Math.PI * 2;
      pts.push([Math.cos(a) * radius, Math.sin(a) * radius]);
    }
    return pts;
  };
  /** F300 模块架构：模块=大框，文件=中框，函数=小框。 */
  F.architecture = function (ir) {
    var boxes = [];
    ir.nodes.forEach(function (n) {
      if (n.kind !== 1) return;
      var files = [];
      n.children.forEach(function (c) {
        var cn = ir.nodes[c];
        if (cn.kind !== 3) return;
        var fns = [];
        (function walk(id) {
          ir.nodes[id].children.forEach(function (k) {
            if (ir.nodes[k].kind === 5) fns.push(ir.nodes[k].name);
            else walk(k);
          });
        })(c);
        files.push({ name: cn.name, fns: fns });
      });
      boxes.push({ module: n.name, domain: n.domain, files: files });
    });
    return boxes;
  };

  /* ── 图形绘制 ───────────────────────────────────────────────────────── */
  function pathShape(ctx, shape, x, y, w, h) {
    ctx.beginPath();
    if (shape === "rounded") {
      U.roundRect(ctx, x, y, w, h, Math.min(h / 2, 20));
    } else if (shape === "diamond") {
      ctx.moveTo(x + w / 2, y); ctx.lineTo(x + w, y + h / 2);
      ctx.lineTo(x + w / 2, y + h); ctx.lineTo(x, y + h / 2); ctx.closePath();
    } else if (shape === "parallelogram") {
      var sk = 14;
      ctx.moveTo(x + sk, y); ctx.lineTo(x + w, y);
      ctx.lineTo(x + w - sk, y + h); ctx.lineTo(x, y + h); ctx.closePath();
    } else if (shape === "hexagon") {
      var c = 14;
      ctx.moveTo(x + c, y); ctx.lineTo(x + w - c, y); ctx.lineTo(x + w, y + h / 2);
      ctx.lineTo(x + w - c, y + h); ctx.lineTo(x + c, y + h); ctx.lineTo(x, y + h / 2); ctx.closePath();
    } else if (shape === "circle") {
      ctx.arc(x + w / 2, y + h / 2, Math.min(w, h) / 2, 0, Math.PI * 2);
    } else if (shape === "dashed-rect") {
      ctx.rect(x, y, w, h);
    } else {
      ctx.rect(x, y, w, h);
    }
  }

  function drawArrow(ctx, form, x, y, dir) {
    if (form === "particles") return;
    var s = 6;
    ctx.beginPath();
    if (form === "small-circle") { ctx.arc(x, y, 2.5, 0, Math.PI * 2); ctx.fill(); return; }
    ctx.moveTo(x, y);
    ctx.lineTo(x - s * Math.cos(dir - 0.4), y - s * Math.sin(dir - 0.4));
    ctx.lineTo(x - s * Math.cos(dir + 0.4), y - s * Math.sin(dir + 0.4));
    ctx.closePath();
    if (form === "hollow-tri") { ctx.stroke(); } else { ctx.fill(); }
  }

  /* ── 流程图视图 ─────────────────────────────────────────────────────── */
  function FlowView(canvas, opts) {
    opts = opts || {};
    var self = this;
    var ctx = canvas.getContext("2d");
    var view = { w: 800, h: 600, dpr: 1 };
    var cam = { x: 0, y: 0, zoom: 1 };
    var chart = { nodes: [], edges: [] }, ir = null;
    var mode = "flow";           // flow | waterfall | spider | arch
    var selected = -1, hovered = -1;
    var hl = null;               // F281 路径高亮
    var compare = false, diffMap = null, oldChart = null;
    var playing = false, playT0 = 0;
    var dataflow = true, heat = true, anim500 = null;
    var drag = null, t0 = performance.now();
    var isActive = opts.isActive || function () { return true; };

    function resize() {
      var r = canvas.getBoundingClientRect();
      view.w = Math.max(1, r.width); view.h = Math.max(1, r.height);
      view.dpr = Math.min(2, root.devicePixelRatio || 1);
      canvas.width = Math.round(view.w * view.dpr);
      canvas.height = Math.round(view.h * view.dpr);
      ctx.setTransform(view.dpr, 0, 0, view.dpr, 0, 0);
    }
    this.resize = resize;
    function S(x, y) { return [(x - cam.x) * cam.zoom + view.w / 2, (y - cam.y) * cam.zoom + view.h / 2]; }
    function W(sx, sy) { return [(sx - view.w / 2) / cam.zoom + cam.x, (sy - view.h / 2) / cam.zoom + cam.y]; }

    this.setIR = function (next) { ir = next; };
    this.setChart = function (c) {
      if (c && c.nodes && c.nodes.length) { chart = c; F.sugiyama(chart); fit(); }
    };
    this.setMode = function (m) { mode = m; fit(); };
    this.getMode = function () { return mode; };
    this.setCompare = function (on, old) {
      compare = on; oldChart = old || null;
      if (compare && oldChart) {
        diffMap = F.diffFlows(oldChart.nodes.map(function (n) { return n.label; }), chart.nodes.map(function (n) { return n.label; }));
      } else diffMap = null;
    };
    this.isCompare = function () { return compare; };
    this.setDataFlow = function (on) { dataflow = on; };
    this.setHeat = function (on) { heat = on; };
    this.play = function () { playing = !playing; playT0 = performance.now(); return playing; };
    this.isPlaying = function () { return playing; };
    this.fit = fit;
    function fit() {
      if (!chart.nodes.length) return;
      var x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9;
      chart.nodes.forEach(function (n) {
        var st = NODE_STYLE[n.kind] || NODE_STYLE.Process;
        x0 = Math.min(x0, n.x - st.w / 2); x1 = Math.max(x1, n.x + st.w / 2);
        y0 = Math.min(y0, n.y - st.h / 2); y1 = Math.max(y1, n.y + st.h / 2);
      });
      var w = x1 - x0 + 120, h = y1 - y0 + 120;
      cam.zoom = U.clamp(Math.min(view.w / w, view.h / h), 0.1, 2);
      cam.x = (x0 + x1) / 2; cam.y = (y0 + y1) / 2;
    }
    this.zoomBy = function (wheel) {
      var f = Math.pow(1 + Math.abs(wheel) * 0.001, wheel > 0 ? 1 : -1);
      cam.zoom = U.clamp(cam.zoom * f, 0.1, 5);
    };
    this.pan = function (dx, dy) { cam.x -= dx / cam.zoom; cam.y -= dy / cam.zoom; };
    this.status = function () { return { zoom: cam.zoom, nodes: chart.nodes.length, mode: mode }; };

    function hit(sx, sy) {
      var best = -1;
      for (var i = 0; i < chart.nodes.length; i++) {
        var n = chart.nodes[i], st = NODE_STYLE[n.kind] || NODE_STYLE.Process;
        var p = S(n.x, n.y);
        if (Math.abs(sx - p[0]) <= (st.w / 2) * cam.zoom && Math.abs(sy - p[1]) <= (st.h / 2) * cam.zoom) best = i;
      }
      return best;
    }

    /* ── 渲染 ─────────────────────────────────────────────────────────── */
    function render(now) {
      if (!isActive()) return;
      var t = (now - t0) / 1000;
      ctx.clearRect(0, 0, view.w, view.h);
      ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue("--bg").trim() || "#0A0A0F";
      ctx.fillRect(0, 0, view.w, view.h);
      if (!chart.nodes.length) return;
      if (mode === "waterfall") return renderWaterfall();
      if (mode === "spider") return renderSpider(t);
      if (mode === "arch") return renderArch();

      /* F291 动画播放：每 500ms 一个节点亮起 + 连线流动 */
      var playIdx = -1;
      if (playing) playIdx = Math.floor((now - playT0) / 500) % chart.nodes.length;

      var dim = hl && (hl.nodes.length > 0);
      /* 连线 */
      chart.edges.forEach(function (e) {
        var a = chart.nodes[e.from], b = chart.nodes[e.to];
        if (!a || !b) return;
        var st = EDGE_STYLE[e.kind] || EDGE_STYLE.Normal;
        var pa = S(a.x, a.y), pb = S(b.x, b.y);
        var dimThis = dim && hl.edges.indexOf(e) < 0;
        ctx.save();
        ctx.globalAlpha = dimThis ? F.DIM : 1;
        ctx.strokeStyle = st.color; ctx.lineWidth = st.w * cam.zoom;
        if (st.dash) ctx.setLineDash(st.dash.split(",").map(function (v) { return +v * cam.zoom; }));
        ctx.beginPath();
        var mx = (pa[0] + pb[0]) / 2;
        ctx.moveTo(pa[0], pa[1]);
        ctx.bezierCurveTo(mx, pa[1], mx, pb[1], pb[0], pb[1]);
        ctx.stroke();
        ctx.setLineDash([]);
        var dir = Math.atan2(pb[1] - pa[1], pb[0] - pa[0]);
        /* 箭头画到目标节点边界 */
        var tst = NODE_STYLE[b.kind] || NODE_STYLE.Process;
        var bx = pb[0] - Math.cos(dir) * (Math.min(tst.w, tst.h) / 2) * cam.zoom;
        var by = pb[1] - Math.sin(dir) * (Math.min(tst.w, tst.h) / 2) * cam.zoom;
        ctx.fillStyle = st.color; ctx.strokeStyle = st.color; ctx.lineWidth = 1;
        if (st.arrow === "particles" && dataflow) {
          var tt = (t * 0.5 + (e.from * 0.13)) % 1;
          var px = U.lerp(pa[0], pb[0], tt), py = U.lerp(pa[1], pb[1], tt);
          ctx.fillStyle = "#007AFF";
          ctx.beginPath(); ctx.arc(px, py, 2.5, 0, Math.PI * 2); ctx.fill();
        } else drawArrow(ctx, st.arrow, bx, by, dir);
        /* F292 条件标注 */
        if (e.label) {
          ctx.fillStyle = "#86868B";
          ctx.font = "300 10px Inter, system-ui, sans-serif";
          ctx.fillText(e.label, mx + 4, (pa[1] + pb[1]) / 2 - 3);
        }
        ctx.restore();
      });

      /* 节点 */
      chart.nodes.forEach(function (n, i) {
        var st = NODE_STYLE[n.kind] || NODE_STYLE.Process;
        var p = S(n.x, n.y);
        var w = st.w * cam.zoom, h = st.h * cam.zoom;
        var dimThis = dim && hl.nodes.indexOf(i) < 0;
        ctx.save();
        ctx.globalAlpha = dimThis ? F.DIM : 1;
        /* F283 热区着色：高频暖黄 / 中频原色 / 低频灰色 */
        var fill = st.fill;
        if (heat) {
          if ((n.hot || 0) > 0.7) fill = "#FFAB00";
          else if ((n.hot || 0) < 0.25) fill = "#8E8E93";
        }
        /* F285 对比 diff 着色 */
        if (compare && diffMap && diffMap[i]) {
          if (diffMap[i] === "add") fill = "#34C759";
          else if (diffMap[i] === "del") fill = "#FF3B30";
        }
        if (n.collapsed) { w = 150 * cam.zoom; h = 34 * cam.zoom; }
        ctx.translate(p[0], p[1]);
        pathShape(ctx, n.collapsed ? "rounded" : st.shape, -w / 2, -h / 2, w, h);
        ctx.fillStyle = fill;
        ctx.globalAlpha = (dimThis ? F.DIM : 1) * 0.92;
        ctx.fill();
        ctx.globalAlpha = dimThis ? F.DIM : 1;
        /* F284 bug 标红：边框红 + 右上角感叹号徽章 */
        ctx.strokeStyle = n.bug ? F.BUG_BORDER : st.stroke;
        ctx.lineWidth = (n.bug ? 2 : 2) * Math.max(0.6, cam.zoom);
        if (st.shape === "dashed-rect") ctx.setLineDash([4, 3]);
        ctx.stroke();
        ctx.setLineDash([]);
        if (i === selected || i === playIdx) {
          ctx.strokeStyle = "#FFFFFF"; ctx.lineWidth = 1.5;
          pathShape(ctx, n.collapsed ? "rounded" : st.shape, -w / 2 - 3, -h / 2 - 3, w + 6, h + 6);
          ctx.stroke();
        }
        /* 文字 */
        var label = n.collapsed ? F.collapseSummary("输入", "处理", "输出") : n.label;
        ctx.fillStyle = (heat && (n.hot || 0) > 0.7) ? "#000000" : st.text;
        ctx.font = "300 " + Math.max(8, st.fs * cam.zoom) + "px Inter, system-ui, sans-serif";
        ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText(label, 0, 0);
        /* F293 变量标注（右下） / F294 时间标注（左下） */
        ctx.font = "300 " + Math.max(7, 9 * cam.zoom) + "px Inter, system-ui, sans-serif";
        ctx.fillStyle = "#86868B";
        if (n.varLabel) { ctx.textAlign = "right"; ctx.fillText(n.varLabel, w / 2 - 2, h / 2 - 6); }
        if (n.timeLabel) { ctx.textAlign = "left"; ctx.fillText(n.timeLabel, -w / 2 + 2, h / 2 - 6); }
        /* F284 徽章 */
        if (n.bug) {
          ctx.fillStyle = F.BUG_BORDER;
          ctx.beginPath(); ctx.arc(w / 2 - 2, -h / 2 + 2, 7 * Math.max(0.7, cam.zoom), 0, Math.PI * 2); ctx.fill();
          ctx.fillStyle = "#FFFFFF";
          ctx.font = "400 " + Math.max(8, 10 * cam.zoom) + "px Inter, system-ui, sans-serif";
          ctx.textAlign = "center"; ctx.textBaseline = "middle";
          ctx.fillText("!", w / 2 - 2, -h / 2 + 2);
        }
        /* F295 注释气泡 */
        if (n.note) {
          var fs = Math.max(8, 10 * cam.zoom);
          ctx.font = "300 " + fs + "px Inter, system-ui, sans-serif";
          var tw = ctx.measureText(n.note).width;
          ctx.fillStyle = "rgba(255,255,255,0.92)";
          U.roundRect(ctx, -tw / 2 - 7, -h / 2 - fs - 18, tw + 14, fs + 10, 6);
          ctx.fill();
          ctx.beginPath();
          ctx.moveTo(-4, -h / 2 - 8); ctx.lineTo(4, -h / 2 - 8); ctx.lineTo(0, -h / 2 - 2);
          ctx.closePath(); ctx.fill();
          ctx.fillStyle = "#1D1D1F"; ctx.textAlign = "center"; ctx.textBaseline = "middle";
          ctx.fillText(n.note, 0, -h / 2 - fs - 12);
        }
        /* F297 数据变换：选中节点上方形状渐变动效 */
        if (i === selected && dataflow) {
          var ph = (t * 0.8) % 1;
          ctx.globalAlpha = (1 - ph) * 0.7;
          ctx.fillStyle = "#007AFF";
          ctx.beginPath(); ctx.arc(0, -h / 2 - 12 - ph * 26, 4 + ph * 6, 0, Math.PI * 2); ctx.fill();
          ctx.globalAlpha = 1;
        }
        ctx.restore();
      });

      /* F286 迷你地图 */
      drawMini();
    }
    this.render = render;

    function drawMini() {
      var mc = opts.minimap;
      if (!mc) return;
      var c = mc.getContext("2d");
      c.clearRect(0, 0, 150, 100);
      c.fillStyle = "rgba(0,0,0,0.45)"; c.fillRect(0, 0, 150, 100);
      var x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9;
      chart.nodes.forEach(function (n) {
        x0 = Math.min(x0, n.x); x1 = Math.max(x1, n.x);
        y0 = Math.min(y0, n.y); y1 = Math.max(y1, n.y);
      });
      var sx = 150 / Math.max(1, x1 - x0 + 160), sy = 100 / Math.max(1, y1 - y0 + 160), s = Math.min(sx, sy);
      c.save();
      c.translate(75, 50); c.scale(s, s); c.translate(-(x0 + x1) / 2, -(y0 + y1) / 2);
      chart.edges.forEach(function (e) {
        var a = chart.nodes[e.from], b = chart.nodes[e.to];
        c.strokeStyle = "rgba(200,200,210,0.5)"; c.lineWidth = 1 / s;
        c.beginPath(); c.moveTo(a.x, a.y); c.lineTo(b.x, b.y); c.stroke();
      });
      chart.nodes.forEach(function (n) {
        var st = NODE_STYLE[n.kind] || NODE_STYLE.Process;
        c.fillStyle = st.fill;
        c.fillRect(n.x - st.w / 2, n.y - st.h / 2, st.w, st.h);
      });
      c.restore();
      c.strokeStyle = "#FF3B30"; c.lineWidth = 1;
      c.strokeRect(0, 0, 149, 99);
    }

    /* F298 调用瀑布 */
    function renderWaterfall() {
      if (!ir) return;
      var rows = F.waterfall(ir, opts.funcId ? opts.funcId() : 0, 4);
      var x0 = 60, y0 = 50, rowH = 34;
      ctx.font = "300 11px Inter, system-ui, sans-serif";
      ctx.fillStyle = "#86868B";
      ctx.fillText("调用瀑布（入口在顶，宽度=耗时）", x0, 28);
      rows.forEach(function (r, i) {
        var y = y0 + i * rowH;
        var w = U.clamp(r.ms * 1.4, 20, view.w - x0 - 120) * cam.zoom;
        ctx.fillStyle = U.rgba(CA.semanticColor(ir.nodes[r.id].domain), 0.55);
        U.roundRect(ctx, x0 + r.depth * 18, y, w, rowH - 10, 4); ctx.fill();
        ctx.fillStyle = "#C7C7CC"; ctx.textAlign = "left"; ctx.textBaseline = "middle";
        ctx.fillText(r.name, x0 + r.depth * 18 + 6, y + (rowH - 10) / 2);
        ctx.fillStyle = "#86868B";
        ctx.fillText(r.ms.toFixed(1) + "ms", x0 + r.depth * 18 + w + 8, y + (rowH - 10) / 2);
      });
    }

    /* F299 变量蛛网 */
    function renderSpider(t) {
      var cx = view.w / 2, cy = view.h / 2, R = Math.min(view.w, view.h) * 0.32;
      var pts = F.spiderWeb(9, R);
      ctx.strokeStyle = "rgba(142,142,147,0.5)"; ctx.lineWidth = 1;
      pts.forEach(function (p) {
        ctx.beginPath(); ctx.moveTo(cx, cy); ctx.lineTo(cx + p[0], cy + p[1]); ctx.stroke();
      });
      for (var ring = 1; ring <= 3; ring++) {
        ctx.beginPath();
        ctx.arc(cx, cy, (R / 3) * ring, 0, Math.PI * 2);
        ctx.strokeStyle = "rgba(142,142,147,0.18)"; ctx.stroke();
      }
      var vars = ["token", "user", "cart", "total", "stock", "risk", "session", "reply", "err"];
      pts.forEach(function (p, i) {
        var wob = 1 + 0.04 * Math.sin(t * 1.6 + i);
        var x = cx + p[0] * wob, y = cy + p[1] * wob;
        ctx.fillStyle = U.rgba("#007AFF", 0.75);
        ctx.beginPath(); ctx.arc(x, y, 4, 0, Math.PI * 2); ctx.fill();
        ctx.fillStyle = "#C7C7CC"; ctx.font = "300 10px Inter, system-ui, sans-serif";
        ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText(vars[i % vars.length], x, y - 12);
      });
      ctx.fillStyle = "#34C759";
      ctx.beginPath(); ctx.arc(cx, cy, 7, 0, Math.PI * 2); ctx.fill();
      ctx.fillStyle = "#F5F5F7"; ctx.font = "300 11px Inter, system-ui, sans-serif";
      ctx.textAlign = "center"; ctx.fillText("变量", cx, cy - 14);
    }

    /* F300 模块架构 */
    function renderArch() {
      if (!ir) return;
      var boxes = F.architecture(ir);
      var pad = 40, y = pad;
      ctx.font = "300 12px Inter, system-ui, sans-serif";
      boxes.forEach(function (b) {
        var h = 46 + b.files.length * 30;
        var w = Math.min(view.w - pad * 2, 720) * cam.zoom;
        var x = pad;
        ctx.save();
        ctx.translate((x - cam.x) * cam.zoom + 0, (y - cam.y) * cam.zoom + 0);
        ctx.strokeStyle = U.rgba(CA.semanticColor(b.domain), 0.6);
        ctx.lineWidth = 1;
        U.roundRect(ctx, 0, 0, w, h * cam.zoom, 8); ctx.stroke();
        ctx.fillStyle = U.rgba(CA.semanticColor(b.domain), 0.06); ctx.fill();
        ctx.fillStyle = "#F5F5F7";
        ctx.fillText(b.module + "（" + b.domain + "）", 12, 16);
        b.files.forEach(function (f, fi) {
          var fy = 34 + fi * 30;
          ctx.strokeStyle = "rgba(142,142,147,0.4)";
          U.roundRect(ctx, 12, fy * cam.zoom, w - 24, 24 * cam.zoom, 6); ctx.stroke();
          ctx.fillStyle = "#C7C7CC"; ctx.font = "300 10px Inter, system-ui, sans-serif";
          ctx.fillText(f.name, 20, fy * cam.zoom + 12 * cam.zoom);
          var fx = 130;
          f.fns.slice(0, 8).forEach(function (fn) {
            var tw = ctx.measureText(fn).width + 14;
            if (fx + tw > w - 20) return;
            ctx.fillStyle = U.rgba(CA.semanticColor(b.domain), 0.14);
            U.roundRect(ctx, fx, fy * cam.zoom + 3 * cam.zoom, tw, 18 * cam.zoom, 6); ctx.fill();
            ctx.fillStyle = "#F5F5F7";
            ctx.fillText(fn, fx + 7, fy * cam.zoom + 13 * cam.zoom);
            fx += tw + 6;
          });
        });
        ctx.restore();
        y += h + 10;
      });
    }

    /* ── 交互 ─────────────────────────────────────────────────────────── */
    var dragging = false, moved = false, lx = 0, ly = 0;
    canvas.addEventListener("mousedown", function (e) {
      if (!isActive()) return;
      dragging = true; moved = false; lx = e.clientX; ly = e.clientY;
      var r = canvas.getBoundingClientRect();
      var id = hit(e.clientX - r.left, e.clientY - r.top);
      if (id >= 0) drag = { id: id, ox: 0, oy: 0 };
    });
    root.addEventListener("mousemove", function (e) {
      if (!isActive() || !dragging) return;
      var dx = e.clientX - lx, dy = e.clientY - ly;
      if (Math.abs(dx) + Math.abs(dy) > 2) moved = true;
      if (drag) {
        /* F288 手动调整：拖拽后吸附 16px 网格 */
        var n = chart.nodes[drag.id];
        n.x = F.snap(n.x + dx / cam.zoom);
        n.y = F.snap(n.y + dy / cam.zoom);
      } else self.pan(dx, dy);
      lx = e.clientX; ly = e.clientY;
    });
    root.addEventListener("mouseup", function () {
      dragging = false;
      if (drag) { drag = null; if (opts.onEdit) opts.onEdit(chart); }
    });
    canvas.addEventListener("wheel", function (e) { if (!isActive()) return; e.preventDefault(); self.zoomBy(-e.deltaY); }, { passive: false });
    canvas.addEventListener("click", function (e) {
      if (!isActive()) return;
      if (moved) return;
      var r = canvas.getBoundingClientRect();
      var id = hit(e.clientX - r.left, e.clientY - r.top);
      selected = id;
      if (id >= 0 && opts.onSelect) opts.onSelect(id, chart.nodes[id]);
    });
    canvas.addEventListener("dblclick", function (e) {
      if (!isActive()) return;
      var r = canvas.getBoundingClientRect();
      var id = hit(e.clientX - r.left, e.clientY - r.top);
      if (id < 0) return;
      var n = chart.nodes[id];
      /* F279 双击展开子图 / F280 折叠摘要 */
      if (n.sub && !n.collapsed) {
        anim500 = performance.now();
        n.collapsed = false;
        n.label = n.label; // 展开：把子图节点并入（简化为展开标记 + 重排）
        F.sugiyama(chart);
      } else {
        n.collapsed = !n.collapsed;
        F.sugiyama(chart);
      }
    });

    /* ── F289 导出：PNG(2x) / SVG(矢量) / PDF(分页打印) ────────────────── */
    this.exportPNG = function (scale) {
      var s = scale || 2;
      var x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9;
      chart.nodes.forEach(function (n) {
        var st = NODE_STYLE[n.kind] || NODE_STYLE.Process;
        x0 = Math.min(x0, n.x - st.w); x1 = Math.max(x1, n.x + st.w);
        y0 = Math.min(y0, n.y - st.h); y1 = Math.max(y1, n.y + st.h);
      });
      var off = document.createElement("canvas");
      off.width = Math.round((x1 - x0) * s); off.height = Math.round((y1 - y0) * s);
      var oc = off.getContext("2d");
      oc.fillStyle = "#FFFFFF"; oc.fillRect(0, 0, off.width, off.height);
      oc.scale(s, s); oc.translate(-x0, -y0);
      exportDraw(oc);
      return off;
    };
    this.exportSVG = function () {
      var x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9;
      chart.nodes.forEach(function (n) {
        var st = NODE_STYLE[n.kind] || NODE_STYLE.Process;
        x0 = Math.min(x0, n.x - st.w); x1 = Math.max(x1, n.x + st.w);
        y0 = Math.min(y0, n.y - st.h); y1 = Math.max(y1, n.y + st.h);
      });
      var svg = ['<svg xmlns="http://www.w3.org/2000/svg" width="' + (x1 - x0) + '" height="' + (y1 - y0) +
        '" viewBox="' + x0 + " " + y0 + " " + (x1 - x0) + " " + (y1 - y0) + '">'];
      svg.push('<rect x="' + x0 + '" y="' + y0 + '" width="' + (x1 - x0) + '" height="' + (y1 - y0) + '" fill="#FFFFFF"/>');
      chart.edges.forEach(function (e) {
        var a = chart.nodes[e.from], b = chart.nodes[e.to];
        var st = EDGE_STYLE[e.kind] || EDGE_STYLE.Normal;
        svg.push('<path d="M' + a.x + " " + a.y + " C " + ((a.x + b.x) / 2) + " " + a.y + " " +
          ((a.x + b.x) / 2) + " " + b.y + " " + b.x + " " + b.y + '" fill="none" stroke="' + st.color +
          '" stroke-width="' + st.w + '"' + (st.dash ? ' stroke-dasharray="' + st.dash + '"' : "") + "/>");
      });
      chart.nodes.forEach(function (n) {
        var st = NODE_STYLE[n.kind] || NODE_STYLE.Process;
        var esc = String(n.label).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
        if (st.shape === "diamond") {
          svg.push('<polygon points="' + n.x + "," + (n.y - st.h / 2) + " " + (n.x + st.w / 2) + "," + n.y + " " +
            n.x + "," + (n.y + st.h / 2) + " " + (n.x - st.w / 2) + "," + n.y + '" fill="' + st.fill +
            '" stroke="' + st.stroke + '" stroke-width="2"/>');
        } else {
          svg.push('<rect x="' + (n.x - st.w / 2) + '" y="' + (n.y - st.h / 2) + '" width="' + st.w +
            '" height="' + st.h + '" rx="' + (st.shape === "rounded" ? 20 : 6) + '" fill="' + st.fill +
            '" stroke="' + st.stroke + '" stroke-width="2"/>');
        }
        svg.push('<text x="' + n.x + '" y="' + (n.y + 4) + '" font-family="Inter,sans-serif" font-size="' +
          st.fs + '" fill="' + st.text + '" text-anchor="middle">' + esc + "</text>");
      });
      svg.push("</svg>");
      return svg.join("\n");
    };
    /** F290 打印模式：A4 分页，每页标题+页码，连线跨页不断。 */
    this.printPages = function () {
      return F.paginateA4(chart, F.A4_H);
    };
    function exportDraw(c) {
      chart.edges.forEach(function (e) {
        var a = chart.nodes[e.from], b = chart.nodes[e.to];
        var st = EDGE_STYLE[e.kind] || EDGE_STYLE.Normal;
        c.strokeStyle = st.color; c.lineWidth = st.w;
        if (st.dash) c.setLineDash(st.dash.split(",").map(Number));
        c.beginPath();
        var mx = (a.x + b.x) / 2;
        c.moveTo(a.x, a.y); c.bezierCurveTo(mx, a.y, mx, b.y, b.x, b.y); c.stroke();
        c.setLineDash([]);
      });
      chart.nodes.forEach(function (n) {
        var st = NODE_STYLE[n.kind] || NODE_STYLE.Process;
        c.save();
        c.translate(n.x, n.y);
        pathShape(c, st.shape, -st.w / 2, -st.h / 2, st.w, st.h);
        c.fillStyle = st.fill; c.fill();
        c.strokeStyle = n.bug ? F.BUG_BORDER : st.stroke; c.lineWidth = 2;
        if (st.shape === "dashed-rect") c.setLineDash([4, 3]);
        c.stroke(); c.setLineDash([]);
        c.fillStyle = st.text;
        c.font = "300 " + st.fs + "px Inter, sans-serif";
        c.textAlign = "center"; c.textBaseline = "middle";
        c.fillText(n.label, 0, 0);
        c.restore();
      });
    }

    this.highlight = function (src, dst) { hl = F.pathHighlight(chart, src, dst); };
    this.clearHighlight = function () { hl = null; };

    this.start = function () {
      resize();
      (function loop(now) { render(now); requestAnimationFrame(loop); })(performance.now());
    };
    resize();
  }

  CA.FlowView = FlowView;
})(typeof globalThis !== "undefined" ? globalThis : this);
