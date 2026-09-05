import { useEffect, useRef, useState } from "react";
import { Camera, Mic, X } from "lucide-react";
import { isTauriRuntime } from "../../entries/runtime";
import { ipc } from "../../lib/ipc";
import { pushNotify, useDnd } from "../../state/notifyStore";
import { useI18n } from "../../i18n";

/**
 * 隐私占用横幅（M5 → 批次C，隐私核心）：
 * - 轮询本机 ConsentStore（与 Windows 隐私仪表盘同源，零网络），检测摄像头/麦克风被谁占用；
 * - 新出现占用 → 顶部横幅 + 写入通知中心历史；
 * - 勿扰模式（规格 6.5.3）横幅静默 —— 但历史照常记录，通知中心仍可查看；
 * - "知道了"只收起当前这条横幅，占用仍在时托盘/通知中心仍可看到历史；
 * - 占用方名称来自系统数据（包族名或可执行文件名），不做任何改写。
 */
export function PrivacyBanner(): React.ReactElement | null {
  const { t } = useI18n();
  const dnd = useDnd();
  const [usage, setUsage] = useState<{ kind: "microphone" | "webcam"; app: string }[]>([]);
  const [dismissed, setDismissed] = useState<Set<string>>(new Set());
  const knownRef = useRef<Set<string>>(new Set());
  const firstRef = useRef(true);

  useEffect(() => {
    if (!isTauriRuntime()) return;
    let alive = true;
    const poll = (): void => {
      void ipc
        .privacyUsage()
        .then((list) => {
          if (!alive) return;
          setUsage(list);
          const keys = new Set(list.map((u) => `${u.kind}:${u.app}`));
          for (const u of list) {
            const key = `${u.kind}:${u.app}`;
            if (!firstRef.current && !knownRef.current.has(key)) {
              pushNotify(
                "privacy",
                u.kind === "microphone" ? t("privacyMicInUse") : t("privacyCamInUse"),
                `${t("privacyBy")} ${u.app}`,
              );
            }
          }
          knownRef.current = keys;
          firstRef.current = false;
        })
        .catch(() => {
          /* 无后端/读注册表失败：如实不显示，不伪造 */
        });
    };
    poll();
    const id = window.setInterval(poll, 5000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [t]);

  const visible = usage.filter((u) => !dismissed.has(`${u.kind}:${u.app}`));
  if (visible.length === 0 || dnd) return null;

  return (
    <div className="privacy-banner" role="alert">
      {visible.map((u) => {
        const Icon = u.kind === "microphone" ? Mic : Camera;
        return (
          <div key={`${u.kind}:${u.app}`} className="privacy-item">
            <Icon size={16} strokeWidth={1.8} className="privacy-icon" />
            <div className="privacy-text">
              <b>{u.kind === "microphone" ? t("privacyMicInUse") : t("privacyCamInUse")}</b>
              <span className="dim small">{`${t("privacyBy")} ${u.app}`}</span>
            </div>
            <button
              type="button"
              className="privacy-dismiss"
              aria-label={t("privacyDismiss")}
              title={t("privacyDismiss")}
              onClick={() =>
                setDismissed((prev) => new Set(prev).add(`${u.kind}:${u.app}`))
              }
            >
              <X size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
