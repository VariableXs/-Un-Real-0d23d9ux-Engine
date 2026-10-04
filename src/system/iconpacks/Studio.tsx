/**
 * N-12 图标包工坊 Studio（车道 E 自挂载 overlay，事件 detail.feature === "iconpacks"）：
 * - 安装（文件选择 + 校验 + 12 宫格对比预览 → 确认）/ 卸载（全部还原）；
 * - 缺键回退计数展示（registry.getFallbackCount）；
 * - 导出骨架（从 activePack 生成 .vicon 模板，便于社区基于现包再创作）。
 * 诚实边界：预览「原」列 = 占位首字母（真实原生图需 DesktopIcons/StartMenu 接线
 * useIcon，集成阶段完成）；.vicon 为 JSON 而非 zip。
 */
import { useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { pushOverlay, popOverlay, pushToast } from "../../state/uiStore";
import {
  activePack, getFallbackCount, installPack, listPacks, uninstallPack, type IconPackMeta,
} from "./registry";
import { exportSkeleton, iconResource, PREVIEW_KEYS, validateVicon, type ViconFile } from "./vicon";
import { LABELS, useLaneLang } from "./labels";

interface StudioProps {
  onClose: () => void;
}

function keyTail(key: string): string {
  const tail = key.split(":").pop() ?? key;
  return (tail.trim()[0] ?? "?").toUpperCase();
}

function downloadVicon(file: ViconFile): void {
  const blob = new Blob([JSON.stringify(file, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = `${file.name.replace(/[\\/:*?"<>|]/g, "_")}.vicon`;
  void a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export default function IconPackStudio({ onClose }: StudioProps): React.ReactPortal {
  const lang = useLaneLang();
  const t = LABELS[lang];
  const [packs, setPacks] = useState<IconPackMeta[]>(listPacks);
  const [pending, setPending] = useState<ViconFile | null>(null);
  const [pendingErrors, setPendingErrors] = useState<string[]>([]);
  const [fallback, setFallback] = useState(getFallbackCount());

  useEffect(() => {
    pushOverlay("iconpacks");
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      popOverlay("iconpacks");
      window.removeEventListener("keydown", onKey, true);
    };
  }, [onClose]);

  const current = activePack();

  const previewKeys = useMemo(() => {
    const fromPack = current ? Object.keys(current.icons) : [];
    const keys = [...new Set([...(fromPack.length >= 12 ? fromPack.slice(0, 12) : PREVIEW_KEYS)])];
    return keys.slice(0, 12);
    // 预览列固定按当前包（或模板键）展示
  }, [current]);

  const doPick = async (file: File | undefined): Promise<void> => {
    if (!file) return;
    const text = await file.text();
    const r = validateVicon(text);
    if (!r.ok || !r.data) {
      setPending(null);
      setPendingErrors(r.errors.slice(0, 6));
      pushToast("error", t.bad, r.errors[0]);
      return;
    }
    setPendingErrors([]);
    setPending(r.data);
  };

  const doInstall = (): void => {
    if (!pending) return;
    const id = pending.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || `pack-${Date.now()}`;
    installPack({ id, name: pending.name, version: "1.0", icons: pending.icons });
    setPacks(listPacks());
    setPending(null);
    pushToast("success", t.imported, pending.name);
  };

  const doUninstall = (id: string): void => {
    uninstallPack(id);
    setPacks(listPacks());
    pushToast("info", t.uninstalled);
  };

  const renderIcon = (res: string | null): React.ReactNode => {
    if (!res) return <span className="iconpk-fb">{t.fallbackRow}</span>;
    const r = iconResource(res);
    return r.kind === "img"
      ? <img className="iconpk-icon" src={r.value} alt="" />
      : <span className="iconpk-icon svg" dangerouslySetInnerHTML={{ __html: r.value }} />;
  };

  return createPortal(
    <div className="iconpk-overlay" role="dialog" aria-label={t.title}>
      <div className="iconpk-win">
        <header className="iconpk-head">
          <h3>{t.title}</h3>
          <span className="iconpk-note">{t.zipNote}</span>
          <button type="button" className="iconpk-btn" aria-label={t.close} onClick={onClose}>
            <X size={16} />
          </button>
        </header>

        <div className="iconpk-body">
          <section className="iconpk-col">
            <div className="iconpk-sec-title">{t.installed}</div>
            {packs.length === 0 && <div className="dim">{t.noPack}</div>}
            {packs.map((p) => (
              <div key={p.id} className="iconpk-packrow">
                <div>
                  <strong>{p.name}</strong>
                  <div className="dim tiny">
                    {t.version} {p.version} · {Object.keys(p.icons).length} {t.keys}
                  </div>
                </div>
                <button type="button" className="iconpk-btn danger" onClick={() => doUninstall(p.id)}>{t.uninstall}</button>
              </div>
            ))}
            <label className="iconpk-btn primary">
              {t.pick}
              <input type="file" accept=".vicon,.json,application/json" hidden onChange={(e) => void doPick(e.target.files?.[0])} />
            </label>
            <button type="button" className="iconpk-btn" onClick={() => downloadVicon(exportSkeleton(current ? current.icons : null, current?.name ?? "variable"))}>
              {t.exportSkeleton}
            </button>
            <div className="iconpk-fbcount">
              <strong>{fallback}</strong>
              <span>{t.fallbackCount}</span>
              <div className="dim tiny">{t.fallbackNote}</div>
            </div>
            <button type="button" className="iconpk-btn tiny" onClick={() => setFallback(getFallbackCount())}>↻</button>

            {pendingErrors.length > 0 && (
              <div className="iconpk-errors">
                {pendingErrors.map((e, i) => (
                  <div key={i}>{e}</div>
                ))}
              </div>
            )}

            {pending && (
              <div className="iconpk-pending">
                <div className="iconpk-sec-title">{t.preview}: {pending.name}</div>
                <div className="iconpk-grid">
                  {previewKeys.map((k) => {
                    const hit = pending.icons[k];
                    return (
                      <div key={k} className={`iconpk-cell ${hit === undefined ? "miss" : ""}`} title={k}>
                        <span className="iconpk-cell-key">{k}</span>
                        <div className="iconpk-cell-pair">
                          <span className="iconpk-old">{keyTail(k)}</span>
                          {hit === undefined ? <span className="iconpk-fb">{t.fallbackRow}</span> : renderIcon(hit)}
                        </div>
                      </div>
                    );
                  })}
                </div>
                <div className="row end gap8">
                  <button type="button" className="iconpk-btn" onClick={() => setPending(null)}>{t.cancel}</button>
                  <button type="button" className="iconpk-btn primary" onClick={doInstall}>{t.confirm}</button>
                </div>
              </div>
            )}
          </section>

          <section className="iconpk-col">
            <div className="iconpk-sec-title">{t.grid}</div>
            <div className="dim tiny">{t.nativeNote}</div>
            <div className="iconpk-grid">
              {previewKeys.map((k) => {
                const orig = current?.icons[k];
                return (
                  <div key={k} className={`iconpk-cell ${orig === undefined ? "miss" : ""}`} title={k}>
                    <span className="iconpk-cell-key">{k}</span>
                    <div className="iconpk-cell-pair">
                      <span className="iconpk-old">{keyTail(k)}</span>
                      {renderIcon(orig ?? null)}
                    </div>
                  </div>
                );
              })}
            </div>
          </section>
        </div>
      </div>
    </div>,
    document.body,
  );
}