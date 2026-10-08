/**
 * UNREAL-X AI-02：PowerTheaterTab.tsx — 电源状态剧场设置页（族0011~0020 · X00251~X00500）。
 * 值持久化到 settings.powerTheater（键值白名单钳制在 settings.ts）。
 * 样式全部走既有 class + design/tokens.css 语义 token（var(--sp-*) / var(--px2-veil-*)），不写裸值。
 */
import { useRef, useState } from "react";
import type { Settings } from "../../lib/settings";
import { POWER_KEYS, POWER_VALUE_SETS, type PowerKey } from "../../lib/settings";
import { runAi02Checks } from "../uikit/checks";
import * as T from "../oobe/powerTheater";
import * as H from "./hiberArchive";
import * as E from "./powerEvents";
import * as G from "./powerGauge";
import * as W from "./warmupPlan";
import * as Q from "./bootQuiet";
import * as K from "./powerEggs";
import * as A from "../ambience/powerX2";

const KEY_META: Record<PowerKey, { title: string; desc: string }> = {
  ceremony: { title: "族0011 关机重启仪式 2.0", desc: "五档：直通/轻快/均衡/影院/告别式；off = 现状。可取消、断点可续作。" },
  wake: { title: "族0012 睡眠唤醒剧场 2.0", desc: "五档：瞬醒/轻快/均衡/黎明/日出；veil 档有揭幕氛围。" },
  hiber: { title: "族0013 休眠档案", desc: "休眠会话快照（应用集+布局），导出/导入/一键恢复。" },
  events: { title: "族0016 电源事件", desc: "事件流记录 + 订阅；下面列表展示最近事件。" },
  gauge: { title: "族0017 性能仪表 2.0", desc: "电量/处理器/热度/续航四通道采样与健康度。" },
  warmup: { title: "族0018 预热编排", desc: "唤醒后按优先级逐 tick 执行预热任务；低电量自动挂起。" },
  quiet: { title: "族0019 启动降噪", desc: "四档：关闭（现状）/轻柔/静谧/全静；窗口 0~300s。" },
  eggs: { title: "族0020 彩蛋层", desc: "仪式彩蛋附加行，默认关，可单独屏蔽。" },
};

export function PowerTheaterTab(props: {
  settings: Settings;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const sel = props.settings.powerTheater ?? {};
  const set = (key: PowerKey, value: string): void => {
    props.onPatch({ powerTheater: { ...sel, [key]: value } });
  };

  const runner = useRef(new T.CeremonyRunner());
  const [, forceRender] = useState(0);
  const rerender = (): void => forceRender((n) => n + 1);
  const logRef = useRef<H.HiberArchive>(new H.HiberArchive());
  const eventsRef = useRef<E.PowerEventStream>(new E.PowerEventStream());
  const gaugeRef = useRef<G.PowerGauge>(new G.PowerGauge());
  const warmupRef = useRef<W.WarmupPlanner>(new W.WarmupPlanner());
  const eggsRef = useRef<K.EggKeeper>(new K.EggKeeper());

  const selfCheck = (): string => {
    const { entries, failed } = runAi02Checks();
    return failed.length === 0
      ? `自检通过：${entries.length} 项全绿`
      : `自检失败：${failed.map((f) => f.id).join("、")}`;
  };

  /* 族0011：仪式演练（tick 驱动，可暂停/取消/续作）。 */
  const ceremonyStep = (): void => {
    const r = runner.current;
    if (r.phase === "idle" || r.phase === "done" || r.phase === "canceled") {
      runner.current = new T.CeremonyRunner("shutdown", sel.ceremony || "balanced");
      runner.current.start();
    }
    runner.current.tick();
    if (runner.current.phase === "done") {
      eggsRef.current.celebrate("shutdown");
      eventsRef.current.record("shutdown", Date.now(), T.ceremonyPercent(runner.current) + "%");
    }
    rerender();
  };

  /* 族0013：休眠档案登记/恢复/导出。 */
  const hiberLog = (): void => {
    logRef.current.snapshot("演示档案", [
      { id: "files", focus: 9 },
      { id: "editor", focus: 7 },
    ], { preset: "flow", columns: 2 });
    eventsRef.current.record("sleep", Date.now(), "档案已登记");
    rerender();
  };

  /* 族0016/0017：事件与采样演练。 */
  const sampleGauge = (): void => {
    gaugeRef.current.sample(Date.now(), { battery: 64, cpu: 42, thermal: 55, runtime: 60 });
    eventsRef.current.record("wake", Date.now(), "演练采样");
    rerender();
  };

  const quiet = new Q.BootQuiet(sel.quiet || "off");

  return (
    <div className="tab-body">
      <p className="dim small">
        电源状态剧场（X00251~X00500）：全部默认「关闭/现状」，选择后下次电源动作生效。
      </p>
      {POWER_KEYS.map((key) => {
        const meta = KEY_META[key];
        const options = POWER_VALUE_SETS[key];
        const current = sel[key] ?? "";
        const isSwitch = options.join() === "off,on";
        return (
          <div key={key} className="field">
            <span className="field-label">
              {meta.title}
              <span className="dim small"> · {meta.desc}</span>
            </span>
            <select
              value={current}
              onChange={(e) => set(key, e.target.value)}
              aria-label={`选择${meta.title}档位`}
              style={{ width: "100%" }}
            >
              <option value="">关闭（默认）</option>
              {options.filter((v) => v !== "off").map((v) => (
                <option key={v} value={v}>
                  {isSwitch ? (v === "on" ? "开" : "关") : v}
                </option>
              ))}
            </select>
          </div>
        );
      })}

      {/* 族0011：仪式演练台 */}
      <div className="field">
        <span className="field-label">
          仪式演练台
          <span className="dim small"> · 阶段推进/暂停/取消/续作，进度 {T.ceremonyPercent(runner.current)}%</span>
        </span>
        <div className="row" style={{ display: "flex", gap: "var(--sp-2)", flexWrap: "wrap" }}>
          <button type="button" onClick={ceremonyStep}>推进一拍</button>
          <button type="button" onClick={() => { runner.current.pause(); rerender(); }}>暂停</button>
          <button type="button" onClick={() => { runner.current.cancel(); rerender(); }}>取消</button>
          <button type="button" onClick={() => { runner.current.resume(); rerender(); }}>续作</button>
        </div>
        <span className="dim small" aria-live="polite">
          {runner.current.stageCaption()} · {runner.current.phase}
        </span>
      </div>

      {/* 族0013：休眠档案 */}
      <div className="field">
        <span className="field-label">
          休眠档案操作
          <span className="dim small"> · {logRef.current.caption()}</span>
        </span>
        <div className="row" style={{ display: "flex", gap: "var(--sp-2)", flexWrap: "wrap" }}>
          <button type="button" onClick={hiberLog}>登记快照</button>
          <button type="button" onClick={() => { logRef.current.resume(); rerender(); }}>续作半成品</button>
          <button type="button" onClick={() => { logRef.current.wipe(); rerender(); }}>净身</button>
        </div>
      </div>

      {/* 族0016/0017：事件与仪表 */}
      <div className="field">
        <span className="field-label">
          电源事件 · 性能仪表
          <span className="dim small"> · {G.GAUGE_CHANNELS.map((c) => G.GAUGE_BUDGETS[c].label).join("/")}四通道</span>
        </span>
        <button type="button" onClick={sampleGauge}>采样一次</button>
          {gaugeRef.current.samples.length > 0 ? (
            <span className="dim small">{gaugeRef.current.caption()}</span>
          ) : undefined}
        <ul className="dim small" aria-label="电源事件列表">
          {eventsRef.current.query().slice(0, 5).map((e, i) => (
            <li key={`${e.stamp}-${i}`}>{E.PowerEventStream.line(e)}</li>
          ))}
        </ul>
      </div>

      {/* 族0018/0019：预热与降噪摘要 */}
      <div className="field">
        <span className="field-label">
          预热编排 · 启动降噪
          <span className="dim small">
            · 队列 {warmupRef.current.queue.length} 项 · {quiet.caption()}
          </span>
        </span>
        <div className="row" style={{ display: "flex", gap: "var(--sp-2)", flexWrap: "wrap" }}>
          <button type="button" onClick={() => { warmupRef.current.enqueue({ id: "net", type: "network", priority: 1, budgetMs: 300 }); warmupRef.current.begin(); rerender(); }}>入队网络预热</button>
          <button type="button" onClick={() => { warmupRef.current.tick(); rerender(); }}>执行一拍</button>
        </div>
        <span className="dim small">
          降噪决策示例——{quiet.decision("notify").reason}
        </span>
      </div>

      {/* 族0020：彩蛋 */}
      <div className="field">
        <span className="field-label">
          彩蛋层预览
          <span className="dim small"> · {K.POWER_EGGS.length} 枚彩蛋，默认关闭</span>
        </span>
        <button type="button" onClick={() => { eggsRef.current.celebrate("wake"); rerender(); }}>
          演练唤醒彩蛋
        </button>
        {eggsRef.current.lineFor("wake") ? (
          <span className="dim small">{eggsRef.current.lineFor("wake")}</span>
        ) : undefined}
      </div>

      <div className="field">
        <span className="field-label">
          氛围令牌
          <span className="dim small"> · {A.ambientCaption("shutdown")}</span>
        </span>
        <span className="dim small">{A.ambientToken(2)} → var(--px2-veil-l2)</span>
      </div>

      <div className="field">
        <span className="field-label">
          AI-02 自检
          <span className="dim small"> · 运行电源剧场断言组（只增不删）</span>
        </span>
        <button type="button" onClick={() => window.alert(selfCheck())}>运行自检</button>
      </div>
    </div>
  );
}
