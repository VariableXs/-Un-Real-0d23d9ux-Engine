import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "../../entries/runtime";
import { ipc } from "../../lib/ipc";

/**
 * 批次C-4：L3 画面捕获合成画布 + 输入转发。
 * - 后端（Windows.Graphics.Capture）经 `embed-frame` 事件推送 BGRA 帧（自适应帧率，
 *   静止 5fps / 活动 30fps）；本组件按 embedId 过滤 → Canvas 绘制。
 * - 收到帧 = 会话为 L3 → 激活指针/键盘转发：归一化坐标(0..1) → ipc.embedInput
 *   （后端 PostMessage 直注屏外真实窗口：不泄漏光标、不抢焦点）。
 * - 无帧 = L1/L2 直嵌会话 → 画布静默（透明、pointer-events none），零开销。
 * 帧尺寸变化时重建画布缓冲；键事件仅在画布收到过帧后转发。
 */

interface FramePayload {
  embedId: string;
  width: number;
  height: number;
  bgra: string;
}

export function L3CaptureView(props: { embedId: string }): React.ReactElement {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const bufRef = useRef<HTMLCanvasElement | null>(null);
  const [active, setActive] = useState(false);
  const lastFrameRef = useRef(0);
  const activeRef = useRef(false);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<FramePayload>("embed-frame", (ev) => {
      if (disposed || ev.payload.embedId !== props.embedId) return;
      const f = ev.payload;
      if (!f.width || !f.height || !f.bgra) return;
      let buf = bufRef.current;
      const w = Math.max(1, Math.floor(f.width ?? 0));
      const h = Math.max(1, Math.floor(f.height ?? 0));
      if (!buf || buf.width !== w || buf.height !== h) {
        buf = document.createElement("canvas");
        buf.width = w;
        buf.height = h;
        bufRef.current = buf;
      }
      const bctx = buf.getContext("2d");
      if (!bctx) return;
      const bin = atob(f.bgra);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      // BGRA → RGBA（putImageData 需要 RGBA 顺序）
      const img = new ImageData(w, h);
      const out = img.data;
      for (let i = 0; i < bytes.length; i += 4) {
        out[i] = bytes[i + 2] ?? 0;
        out[i + 1] = bytes[i + 1] ?? 0;
        out[i + 2] = bytes[i] ?? 0;
        out[i + 3] = bytes[i + 3] ?? 0;
      }
      bctx.putImageData(img, 0, 0);
      const cv = canvasRef.current;
      if (cv) {
        const ctx = cv.getContext("2d");
        if (ctx) ctx.drawImage(buf, 0, 0, cv.width, cv.height);
      }
      lastFrameRef.current = Date.now();
      if (!activeRef.current) {
        activeRef.current = true;
        setActive(true);
      }
    });
    void p
      .then((u) => {
        if (disposed) u();
        else un = u;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, [props.embedId]);

  const norm = (e: React.PointerEvent<HTMLCanvasElement>): [number, number] => {
    const r = e.currentTarget.getBoundingClientRect();
    const x = r.width > 0 ? (e.clientX - r.left) / r.width : 0;
    const y = r.height > 0 ? (e.clientY - r.top) / r.height : 0;
    return [Math.min(1, Math.max(0, x)), Math.min(1, Math.max(0, y))];
  };

  if (!active) {
    // 未确认 L3：静默占位（无帧时与 L1/L2 透明占位行为一致）
    return <div className="vwm-tp-l3-off" style={{ position: "absolute", inset: 0 }} />;
  }

  const buttonOf = (e: React.PointerEvent<HTMLCanvasElement>): string =>
    e.button === 2 ? "right" : e.button === 1 ? "middle" : "left";

  return (
    <canvas
      ref={canvasRef}
      tabIndex={0}
      style={{ position: "absolute", inset: 0, width: "100%", height: "100%", outline: "none" }}
      onPointerMove={(e) => {
        const [x, y] = norm(e);
        void ipc.embedInput(props.embedId, "move", x, y, activeRef.current ? buttonOf(e) : undefined);
      }}
      onPointerDown={(e) => {
        e.currentTarget.focus();
        const [x, y] = norm(e);
        void ipc.embedInput(props.embedId, "down", x, y, buttonOf(e));
      }}
      onPointerUp={(e) => {
        const [x, y] = norm(e);
        void ipc.embedInput(props.embedId, "up", x, y, buttonOf(e));
      }}
      onDoubleClick={(e) => {
        const r = e.currentTarget.getBoundingClientRect();
        const x = r.width > 0 ? (e.clientX - r.left) / r.width : 0;
        const y = r.height > 0 ? (e.clientY - r.top) / r.height : 0;
        void ipc.embedInput(props.embedId, "dbl", x, y, "left");
      }}
      onWheel={(e) => {
        e.preventDefault();
        const r = e.currentTarget.getBoundingClientRect();
        const x = r.width > 0 ? (e.clientX - r.left) / r.width : 0;
        const y = r.height > 0 ? (e.clientY - r.top) / r.height : 0;
        void ipc.embedInput(props.embedId, "wheel", x, y, undefined, undefined, -e.deltaY);
      }}
      onKeyDown={(e) => {
        e.preventDefault();
        void ipc.embedInput(props.embedId, "key", 0, 0, undefined, e.keyCode);
      }}
    />
  );
}
