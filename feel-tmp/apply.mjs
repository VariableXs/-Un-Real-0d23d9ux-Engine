import { readFileSync, writeFileSync } from "node:fs";

const norm = (s) => s.replace(/\r\n/g, "\n");
const read = (p) => norm(readFileSync(p, "utf8"));
const snip = (p) => norm(readFileSync("feel-tmp/" + p, "utf8")).trimEnd();

function apply(file, ops) {
  let src = read(file);
  ops.forEach((op, i) => {
    let from, to;
    if (op.rep !== undefined) { from = op.from; to = op.rep; }
    else if (op.after !== undefined) { from = op.from; to = op.from + "\n\n" + snip(op.after); }
    else { from = op.from; to = snip(op.before) + "\n\n" + op.from; }
    const count = src.split(from).length - 1;
    if (count !== 1) {
      console.error("FAIL " + file + " op#" + i + " count=" + count + " :: " + JSON.stringify(from.slice(0, 70)));
      process.exit(1);
    }
    src = src.replace(from, () => to);
  });
  writeFileSync(file, src);
  console.log("OK " + file + " (" + ops.length + " ops)");
}

apply("src/system/windows/vwm.ts", [
  { from: "const TB_SIDE = 62;", rep: "const TB_SIDE = 62;\n/** M-02 标题栏高度（与 EmbedBridge 嵌入偏移的 38px 一致；卷帘收起后的窗高）。 */\nexport const VWM_TITLEBAR_H = 38;" },
  { from: 'import type { AppMode } from "../../state/uiStore";', rep: 'import { parseScreenDetails, screenShift } from "./winfeel";\nimport type { AppMode } from "../../state/uiStore";' },
  { from: "  groupActive: boolean;\n}", rep: "  groupActive: boolean;\n  /** M-02 卷帘：收起后仅剩标题栏高度（rolledFromH 记忆原高）。 */\n  rolledUp: boolean;\n  /** M-02 卷帘：收起前的高度（undefined = 从未收起）。 */\n  rolledFromH?: number;\n  /** M-03 最小化时刻（抽屉排序用；null = 不在最小化态）。 */\n  minimizedAt: number | null;\n  /** Z-36 不透明度（0.2..1）。 */\n  opacity: number;\n  /** Z-36 置顶（浮于普通窗口之上）。 */\n  topmost: boolean;\n}" },
  { from: '      { id, app, path, x: rect.x, y: rect.y, w: rect.w, h: rect.h, state: "normal", minimized: false, z, restore: null, group: null, groupActive: false },', rep: '      { id, app, path, x: rect.x, y: rect.y, w: rect.w, h: rect.h, state: "normal", minimized: false, z, restore: null, group: null, groupActive: false, rolledUp: false, minimizedAt: null, opacity: 1, topmost: false },' },
  { from: 'export function focusVwmWin(id: string): void {\n  const s = vwmStore.getState();\n  const w = s.wins.find((x) => x.id === id);\n  if (!w) return;\n  const z = s.topZ + 1;\n  patch((st) => ({\n    wins: st.wins.map((x) => (x.id === id ? { ...x, z, minimized: false } : x)),\n    topZ: z,\n    focusedId: id,\n  }));\n}', after: "s1-focus.txt" },
  { from: "  if (!w || s.closing.includes(id)) return;", rep: '  if (!w || s.closing.includes(id)) return;\n  // Z-37 几何记忆增强：normal 态关闭也持久化（卷帘中按记忆原高）\n  if (w.state === "normal") {\n    persistGeom(w.app, { x: w.x, y: w.y, w: w.w, h: w.rolledUp ? (w.rolledFromH ?? w.h) : w.h });\n  }' },
  { from: "    wins: st.wins.map((x) => (x.id === id ? { ...x, minimized: true } : x)),", rep: "    wins: st.wins.map((x) => (x.id === id ? { ...x, minimized: true, minimizedAt: Date.now() } : x))," },
  { from: 'export function toggleMaxVwmWin(id: string): void {\n  const s = vwmStore.getState();\n  const w = s.wins.find((x) => x.id === id);\n  if (!w) return;\n  if (w.state === "max") {', rep: 'export function toggleMaxVwmWin(id: string): void {\n  const s = vwmStore.getState();\n  let w = s.wins.find((x) => x.id === id);\n  if (!w) return;\n  if (w.rolledUp) {\n    // M-02：卷帘中先还原高度再最大化/还原\n    patch((st) => ({ wins: st.wins.map((x) => (x.id === id ? { ...x, ...unrollPatch(x) } : x)) }));\n    w = vwmStore.getState().wins.find((x) => x.id === id);\n    if (!w) return;\n  }\n  if (w.state === "max") {' },
  { from: 'export function snapVwmWin(id: string, dir: "left" | "right" | "up" | "down"): void {\n  const s = vwmStore.getState();\n  const w = s.wins.find((x) => x.id === id);\n  if (!w) return;', rep: 'export function snapVwmWin(id: string, dir: "left" | "right" | "up" | "down"): void {\n  const s = vwmStore.getState();\n  let w = s.wins.find((x) => x.id === id);\n  if (!w) return;\n  if (w.rolledUp) {\n    // M-02：卷帘中先还原高度再贴靠\n    patch((st) => ({ wins: st.wins.map((x) => (x.id === id ? { ...x, ...unrollPatch(x) } : x)) }));\n    w = vwmStore.getState().wins.find((x) => x.id === id);\n    if (!w) return;\n  }' },
  { from: 'export function snapVwmRect(id: string, rect: VwmRect): void {\n  const s = vwmStore.getState();\n  const w = s.wins.find((x) => x.id === id);\n  if (!w) return;', rep: 'export function snapVwmRect(id: string, rect: VwmRect): void {\n  const s = vwmStore.getState();\n  let w = s.wins.find((x) => x.id === id);\n  if (!w) return;\n  if (w.rolledUp) {\n    // M-02：卷帘中先还原高度再贴靠\n    patch((st) => ({ wins: st.wins.map((x) => (x.id === id ? { ...x, ...unrollPatch(x) } : x)) }));\n    w = vwmStore.getState().wins.find((x) => x.id === id);\n    if (!w) return;\n  }' },
  { from: "export function setVwmSnapPreview(r: VwmRect | null): void {", before: "s2-roll.txt" },
  { from: '  patch({ wins: s.wins.map((w) => ({ ...w, minimized: true })), focusedId: null });\n}', after: "s3-minorder.txt" },
  { from: '  if (w && w.state === "normal") persistGeom(w.app, { x: w.x, y: w.y, w: w.w, h: w.h });\n}', after: "s4-restoregeom.txt" },
  { from: "  if (target) focusVwmWin(target.id);\n}", after: "s5-cyclefiltered.txt" },
  { from: "  return !w.group || w.groupActive;\n}", after: "s6-tail.txt" },
]);

apply("src/system/windows/snapshots.ts", [
  { from: "      restore: null,\n      group: null,\n      groupActive: false,\n    };", rep: "      restore: null,\n      group: null,\n      groupActive: false,\n      rolledUp: false,\n      minimizedAt: null,\n      opacity: 1,\n      topmost: false,\n    };" },
]);

apply("src/system/windows/__tests__/snapshots.test.ts", [
  { from: "      restore: null,\n      group: null,\n      groupActive: false,\n    })),", rep: "      restore: null,\n      group: null,\n      groupActive: false,\n      rolledUp: false,\n      minimizedAt: null,\n      opacity: 1,\n      topmost: false,\n    }))," },
]);

console.log("ALL DONE");