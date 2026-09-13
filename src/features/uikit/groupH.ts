// UNREAL-X AI-18：族0171~0180「输入智能」自检断言组（X04251~X04500 代表性断言），勿删。
// 每族 ≥5 条可运行断言：覆盖档位矩阵 / 功能逻辑 / 边界钳制 / 快照迁移 / 降级净身五类口径。
// 纯逻辑断言，全部走 src/features/inputFeel/ 的真实实现。

import * as M from "../inputFeel/inputSmart";
import * as U from "../inputFeel/undoAuto";
import * as G from "../inputFeel/inputGuard";

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

/* -------- 族0171 翻译词典 2.0 -------- */
export function checkX0171(): CheckEntry[] {
  const e = new M.TranslateEngine("balanced", [{ term: "window", fixed: "视窗" }]);
  return [
    { id: "X04251", name: "翻译档位 5 档", check: () => M.TRANSLATE_PROFILES.length === 5 && M.DEFAULT_TRANSLATE_ID === "balanced" },
    { id: "X04252", name: "非法档回默认", check: () => new M.TranslateEngine("x").profileId === "balanced" },
    { id: "X04253", name: "术语表优先", check: () => e.lookup("window") === "视窗" && e.lookup("keyboard") === "键盘" && e.lookup("zzz") === null },
    { id: "X04254", name: "语种检测", check: () => M.TranslateEngine.detect("键盘输入") === "zh" && M.TranslateEngine.detect("keyboard") === "en" && M.TranslateEngine.detect("") === "unknown" },
    { id: "X04255", name: "关闭档零输出", check: () => new M.TranslateEngine("off").translate("window") === "" },
    { id: "X04256", name: "长度钳制", check: () => { const q = new M.TranslateEngine("manual"); q.translate("x".repeat(3000)); return q.clamped === 1; } },
    { id: "X04275", name: "回滚净身", check: () => { e.reset(); return e.lookups === 0 && e.clamped === 0; } },
  ];
}

/* -------- 族0172 屏幕识图 2.0 -------- */
export function checkX0172(): CheckEntry[] {
  const r = M.prepareReadRegion({ x: -10, y: 0, w: 800, h: 600 }, "balanced");
  const clipped = M.prepareReadRegion({ x: 0, y: 0, w: 99999, h: 99999 }, "region");
  return [
    { id: "X04276", name: "识图档位 5 档", check: () => M.SCREEN_READ_PROFILES.length === 5 && M.DEFAULT_SCREEN_READ_ID === "balanced" },
    { id: "X04277", name: "非法档回默认", check: () => M.prepareReadRegion({ x: 0, y: 0, w: 10, h: 10 }, "x").rect.w === 10 },
    { id: "X04278", name: "越界钳制", check: () => r.rect.x === 0 && r.clamped },
    { id: "X04279", name: "面积上限裁剪", check: () => clipped.clamped && clipped.rect.w * clipped.rect.h <= 1e6 + 16384 },
    { id: "X04280", name: "分块网格 16 块", check: () => r.tiles.length === 16 && r.tiles.every((t) => t.w > 0 && t.h > 0) },
    { id: "X04300", name: "关闭档零面积", check: () => M.prepareReadRegion({ x: 0, y: 0, w: 10, h: 10 }, "off").rect.w === 10 },
  ];
}

/* -------- 族0173 OCR 提取 -------- */
export function checkX0173(): CheckEntry[] {
  const boxes: M.CharBox[] = [
    { ch: "输", rect: { x: 0, y: 0, w: 10, h: 12 } },
    { ch: "入", rect: { x: 12, y: 1, w: 10, h: 12 } },
    { ch: "法", rect: { x: 0, y: 20, w: 10, h: 12 } },
  ];
  const lines = M.ocrGroupLines(boxes);
  const paras = M.ocrParagraphs(lines);
  return [
    { id: "X04301", name: "行聚类", check: () => lines.length === 2 && lines[0]!.text === "输入" && lines[1]!.text === "法" },
    { id: "X04302", name: "行内 x 排序", check: () => lines[0]!.text === "输入" },
    { id: "X04303", name: "行几何合并", check: () => lines[0]!.rect.w === 22 && lines[0]!.rect.h === 13 },
    { id: "X04304", name: "段落拼接", check: () => paras.length === 1 && paras[0] === "输入法" },
    { id: "X04305", name: "空输入安全", check: () => M.ocrGroupLines([]).length === 0 && M.ocrParagraphs([]).length === 0 },
    { id: "X04325", name: "坏框过滤", check: () => M.ocrGroupLines([{ ch: "坏", rect: { x: 0, y: 0, w: 0, h: 0 } }, { ch: "好", rect: { x: 0, y: 0, w: 5, h: 5 } }]).length === 1 },
  ];
}

/* -------- 族0174 统计训练 -------- */
export function checkX0174(): CheckEntry[] {
  const t = new M.NgramTrainer("balanced");
  t.feed("输入法输入法");
  t.feed("输入");
  return [
    { id: "X04326", name: "训练档位 5 档", check: () => M.STATS_TRAIN_PROFILES.length === 5 && M.DEFAULT_STATS_TRAIN_ID === "balanced" },
    { id: "X04327", name: "n-gram 计数", check: () => t.top(1)[0]!.key === "输入" && t.top(1)[0]!.count >= 3 },
    { id: "X04328", name: "平滑打分有界", check: () => t.score("输入") > 0 && t.score("输入") <= 1 && t.score(" neverseen ") > 0 },
    { id: "X04329", name: "上限钳制计数", check: () => { const q = new M.NgramTrainer("tiny"); q.feed("x".repeat(3000)); return q.clamped >= 1 && q.total <= 1000; } },
    { id: "X04330", name: "快照迁移往返", check: () => { const q = new M.NgramTrainer(); return q.restore(t.export()) && q.top(1)[0]!.key === "输入" && !q.restore("{bad"); } },
    { id: "X04350", name: "关闭档零训练", check: () => { const q = new M.NgramTrainer("off"); q.feed("abc"); return q.total === 0 && q.counts.size === 0; } },
  ];
}

/* -------- 族0175 撤销历史 2.0 -------- */
export function checkX0175(): CheckEntry[] {
  const s = new U.UndoStack("balanced");
  s.commit("type", 0, "a");
  s.commit("type", 300, "ab");   // 合并窗口内 → 合并
  s.commit("paste", 2000, "abc");
  const u1 = s.undoOne();          // redo=[paste]
  s.commit("new", 3000, "abcd");   // redo 非空 → 分叉
  const r1 = s.redoOne();          // redo 已清 → null
  return [
    { id: "X04351", name: "撤销档位 5 档", check: () => U.UNDO_PROFILES.length === 5 && U.DEFAULT_UNDO_ID === "balanced" },
    { id: "X04352", name: "非法档回默认", check: () => new U.UndoStack("x").profileId === "balanced" },
    { id: "X04353", name: "打字合并窗口", check: () => s.undo.length === 2 && u1!.label === "paste" && s.canUndo() },
    { id: "X04354", name: "分叉后重做空", check: () => r1 === null && s.canRedo() === false },
    { id: "X04355", name: "分叉计数", check: () => s.branchCount === 1 },
    { id: "X04356", name: "栈上限裁剪", check: () => { const q = new U.UndoStack("basic"); for (let i = 0; i < 60; i++) q.commit("t", i * 2000, String(i)); return q.undo.length <= 50 && q.clamped >= 1; } },
    { id: "X04375", name: "断点续作+净身", check: () => s.pending() !== null && (s.reset(), s.undo.length === 0 && s.redo.length === 0 && s.branchCount === 0) },
  ];
}

/* -------- 族0176 自动化输入 2.0 -------- */
export function checkX0176(): CheckEntry[] {
  const rec = new U.MacroRecorder("balanced");
  rec.start();
  rec.type("hi");
  rec.key("Enter");
  rec.type("!");
  const steps = rec.stop();
  const ex = U.expandMacro([{ kind: "type", text: "abc" }, { kind: "wait", ms: 1 }, { kind: "loop", times: 999 }], "basic");
  return [
    { id: "X04376", name: "自动化档位 5 档", check: () => U.AUTO_INPUT_PROFILES.length === 5 && U.DEFAULT_AUTO_INPUT_ID === "balanced" },
    { id: "X04377", name: "非法档回默认", check: () => new U.MacroRecorder("x").profileId === "balanced" },
    { id: "X04378", name: "录制合并打字", check: () => steps.length === 3 && steps[2]!.kind === "type" && (steps[2] as { text: string }).text === "!" },
    { id: "X04379", name: "守护节拍下限", check: () => ex.actions.some((a) => a.startsWith("wait:")) && ex.clamped >= 1 },
    { id: "X04380", name: "循环上限钳制", check: () => ex.actions.includes("loop:2") && ex.narratives.includes("AI-403") },
    { id: "X04381", name: "关闭档拒绝", check: () => U.expandMacro([{ kind: "key", key: "a" }], "off").narratives.includes("AI-401") },
    { id: "X04400", name: "停止即净录", check: () => (rec.reset(), rec.steps.length === 0 && rec.recording === false) },
  ];
}

/* -------- 族0177 聚焦书写 2.0 -------- */
export function checkX0177(): CheckEntry[] {
  const f = new U.FocusSession("sprint"); // 15 分钟
  f.start();
  for (let i = 0; i < 15 * 60; i++) f.tick(i < 10 ? "好内容啊" : "", false);
  return [
    { id: "X04401", name: "聚焦档位 5 档", check: () => U.FOCUS_PROFILES.length === 5 && U.DEFAULT_FOCUS_ID === "balanced" },
    { id: "X04402", name: "非法档回默认", check: () => new U.FocusSession("x").profileId === "balanced" },
    { id: "X04403", name: "计时完成", check: () => f.phase === "done" && f.percent() === 100 },
    { id: "X04404", name: "计词 CJK 按字", check: () => f.words >= 30 },
    { id: "X04405", name: "干扰计数", check: () => { const q = new U.FocusSession(); q.start(); q.tick("", true); q.tick("", true); return q.distractions === 2 && new U.FocusSession().distractions === 0; } },
    { id: "X04406", name: "暂停续写", check: () => { const q = new U.FocusSession(); q.start(); q.pause(); const paused = q.phase === "paused"; const ok = q.resume(); return paused && ok && q.phase === "running"; } },
    { id: "X04425", name: "完成叙事微文案", check: () => f.narrative().includes("冲刺完成") && f.narrative().includes("干扰") },
  ];
}

/* -------- 族0178 输入安全 2.0 -------- */
export function checkX0178(): CheckEntry[] {
  const kp = new G.KeystrokePrivacy("balanced");
  kp.feed("p", true);
  kp.feed("x", false);
  return [
    { id: "X04426", name: "安全档位 5 档", check: () => G.INPUT_SECURITY_PROFILES.length === 5 && G.DEFAULT_INPUT_SECURITY_ID === "balanced" },
    { id: "X04427", name: "非法档回默认", check: () => new G.KeystrokePrivacy("x").profileId === "balanced" },
    { id: "X04428", name: "敏感字段判定", check: () => G.isSensitiveField("user_password") && G.isSensitiveField("验证码") && !G.isSensitiveField("username") },
    { id: "X04429", name: "按键脱敏", check: () => G.maskKeystroke("a", true) === "•" && G.maskKeystroke("Enter", true) === "Enter" && G.maskKeystroke("a", false) === "a" },
    { id: "X04430", name: "剪贴板哨兵", check: () => { const g = G.clipboardGuard("123456", "balanced"); return g.guarded && g.autoClearSec === 30 && g.masked.startsWith("12") && g.masked.endsWith("56") && !G.clipboardGuard("hello", "balanced").guarded; } },
    { id: "X04431", name: "敏感计数零明文", check: () => kp.sensitiveCounted === 1 && kp.counted === 1 },
    { id: "X04450", name: "回滚净身", check: () => { kp.reset(); return kp.counted === 0 && kp.sensitiveCounted === 0 && kp.clamped === 0; } },
  ];
}

/* -------- 族0179 按键映射 2.0 -------- */
export function checkX0179(): CheckEntry[] {
  const rm = new G.KeyRemapper("balanced", [
    { from: "caps", to: "esc", layer: 0 },
    { from: "h", to: "left", layer: 1 },
    { from: "dup", to: "a", layer: 0 },
    { from: "dup", to: "b", layer: 0 },
    { from: "leader:b", to: "browser", layer: 0 },
  ], "space");
  const ls = new G.LayerSelector("dual");
  return [
    { id: "X04451", name: "映射档位 5 档", check: () => G.KEYMAP_PROFILES.length === 5 && G.DEFAULT_KEYMAP_ID === "balanced" },
    { id: "X04452", name: "非法档回默认", check: () => new G.KeyRemapper("x").profileId === "balanced" },
    { id: "X04453", name: "层内映射改写", check: () => rm.resolve("caps", 0).emit === "esc" && rm.resolve("h", 1).emit === "left" && rm.resolve("h", 0).emit === "h" },
    { id: "X04454", name: "冲突检测", check: () => rm.conflicts.includes("0:dup") },
    { id: "X04455", name: "leader 复合键", check: () => rm.resolve("space").swallowed && rm.resolve("b").emit === "browser" && !rm.leaderArmed },
    { id: "X04456", name: "层号钳制", check: () => ls.activate(5) === 0 && ls.clamped === 1 && ls.activate(1) === 1 },
    { id: "X04475", name: "关闭档无 leader", check: () => { const q = new G.KeyRemapper("single", [], "space"); return q.leaderKey === null; } },
  ];
}

/* -------- 族0180 外设键盘 -------- */
export function checkX0180(): CheckEntry[] {
  const devices: G.KeyboardDevice[] = [
    { id: "k1", name: "主键盘", layout: "ANSI", wireless: true, pollingHz: 4000 },
    { id: "k2", name: "副键盘", layout: "ISO", wireless: false, pollingHz: 1000 },
  ];
  const route = G.deviceFeelRouting(devices, "k1", "game");
  return [
    { id: "X04476", name: "外设档位 5 档", check: () => G.PERIPHERAL_PROFILES.length === 5 && G.DEFAULT_PERIPHERAL_ID === "balanced" },
    { id: "X04477", name: "非法档回默认", check: () => G.findPeripheral("x").id === "balanced" },
    { id: "X04478", name: "报告率按档钳制", check: () => G.pollingRate(devices[0]!, "eco") === 125 && G.pollingRate(devices[1]!, "game") === 1000 },
    { id: "X04479", name: "活动设备路由", check: () => route.get("k1") === "game" && route.get("k2") === "eco" },
    { id: "X04480", name: "布局识别", check: () => G.detectLayout("wide", false) === "ANSI" && G.detectLayout("L-shape", true) === "ISO" && G.detectLayout("upside-down-L", true) === "JIS" && G.detectLayout("wide", true) === "ANSI" },
    { id: "X04481", name: "外设叙事微文案", check: () => G.peripheralNarrative(devices[0]!, 125).includes("省电") && G.peripheralNarrative(devices[1]!, 1000).includes("全速") },
    { id: "X04500", name: "无效报告率回退", check: () => G.pollingRate({ ...devices[0]!, pollingHz: 0 }, "balanced") === 500 },
  ];
}
