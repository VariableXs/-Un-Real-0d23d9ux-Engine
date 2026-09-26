import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * F-2.4 截图工具（VWM 虚拟窗口应用）：
 * - 全屏（虚拟屏）/ 延时 3s / 窗口（= Variable 自身窗口）/ 区域（先全屏后拖框裁剪）
 * - 标注：画笔 / 箭头 / 马赛克 / 文字；保存 = 下载 PNG 或复制到剪贴板
 * - S-1 防截屏联动：防截屏开启时，如实声明只允许截 Variable 自身内容
 *   （桌面窗口范围），「全屏」自动降级为「窗口」并提示。
 */

type ShotMode = "full" | "window" | "region" | "delay";
type Tool = "pen" | "arrow" | "mosaic" | "text" | "none" | "picker";

interface Stroke {
  tool: Tool;
  x0: number; y0: number; x1: number; y1: number;
  text?: string;
}

export function SnapshotApp(): React.ReactElement {
  const { t } = useI18n();
  const [img, setImg] = useState<HTMLImageElement | null>(null);
  const [shield, setShield] = useState(false);
  const [busy, setBusy] = useState(false);
  const [countdown, setCountdown] = useState(0);
  const [tool, setTool] = useState<Tool>("pen");
  /** F098 取色结果（RGB+HEX 双格式——与实际像素对拍）。 */
  const [picked, setPicked] = useState<{ rgb: string; hex: string } | null>(null);
  const [strokes, setStrokes] = useState<Stroke[]>([]);
  const [textDraft, setTextDraft] = useState("");
  const [region, setRegion] = useState<{ x: number; y: number; w: number; h: number } | null>(null);
  // Variable 相册（截图共享目录）：挂载时取一次，用于空态提示与「打开文件夹」
  const [albumDir, setAlbumDir] = useState("");
  const dragRef = useRef<{ x0: number; y0: number } | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const displayRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void ipc.shieldGet().then(setShield).catch(() => setShield(false));
    void ipc.shotDir().then(setAlbumDir).catch(() => setAlbumDir(""));
  }, []);

  const capture = useCallback(
    async (mode: ShotMode) => {
      if (shield && mode === "full") {
        pushToast("info", t("shotTitle"), t("shotShieldNotice"));
        return capture("window");
      }
      setBusy(true);
      try {
        if (mode === "delay") {
          for (let c = 3; c > 0; c--) {
            setCountdown(c);
            await new Promise((r) => window.setTimeout(r, 1000));
          }
          setCountdown(0);
        }
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
        setImg(image);
        setStrokes([]);
        setRegion(null);
        pushToast("success", t("shotTitle"), t("shotCaptured"));
      } catch (e) {
        pushToast("error", t("shotTitle"), e instanceof Error ? e.message : t("shotFail"));
      } finally {
        setBusy(false);
        setCountdown(0);
      }
    },
    [shield, t],
  );

  /**
   * 统一绘制管线：标注坐标一律记在「抓取图像坐标系」里，绘制时按 scale 换算到
   * 画布设备像素 —— 于是同一份 paint 同时服务预览（可能缩放）与导出（1:1 原图），
   * 预览里的笔迹位置与导出图逐像素一致（此前预览画布缩放时笔迹会整体偏移）。
   */
  const paint = useCallback(
    (
      ctx: CanvasRenderingContext2D,
      source: HTMLImageElement,
      crop: { x: number; y: number; w: number; h: number },
      list: Stroke[],
      scale: number,
    ) => {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
      if (crop.w <= 0 || crop.h <= 0) return;
      ctx.drawImage(source, crop.x, crop.y, crop.w, crop.h, 0, 0, ctx.canvas.width, ctx.canvas.height);
      const X = (v: number): number => (v - crop.x) * scale;
      const Y = (v: number): number => (v - crop.y) * scale;
      for (const s of list) {
        const x = X(s.x1);
        const y = Y(s.y1);
        const x0 = X(s.x0);
        const y0 = Y(s.y0);
        ctx.lineWidth = Math.max(1, 2 * scale);
        if (s.tool === "pen") {
          ctx.strokeStyle = "#ff5555";
          ctx.beginPath();
          ctx.moveTo(x0, y0);
          ctx.lineTo(x, y);
          ctx.stroke();
        } else if (s.tool === "arrow") {
          ctx.strokeStyle = "#ff5555";
          const ang = Math.atan2(y - y0, x - x0);
          const head = Math.max(4, 10 * scale);
          ctx.beginPath();
          ctx.moveTo(x0, y0);
          ctx.lineTo(x, y);
          ctx.stroke();
          ctx.beginPath();
          ctx.moveTo(x, y);
          ctx.lineTo(x - head * Math.cos(ang - 0.5), y - head * Math.sin(ang - 0.5));
          ctx.moveTo(x, y);
          ctx.lineTo(x - head * Math.cos(ang + 0.5), y - head * Math.sin(ang + 0.5));
          ctx.stroke();
        } else if (s.tool === "mosaic") {
          // 取样块（图像坐标 14px）→ 单色块覆盖。原实现用 putImageData 落设备像素，
          // 缩放预览下块位置会错位；改用设备坐标 fillRect（scale=1 时像素等价）。
          const size = 14;
          const sx = Math.max(0, Math.min(s.x0, source.naturalWidth - size));
          const sy = Math.max(0, Math.min(s.y0, source.naturalHeight - size));
          const tmp = document.createElement("canvas");
          tmp.width = 1;
          tmp.height = 1;
          const tctx = tmp.getContext("2d");
          if (!tctx) continue;
          tctx.drawImage(source, sx, sy, size, size, 0, 0, 1, 1);
          const d = tctx.getImageData(0, 0, 1, 1).data;
          ctx.fillStyle = `rgb(${d[0]}, ${d[1]}, ${d[2]})`;
          ctx.fillRect(X(sx), Y(sy), size * scale, size * scale);
        } else if (s.tool === "text") {
          ctx.fillStyle = "#ff5555";
          ctx.font = `${Math.max(10, Math.round(16 * scale))}px sans-serif`;
          ctx.fillText(s.text ?? "", x, y);
        }
      }
    },
    [],
  );

  const redraw = useCallback(() => {
    const cv = canvasRef.current;
    if (!cv) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    if (!img) {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, cv.width, cv.height);
      return;
    }
    const crop = region ?? { x: 0, y: 0, w: img.naturalWidth, h: img.naturalHeight };
    paint(ctx, img, crop, strokes, crop.w > 0 ? cv.width / crop.w : 1);
  }, [img, paint, strokes, region]);

  /**
   * 导出画布：与预览无关的 1:1 原图分辨率（区域截图即所选区域尺寸）。
   * 预览画布为适配窗口常被缩小，直接 toDataURL 会丢分辨率 —— 保存到相册的
   * 必须是全尺寸原图。
   */
  const exportCanvas = (): HTMLCanvasElement | null => {
    if (!img) return null;
    const crop = region ?? { x: 0, y: 0, w: img.naturalWidth, h: img.naturalHeight };
    if (crop.w <= 0 || crop.h <= 0) return null;
    const out = document.createElement("canvas");
    out.width = Math.round(crop.w);
    out.height = Math.round(crop.h);
    const ctx = out.getContext("2d");
    if (!ctx) return null;
    paint(ctx, img, crop, strokes, 1);
    return out;
  };

  useEffect(() => {
    const cv = canvasRef.current;
    if (!cv || !img) return;
    const maxW = displayRef.current?.clientWidth ?? 640;
    const crop = region ?? { w: img.naturalWidth, h: img.naturalHeight };
    const scale = Math.min(1, maxW / crop.w);
    cv.width = Math.max(1, Math.round(crop.w * scale));
    cv.height = Math.max(1, Math.round(crop.h * scale));
    redraw();
  }, [img, redraw, region, strokes]);

  const toCanvasPos = (e: React.MouseEvent): { x: number; y: number } => {
    const cv = canvasRef.current!;
    const r = cv.getBoundingClientRect();
    const scaleX = img ? (region?.w ?? img.naturalWidth) / r.width : 1;
    const scaleY = img ? (region?.h ?? img.naturalHeight) / r.height : 1;
    return { x: (e.clientX - r.left) * scaleX, y: (e.clientY - r.top) * scaleY };
  };

  const onDown = (e: React.MouseEvent) => {
    if (!img) return;
    const p = toCanvasPos(e);
    dragRef.current = { x0: p.x, y0: p.y };
    // F098 取色：读原始像素（对拍口径——色值与实际像素一致），不产生标注笔迹。
    if (tool === "picker") {
      const tmp = document.createElement("canvas");
      tmp.width = 1;
      tmp.height = 1;
      const tctx = tmp.getContext("2d");
      if (tctx) {
        tctx.drawImage(img, Math.floor(p.x), Math.floor(p.y), 1, 1, 0, 0, 1, 1);
        const d = tctx.getImageData(0, 0, 1, 1).data;
        const hex = `#${[d[0], d[1], d[2]].map((v) => (v ?? 0).toString(16).padStart(2, "0")).join("")}`;
        setPicked({ rgb: `rgb(${d[0]}, ${d[1]}, ${d[2]})`, hex });
        void navigator.clipboard?.writeText(hex).then(
          () => pushToast("success", "已取色并复制", `${hex}（${d[0]}, ${d[1]}, ${d[2]}）——与实际像素一致`),
          () => {},
        );
      }
      return;
    }
    if (tool === "text") return;
    if (tool === "none") return; // 区域选择：onMove 中生成 region
    setStrokes((ss) => [...ss, { tool, x0: p.x, y0: p.y, x1: p.x, y1: p.y }]);
  };

  const onMove = (e: React.MouseEvent) => {
    if (!dragRef.current || !img) return;
    const p = toCanvasPos(e);
    if (tool === "none") {
      // 区域选择拖框（对抓取图像做裁剪）
      const { x0, y0 } = dragRef.current;
      setRegion({
        x: Math.min(x0, p.x),
        y: Math.min(y0, p.y),
        w: Math.abs(p.x - x0),
        h: Math.abs(p.y - y0),
      });
      return;
    }
    if (tool === "text") return;
    setStrokes((ss) => {
      const last = ss[ss.length - 1];
      if (last && last.tool === tool) {
        return [...ss.slice(0, -1), { ...last, x1: p.x, y1: p.y }];
      }
      return ss;
    });
  };

  const onUp = () => {
    dragRef.current = null;
  };

  const commitText = () => {
    if (!textDraft.trim() || !img) return;
    setStrokes((ss) => [...ss, { tool: "text", x0: 20, y0: 30, x1: 20, y1: 30, text: textDraft }]);
    setTextDraft("");
  };

  const copyToClipboard = async () => {
    const cv = exportCanvas();
    if (!cv) return;
    try {
      const blob = await new Promise<Blob | null>((res) => cv.toBlob(res, "image/png"));
      if (!blob) throw new Error("blob");
      await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
      pushToast("success", t("shotTitle"), t("shotCopied"));
    } catch {
      pushToast("error", t("shotTitle"), t("shotCopyFail"));
    }
  };

  /** 保存到 Variable 相册（共享目录）——全系统同一份文件，而非浏览器下载目录。 */
  const saveToAlbum = async () => {
    const cv = exportCanvas();
    if (!cv) return;
    try {
      const path = await ipc.shotSave(cv.toDataURL("image/png"));
      setAlbumDir(path.replace(/[\\/][^\\/]+$/, ""));
      pushToast("success", t("shotTitle"), t("shotSavedTo", { path }));
    } catch (e) {
      pushToast("error", t("shotTitle"), errMessage(e).message);
    }
  };

  /** 打开相册所在文件夹（复用系统「在文件夹中显示」）。 */
  const openAlbum = async () => {
    try {
      await ipc.revealPath(albumDir || (await ipc.shotDir()));
    } catch (e) {
      pushToast("error", t("shotTitle"), errMessage(e).message);
    }
  };

  const modeBtn = (id: ShotMode, label: string) => (
    <button type="button" className="btn ghost tiny" disabled={busy || (shield && id === "full")} onClick={() => void capture(id)}>
      {label}
    </button>
  );

  return (
    <div className="shot-app">
      <div className="notes-toolbar">
        {modeBtn("window", t("shotModeWindow"))}
        {modeBtn("full", t("shotModeFull"))}
        {modeBtn("delay", t("shotModeDelay"))}
        {modeBtn("region", t("shotModeRegion"))}
        <span className="flex-1" />
        <button type="button" className="btn ghost tiny" onClick={() => void openAlbum()}>{t("shotAlbumOpen")}</button>
        <button type="button" className="btn ghost tiny" disabled={!img} onClick={() => void copyToClipboard()}>{t("shotCopy")}</button>
        <button type="button" className="btn ghost tiny" disabled={!img} onClick={() => void saveToAlbum()}>{t("shotSave")}</button>
      </div>
      {shield && <p className="dim small" style={{ margin: "4px 8px" }}>{t("shotShieldNotice")}</p>}
      {albumDir && <p className="dim small" style={{ margin: "4px 8px" }}>{t("shotAlbumHint", { dir: albumDir })}</p>}
      {busy && countdown > 0 && <p className="cal-bigtime">{countdown}</p>}
      {img && (
        <div className="shot-tools">
          {(["pen", "arrow", "mosaic", "text", "picker", "none"] as Tool[]).map((tl) => (
            <button key={tl} type="button" className={`btn ghost tiny${tool === tl ? " active" : ""}`} aria-pressed={tool === tl} onClick={() => { setTool(tl); if (tl === "none") setRegion(null); }}>
              {t(`shotTool_${tl}`)}
            </button>
          ))}
          {/* F098 标注撤销：逐步回退笔迹（20 步撤销判据的交互面，栈语义同内核面）。 */}
          <button type="button" className="btn ghost tiny" disabled={strokes.length === 0} onClick={() => setStrokes((ss) => ss.slice(0, -1))} aria-label="撤销上一笔标注">
            撤销
          </button>
          {picked && (
            <span className="row gap4" role="status">
              <span className="notes-swatch" style={{ background: picked.hex, width: 14, height: 14, display: "inline-block", borderRadius: 3 }} aria-hidden />
              <span className="dim small">{picked.hex} · {picked.rgb}</span>
            </span>
          )}
          {tool === "text" && (
            <span className="row gap4">
              <input className="text-input tiny" style={{ width: 140 }} value={textDraft} onChange={(e) => setTextDraft(e.target.value)} placeholder={t("shotTextHint")} />
              <button type="button" className="btn ghost tiny" onClick={commitText}>{t("shotTextAdd")}</button>
            </span>
          )}
        </div>
      )}
      <div className="shot-canvas-wrap" ref={displayRef}>
        {!img ? (
          <p className="dim small" style={{ padding: 16 }}>{t("shotEmpty")}</p>
        ) : (
          <canvas
            ref={canvasRef}
            className={`shot-canvas${tool !== "none" ? " shot-editing" : ""}`}
            onMouseDown={onDown}
            onMouseMove={onMove}
            onMouseUp={onUp}
            onMouseLeave={onUp}
            aria-label={t("shotTitle")}
          />
        )}
      </div>
    </div>
  );
}
