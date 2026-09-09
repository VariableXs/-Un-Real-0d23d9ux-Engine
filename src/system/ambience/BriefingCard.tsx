/**
 * AI-18 M-69 今日简报卡 — 每日首启浮出的小卡（日期 + 第 N 周 + 一条小贴士）。
 *
 * 口径：
 * - 每日首次进入环境触发（briefing.ts 纯逻辑判定；本地日期键）；
 * - 8s 自动收起 / 点「知道了」收起 / 「今日不再」记 dismissed；
 * - 关闭开关（settings.ambience.briefing）后整卡不出现。
 */

import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../../i18n";
import { onDayRollover } from "../../lib/dayRollover";
import { isoWeek, nextTip, shouldShowBriefing, localDayKey, TIPS } from "./briefing";

const STATE_KEY = "ai18.briefingState";

interface BriefingState {
  lastShownDay: string;
  dismissedDay: string;
  shownIds: number[];
}

function loadState(): BriefingState {
  try {
    const raw = localStorage.getItem(STATE_KEY);
    if (!raw) return { lastShownDay: "", dismissedDay: "", shownIds: [] };
    const s = JSON.parse(raw) as Partial<BriefingState>;
    return {
      lastShownDay: typeof s.lastShownDay === "string" ? s.lastShownDay : "",
      dismissedDay: typeof s.dismissedDay === "string" ? s.dismissedDay : "",
      shownIds: Array.isArray(s.shownIds) ? s.shownIds.filter((n) => typeof n === "number") : [],
    };
  } catch {
    return { lastShownDay: "", dismissedDay: "", shownIds: [] };
  }
}

function saveState(s: BriefingState): void {
  try {
    localStorage.setItem(STATE_KEY, JSON.stringify(s));
  } catch { /* quota：忽略 */ }
}

export function BriefingCard(props: { enabled: boolean }): React.ReactElement | null {
  const { t, lang } = useI18n();
  const [state, setState] = useState<BriefingState>(() => loadState());
  const [visible, setVisible] = useState(false);
  const today = localDayKey(new Date());

  // 每日首启判定（挂载 + AI-20 M-90 统一日界事件 day://rollover —— 跨午夜
  // 即时翻页，替代 60s 轮询；系统时间手动调整同样触发）
  useEffect(() => {
    if (!props.enabled) return undefined;
    const check = (): void => {
      const s = loadState();
      setState(s);
      if (shouldShowBriefing(s.lastShownDay, new Date()) && s.dismissedDay !== localDayKey(new Date())) {
        setVisible(true);
      }
    };
    check();
    const offRollover = onDayRollover(check);
    return () => offRollover();
  }, [props.enabled]);

  // 8s 自动收起
  useEffect(() => {
    if (!visible) return undefined;
    const id = window.setTimeout(() => setVisible(false), 8000);
    return () => window.clearTimeout(id);
  }, [visible]);

  const tip = useMemo(() => nextTip(state.shownIds)[0], [state.shownIds]);

  if (!props.enabled || !visible || !tip) return null;

  const markShown = (): void => {
    const [, nextShown] = nextTip(state.shownIds);
    const next = { ...state, lastShownDay: today, shownIds: nextShown };
    setState(next);
    saveState(next);
  };

  const dismiss = (forToday: boolean): void => {
    const next = { ...state, lastShownDay: today, dismissedDay: forToday ? today : state.dismissedDay };
    markShown();
    setState(next);
    saveState(next);
    setVisible(false);
  };

  return (
    <div className="ai18-briefing" data-testid="ai18-briefing" role="status">
      <div className="ai18-bf-date">{t("amb18BriefingTitle")}</div>
      <div className="ai18-bf-row">
        <span>{today}</span>
        <span>{t("amb18BriefingWeek", { n: isoWeek(new Date()) })}</span>
      </div>
      <div className="ai18-bf-tip">{lang === "en" ? tip.en : tip.zh}</div>
      <div className="ai18-bf-actions">
        <button type="button" className="btn tiny ghost" onClick={() => dismiss(true)}>
          {t("amb18BriefingNotToday")}
        </button>
        <button type="button" className="btn tiny" onClick={() => dismiss(false)}>
          {t("amb18BriefingGotIt")}
        </button>
      </div>
      <span className="visually-hidden">{`${TIPS.length}`}</span>
    </div>
  );
}
