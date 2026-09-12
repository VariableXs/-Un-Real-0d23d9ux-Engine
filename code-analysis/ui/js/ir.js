/* =============================================================================
 * AI-04 · 域数据层（#251~#336 消费侧）
 *   统一语义 IR（项目→模块→子系统→文件→类→函数→行 七级）+ 结构哈希（F336）。
 *   与 core crate 的 ir.rs / model.rs 契约一一对应：core 产出，UI 只消费。
 *   纯数据、无 DOM 依赖 —— 壳A/B/C 三端与浏览器均可直接加载。
 * ========================================================================== */
(function (root) {
  "use strict";
  var CA = (root.CA = root.CA || {});

  /* ── 通用工具 ─────────────────────────────────────────────────────────── */
  var U = (CA.util = {
    clamp: function (v, a, b) { return v < a ? a : v > b ? b : v; },
    lerp: function (a, b, t) { return a + (b - a) * t; },
    /** FNV-1a（F336 结构哈希用）。 */
    hash: function (str) {
      var h = 0x811c9dc5;
      for (var i = 0; i < str.length; i++) {
        h ^= str.charCodeAt(i);
        h = (h + ((h << 1) + (h << 4) + (h << 7) + (h << 8) + (h << 24))) >>> 0;
      }
      return h >>> 0;
    },
    /** 确定性伪随机：同种子必得同序列（星空/爆炸角度/演示数据都靠它）。 */
    rng: function (seed) {
      var s = (seed >>> 0) || 1;
      return function () {
        s ^= s << 13; s >>>= 0;
        s ^= s >>> 17;
        s ^= s << 5; s >>>= 0;
        return s / 4294967296;
      };
    },
    hexRgb: function (hex) {
      var h = hex.replace("#", "");
      if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
      var n = parseInt(h, 16);
      return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    },
    rgba: function (hex, a) {
      var c = U.hexRgb(hex);
      return "rgba(" + c[0] + "," + c[1] + "," + c[2] + "," + a + ")";
    },
    mix: function (hexA, hexB, t) {
      var a = U.hexRgb(hexA), b = U.hexRgb(hexB);
      return "rgb(" + Math.round(U.lerp(a[0], b[0], t)) + "," +
        Math.round(U.lerp(a[1], b[1], t)) + "," + Math.round(U.lerp(a[2], b[2], t)) + ")";
    },
    easeOutCubic: function (p) { return 1 - Math.pow(1 - p, 3); },
    easeInOut: function (p) { return p < 0.5 ? 2 * p * p : 1 - Math.pow(-2 * p + 2, 2) / 2; },
    roundRect: function (ctx, x, y, w, h, r) {
      var rr = Math.min(r, w / 2, h / 2);
      ctx.beginPath();
      ctx.moveTo(x + rr, y);
      ctx.arcTo(x + w, y, x + w, y + h, rr);
      ctx.arcTo(x + w, y + h, x, y + h, rr);
      ctx.arcTo(x, y + h, x, y, rr);
      ctx.arcTo(x, y, x + w, y, rr);
      ctx.closePath();
    }
  });

  /* ── F256 语义着色：色相环等分，规格固定四例色值优先 ─────────────────── */
  var DOMAIN_COLOR = { 用户: "#007AFF", 订单: "#34C759", 支付: "#FF9500", 数据: "#AF52DE" };
  CA.semanticColor = function (domain) { return DOMAIN_COLOR[domain] || "#8E8E93"; };
  CA.DOMAIN_COLOR = DOMAIN_COLOR;

  /* ── F307 颜色即含义（全界面统一） ────────────────────────────────────── */
  CA.statusColor = function (status) {
    switch (status) {
      case "bug": return "#FF3B30";
      case "warn": return "#FFAB00";
      case "dead": return "#8E8E93";
      default: return "#34C759";
    }
  };

  /* ── F306 比喻图标（emoji 映射） ──────────────────────────────────────── */
  CA.metaphorIcon = function (name) {
    var n = String(name || "").toLowerCase();
    if (/login|token|session|auth|password|risk|verify/.test(n)) return "🔒";
    if (/db|query|insert|cache|migrate|queue|store|storage|save/.test(n)) return "📦";
    if (/^load|^get|entry|start|open|init/.test(n)) return "🚪";
    if (/loop|drain|refresh|retry|each|scan/.test(n)) return "🔄";
    if (/calc|total|perf|bench|hot|charge|pay/.test(n)) return "⚡";
    return "•";
  };

  /* ── F309 生活类比：项目=餐厅 ─────────────────────────────────────────── */
  CA.lifeAnalogy = function (part) {
    var m = {
      用户: "前台（登记身份、发通行证）",
      订单: "点餐台（记下客人要什么）",
      支付: "收银台（收钱、退款）",
      数据: "后厨仓库（存取原料）",
      入口: "大门（客人从这里进来）"
    };
    return m[part] || "餐厅里的一个工位";
  };

  /* ── 七级下钻层级（#001 承接，AI-04 消费） ────────────────────────────── */
  var KIND_NAME = ["project", "module", "subsystem", "file", "class", "func", "line"];
  var LEVEL_LABEL = ["L1 项目", "L2 模块", "L3 子系统", "L4 文件", "L5 类", "L6 函数", "L7 行"];
  CA.KIND_NAME = KIND_NAME;
  CA.LEVEL_LABEL = LEVEL_LABEL;

  /* ── 演示工程规格（真实项目由 core::open_project 产出同一结构） ─────────── */
  var SPEC = {
    name: "auth-service",
    lang: "TypeScript",
    modules: [
      {
        key: "user", domain: "用户", cls: "Service",
        files: [
          ["user/login.ts", 186, [
            ["login", "核对账号密码，对上了就发一张通行证", 42, 6, 0.82, 0.72, "warn"],
            ["verifyToken", "检查这张通行证是不是真的、过没过期", 28, 4, 0.95, 0.31, null],
            ["hashPassword", "把密码打碎成一串谁也认不出的字符", 36, 3, 0.60, 0.18, null]
          ]],
          ["user/profile.ts", 142, [
            ["loadUser", "按名字把这位客人的档案翻出来", 54, 7, 0.74, 0.55, null],
            ["updateAvatar", "换一张头像照片", 31, 3, 0.40, 0.12, "dead"]
          ]],
          ["user/session.ts", 168, [
            ["createSession", "开一张新的通行证并存进柜子", 47, 5, 0.88, 0.63, null],
            ["refreshSession", "快过期了，续一张新的", 33, 6, 0.51, 0.29, "warn"],
            ["revokeSession", "把通行证作废", 19, 2, 0.77, 0.08, null]
          ]]
        ]
      },
      {
        key: "order", domain: "订单", cls: "Store",
        files: [
          ["order/cart.ts", 154, [
            ["addItem", "往购物车里放一件商品", 38, 4, 0.80, 0.66, null],
            ["removeItem", "从购物车里拿掉一件", 22, 2, 0.70, 0.21, null],
            ["calcTotal", "把车里所有东西加个总价", 44, 8, 0.69, 0.58, "warn"]
          ]],
          ["order/checkout.ts", 231, [
            ["placeOrder", "下单：查库存、算钱、扣款、排队发货", 72, 11, 0.61, 0.84, "bug"],
            ["validateStock", "看看仓库里还有没有货", 29, 5, 0.90, 0.44, null]
          ]]
        ]
      },
      {
        key: "pay", domain: "支付", cls: "Gateway",
        files: [
          ["pay/gateway.ts", 208, [
            ["charge", "真正去收钱，收完记一笔账", 66, 9, 0.57, 0.91, "bug"],
            ["refund", "把钱退回去", 41, 6, 0.48, 0.27, "warn"]
          ]],
          ["pay/risk.ts", 132, [
            ["scanRisk", "扫一遍这笔生意有没有风险", 58, 12, 0.52, 0.49, "warn"]
          ]]
        ]
      },
      {
        key: "data", domain: "数据", cls: "Repo",
        files: [
          ["data/db.ts", 247, [
            ["query", "去仓库里查东西", 51, 5, 0.86, 0.77, null],
            ["insert", "往仓库里存东西", 35, 3, 0.79, 0.52, null],
            ["migrate", "搬仓库，把旧架子换成新架子", 88, 7, 0.23, 0.05, "dead"]
          ]],
          ["data/cache.ts", 96, [
            ["getCache", "先从手边的小柜子里找（快）", 24, 2, 0.92, 0.83, null],
            ["setCache", "顺手放一份到小柜子里", 18, 2, 0.90, 0.70, null]
          ]],
          ["data/queue.ts", 118, [
            ["enqueue", "把要办的事排进队伍", 27, 3, 0.65, 0.33, null],
            ["drainLoop", "一直干活，直到队伍空了", 46, 6, 0.44, 0.38, null]
          ]]
        ]
      }
    ],
    /* 调用边：[from, to, 频率 0~1] */
    calls: [
      ["login", "verifyToken", 0.92], ["login", "loadUser", 0.81], ["login", "createSession", 0.70],
      ["verifyToken", "getCache", 0.58], ["loadUser", "query", 0.74], ["loadUser", "getCache", 0.51],
      ["updateAvatar", "insert", 0.31], ["createSession", "setCache", 0.63], ["createSession", "insert", 0.44],
      ["refreshSession", "verifyToken", 0.52], ["refreshSession", "setCache", 0.41], ["revokeSession", "setCache", 0.22],
      ["addItem", "calcTotal", 0.60], ["removeItem", "calcTotal", 0.48], ["calcTotal", "query", 0.43],
      ["placeOrder", "validateStock", 0.83], ["placeOrder", "calcTotal", 0.72], ["placeOrder", "charge", 0.90],
      ["placeOrder", "enqueue", 0.53], ["validateStock", "query", 0.61],
      ["charge", "scanRisk", 0.78], ["charge", "insert", 0.52], ["charge", "enqueue", 0.42],
      ["refund", "insert", 0.40], ["refund", "enqueue", 0.30],
      ["scanRisk", "query", 0.50], ["scanRisk", "getCache", 0.38],
      ["migrate", "insert", 0.21], ["drainLoop", "query", 0.60], ["drainLoop", "insert", 0.47],
      ["getCache", "query", 0.34]
    ]
  };

  var LINE_TEMPLATES = [
    "把传进来的参数先检查一遍", "从缓存里试着取一次结果", "缓存没有，只能去查库",
    "把查到的东西整理成要的样子", "这里要小心：为空就会崩", "算出一个中间值放着",
    "满足条件才继续往下走", "把结果写回仓库", "记一行日志，方便以后查",
    "收尾：把用过的东西还回去"
  ];

  /** 生成演示 IR（结构与 core::open_project 一致）。 */
  function buildIR(spec) {
    spec = spec || SPEC;
    var nodes = [];
    function add(kind, name, parent, extra) {
      var n = {
        id: nodes.length, kind: kind, kindName: KIND_NAME[kind], name: name,
        parent: parent == null ? -1 : parent, children: [], loc: 1,
        domain: "", plain: "", icon: "", sig: "", cc: 0, cov: 0, hot: 0,
        status: "", loop: false, x: 0, y: 0
      };
      if (extra) for (var k in extra) if (Object.prototype.hasOwnProperty.call(extra, k)) n[k] = extra[k];
      if (parent != null && parent >= 0) nodes[parent].children.push(n.id);
      nodes.push(n);
      return n.id;
    }

    var rootId = add(0, spec.name, -1, { plain: "整个项目：一家餐厅", icon: "🚪", loc: 1 });
    var funcByName = {};
    var rnd = U.rng(0x9e3779b9);

    spec.modules.forEach(function (mod, mi) {
      var modId = add(1, mod.key, rootId, {
        domain: mod.domain, plain: CA.lifeAnalogy(mod.domain), icon: CA.metaphorIcon(mod.key), loc: 0
      });
      mod.files.forEach(function (f, fi) {
        var rel = f[0], fileLoc = f[1], funcs = f[2];
        var parts = rel.split("/");
        var parent = modId;
        // 三级目录 → 子系统节点
        if (parts.length > 2) {
          parent = add(2, parts[1], modId, { domain: mod.domain, loc: 0, plain: "一组相关的文件" });
        }
        var fileId = add(3, parts[parts.length - 1], parent, {
          domain: mod.domain, loc: fileLoc, plain: "一个文件：一叠纸", icon: "📄"
        });
        nodes[modId].loc += fileLoc;
        var clsName = mod.key.charAt(0).toUpperCase() + mod.key.slice(1) + mod.cls;
        var clsId = add(4, clsName, fileId, { domain: mod.domain, loc: fileLoc, plain: "一个工具箱", icon: "🧰" });
        funcs.forEach(function (fn) {
          var fname = fn[0], plain = fn[1], loc = fn[2], cc = fn[3], cov = fn[4], hot = fn[5], status = fn[6];
          var sig = "fn " + fname + "(ctx: &Ctx, req: Request) -> Result<Reply, Error>";
          var fid = add(5, fname, clsId, {
            domain: mod.domain, plain: plain, icon: CA.metaphorIcon(fname),
            loc: loc, cc: cc, cov: cov, hot: hot, status: status || "",
            loop: /loop|drain|refresh|retry/i.test(fname), sig: sig
          });
          funcByName[fname] = fid;
          // L7 行级：确定性生成，行数 ∝ 函数行数
          var lines = U.clamp(Math.round(loc / 6), 4, 10);
          for (var i = 0; i < lines; i++) {
            add(6, "L" + (i * 7 + 3), fid, {
              domain: mod.domain,
              plain: "第 " + (i * 7 + 3) + " 行：" + LINE_TEMPLATES[(i + mi + fi) % LINE_TEMPLATES.length],
              sig: "line " + (i * 7 + 3),
              loc: 1, hot: hot * (0.4 + 0.6 * rnd()),
              status: (status === "bug" && i === 2) ? "bug" : ""
            });
          }
        });
      });
    });

    var edges = [];
    spec.calls.forEach(function (c) {
      var a = funcByName[c[0]], b = funcByName[c[1]];
      if (a == null || b == null) return;
      edges.push({ from: a, to: b, freq: c[2], kind: "call" });
    });

    // 低频依赖边（F296 依赖连线）
    for (var i = 0; i < nodes.length; i++) {
      if (nodes[i].kind === 5 && i % 5 === 0) {
        var t = nodes[(i * 7 + 11) % nodes.length];
        if (t && t.kind === 5 && t.id !== i) edges.push({ from: i, to: t.id, freq: 0.12, kind: "dep" });
      }
    }

    var loc = 0, fileCount = 0;
    nodes.forEach(function (n) { if (n.kind === 3) { fileCount++; loc += n.loc; } });

    var ir = {
      name: spec.name, lang: spec.lang, nodes: nodes, edges: edges, root: rootId,
      meta: { fileCount: fileCount, loc: loc, funcCount: Object.keys(funcByName).length },
      byName: funcByName
    };
    ir.structureHash = CA.structureHash(ir);
    return ir;
  }

  /** F336 结构哈希：切换前后校验 AST（(id, kind, 子节点数) 三元组 → FNV-1a）。 */
  CA.structureHash = function (ir) {
    var s = "";
    for (var i = 0; i < ir.nodes.length; i++) {
      var n = ir.nodes[i];
      s += n.id + ":" + n.kind + ":" + n.children.length + "|";
    }
    return U.hash(s);
  };

  /** 取某级别的全部节点（七级下钻：L1 项目 … L7 行）。 */
  CA.nodesAtLevel = function (ir, level) {
    var out = [];
    for (var i = 0; i < ir.nodes.length; i++) if (ir.nodes[i].kind === level - 1) out.push(ir.nodes[i]);
    return out;
  };

  /** 从根到 id 的路径（面包屑 / F334 占位符用）。 */
  CA.pathTo = function (ir, id) {
    var path = [], cur = id;
    while (cur >= 0) { path.push(cur); cur = ir.nodes[cur].parent; }
    path.reverse();
    return path;
  };

  /**
   * 外部 JSON → 可渲染 IR（F315 载入真实工程数据）。
   * 两种形态都收：
   *   1) 规格形态（分析器最省事）：
   *      { name, lang, modules:[{key,domain,cls,files:[[rel,loc,[[fn,plain,loc,cc,cov,hot,status]]]]}],
   *        calls:[[from,to,freq]] }
   *   2) 完整 IR 转储：{ name, lang, root, nodes:[{id,kind,name,parent,...}], edges:[{from,to,freq,kind}] }
   * 结构不对返回 null（调用方据此回退到内置演示 IR）。
   */
  CA.irFromJSON = function (json) {
    if (!json || typeof json !== "object") return null;
    if (Array.isArray(json.modules) && json.modules.length) return buildIR(json);
    if (!Array.isArray(json.nodes) || !json.nodes.length) return null;

    var nodes = [], byName = {};
    for (var i = 0; i < json.nodes.length; i++) {
      var s = json.nodes[i] || {};
      var k = +s.kind || 0;
      var n = {
        id: i, kind: k, kindName: KIND_NAME[k], name: String(s.name || ("n" + i)),
        parent: (s.parent == null || +s.parent < 0) ? -1 : (+s.parent), children: [],
        loc: +s.loc || 1, domain: s.domain || "", plain: s.plain || "", icon: s.icon || "",
        sig: s.sig || "", cc: +s.cc || 0, cov: +s.cov || 0, hot: +s.hot || 0,
        status: s.status || "", loop: !!s.loop, x: 0, y: 0
      };
      nodes.push(n);
      if (k === 5) byName[n.name] = i;
    }
    /* children 一律按 parent 重建：外部 children 可能与 parent 不一致 */
    nodes.forEach(function (n) {
      if (n.parent >= 0 && n.parent < nodes.length && n.parent !== n.id) nodes[n.parent].children.push(n.id);
      else n.parent = -1;
    });
    var edges = (json.edges || []).map(function (e) {
      return { from: +e.from, to: +e.to, freq: +e.freq || 0.2, kind: e.kind || "call" };
    }).filter(function (e) {
      return nodes[e.from] && nodes[e.to] && e.from !== e.to;
    });
    var root = (json.root == null || !nodes[+json.root]) ? 0 : +json.root;
    var fileCount = 0, loc = 0, funcCount = 0;
    nodes.forEach(function (n) {
      if (n.kind === 3) { fileCount++; loc += n.loc; }
      if (n.kind === 5) funcCount++;
    });
    var ir = {
      name: String(json.name || "项目"), lang: json.lang || "rust",
      nodes: nodes, edges: edges, root: root,
      meta: { fileCount: fileCount, loc: loc, funcCount: funcCount },
      byName: byName
    };
    ir.structureHash = CA.structureHash(ir);
    return ir;
  };

  CA.buildIR = buildIR;
  CA.DEMO_SPEC = SPEC;
})(typeof globalThis !== "undefined" ? globalThis : this);
