import { useEffect, useRef, useState } from "react";
import { errMessage, ipc } from "../../lib/ipc";
import { LivingWallpaper } from "./LivingWallpaper";

/**
 * 实机反馈：scene 着色器壁纸必须全本地渲染（不靠 Wallpaper Engine 本体，
 * 也不能退化为静态预览图）。本组件用 WebGL 编译 WE 片元着色器全屏渲染：
 * - 源码读取与 `#include` 递归展开全部在后端完成（只读、PathBuf 拼接、限深防环）
 * - WE 全局变量：CURTIME / TIME / resolution / g_Resolution / MOUSE /
 *   g_Texture0..3（未绑定的 sampler 挂 1×1 白纹理，缺纹理不致全黑）
 * - 任何一步失败（读取/编译/链接）→ 回退「活化图片」（粒子 + 缓动），
 *   绝不黑屏也绝不退回死静态（实机反馈：动态壁纸变静态的根因之一）
 */

/** WE 全局变量：只补声明缺失的（用户源码里已声明的不重复声明）。 */
function prependGlobals(src: string): string {
  const globals = [
    "uniform float CURTIME;",
    "uniform float TIME;",
    "uniform vec2 resolution;",
    "uniform vec4 g_Resolution;",
    "uniform vec4 MOUSE;",
    "uniform sampler2D g_Texture0;",
    "uniform sampler2D g_Texture1;",
    "uniform sampler2D g_Texture2;",
    "uniform sampler2D g_Texture3;",
  ];
  const head = globals.filter((g) => {
    const name = /uniform\s+\w+\s+(\w+);/.exec(g)?.[1] ?? "";
    return name && !new RegExp(`\\b${name}\\b`).test(src);
  });
  return `${head.join("\n")}\n${src}`;
}

function makeWhiteTexture(gl: WebGLRenderingContext): WebGLTexture {
  const tex = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 1, 1, 0, gl.RGBA, gl.UNSIGNED_BYTE, new Uint8Array([255, 255, 255, 255]));
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
  return tex;
}

export function SceneShaderWallpaper(props: {
  shaderPath: string;
  fallbackImage?: string;
  reduceMotion?: boolean;
  safeMode?: boolean;
  perfMode?: string;
}): React.ReactElement {
  const { shaderPath, fallbackImage } = props;
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
    const canvas = canvasRef.current;
    if (!canvas || !shaderPath) return;
    let disposed = false;
    let cleanup: (() => void) | null = null;

    void (async () => {
      try {
        const raw = await ipc.wpSceneShader(shaderPath);
        if (disposed) return;
        const src = prependGlobals(raw);

        const gl = canvas.getContext("webgl", { antialias: false, alpha: false, preserveDrawingBuffer: false })
          ?? canvas.getContext("experimental-webgl") as WebGLRenderingContext | null;
        if (!gl) throw new Error("no webgl");

        const compile = (type: number, s: string): WebGLShader => {
          const sh = gl.createShader(type);
          if (!sh) throw new Error("createShader");
          gl.shaderSource(sh, s);
          gl.compileShader(sh);
          if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
            const log = gl.getShaderInfoLog(sh) ?? "";
            throw new Error(`compile: ${log.slice(0, 400)}`);
          }
          return sh;
        };
        const vs = compile(gl.VERTEX_SHADER, `attribute vec2 aPos; void main(){ gl_Position = vec4(aPos, 0.0, 1.0); }`);
        const fs = compile(gl.FRAGMENT_SHADER, src);
        const prog = gl.createProgram();
        if (!prog) throw new Error("createProgram");
        gl.attachShader(prog, vs);
        gl.attachShader(prog, fs);
        gl.linkProgram(prog);
        if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
          throw new Error(`link: ${(gl.getProgramInfoLog(prog) ?? "").slice(0, 400)}`);
        }
        gl.useProgram(prog);
        const buf = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, buf);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
        const aPos = gl.getAttribLocation(prog, "aPos");
        gl.enableVertexAttribArray(aPos);
        gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);

        const white = makeWhiteTexture(gl);
        for (let i = 0; i < 4; i++) gl.uniform1i(gl.getUniformLocation(prog, `g_Texture${i}`), i);

        const uTime = gl.getUniformLocation(prog, "CURTIME");
        const uTime2 = gl.getUniformLocation(prog, "TIME");
        const uRes = gl.getUniformLocation(prog, "resolution");
        const uGRes = gl.getUniformLocation(prog, "g_Resolution");
        const uMouse = gl.getUniformLocation(prog, "MOUSE");

        const resize = () => {
          const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
          const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
          const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
          if (canvas.width !== w || canvas.height !== h) {
            canvas.width = w;
            canvas.height = h;
          }
        };
        resize();

        const capFps = props.perfMode === "low" || props.reduceMotion ? 30 : 60;
        const frameMs = 1000 / capFps;
        let raf = 0;
        let last = 0;
        const t0 = performance.now();
        const loop = (now: number) => {
          if (disposed) return;
          raf = requestAnimationFrame(loop);
          if (now - last < frameMs - 1) return;
          last = now;
          resize();
          gl.viewport(0, 0, canvas.width, canvas.height);
          const t = (now - t0) / 1000;
          if (uTime) gl.uniform1f(uTime, t);
          if (uTime2) gl.uniform1f(uTime2, t);
          if (uRes) gl.uniform2f(uRes, canvas.width, canvas.height);
          if (uGRes) gl.uniform4f(uGRes, canvas.width, canvas.height, 1 / canvas.width, 1 / canvas.height);
          if (uMouse) gl.uniform4f(uMouse, 0, 0, 0, 0);
          for (let i = 0; i < 4; i++) {
            gl.activeTexture(gl.TEXTURE0 + i);
            gl.bindTexture(gl.TEXTURE_2D, white);
          }
          gl.drawArrays(gl.TRIANGLES, 0, 3);
        };
        raf = requestAnimationFrame(loop);
        cleanup = () => {
          cancelAnimationFrame(raf);
          gl.getExtension("WEBGL_lose_context")?.loseContext();
        };
      } catch (e) {
        if (!disposed) {
          console.warn("[wallpaper] scene shader fallback to preview", errMessage(e).message);
          setFailed(true);
        }
      }
    })();

    return () => {
      disposed = true;
      cleanup?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shaderPath, props.reduceMotion, props.perfMode]);

  if (failed || !shaderPath) {
    // 编译失败回退「活化图片」：粒子 + Ken Burns，保持壁纸的呼吸感（不退死静态）。
    return fallbackImage ? (
      <LivingWallpaper
        imagePath={fallbackImage}
        reduceMotion={props.reduceMotion}
        safeMode={props.safeMode}
        perfMode={props.perfMode}
      />
    ) : (
      <div className="wallpaper wallpaper-solid" aria-hidden />
    );
  }
  return <canvas ref={canvasRef} className="wallpaper-canvas-fill" aria-hidden />;
}
