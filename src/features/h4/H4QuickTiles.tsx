/**
 * H4 快速设置磁贴——渲染壳（v9）：
 * QuickPanel 磁贴行内的 H4 磁贴组（DemoModeButton 同款自包含先例——一行挂载零侵入）。
 * 全部状态来自引擎（quickTiles 纯模型 + f387 引擎持久化），壳零自研逻辑；
 * 灰度态订阅 h4Bus 跟随全系统变更（设置中心/快捷键路径的变更同样即时反映）。
 */

import React from "react";
import { Contrast, Pipette, Ruler } from "lucide-react";
import * as grayscale from "../../system/h4/f387-grayscaleMode";
import { defaultStore } from "../../system/h4/internal/store";
import { h4Bus } from "./bus";
import { buildQuickTiles, toggleGrayscale } from "./quickTiles";

export function H4QuickTiles(): React.ReactElement {
  const [now, setNow] = React.useState(() => grayscale.activeFilter(defaultStore()));

  // 灰度态跟随全系统变更（互斥替换/关闭的任何路径都广播 f387.filter-changed）
  React.useEffect(() => h4Bus.on("f387.filter-changed", (p) => {
    const pl = p as { now?: grayscale.ColorFilter } | null;
    if (pl?.now) setNow(pl.now);
  }), []);

  const tiles = buildQuickTiles(now);

  const onTile = (t: (typeof tiles)[number]): void => {
    if (t.id === "grayscale") {
      toggleGrayscale(defaultStore()); // 内部已广播 bus——磁贴态经上面的订阅自动跟随
      return;
    }
    if (t.summon) {
      h4Bus.emit(t.summon, {}); // v10 通道唯一化：呼出走契约总线
    }
  };

  return (
    <>
      {tiles.map((t) => (
        <button key={t.id} type="button" className={`qp-tile${t.pressed ? " on" : ""}`} aria-pressed={t.pressed} title={t.label} onClick={() => onTile(t)}>
          {t.id === "grayscale" && <Contrast size={17} strokeWidth={1.8} />}
          {t.id === "picker" && <Pipette size={17} strokeWidth={1.8} />}
          {t.id === "ruler" && <Ruler size={17} strokeWidth={1.8} />}
          <span>{t.label}</span>
        </button>
      ))}
    </>
  );
}
