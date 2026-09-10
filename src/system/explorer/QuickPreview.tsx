import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import { toAssetUrl } from "../../features/background/CosmicBackground";
import type { ArchiveListing } from "../../lib/ipc";

/**
 * F-2.6 快速预览：文件管理器/桌面图标选中项按空格呼出（按空格或 Esc 关闭）。
 * 图片/视频/音频走受控 asset 协议直显；文本读前 200 行；其余如实给出
 * 类型/大小信息（不做假缩略图）。
 *
 * AI-09 增强（Z-29/M-22）：
 * - Z-29 格式扩展：zip 压缩包只读浏览（条目列表 + 单文件提取预览）；
 *   文本格式补充 vue/svelte/kt/swift/lua 等常见源码
 * - M-22 键导航：← → 在同目录文件间切换（由宿主注入 onPrev/onNext）
 */

export interface QuickPreviewTarget {
  name: string;
  path: string;
  kind: "dir" | "file";
  ext: string | null;
  size: number;
}

const IMG = new Set(["png", "jpg", "jpeg", "webp", "gif", "bmp", "ico", "avif"]);
const VID = new Set(["mp4", "webm", "ogv", "mov", "m4v"]);
const AUD = new Set(["mp3", "wav", "ogg", "flac", "m4a", "aac"]);
const TEXT = new Set([
  "txt", "md", "markdown", "json", "js", "mjs", "cjs", "ts", "tsx", "jsx",
  "css", "scss", "html", "xml", "yml", "yaml", "toml", "ini", "cfg", "conf",
  "rs", "py", "go", "java", "c", "h", "cpp", "hpp", "cs", "rb", "php", "sh",
  "bat", "cmd", "ps1", "sql", "log", "csv", "svg",
  // AI-09 Z-29：常见源码格式补充
  "vue", "svelte", "kt", "kts", "swift", "dart", "r", "jl", "lua", "pl",
  "gradle", "properties", "env", "gitignore", "dockerfile", "makefile",
]);
const ARCHIVE = new Set(["zip"]);

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export function QuickPreview(props: {
  target: QuickPreviewTarget;
  onClose: () => void;
  /** M-22 键导航：同目录上一个/下一个文件（无则不显示导航键）。 */
  onPrev?: () => void;
  onNext?: () => void;
}): React.ReactElement {
  const { t } = useI18n();
  const [text, setText] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [archive, setArchive] = useState<ArchiveListing | null>(null);
  const e = props.target;
  const ext = (e.ext ?? "").toLowerCase();
  // 压缩包内条目预览的序号（丢弃迟到链：用户快速点多个条目时只认最后一次）
  const previewSeq = useRef(0);

  useEffect(() => {
    let cancelled = false;
    previewSeq.current += 1;
    setText(null);
    setErr(null);
    setArchive(null);
    if (TEXT.has(ext)) {
      void (async () => {
        try {
          const raw = await ipc.readTextFile(e.path);
          if (cancelled) return;
          const lines = raw.split("\n").slice(0, 200).join("\n");
          setText(lines + (raw.split("\n").length > 200 ? "\n…" : ""));
        } catch (ex) {
          if (!cancelled) setErr(ex instanceof Error ? ex.message : String(ex));
        }
      })();
    }
    if (ARCHIVE.has(ext)) {
      void ipc
        .archiveLs(e.path)
        .then((a) => {
          if (!cancelled) setArchive(a);
        })
        .catch((ex: unknown) => {
          if (!cancelled) setErr(ex instanceof Error ? ex.message : String(ex));
        });
    }
    return () => {
      cancelled = true;
    };
  }, [e.path, ext]);

  // M-22 键导航：← → 切换同目录文件
  useEffect(() => {
    const onKey = (ev: KeyboardEvent): void => {
      if (ev.key === "ArrowLeft" && props.onPrev) {
        ev.preventDefault();
        props.onPrev();
      } else if (ev.key === "ArrowRight" && props.onNext) {
        ev.preventDefault();
        props.onNext();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [props.onPrev, props.onNext]);

  /** Z-29：压缩包内文本条目 → 提取到临时目录并预览内容。 */
  const previewInner = (innerPath: string): void => {
    const seq = ++previewSeq.current;
    void ipc
      .archiveExtractOne(e.path, innerPath)
      .then((tmp) => ipc.readTextFile(tmp))
      .then((raw) => {
        if (previewSeq.current !== seq) return;
        const lines = raw.split("\n").slice(0, 200).join("\n");
        setText(`${innerPath}\n\n${lines}${raw.split("\n").length > 200 ? "\n…" : ""}`);
      })
      .catch((ex: unknown) => {
        if (previewSeq.current !== seq) return;
        setErr(ex instanceof Error ? ex.message : String(ex));
      });
  };

  return (
    <div className="qprev-backdrop" role="dialog" aria-label={`${t("qprevTitle")} · ${e.name}`} onClick={props.onClose}>
      <div className="qprev-card" onClick={(ev) => ev.stopPropagation()}>
        <div className="qprev-head">
          {(props.onPrev || props.onNext) && (
            <span className="qprev-nav" role="navigation" aria-label={t("qprevNav")}>
              <button
                type="button"
                className="icon-btn tiny"
                aria-label={t("qprevPrev")}
                disabled={!props.onPrev}
                onClick={props.onPrev}
              >
                ‹
              </button>
              <button
                type="button"
                className="icon-btn tiny"
                aria-label={t("qprevNext")}
                disabled={!props.onNext}
                onClick={props.onNext}
              >
                ›
              </button>
            </span>
          )}
          <span className="ellipsis" title={e.name}>{e.name}</span>
          <span className="dim small">{e.kind === "dir" ? t("qprevDir") : fmtBytes(e.size)}</span>
          <button type="button" className="icon-btn tiny" aria-label={t("qprevClose")} onClick={props.onClose}>
            <X size={14} />
          </button>
        </div>
        <div className="qprev-body">
          {IMG.has(ext) ? (
            <img src={toAssetUrl(e.path)} alt={e.name} className="qprev-media" />
          ) : VID.has(ext) ? (
            <video src={toAssetUrl(e.path)} className="qprev-media" controls autoPlay muted playsInline />
          ) : AUD.has(ext) ? (
            <audio src={toAssetUrl(e.path)} controls />
          ) : ARCHIVE.has(ext) ? (
            <div className="qprev-archive">
              {err ? (
                <p className="dim small">{err}</p>
              ) : !archive ? (
                <p className="dim small" aria-busy="true">…</p>
              ) : (
                <ul className="qprev-archive-list">
                  {archive.entries.map((en) => (
                    <li key={en.innerPath} className={en.isDir ? "qprev-archive-dir" : ""}>
                      <button
                        type="button"
                        className="qprev-archive-entry"
                        disabled={en.isDir}
                        title={en.innerPath}
                        onClick={() => previewInner(en.innerPath)}
                      >
                        <span className="ellipsis">{en.name}</span>
                        <span className="dim small">{en.isDir ? "—" : fmtBytes(en.size)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          ) : TEXT.has(ext) ? (
            <pre className="qprev-text small">{err ?? text ?? "…"}</pre>
          ) : ext === "pdf" ? (
            <iframe src={toAssetUrl(e.path)} title={e.name} className="qprev-frame" />
          ) : (
            <p className="dim small" style={{ padding: 20 }}>
              {t("qprevUnsupported")}（.{ext || "?"} · {fmtBytes(e.size)}）
            </p>
          )}
          {/* Z-29：压缩包内提取出的文本预览（覆盖在条目列表之上） */}
          {ARCHIVE.has(ext) && text && (
            <pre className="qprev-text small qprev-archive-text">{text}</pre>
          )}
        </div>
        <p className="dim small" style={{ padding: "4px 12px 8px" }}>
          {t("qprevHint")}
          {(props.onPrev || props.onNext) && ` · ${t("qprevNavHint")}`}
          {ARCHIVE.has(ext) && ` · ${t("qprevArchiveHint")}`}
        </p>
      </div>
    </div>
  );
}
