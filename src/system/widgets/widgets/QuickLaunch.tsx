/** N-09 快捷启动列：四个官方软件入口（复用 variable:notify-action → open-app 既有契约）。 */
import { LABELS, useLaneLang } from "../labels";

const APPS: { mode: string; zh: string; en: string; glyph: string }[] = [
  { mode: "write", zh: "写作", en: "Write", glyph: "W" },
  { mode: "mindmap", zh: "脑图", en: "Mind", glyph: "M" },
  { mode: "project", zh: "项目", en: "Project", glyph: "P" },
  { mode: "fate", zh: "命理", en: "Fate", glyph: "F" },
];

export default function QuickLaunch({ paused }: { paused: boolean }): React.ReactElement {
  const lang = useLaneLang();
  const t = LABELS[lang];
  void paused;

  const open = (mode: string): void => {
    // DesktopShell 已监听 variable:notify-action（open-app → openVwmApp）。
    window.dispatchEvent(
      new CustomEvent("variable:notify-action", {
        detail: { label: t.openApp, type: "open-app", data: mode },
      }),
    );
  };

  return (
    <div className="wgt-quick">
      {APPS.map((a) => (
        <button key={a.mode} type="button" className="wgt-quick-btn" onClick={() => open(a.mode)} title={lang === "en" ? a.en : a.zh}>
          <span className="wgt-quick-glyph">{a.glyph}</span>
          <span className="wgt-quick-label">{lang === "en" ? a.en : a.zh}</span>
        </button>
      ))}
    </div>
  );
}