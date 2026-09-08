/**
 * N-07 壁纸工坊 2.0（Wallpaper Studio，功能全景 L807-825）。
 *
 * 四引擎：Video（mp4/webm 本地）/ Shader（WebGL 片元着色器 + uniform 滑杆）/
 * Generative（星空参数化预设，复用 features/background/webgl.ts）/ Web（本地 html）。
 * 另含 .vwp 壁纸包导入导出（vwp.ts）与性能联动（perfGate.ts）。
 *
 * 诚实边界（本车道口径，与主控集成阶段对接）：
 * - 工坊不写 settings.ts（禁改）：「应用」= 派发 window CustomEvent
 *   `ai04:wallpaper-apply` { detail: { patch: { wallpaperMode, customBg } } }，
 *   由主控在集成阶段接到 onPatchSettings；工坊内先行本地预览；
 * - 视频应用需要真实路径（Tauri 文件对话框）；纯浏览器环境仅能本地预览；
 * - 网页引擎仅允许本地 html（远程 URL 直接拒绝，L818 红线）；
 * - shader 预览为工坊内本地 WebGL 编译（源码来自 wpSceneShader 读盘或本地文件），
 *   编译失败降级静态帧 + 诚实提示，绝不黑屏（与 SceneShaderWallpaper 同策略）。
 *
 * 自挂载：模块加载即监听 `ai04:open-feature` {feature:"wallpaper-studio"}，
 * 主控集成阶段 `void import("system/wallpaper/Workshop")` 一次即激活。
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { toAssetUrl } from "../../features/background/CosmicBackground";
import { createStarfield, type StarfieldHandle } from "../../features/background/webgl";
import { errMessage, ipc } from "../../lib/ipc";
import { loadSettings } from "../../lib/settings";
import { mountOnEvent, installCloseHandler, dispatchClose } from "./mount";
import { isDegradeActive, isTypingRecent, type PerfGateDecision } from "./perfGate";
import { parseUniforms, type ParsedUniform } from "./uniforms";
import { wpT } from "./labels";
import {
  buildVwp,
  dataUrlToText,
  downloadVwp,
  parseVwpText,
  textToDataUrl,
  validateVwp,
  type VwpError,
  type VwpPack,
} from "./vwp";

type TabId = "video" | "shader" | "generative" | "web" | "pack";

/** 派发应用请求（主控集成阶段消费）。customBg 为 Partial，由主控浅合并。 */
function requestApply(patch: { wallpaperMode: string; customBg?: Record<string, unknown> }): void {
  window.dispatchEvent(new CustomEvent("ai04:wallpaper-apply", { detail: { patch } }));
}

/** Tauri 文件对话框选路径；不可用（纯浏览器）返回 null → 调用方走诚实降级。 */
async function pickPath(extensions: string[]): Promise<string | null> {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const sel = await open({ multiple: false, directory: false, filters: [{ name: "file", extensions }] });
    if (typeof sel === "string") return sel;
    if (Array.isArray(sel)) return sel[0] ?? null;
    return null;
  } catch {
    return null;
  }
}

/* ---------------- 性能联动 hook（事件源在此，决策在 perfGate.ts 纯函数） ---------------- */

function usePerfGate(): PerfGateDecision {
  const lastKeyAt = useRef(0);
  const [now, setNow] = useState(() => Date.now());
  const [fullscreen, setFullscreen] = useState(false);
  const [battery, setBattery] = useState(false);

  useEffect(() => {
    const onKey = (): void => {
      lastKeyAt.current = Date.now();
    };
    window.addEventListener("keydown", onKey);
    const timer = window.setInterval(() => setNow(Date.now()), 500);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.clearInterval(timer);
    };
  }, []);

  // sys://fullscreen（与 DesktopShell 同款监听）；非 Tauri 环境静默视为非全屏。
  useEffect(() => {
    let un: (() => void) | null = null;
    let disposed = false;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<boolean>("sys://fullscreen", (e) => {
          if (!disposed) setFullscreen(e.payload === true);
        }),
      )
      .then((fn) => {
        if (disposed) fn();
        else un = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, []);

  // 电池：navigator.getBattery 可用才启用；不可用视为非电池（台式机口径）。
  useEffect(() => {
    const nav = navigator as Navigator & { getBattery?: () => Promise<BatteryLike> };
    if (!nav.getBattery) return;
    let disposed = false;
    let b: BatteryLike | null = null;
    const upd = (): void => {
      if (b && !disposed) setBattery(!b.charging);
    };
    void nav
      .getBattery()
      .then((bat) => {
        if (disposed) return;
        b = bat;
        upd();
        bat.addEventListener?.("chargingchange", upd);
      })
      .catch(() => {});
    return () => {
      disposed = true;
      b?.removeEventListener?.("chargingchange", upd);
    };
  }, []);

  return isDegradeActive({ typing: isTypingRecent(lastKeyAt.current, now), fullscreen, battery });
}

interface BatteryLike extends EventTarget {
  charging: boolean;
}

/* ---------------- Shader 预览（本地编译；失败降级静态帧） ---------------- */

/** 与 SceneShaderWallpaper 同款 WE 全局变量补声明（该函数未导出，本地等价实现）。 */
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

function ShaderPreview(props: {
  source: string;
  fps: number;
  paused: boolean;
  uniforms: Record<string, number>;
}): React.ReactElement {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  const uniformsRef = useRef(props.uniforms);
  uniformsRef.current = props.uniforms;

  useEffect(() => {
    setFailed(null);
    const canvas = canvasRef.current;
    if (!canvas || !props.source) return;
    let disposed = false;

    void (async () => {
      try {
        const src = prependGlobals(props.source);
        const gl =
          canvas.getContext("webgl", { antialias: false, alpha: false }) ??
          (canvas.getContext("experimental-webgl") as WebGLRenderingContext | null);
        if (!gl) throw new Error("no webgl");

        const compile = (type: number, s: string): WebGLShader => {
          const sh = gl.createShader(type);
          if (!sh) throw new Error("createShader");
          gl.shaderSource(sh, s);
          gl.compileShader(sh);
          if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
            throw new Error((gl.getShaderInfoLog(sh) ?? "").slice(0, 200));
          }
          return sh;
        };
        const vs = compile(gl.VERTEX_SHADER, "attribute vec2 aPos; void main(){ gl_Position = vec4(aPos, 0.0, 1.0); }");
        const fs = compile(gl.FRAGMENT_SHADER, src);
        const prog = gl.createProgram();
        if (!prog) throw new Error("createProgram");
        gl.attachShader(prog, vs);
        gl.attachShader(prog, fs);
        gl.linkProgram(prog);
        if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
          throw new Error((gl.getProgramInfoLog(prog) ?? "").slice(0, 200));
        }
        gl.useProgram(prog);
        const buf = gl.createBuffer();
        gl.bindBuffer(gl.ARRAY_BUFFER, buf);
        gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
        const aPos = gl.getAttribLocation(prog, "aPos");
        gl.enableVertexAttribArray(aPos);
        gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);

        const uTime = gl.getUniformLocation(prog, "CURTIME") ?? gl.getUniformLocation(prog, "TIME");
        const uRes = gl.getUniformLocation(prog, "resolution");
        const uLocs = new Map<string, WebGLUniformLocation | null>();
        for (const name of Object.keys(uniformsRef.current)) {
          uLocs.set(name, gl.getUniformLocation(prog, name));
        }

        const resize = (): void => {
          const dpr = Math.min(window.devicePixelRatio || 1, 1.5);
          const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
          const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
          if (canvas.width !== w || canvas.height !== h) {
            canvas.width = w;
            canvas.height = h;
          }
        };

        const frameMs = 1000 / Math.max(1, props.fps);
        let raf = 0;
        let last = 0;
        const t0 = performance.now();
        const loop = (t: number): void => {
          if (disposed) return;
          raf = requestAnimationFrame(loop);
          if (props.paused) return; // 降级 = 停帧（保留最后一帧，即静态帧）
          if (t - last < frameMs - 1) return;
          last = t;
          resize();
          gl.viewport(0, 0, canvas.width, canvas.height);
          if (uTime) gl.uniform1f(uTime, (t - t0) / 1000);
          if (uRes) gl.uniform2f(uRes, canvas.width, canvas.height);
          for (const [name, loc] of uLocs) {
            if (loc) gl.uniform1f(loc, uniformsRef.current[name] ?? 0);
          }
          gl.drawArrays(gl.TRIANGLES, 0, 3);
        };
        raf = requestAnimationFrame(loop);
        return () => {
          cancelAnimationFrame(raf);
          gl.getExtension("WEBGL_lose_context")?.loseContext();
        };
      } catch (e) {
        if (!disposed) setFailed(errMessage(e).message || "compile failed");
      }
    })();

    return () => {
      disposed = true;
    };
    // fps/paused 经 ref 与每帧读取生效，源码变化才重建 GL 资源
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.source]);

  if (failed) {
    // 诚实降级：静态帧（CSS 渐变占位）+ 明确原因，绝不黑屏（L822）
    return (
      <div className="wp-studio-shader-fallback" role="img" aria-label="shader fallback">
        <span>{`⚠ ${failed}`}</span>
      </div>
    );
  }
  return <canvas ref={canvasRef} className="wp-studio-canvas" aria-hidden />;
}

/* ---------------- Generative 预览（复用现成星空渲染器） ---------------- */

function GenerativePreview(props: { motion: number; parallax: number; paused: boolean }): React.ReactElement {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [failed, setFailed] = useState(false);
  const handleRef = useRef<StarfieldHandle | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const handle = createStarfield(canvas, {
      motion: props.motion,
      parallax: props.parallax,
      dprScale: 1,
    });
    if (!handle) {
      setFailed(true);
      return;
    }
    setFailed(false);
    handleRef.current = handle;
    handle.start();
    return () => {
      handle.dispose();
      handleRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    handleRef.current?.setOptions({ motion: props.motion, parallax: props.parallax });
  }, [props.motion, props.parallax]);

  useEffect(() => {
    const h = handleRef.current;
    if (!h) return;
    if (props.paused) h.stop();
    else h.start();
  }, [props.paused]);

  if (failed) {
    return (
      <div className="wp-studio-shader-fallback" role="img" aria-label="starfield fallback">
        <span>WebGL 不可用，星空预览停用（桌面壁纸不受影响）</span>
      </div>
    );
  }
  return <canvas ref={canvasRef} className="wp-studio-canvas" aria-hidden />;
}

/* ---------------- 生成式预设（现成星空渲染器产品化） ---------------- */

interface GenPreset {
  id: string;
  name: string;
  desc: string;
  motion: number;
  parallax: number;
}

const GENERATIVE_PRESETS: GenPreset[] = [
  { id: "deep-space", name: "深空星野", desc: "致密星点 + 深蓝云气", motion: 0.55, parallax: 0.35 },
  { id: "calm-night", name: "静夜低语", desc: "稀疏缓动 · 打字友好", motion: 0.18, parallax: 0.1 },
];

/* ---------------- .vwp 错误码 → 文案 ---------------- */

function errText(t: (k: string) => string, code: VwpError): string {
  switch (code) {
    case "format":
      return t("packErrFormat");
    case "kind":
      return t("packErrKind");
    case "manifest":
      return t("packErrManifest");
    case "uniforms":
      return t("packErrUniforms");
    case "resource-url":
      return t("packErrResourceUrl");
    case "resource-type":
      return t("packErrResourceType");
    case "resource-size":
      return t("packErrResourceSize");
    default:
      return code;
  }
}

/* ---------------- 主组件 ---------------- */

export function Workshop(): React.ReactElement {
  const t = useMemo(() => wpT(), []);
  const gate = usePerfGate();
  const [tab, setTab] = useState<TabId>("video");
  const [toast, setToast] = useState("");

  // 只读预填（settings.ts 禁改，仅读取）
  const [curHtmlPath, setCurHtmlPath] = useState("");
  useEffect(() => {
    let alive = true;
    void loadSettings().then((s) => {
      if (alive) setCurHtmlPath(s.customBg.htmlPath);
    });
    return () => {
      alive = false;
    };
  }, []);

  // ---- Video 引擎 ----
  const [videoPath, setVideoPath] = useState("");
  const [videoUrl, setVideoUrl] = useState("");
  const [videoBrowserOnly, setVideoBrowserOnly] = useState(false);
  const videoRef = useRef<HTMLVideoElement | null>(null);
  useEffect(() => {
    const v = videoRef.current;
    if (!v || !videoUrl) return;
    if (gate.videoPaused) v.pause();
    else void v.play().catch(() => {});
  }, [gate.videoPaused, videoUrl]);

  const pickVideo = async (viaDialog: boolean): Promise<void> => {
    if (viaDialog) {
      const p = await pickPath(["mp4", "webm"]);
      if (p) {
        setVideoPath(p);
        setVideoBrowserOnly(false);
        setVideoUrl(toAssetUrl(p));
        return;
      }
    }
    // 浏览器降级：<input type=file> 本地预览（无路径，应用禁用）
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "video/mp4,video/webm";
    input.onchange = (): void => {
      const f = input.files?.[0];
      if (!f) return;
      setVideoPath("");
      setVideoBrowserOnly(true);
      setVideoUrl(URL.createObjectURL(f));
    };
    input.click();
  };

  // ---- Shader 引擎 ----
  const [shaderSource, setShaderSource] = useState("");
  const [shaderPath, setShaderPath] = useState("");
  const [uniformList, setUniformList] = useState<ParsedUniform[]>([]);
  const [uniformValues, setUniformValues] = useState<Record<string, number>>({});
  const [shaderBrowserOnly, setShaderBrowserOnly] = useState(false);

  const applyShaderSource = (src: string, path: string, browserOnly: boolean): void => {
    setShaderSource(src);
    setShaderPath(path);
    setShaderBrowserOnly(browserOnly);
    const list = parseUniforms(src);
    setUniformList(list);
    setUniformValues(Object.fromEntries(list.map((u) => [u.name, u.value])));
  };

  const pickShader = async (viaDialog: boolean): Promise<void> => {
    if (viaDialog) {
      const p = await pickPath(["frag", "glsl", "txt"]);
      if (p) {
        try {
          const src = await ipc.wpSceneShader(p); // 既有只读命令：读本地着色器源码
          applyShaderSource(src, p, false);
          return;
        } catch {
          /* 读取失败 → 走浏览器降级 */
        }
      }
    }
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".frag,.glsl,.txt";
    input.onchange = (): void => {
      const f = input.files?.[0];
      if (!f) return;
      const reader = new FileReader();
      reader.onload = (): void => applyShaderSource(String(reader.result ?? ""), "", true);
      reader.readAsText(f);
    };
    input.click();
  };

  // ---- Generative 引擎 ----
  const [genPreset, setGenPreset] = useState<GenPreset>(GENERATIVE_PRESETS[0]);
  const [genMotion, setGenMotion] = useState(GENERATIVE_PRESETS[0].motion);
  const [genParallax, setGenParallax] = useState(GENERATIVE_PRESETS[0].parallax);

  // ---- Web 引擎 ----
  const [webPath, setWebPath] = useState("");
  const [webRejected, setWebRejected] = useState(false);
  const pickWeb = async (): Promise<void> => {
    const p = await pickPath(["html", "htm"]);
    if (!p) return;
    if (/^https?:\/\//i.test(p)) {
      setWebRejected(true); // 红线：远程 URL 拒绝（L818）
      return;
    }
    setWebRejected(false);
    setWebPath(p);
  };

  // ---- .vwp 包 ----
  const [pack, setPack] = useState<VwpPack | null>(null);
  const [packErrors, setPackErrors] = useState<VwpError[]>([]);
  const importPack = (file: File): void => {
    const reader = new FileReader();
    reader.onload = (): void => {
      const res = parseVwpText(String(reader.result ?? ""));
      if (res.ok) {
        setPack(res.pack);
        setPackErrors([]);
      } else {
        setPack(null);
        setPackErrors(res.errors);
      }
    };
    reader.readAsText(file);
  };
  const exportPack = async (kind: "shader" | "generative" | "web"): Promise<void> => {
    let vwp: VwpPack | null = null;
    if (kind === "shader") {
      if (!shaderSource) {
        setToast(t("packExportShaderNeed"));
        return;
      }
      vwp = buildVwp({
        kind: "shader",
        manifest: { name: shaderPath.split(/[\\/]/).pop() || "shader-pack", netAccess: false },
        uniforms: uniformValues,
        resources: { "shader.frag": textToDataUrl(shaderSource, "text/plain") },
      });
    } else if (kind === "generative") {
      vwp = buildVwp({
        kind: "generative",
        manifest: {
          name: genPreset.name,
          preset: genPreset.id,
          params: { motion: genMotion, parallax: genParallax },
          netAccess: false,
        },
      });
    } else {
      // web 包：经本地资源协议读取 html 文本（非 Tauri 环境诚实失败）
      try {
        setToast(t("packExportWebTry"));
        const res = await fetch(toAssetUrl(webPath));
        const html = await res.text();
        vwp = buildVwp({
          kind: "web",
          manifest: { name: webPath.split(/[\\/]/).pop() || "web-pack", netAccess: false },
          resources: { "index.html": textToDataUrl(html, "text/html") },
        });
      } catch {
        setToast(t("packExportWebFail"));
        return;
      }
    }
    const check = validateVwp(vwp);
    if (check.ok) downloadVwp(check.pack, check.pack.manifest.name);
  };

  const packPreview = useMemo((): React.ReactElement | null => {
    if (!pack) return null;
    if (pack.kind === "shader") {
      const srcKey = Object.keys(pack.resources).find((k) => /\.(frag|glsl|txt)$/.test(k)) ?? "";
      const src = srcKey ? dataUrlToText(pack.resources[srcKey] ?? "") ?? "" : "";
      return <ShaderPreview source={src} fps={gate.shaderFps} paused={gate.degraded && gate.videoPaused} uniforms={pack.uniforms ?? {}} />;
    }
    if (pack.kind === "web") {
      const htmlKey = Object.keys(pack.resources).find((k) => /\.html?$/.test(k)) ?? "";
      const html = htmlKey ? dataUrlToText(pack.resources[htmlKey] ?? "") ?? "" : "";
      return <iframe className="wp-studio-web-frame" srcDoc={html} sandbox="" title="vwp web preview" />;
    }
    const params = pack.manifest.params ?? {};
    return <GenerativePreview motion={params.motion ?? 0.5} parallax={params.parallax ?? 0.3} paused={gate.generativePaused} />;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pack, gate.shaderFps, gate.generativePaused, gate.videoPaused]);

  const doApply = (mode: string, customBg: Record<string, unknown>): void => {
    requestApply({ wallpaperMode: mode, customBg });
    setToast(t("applied"));
  };

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(""), 4000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  return (
    <div className="wp-studio-root" role="dialog" aria-label={t("title")} tabIndex={-1}
      onKeyDown={(e) => {
        if (e.key === "Escape") dispatchClose("wallpaper-studio");
      }}>
      <header className="wp-studio-header">
        <h2>{t("title")}</h2>
        <div className="wp-studio-gate" title={gate.reasons.join(", ") || t("gateNone")}>
          <span className="wp-studio-gate-label">{t("perfGate")}</span>
          {gate.reasons.includes("typing") && <em>{t("gateTyping")}</em>}
          {gate.reasons.includes("fullscreen") && <em>{t("gateFullscreen")}</em>}
          {gate.reasons.includes("battery") && <em>{t("gateBattery")}</em>}
          {gate.reasons.length === 0 && <em className="ok">{t("gateNone")}</em>}
        </div>
        <button className="wp-studio-close" onClick={() => dispatchClose("wallpaper-studio")}>✕</button>
      </header>

      <nav className="wp-studio-tabs">
        {(["video", "shader", "generative", "web", "pack"] as TabId[]).map((id) => (
          <button key={id} className={tab === id ? "on" : ""} onClick={() => setTab(id)}>
            {t(`tab${id.charAt(0).toUpperCase()}${id.slice(1)}` as never)}
          </button>
        ))}
      </nav>

      <div className="wp-studio-body">
        {tab === "video" && (
          <section className="wp-studio-pane">
            <div className="wp-studio-toolbar">
              <button onClick={() => void pickVideo(true)}>{t("pickFile")}</button>
              <button disabled={!videoPath} onClick={() => doApply("video", { videoPath })}>
                {t("apply")}
              </button>
            </div>
            <p className="wp-studio-note">
              {videoBrowserOnly ? t("videoPreviewOnly") : videoPath || t("videoNoFile")}
            </p>
            <div className="wp-studio-preview">
              {videoUrl ? (
                <video ref={videoRef} src={videoUrl} autoPlay loop muted playsInline />
              ) : (
                <span className="wp-studio-empty">{t("preview")}</span>
              )}
            </div>
          </section>
        )}

        {tab === "shader" && (
          <section className="wp-studio-pane">
            <div className="wp-studio-toolbar">
              <button onClick={() => void pickShader(true)}>{t("pickFile")}</button>
              <button disabled={shaderBrowserOnly || !shaderPath} onClick={() => doApply("shader", { shaderPath })}>
                {t("apply")}
              </button>
            </div>
            <p className="wp-studio-note">{shaderBrowserOnly ? t("shaderApplyNeedsPath") : t("shaderPickHint")}</p>
            <div className="wp-studio-preview">
              {shaderSource ? (
                <ShaderPreview source={shaderSource} fps={gate.shaderFps} paused={gate.generativePaused} uniforms={uniformValues} />
              ) : (
                <span className="wp-studio-empty">{t("shaderNoFile")}</span>
              )}
            </div>
            {uniformList.length > 0 && (
              <div className="wp-studio-uniforms">
                <h4>{t("shaderUniforms")}</h4>
                {uniformList.map((u) => (
                  <label key={u.name} className="wp-studio-slider">
                    <span>{u.name}</span>
                    <input
                      type="range"
                      min={u.min}
                      max={u.max}
                      step={(u.max - u.min) / 100}
                      value={uniformValues[u.name] ?? u.value}
                      onChange={(e) =>
                        setUniformValues((prev) => ({ ...prev, [u.name]: Number(e.target.value) }))
                      }
                    />
                    <code>{(uniformValues[u.name] ?? u.value).toFixed(2)}</code>
                  </label>
                ))}
              </div>
            )}
          </section>
        )}

        {tab === "generative" && (
          <section className="wp-studio-pane">
            <div className="wp-studio-cards">
              {GENERATIVE_PRESETS.map((p) => (
                <button
                  key={p.id}
                  className={`wp-studio-card ${genPreset.id === p.id ? "on" : ""}`}
                  onClick={() => {
                    setGenPreset(p);
                    setGenMotion(p.motion);
                    setGenParallax(p.parallax);
                  }}
                >
                  <strong>{p.name}</strong>
                  <span>{p.desc}</span>
                </button>
              ))}
            </div>
            <div className="wp-studio-uniforms">
              <label className="wp-studio-slider">
                <span>{t("genFlow")}</span>
                <input type="range" min={0} max={1} step={0.01} value={genMotion}
                  onChange={(e) => setGenMotion(Number(e.target.value))} />
                <code>{genMotion.toFixed(2)}</code>
              </label>
              <label className="wp-studio-slider">
                <span>{t("genParallax")}</span>
                <input type="range" min={0} max={1} step={0.01} value={genParallax}
                  onChange={(e) => setGenParallax(Number(e.target.value))} />
                <code>{genParallax.toFixed(2)}</code>
              </label>
            </div>
            <div className="wp-studio-toolbar">
              <button onClick={() => doApply("gravity", { dynamicStrength: genMotion, parallaxStrength: genParallax })}>
                {t("apply")}
              </button>
            </div>
            <p className="wp-studio-note">{t("genNote")}</p>
            <div className="wp-studio-preview">
              <GenerativePreview motion={genMotion} parallax={genParallax} paused={gate.generativePaused} />
            </div>
          </section>
        )}

        {tab === "web" && (
          <section className="wp-studio-pane">
            <div className="wp-studio-toolbar">
              <button onClick={() => void pickWeb()}>{t("pickFile")}</button>
              <button disabled={!webPath} onClick={() => doApply("web", { htmlPath: webPath })}>
                {t("apply")}
              </button>
            </div>
            <p className="wp-studio-note">{t("webPath")}: {curHtmlPath || "—"}</p>
            <p className="wp-studio-note warn">{t("webNetline")}</p>
            {webRejected && <p className="wp-studio-note err">{t("webRejected")}</p>}
            <div className="wp-studio-preview">
              {webPath ? (
                <iframe className="wp-studio-web-frame" src={toAssetUrl(webPath)} sandbox="" title="web wallpaper preview" />
              ) : (
                <span className="wp-studio-empty">{t("preview")}</span>
              )}
            </div>
          </section>
        )}

        {tab === "pack" && (
          <section className="wp-studio-pane">
            <div className="wp-studio-toolbar">
              <label className="wp-studio-file">
                {t("packImport")}
                <input type="file" accept=".vwp,application/json" style={{ display: "none" }}
                  onChange={(e) => {
                    const f = e.target.files?.[0];
                    if (f) importPack(f);
                    e.currentTarget.value = "";
                  }} />
              </label>
              <button onClick={() => void exportPack("shader")}>{t("packExport")} · shader</button>
              <button onClick={() => void exportPack("generative")}>{t("packExport")} · generative</button>
              <button onClick={() => void exportPack("web")}>{t("packExport")} · web</button>
            </div>
            {packErrors.length > 0 && (
              <div className="wp-studio-note err">
                {t("packErr")}
                <ul>
                  {packErrors.map((c, i) => (
                    <li key={`${c}-${i}`}>{errText(t, c)}</li>
                  ))}
                </ul>
              </div>
            )}
            {pack && (
              <>
                <p className="wp-studio-note ok">
                  {t("packOk")} — {pack.kind} · {pack.manifest.name}
                </p>
                <p className="wp-studio-note">{t("packImportNoApply")}</p>
                <h4>{t("packImportPreview")}</h4>
                <div className="wp-studio-preview">{packPreview}</div>
              </>
            )}
          </section>
        )}
      </div>

      <footer className="wp-studio-footer">
        <button onClick={() => window.dispatchEvent(new CustomEvent("ai04:open-feature", { detail: { feature: "scene-settings" } }))}>
          {t("sceneEntry")}
        </button>
      </footer>

      {toast && <div className="wp-studio-toast">{toast}</div>}
    </div>
  );
}

/* 模块加载即监听（主控集成阶段动态 import 本模块即激活；不改 DesktopShell）。 */
if (typeof window !== "undefined") {
  mountOnEvent(window, "wallpaper-studio", async () => ({ Overlay: Workshop }));
  installCloseHandler(window, "wallpaper-studio");
}