/**
 * AI-20 M-83：默认键位表转储（keymap-audit.cjs 的数据源）。
 *
 * 以 vite-node 执行（TS 直跑），输出单行 JSON：
 *   { actions: [{id, accel}], reserved: [combo...] }
 * 供 keymap-audit.cjs 做功能冲突（error）与系统保留键重叠（warn）判定。
 */
import { SHORTCUT_ACTIONS } from "../src/lib/shortcuts";
import { SYSTEM_RESERVED } from "../src/lib/keymap/system-combos";

console.log(
  JSON.stringify({
    actions: SHORTCUT_ACTIONS.map((a) => ({ id: a.id, accel: a.accel })),
    reserved: [...SYSTEM_RESERVED],
  }),
);
