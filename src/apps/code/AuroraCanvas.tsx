/**
 * 一/七章 · 真实光影与绚烂极光引擎（Physically-Based Aurora & Stars）v2。
 * 项目分析空间专属的 WebGL 2.0 背景层，8 层严格顺序合成：
 *   L0 极深空底衬 → L1 瑞利散射梯度 → L2 远景星尘 → L3 体积冷雾
 *   → L4 三轨交织极光(A 主帘幕/B 缠绕副带/C 晶莹光丝 + 深层晕染/柔焦辉光)
 *   → L5 褶皱光柱(纵向拉伸,随帘幕流动) → L6 中景主星阵
 *   → L7 近景 4 角 SDF 衍射星芒 → L8 后期（Reinhard 色调映射/暗角/中央压暗/纸纹噪点）
 * 四层视差：随项目画布相机 5% / 15% / 45% / 100% 移动（variable:pv-cam）。
 * 泊松圆盘采样星表；仅最亮 2% 恒星带十字星芒；偶发冰晶粒子（4.2 惊喜感）。
 * safeMode/reduceMotion 渲染静态单帧；Shader 编译失败自动降级并轻提示。
 */

import { useEffect, useRef } from "react";
import { pushToast } from "../../state/uiStore";
import { VERT, FRAG_AURORA } from "./auroraShader";

const VERT_STAR = `#version 300 es
layout(location=0) in vec2 aPos;
layout(location=1) in float aSize;
layout(location=2) in float aBright;
layout(location=3) in float aFlare;
layout(location=4) in float aSeed;
uniform vec2 uCam;      // 归一化相机偏移
uniform float uTime;
out vec2 vUv;
out float vBright;
out float vFlare;
void main() {
  // 四层视差（七章 7.1）：最亮星=近景 100%，中等 45%，远景 15%
  float parallax = aBright > 0.98 ? 1.0 : (aBright > 0.55 ? 0.45 : 0.15);
  vec2 p = fract(aPos + uCam * parallax * 0.5 + 0.5);
  vUv = p;
  vBright = aBright;
  vFlare = aFlare;
  float tw = 0.72 + 0.28 * sin(uTime * (0.6 + aSeed * 2.2) + aSeed * 40.0);
  vBright *= tw;
  gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
  gl_PointSize = aSize;
}`;

const FRAG_BG = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform float uTime;
uniform vec2 uCam;

float hash(vec3 p) {
  p = fract(p * 0.3183099 + vec3(0.71, 0.113, 0.419));
  p *= 17.0;
  return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}
float noise(vec3 x) {
  vec3 i = floor(x); vec3 f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(mix(hash(i), hash(i + vec3(1,0,0)), f.x),
                 mix(hash(i + vec3(0,1,0)), hash(i + vec3(1,1,0)), f.x), f.y),
             mix(mix(hash(i + vec3(0,0,1)), hash(i + vec3(1,0,1)), f.x),
                 mix(hash(i + vec3(0,1,1)), hash(i + vec3(1,1,1)), f.x), f.y), f.z);
}
float fbm(vec3 p) {
  float v = 0.0; float a = 0.5;
  for (int i = 0; i < 4; i++) { v += a * noise(p); p *= 2.07; a *= 0.5; }
  return v;
}

void main() {
  // L0 极深空底衬 + L1 瑞利散射梯度（越靠近极光源方向越偏蓝越亮）
  vec2 uv = vUv;
  vec3 top = vec3(0.012, 0.020, 0.055);
  vec3 bottom = vec3(0.045, 0.075, 0.150);
  vec3 col = mix(bottom, top, pow(uv.y, 0.8));
  float rayleigh = pow(1.0 - abs(uv.y - 0.75) * 1.2, 3.0);
  col += vec3(0.05, 0.09, 0.17) * max(0.0, rayleigh);
  // L3 体积冷雾（Mie 感）：底层极缓流动，迎光面自然提亮
  vec2 fp = uv + uCam * 0.45 * 0.02;
  float fog = fbm(vec3(fp.x * 2.6, fp.y * 1.4 - uTime * 0.008, uTime * 0.012));
  float fogMask = smoothstep(0.65, 0.0, uv.y) * 0.5 + 0.12;
  col += vec3(0.10, 0.16, 0.26) * fog * fogMask;
  // L8 后期：暗角 + 中央压暗 + 水彩纸噪点（色彩防火墙内不出暖色）
  vec2 c = uv - 0.5;
  float vig = 1.0 - dot(c, c) * 0.85;
  col *= clamp(vig, 0.55, 1.0);
  float centerDim = 1.0 - smoothstep(0.18, 0.45, length(c)) * 0.30;
  col *= centerDim;
  col += (hash(vec3(uv * 900.0, uTime)) - 0.5) * 0.018;
  col = col / (1.0 + col * 0.35);   // 软色调映射，亮度硬封顶
  outColor = vec4(col, 1.0);
}`;

const FRAG_STAR = `#version 300 es
precision highp float;
in vec2 vUv;
in float vBright;
in float vFlare;
out vec4 outColor;
void main() {
  vec2 d = gl_PointCoord - 0.5;
  float r2 = dot(d, d);
  float core = exp(-r2 * 34.0);
  // L7：仅最亮 2% —— SDF 锐利 4 角冷蓝十字星芒，长度与亮度绑定
  float cross = 0.0;
  if (vFlare > 0.5) {
    float ax = abs(d.x);
    float ay = abs(d.y);
    float len = 0.30 + vBright * 0.18;
    cross = exp(-ax * ax * 320.0) * (1.0 - smoothstep(0.02, len, ay))
          + exp(-ay * ay * 320.0) * (1.0 - smoothstep(0.02, len, ax));
  }
  float a = (core + cross * 0.75) * vBright;
  if (a < 0.004) discard;
  vec3 tint = mix(vec3(0.78, 0.87, 1.0), vec3(0.95, 0.97, 1.0), core);
  outColor = vec4(tint * a, a);
}`;

// ---------- 泊松圆盘采样（Bridson）+ 星河带/虚空遮罩 ----------

interface Star { x: number; y: number; size: number; bright: number; flare: number; seed: number }

function mulberry(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0; a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function poissonDisk(count: number, seed: number): Star[] {
  const rnd = mulberry(seed);
  const stars: Star[] = [];
  const R = 0.022;
  const grid: Map<string, Star> = new Map();
  const cell = R / Math.SQRT2;
  const active: Star[] = [];
  const gk = (s: Star): string => `${Math.floor(s.x / cell)},${Math.floor(s.y / cell)}`;
  const tryPlace = (s: Star): boolean => {
    const gx = Math.floor(s.x / cell);
    const gy = Math.floor(s.y / cell);
    for (let dx = -2; dx <= 2; dx++) {
      for (let dy = -2; dy <= 2; dy++) {
        const n = grid.get(`${gx + dx},${gy + dy}`);
        if (n) {
          const ddx = n.x - s.x;
          const ddy = n.y - s.y;
          if (ddx * ddx + ddy * ddy < R * R) return false;
        }
      }
    }
    return true;
  };
  const first: Star = { x: rnd(), y: rnd(), size: 0, bright: 0, flare: 0, seed: rnd() };
  grid.set(gk(first), first);
  active.push(first);
  stars.push(first);
  while (active.length > 0 && stars.length < count) {
    const idx = Math.floor(rnd() * active.length);
    const base = active[idx]!;
    let placed = false;
    for (let k = 0; k < 24; k++) {
      const ang = rnd() * Math.PI * 2;
      const rad = R * (1 + rnd());
      const cand: Star = {
        x: Math.min(0.999, Math.max(0.001, base.x + Math.cos(ang) * rad)),
        y: Math.min(0.999, Math.max(0.001, base.y + Math.sin(ang) * rad)),
        size: 0, bright: 0, flare: 0, seed: rnd(),
      };
      // 星河带 + 虚空遮罩： Simplex 感密度加权（带外即暗物质虚空）
      const band = Math.exp(-Math.pow((cand.y - (0.85 - cand.x * 0.55)) * 3.4, 2));
      if (rnd() > 0.25 + band * 0.75) continue;
      if (!tryPlace(cand)) continue;
      grid.set(gk(cand), cand);
      active.push(cand);
      stars.push(cand);
      placed = true;
      break;
    }
    if (!placed) active.splice(idx, 1);
  }
  for (const s of stars) {
    const b = Math.pow(rnd(), 3);
    s.bright = 0.25 + b * 0.75;
    s.size = (1.5 + b * 5.0) * (s.bright > 0.985 ? 1.7 : 1.0);
    s.flare = s.bright > 0.98 ? 1 : 0;
  }
  return stars;
}

export function AuroraCanvas(props: { safeMode: boolean; reduceMotion: boolean }): React.ReactElement {
  const ref = useRef<HTMLCanvasElement>(null);
  const camRef = useRef({ x: 0, y: 0 });

  useEffect(() => {
    const onCam = (e: Event): void => {
      const d = (e as CustomEvent<{ x: number; y: number; z: number }>).detail;
      camRef.current = { x: d.x / Math.max(1, d.z), y: d.y / Math.max(1, d.z) };
    };
    window.addEventListener("variable:pv-cam", onCam);
    return () => window.removeEventListener("variable:pv-cam", onCam);
  }, []);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const gl = canvas.getContext("webgl2", { alpha: true, antialias: false, powerPreference: "low-power" });
    if (!gl) return; // 无 WebGL2：退回 CSS 星空层

    let degraded = false;
    const compile = (type: number, src: string): WebGLShader | null => {
      const sh = gl.createShader(type);
      if (!sh) return null;
      gl.shaderSource(sh, src);
      gl.compileShader(sh);
      if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
        console.warn("[aurora] shader compile failed:", gl.getShaderInfoLog(sh));
        gl.deleteShader(sh);
        return null;
      }
      return sh;
    };
    const program = (vs: string, fs: string): WebGLProgram | null => {
      const v = compile(gl.VERTEX_SHADER, vs);
      const f = compile(gl.FRAGMENT_SHADER, fs);
      if (!v || !f) return null;
      const p = gl.createProgram();
      if (!p) return null;
      gl.attachShader(p, v);
      gl.attachShader(p, f);
      gl.linkProgram(p);
      if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
        console.warn("[aurora] link failed:", gl.getProgramInfoLog(p));
        return null;
      }
      return p;
    };

    const progBg = program(VERT, FRAG_BG);
    const progAurora = program(VERT, FRAG_AURORA);
    const progStar = program(VERT_STAR, FRAG_STAR);
    if (!progBg || !progAurora || !progStar) {
      // 10.2 Shader 编译失败降级：界面完整，仅缺少特效层
      pushToast("info", "检测到显卡兼容问题，已自动切换到简化星空");
      return;
    }

    const quadVao = gl.createVertexArray();
    gl.bindVertexArray(quadVao);
    const quadBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, quadBuf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);

    const stars = poissonDisk(760, 20260829);
    const starData = new Float32Array(stars.length * 5);
    stars.forEach((s, i) => {
      starData[i * 5] = s.x;
      starData[i * 5 + 1] = s.y;
      starData[i * 5 + 2] = s.size;
      starData[i * 5 + 3] = s.bright;
      starData[i * 5 + 4] = s.flare;
    });
    const starVao = gl.createVertexArray();
    gl.bindVertexArray(starVao);
    const starBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, starBuf);
    gl.bufferData(gl.ARRAY_BUFFER, starData, gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 20, 0);
    gl.enableVertexAttribArray(1);
    gl.vertexAttribPointer(1, 1, gl.FLOAT, false, 20, 8);
    gl.enableVertexAttribArray(2);
    gl.vertexAttribPointer(2, 1, gl.FLOAT, false, 20, 12);
    gl.enableVertexAttribArray(3);
    gl.vertexAttribPointer(3, 1, gl.FLOAT, false, 20, 16);
    gl.enableVertexAttribArray(4);
    gl.vertexAttribPointer(4, 1, gl.FLOAT, false, 20, 20);
    gl.bindVertexArray(null);

    const uTimeBg = gl.getUniformLocation(progBg, "uTime");
    const uCamBg = gl.getUniformLocation(progBg, "uCam");
    const uTimeAu = gl.getUniformLocation(progAurora, "uTime");
    const uCamAu = gl.getUniformLocation(progAurora, "uCam");
    const uTimeSt = gl.getUniformLocation(progStar, "uTime");
    const uCamSt = gl.getUniformLocation(progStar, "uCam");

    const dpr = Math.min(window.devicePixelRatio || 1, 1.25);
    let disposed = false;
    let raf = 0;
    let frameCount = 0;
    void degraded;

    const resize = (): void => {
      const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
      const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
      if (canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
      }
      gl.viewport(0, 0, w, h);
    };

    const draw = (time: number): void => {
      if (disposed) return;
      resize();
      gl.clearColor(0, 0, 0, 0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      // L0-L3 背景幕（不透明）
      gl.disable(gl.BLEND);
      gl.useProgram(progBg);
      gl.bindVertexArray(quadVao);
      gl.uniform1f(uTimeBg, time);
      gl.uniform2f(uCamBg, camRef.current.x, camRef.current.y);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      // L2/L6/L7 星场（加性混合，位于极光之后）
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE);
      gl.useProgram(progStar);
      gl.bindVertexArray(starVao);
      gl.uniform1f(uTimeSt, time);
      gl.uniform2f(uCamSt, camRef.current.x, camRef.current.y);
      gl.drawArrays(gl.POINTS, 0, stars.length);
      // L4/L5 极光（预乘 alpha 混合：emit 可超出覆盖度，等效柔焦 Bloom 辉光）
      gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
      gl.useProgram(progAurora);
      gl.bindVertexArray(quadVao);
      gl.uniform1f(uTimeAu, time);
      gl.uniform2f(uCamAu, camRef.current.x, camRef.current.y);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
      gl.bindVertexArray(null);
    };

    const staticFrame = (): void => draw(12.34);

    if (props.safeMode || props.reduceMotion) {
      staticFrame();
    } else {
      const loop = (now: number): void => {
        if (disposed) return;
        frameCount++;
        // 30FPS 足够呈现"微风翻滚"，显著省电
        if (frameCount % 2 === 0 && document.visibilityState === "visible") {
          draw(now / 1000);
        }
        raf = requestAnimationFrame(loop);
      };
      raf = requestAnimationFrame(loop);
    }

    const onResize = (): void => {
      if (props.safeMode || props.reduceMotion) staticFrame();
    };
    window.addEventListener("resize", onResize);

    return () => {
      disposed = true;
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", onResize);
      const ext = gl.getExtension("WEBGL_lose_context");
      ext?.loseContext();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.safeMode, props.reduceMotion]);

  return <canvas ref={ref} className="pv-aurora" aria-hidden />;
}
