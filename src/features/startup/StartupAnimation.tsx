import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";

/**
 * Non-blocking launch animation (1.8 s). Never blocks input: it is a pure
 * overlay that fades out on click, key press, or timeout. Disabled by
 * settings, reduced motion, or safe mode.
 */
export function StartupAnimation(props: { enabled: boolean }): React.ReactElement | null {
  const { lang } = useI18n();
  const [gone, setGone] = useState(!props.enabled);

  useEffect(() => {
    if (!props.enabled) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setGone(true);
      return;
    }
    const timer = setTimeout(() => setGone(true), 1800);
    const skip = () => setGone(true);
    window.addEventListener("keydown", skip, { once: true });
    return () => {
      clearTimeout(timer);
      window.removeEventListener("keydown", skip);
    };
  }, [props.enabled]);

  if (gone) return null;
  return (
    <div className="startup-overlay" onClick={() => setGone(true)} role="presentation">
      <div className="startup-stars" aria-hidden>
        {Array.from({ length: 40 }, (_, i) => (
          <i
            key={i}
            style={{
              left: `${(i * 37.7 + 13) % 100}%`,
              top: `${(i * 53.3 + 7) % 100}%`,
              animationDelay: `${(i % 10) * 0.18}s`,
              opacity: 0.25 + ((i * 17) % 60) / 100,
            }}
          />
        ))}
      </div>
      <div className="startup-nebula" aria-hidden />
      <div className="startup-title" aria-label="Variable">
        <span>V</span>
        <em>Variable</em>
        <small>{lang === "zh" ? "私人空间" : "Private Space"}</small>
      </div>
      <div className="startup-skip">{lang === "zh" ? "点击任意处跳过" : "Click anywhere to skip"}</div>
    </div>
  );
}
