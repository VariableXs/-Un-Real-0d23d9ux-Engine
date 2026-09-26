/* ═══════════════════════════════════════════════════════════════════════════
 * v4 · 无限全景画布 · OBSIDIAN 光环境（F251~F275）
 *   五层渲染管线：
 *     L0 深空底色 + 极光星云（预渲染发光精灵，screen 叠加，缓慢漂移）
 *     L1 世界坐标点阵网格 + 主格线（无限画布的空间锚）
 *     L2 边层（域色低透明基线 → 相关边辉光渐变 + 流动虚线 + 软光粒子）
 *     L3 节点卡片（物理阴影 + 受光斜面 + 域色脊柱 + 指标微条 + 状态辉光）
 *     L4 摄影后期（暗角 + 胶片噪点封层）
 *   契约：API 与常量与 core::canvas.rs 一一对应；CA.CanvasView 全方法保留。
 * ═══════════════════════════════════════════════════════════════════════ */
(function (root) {
  "use strict";
  var CA = (root.CA = root.CA || {});
  var U = CA.util;

  /* ── 规格常量（Core 契约透传） ─────────────────────────────────────────── */
  var C = (CA.CANVAS = {
    BG_LIGHT: "#FFFFFF", BG_DARK: "#0A0A0F", NEON_BG: "#000000",
    /* C17 壁纸透出：深色普通态画布底全透明（#wallpaper 1:1 透出）；
     * Bloom 态 0.9 黑（氛围自持、壁纸微透），浅色态近实底（壁纸是深色系）。 */
    CANVAS_ALPHA_BLOOM: 0.9, CANVAS_ALPHA_LIGHT: 0.92,
    MINIMAP_W: 150, MINIMAP_H: 100,
    REST: 120, REPULSION_K: 4000, GRAVITY_K: 0.01, STEP: 0.1,
    NODE_MIN: 16, NODE_MAX: 200,
    BLOOM_BLUR: 16, BLOOM_STRENGTH: 1.5, NEON_THRESHOLD: 0.7,
    RAIN_COLOR: "#34C759", RAIN_ALPHA: 0.05, RAIN_SPEED: 2,
    SPECTRUM_BARS: 64, NEBULA_ALPHA: 0.05, PULSE_COLOR: "#FF00FF",
    EXPLODE_MS: 500, DEGRADE_AT: 10000,
    /* v4 光环境参数 */
    GRID_MINOR: 96, GRID_MAJOR: 5, ACCENT: "#5B9DFF", ACCENT2: "#8B6CFF",
    NEBULA: ["#2E5FD0", "#6B4CD8", "#1F7A68"],
    CAM_EASE: 9.5
  });

  /** F253 LOD：>80% 行级，30~80% 函数签名，<30% 模块名。 */
  function lodOf(zoom) { return zoom > 0.8 ? "line" : zoom >= 0.3 ? "func" : "module"; }

  /** F254 自适应密度：面积 = 行数 × 系数，钳制 16~200。 */
  function nodeSize(loc) { return U.clamp(loc * 0.5, C.NODE_MIN, C.NODE_MAX); }

  /** F257 发光参数：大小=代码量(8~64)，亮度=调用频率(0.3~1)。 */
  function glowOf(loc, freq) { return { size: U.clamp(loc / 10, 8, 64), bright: U.clamp(freq, 0.3, 1) }; }

  /** F258 光束样式：线宽=调用频率对数(1~4)，透明度 0.3~0.8。 */
  function beamStyle(freq) { return { w: U.clamp(Math.log(Math.max(freq, 0.1)) * 1.2, 1, 4), a: U.clamp(freq, 0.3, 0.8) }; }

  /** F265 渐变数据河：#007AFF → #AF52DE → #FF2D95。 */
  function riverColor(t) {
    var stops = [[0, 122 / 255, 1], [172 / 255, 82 / 255, 222 / 255], [1, 45 / 255, 149 / 255]];
    t = U.clamp(t, 0, 1);
    var a = stops[0], b = stops[1], m = t * 2;
    if (t >= 0.5) { a = stops[1]; b = stops[2]; m = (t - 0.5) * 2; }
    return "rgb(" + Math.round(U.lerp(a[0], b[0], m) * 255) + "," +
      Math.round(U.lerp(a[1], b[1], m) * 255) + "," + Math.round(U.lerp(a[2], b[2], m) * 255) + ")";
  }

  /** F266 呼吸：周期 4s，亮度振幅 ±10%，全局同步。 */
  function breathe(t) { return 1 + 0.1 * Math.sin((Math.PI * 2 * t) / 4); }

  /** F273 时间光影：6 暖黄 / 12 冷白 / 18 暖橙 / 22 冷蓝。 */
  function dayTint(hour) {
    var st = [[6, [1, .93, .70]], [12, [1, 1, 1]], [18, [1, .80, .60]], [22, [.70, .80, 1]]];
    var h = ((hour % 24) + 24) % 24, a = st[st.length - 1], b = st[0], mix = 0;
    for (var i = 0; i < st.length; i++) {
      var n = st[(i + 1) % st.length];
      if (h >= st[i][0] && h < n[0]) { a = st[i]; b = n; mix = (h - a[0]) / (n[0] - a[0]); break; }
    }
    return [U.lerp(a[1][0], b[1][0], mix), U.lerp(a[1][1], b[1][1], mix), U.lerp(a[1][2], b[1][2], mix)];
  }

  /** F275 爆炸展开：子节点 500ms ease-out 飞出。 */
  function explode(p, angles, dist, tMs) {
    var p1 = U.clamp(tMs / C.EXPLODE_MS, 0, 1), e = U.easeOutCubic(p1);
    return angles.map(function (a) { return [p[0] + dist * Math.cos(a) * e, p[1] + dist * Math.sin(a) * e]; });
  }

  /**
   * 画布视图。
   * opts: { minimap, onSelect, onHover, onStatus, isActive }
   */
  function CanvasView(canvas, opts) {
    opts = opts || {};
    var self = this;
    var ctx = canvas.getContext("2d");
    var mm = opts.minimap ? opts.minimap.getContext("2d") : null;

    var ir = null, level = 2, mode = "plain", theme = "dark";
    var cam = { x: 0, y: 0, zoom: 1 };          /* 当前镜头（渲染与命中的真值） */
    var camT = { x: 0, y: 0, zoom: 1 };         /* 目标镜头（fit/focus 设定，render 内缓动逼近） */
    var view = { w: 800, h: 600, dpr: 1 };
    var isActive = opts.isActive || function () { return true; };
    var compare = false, compareSplit = 0.5;
    /* F331 固定布局引擎：首次计算全局坐标，后续只改内容 */
    var coords = {};       // id -> [x,y]
    var layoutDone = false;
    var selected = -1, hovered = -1;
    var visible = [];      // 当前级别可见节点
    var bbox = [0, 0, 0, 0];
    var t0 = performance.now(), last = t0;
    var stars = null, rain = [], particles = [];
    var explodeAnim = null;
    var flags = {
      particles: true, glow: true, bloom: false, heat: false, rain: false,
      nebula: true, spectrum: false, breathe: true, orbit: true, trail: true, tint: true, glass: true
    };
    var degraded = false;
    var hoverPos = { x: 0, y: 0 };
    var searchHits = null;
    var fps = 60, fpsInit = false;

    /* ── 尺寸 / DPR（4K 级渲染：主画布 DPR 上限 3）── */
    function resize() {
      var r = canvas.getBoundingClientRect();
      view.w = Math.max(1, r.width); view.h = Math.max(1, r.height);
      view.dpr = Math.min(3, root.devicePixelRatio || 1);
      canvas.width = Math.round(view.w * view.dpr);
      canvas.height = Math.round(view.h * view.dpr);
      ctx.setTransform(view.dpr, 0, 0, view.dpr, 0, 0);
      vigCache.w = 0;   /* 视口变了 → 暗角重建 */
      bgCache.w = 0;    /* 视口变了 → 深空底色重建 */
      if (opts.minimap) {
        opts.minimap.width = Math.round(C.MINIMAP_W * view.dpr);
        opts.minimap.height = Math.round(C.MINIMAP_H * view.dpr);
        mm.setTransform(view.dpr, 0, 0, view.dpr, 0, 0);
      }
    }
    this.resize = resize;

    function toScreen(wx, wy) {
      return [(wx - cam.x) * cam.zoom + view.w / 2, (wy - cam.y) * cam.zoom + view.h / 2];
    }
    function toWorld(sx, sy) {
      return [(sx - view.w / 2) / cam.zoom + cam.x, (sy - view.h / 2) / cam.zoom + cam.y];
    }
    this.toScreen = toScreen; this.toWorld = toWorld;

    /* ── F252 语义引力布局（v3 起：边弹簧 + 中心引力）
     * 「调用边=弹簧、斥力防重叠、弱中心引力防漂」，结构跟随真实调用关系。 */
    function layout() {
      visible = CA.nodesAtLevel(ir, level);
      var ids = visible.map(function (n) { return n.id; });
      var idSet = new Set(ids);
      var indexOf = new Map(ids.map(function (id, i) { return [id, i]; }));
      var bodies = ids.map(function (id, i) {
        var n = ir.nodes[id];
        var a = (i / Math.max(1, ids.length)) * Math.PI * 2;
        var r = 200 + 280 * ((i % 5) / 5);
        return {
          x: coords[id] ? coords[id][0] : Math.cos(a) * r,
          y: coords[id] ? coords[id][1] : Math.sin(a) * r,
          repulsion: 0.4 + 0.6 * (n.hot || 0.1)
        };
      });
      /* 边弹簧表：rest 长度按两端节点盒尺寸走，大节点离得远 */
      var springs = [];
      ir.edges.forEach(function (e) {
        if (!idSet.has(e.from) || !idSet.has(e.to)) return;
        var a = ir.nodes[e.from], b = ir.nodes[e.to];
        springs.push([
          indexOf.get(e.from), indexOf.get(e.to),
          90 + (nodeSize(a.loc) + nodeSize(b.loc)) * 0.22
        ]);
      });
      /* 冻结防护（QA 实测：9 万节点项目 L4/L5 会 O(n²) 卡死）：
       * n>1500 跳过力导向直接环形确定性排布；n>150 缩减迭代。 */
      if (!layoutDone) {
        if (bodies.length > 1500) {
          bodies.forEach(function (b, i) {
            var a = (i / bodies.length) * Math.PI * 2;
            var rr = 60 + 14 * Math.sqrt(bodies.length);
            b.x = Math.cos(a) * rr; b.y = Math.sin(a) * rr;
          });
        } else {
          var iters = bodies.length > 150 ? 60 : 240;
          for (var it = 0; it < iters; it++) if (forceStep(bodies, springs) < 0.05) break;
        }
        layoutDone = true;
      }
      bodies.forEach(function (b, i) {
        var id = ids[i];
        coords[id] = [b.x, b.y];
      });
      computeBBox();
      buildParticles();
    }
    function forceStep(bs, springs) {
      var n = bs.length, fx = new Float64Array(n), fy = new Float64Array(n), i, j;
      for (i = 0; i < n; i++) for (j = i + 1; j < n; j++) {
        var dx = bs[j].x - bs[i].x, dy = bs[j].y - bs[i].y;
        var d = Math.max(1, Math.sqrt(dx * dx + dy * dy));
        var ux = dx / d, uy = dy / d;
        var f = bs[i].repulsion * bs[j].repulsion * C.REPULSION_K / (d * d);
        fx[i] -= ux * f; fy[i] -= uy * f; fx[j] += ux * f; fy[j] += uy * f;
        fx[i] -= bs[i].x * 0.0012; fy[i] -= bs[i].y * 0.0012;
        fx[j] -= bs[j].x * 0.0012; fy[j] -= bs[j].y * 0.0012;
      }
      for (i = 0; i < springs.length; i++) {
        var sp = springs[i], a = bs[sp[0]], b = bs[sp[1]];
        if (!a || !b) continue;
        var ddx = b.x - a.x, ddy = b.y - a.y;
        var dd = Math.max(1, Math.sqrt(ddx * ddx + ddy * ddy));
        var k = (dd - sp[2]) * 0.018;
        var ux2 = ddx / dd, uy2 = ddy / dd;
        fx[sp[0]] += ux2 * k; fy[sp[0]] += uy2 * k;
        fx[sp[1]] -= ux2 * k; fy[sp[1]] -= uy2 * k;
      }
      var mv = 0;
      for (i = 0; i < n; i++) {
        var vx = U.clamp(fx[i] * C.STEP, -14, 14), vy = U.clamp(fy[i] * C.STEP, -14, 14);
        bs[i].x += vx; bs[i].y += vy;
        mv = Math.max(mv, Math.abs(vx) + Math.abs(vy));
      }
      return mv;
    }
    function computeBBox() {
      if (!visible.length) { bbox = [0, 0, 1, 1]; return; }
      var x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9;
      visible.forEach(function (n) {
        var c = coords[n.id] || [0, 0];
        x0 = Math.min(x0, c[0]); y0 = Math.min(y0, c[1]);
        x1 = Math.max(x1, c[0]); y1 = Math.max(y1, c[1]);
      });
      var pad = 140;
      bbox = [x0 - pad, y0 - pad, x1 + pad, y1 + pad];
    }

    /* ── F259 数据流粒子 ───────────────────────────────────────────────── */
    function buildParticles() {
      particles = [];
      if (!ir) return;
      ir.edges.forEach(function (e) {
        if (e.kind !== "call") return;
        if (coords[e.from] == null || coords[e.to] == null) return;
        particles.push({ e: e, n: e.freq > 0.6 ? 3 : 2, off: U.hash(String(e.from) + ":" + e.to) % 100 / 100 });
      });
    }

    /* ── F267 星空：双层视差 + 相位闪烁 ────────────────────────────────── */
    function buildStars() {
      var r = U.rng(20240913), n = 110 + Math.floor(r() * 130), s = [];
      for (var i = 0; i < n; i++) {
        s.push({
          x: r(), y: r(),
          z: i % 3 === 0 ? 0.75 : 0.35,
          sz: 0.8 + r() * 1.3,
          a: 0.05 + r() * 0.10,
          ph: r() * Math.PI * 2,
          tw: 0.5 + r() * 1.4
        });
      }
      stars = s;
    }
    function buildRain() {
      rain = [];
      var r = U.rng(777);
      for (var i = 0; i < 40; i++) rain.push({ x: r(), y: r() * 600, ch: String.fromCharCode(0x30a0 + Math.floor(r() * 90)) });
    }

    /* ── v4 发光精灵系统：域色 / 粒子光点均预渲染离屏缓存 ──────────────── */
    var spriteCache = {};
    function glowSprite(color) {
      if (spriteCache[color]) return spriteCache[color];
      var S = 256;
      var cv = document.createElement("canvas");
      cv.width = S; cv.height = S;
      var g2 = cv.getContext("2d");
      var grad = g2.createRadialGradient(S / 2, S / 2, 0, S / 2, S / 2, S / 2);
      grad.addColorStop(0, U.rgba(color, 0.50));
      grad.addColorStop(0.30, U.rgba(color, 0.16));
      grad.addColorStop(0.65, U.rgba(color, 0.045));
      grad.addColorStop(1, U.rgba(color, 0));
      g2.fillStyle = grad;
      g2.fillRect(0, 0, S, S);
      spriteCache[color] = cv;
      return cv;
    }
    /* 极光星云：三张大尺度柔光云（蓝/紫/青），慢速漂移形成深空氛围 */
    var nebulae = null;
    function buildNebulae() {
      var r = U.rng(0xC0FFEE);
      nebulae = C.NEBULA.map(function (col, i) {
        return {
          sp: glowSprite(col),
          bx: r(), by: r(),
          ax: 0.10 + r() * 0.16, ay: 0.08 + r() * 0.12,
          wx: 0.05 + r() * 0.05, wy: 0.04 + r() * 0.05,
          ph: r() * Math.PI * 2, sc: 1.0 + r() * 0.9, al: 0.16 + i * 0.02
        };
      });
    }
    /* 暗角渐变缓存（按视口尺寸）——摄影式边缘压暗 */
    var vigCache = { w: 0, h: 0, grad: null };
    function vignette() {
      if (vigCache.w !== view.w || vigCache.h !== view.h || !vigCache.grad) {
        var g = ctx.createRadialGradient(
          view.w / 2, view.h / 2, Math.min(view.w, view.h) * 0.36,
          view.w / 2, view.h / 2, Math.max(view.w, view.h) * 0.74
        );
        g.addColorStop(0, "rgba(0,0,0,0)");
        g.addColorStop(1, theme === "dark" ? "rgba(0,0,0,0.32)" : "rgba(0,0,0,0.10)");
        vigCache = { w: view.w, h: view.h, grad: g };
      }
      return vigCache.grad;
    }
    /* 深空底色缓存（垂直渐变，非纯色——上冷下深的空间纵深） */
    var bgCache = { w: 0, h: 0, t: "", grad: null };
    function backdrop() {
      var key = theme + (flags.bloom ? "b" : "");
      if (bgCache.w !== view.w || bgCache.h !== view.h || bgCache.t !== key || !bgCache.grad) {
        var g = ctx.createLinearGradient(0, 0, 0, view.h);
        if (theme === "dark") {
          if (flags.bloom) {
            g.addColorStop(0, "rgba(2,2,4,0.94)");
            g.addColorStop(1, "rgba(0,0,0,0.94)");
          } else {
            g.addColorStop(0, "rgba(12,16,25,0.26)");
            g.addColorStop(1, "rgba(7,9,14,0.40)");
          }
        } else {
          g.addColorStop(0, U.rgba(C.BG_LIGHT, C.CANVAS_ALPHA_LIGHT));
          g.addColorStop(1, U.rgba("#EDF0F5", C.CANVAS_ALPHA_LIGHT));
        }
        bgCache = { w: view.w, h: view.h, t: key, grad: g };
      }
      return bgCache.grad;
    }

    /* ── v4 材质封层：胶片噪点 ── */
    var grainTile = null;
    function grainPattern() {
      if (grainTile !== null) return grainTile;
      try {
        var N = 96;
        var cv = document.createElement("canvas");
        cv.width = N; cv.height = N;
        var g2 = cv.getContext("2d");
        var img = g2.createImageData(N, N);
        var r = U.rng(424242);
        for (var i = 0; i < img.data.length; i += 4) {
          var v = Math.floor(r() * 255);
          img.data[i] = v; img.data[i + 1] = v; img.data[i + 2] = v;
          img.data[i + 3] = 26;
        }
        g2.putImageData(img, 0, 0);
        grainTile = ctx.createPattern(cv, "repeat") || false;
      } catch (e) { grainTile = false; }
      return grainTile;
    }

    /* ── 数据装载 ─────────────────────────────────────────────────────── */
    this.setIR = function (next) {
      ir = next; coords = {}; layoutDone = false; selected = -1;
      buildStars(); buildRain(); buildNebulae();
      layout();
      fit(true);
    };
    this.setLevel = function (lv) {
      level = U.clamp(lv, 1, 7);
      layout();
      fit(); /* 切级别自动适应视野（带缓动飞行动画） */
      if (opts.onStatus) opts.onStatus(status());
    };
    this.setMode = function (m) { mode = m; };
    /** F303 对照界面：左半屏通俗 / 右半屏专业，树的形状位置不变。 */
    this.setCompare = function (on, split) { compare = on; compareSplit = split || 0.5; };
    this.setTheme = function (t) { theme = t; bgCache.w = 0; vigCache.w = 0; };
    this.setFlags = function (f) { for (var k in f) if (k in flags) flags[k] = f[k]; };
    this.getFlags = function () { return flags; };
    this.setSearch = function (set) { searchHits = set; };
    this.select = function (id) { selected = id; if (opts.onSelect) opts.onSelect(id); };
    this.getSelected = function () { return selected; };
    /** 定位节点：缓动飞行（render 循环内逼近 camT）。 */
    this.focus = function (id) {
      if (coords[id] == null) return;
      camT.x = coords[id][0]; camT.y = coords[id][1];
      camT.zoom = Math.max(cam.zoom, 1.15);
    };
    this.fit = fit;
    function fit(snap) {
      var w = bbox[2] - bbox[0], h = bbox[3] - bbox[1];
      var z = U.clamp(Math.min(view.w / Math.max(1, w), view.h / Math.max(1, h)), 0.1, 2);
      camT.zoom = z;
      camT.x = (bbox[0] + bbox[2]) / 2; camT.y = (bbox[1] + bbox[3]) / 2;
      if (snap) { cam.zoom = z; cam.x = camT.x; cam.y = camT.y; }   /* 首次直接就位 */
    }
    this.zoomBy = function (wheel) {
      var f = Math.pow(1 + Math.abs(wheel) * 0.001, wheel > 0 ? 1 : -1);
      cam.zoom = U.clamp(cam.zoom * f, 0.1, 5);
      camT.zoom = cam.zoom;
    };
    /** 光标锚定缩放：滚轮缩放时保持光标下的世界点不动（设计工具标配手感）。 */
    function zoomAt(sx, sy, wheel) {
      var f = Math.pow(1 + Math.abs(wheel) * 0.0012, wheel > 0 ? 1 : -1);
      var z2 = U.clamp(cam.zoom * f, 0.1, 5);
      f = z2 / cam.zoom;
      var wx = (sx - view.w / 2) / cam.zoom + cam.x;
      var wy = (sy - view.h / 2) / cam.zoom + cam.y;
      cam.zoom = z2; camT.zoom = z2;
      cam.x = wx - (sx - view.w / 2) / z2; cam.y = wy - (sy - view.h / 2) / z2;
      camT.x = cam.x; camT.y = cam.y;
    }
    this.pan = function (dx, dy) {
      cam.x -= dx / cam.zoom; cam.y -= dy / cam.zoom;
      camT.x = cam.x; camT.y = cam.y;
    };
    this.camera = function () { return cam; };
    this.status = status;
    function status() {
      return {
        level: level, zoom: cam.zoom, nodes: visible.length,
        degraded: degraded, lod: lodOf(cam.zoom),
        fps: Math.round(fps)
      };
    }
    /** F274 截图壁纸：2x 超采样 + sRGB 校正。 */
    this.screenshot = function (scale) {
      var s = scale || 2, off = document.createElement("canvas");
      off.width = Math.round(view.w * s); off.height = Math.round(view.h * s);
      var octx = off.getContext("2d");
      octx.fillStyle = theme === "dark" ? C.BG_DARK : C.BG_LIGHT;
      octx.fillRect(0, 0, off.width, off.height);
      octx.drawImage(canvas, 0, 0, off.width, off.height);
      return off;
    };

    /* ── 命中测试（与卡片盒一致，宽 46px 内的最近节点） ────────────────── */
    function hit(sx, sy) {
      var lod = lodOf(cam.zoom);
      var best = -1, bd = 1e9;
      for (var i = 0; i < visible.length; i++) {
        var n = visible[i], c = coords[n.id];
        if (!c) continue;
        var p = toScreen(c[0], c[1]);
        var wh = nodeBox(n, lod);
        var dx = Math.abs(p[0] - sx), dy = Math.abs(p[1] - sy);
        if (dx > wh[0] / 2 + 5 || dy > wh[1] / 2 + 5) continue;
        var d = dx * dx + dy * dy;
        if (d < bd) { bd = d; best = n.id; }
      }
      return best;
    }

    /* ── 标签：F301/302/312 三界面 × F253 LOD × F303 对照分屏 ─────────── */
    function labelFor(n, lod, p) {
      var m = mode;
      if (m === "compare") m = (p && p[0] < view.w * compareSplit) ? "plain" : "pro";
      if (m === "plain") {
        if (lod === "module") return n.plain ? n.plain.slice(0, 12) : n.name;
        return n.plain || n.name;
      }
      if (lod === "module") return n.domain || n.name;
      if (lod === "func" && n.sig) return n.sig;
      return n.name;
    }

    /* ── 主渲染 ───────────────────────────────────────────────────────── */
    var off = document.createElement("canvas"), offCtx = off.getContext("2d");
    function render(now) {
      if (!isActive()) return;
      var dt = Math.min(0.05, Math.max(0.0001, (now - last) / 1000)); last = now;
      if (!fpsInit) { fps = 1 / dt; fpsInit = true; } else fps += (1 / dt - fps) * 0.06;
      var t = (now - t0) / 1000;
      var dark = theme === "dark";

      /* 镜头缓动：camT → cam（指数逼近，fit/focus 的飞行动画） */
      var k = 1 - Math.exp(-dt * C.CAM_EASE);
      cam.x += (camT.x - cam.x) * k;
      cam.y += (camT.y - cam.y) * k;
      cam.zoom += (camT.zoom - cam.zoom) * k;

      ctx.clearRect(0, 0, view.w, view.h);

      if (!ir || !visible.length) {
        drawMinimap();
        if (opts.onStatus) opts.onStatus(status());
        return;
      }

      /* 深色普通态：底色半透明渐变（壁纸 1:1 透出 + 纵深）；Bloom/浅色近实底 */
      ctx.fillStyle = backdrop();
      ctx.fillRect(0, 0, view.w, view.h);

      /* 性能策略：>10000 节点自动降级 */
      degraded = visible.length > C.DEGRADE_AT;
      var useParticles = flags.particles && !degraded;
      var useGlow = flags.glow && !degraded;
      var useBreathe = flags.breathe && !degraded;

      /* F273 时间光影 */
      if (flags.tint) {
        var h = new Date().getHours() + new Date().getMinutes() / 60;
        var tint = dayTint(h);
        ctx.fillStyle = "rgba(" + Math.round(tint[0] * 255) + "," + Math.round(tint[1] * 255) + "," +
          Math.round(tint[2] * 255) + "," + (dark ? 0.035 : 0.05) + ")";
        ctx.fillRect(0, 0, view.w, view.h);
      }

      /* ── L0 极光星云：三张柔光云 screen 叠加，缓慢漂移 + 相机视差 ── */
      if (flags.nebula && nebulae && dark && !degraded) {
        ctx.save();
        ctx.globalCompositeOperation = "screen";
        for (var nb = 0; nb < nebulae.length; nb++) {
          var N2 = nebulae[nb];
          var nx = ((N2.bx + Math.sin(t * N2.wx + N2.ph) * N2.ax) * view.w - cam.x * cam.zoom * 0.05) % view.w;
          var ny = ((N2.by + Math.cos(t * N2.wy + N2.ph) * N2.ay) * view.h - cam.y * cam.zoom * 0.05) % view.h;
          if (nx < 0) nx += view.w;
          if (ny < 0) ny += view.h;
          var nsz = Math.max(view.w, view.h) * 1.35 * N2.sc;
          ctx.globalAlpha = N2.al;
          ctx.drawImage(N2.sp, nx - nsz / 2, ny - nsz / 2, nsz, nsz);
        }
        ctx.restore();
      }

      /* F267 星空：双层视差 + 相位闪烁 */
      if (stars && dark) {
        for (var i = 0; i < stars.length; i++) {
          var s = stars[i];
          var sx = ((s.x * view.w - cam.x * cam.zoom * s.z * 0.06) % view.w + view.w) % view.w;
          var sy = ((s.y * view.h - cam.y * cam.zoom * s.z * 0.06) % view.h + view.h) % view.h;
          var twk = 0.72 + 0.28 * Math.sin(t * s.tw + s.ph);
          ctx.fillStyle = "rgba(255,255,255," + (s.a * twk).toFixed(3) + ")";
          ctx.fillRect(sx, sy, s.sz, s.sz);
        }
      }

      /* F271 代码雨 */
      if (flags.rain && dark) {
        ctx.font = "10px ui-monospace, Menlo, Consolas, monospace";
        ctx.fillStyle = U.rgba(C.RAIN_COLOR, C.RAIN_ALPHA);
        for (var r = 0; r < rain.length; r++) {
          var d = rain[r];
          d.y = (d.y + C.RAIN_SPEED) % (view.h + 40);
          ctx.fillText(d.ch, d.x * view.w, d.y);
        }
      }

      /* ── L1 世界点阵网格：小格点 + 每 5 格一条主十字线（空间锚） ── */
      if (cam.zoom > 0.12 && cam.zoom < 2.6) {
        var sp = C.GRID_MINOR * cam.zoom;
        if (sp > 22) {
          var ox = ((-cam.x * cam.zoom) % sp + sp) % sp;
          var oy = ((-cam.y * cam.zoom) % sp + sp) % sp;
          ctx.fillStyle = dark ? "rgba(160,180,220,0.11)" : "rgba(30,45,80,0.13)";
          for (var gx = ox; gx < view.w; gx += sp) {
            for (var gy = oy; gy < view.h; gy += sp) {
              ctx.fillRect(gx - 0.75, gy - 0.75, 1.5, 1.5);
            }
          }
          if (sp > 64) {
            var spM = sp * C.GRID_MAJOR;
            var oxM = ((-cam.x * cam.zoom) % spM + spM) % spM;
            var oyM = ((-cam.y * cam.zoom) % spM + spM) % spM;
            ctx.strokeStyle = dark ? "rgba(160,180,220,0.09)" : "rgba(30,45,80,0.11)";
            ctx.lineWidth = 1;
            ctx.beginPath();
            for (var mx = oxM; mx < view.w; mx += spM) { ctx.moveTo(mx, 0); ctx.lineTo(mx, view.h); }
            for (var my = oyM; my < view.h; my += spM) { ctx.moveTo(0, my); ctx.lineTo(view.w, my); }
            ctx.stroke();
          }
        }
      }

      /* ── 光环境：发光精灵（艺术级光感核心）──
       * 每个节点向暗处发射域色光晕，screen 混合自然叠亮。 */
      if (flags.nebula && !degraded) {
        ctx.save();
        ctx.globalCompositeOperation = "screen";
        var tGlow = useBreathe ? breathe(t) : 1;
        visible.forEach(function (n) {
          var c = coords[n.id]; if (!c) return;
          var p = toScreen(c[0], c[1]);
          if (p[0] < -400 || p[1] < -400 || p[0] > view.w + 400 || p[1] > view.h + 400) return;
          var color = n.status === "dead" ? "#8E8E93" : CA.semanticColor(n.domain);
          var g2 = glowOf(n.loc, n.hot || 0.3);
          var size = nodeSize(n.loc) * (3.0 + g2.bright * 1.6) * cam.zoom * tGlow;
          if (size < 14) return;
          ctx.globalAlpha = (n.id === selected ? 0.42 : 0.22) * Math.max(0.5, g2.bright);
          ctx.drawImage(glowSprite(color), p[0] - size / 2, p[1] - size / 2, size, size);
        });
        ctx.restore();
      }

      /* F262 性能热力（高斯核） */
      if (flags.heat) {
        var src = visible.map(function (n) { var c = coords[n.id] || [0, 0]; return [c[0], c[1], n.hot || 0]; });
        var step = 32;
        ctx.save();
        for (var hx = 0; hx < view.w; hx += step) {
          for (var hy = 0; hy < view.h; hy += step) {
            var w = toWorld(hx + step / 2, hy + step / 2), v = 0;
            for (var si = 0; si < src.length; si++) {
              var d2 = Math.pow(w[0] - src[si][0], 2) + Math.pow(w[1] - src[si][1], 2);
              v += src[si][2] * Math.exp(-d2 / (2 * 40 * 40));
            }
            if (v < 1e-3) continue;
            ctx.fillStyle = U.rgba("#FF9500", U.clamp(v, 0, 1) * 0.55);
            ctx.fillRect(hx, hy, step, step);
          }
        }
        ctx.restore();
      }

      /* 视口裁剪（虚拟化） */
      var lod = lodOf(cam.zoom);
      var shown = [];
      visible.forEach(function (n) {
        var c = coords[n.id]; if (!c) return;
        var p = toScreen(c[0], c[1]);
        if (p[0] < -220 || p[1] < -220 || p[0] > view.w + 220 || p[1] > view.h + 220) return;
        shown.push({ n: n, p: p });
      });

      /* ── L2 边层：域色基线 → 相关边辉光渐变 + 流动虚线 + 软光粒子 ── */
      ctx.save();
      ctx.lineCap = "round";
      ir.edges.forEach(function (e) {
        var a = coords[e.from], b = coords[e.to];
        if (!a || !b) return;
        var pa = toScreen(a[0], a[1]), pb = toScreen(b[0], b[1]);
        if (Math.max(pa[0], pb[0]) < -100 || Math.min(pa[0], pb[0]) > view.w + 100) return;
        if (Math.max(pa[1], pb[1]) < -100 || Math.min(pa[1], pb[1]) > view.h + 100) return;
        var mx = (pa[0] + pb[0]) / 2, my = (pa[1] + pb[1]) / 2;
        var nx = -(pb[1] - pa[1]) * 0.20, ny = (pb[0] - pa[0]) * 0.20;
        var p1 = [mx + nx, my + ny], p2 = [mx - nx, my - ny];
        var st = beamStyle(e.freq);
        var src = ir.nodes[e.from], dst = ir.nodes[e.to];
        var baseColor = e.kind === "dep" ? "#8E8E93" : CA.semanticColor(src.domain);
        var relSel = selected >= 0 && (e.from === selected || e.to === selected);
        var relHov = hovered >= 0 && (e.from === hovered || e.to === hovered);

        /* 选中边：辉光底层 + 域色→强调色渐变主线 + 流动虚线 */
        if (relSel) {
          ctx.strokeStyle = U.rgba(useGlow ? C.ACCENT : baseColor, 0.13);
          ctx.lineWidth = Math.max(4, st.w * cam.zoom * 2.6);
          strokeBez(pa, p1, p2, pb);
        }
        ctx.setLineDash(e.kind === "dep" ? [1, 3] : []);
        if (relSel) {
          var eg = ctx.createLinearGradient(pa[0], pa[1], pb[0], pb[1]);
          eg.addColorStop(0, U.rgba(baseColor, 0.95));
          eg.addColorStop(1, U.rgba(C.ACCENT, 0.95));
          ctx.strokeStyle = eg;
          ctx.lineWidth = Math.max(1.2, st.w * cam.zoom * 0.85);
          strokeBez(pa, p1, p2, pb);
          /* 流动虚线：数据在管道里跑的动感 */
          ctx.save();
          ctx.setLineDash([2, 9]);
          ctx.lineDashOffset = -t * 34;
          ctx.strokeStyle = U.rgba("#FFFFFF", 0.5);
          ctx.lineWidth = Math.max(0.8, st.w * cam.zoom * 0.4);
          strokeBez(pa, p1, p2, pb);
          ctx.restore();
        } else if (relHov) {
          ctx.strokeStyle = U.rgba(baseColor, 0.55);
          ctx.lineWidth = Math.max(1, st.w * cam.zoom * 0.6);
          strokeBez(pa, p1, p2, pb);
        } else {
          ctx.strokeStyle = U.rgba(baseColor, e.kind === "dep" ? 0.15 : 0.28);
          ctx.lineWidth = Math.max(0.5, (0.8 + e.freq * 0.8) * cam.zoom);
          strokeBez(pa, p1, p2, pb);
        }
        ctx.setLineDash([]);

        /* 调用方向箭头：只在相关边绘制，保持全景干净 */
        if (relSel || relHov) {
          var ta = 0.93;
          var pos = bez(pa, p1, p2, pb, ta);
          var dv = bezd(pa, p1, p2, pb, ta);
          var dl = Math.max(1, Math.sqrt(dv[0] * dv[0] + dv[1] * dv[1]));
          var ux = dv[0] / dl, uy = dv[1] / dl;
          var ah = 4.6;
          ctx.fillStyle = relSel ? U.rgba(C.ACCENT, 0.95) : U.rgba(baseColor, 0.7);
          ctx.beginPath();
          ctx.moveTo(pos[0] + ux * ah, pos[1] + uy * ah);
          ctx.lineTo(pos[0] - uy * ah * 0.62 - ux * ah * 0.25, pos[1] + ux * ah * 0.62 - uy * ah * 0.25);
          ctx.lineTo(pos[0] + uy * ah * 0.62 - ux * ah * 0.25, pos[1] - ux * ah * 0.62 - uy * ah * 0.25);
          ctx.closePath();
          ctx.fill();
        }

        /* F259 数据流粒子：软光点（精灵渲染），相关边或高倍缩放 */
        var wantParticles = useParticles && e.kind === "call" && (relSel || relHov || cam.zoom >= 0.5);
        if (wantParticles) {
          var speed = U.clamp(e.freq * 100, 1, 100);
          var cnt = relSel || relHov ? 2 : 1;
          var pc = relSel ? C.ACCENT : baseColor;
          var spr = glowSprite(pc);
          for (var q = 0; q < cnt; q++) {
            var tt = ((t * speed / 100 + q / cnt) % 1);
            var pos2 = bez(pa, p1, p2, pb, tt);
            var pr = (relSel ? 7 : 5) + e.freq * 3;
            ctx.globalAlpha = relSel ? 0.85 : 0.45;
            ctx.drawImage(spr, pos2[0] - pr / 2, pos2[1] - pr / 2, pr, pr);
            ctx.globalAlpha = 1;
          }
        }
      });
      ctx.restore();
      function strokeBez(a, b, c, d) {
        ctx.beginPath(); ctx.moveTo(a[0], a[1]);
        ctx.bezierCurveTo(b[0], b[1], c[0], c[1], d[0], d[1]); ctx.stroke();
      }
      function bez(p0, p1, p2, p3, tt) {
        var u = 1 - tt;
        return [
          u * u * u * p0[0] + 3 * u * u * tt * p1[0] + 3 * u * tt * tt * p2[0] + tt * tt * tt * p3[0],
          u * u * u * p0[1] + 3 * u * u * tt * p1[1] + 3 * u * tt * tt * p2[1] + tt * tt * tt * p3[1]
        ];
      }
      function bezd(p0, p1, p2, p3, tt) {
        var u = 1 - tt;
        return [
          3 * u * u * (p1[0] - p0[0]) + 6 * u * tt * (p2[0] - p1[0]) + 3 * tt * tt * (p3[0] - p2[0]),
          3 * u * u * (p1[1] - p0[1]) + 6 * u * tt * (p2[1] - p1[1]) + 3 * tt * tt * (p3[1] - p2[1])
        ];
      }

      /* F263/F264 暗色霓虹 + Bloom 光晕（离屏高亮层 → lighter 叠加） */
      if (flags.bloom && dark) {
        off.width = canvas.width; off.height = canvas.height;
        offCtx.setTransform(view.dpr, 0, 0, view.dpr, 0, 0);
        offCtx.clearRect(0, 0, view.w, view.h);
        drawNodes(offCtx, shown, t, lod, true, useBreathe);
        ctx.save();
        if ("filter" in ctx) ctx.filter = "blur(" + C.BLOOM_BLUR + "px)";
        ctx.globalCompositeOperation = "lighter";
        ctx.globalAlpha = U.clamp(C.BLOOM_STRENGTH * 0.5, 0, 1);
        ctx.drawImage(off, 0, 0, view.w, view.h);
        ctx.restore();
        if ("filter" in ctx) ctx.filter = "none";
      }

      /* L3：节点卡片 */
      drawNodes(ctx, shown, t, lod, false, useBreathe);

      /* F272 声波频谱 */
      if (flags.spectrum) {
        var bars = C.SPECTRUM_BARS, bw = view.w / bars;
        ctx.save();
        for (var bi = 0; bi < bars; bi++) {
          var n2 = visible[bi % Math.max(1, visible.length)];
          var hgt = U.clamp((n2 ? n2.cc || 0.2 : 0.2) / 12, 0, 1) * 26 * (0.6 + 0.4 * Math.abs(Math.sin(t * 1.2 + bi * 0.4)));
          ctx.fillStyle = U.rgba(CA.semanticColor(n2 ? n2.domain : ""), 0.35);
          ctx.fillRect(bi * bw, view.h - hgt, bw - 1, hgt);
        }
        ctx.restore();
      }

      /* 搜索命中：外圈白环（盒模型外扩环） */
      if (searchHits && searchHits.size) {
        var lodS = lodOf(cam.zoom);
        ctx.save();
        ctx.strokeStyle = U.rgba("#FFFFFF", 0.8); ctx.lineWidth = 1;
        shown.forEach(function (s) {
          if (!searchHits.has(s.n.id)) return;
          var wh = nodeBox(s.n, lodS);
          ctx.strokeRect(s.p[0] - wh[0] / 2 - 5, s.p[1] - wh[1] / 2 - 5, wh[0] + 10, wh[1] + 10);
        });
        ctx.restore();
      }

      /* F275 爆炸展开动画层 */
      if (explodeAnim) {
        var pts = explode(explodeAnim.center, explodeAnim.angles, explodeAnim.dist, now - explodeAnim.t0);
        ctx.save();
        pts.forEach(function (p, i) {
          var sp = toScreen(p[0], p[1]);
          ctx.fillStyle = U.rgba(CA.semanticColor(explodeAnim.domain), 0.5);
          ctx.beginPath(); ctx.arc(sp[0], sp[1], 6 * cam.zoom + 2, 0, Math.PI * 2); ctx.fill();
        });
        ctx.restore();
        if (now - explodeAnim.t0 > C.EXPLODE_MS) explodeAnim = null;
      }

      /* ── L4 摄影后期：暗角 → 胶片噪点封层 ── */
      ctx.fillStyle = vignette();
      ctx.fillRect(0, 0, view.w, view.h);
      var pat = grainPattern();
      if (pat) {
        ctx.save();
        ctx.globalAlpha = 0.4;
        ctx.fillStyle = pat;
        ctx.fillRect(0, 0, view.w, view.h);
        ctx.restore();
      }

      drawMinimap();
      if (opts.onStatus) opts.onStatus(status());
    }
    this.render = render;

    /* ── 节点几何 ── */
    function nodeBox(n, lod) {
      var size = nodeSize(n.loc) * (lod === "module" ? 0.7 : 1) * cam.zoom;
      return [Math.max(20, size), Math.max(20, size * 0.56)];
    }
    function ellipsize(c, txt, maxW) {
      if (c.measureText(txt).width <= maxW) return txt;
      while (txt.length > 1 && c.measureText(txt + "…").width > maxW) txt = txt.slice(0, -1);
      return txt + "…";
    }

    /* ── L3 节点卡片（v4 材质）
     * 物理双层阴影 + 垂直渐变受光面 + 受光斜面 + 玻璃釉面反光 +
     * 域色发光脊柱 + 图标 + 指标微条（cov/hot）+ 状态辉光角点 +
     * 选中四角刻线 + 悬停脉冲环。低倍缩放退化为域色圆角块。 ── */
    function drawNodes(c, shown, t, lod, brightOnly, useBreathe) {
      var br = useBreathe ? breathe(t) : 1;
      var dark = theme === "dark";
      var titleCol = dark ? "#E9EBF1" : "#1B1E24";
      var subCol = dark ? "#8B93A3" : "#697180";
      var ACC = C.ACCENT;

      shown.forEach(function (s) {
        var n = s.n, p = s.p;
        var g = glowOf(n.loc, n.hot || 0.3);
        var color = n.status === "dead" ? "#8E8E93" : CA.semanticColor(n.domain);
        var wh = nodeBox(n, lod);
        var w = wh[0], h = wh[1];
        var isSel = n.id === selected, isHov = n.id === hovered;

        /* Bloom 高亮层：只画亮度超阈值的发光核心 */
        if (brightOnly) {
          if (g.bright < C.NEON_THRESHOLD && !isSel) return;
          c.save();
          c.shadowColor = color; c.shadowBlur = 18 * cam.zoom;
          c.fillStyle = U.rgba(color, 0.5 * g.bright * br);
          c.beginPath(); c.arc(p[0], p[1], w / 2, 0, Math.PI * 2); c.fill();
          c.restore();
          return;
        }

        /* 模块 LOD：域色圆角块（远看是色块地图，不堆文字） */
        if (lod === "module") {
          c.save();
          var mg = c.createLinearGradient(0, p[1] - h / 2, 0, p[1] + h / 2);
          mg.addColorStop(0, U.rgba(color, U.clamp(0.34 * g.bright * br + 0.10, 0.14, 0.6)));
          mg.addColorStop(1, U.rgba(color, U.clamp(0.18 * g.bright * br + 0.05, 0.08, 0.4)));
          c.fillStyle = mg;
          U.roundRect(c, p[0] - w / 2, p[1] - h / 2, w, h, Math.min(6, h / 2));
          c.fill();
          c.strokeStyle = U.rgba(color, U.clamp(0.55 * br, 0.2, 0.9));
          c.lineWidth = isSel ? 1.6 : 0.7;
          c.stroke();
          c.restore();
        } else {
          var rr = Math.min(9, h / 2);
          c.save();
          /* 悬停抬升：卡片向上浮 1.2px（物理感） */
          var liftY = isHov && !isSel ? -1.2 : 0;
          var py = p[1] + liftY;

          /* 双层物理阴影 */
          c.shadowColor = dark ? "rgba(0,0,0,0.55)" : "rgba(18,24,36,0.18)";
          c.shadowBlur = 10; c.shadowOffsetY = 5;
          c.fillStyle = "rgba(0,0,0,0.01)";
          U.roundRect(c, p[0] - w / 2, py - h / 2, w, h, rr);
          c.fill();
          c.shadowColor = dark ? "rgba(0,0,0,0.45)" : "rgba(18,24,36,0.12)";
          c.shadowBlur = 3; c.shadowOffsetY = 2;
          c.fill();
          c.shadowColor = "transparent"; c.shadowBlur = 0; c.shadowOffsetY = 0;

          /* 卡片本体：垂直渐变受光面（选中带强调色染色） */
          var cardGrad = c.createLinearGradient(0, py - h / 2, 0, py + h / 2);
          if (dark) {
            if (isSel) {
              cardGrad.addColorStop(0, "rgba(38,48,66,0.99)");
              cardGrad.addColorStop(1, "rgba(21,27,38,0.985)");
            } else {
              cardGrad.addColorStop(0, "rgba(35,41,52,0.985)");
              cardGrad.addColorStop(1, "rgba(19,24,31,0.975)");
            }
          } else {
            cardGrad.addColorStop(0, "rgba(255,255,255,0.99)");
            cardGrad.addColorStop(1, "rgba(238,241,246,0.98)");
          }
          c.fillStyle = cardGrad;
          U.roundRect(c, p[0] - w / 2, py - h / 2, w, h, rr);
          c.fill();

          /* 玻璃釉面反光：左上渐隐光带 */
          c.save();
          U.roundRect(c, p[0] - w / 2, py - h / 2, w, h, rr);
          c.clip();
          var sheen = c.createLinearGradient(p[0] - w / 2, py - h / 2, p[0] + w * 0.15, py + h / 2);
          sheen.addColorStop(0, dark ? "rgba(255,255,255,0.07)" : "rgba(255,255,255,0.6)");
          sheen.addColorStop(0.5, "rgba(255,255,255,0)");
          c.fillStyle = sheen;
          c.fillRect(p[0] - w / 2, py - h / 2, w, h);
          c.restore();

          /* 描边：域色（悬停加亮），选中用强调色 + 柔光 */
          if (flags.glow && (isSel || isHov)) {
            c.shadowColor = isSel ? ACC : color;
            c.shadowBlur = (isSel ? 13 : 7) * g.bright;
          }
          c.strokeStyle = isSel ? U.rgba(ACC, 0.95) : U.rgba(color, isHov ? 0.85 : 0.42);
          c.lineWidth = isSel ? 1.5 : 0.8;
          U.roundRect(c, p[0] - w / 2, py - h / 2, w, h, rr);
          c.stroke();
          c.shadowBlur = 0;
          /* 顶部受光线（厚度签名） */
          c.strokeStyle = dark ? "rgba(255,255,255,0.09)" : "rgba(255,255,255,0.92)";
          c.lineWidth = 1;
          c.beginPath();
          c.moveTo(p[0] - w / 2 + rr, py - h / 2 + 0.5);
          c.lineTo(p[0] + w / 2 - rr, py - h / 2 + 0.5);
          c.stroke();

          /* 域色发光脊柱：左缘 3px 渐变（上亮下隐） */
          var spine = c.createLinearGradient(0, py - h / 2, 0, py + h / 2);
          spine.addColorStop(0, U.rgba(color, 0.95));
          spine.addColorStop(1, U.rgba(color, 0.25));
          c.fillStyle = spine;
          U.roundRect(c, p[0] - w / 2 + 2, py - h / 2 + 3, 3, h - 6, 1.5);
          c.fill();

          /* 悬停脉冲环：呼吸式外扩 */
          if (isHov && !isSel) {
            var ph2 = (t % 1.6) / 1.6;
            c.strokeStyle = U.rgba(ACC, (1 - ph2) * 0.35);
            c.lineWidth = 1.5;
            U.roundRect(c, p[0] - w / 2 - 3 - ph2 * 5, py - h / 2 - 3 - ph2 * 5,
              w + 6 + ph2 * 10, h + 6 + ph2 * 10, Math.min(12, h / 2 + 4));
            c.stroke();
          }
          /* 选中：外圈柔光环 + 四角刻线（取景框语言） */
          if (isSel) {
            c.strokeStyle = U.rgba(ACC, 0.30);
            c.lineWidth = 3;
            U.roundRect(c, p[0] - w / 2 - 3.5, py - h / 2 - 3.5, w + 7, h + 7, Math.min(11, h / 2 + 3));
            c.stroke();
            c.strokeStyle = U.rgba(ACC, 0.9);
            c.lineWidth = 1.4;
            var tk = Math.min(7, w / 4, h / 4), m2 = 6.5;
            [[-1, -1], [1, -1], [-1, 1], [1, 1]].forEach(function (q) {
              var qx = p[0] + q[0] * (w / 2 + m2), qy = py + q[1] * (h / 2 + m2);
              c.beginPath();
              c.moveTo(qx - q[0] * tk, qy); c.lineTo(qx, qy); c.lineTo(qx, qy - q[1] * tk);
              c.stroke();
            });
          }
          /* 状态角点（颜色即含义：红 bug / 黄 warn / 灰 dead / 绿 正常） */
          if (n.status && n.status !== "ok") {
            c.save();
            c.shadowColor = CA.statusColor(n.status);
            c.shadowBlur = 6;
            c.fillStyle = U.rgba(CA.statusColor(n.status), 0.95);
            c.beginPath(); c.arc(p[0] + w / 2 - 7, py - h / 2 + 7, 2.5, 0, Math.PI * 2); c.fill();
            c.restore();
          }

          /* 卡内文字 + 图标 + 指标微条 */
          var fs = U.clamp(11 * Math.min(1.25, cam.zoom), 9, 13);
          var padL = 12, padR = n.status ? 13 : 9;
          var innerW = w - padL - padR;
          var hasSub = cam.zoom >= 0.55 && !!n.plain && n.plain !== n.name;
          var hasBars = n.kind === 5 && cam.zoom >= 0.5 && h > 30;
          if (innerW > 24) {
            c.textAlign = "left"; c.textBaseline = "middle";
            var ty = py - h * (hasBars ? 0.20 : (hasSub ? 0.14 : 0));
            var icon = n.icon || CA.metaphorIcon(n.name);
            var tx0 = p[0] - w / 2 + padL;
            c.font = "500 " + fs.toFixed(1) + "px " + (dark ? "'Segoe UI Variable Text','Segoe UI',sans-serif" : "'Segoe UI Variable Text','Segoe UI',sans-serif");
            c.fillStyle = n.status === "dead" ? subCol : (isSel ? (dark ? "#FFFFFF" : "#000") : titleCol);
            if (icon && icon.length <= 2 && cam.zoom >= 0.42 && innerW > 60) {
              c.fillText(icon, tx0, ty);
              tx0 += fs * 1.35;
            }
            c.fillText(ellipsize(c, n.name, w / 2 + w / 2 - padL - padR - (tx0 - (p[0] - w / 2 + padL))), tx0, ty);
            if (hasSub) {
              c.font = "400 " + (fs - 1.5).toFixed(1) + "px 'Segoe UI',system-ui,sans-serif";
              c.fillStyle = subCol;
              c.fillText(ellipsize(c, n.plain, innerW), p[0] - w / 2 + padL, py + h * 0.20);
            }
            /* 指标微条：覆盖率（蓝）+ 热度（橙），函数节点专属 */
            if (hasBars) {
              var barY = py + h / 2 - 6, barW = innerW;
              c.fillStyle = dark ? "rgba(255,255,255,0.08)" : "rgba(0,0,0,0.08)";
              U.roundRect(c, tx0, barY, barW, 2, 1); c.fill();
              c.fillStyle = U.rgba("#5B9DFF", 0.9);
              U.roundRect(c, tx0, barY, Math.max(1.5, barW * (n.cov || 0)), 2, 1); c.fill();
              c.fillStyle = dark ? "rgba(255,255,255,0.08)" : "rgba(0,0,0,0.08)";
              U.roundRect(c, tx0, barY + 3.5, barW, 2, 1); c.fill();
              c.fillStyle = U.rgba("#FF9500", 0.85);
              U.roundRect(c, tx0, barY + 3.5, Math.max(1.5, barW * (n.hot || 0)), 2, 1); c.fill();
            }
          } else if (cam.zoom > 0.18 || isSel || isHov) {
            /* 卡片太小 → 外部短标签 */
            c.font = "400 " + U.clamp(10 * Math.min(1.4, cam.zoom), 8, 14).toFixed(1) + "px 'Segoe UI',system-ui,sans-serif";
            c.textAlign = "center"; c.textBaseline = "top";
            c.fillStyle = isSel ? (dark ? "#FFFFFF" : "#000") : (dark ? "#B8BEC7" : "#5A6068");
            var shortName = n.name.length > 14 ? n.name.slice(0, 13) + "…" : n.name;
            c.fillText(shortName, p[0], py + h / 2 + 4);
          }
          c.restore();
        }

        /* F260 循环光环：半径=节点×1.5，转速=迭代次数 */
        if (flags.orbit && n.loop) {
          var orad = w / 2 * 1.5, ang = t * (n.cc || 1) * 0.5;
          c.save();
          c.strokeStyle = U.rgba("#AF52DE", 0.5);
          c.lineWidth = 1;
          c.beginPath(); c.arc(p[0], p[1], orad, ang, ang + Math.PI * 1.2); c.stroke();
          c.restore();
        }

        /* F261 异常脉冲：品红波纹，周期 2s */
        if (n.status === "bug" || n.status === "warn") {
          var ph = (t % 2) / 2, pr = (w / 2) * 2.2 * ph;
          c.save();
          c.strokeStyle = U.rgba(C.PULSE_COLOR, (1 - ph) * 0.55);
          c.lineWidth = 1;
          c.beginPath(); c.arc(p[0], p[1], pr, 0, Math.PI * 2); c.stroke();
          c.restore();
        }

        /* 模块 LOD 外部标签 */
        if (lod === "module" && (cam.zoom > 0.18 || isSel || isHov)) {
          var txt = labelFor(n, lod, p);
          var fs2 = U.clamp(10 * Math.min(1.4, cam.zoom), 8, 14);
          c.save();
          c.font = "400 " + fs2.toFixed(1) + "px 'Segoe UI',system-ui,sans-serif";
          c.textAlign = "center"; c.textBaseline = "top";
          c.fillStyle = isSel ? (dark ? "#FFFFFF" : "#000") : (dark ? "#B8BEC7" : "#5A6068");
          c.fillText(txt, p[0], p[1] + h / 2 + 5);
          c.restore();
        }
      });
    }

    /* ── F255 全景小地图（光点化 + 圆角视口框） ────────────────────────── */
    function drawMinimap() {
      if (!mm) return;
      mm.clearRect(0, 0, C.MINIMAP_W, C.MINIMAP_H);
      mm.fillStyle = theme === "dark" ? "rgba(6,8,12,0.55)" : "rgba(255,255,255,0.6)";
      mm.fillRect(0, 0, C.MINIMAP_W, C.MINIMAP_H);
      var ww = Math.max(1, bbox[2] - bbox[0]), wh = Math.max(1, bbox[3] - bbox[1]);
      var sx = C.MINIMAP_W / ww, sy = C.MINIMAP_H / wh;
      mm.save();
      mm.globalCompositeOperation = "screen";
      visible.forEach(function (n) {
        var c = coords[n.id]; if (!c) return;
        var color = n.status === "dead" ? "#8E8E93" : CA.semanticColor(n.domain);
        var px = (c[0] - bbox[0]) * sx, py = (c[1] - bbox[1]) * sy;
        mm.drawImage(glowSprite(color), px - 3.5, py - 3.5, 7, 7);
        mm.fillStyle = U.rgba(color, 0.9);
        mm.fillRect(px - 1, py - 1, 2, 2);
      });
      mm.restore();
      /* 视口框：圆角 + 强调色辉光 */
      var vw = view.w / cam.zoom, vh = view.h / cam.zoom;
      var vx = (cam.x - vw / 2 - bbox[0]) * sx, vy = (cam.y - vh / 2 - bbox[1]) * sy;
      mm.strokeStyle = C.ACCENT; mm.lineWidth = 1;
      U.roundRect(mm, vx, vy, vw * sx, vh * sy, 3);
      mm.stroke();
    }

    /* ── 交互：平移 / 光标锚定缩放 / 选中 / 双击爆炸展开 ───────────────── */
    var dragging = false, moved = false, lx = 0, ly = 0;
    canvas.addEventListener("mousedown", function (e) {
      if (!isActive()) return;
      dragging = true; moved = false; lx = e.clientX; ly = e.clientY;
      canvas.classList.add("panning");
    });
    root.addEventListener("mousemove", function (e) {
      if (!isActive()) return;
      var r = canvas.getBoundingClientRect();
      var sx = e.clientX - r.left, sy = e.clientY - r.top;
      if (dragging) {
        var dx = e.clientX - lx, dy = e.clientY - ly;
        if (Math.abs(dx) + Math.abs(dy) > 2) moved = true;
        self.pan(dx, dy); lx = e.clientX; ly = e.clientY;
      }
      if (sx < 0 || sy < 0 || sx > view.w || sy > view.h) { hovered = -1; return; }
      hoverPos = { x: sx, y: sy };
      var h = hit(sx, sy);
      if (h !== hovered) {
        hovered = h;
        if (opts.onHover) opts.onHover(h, hoverPos);
      } else if (h >= 0 && opts.onHover) opts.onHover(h, hoverPos);
    });
    root.addEventListener("mouseup", function () {
      dragging = false;
      canvas.classList.remove("panning");
    });
    canvas.addEventListener("wheel", function (e) {
      if (!isActive()) return;
      e.preventDefault();
      var r = canvas.getBoundingClientRect();
      zoomAt(e.clientX - r.left, e.clientY - r.top, -e.deltaY);
      if (opts.onStatus) opts.onStatus(status());
    }, { passive: false });
    canvas.addEventListener("click", function (e) {
      if (!isActive()) return;
      if (moved) return;
      var r = canvas.getBoundingClientRect();
      var id = hit(e.clientX - r.left, e.clientY - r.top);
      self.select(id);
    });
    canvas.addEventListener("dblclick", function (e) {
      if (!isActive()) return;
      var r = canvas.getBoundingClientRect();
      var id = hit(e.clientX - r.left, e.clientY - r.top);
      if (id < 0) return;
      var n = ir.nodes[id];
      var kids = n.children.filter(function (k) { return ir.nodes[k].kind === n.kind + 1; });
      if (!kids.length) { self.select(id); return; }
      var c = coords[id] || [0, 0];
      var angles = kids.map(function (k, i) { return (i / kids.length) * Math.PI * 2 + 0.3; });
      explodeAnim = { center: c, angles: angles, dist: 160, t0: performance.now(), domain: n.domain };
      /* F275：展开后进入下一级 */
      self.setLevel(level + 1);
      self.select(kids[0]);
    });

    /* 小地图点击跳转 */
    if (opts.minimap) {
      opts.minimap.addEventListener("click", function (e) {
        if (!isActive()) return;
        var r = opts.minimap.getBoundingClientRect();
        var mx = e.clientX - r.left, my = e.clientY - r.top;
        cam.x = camT.x = bbox[0] + (mx / C.MINIMAP_W) * (bbox[2] - bbox[0]);
        cam.y = camT.y = bbox[1] + (my / C.MINIMAP_H) * (bbox[3] - bbox[1]);
      });
    }

    this.start = function () {
      resize();
      (function loop(now) { render(now); requestAnimationFrame(loop); })(performance.now());
    };
    resize();
  }

  CA.CanvasView = CanvasView;
  CA.lodOf = lodOf;
  CA.nodeSize = nodeSize;
  CA.riverColor = riverColor;
  CA.breathe = breathe;
  CA.dayTint = dayTint;
  CA.explode = explode;
  CA.beamStyle = beamStyle;
})(typeof globalThis !== "undefined" ? globalThis : this);
