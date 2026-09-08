import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Copy } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import {
  HISTORY_CAP,
  addColorHistory,
  clampZoom,
  gridLines,
  hexToRgb,
  loadHistory,
  pickPixel,
  rgbToHex,
  rgbToHsl,
  saveHistory,
  sourceRect,
} from "./magnifierCore";
import "../../styles/ai08-magnifier.css";

/**
 * Z-25 放大镜与取色器（VWM 虚拟窗口应用）：
 * - 倍率 2×–16×（按钮 / 滑块 / 键盘 +-），imageSmoothingEnabled=false 整数放大
 * - 跟随鼠标：100ms 轮询 cursorPos 取物理坐标 + 定期 snapshotCapture 抓屏，
 *   在 canvas 上放大绘制光标周围区域；≥8× 叠加像素网格线
 * - 「冻结」停帧（停跟随 + 停抓屏），便于稳定取色
 * - 取色器：点击画布像素 → HEX/RGB/HSL 三格式复制按钮；16 色历史条
 * - 无屏幕数据（非 Tauri / 抓屏失败）如实占位提示，不伪造画面
 * 红线：不做截图标注/保存；不修改任何系统设置。
 */

/** 光标轮询间隔（约 100ms，规格要求）。 */
const CURSOR_MS = 100;
/** 抓屏间隔（BMP 全虚拟屏开销较大，500ms 折中跟手感与开销）。 */
const SNAP_MS = 500;

export function MagnifierApp(_props: { winId: string }): React.ReactElement {
  const { t } = useI18n();
  const [zoom, setZoom] = useState(4);
  const [frozen, setFrozen] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);
  const [history, setHistory] = useState<string[]>([]);
  const [noData, setNoData] = useState(true);
  const [loadErr, setLoadErr] = useState<string | null>(null);
  const [posLabel, setPosLabel] = useState("–, –");
  const [copiedFmt, setCopiedFmt] = useState<"hex" | "rgb" | "hsl" | null>(null);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  /** 离屏 1:1 快照画布：取色直接读源像素（避开网格线/缩放干扰）。 */
  const offRef = useRef<HTMLCanvasElement | null>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);
  const posRef = useRef({ x: 0, y: 0 });
  const zoomRef = useRef(4);
  const frozenRef = useRef(false);
  const hasFrameRef = useRef(false);

  // ---------- 绘制（全部经 ref 读最新值，回调零依赖） ----------

  const draw = useCallback((): void => {
    const cv = canvasRef.current;
    if (!cv || cv.width === 0) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    const img = imgRef.current;
    ctx.imageSmoothingEnabled = false; // 像素风整数放大
    ctx.clearRect(0, 0, cv.width, cv.height);
    if (!img) return;
    const z = zoomRef.current;
    const sr = sourceRect(posRef.current, z, cv.width, cv.height, img.naturalWidth, img.naturalHeight);
    ctx.drawImage(img, sr.sx, sr.sy, sr.sw, sr.sh, 0, 0, sr.sw * z, sr.sh * z);
    // ≥8× 叠加像素网格线（0.5 偏移保证 1px 锐利）
    const gl = gridLines(z, cv.width, cv.height);
    if (gl.vxs.length > 0 || gl.hys.length > 0) {
      ctx.strokeStyle = "rgba(255, 255, 255, 0.25)";
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (const x of gl.vxs) {
        ctx.moveTo(x + 0.5, 0);
        ctx.lineTo(x + 0.5, cv.height);
      }
      for (const y of gl.hys) {
        ctx.moveTo(0, y + 0.5);
        ctx.lineTo(cv.width, y + 0.5);
      }
      ctx.stroke();
    }
  }, []);

  useEffect(() => {
    zoomRef.current = zoom;
    draw();
  }, [zoom, draw]);

  useEffect(() => {
    frozenRef.current = frozen;
  }, [frozen]);

  // 画布尺寸跟随舞台（ResizeObserver；容器始终渲染，占位提示浮在上层）
  useEffect(() => {
    const stage = stageRef.current;
    const cv = canvasRef.current;
    if (!stage || !cv) return;
    const fit = (): void => {
      const w = Math.max(1, stage.clientWidth);
      const h = Math.max(1, stage.clientHeight);
      if (cv.width !== w || cv.height !== h) {
        cv.width = w;
        cv.height = h;
        draw();
      }
    };
    fit();
    const ro = new ResizeObserver(fit);
    ro.observe(stage);
    return () => ro.disconnect();
  }, [draw]);

  // ---------- 抓屏（BMP 字节 → Image，参考 SnapshotApp） ----------

  const grab = useCallback(async (): Promise<void> => {
    try {
      const bytes = await ipc.snapshotCapture();
      const blob = new Blob([new Uint8Array(bytes)], { type: "image/bmp" });
      const url = URL.createObjectURL(blob);
      const image = new Image();
      await new Promise<void>((res, rej) => {
        image.onload = () => res();
        image.onerror = () => rej(new Error("decode"));
        image.src = url;
      });
      URL.revokeObjectURL(url);
      let off = offRef.current;
      if (!off) {
        off = document.createElement("canvas");
        offRef.current = off;
      }
      off.width = image.naturalWidth;
      off.height = image.naturalHeight;
      off.getContext("2d")?.drawImage(image, 0, 0);
      imgRef.current = image;
      hasFrameRef.current = true;
      setNoData(false);
      setLoadErr(null);
      draw();
    } catch (e) {
      // 首帧失败如实提示；已有帧则保持上一帧（不闪断）
      if (!hasFrameRef.current) setLoadErr(errMessage(e).message);
    }
  }, [draw]);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let alive = true;
    const tick = (): void => {
      if (!alive || frozenRef.current) return;
      void grab();
    };
    tick();
    const iv = window.setInterval(tick, SNAP_MS);
    return () => {
      alive = false;
      window.clearInterval(iv);
    };
  }, [grab]);

  // ---------- 光标跟随（100ms 轮询物理坐标） ----------

  useEffect(() => {
    if (!isTauriRuntime()) return;
    const iv = window.setInterval(() => {
      void ipc
        .cursorPos()
        .then((p) => {
          if (frozenRef.current) return; // 冻结：停帧停跟随，保持取色映射稳定
          posRef.current = p;
          setPosLabel(`${p.x}, ${p.y}`);
          draw();
        })
        .catch(() => {});
    }, CURSOR_MS);
    return () => window.clearInterval(iv);
  }, [draw]);

  // ---------- 历史 ----------

  useEffect(() => {
    setHistory(loadHistory());
  }, []);

  // ---------- 取色 ----------

  const onPick = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>): void => {
      const cv = canvasRef.current;
      const off = offRef.current;
      const img = imgRef.current;
      if (!cv || !off || !img) return;
      const r = cv.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) return;
      const cx = Math.floor(((e.clientX - r.left) / r.width) * cv.width);
      const cy = Math.floor(((e.clientY - r.top) / r.height) * cv.height);
      const z = zoomRef.current;
      const sr = sourceRect(posRef.current, z, cv.width, cv.height, img.naturalWidth, img.naturalHeight);
      const px = sr.sx + Math.floor(cx / z);
      const py = sr.sy + Math.floor(cy / z);
      if (px < 0 || py < 0 || px >= off.width || py >= off.height) return;
      const ctx = off.getContext("2d");
      if (!ctx) return;
      const rgb = pickPixel(ctx.getImageData(px, py, 1, 1).data, 1, 0, 0);
      if (!rgb) return;
      const hex = rgbToHex(rgb);
      setPicked(hex);
      const next = addColorHistory(history, hex);
      setHistory(next);
      saveHistory(next);
    },
    [history],
  );

  const fmts = useMemo(() => {
    if (!picked) return null;
    const rgb = hexToRgb(picked);
    if (!rgb) return null;
    const hsl = rgbToHsl(rgb);
    return {
      hex: picked.toUpperCase(),
      rgb: `rgb(${rgb.r}, ${rgb.g}, ${rgb.b})`,
      hsl: `hsl(${hsl.h}, ${hsl.s}%, ${hsl.l}%)`,
    };
  }, [picked]);

  const copy = (fmt: "hex" | "rgb" | "hsl"): void => {
    if (!fmts) return;
    void navigator.clipboard
      .writeText(fmts[fmt])
      .then(() => {
        setCopiedFmt(fmt);
        window.setTimeout(() => setCopiedFmt(null), 1200);
      })
      .catch(() => pushToast("error", t("mgTitle"), t("mgCopyFail")));
  };

  // ---------- 倍率 / 冻结 ----------

  const onKey = (e: React.KeyboardEvent): void => {
    if (e.key === "+" || e.key === "=") {
      e.preventDefault();
      setZoom((z) => clampZoom(z + 1));
    } else if (e.key === "-" || e.key === "_") {
      e.preventDefault();
      setZoom((z) => clampZoom(z - 1));
    }
  };

  const toggleFreeze = (): void => {
    const next = !frozen;
    setFrozen(next);
    frozenRef.current = next;
    if (!next && isTauriRuntime()) void grab(); // 解冻立即抓新帧
  };

  const fmtBtn = (fmt: "hex" | "rgb" | "hsl", label: string): React.ReactElement => (
    <span className="mg-fmt">
      <code>{fmts ? fmts[fmt] : ""}</code>
      <button type="button" className="btn ghost tiny" onClick={() => copy(fmt)} aria-label={`${label} ${t("mgCopy")}`}>
        <Copy size={11} /> {copiedFmt === fmt ? t("mgCopied") : label}
      </button>
    </span>
  );

  const showPlaceholder = !isTauriRuntime() || noData;

  return (
    <div className="mg-app" tabIndex={0} onKeyDown={onKey} aria-label={t("mgTitle")}>
      <div className="mg-toolbar">
        <button type="button" className="btn ghost tiny" onClick={() => setZoom((z) => clampZoom(z - 1))} aria-label={t("mgZoomOut")}>
          −
        </button>
        <span className="mg-zoom-label dim small">
          {zoom}×{zoom >= 8 ? ` · ${t("mgGridOn")}` : ""}
        </span>
        <button type="button" className="btn ghost tiny" onClick={() => setZoom((z) => clampZoom(z + 1))} aria-label={t("mgZoomIn")}>
          ＋
        </button>
        <input
          className="mg-zoom-range"
          type="range"
          min={2}
          max={16}
          step={1}
          value={zoom}
          onChange={(e) => setZoom(clampZoom(Number(e.target.value)))}
          aria-label={t("mgZoom")}
        />
        <button
          type="button"
          className={`btn ghost tiny${frozen ? " active" : ""}`}
          aria-pressed={frozen}
          onClick={toggleFreeze}
        >
          {frozen ? t("mgUnfreeze") : t("mgFreeze")}
        </button>
        <span className="flex-1" />
        <span className="mg-pos dim small" aria-label={t("mgPosLabel")}>
          {posLabel}
        </span>
      </div>

      <div className="mg-stage" ref={stageRef}>
        <canvas ref={canvasRef} className="mg-canvas" onClick={onPick} aria-label={t("mgPickHint")} />
        {showPlaceholder && (
          <div className="mg-empty" role="status">
            <p className="dim small">
              {!isTauriRuntime() ? t("mgNoTauri") : loadErr ? t("mgLoadFail", { msg: loadErr }) : t("mgLoading")}
            </p>
          </div>
        )}
      </div>

      <div className="mg-color-row">
        <span className="mg-swatch" style={{ background: picked ?? "transparent" }} aria-hidden />
        {fmts ? (
          <div className="mg-formats">
            {fmtBtn("hex", "HEX")}
            {fmtBtn("rgb", "RGB")}
            {fmtBtn("hsl", "HSL")}
          </div>
        ) : (
          <span className="dim small">{t("mgPickHint")}</span>
        )}
      </div>

      <div className="mg-history" aria-label={t("mgHistory")}>
        {history.length === 0 && <span className="dim small">{t("mgHistoryEmpty")}</span>}
        {history.map((h) => (
          <button
            key={h}
            type="button"
            className={`mg-hist-item${h === picked ? " active" : ""}`}
            style={{ background: h }}
            title={h}
            aria-label={h}
            onClick={() => setPicked(h)}
          />
        ))}
        {history.length > 0 && (
          <span className="dim small mg-hist-count">
            {history.length}/{HISTORY_CAP}
          </span>
        )}
      </div>
    </div>
  );
}
