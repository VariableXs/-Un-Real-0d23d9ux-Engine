/**
 * AI-J1 · 批次八面板（接线纵深的消费端）。
 * F611 滤波基准台（你的手适合哪个引擎）/ F619 长按仲裁台（声明与进度）/
 * F615 侧键和弦台（和弦清单 + 单键冲突显式化）。
 * 消费的都是纯函数引擎——面板只做参数输入与结果呈现，不复制逻辑。
 */
import React, { useState } from "react";
import { SectionCard, MiniButton } from "./MouseJ1Panels";
import { benchEngines, TREMOR_PHYSIOLOGIC, TREMOR_POSTURAL, type BenchResult } from "../mouse/filterbench";
import { holdClaimsFromRegistry, HOLD_CANCEL_RADIUS_PX } from "../mouse/longpress";
import { conflicts, CHORD_WINDOW_MS, type ChordBinding } from "../mouse/chordengine";
import { j1Store } from "../mouse/j1store";
import { pushToast } from "../../state/uiStore";

/** F611 滤波基准台：双谱型 × 双档位跑分 → 推荐（数据说话，不硬造差异）。 */
function FilterBenchPanel(): React.ReactElement {
  const [results, setResults] = useState<{ name: string; level: "light" | "strong"; r: BenchResult }[]>([]);
  const run = (): void => {
    const rows: { name: string; level: "light" | "strong"; r: BenchResult }[] = [];
    for (const [name, p] of [
      ["生理性（8-12Hz）", TREMOR_PHYSIOLOGIC],
      ["姿势性（4-8Hz）", TREMOR_POSTURAL],
    ] as const) {
      for (const level of ["light", "strong"] as const) rows.push({ name, level, r: benchEngines(p, level) });
    }
    setResults(rows);
    pushToast("success", "基准台跑分完成（种子确定性——同条件可复现）");
  };
  return (
    <div>
      <div className="j1x-btnrow">
        <MiniButton onClick={run}>运行基准（合成谱 × 双引擎）</MiniButton>
      </div>
      {results.length > 0 && (
        <table className="j1x-table">
          <thead>
            <tr>
              <th>谱型</th>
              <th>档位</th>
              <th>估计主频</th>
              <th>IIR 削减</th>
              <th>Euro 削减</th>
              <th>推荐</th>
            </tr>
          </thead>
          <tbody>
            {results.map(({ name, level, r }) => (
              <tr key={`${name}-${level}`}>
                <td>{name}</td>
                <td>{level}</td>
                <td>{r.measuredHz} Hz</td>
                <td>{Math.round(r.iirCut * 100)}%</td>
                <td>{Math.round(r.euroCut * 100)}%</td>
                <td>{r.recommend === "tie" ? "平手（选 IIR）" : r.recommend === "euro" ? "One Euro" : "IIR"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <p className="j1x-hint">推荐口径：同一合成信号喂双引擎，残余差幅 &lt;8% 判平手（此时选计算更廉的 IIR——诚实推荐不硬造差异）。实测你的手：在设置打开状态下正常使用一分钟，遥测会记录真实震颤谱。</p>
    </div>
  );
}

/** F619 长按仲裁台：registry × scale → 声明清单与最短先得预演。 */
function HoldPanel(): React.ReactElement {
  const [, tick] = useState(0);
  const lp = j1Store.get("longPress") as { scale: number; registry: Record<string, number> };
  const claims = holdClaimsFromRegistry(lp.registry ?? {}, lp.scale ?? 1).sort((a, b) => a.durationMs - b.durationMs);
  return (
    <div>
      {claims.length === 0 ? (
        <p className="j1x-hint">尚无长按声明——注册了长按动作的功能会出现在这里。</p>
      ) : (
        <table className="j1x-table">
          <thead>
            <tr>
              <th>功能</th>
              <th>生效时长（含 ×{lp.scale}）</th>
              <th>仲裁位次</th>
            </tr>
          </thead>
          <tbody>
            {claims.map((c, i) => (
              <tr key={c.id}>
                <td>{c.id}</td>
                <td>{c.durationMs} ms</td>
                <td>{i === 0 ? "① 最短先得" : `被 ① 短路（若与 ① 竞争同一按住）`}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <div className="j1x-kv">
        <span className="j1x-kv-k">移动取消半径</span>
        <span className="j1x-kv-v">{HOLD_CANCEL_RADIUS_PX}px（超出即取消——拖拽意图优先）</span>
      </div>
      <div className="j1x-btnrow">
        <MiniButton onClick={() => tick((v) => v + 1)}>刷新</MiniButton>
      </div>
    </div>
  );
}

/** F615 侧键和弦台：和弦清单 + 单键冲突显式化（和弦赢，但必须警示）。 */
function ChordPanel(): React.ReactElement {
  const [, tick] = useState(0);
  const sb = j1Store.get("sideButtons") as { global: Record<string, string>; chords?: ChordBinding[] };
  const chords = sb.chords ?? [];
  const touched = conflicts(chords);
  return (
    <div>
      {chords.length === 0 ? (
        <p className="j1x-hint">尚无侧键和弦——配置后双键组合可零硬件成本扩容动作位。</p>
      ) : (
        <table className="j1x-table">
          <thead>
            <tr>
              <th>和弦</th>
              <th>键位</th>
              <th>形态</th>
              <th>动作</th>
            </tr>
          </thead>
          <tbody>
            {chords.map((c) => (
              <tr key={c.id}>
                <td>{c.id}</td>
                <td>
                  {c.keys[0]} + {c.keys[1]}
                </td>
                <td>{c.kind === "simultaneous" ? "同时（重叠按下）" : "顺序（窗内跟进）"}</td>
                <td>{c.action}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {touched.length > 0 && (
        <div className="j1x-verdict j1x-verdict-warn">
          <strong>单键冲突显式化（章九：错误与引导）</strong>
          以下键位既有单键映射又参与和弦：{touched.join("、")}。和弦赢——这些键的单键动作将等待 {CHORD_WINDOW_MS}ms 消歧窗后才触发。
        </div>
      )}
      <div className="j1x-btnrow">
        <MiniButton onClick={() => tick((v) => v + 1)}>刷新</MiniButton>
      </div>
    </div>
  );
}

/** 批次八面板组（单一挂载点）。 */
export function V8Panels(): React.ReactElement {
  return (
    <>
      <SectionCard title="滤波基准台（你的手适合哪个引擎）" f="F611 · v8">
        <FilterBenchPanel />
      </SectionCard>
      <SectionCard title="长按仲裁台（声明清单与最短先得）" f="F619 · v8">
        <HoldPanel />
      </SectionCard>
      <SectionCard title="侧键和弦台（组合扩容与冲突警示）" f="F615 · v8">
        <ChordPanel />
      </SectionCard>
    </>
  );
}
