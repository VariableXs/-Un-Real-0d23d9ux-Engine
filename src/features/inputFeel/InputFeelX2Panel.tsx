/**
 * UNREAL-X AI-17/AI-18 · 输入手感与输入智能 2.0 面板（X04001~X04500 消费端）。
 *
 * 只读展示：把 src/features/inputFeel/ 的纯逻辑模块实例化为可视化自检面，
 * 每族一行 → 断言条数与全绿状态；不动既有 InputFeelTab 的受控设置流。
 */

import { useMemo } from "react";
import { runAi17Checks, runAi18Checks } from "../uikit/checks";

export function InputFeelX2Panel() {
  const ai17 = useMemo(() => runAi17Checks(), []);
  const ai18 = useMemo(() => runAi18Checks(), []);
  const total = ai17.entries.length + ai18.entries.length;
  const failed = ai17.failed.length + ai18.failed.length;

  return (
    <section className="if-x2-panel" aria-label="输入手感与输入智能 2.0 自检">
      <h3>输入手感与输入智能 2.0</h3>
      <p className="dim small">20 族 500 项的代表性断言自检（族0161~0180 · X04001~X04500），全部纯本地求值。</p>
      <div className="if-x2-summary" data-ok={failed === 0 ? "yes" : "no"}>
        {failed === 0 ? `全部通过 · ${total} 条断言` : `${failed} 条未通过，请重跑自检`}
      </div>
      <ul className="if-x2-list">
        {[...ai17.entries.slice(0, 8), ...ai18.entries.slice(0, 8)].map((e) => (
          <li key={e.id} className="if-x2-item">
            <code>{e.id}</code>
            <span>{e.name}</span>
            <span className={e.check() ? "ok" : "bad"}>{e.check() ? "✓" : "✗"}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}
