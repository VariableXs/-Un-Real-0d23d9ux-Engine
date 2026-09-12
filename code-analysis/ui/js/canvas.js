/* =============================================================================
 * AI-04 · 无限全景画布 + 现代艺术风格（F251~F275）
 *   三层渲染：底层光效(离屏 Bloom) / 中层 Canvas2D(连线·节点·文字) / 顶层 DOM(UI)。
 *   所有参数与 core::canvas.rs 一一对应（引力/LOD/密度/发光/光束/粒子…），
 *   三端各自把同一份数据喂给自己的渲染器 → 等价。
 * ========================================================================== */
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
    EXPLODE_MS: 500, DEGRADE_AT: 10000
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
   * opts: { minimap, onSelect, onHover, onStatus }
   */
  function CanvasView(canvas, opts) {
    opts = opts || {};
    var self = this;
    var ctx = canvas.getContext("2d");
    var mm = opts.minimap ? opts.minimap.getContext("2d") : null;

    var ir = null, level = 2, mode = "plain", theme = "dark";
    var cam = { x: 0, y: 0, zoom: 1 };
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
      particles: true, glow: true, bloom: false, heat: false, rain: true,
      nebula: true, spectrum: true, breathe: true, orbit: true, trail: true, tint: true, glass: true
    };
    var degraded = false;
    var hoverPos = { x: 0, y: 0 };
    var searchHits = null;

    /* ── 尺寸 / DPR ───────────────────────────────────────────────────── */
    function resize() {
      var r = canvas.getBoundingClientRect();
      view.w = Math.max(1, r.width); view.h = Math.max(1, r.height);
      view.dpr = Math.min(2, root.devicePixelRatio || 1);
      canvas.width = Math.round(view.w * view.dpr);
      canvas.height = Math.round(view.h * view.dpr);
      ctx.setTransform(view.dpr, 0, 0, view.dpr, 0, 0);
    }
    this.resize = resize;

    function toScreen(wx, wy) {
      return [(wx - cam.x) * cam.zoom + view.w / 2, (wy - cam.y) * cam.zoom + view.h / 2];
    }
    function toWorld(sx, sy) {
      return [(sx - view.w / 2) / cam.zoom + cam.x, (sy - view.h / 2) / cam.zoom + cam.y];
    }
    this.toScreen = toScreen; this.toWorld = toWorld;

    /* ── F252 语义引力布局：引力=依赖强度，斥力=调用频率，收敛后锁定 ────── */
    function layout() {
      visible = CA.nodesAtLevel(ir, level);
      var ids = visible.map(function (n) { return n.id; });
      var bodies = ids.map(function (id, i) {
        var n = ir.nodes[id];
        var a = (i / Math.max(1, ids.length)) * Math.PI * 2;
        var r = 180 + 260 * ((i % 5) / 5);
        return {
          x: coords[id] ? coords[id][0] : Math.cos(a) * r,
          y: coords[id] ? coords[id][1] : Math.sin(a) * r,
          gravity: 0.2 + 0.8 * (n.hot || 0.1),
          repulsion: 0.3 + 0.7 * (n.hot || 0.1)
        };
      });
      if (!layoutDone) {
        for (var it = 0; it < 220; it++) if (forceStep(bodies) < 0.05) break;
        layoutDone = true;
      }
      bodies.forEach(function (b, i) {
        var id = ids[i];
        coords[id] = [b.x, b.y];
      });
      computeBBox();
      buildParticles();
    }
    function forceStep(bs) {
      var n = bs.length, fx = new Float64Array(n), fy = new Float64Array(n), i, j;
      for (i = 0; i < n; i++) for (j = i + 1; j < n; j++) {
        var dx = bs[j].x - bs[i].x, dy = bs[j].y - bs[i].y;
        var d = Math.max(1, Math.sqrt(dx * dx + dy * dy));
        var ux = dx / d, uy = dy / d;
        var f = bs[i].repulsion * bs[j].repulsion * C.REPULSION_K / (d * d);
        fx[i] -= ux * f; fy[i] -= uy * f; fx[j] += ux * f; fy[j] += uy * f;
        var g = bs[i].gravity * bs[j].gravity * (d - C.REST) * C.GRAVITY_K;
        fx[i] += ux * g; fy[i] += uy * g; fx[j] -= ux * g; fy[j] -= uy * g;
      }
      var mv = 0;
      for (i = 0; i < n; i++) {
        bs[i].x += fx[i] * C.STEP; bs[i].y += fy[i] * C.STEP;
        mv = Math.max(mv, Math.abs(fx[i]) + Math.abs(fy[i]));
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
      var pad = 200;
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

    /* ── F267 粒子星空 ─────────────────────────────────────────────────── */
    function buildStars() {
      var r = U.rng(20240913), n = 200 + Math.floor(r() * 300), s = [];
      for (var i = 0; i < n; i++) s.push({ x: r(), y: r(), sz: 1 + r(), a: 0.1 + r() * 0.2 });
      stars = s;
    }
    function buildRain() {
      rain = [];
      var r = U.rng(777);
      for (var i = 0; i < 40; i++) rain.push({ x: r(), y: r() * 600, ch: String.fromCharCode(0x30a0 + Math.floor(r() * 90)) });
    }

    /* ── 数据装载 ─────────────────────────────────────────────────────── */
    this.setIR = function (next) {
      ir = next; coords = {}; layoutDone = false; selected = -1;
      buildStars(); buildRain();
      layout();
      fit();
    };
    this.setLevel = function (lv) {
      level = U.clamp(lv, 1, 7);
      layout();
      if (opts.onStatus) opts.onStatus(status());
    };
    this.setMode = function (m) { mode = m; };
    /** F303 对照界面：左半屏通俗 / 右半屏专业，树的形状位置不变。 */
    this.setCompare = function (on, split) { compare = on; compareSplit = split || 0.5; };
    this.setTheme = function (t) { theme = t; };
    this.setFlags = function (f) { for (var k in f) if (k in flags) flags[k] = f[k]; };
    this.getFlags = function () { return flags; };
    this.setSearch = function (set) { searchHits = set; };
    this.select = function (id) { selected = id; if (opts.onSelect) opts.onSelect(id); };
    this.getSelected = function () { return selected; };
    this.focus = function (id) {
      if (coords[id] == null) return;
      cam.x = coords[id][0]; cam.y = coords[id][1];
      cam.zoom = Math.max(cam.zoom, 1.1);
    };
    this.fit = fit;
    function fit() {
      var w = bbox[2] - bbox[0], h = bbox[3] - bbox[1];
      cam.zoom = U.clamp(Math.min(view.w / Math.max(1, w), view.h / Math.max(1, h)), 0.1, 2);
      cam.x = (bbox[0] + bbox[2]) / 2; cam.y = (bbox[1] + bbox[3]) / 2;
    }
    this.zoomBy = function (wheel) {
      var f = Math.pow(1 + Math.abs(wheel) * 0.001, wheel > 0 ? 1 : -1);
      cam.zoom = U.clamp(cam.zoom * f, 0.1, 5);
    };
    this.pan = function (dx, dy) { cam.x -= dx / cam.zoom; cam.y -= dy / cam.zoom; };
    this.camera = function () { return cam; };
    this.status = status;
    function status() {
      return {
        level: level, zoom: cam.zoom, nodes: visible.length,
        degraded: degraded, lod: lodOf(cam.zoom)
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

    /* ── 命中测试 ─────────────────────────────────────────────────────── */
    function hit(sx, sy) {
      var best = -1, bd = 1e9;
      for (var i = 0; i < visible.length; i++) {
        var n = visible[i], c = coords[n.id];
        if (!c) continue;
        var p = toScreen(c[0], c[1]);
        var r = Math.max(10, (nodeSize(n.loc) / 2 + 10) * cam.zoom);
        var d = (p[0] - sx) * (p[0] - sx) + (p[1] - sy) * (p[1] - sy);
        if (d < r * r && d < bd) { bd = d; best = n.id; }
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
      var dt = Math.min(0.05, (now - last) / 1000); last = now;
      var t = (now - t0) / 1000;
      var dark = theme === "dark";
      var bg = dark ? (flags.bloom ? C.NEON_BG : C.BG_DARK) : C.BG_LIGHT;

      ctx.clearRect(0, 0, view.w, view.h);
      /* C17 壁纸透出策略：深色普通态不铺底（壁纸 1:1 透出）；
       * Bloom 态 0.9 黑（保留氛围同时壁纸微透）；浅色态近实底（壁纸是深色系）。 */
      if (!(dark && !flags.bloom)) {
        ctx.fillStyle = dark
          ? U.rgba(C.NEON_BG, C.CANVAS_ALPHA_BLOOM)
          : U.rgba(C.BG_LIGHT, C.CANVAS_ALPHA_LIGHT);
        ctx.fillRect(0, 0, view.w, view.h);
      }

      if (!ir || !visible.length) return;

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

      /* F267 粒子星空 */
      if (stars && dark) {
        for (var i = 0; i < stars.length; i++) {
          var s = stars[i];
          s.x = (s.x + dt * 0.002) % 1; s.y = (s.y + dt * 0.001) % 1;
          ctx.fillStyle = "rgba(255,255,255," + s.a + ")";
          ctx.fillRect(s.x * view.w, s.y * view.h, s.sz, s.sz);
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

      /* F270 模块星云 */
      if (flags.nebula) {
        var mods = {};
        visible.forEach(function (n) {
          var c = coords[n.id]; if (!c) return;
          (mods[n.domain || "?"] = mods[n.domain || "?"] || []).push(c);
        });
        Object.keys(mods).forEach(function (k) {
          var pts = mods[k], x0 = 1e9, y0 = 1e9, x1 = -1e9, y1 = -1e9;
          pts.forEach(function (p) { x0 = Math.min(x0, p[0]); y0 = Math.min(y0, p[1]); x1 = Math.max(x1, p[0]); y1 = Math.max(y1, p[1]); });
          var cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
          var rad = Math.max(x1 - x0, y1 - y0) * 1.5;
          var p = toScreen(cx, cy), rr = rad * cam.zoom;
          if (rr < 4) return;
          var g = ctx.createRadialGradient(p[0], p[1], 0, p[0], p[1], rr);
          g.addColorStop(0, U.rgba(CA.semanticColor(k), C.NEBULA_ALPHA * 3));
          g.addColorStop(1, U.rgba(CA.semanticColor(k), 0));
          ctx.fillStyle = g;
          ctx.beginPath(); ctx.arc(p[0], p[1], rr, 0, Math.PI * 2); ctx.fill();
        });
      }

      /* F262 性能热力（高斯核） */
      if (flags.heat) {
        var src = visible.map(function (n) { var c = coords[n.id] || [0, 0]; return [c[0], c[1], n.hot || 0]; });
        var step = 32;
        ctx.save();
        for (var gx = 0; gx < view.w; gx += step) {
          for (var gy = 0; gy < view.h; gy += step) {
            var w = toWorld(gx + step / 2, gy + step / 2), v = 0;
            for (var si = 0; si < src.length; si++) {
              var d2 = Math.pow(w[0] - src[si][0], 2) + Math.pow(w[1] - src[si][1], 2);
              v += src[si][2] * Math.exp(-d2 / (2 * 40 * 40));
            }
            if (v < 1e-3) continue;
            ctx.fillStyle = U.rgba("#FF9500", U.clamp(v, 0, 1) * 0.55);
            ctx.fillRect(gx, gy, step, step);
          }
        }
        ctx.restore();
      }

      /* 视口裁剪（虚拟化）：视口外节点不渲染 */
      var lod = lodOf(cam.zoom);
      var shown = [];
      visible.forEach(function (n) {
        var c = coords[n.id]; if (!c) return;
        var p = toScreen(c[0], c[1]);
        if (p[0] < -200 || p[1] < -200 || p[0] > view.w + 200 || p[1] > view.h + 200) return;
        shown.push({ n: n, p: p });
      });

      /* F258 贝塞尔光束 / F265 渐变数据河 / F259 粒子 / F269 拖尾 */
      ctx.save();
      ctx.lineCap = "round";
      ir.edges.forEach(function (e) {
        var a = coords[e.from], b = coords[e.to];
        if (!a || !b) return;
        var pa = toScreen(a[0], a[1]), pb = toScreen(b[0], b[1]);
        if (Math.max(pa[0], pb[0]) < -100 || Math.min(pa[0], pb[0]) > view.w + 100) return;
        if (Math.max(pa[1], pb[1]) < -100 || Math.min(pa[1], pb[1]) > view.h + 100) return;
        var mx = (pa[0] + pb[0]) / 2, my = (pa[1] + pb[1]) / 2;
        var nx = -(pb[1] - pa[1]) * 0.22, ny = (pb[0] - pa[0]) * 0.22;
        var p1 = [mx + nx, my + ny], p2 = [mx - nx, my - ny];
        var st = beamStyle(e.freq);
        var src = ir.nodes[e.from];
        var color = e.kind === "dep" ? "#8E8E93" : CA.semanticColor(src.domain);
        var isSel = selected >= 0 && (e.from === selected || e.to === selected);

        /* F269 连线拖尾：透明度指数衰减 */
        if (flags.trail && isSel) {
          for (var k = 6; k >= 1; k--) {
            ctx.strokeStyle = U.rgba(color, st.a * Math.exp(-k / 4) * 0.5);
            ctx.lineWidth = st.w * cam.zoom * (1 + k * 0.25);
            strokeBez(pa, [mx + nx * (1 + k * .06), my + ny * (1 + k * .06)], [mx - nx * (1 + k * .06), my - ny * (1 + k * .06)], pb);
          }
        }
        if (e.kind === "dep") {
          ctx.setLineDash([1, 3]);
        } else {
          ctx.setLineDash([]);
        }
        /* F265 数据河：沿路径线性插值 */
        var grad = ctx.createLinearGradient(pa[0], pa[1], pb[0], pb[1]);
        grad.addColorStop(0, U.rgba(color, st.a));
        grad.addColorStop(0.5, riverColor(((e.from * 37 + e.to * 11) % 100) / 100));
        grad.addColorStop(1, U.rgba(CA.semanticColor(ir.nodes[e.to].domain), st.a * 0.6));
        ctx.strokeStyle = isSel ? U.rgba("#FFFFFF", 0.5) : grad;
        ctx.lineWidth = Math.max(0.6, st.w * cam.zoom);
        strokeBez(pa, p1, p2, pb);
        ctx.setLineDash([]);

        /* F259 数据流粒子：速度=吞吐量，沿贝塞尔路径 */
        if (useParticles && e.kind === "call") {
          var speed = U.clamp(e.freq * 100, 1, 100);
          var cnt = e.freq > 0.6 ? 3 : 2;
          for (var q = 0; q < cnt; q++) {
            var tt = ((t * speed + q / cnt) % 1);
            var pos = bez(pa, p1, p2, pb, tt);
            ctx.fillStyle = U.rgba("#007AFF", 0.85);
            ctx.beginPath();
            ctx.arc(pos[0], pos[1], 1.5 + e.freq * 1.5, 0, Math.PI * 2);
            ctx.fill();
          }
        }
      });
      ctx.restore();
      function strokeBez(a, b, c, d) {
        ctx.beginPath(); ctx.moveTo(a[0], a[1]);
        ctx.bezierCurveTo(b[0], b[1], c[0], c[1], d[0], d[1]); ctx.stroke();
      }
      function bez(p0, p1, p2, p3, t) {
        var u = 1 - t;
        return [
          u * u * u * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t * t * t * p3[0],
          u * u * u * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t * t * t * p3[1]
        ];
      }

      /* F263/F264 暗色霓虹 + Bloom 光晕（离屏高亮层 → screen 叠加） */
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

      /* 中层：节点 + 文字 */
      drawNodes(ctx, shown, t, lod, false, useBreathe);

      /* F272 声波频谱：画布底部 64 根柱，高度=复杂度 */
      if (flags.spectrum) {
        var bars = C.SPECTRUM_BARS, bw = view.w / bars;
        ctx.save();
        for (var bi = 0; bi < bars; bi++) {
          var n = visible[bi % Math.max(1, visible.length)];
          var hgt = U.clamp((n ? n.cc || 0.2 : 0.2) / 12, 0, 1) * 26 * (0.6 + 0.4 * Math.abs(Math.sin(t * 1.2 + bi * 0.4)));
          ctx.fillStyle = U.rgba(CA.semanticColor(n ? n.domain : ""), 0.35);
          ctx.fillRect(bi * bw, view.h - hgt, bw - 1, hgt);
        }
        ctx.restore();
      }

      /* 搜索命中：外圈白环 */
      if (searchHits && searchHits.size) {
        ctx.save();
        ctx.strokeStyle = U.rgba("#FFFFFF", 0.8); ctx.lineWidth = 1;
        shown.forEach(function (s) {
          if (!searchHits.has(s.n.id)) return;
          var r = Math.max(10, nodeSize(s.n.loc) / 2 * cam.zoom) + 8;
          ctx.beginPath(); ctx.arc(s.p[0], s.p[1], r, 0, Math.PI * 2); ctx.stroke();
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

      drawMinimap();
      if (opts.onStatus) opts.onStatus(status());
    }
    this.render = render;

    function drawNodes(c, shown, t, lod, brightOnly, useBreathe) {
      var br = useBreathe ? breathe(t) : 1;
      shown.forEach(function (s) {
        var n = s.n, p = s.p;
        var size = nodeSize(n.loc) * (lod === "module" ? 0.7 : 1) * cam.zoom;
        var g = glowOf(n.loc, n.hot || 0.3);
        var color = n.status === "dead" ? "#8E8E93" : CA.semanticColor(n.domain);
        var r = Math.max(3, size / 2);
        var isSel = n.id === selected, isHov = n.id === hovered;

        /* F257 发光节点 */
        if (!brightOnly) {
          c.save();
          if (flags.glow) { c.shadowColor = color; c.shadowBlur = (isSel ? 26 : 14) * g.bright * cam.zoom; }
          /* F268 玻璃态节点（Canvas 侧近似：半透明填充 + 高光描边） */
          c.fillStyle = U.rgba(color, U.clamp(0.14 * g.bright * br + 0.06, 0.06, 0.5));
          U.roundRect(c, p[0] - r, p[1] - r, r * 2, r * 2, Math.min(8, r));
          c.fill();
          c.shadowBlur = 0;
          c.strokeStyle = U.rgba(color, U.clamp(0.5 * br, 0.15, 0.9));
          c.lineWidth = isSel ? 1.6 : 0.7;
          c.stroke();
          c.restore();
        } else {
          /* Bloom 层：只画亮度超过阈值的发光核心 */
          if (g.bright < C.NEON_THRESHOLD && !isSel) return;
          c.save();
          c.shadowColor = color; c.shadowBlur = 18 * cam.zoom;
          c.fillStyle = U.rgba(color, 0.5 * g.bright * br);
          c.beginPath(); c.arc(p[0], p[1], r, 0, Math.PI * 2); c.fill();
          c.restore();
        }
        if (brightOnly) return;

        /* F260 循环光环：半径=节点×1.5，转速=迭代次数 */
        if (flags.orbit && n.loop) {
          var orad = r * 1.5, ang = t * (n.cc || 1) * 0.5;
          c.save();
          c.strokeStyle = U.rgba("#AF52DE", 0.55);
          c.lineWidth = 1;
          c.beginPath(); c.arc(p[0], p[1], orad, ang, ang + Math.PI * 1.2); c.stroke();
          c.restore();
        }

        /* F261 异常脉冲：品红波纹，周期 2s，半径=节点×3 */
        if (n.status === "bug" || n.status === "warn") {
          var ph = (t % 2) / 2, pr = r * 3 * ph;
          c.save();
          c.strokeStyle = U.rgba(C.PULSE_COLOR, (1 - ph) * 0.7);
          c.lineWidth = 1;
          c.beginPath(); c.arc(p[0], p[1], pr, 0, Math.PI * 2); c.stroke();
          c.restore();
        }

        /* 标签（F253 LOD × F301/302 三界面） */
        if (cam.zoom > 0.18 || isSel || isHov) {
          var txt = labelFor(n, lod, p);
          var fs = U.clamp(10 * Math.min(1.4, cam.zoom), 8, 14);
          c.save();
          c.font = "300 " + fs.toFixed(1) + "px Inter, system-ui, sans-serif";
          c.textAlign = "center"; c.textBaseline = "top";
          var tw = c.measureText(txt).width;
          var ty = p[1] + r + 5;
          /* F295 注释气泡（选中时） */
          if (isSel || isHov) {
            c.fillStyle = "rgba(10,10,15,0.72)";
            U.roundRect(c, p[0] - tw / 2 - 6, ty - 3, tw + 12, fs + 8, 6);
            c.fill();
          }
          c.fillStyle = n.status === "dead" ? "#8E8E93" : (isSel ? "#FFFFFF" : "#C7C7CC");
          c.fillText(txt, p[0], ty);
          /* F305 大白话标签（专业界面下补 11px 灰字） */
          if (mode !== "plain" && n.plain && (isSel || isHov)) {
            c.font = "300 10px Inter, system-ui, sans-serif";
            c.fillStyle = "#86868B";
            c.fillText(n.plain, p[0], ty + fs + 5);
          }
          c.restore();
        }
      });
    }

    /* ── F255 全景小地图 ───────────────────────────────────────────────── */
    function drawMinimap() {
      if (!mm) return;
      mm.clearRect(0, 0, C.MINIMAP_W, C.MINIMAP_H);
      mm.fillStyle = "rgba(0,0,0,0.45)";
      mm.fillRect(0, 0, C.MINIMAP_W, C.MINIMAP_H);
      var ww = Math.max(1, bbox[2] - bbox[0]), wh = Math.max(1, bbox[3] - bbox[1]);
      var sx = C.MINIMAP_W / ww, sy = C.MINIMAP_H / wh;
      mm.save();
      visible.forEach(function (n) {
        var c = coords[n.id]; if (!c) return;
        mm.fillStyle = U.rgba(n.status === "dead" ? "#8E8E93" : CA.semanticColor(n.domain), 0.75);
        mm.fillRect((c[0] - bbox[0]) * sx - 1, (c[1] - bbox[1]) * sy - 1, 2.5, 2.5);
      });
      mm.restore();
      /* 红框 = 当前视口 */
      var vw = view.w / cam.zoom, vh = view.h / cam.zoom;
      mm.strokeStyle = "#FF3B30"; mm.lineWidth = 1;
      mm.strokeRect((cam.x - vw / 2 - bbox[0]) * sx, (cam.y - vh / 2 - bbox[1]) * sy, vw * sx, vh * sy);
    }

    /* ── 交互：平移 / 对数缩放 / 选中 / 双击爆炸展开 ───────────────────── */
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
      self.zoomBy(-e.deltaY);
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
        cam.x = bbox[0] + (mx / C.MINIMAP_W) * (bbox[2] - bbox[0]);
        cam.y = bbox[1] + (my / C.MINIMAP_H) * (bbox[3] - bbox[1]);
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
