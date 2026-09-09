import { useMemo, useState } from "react";
import { GraduationCap } from "lucide-react";
import { useI18n } from "../../../i18n";
import { Modal } from "../../../components/Modal";
import { COMPLETION_TASKS, COMPLETION_TOTAL, COMPLETION_DONE, COMPLETION_GENERATED_AT, type CompletionTask } from "../../../generated/completion";
import type { Settings } from "../../../lib/settings";

/**
 * AI-20 质量门禁与收官组 — V-100 收官毕业页。
 *
 * 设置「关于」入口 + 首启一次性显示（全部完成后）：
 * - 完成度墙：按域分组、数据从进度文档自动生成（gen-completion.cjs，
 *   不手工维护；未完成项如实灰色 —— 未出现在进度文档 = 未交付口径）；
 * - 本地使用摘要：最常用 5 个功能（M-28 键位使用统计，仅本地、可关闭，
 *   V-47 隐私三态精神：关闭 = 摘要退化为纯完成度墙）；
 * - 致谢；「不再显示」一次点击永久生效（localStorage）。
 */

const DISMISS_KEY = "variable:graduation:dismissed:v1";

export function isGraduationDismissed(): boolean {
  try {
    return localStorage.getItem(DISMISS_KEY) === "1";
  } catch {
    return false;
  }
}

function dismissGraduation(): void {
  try {
    localStorage.setItem(DISMISS_KEY, "1");
  } catch {
    /* ignore */
  }
}

/** 按域分组（保留首见顺序）。 */
function byDomain(): { domain: string; tasks: CompletionTask[] }[] {
  const map = new Map<string, CompletionTask[]>();
  for (const t of COMPLETION_TASKS) {
    if (!map.has(t.domain)) map.set(t.domain, []);
    map.get(t.domain)!.push(t);
  }
  return [...map.entries()].map(([domain, tasks]) => ({ domain, tasks }));
}

/** 本地使用摘要：M-28 键位统计 Top5（仅本地；关闭统计 = 如实无摘要）。 */
function topFeatures(settings: Settings): { action: string; count: number }[] {
  const entries = Object.entries<unknown>(settings.keyStats ?? {});
  return entries
    .filter(([, n]) => typeof n === "number" && n > 0)
    .sort((a, b) => (b[1] as number) - (a[1] as number))
    .slice(0, 5)
    .map(([action, count]) => ({ action, count: count as number }));
}

export function GraduationWall(props: {
  settings: Settings;
  open: boolean;
  onClose: () => void;
  /** 首启模式（true = 「不再显示」按钮；设置入口模式 = 仅关闭） */
  firstRun?: boolean;
}): React.ReactElement | null {
  const { t } = useI18n();
  const domains = useMemo(byDomain, []);
  const features = useMemo(() => topFeatures(props.settings), [props.settings]);
  const pct = COMPLETION_TOTAL > 0 ? Math.round((COMPLETION_DONE / COMPLETION_TOTAL) * 100) : 0;

  if (!props.open) return null;
  return (
    <Modal open onClose={props.onClose} title={t("v100Title")} width={640}>
      <div className="graduation" data-testid="graduation-wall">
        <div className="row gap8" style={{ alignItems: "center", marginBottom: 10 }}>
          <GraduationCap size={22} />
          <strong>{t("v100Headline")}</strong>
          <span className="flex-1" />
          <span className="chip">{pct}% · {COMPLETION_DONE}/{COMPLETION_TOTAL}</span>
        </div>
        <p className="dim small">{t("v100Intro", { date: COMPLETION_GENERATED_AT })}</p>

        {/* 完成度墙：按域分组 */}
        <div className="graduation-wall" data-testid="graduation-domains">
          {domains.map((d) => {
            const done = d.tasks.filter((x) => x.delivered).length;
            return (
              <div key={d.domain} className="graduation-domain">
                <div className="row gap8 small" style={{ marginBottom: 4 }}>
                  <strong className="ellipsis">{d.domain}</strong>
                  <span className="dim">{done}/{d.tasks.length}</span>
                </div>
                <div className="graduation-chips">
                  {d.tasks.map((x) => (
                    <span
                      key={x.id}
                      className={`chip g-task ${x.delivered ? "done" : "todo"}`}
                      title={`${x.id} ${x.name}${x.delivered ? "" : ` — ${t("v100Pending")}`}`}
                    >
                      {x.id}
                    </span>
                  ))}
                </div>
              </div>
            );
          })}
        </div>

        {/* 本地使用摘要（仅本地；可关闭） */}
        <h4 style={{ marginTop: 16 }}>{t("v100UsageTitle")}</h4>
        {features.length === 0 ? (
          <p className="dim small">{t("v100UsageEmpty")}</p>
        ) : (
          <ol className="graduation-usage small" data-testid="graduation-usage">
            {features.map((f, i) => (
              <li key={f.action}>
                {i + 1}. {t(f.action)} <span className="dim">×{f.count}</span>
              </li>
            ))}
          </ol>
        )}
        <p className="dim small" style={{ marginTop: 4 }}>{t("v100UsagePrivacy")}</p>

        {/* 致谢 */}
        <p className="dim small" style={{ marginTop: 12, whiteSpace: "pre-line" }}>{t("v100Thanks")}</p>

        <div className="row gap8" style={{ justifyContent: "flex-end", marginTop: 12 }}>
          {props.firstRun && (
            <button
              type="button"
              className="btn ghost"
              onClick={() => { dismissGraduation(); props.onClose(); }}
            >
              {t("v100NeverAgain")}
            </button>
          )}
          <button type="button" className="btn primary" onClick={props.onClose}>
            {t("v100Close")}
          </button>
        </div>
      </div>
    </Modal>
  );
}

/** 首启一次性毕业页（App 挂载：全部收口后首启显示；「不再显示」永久生效）。 */
export function GraduationFirstRun(props: { settings: Settings }): React.ReactElement | null {
  const [open, setOpen] = useState(!isGraduationDismissed() && COMPLETION_DONE >= COMPLETION_TOTAL);
  if (COMPLETION_DONE < COMPLETION_TOTAL) return null; // 未全部收口不显示（诚实口径）
  return <GraduationWall settings={props.settings} open={open} onClose={() => setOpen(false)} firstRun />;
}

/** 设置→关于入口按钮。 */
export function GraduationEntry(props: { settings: Settings }): React.ReactElement {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" className="btn" data-testid="graduation-entry" onClick={() => setOpen(true)}>
        <GraduationCap size={14} style={{ marginRight: 4, verticalAlign: -2 }} />
        {t("v100EntryBtn")}
      </button>
      {open && <GraduationWall settings={props.settings} open onClose={() => setOpen(false)} />}
    </>
  );
}
