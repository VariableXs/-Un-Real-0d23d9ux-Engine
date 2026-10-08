/**
 * AURORA-10000 · AI-11~AI-15 · 族0075 仪式卡 overlay。
 * 打开来源：
 * - RitualRunner 到期派发 aurora-w2:ritual → 转发为 ai04:open-feature；
 * - 设计中心「试播仪式卡」直接 open-feature（entryId 可选）。
 */
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { useLaneLang, LABELS } from "./labels";
import { dispatchClose, installCloseHandler } from "../wallpaper/mount";
import { findEntry } from "./catalog";
import { takePendingRitual } from "./ritualBus";

interface RitualCopy {
  zh: string;
  en: string;
  celebrate: boolean;
}

const COPY: Record<string, RitualCopy> = {
  F01851: { zh: "新的一天开始了，今天想先完成哪一件事？", en: "A new day — what will you do first?", celebrate: false },
  F01852: { zh: "周五快乐，这周辛苦了 🎉", en: "Happy Friday — week well done 🎉", celebrate: true },
  F01853: { zh: "生日快乐！这是属于你的一天 🎂", en: "Happy birthday — today is yours 🎂", celebrate: true },
  F01854: { zh: "连续陪伴 100 天，谢谢你 🏅", en: "100 days together — thank you 🏅", celebrate: true },
  F01855: { zh: "夜深了，早点休息。", en: "It's late — rest well.", celebrate: false },
  F01856: { zh: "清晨第一杯水，唤醒身体 💧", en: "First glass of water 💧", celebrate: false },
  F01861: { zh: "成就达成！🎉", en: "Achievement unlocked! 🎉", celebrate: true },
  F01865: { zh: "新年倒数中… ✨", en: "Counting down to the new year… ✨", celebrate: true },
  F01867: { zh: "开工大吉，新年新起点。", en: "Back to work — fresh start.", celebrate: false },
  F01868: { zh: "项目完工，撒花！🎀", en: "Project shipped — confetti! 🎀", celebrate: true },
  F01870: { zh: "咖啡时间 ☕，起来走走。", en: "Coffee break ☕ — take a walk.", celebrate: false },
  F01871: { zh: "下午茶时间，放松一下 🍵", en: "Tea break 🍵 — relax a bit.", celebrate: false },
  F01872: { zh: "午休模式已开启，安静一会儿。", en: "Lunch-break mode on.", celebrate: false },
  F01873: { zh: "晚安，屏幕会慢慢变暗。", en: "Goodnight — the screen will dim.", celebrate: false },
  F01874: { zh: "晨读模式：专注的一个小时。", en: "Morning focus hour.", celebrate: false },
};

export function RitualOverlay(): React.JSX.Element {
  const lang = useLaneLang();
  const L = LABELS[lang];
  const [entryId] = useState<string | null>(() => takePendingRitual());

  useEffect(() => {
    const off = installCloseHandler(window, "design-ritual");
    return () => {
      off();
    };
  }, []);

  const ent = entryId ? findEntry(entryId) : null;
  const copy: RitualCopy | null = entryId ? COPY[entryId] ?? null : null;
  const text = copy ? (lang === "en" ? copy.en : copy.zh) : ent ? ent.label[lang] : "";

  return (
    <div className={`w2-ritual${copy?.celebrate ? " w2-ritual-celebrate" : ""}`} role="dialog" aria-label={ent ? ent.label[lang] : L.ritualTest}>
      <button type="button" className="w2-ritual-close" aria-label={L.close} onClick={() => dispatchClose("design-ritual")}>
        <X size={14} aria-hidden="true" />
      </button>
      <p className="w2-ritual-text">{text}</p>
      {ent && <p className="w2-ritual-id">{ent.id} · {L.ritualTest}</p>}
    </div>
  );
}
