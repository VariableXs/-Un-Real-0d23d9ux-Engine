import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * F-2.4 截图工具（VWM 虚拟窗口应用）：
 * - 全屏（虚拟屏）/ 延时 3s / 窗口（= Variable 自身窗口）/ 区域（先全屏后拖框裁剪）
 * - 标注：画笔 / 箭头 / 马赛克 / 文字；保存 = 下载 PNG 或复制到剪贴板
 * - S-1 防截屏联动：防截屏开启时，如实声明只允许截 Variable 自身内容
 *   （桌面窗口范围），「全屏」自动降级为「窗口」并提示。
 */

type ShotMode = "full" | "window" | "region" | "delay";
type Tool = "pen" | "arrow" | "mosaic" | "text" | "none";

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
  const [strokes, setStrokes] = useState<Stroke[]>([]);
  const [textDraft, setTextDraft] = useState("");
  const [region, setRegion] = useState<{ x: number; y: number; w: number; h: number } | null>(null);
  const dragRef = useRef<{ x0: number; y0: number } | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const displayRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    void ipc.shieldGet().then(setShield).catch(() => setShield(false));
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

  // 绘制（截图原图 + 标注 + 区域裁剪）
  const redraw = useCallback(() => {
    const cv = canvasRef.current;
    if (!cv) return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    ctx.clearRect(0, 0, cv.width, cv.height);
    if (img) {
      const crop = region ?? { x: 0, y: 0, w: img.naturalWidth, h: img.naturalHeight };
      ctx.drawImage(img, crop.x, crop.y, crop.w, crop.h, 0, 0, cv.width, cv.height);
      for (const s of strokes) {
        const x = s.x1 - crop.x;
        const y = s.y1 - crop.y;
        const x0 = s.x0 - crop.x;
        const y0 = s.y0 - crop.y;
        ctx.lineWidth = 2;
        if (s.tool === "pen") {
          ctx.strokeStyle = "#ff5555";
          ctx.beginPath();
          ctx.moveTo(x0, y0);
          ctx.lineTo(x, y);
          ctx.stroke();
        } else if (s.tool === "arrow") {
          ctx.strokeStyle = "#ff5555";
          const ang = Math.atan2(y - y0, x - x0);
          ctx.beginPath();
          ctx.moveTo(x0, y0);
          ctx.lineTo(x, y);
          ctx.stroke();
          ctx.beginPath();
          ctx.moveTo(x, y);
          ctx.lineTo(x - 10 * Math.cos(ang - 0.5), y - 10 * Math.sin(ang - 0.5));
          ctx.moveTo(x, y);
          ctx.lineTo(x - 10 * Math.cos(ang + 0.5), y - 10 * Math.sin(ang + 0.5));
          ctx.stroke();
        } else if (s.tool === "mosaic") {
          const size = 14;
          const sx = Math.max(0, Math.min(x0, img.naturalWidth - size));
          const sy = Math.max(0, Math.min(y0, img.naturalHeight - size));
          const patch = ctx.createImageData(size, size);
          // 从原图区域取块后按块放大 → 马赛克
          const tmp = document.createElement("canvas");
          tmp.width = size;
          tmp.height = size;
          const tctx = tmp.getContext("2d");
          if (tctx) {
            tctx.drawImage(img, sx, sy, size, size, 0, 0, 1, 1);
            const d = tctx.getImageData(0, 0, 1, 1).data;
            for (let i = 0; i < patch.data.length; i += 4) {
              patch.data[i] = d[0]!;
              patch.data[i + 1] = d[1]!;
              patch.data[i + 2] = d[2]!;
              patch.data[i + 3] = 255;
            }
            ctx.putImageData(patch, sx - crop.x, sy - crop.y);
          }
        } else if (s.tool === "text") {
          ctx.fillStyle = "#ff5555";
          ctx.font = "16px sans-serif";
          ctx.fillText(s.text ?? "", x, y);
        }
      }
    }
  }, [img, strokes, region]);

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

  const outputCanvas = (): HTMLCanvasElement | null => {
    if (!img) return null;
    const cv = canvasRef.current;
    return cv;
  };

  const copyToClipboard = async () => {
    const cv = outputCanvas();
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

  const download = () => {
    const cv = outputCanvas();
    if (!cv) return;
    const a = document.createElement("a");
    a.href = cv.toDataURL("image/png");
    a.download = `variable-shot-${Date.now()}.png`;
    a.click();
    pushToast("success", t("shotTitle"), t("shotSaved"));
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
        <button type="button" className="btn ghost tiny" disabled={!img} onClick={copyToClipboard}>{t("shotCopy")}</button>
        <button type="button" className="btn ghost tiny" disabled={!img} onClick={download}>{t("shotSave")}</button>
      </div>
      {shield && <p className="dim small" style={{ margin: "4px 8px" }}>{t("shotShieldNotice")}</p>}
      {busy && countdown > 0 && <p className="cal-bigtime">{countdown}</p>}
      {img && (
        <div className="shot-tools">
          {(["pen", "arrow", "mosaic", "text", "none"] as Tool[]).map((tl) => (
            <button key={tl} type="button" className={`btn ghost tiny${tool === tl ? " active" : ""}`} aria-pressed={tool === tl} onClick={() => { setTool(tl); if (tl === "none") setRegion(null); }}>
              {t(`shotTool_${tl}`)}
            </button>
          ))}
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
