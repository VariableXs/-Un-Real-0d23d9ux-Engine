/** N-09 灵感卡：本地固定语录随机抽取，零网络。 */
import { useState } from "react";
import { RefreshCw } from "lucide-react";
import { LABELS, useLaneLang } from "../labels";
import { useRefresh } from "./common";

const QUOTES: { zh: string; en: string }[] = [
  { zh: "简洁是终极的复杂。", en: "Simplicity is the ultimate sophistication." },
  { zh: "做出正确的事，然后把事情做对。", en: "Do the right things, then do things right." },
  { zh: "慢即是稳，稳即是快。", en: "Slow is smooth, smooth is fast." },
  { zh: "工具应当退到心流之后。", en: "Good tools disappear into flow." },
  { zh: "每一次保存都是一次承诺。", en: "Every save is a promise." },
  { zh: "留白不是空，是呼吸。", en: "Whitespace is not emptiness; it is breathing room." },
  { zh: "先完成，再完美。", en: "Make it work, then make it right." },
  { zh: "注意力是最贵的货币。", en: "Attention is the most expensive currency." },
];

export default function InspireCard({ paused }: { paused: boolean }): React.ReactElement {
  const lang = useLaneLang();
  const t = LABELS[lang];
  const [idx, setIdx] = useState(() => Math.floor(Math.random() * QUOTES.length));
  useRefresh(() => setIdx(Math.floor(Math.random() * QUOTES.length)), 15 * 60_000, paused);

  const q = QUOTES[idx] ?? QUOTES[0];
  return (
    <div className="wgt-inspire">
      <p className="wgt-inspire-text">{lang === "en" ? q?.en : q?.zh}</p>
      <button type="button" className="wgt-x" aria-label={t.shuffle} onClick={() => setIdx(Math.floor(Math.random() * QUOTES.length))}>
        <RefreshCw size={12} />
      </button>
    </div>
  );
}