import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { TrustEntry, TrustWall } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtTime } from "./shared";

/** U-35 信任链：外来文件登记（WinVerifyTrust 签名/链验证）→ 信任墙 + 签名者熟识度。 */
export function TrustPanel(): React.ReactElement {
  const { t } = useI18n();
  const [wall, setWall] = useState<TrustWall | null>(null);
  const [path, setPath] = useState("");
  const [origin, setOrigin] = useState<"drag" | "download" | "copy" | "install">("copy");
  const [busy, setBusy] = useState(false);

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.trustWall().then(setWall).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const register = async (): Promise<void> => {
    if (!path) return;
    setBusy(true);
    try {
      await ipc.trustRegister(path, origin);
      setPath("");
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  const reverifyAll = async (): Promise<void> => {
    setBusy(true);
    try {
      setWall(await ipc.trustReverifyAll());
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (p: string): Promise<void> => {
    try {
      await ipc.trustRemove(p);
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const verdictChip = (e: TrustEntry): { cls: string; label: string } => {
    const v = e.verdict;
    if (!v) return { cls: "warn", label: "—" };
    switch (v.status) {
      case "valid": return { cls: "ok", label: t("twValid") };
      case "unsigned": return { cls: "", label: t("twUnsigned") };
      case "expired": return { cls: "warn", label: t("twExpired") };
      case "tampered": return { cls: "bad", label: t("twTampered") };
      default: return { cls: "warn", label: t("twUnsupported") };
    }
  };

  return (
    <section>
      <h2>{t("twTitle")}</h2>
      <div className="dv-row">
        <input className="dv-input wide" placeholder={t("twPathLabel")}
          value={path} onChange={(e) => setPath(e.target.value)} />
        <select className="dv-select" value={origin} onChange={(e) => setOrigin(e.target.value as typeof origin)}>
          <option value="drag">drag</option>
          <option value="download">download</option>
          <option value="copy">copy</option>
          <option value="install">install</option>
        </select>
        <button className="dv-btn primary" disabled={busy} onClick={() => void register()}>{t("twRegister")}</button>
        <button className="dv-btn" disabled={busy} onClick={() => void reverifyAll()}>{t("twReverifyAll")}</button>
      </div>

      {wall && Object.keys(wall.signerCounts).length > 0 && (
        <div className="dv-row">
          {Object.entries(wall.signerCounts).map(([signer, n]) => (
            <span key={signer} className="dv-chip">
              {signer} ×{n}{n >= 2 ? ` · ${t("twFamiliar")}` : ""}
            </span>
          ))}
        </div>
      )}

      {(!wall || wall.entries.length === 0) ? (
        <div className="dv-empty">{t("twEmpty")}</div>
      ) : (
        <div className="dv-list">
          {wall.entries.map((e) => {
            const chip = verdictChip(e);
            return (
              <div key={e.path} className="dv-item">
                <span className={`dv-chip ${chip.cls}`}>{chip.label}</span>
                <span className="grow" title={e.display}>{e.display}</span>
                {e.verdict?.signer && <span className="dv-chip">{e.verdict.signer}</span>}
                <span className="dv-chip">{e.origin}</span>
                <span className="dv-tl-meta">{fmtTime(e.lastVerified)}</span>
                <button className="dv-btn danger" onClick={() => void remove(e.path)}>{t("twRemove")}</button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
