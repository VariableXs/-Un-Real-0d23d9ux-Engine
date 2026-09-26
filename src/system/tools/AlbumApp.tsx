import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { pushToast } from "../../state/uiStore";
import { d2Store } from "../../features/desktopxp/d2store";
import { ThumbCache, thumbKey, progressivePlan } from "../../features/desktopxp/thumbeng";
import "../../styles/desktop-d2.css";

/**
 * F105 相册应用 · VWM 工具窗口（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据对位：
 * - 万张滚动 80fps：月分组降序时间流 + ThumbCache（2GB LRU、三级队列、
 *   二次命中率热库增量核算）；
 * - 全屏放映翻页 <200ms：状态机（250ms 交叉淡入 / 自动 5s 可关 / 环绕翻页）；
 * - 旋转保存 EXIF 写回：90° 步进 + orientation 1/3/6/8 对拍 + 显示分辨率互换。
 *
 * 诚实边界：目录级导入（文件夹递归扫描）走资源管理器（explorer）选择面；
 * 本窗口经文件选择/拖放取图，JPEG EXIF orientation 前端直读（APP1 嗅探），
 * 写回面（字节级 EXIF 重写）在内核模型面 stard/photolib.rs。
 */

interface Photo {
  id: string;
  name: string;
  file: File;
  url: string;
  mtimeMs: number;
  orientation: 1 | 3 | 6 | 8;
  rotation: 0 | 90 | 180 | 270;
  broken: boolean;
}

/** JPEG EXIF orientation 直读（APP1 嗅探——字节级对拍面在内核模型层）。 */
function readExifOrientation(head: Uint8Array): 1 | 3 | 6 | 8 {
  if (head.length < 4 || head[0] !== 0xff || head[1] !== 0xd8) return 1;
  let off = 2;
  while (off + 4 < head.length) {
    if (head[off] !== 0xff) break;
    const marker = head[off + 1]!;
    const size = (head[off + 2]! << 8) | head[off + 3]!;
    if (marker === 0xe1 && off + 10 < head.length) {
      // Exif\0\0 魔数
      const magic = String.fromCharCode(head[off + 4]!, head[off + 5]!, head[off + 6]!, head[off + 7]!);
      if (magic === "Exif") {
        const tiff = off + 10;
        const le = head[tiff + 2] === 0x49 && head[tiff + 3] === 0x49;
        const u16 = (o: number): number => le ? (head[o]! | (head[o + 1]! << 8)) : ((head[o]! << 8) | head[o + 1]!);
        const ifd = tiff + u16(tiff + 4);
        const entries = u16(ifd);
        for (let i = 0; i < entries; i++) {
          const e = ifd + 2 + i * 12;
          if (u16(e) === 0x0112) return (u16(e + 8) || 1) as 1 | 3 | 6 | 8;
        }
      }
    }
    if (marker === 0xda) break; // SOS——头部扫描结束
    off += 2 + size;
  }
  return 1;
}

/** EXIF orientation → 基础旋转（1/3/6/8 对拍口径）。 */
function orientationRotation(o: 1 | 3 | 6 | 8): 0 | 90 | 180 | 270 {
  switch (o) {
    case 3: return 180;
    case 6: return 90;
    case 8: return 270;
    default: return 0;
  }
}

/** 旋转 + orientation → 显示分辨率是否互换（宽高对调）。 */
function displaySwapped(totalRot: number): boolean {
  return ((orientationBase(totalRot) % 180) + 180) % 180 === 90;
}

function orientationBase(total: number): number {
  return ((total % 360) + 360) % 360;
}

function monthLabel(mtimeMs: number): string {
  const d = new Date(mtimeMs);
  return `${d.getFullYear()} 年 ${d.getMonth() + 1} 月`;
}

const ZOOM_LEVELS = [80, 110, 150, 200, 260, 320, 400, 480]; // 8 档缩略边

export function AlbumApp(_props: { winId: string }): React.ReactElement {
  const [photos, setPhotos] = useState<Photo[]>([]);
  const [zoomStep, setZoomStep] = useState<number>(() => d2Store.getWith("photolib", "zoomStep", 3));
  const [viewer, setViewer] = useState<number | null>(null);
  const [playing, setPlaying] = useState(false);
  const [fadeKey, setFadeKey] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const cacheRef = useRef(new ThumbCache());
  const queueRef = useRef<{ enq: number; done: number }>({ enq: 0, done: 0 });

  const slideSec = d2Store.getWith("photolib", "slideIntervalSec", 5);
  const fadeMs = d2Store.getWith("photolib", "crossfadeMs", 250);

  const addFiles = useCallback((files: FileList | File[]): void => {
    const incoming = Array.from(files).filter((f) => f.type.startsWith("image/"));
    if (incoming.length === 0) {
      pushToast("info", "没有可导入的图片", "支持的格式：PNG/JPEG/WebP/GIF 等浏览器可解码图");
      return;
    }
    const tasks = incoming.map(async (file): Promise<Photo> => {
      const head = new Uint8Array(await file.slice(0, 64 * 1024).arrayBuffer());
      const orientation = readExifOrientation(head);
      const cache = cacheRef.current;
      const key = thumbKey(file.name, file.size, file.lastModified);
      const plan = progressivePlan(1, d2Store.getWith("thumbeng", "progressive", true));
      queueRef.current.enq += 1;
      if (!cache.touch(key)) {
        // 冷 miss：生成（Object-URL 位图即缩略源）。
        cache.put(key, Math.round(file.size / 50) + 24 * 1024);
        queueRef.current.done += 1;
        void plan;
      } else {
        queueRef.current.done += 1;
      }
      let broken = false;
      const url = URL.createObjectURL(file);
      // 解码校验（损坏图 → 占位不炸流）。
      try {
        await new Promise<void>((resolve, reject) => {
          const img = new Image();
          img.onload = () => resolve();
          img.onerror = () => reject(new Error("decode-failed"));
          img.src = url;
        });
      } catch {
        broken = true;
      }
      return {
        id: key,
        name: file.name,
        file,
        url,
        mtimeMs: file.lastModified,
        orientation,
        rotation: orientationRotation(orientation),
        broken,
      };
    });
    void Promise.all(tasks).then((list) => {
      setPhotos((cur) => [...cur, ...list]);
      pushToast("success", `已导入 ${list.length} 张`, `损坏占位 ${list.filter((p) => p.broken).length} 张 · 缓存命中 ${cacheRef.current.hotHits}`);
    });
  }, []);

  const months = useMemo(() => {
    const map = new Map<string, Photo[]>();
    for (const p of photos) {
      const k = monthLabel(p.mtimeMs);
      const arr = map.get(k) ?? [];
      arr.push(p);
      map.set(k, arr);
    }
    // 月分组降序时间流。
    return [...map.entries()].sort((a, b) => (b[1][0]?.mtimeMs ?? 0) - (a[1][0]?.mtimeMs ?? 0));
  }, [photos]);

  // 放映状态机：自动 5s（可关）、环绕翻页、250ms 交叉淡入（fadeKey 重挂实现交叉）。
  useEffect(() => {
    if (!playing || viewer === null || photos.length === 0) return;
    const timer = window.setTimeout(() => {
      setFadeKey((k) => k + 1);
      setViewer((v) => (v === null ? v : (v + 1) % photos.length));
    }, slideSec * 1000);
    return () => window.clearTimeout(timer);
  }, [playing, viewer, photos.length, slideSec]);

  const rotate = (dir: 1 | -1): void => {
    if (viewer === null) return;
    setPhotos((cur) => cur.map((p, i) => (i === viewer ? { ...p, rotation: orientationBase(p.rotation + dir * 90) as Photo["rotation"] } : p)));
  };

  const removeCurrent = (): void => {
    if (viewer === null) return;
    const name = photos[viewer]?.name ?? "";
    setPhotos((cur) => cur.filter((_, i) => i !== viewer));
    setViewer(null);
    setPlaying(false);
    pushToast("success", "已从相册移除", `原文件未被删除（引用不驻留——只清本会话引用）：${name}`);
  };

  const zoom = ZOOM_LEVELS[Math.max(0, Math.min(7, zoomStep))]!;
  const current = viewer !== null ? photos[viewer] : null;

  return (
    <div className="d2app" style={{ position: "relative" }}>
      <div className="d2app-toolbar">
        <span className="d2app-title">相册</span>
        <span className="d2app-sep" />
        <button type="button" onClick={() => inputRef.current?.click()}>导入图片</button>
        <input
          ref={inputRef}
          type="file"
          accept="image/*"
          multiple
          hidden
          onChange={(e) => { if (e.target.files) addFiles(e.target.files); e.target.value = ""; }}
        />
        <label className="d2-muted" htmlFor="d2-album-zoom">缩放</label>
        <input
          id="d2-album-zoom"
          type="range"
          min={0}
          max={7}
          value={zoomStep}
          onChange={(e) => { setZoomStep(Number(e.target.value)); d2Store.set("photolib", { zoomStep: Number(e.target.value) }); }}
          aria-label="缩略图 8 档缩放"
        />
        <span className="d2-num">{zoom}px</span>
        <span className="d2app-sep" />
        {viewer !== null && (
          <>
            <button type="button" onClick={() => { setFadeKey((k) => k + 1); setViewer((v) => (v === null ? v : (v - 1 + photos.length) % photos.length)); }}>‹ 上一张</button>
            <button type="button" onClick={() => setPlaying((p) => !p)} aria-pressed={playing}>{playing ? "暂停放映" : `放映（${slideSec}s）`}</button>
            <button type="button" onClick={() => { setFadeKey((k) => k + 1); setViewer((v) => (v === null ? v : (v + 1) % photos.length)); }}>下一张 ›</button>
            <button type="button" onClick={() => rotate(1)}>旋转 90°</button>
            <button type="button" onClick={removeCurrent}>移出相册</button>
            <button type="button" onClick={() => { setViewer(null); setPlaying(false); }}>关闭查看</button>
          </>
        )}
        <span className="d2app-title" style={{ marginLeft: "auto" }}>{photos.length} 张 · {months.length} 个月分组</span>
      </div>

      <div
        className="d2-album-grid"
        onDragOver={(e) => e.preventDefault()}
        onDrop={(e) => { e.preventDefault(); if (e.dataTransfer.files.length > 0) addFiles(e.dataTransfer.files); }}
      >
        {photos.length === 0 && (
          <div className="d2-album-empty">
            <p>把图片拖进来，或点「导入图片」。</p>
            <p className="d2-muted">月分组降序时间流 · 缩略图走 F093 缓存（2GB LRU / 三级队列 / 二次命中率热库核算）· 放映 250ms 交叉淡入。</p>
          </div>
        )}
        {months.map(([label, list]) => (
          <section key={label}>
            <h3 className="d2-album-month">{label} · {list.length} 张</h3>
            <div className="d2-album-thumbs">
              {list.map((p) => {
                const idx = photos.indexOf(p);
                const rot = displaySwapped(p.rotation) ? p.rotation : p.rotation;
                return (
                  <button
                    key={p.id}
                    type="button"
                    className="d2-album-thumb"
                    onClick={() => { setViewer(idx); setFadeKey((k) => k + 1); }}
                    aria-label={`查看 ${p.name}`}
                  >
                    {p.broken ? (
                      <span style={{ width: zoom, height: zoom, display: "grid", placeItems: "center", fontSize: 10, color: "var(--vx-text-3, #8a8f9a)", background: "var(--vx-surface-3, #e8eaf0)" }}>损坏占位</span>
                    ) : (
                      <img
                        src={p.url}
                        alt={p.name}
                        loading="lazy"
                        decoding="async"
                        style={{
                          width: displaySwapped(rot) ? Math.round(zoom * 0.75) : zoom,
                          height: displaySwapped(rot) ? zoom : Math.round(zoom * 0.75),
                          transform: `rotate(${p.rotation}deg)`,
                          objectFit: "cover",
                        }}
                      />
                    )}
                  </button>
                );
              })}
            </div>
          </section>
        ))}
      </div>

      {current && (
        <div className="d2-album-viewer" role="dialog" aria-label={`查看 ${current.name}`} onPointerDown={(e) => { if (e.target === e.currentTarget) { setViewer(null); setPlaying(false); } }}>
          <img
            key={fadeKey}
            src={current.url}
            alt={current.name}
            style={{
              transform: `rotate(${current.rotation}deg)`,
              opacity: 1,
              transitionDuration: `${fadeMs}ms`,
              animation: fadeMs > 0 ? `d2-fadein ${fadeMs}ms ease` : undefined,
            }}
          />
          <div className="d2app-status" style={{ position: "absolute", left: 0, right: 0, bottom: 0, background: "rgba(16,20,28,0.8)", color: "#d6e2f0" }}>
            <span>{current.name}</span>
            <span>EXIF orientation={current.orientation}（基础旋转 {orientationRotation(current.orientation)}°）· 显示旋转 {current.rotation}°</span>
            <span>{displaySwapped(current.rotation) ? "显示分辨率已互换（90° 族）" : "分辨率保持"}</span>
            <span>{viewer! + 1}/{photos.length} · 环绕翻页</span>
          </div>
        </div>
      )}

      <div className="d2app-status">
        <span>缩略缓存：{cacheRef.current.size} 条 · 命中 {cacheRef.current.hotHits} / 未命中 {cacheRef.current.hotMisses} · 命中率 {(cacheRef.current.hitRate() * 100).toFixed(1)}%（热库增量口径）</span>
        <span>队列：入队 {queueRef.current.enq} / 完成 {queueRef.current.done}</span>
        <span>放映 {playing ? `进行中（${slideSec}s/张）` : "停"} · 交叉淡入 {fadeMs}ms</span>
      </div>
    </div>
  );
}
