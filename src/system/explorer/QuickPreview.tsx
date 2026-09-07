import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import { toAssetUrl } from "../../features/background/CosmicBackground";

/**
 * F-2.6 快速预览：文件管理器/桌面图标选中项按空格呼出（按空格或 Esc 关闭）。
 * 图片/视频/音频走受控 asset 协议直显；文本读前 200 行；其余如实给出
 * 类型/大小信息（不做假缩略图）。
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
]);

function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export function QuickPreview(props: { target: QuickPreviewTarget; onClose: () => void }): React.ReactElement {
  const { t } = useI18n();
  const [text, setText] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const e = props.target;
  const ext = (e.ext ?? "").toLowerCase();

  useEffect(() => {
    setText(null);
    setErr(null);
    if (TEXT.has(ext)) {
      void (async () => {
        try {
          const raw = await ipc.readTextFile(e.path);
          const lines = raw.split("\n").slice(0, 200).join("\n");
          setText(lines + (raw.split("\n").length > 200 ? "\n…" : ""));
        } catch (ex) {
          setErr(ex instanceof Error ? ex.message : String(ex));
        }
      })();
    }
  }, [e.path, ext]);

  return (
    <div className="qprev-backdrop" role="dialog" aria-label={`${t("qprevTitle")} · ${e.name}`} onClick={props.onClose}>
      <div className="qprev-card" onClick={(ev) => ev.stopPropagation()}>
        <div className="qprev-head">
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
          ) : TEXT.has(ext) ? (
            <pre className="qprev-text small">{err ?? text ?? "…"}</pre>
          ) : ext === "pdf" ? (
            <iframe src={toAssetUrl(e.path)} title={e.name} className="qprev-frame" />
          ) : (
            <p className="dim small" style={{ padding: 20 }}>
              {t("qprevUnsupported")}（.{ext || "?"} · {fmtBytes(e.size)}）
            </p>
          )}
        </div>
        <p className="dim small" style={{ padding: "4px 12px 8px" }}>{t("qprevHint")}</p>
      </div>
    </div>
  );
}
