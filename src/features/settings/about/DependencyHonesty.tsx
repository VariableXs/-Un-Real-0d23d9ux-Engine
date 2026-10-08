import { useState } from "react";
import { useI18n } from "../../../i18n";
import { ipc, errMessage, type Shell } from "../../../lib/ipc";
import { formatSmartTime } from "../../../lib/formatUnits";

/**
 * AI-20 质量门禁与收官组 — V-99 依赖诚实声明页 v2（设置→关于→外部依赖）。
 *
 * 在 D-5 接管边界声明下新增「外部依赖」节：winget（V-81）、系统 OCR
 * 语言包（V-42）、打印机（V-97/98）、字体回退链（V-76）——逐项标注：
 * 用途 / 缺失时行为 / 如何获取；每项含「检测当前状态」按钮（已装/未装即知）。
 * 与 V-42/V-81 的降级提示互链；文案 zh/en 双语；离线完整可读（纯本地探测）。
 */

type DepId = "winget" | "ocr" | "printer" | "fonts";

const DEP_ORDER: DepId[] = ["winget", "ocr", "printer", "fonts"];

export function DependencyHonesty(): React.ReactElement {
  const { t } = useI18n();
  const [probing, setProbing] = useState<DepId | "all" | null>(null);
  const [results, setResults] = useState<Partial<Record<DepId, { ok: boolean | null; detail: string }>>>({});
  const [probeAllAt, setProbeAllAt] = useState<number | null>(null);

  const probeOne = async (id: DepId): Promise<void> => {
    setProbing(id);
    try {
      const r = await ipc.sysdepProbe();
      const item = r.items.find((i) => i.id === id);
      if (item) setResults((prev) => ({ ...prev, [id]: { ok: item.available, detail: item.detail } }));
      setProbeAllAt(r.probedAt);
    } catch (e) {
      setResults((prev) => ({ ...prev, [id]: { ok: null, detail: errMessage(e).message } }));
    } finally {
      setProbing(null);
    }
  };

  const probeAll = async (): Promise<void> => {
    setProbing("all");
    try {
      const r = await ipc.sysdepProbe();
      const next: Partial<Record<DepId, { ok: boolean | null; detail: string }>> = {};
      for (const item of r.items) next[item.id as DepId] = { ok: item.available, detail: item.detail };
      setResults(next);
      setProbeAllAt(r.probedAt);
    } catch {
      /* 探测整体失败：各按钮可单独重试 */
    } finally {
      setProbing(null);
    }
  };

  const statusLabel = (id: DepId): { text: string; cls: string } | null => {
    const r = results[id];
    if (!r) return null;
    if (r.ok === null) return { text: t("v99ProbeFail"), cls: "dim" };
    return r.ok
      ? { text: t("v99Installed"), cls: "ok" }
      : { text: t("v99Missing"), cls: "warn" };
  };

  return (
    <section className="dep-honesty" data-testid="dep-honesty" aria-label={t("v99Title")}>
      <h4>{t("v99Title")}</h4>
      <p className="dim small" style={{ whiteSpace: "pre-line" }}>{t("v99Intro")}</p>
      <div className="backup-list" style={{ marginTop: 10 }}>
        {DEP_ORDER.map((id) => {
          const st = statusLabel(id);
          const r = results[id];
          return (
            <div key={id} className="backup-row" data-testid={`dep-row-${id}`}>
              <span className="small" style={{ minWidth: 72 }}>{t(`v99Name_${id}`)}</span>
              <span className="dim small flex-1" style={{ whiteSpace: "pre-line" }}>
                {t(`v99Use_${id}`)}
                {r?.detail ? `\n${r.detail}` : ""}
              </span>
              {st && <span className={`chip dep-status ${st.cls}`}>{st.text}</span>}
              <button
                type="button"
                className="btn ghost tiny"
                disabled={probing !== null}
                onClick={() => void probeOne(id)}
              >
                {probing === id ? t("v99Probing") : t("v99Check")}
              </button>
            </div>
          );
        })}
      </div>
      <div className="row gap8" style={{ marginTop: 10 }}>
        <button type="button" className="btn" disabled={probing !== null} onClick={() => void probeAll()} data-testid="dep-probe-all">
          {probing === "all" ? t("v99Probing") : t("v99CheckAll")}
        </button>
        {probeAllAt !== null && <span className="dim small">{t("v99ProbedAt")}: {formatSmartTime(probeAllAt)}</span>}
      </div>
      <p className="dim small" style={{ marginTop: 10 }}>
        {t("v99HowTo")} · {t("v99OfflineNote")}
      </p>
    </section>
  );
}

/** 类型引用（避免未使用告警 + 明确视图形状）。 */
export type SysdepProbeView = Shell.SysdepProbeView;
