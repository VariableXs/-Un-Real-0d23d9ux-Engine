import { useI18n } from "../i18n";

/**
 * 关闭绿灯（批次E-12）：给弹窗/浮层等"非窗口界面"补一个统一的 Mac 风格绿灯，
 * 语义 = 关闭本界面（复用 .win-btn/.win-dot 视觉）。不提供黄/红——
 * 最小化/最大化对浮层无意义，不做假功能。
 */
export function CloseLight(props: { onClose: () => void }): React.ReactElement {
  const { t } = useI18n();
  return (
    <button
      type="button"
      className="win-btn close-light"
      aria-label={t("close")}
      title={t("close")}
      onClick={props.onClose}
    >
      <span className="win-dot green" />
    </button>
  );
}
