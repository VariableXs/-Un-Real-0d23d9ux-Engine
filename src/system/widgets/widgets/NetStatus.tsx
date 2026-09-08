/** N-09 网络状态：navigator.onLine（只读，零网络探测）。 */
import { useEffect, useState } from "react";
import { LABELS, useLaneLang } from "../labels";

export default function NetStatus({ paused }: { paused: boolean }): React.ReactElement {
  const lang = useLaneLang();
  const t = LABELS[lang];
  const [online, setOnline] = useState(() => (typeof navigator === "undefined" ? true : navigator.onLine));
  void paused; // 事件驱动，无轮询

  useEffect(() => {
    const on = (): void => setOnline(true);
    const off = (): void => setOnline(false);
    window.addEventListener("online", on);
    window.addEventListener("offline", off);
    return () => {
      window.removeEventListener("online", on);
      window.removeEventListener("offline", off);
    };
  }, []);

  return (
    <div className="wgt-net">
      <span className={`wgt-net-dot ${online ? "on" : ""}`} />
      <strong>{online ? t.online : t.offline}</strong>
      <span className="wgt-dim tiny">{t.net}</span>
    </div>
  );
}