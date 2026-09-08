/**
 * AI-17 · U-57 引导体系：5 站 spotlight 巡回组件。
 * 每站一个 spotlight 遮罩 + 三行说明；Esc 随时退出；目标缺失自动跳下一站。
 * 完成后 markTourDone；由 VisionRuntime 在 needsTour() 时挂载。
 */
import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { TOUR_STEPS, markTourDone } from "./onboarding";

interface Rect { left: number; top: number; width: number; height: number }

function targetRect(selector: string): Rect | null {
  const el = document.querySelector(selector) as HTMLElement | null;
  if (!el) return null;
  const r = el.getBoundingClientRect();
  if (r.width === 0 && r.height === 0) return null;
  return { left: r.left, top: r.top, width: r.width, height: r.height };
}

export function SpotlightTour({ onDone }: { onDone?: () => void }): React.ReactElement | null {
  const { t } = useI18n();
  const [stepIdx, setStepIdx] = useState(0);
  const [rect, setRect] = useState<Rect | null>(null);

  const finish = useCallback((): void => {
    markTourDone();
    onDone?.();
  }, [onDone]);

  const advance = useCallback((): void => {
    if (stepIdx + 1 >= TOUR_STEPS.length) finish();
    else setStepIdx((i) => i + 1);
  }, [stepIdx, finish]);

  // 定位当前站目标（两帧后量取，避开布局过渡）
  useEffect(() => {
    let cancelled = false;
    const step = TOUR_STEPS[stepIdx]!;
    const measure = (): void => {
      if (cancelled) return;
      const r = targetRect(step.selector);
      if (r) setRect(r);
      else advance(); // 目标不存在 → 跳下一站
    };
    const timer = window.setTimeout(measure, 60);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [stepIdx, advance]);

  // Esc 退出
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") {
        e.stopPropagation();
        finish();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [finish]);

  if (!rect) return null;
  const step = TOUR_STEPS[stepIdx]!;
  const pad = 8;
  // 卡片置于洞下方，越界则放上方
  const cardTop = rect.top + rect.height + pad + 160 < window.innerHeight
    ? rect.top + rect.height + pad
    : Math.max(8, rect.top - 160);

  return (
    <>
      <div className="spotlight-hole" style={{ left: rect.left - pad, top: rect.top - pad, width: rect.width + pad * 2, height: rect.height + pad * 2 }} />
      <div className="spotlight-card" style={{ left: Math.max(8, rect.left), top: cardTop }} role="dialog" aria-label={t(step.titleKey)}>
        <div style={{ fontWeight: 600, marginBottom: 4 }}>{t(step.titleKey)}</div>
        <div style={{ color: "var(--text-secondary)" }}>{t(step.bodyKey)}</div>
        <div style={{ display: "flex", justifyContent: "space-between", marginTop: 8 }}>
          <span className="dim" style={{ fontSize: "var(--fs-12)" }}>{stepIdx + 1} / {TOUR_STEPS.length}</span>
          <span style={{ display: "flex", gap: 8 }}>
            <button type="button" className="mi-ctl" onClick={finish} style={{ minHeight: 28 }}>
              {t("obSkip")}
            </button>
            <button type="button" className="mi-ctl mi-hover-rise" onClick={advance} style={{ minHeight: 28 }}>
              {stepIdx + 1 >= TOUR_STEPS.length ? t("obFinish") : t("obNext")}
            </button>
          </span>
        </div>
      </div>
    </>
  );
}
