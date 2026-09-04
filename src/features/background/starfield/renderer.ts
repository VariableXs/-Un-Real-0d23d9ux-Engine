import type { TierSpec } from "./math";
import {
  AURCOMP_FS, AURORA_FS, BLUR_FS, BRIGHT_FS, BRIGHT_VS, COMPOSITE_FS, QUAD_VS, SKY_FS, STAR_FS, STAR_VS,
} from "./shaders";

/** Per-frame GPU state handed to `Gpu.draw`. */
export interface GpuFrame {
  /** Seconds since engine start. */
  time: number;
  dpr: number;
  /** Canvas viewport offset (CSS px) driving depth parallax. */
  camX: number;
  camY: number;
  /** Star field wrap span (CSS px). */
  spanW: number;
  spanH: number;
  margin: number;
  motion: number;
  cursorX: number;
  cursorY: number;
  zoomVel: number;
  centerDim: boolean;
  tier: TierSpec;
  /** Pointer inside the window (kept for API parity). */
  cursorActive: boolean;
  /** True while the user is flinging the canvas — drops L10 detail (LOD). */
  fastMove: boolean;
}

export interface Gpu {
  resize: (wCss: number, hCss: number, dpr: number) => void;
  uploadStars: (data: Float32Array) => void;
  draw: (f: GpuFrame) => void;
  dispose: () => void;
}

function compile(gl: WebGL2RenderingContext, type: number, src: string): WebGLShader | null {
  const sh = gl.createShader(type);
  if (!sh) return null;
  gl.shaderSource(sh, src);
  gl.compileShader(sh);
  if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
    console.error("[starfield] shader compile error:", gl.getShaderInfoLog(sh));
    gl.deleteShader(sh);
    return null;
  }
  return sh;
}

function link(gl: WebGL2RenderingContext, vs: string, fs: string): WebGLProgram | null {
  const v = compile(gl, gl.VERTEX_SHADER, vs);
  const f = compile(gl, gl.FRAGMENT_SHADER, fs);
  const p = gl.createProgram();
  if (!v || !f || !p) return null;
  gl.attachShader(p, v);
  gl.attachShader(p, f);
  gl.linkProgram(p);
  gl.deleteShader(v);
  gl.deleteShader(f);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
    console.error("[starfield] link error:", gl.getProgramInfoLog(p));
    gl.deleteProgram(p);
    return null;
  }
  return p;
}

interface Target {
  fbo: WebGLFramebuffer;
  tex: WebGLTexture;
  w: number;
  h: number;
}

/**
 * Bakes a tileable 512² fractal-value-noise texture on the CPU once at
 * engine init and uploads it as REPEAT-wrapped mipmapped luminance. Every
 * high-frequency material detail (rock grain, snow sparkle, water shimmer,
 * pine needles, soil) samples this instead of paying runtime fbm — richer
 * octaves at a single texture fetch. Works on both window and worker
 * threads via OffscreenCanvas.
 */
function bakeDetailTexture(gl: WebGL2RenderingContext): WebGLTexture {
  const N = 512;
  const tex = gl.createTexture();
  if (!tex) return tex as unknown as WebGLTexture;
  try {
    let cnv: OffscreenCanvas | HTMLCanvasElement;
    if (typeof OffscreenCanvas !== "undefined") cnv = new OffscreenCanvas(N, N);
    else {
      cnv = document.createElement("canvas");
      cnv.width = N;
      cnv.height = N;
    }
    const ctx = cnv.getContext("2d") as CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D;
    const img = ctx.createImageData(N, N);
    const data = img.data;
    // Tileable value-noise lattice: indices wrap per octave frequency.
    const h2 = (x: number, y: number): number => {
      const s = Math.sin(x * 127.1 + y * 311.7) * 43758.5453;
      return s - Math.floor(s);
    };
    const vnoise = (x: number, y: number, p: number): number => {
      const ix = Math.floor(x), iy = Math.floor(y);
      const fx = x - ix, fy = y - iy;
      const ux = fx * fx * (3 - 2 * fx), uy = fy * fy * (3 - 2 * fy);
    const w = (a: number): number => ((a % p) + p) % p;
    const a = h2(w(ix), w(iy));
    const b = h2(w(ix + 1), w(iy));
    const c = h2(w(ix), w(iy + 1));
    const d = h2(w(ix + 1), w(iy + 1));
      return a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy;
    };
    for (let y = 0; y < N; y++) {
      for (let x = 0; x < N; x++) {
        let v = 0;
        let r = 0;
        let amp = 0.5;
        let ramp = 0.5;
        let f = 8; // cells across the tile
        let norm = 0;
        let rnorm = 0;
        for (let o = 0; o < 6; o++) {
          const nv = vnoise((x / N) * f, (y / N) * f, f);
          v += amp * nv;
          // Ridged channel: sharp crest lines for gullies and bark.
          r += ramp * (1.0 - Math.abs(nv * 2.0 - 1.0));
          norm += amp;
          rnorm += ramp;
          amp *= 0.55;
          ramp *= 0.6;
          f *= 2;
        }
        v /= norm;
        r /= rnorm;
        const i = (y * N + x) * 4;
        data[i] = Math.round(v * 255);
        data[i + 1] = Math.round(r * 255);
        data[i + 2] = Math.round(((v * 0.5 + r * 0.5)) * 255);
        data[i + 3] = 255;
      }
    }
    ctx.putImageData(img, 0, 0);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, gl.RGBA, gl.UNSIGNED_BYTE, cnv as TexImageSource);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.generateMipmap(gl.TEXTURE_2D);
  } catch {
    // No 2D canvas available — fall back to a neutral mid-grey 1×1 so the
    // sampler stays legal and detail terms degrade to flat.
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE,
      new Uint8Array([128, 128, 128, 255]));
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  }
  return tex;
}

/** Soft-focus bloom tuning: a high threshold + gentle intensity keeps the
 *  aurora crisp — heavy bloom is what read as "plastic". */
const BLOOM_THRESHOLD = 0.82;
const BLOOM_INTENSITY = 0.50;

/**
 * Builds the pure-sky pipeline: sky + stars at full resolution, aurora +
 * clouds + meteors on a dedicated layer, tier-gated bloom post-chain.
 * Returns null when WebGL2 / shader compilation is unavailable — callers
 * fall back to the CSS layer.
 */
export function createGpu(gl: WebGL2RenderingContext): Gpu | null {
  const skyProg = link(gl, QUAD_VS, SKY_FS);
  const auroraProg = link(gl, QUAD_VS, AURORA_FS);
  const starProg = link(gl, STAR_VS, STAR_FS);
  const brightProg = link(gl, BRIGHT_VS, BRIGHT_FS);
  const blurProg = link(gl, BRIGHT_VS, BLUR_FS);
  const compProg = link(gl, BRIGHT_VS, COMPOSITE_FS);
  const aurCompProg = link(gl, QUAD_VS, AURCOMP_FS);
  if (!skyProg || !auroraProg || !starProg || !brightProg || !blurProg || !compProg || !aurCompProg) return null;

  const emptyVao = gl.createVertexArray();
  const starVao = gl.createVertexArray();
  const starBuf = gl.createBuffer();
  let starCount = 0;
  const detailTex = bakeDetailTexture(gl);

  // ---- star attribute layout: 6 floats × 4 bytes = 24 B/star ----
  gl.bindVertexArray(starVao);
  gl.bindBuffer(gl.ARRAY_BUFFER, starBuf);
  const STRIDE = 24;
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, STRIDE, 0);
  gl.enableVertexAttribArray(1);
  gl.vertexAttribPointer(1, 1, gl.FLOAT, false, STRIDE, 8);
  gl.enableVertexAttribArray(2);
  gl.vertexAttribPointer(2, 1, gl.FLOAT, false, STRIDE, 12);
  gl.enableVertexAttribArray(3);
  gl.vertexAttribPointer(3, 2, gl.FLOAT, false, STRIDE, 16);
  gl.bindVertexArray(null);

  // ---- uniforms ----
  const sRes = gl.getUniformLocation(skyProg, "uRes");
  const sTime = gl.getUniformLocation(skyProg, "uTime");
  const sCam = gl.getUniformLocation(skyProg, "uCam");
  const sDpr = gl.getUniformLocation(skyProg, "uDpr");

  const aRes = gl.getUniformLocation(auroraProg, "uRes");
  const aTime = gl.getUniformLocation(auroraProg, "uTime");
  const aAurora = gl.getUniformLocation(auroraProg, "uAurora");
  const aFlow = gl.getUniformLocation(auroraProg, "uFlow");
  const aDetail = gl.getUniformLocation(auroraProg, "uDetail");
  const aCam = gl.getUniformLocation(auroraProg, "uCam");
  const aDpr = gl.getUniformLocation(auroraProg, "uDpr");
  const aGrain = gl.getUniformLocation(auroraProg, "uGrain");


  const s = (n: string): WebGLUniformLocation | null => gl.getUniformLocation(starProg, n);
  const u = {
    res: s("uRes"), dpr: s("uDpr"), time: s("uTime"), cam: s("uCam"),
    spanW: s("uSpanW"), spanH: s("uSpanH"), margin: s("uMargin"),
    globalPulse: s("uGlobalPulse"), twoLayer: s("uTwoLayer"),
    indepTwinkle: s("uIndepTwinkle"), bokeh: s("uBokeh"),
    motion: s("uMotion"),
    square: s("uSquare"), flareOn: s("uFlareOn"),
  };

  const bScene = gl.getUniformLocation(brightProg, "uScene");
  const bThresh = gl.getUniformLocation(brightProg, "uThreshold");
  const lTex = gl.getUniformLocation(blurProg, "uTex");
  const lDir = gl.getUniformLocation(blurProg, "uDir");
  const cScene = gl.getUniformLocation(compProg, "uScene");
  const cBloom = gl.getUniformLocation(compProg, "uBloom");
  const cInt = gl.getUniformLocation(compProg, "uIntensity");
  const acScene = gl.getUniformLocation(aurCompProg, "uScene");
  const acAur = gl.getUniformLocation(aurCompProg, "uAur");

  let disposed = false;
  let sceneT: Target | null = null;
  let auroraT: Target | null = null;
  let finalT: Target | null = null;
  let bloomA: Target | null = null;
  let bloomB: Target | null = null;

  function makeTarget(w: number, h: number, linear: boolean): Target | null {
    const tex = gl.createTexture();
    const fbo = gl.createFramebuffer();
    if (!tex || !fbo) return null;
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
    const filter = linear ? gl.LINEAR : gl.NEAREST;
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, filter);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, filter);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    gl.bindFramebuffer(gl.FRAMEBUFFER, fbo);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
    gl.clearColor(0, 0, 0, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.bindTexture(gl.TEXTURE_2D, null);
    return { fbo, tex, w, h };
  }

  function destroyTarget(t: Target | null): void {
    if (!t) return;
    gl.deleteFramebuffer(t.fbo);
    gl.deleteTexture(t.tex);
  }

  function ensureTargets(w: number, h: number, fullResAurora: boolean): void {
    const auW = fullResAurora ? w : Math.max(1, w >> 1);
    const auH = fullResAurora ? h : Math.max(1, h >> 1);
    if (sceneT && sceneT.w === w && sceneT.h === h && auroraT && auroraT.w === auW && auroraT.h === auH) return;
    destroyTarget(sceneT);
    destroyTarget(auroraT);
    destroyTarget(finalT);
    destroyTarget(bloomA);
    destroyTarget(bloomB);
    sceneT = makeTarget(w, h, true);
    // The aurora's fine texture striations need full resolution — half-res
    // upsampling is exactly what read as "plastic". Low tiers keep half-res
    // for perf.
    auroraT = makeTarget(auW, auH, true);
    finalT = makeTarget(w, h, true);
    const bw = Math.max(1, w >> 2);
    const bh = Math.max(1, h >> 2);
    bloomA = makeTarget(bw, bh, true);
    bloomB = makeTarget(bw, bh, true);
  }

  function drawScene(f: GpuFrame): void {
    gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight);
    // Pass A — watercolor night-sky base (opaque).
    gl.disable(gl.BLEND);
    gl.useProgram(skyProg);
    gl.bindVertexArray(emptyVao);
    gl.uniform2f(sRes, gl.drawingBufferWidth, gl.drawingBufferHeight);
    gl.uniform1f(sTime, f.time);
    gl.uniform2f(sCam, f.camX, f.camY);
    gl.uniform1f(sDpr, f.dpr);
    gl.drawArrays(gl.TRIANGLES, 0, 3);

    // Pass B — stars UNDER the aurora so the curtains veil them (depth).
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    gl.useProgram(starProg);
    gl.bindVertexArray(starVao);
    gl.uniform2f(u.res, gl.drawingBufferWidth, gl.drawingBufferHeight);
    gl.uniform1f(u.dpr, f.dpr);
    gl.uniform1f(u.time, f.time);
    gl.uniform2f(u.cam, f.camX, f.camY);
    gl.uniform1f(u.spanW, f.spanW);
    gl.uniform1f(u.spanH, f.spanH);
    gl.uniform1f(u.margin, f.margin);
    const t = f.tier;
    gl.uniform1f(u.globalPulse, t.globalPulse ? 1 : 0);
    gl.uniform1f(u.twoLayer, t.twoLayerDepth ? 1 : 0);
    gl.uniform1f(u.indepTwinkle, t.independentTwinkle ? 1 : 0);
    gl.uniform1f(u.bokeh, t.bokehDispersion ? f.zoomVel : 0);
    gl.uniform1f(u.motion, f.motion);
    gl.uniform1f(u.square, t.squarePoints && !t.independentTwinkle ? 1 : 0);
    gl.uniform1f(u.flareOn, t.flare ? 1 : 0);
    gl.drawArrays(gl.POINTS, 0, starCount);
    gl.bindVertexArray(null);
  }

  /** Pass C at HALF resolution — the aurora is soft plasmatic light, so the
   *  quarter pixel count is visually lossless and ~4× cheaper on the GPU.
   *  Drawn WITHOUT blending: the fullscreen triangle overwrites the target
   *  and empty regions must land at (0,0,0,0) so the composite keeps the
   *  sky visible — blending against the clear color would leave alpha=1
   *  black everywhere and black out the whole sky. */
  function drawAuroraLayer(f: GpuFrame, target: Target): void {
    gl.bindFramebuffer(gl.FRAMEBUFFER, target.fbo);
    gl.viewport(0, 0, target.w, target.h);
    gl.disable(gl.BLEND);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, detailTex);
    gl.uniform1i(aGrain, 1);
    gl.useProgram(auroraProg);
    gl.bindVertexArray(emptyVao);
    gl.uniform2f(aRes, target.w, target.h);
    gl.uniform1f(aTime, f.time);
    gl.uniform1f(aAurora, f.tier.aurora);
    gl.uniform1f(aFlow, f.tier.auroraFlow ? 1 : 0);
    gl.uniform1f(aDetail, f.tier.detailMaster && !f.fastMove ? 1 : 0); // LOD
    gl.uniform2f(aCam, f.camX, f.camY);
    // Keep the camera offset world-locked regardless of layer resolution.
    gl.uniform1f(aDpr, f.dpr * (target.w / gl.drawingBufferWidth));
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, null);
  }

  /** Composite the half-res aurora over the full-res sky+stars. */
  function drawAurComp(dst: Target | null, W: number, H: number): void {
    gl.bindFramebuffer(gl.FRAMEBUFFER, dst ? dst.fbo : null);
    gl.viewport(0, 0, W, H);
    gl.disable(gl.BLEND);
    gl.useProgram(aurCompProg);
    gl.bindVertexArray(emptyVao);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, sceneT!.tex);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, auroraT!.tex);
    gl.uniform1i(acScene, 0);
    gl.uniform1i(acAur, 1);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, null);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, null);
  }

  function blur(src: Target, dst: Target, dx: number, dy: number): void {
    gl.bindFramebuffer(gl.FRAMEBUFFER, dst.fbo);
    gl.viewport(0, 0, dst.w, dst.h);
    gl.useProgram(blurProg);
    gl.bindVertexArray(emptyVao);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, src.tex);
    gl.uniform1i(lTex, 0);
    gl.uniform2f(lDir, dx / src.w, dy / src.h);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }

  return {
    resize(_w, _h, _dpr) { /* targets re-checked in draw */ },

    uploadStars(data: Float32Array): void {
      if (disposed) return;
      gl.bindBuffer(gl.ARRAY_BUFFER, starBuf);
      gl.bufferData(gl.ARRAY_BUFFER, data, gl.STATIC_DRAW);
      starCount = data.length / 6;
    },

    draw(f: GpuFrame): void {
      if (disposed || starCount === 0) return;
      const W = gl.drawingBufferWidth;
      const H = gl.drawingBufferHeight;

      ensureTargets(W, H, !!(f.tier.detailMaster || f.tier.auroraFlow));
      if (!sceneT || !auroraT || !finalT) return;

      // 1) Sky + stars at full resolution.
      gl.bindFramebuffer(gl.FRAMEBUFFER, sceneT.fbo);
      drawScene(f);

      // 2) Aurora + clouds + meteors at HALF resolution (plasmatic light is
      //    resolution-forgiving; the quarter pixel count buys ~4× GPU time).
      drawAuroraLayer(f, auroraT);

      if (f.tier.bloom && bloomA && bloomB) {
        // 3a) Composite, then bloom chain back to the screen.
        drawAurComp(finalT, W, H);

        gl.bindFramebuffer(gl.FRAMEBUFFER, bloomA.fbo);
        gl.viewport(0, 0, bloomA.w, bloomA.h);
        gl.useProgram(brightProg);
        gl.bindVertexArray(emptyVao);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, finalT.tex);
        gl.uniform1i(bScene, 0);
        gl.uniform1f(bThresh, BLOOM_THRESHOLD);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

        blur(bloomA, bloomB, 1.75, 0);
        blur(bloomB, bloomA, 0, 1.75);

        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        gl.viewport(0, 0, W, H);
        gl.useProgram(compProg);
        gl.bindVertexArray(emptyVao);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, finalT.tex);
        gl.uniform1i(cScene, 0);
        gl.activeTexture(gl.TEXTURE1);
        gl.bindTexture(gl.TEXTURE_2D, bloomA.tex);
        gl.uniform1i(cBloom, 1);
        gl.uniform1f(cInt, BLOOM_INTENSITY);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, null);
      } else {
        // 3b) No-bloom tiers: composite straight to the screen.
        drawAurComp(null, W, H);
      }
    },

    dispose(): void {
      disposed = true;
      destroyTarget(sceneT);
      destroyTarget(auroraT);
      destroyTarget(finalT);
      destroyTarget(bloomA);
      destroyTarget(bloomB);
      sceneT = auroraT = finalT = bloomA = bloomB = null;
      gl.deleteBuffer(starBuf);
      gl.deleteVertexArray(starVao);
      gl.deleteVertexArray(emptyVao);
      gl.deleteProgram(skyProg);
      gl.deleteProgram(auroraProg);
      gl.deleteProgram(starProg);
      gl.deleteProgram(aurCompProg);
      gl.deleteProgram(brightProg);
      gl.deleteProgram(blurProg);
      gl.deleteProgram(compProg);
      gl.deleteTexture(detailTex);
    },
  };
}
