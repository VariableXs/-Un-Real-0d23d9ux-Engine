import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { RecItem, RecPolicy } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtSize, fmtTime } from "./shared";

/**
 * U-27/U-34 回收站 2.0 策略：容量/时间阈值 + 自动清理开关（默认关）。
 * 预览先行——先看将清理哪些条目，确认后才执行；星标条目后端永不清理。
 */
export function RecyclePolicyPanel(): React.ReactElement {
  const { t } = useI18n();
  const [policy, setPolicy] = useState<RecPolicy | null>(null);
  const [doomed, setDoomed] = useState<RecItem[] | null>(null);

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.recPolicyGet().then(setPolicy).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const save = async (): Promise<void> => {
    if (!policy) return;
    try {
      await ipc.recPolicySet(policy);
      pushToast("success", t("rcpSaved"));
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const preview = async (): Promise<void> => {
    try {
      setDoomed(await ipc.recPolicyPreview());
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const apply = async (): Promise<void> => {
    try {
      const n = await ipc.recPolicyApply();
      pushToast("success", t("rcpDoomed", { n }));
      setDoomed(null);
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  if (!policy) return <section><h2>{t("dvTabRecycle")}</h2><div className="dv-empty">—</div></section>;

  return (
    <section>
      <h2>{t("dvTabRecycle")}</h2>
      <div className="dv-row">
        <label className="dv-hint">{t("rcpCapacity")}</label>
        <input className="dv-input" type="number" min={0}
          value={Math.round(policy.capacityBytes / 1024 ** 3)}
          onChange={(e) => setPolicy({ ...policy, capacityBytes: Math.max(0, Number(e.target.value) || 0) * 1024 ** 3 })} />
        <label className="dv-hint">{t("rcpMaxDays")}</label>
        <input className="dv-input" type="number" min={0}
          value={policy.maxDays}
          onChange={(e) => setPolicy({ ...policy, maxDays: Math.max(0, Number(e.target.value) || 0) })} />
        <label className="dv-hint" style={{ display: "flex", alignItems: "center", gap: 4 }}>
          <input type="checkbox" checked={policy.autoClean}
            onChange={(e) => setPolicy({ ...policy, autoClean: e.target.checked })} />
          {t("rcpAutoClean")}
        </label>
        <button className="dv-btn primary" onClick={() => void save()}>{t("rcpSaved")}</button>
      </div>
      <p className="dv-hint">{t("rcpStarExempt")}</p>

      <div className="dv-row">
        <button className="dv-btn" onClick={() => void preview()}>{t("rcpPreview")}</button>
        {doomed && doomed.length > 0 && (
          <button className="dv-btn danger" onClick={() => void apply()}>{t("rcpApply")}</button>
        )}
      </div>

      {doomed && (
        doomed.length === 0 ? (
          <div className="dv-empty">{t("rcpNoneDoomed")}</div>
        ) : (
          <>
            <p className="dv-hint">{t("rcpDoomed", { n: doomed.length })}</p>
            <div className="dv-list">
              {doomed.map((it) => (
                <div key={`${it.id}:${it.source}`} className="dv-item">
                  <span className="dv-chip">{it.kind}</span>
                  <span className="grow">{it.title}</span>
                  <span className="dv-tl-meta">{fmtSize(it.size)} · {fmtTime(it.deletedAt)}</span>
                </div>
              ))}
            </div>
          </>
        )
      )}
    </section>
  );
}

