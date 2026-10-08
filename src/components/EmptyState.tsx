/**
 * AI-17 · U-55 空状态设计系统（Empty States）
 * 空状态三要素规范：
 *   ① 单色线性微插画（24px 网格、单色 accent 描边，杜绝卡通彩色位图）
 *   ② 一句话现状描述（诚实）
 *   ③ 一个主行动按钮
 * 插画复用 U-10 图标语言网格（iconRegistry 语义图标，描边体系同族）。
 * 调用方传入已 i18n 化文案（t() 结果），本组件不持词典——保证 audit 键覆盖。
 */
import { resolveIcon, type IconSemantic } from "../lib/iconRegistry";
import { useI18n } from "../i18n";

export function EmptyState(props: {
  icon?: IconSemantic;
  title: string;
  description?: string;
  actionLabel?: string;
  onAction?: () => void;
}): React.ReactElement {
  const { t } = useI18n();
  const Icon = resolveIcon(props.icon ?? "info");
  return (
    <div className="empty-state" role="status">
      <div className="empty-state-art" aria-hidden="true">
        <span className="empty-state-ring">
          <Icon size={24} strokeWidth={1.5} />
        </span>
      </div>
      <div className="empty-state-title">{props.title}</div>
      {props.description ? <div className="empty-state-desc">{props.description}</div> : null}
      {props.actionLabel && props.onAction ? (
        <button type="button" className="empty-state-action mi-ctl mi-hover-rise" onClick={props.onAction}>
          {props.actionLabel}
        </button>
      ) : null}
      <span className="sr-only">{t("emptyStateAnnounce")}</span>
    </div>
  );
}
