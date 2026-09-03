/** Minimal WebGL starfield. Returns a dispose fn; falls back gracefully. */
export interface StarfieldOptions {
  motion: number;      // 0..1 dynamic strength
  parallax: number;    // 0..1
  dprScale: number;    // render scale
}

export interface StarfieldHandle {
  resize: () => void;
  setOptions: (o: Partial<StarfieldOptions>) => void;
  setParallax: (nx: number, ny: number) => void;
  start: () => void;
  stop: () => void;
  renderOnce: () => void;
  dispose: () => void;
}

const VERT = `
attribute vec2 aPos;
varying vec2 vUv;
void main() {
  vUv = aPos * 0.5 + 0.5;
  gl_Position = vec4(aPos, 0.0, 1.0);
}`;

const FRAG = `
precision mediump float;
varying vec2 vUv;
uniform vec2 uRes;
uniform float uTime;
uniform float uMotion;
uniform float uParallaxX;
uniform float uParallaxY;

float hash(vec2 p) {
  p = fract(p * vec2(233.34, 851.73));
  p += dot(p, p + 23.45);
  return fract(p.x * p.y);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
    mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
    u.y);
}

float fbm(vec2 p) {
  float v = 0.0;
  float amp = 0.55;
  for (int i = 0; i < 3; i++) {
    v += amp * noise(p);
    p *= 2.15;
    amp *= 0.5;
  }
  return v;
}

void main() {
  vec2 uv = gl_FragCoord.xy / uRes.xy;
  vec2 p = uv * vec2(uRes.x / uRes.y, 1.0);

  // Deep space base gradient
  vec3 top = vec3(0.016, 0.031, 0.078);
  vec3 bottom = vec3(0.008, 0.012, 0.027);
  vec3 col = mix(bottom, top, uv.y);

  // Slow nebula clouds (very low intensity)
  float t = uTime * 0.008 * uMotion;
  vec2 np = p * 2.6 + vec2(t * 0.35, -t * 0.18) + vec2(uParallaxX * 0.06, uParallaxY * 0.06);
  float n = fbm(np);
  float n2 = fbm(np * 1.9 + vec2(-t * 0.22, t * 0.12));
  float nebula = smoothstep(0.45, 0.95, n * 0.72 + n2 * 0.38);
  vec3 nebCol = vec3(0.10, 0.17, 0.36);
  col += nebula * nebCol * 0.30 * (0.45 + uMotion * 0.55);

  // Distant galaxy band
  float band = exp(-pow((uv.y - 0.62 + n2 * 0.08) * 4.2, 2.0));
  col += band * vec3(0.05, 0.07, 0.13) * (0.5 + uMotion * 0.5);

  // Parallax offset for stars
  vec2 sp = p * 42.0 + vec2(uParallaxX * 1.4, uParallaxY * 1.4);
  vec2 cell = floor(sp);
  vec2 fp = fract(sp);
  float h = hash(cell);
  if (h > 0.928) {
    vec2 spos = vec2(hash(cell + 7.1), hash(cell + 3.7)) * 0.72 + 0.14;
    float d = length(fp - spos);
    float twinkle = 0.72 + 0.28 * sin(uTime * 0.0011 * (0.5 + h) + h * 40.0) * uMotion;
    float star = smoothstep(0.09, 0.012, d) * twinkle;
    col += star * vec3(0.82, 0.88, 1.0) * (0.32 + h * 0.68);
  }
  // Second dust layer (slower drift)
  vec2 sp2 = p * 90.0 + vec2(uParallaxX * 2.6, uParallaxY * 2.6) + vec2(t * 0.6, 0.0);
  float h2 = hash(floor(sp2));
  if (h2 > 0.962) {
    vec2 spos2 = vec2(hash(floor(sp2) + 11.3), hash(floor(sp2) + 5.9)) * 0.8 + 0.1;
    float d2 = length(fract(sp2) - spos2);
    col += smoothstep(0.16, 0.02, d2) * 0.20 * vec3(0.75, 0.83, 1.0);
  }

  // Vignette: darker center is handled by the UI mask layer; here darken edges.
  float vig = smoothstep(1.25, 0.35, length(uv - 0.5) * 1.6);
  col *= mix(0.75, 1.0, vig);

  gl_FragColor = vec4(col, 1.0);
}`;

function compile(gl: WebGLRenderingContext, type: number, src: string): WebGLShader | null {
  const sh = gl.createShader(type);
  if (!sh) return null;
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    console.error("[bg] shader error", gl.getShaderInfoLog(sh));
    gl.deleteShader(sh);
    return null;
  }
  return sh;
}

export function createStarfield(canvas: HTMLCanvasElement, opts: StarfieldOptions): StarfieldHandle | null {
  let gl: WebGLRenderingContext | null = null;
  try {
    gl = canvas.getContext("webgl", { antialias: false, alpha: false, powerPreference: "low-power" });
  } catch {
    gl = null;
  }
  if (!gl) return null;

  const vs = compile(gl, gl.VERTEX_SHADER, VERT);
  const fs = compile(gl, gl.FRAGMENT_SHADER, FRAG);
  const prog = gl.createProgram();
  if (!vs || !fs || !prog) return null;
  gl.attachShader(prog, vs);
  gl.attachShader(prog, fs);
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
    console.error("[bg] link error", gl.getProgramInfoLog(prog));
    return null;
  }
  gl.useProgram(prog);

  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, "aPos");
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

  const uRes = gl.getUniformLocation(prog, "uRes");
  const uTime = gl.getUniformLocation(prog, "uTime");
  const uMotion = gl.getUniformLocation(prog, "uMotion");
  const uPx = gl.getUniformLocation(prog, "uParallaxX");
  const uPy = gl.getUniformLocation(prog, "uParallaxY");

  let o: StarfieldOptions = { ...opts };
  let disposed = false;
  let rafId = 0;
  let running = false;
  let px = 0;
  let py = 0;
  let tx = 0;
  let ty = 0;
  let lost = false;

  const onLost = (e: Event) => {
    e.preventDefault();
    lost = true;
  };
  const onRestored = () => {
    lost = false;
    running = true;
    loop(performance.now());
  };
  canvas.addEventListener("webglcontextlost", onLost);
  canvas.addEventListener("webglcontextrestored", onRestored);

  function resize(): void {
    if (!gl || disposed) return;
    const dpr = Math.min(window.devicePixelRatio || 1, 2) * o.dprScale;
    const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
    const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
      gl.viewport(0, 0, w, h);
    }
  }

  function draw(time: number): void {
    if (!gl) return;
    gl.uniform2f(uRes, canvas.width, canvas.height);
    gl.uniform1f(uTime, time);
    gl.uniform1f(uMotion, o.motion);
    gl.uniform1f(uPx, px);
    gl.uniform1f(uPy, py);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  }

  function loop(time: number): void {
    if (disposed || !running || lost) return;
    px += (tx - px) * 0.04;
    py += (ty - py) * 0.04;
    draw(time);
    rafId = requestAnimationFrame(loop);
  }

  resize();
  draw(performance.now());

  return {
    resize,
    setOptions(next) {
      o = { ...o, ...next };
      if (o.motion <= 0 && running) {
        running = false;
        cancelAnimationFrame(rafId);
        draw(performance.now());
      } else if (o.motion > 0 && !running && !disposed && !lost) {
        running = true;
        loop(performance.now());
      }
    },
    renderOnce() {
      draw(performance.now());
    },
    setParallax(nx: number, ny: number) {
      tx = nx * o.parallax;
      ty = ny * o.parallax;
      if (!running && !disposed && !lost) draw(performance.now());
    },
    start() {
      if (!running && o.motion > 0 && !disposed && !lost) {
        running = true;
        loop(performance.now());
      }
    },
    stop() {
      running = false;
      cancelAnimationFrame(rafId);
    },
    dispose() {
      disposed = true;
      running = false;
      cancelAnimationFrame(rafId);
      canvas.removeEventListener("webglcontextlost", onLost);
      canvas.removeEventListener("webglcontextrestored", onRestored);
      const ext = gl?.getExtension("WEBGL_lose_context");
      ext?.loseContext();
    },
  };
}
