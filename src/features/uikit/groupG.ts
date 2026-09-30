// UNREAL-X AI-17：族0161~0170「输入手感面」自检断言组（X04001~X04250 代表性断言），勿删。
// 每族 ≥5 条可运行断言：覆盖档位矩阵 / 功能逻辑 / 边界钳制 / 快照迁移 / 降级净身五类口径。
// 纯逻辑断言，全部走 src/features/inputFeel/ 的真实实现。

import * as K from "../inputFeel/keyFeelX2";
import * as X from "../inputFeel/crossA11y";
import * as P from "../inputFeel/pointerFeel";
import * as S from "../inputFeel/softInput";

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

/* -------- 族0161 按键手感 2.0 -------- */
export function checkX0161(): CheckEntry[] {
  const pipe = new K.KeyFeelPipeline("mech");
  const bad = new K.KeyFeelPipeline("nope");
  const st = { code: "KeyA", atMs: 1000 };
  for (let i = 0; i < 40; i++) pipe.push({ code: "KeyA", atMs: i });
  return [
    { id: "X04001", name: "按键档位 5 档", check: () => K.KEY_FEEL_PROFILES.length === 5 && K.DEFAULT_KEY_FEEL_ID === "balanced" },
    { id: "X04002", name: "非法档回默认", check: () => bad.profileId === "balanced" && bad.clamped === 1 },
    { id: "X04003", name: "重复节拍计算", check: () => pipe.repeatAt(st, 0) === 1260 && pipe.repeatAt(st, 2) === 1312 },
    { id: "X04004", name: "连击缓冲溢出丢弃", check: () => pipe.droppedOldest === 40 - pipe.buffer.length && pipe.buffer.length <= pipe.bufferLimit },
    { id: "X04005", name: "快照迁移往返", check: () => { const q = new K.KeyFeelPipeline(); return q.restore(pipe.snapshot()) && q.profileId === "mech"; } },
    { id: "X04006", name: "叙事码有下一步", check: () => K.KEY_FEEL_NARRATIVES.length >= 4 && K.findKeyNarrative("KF-403").next.length > 0 && K.findKeyNarrative("KF-999").code === "KF-000" },
    { id: "X04007", name: "低配降级链", check: () => { const d = pipe.degrade(2, false); const b = pipe.degrade(8, true); return d.haptic <= 1 && b.haptic === 0 && b.pressDepth < pipe.profile.pressDepth; } },
    { id: "X04010", name: "回滚净身", check: () => { pipe.reset(); return pipe.buffer.length === 0 && pipe.narratives.length === 0 && pipe.droppedOldest === 0; } },
  ];
}

/* -------- 族0162 文本编辑手感 2.0 -------- */
export function checkX0162(): CheckEntry[] {
  const pe = new K.PairEngine(true);
  const off = new K.PairEngine(false);
  pe.type("（");
  pe.type("「");
  const skip = pe.type("」");
  return [
    { id: "X04026", name: "编辑档位 5 档", check: () => K.EDIT_FEEL_PROFILES.length === 5 && K.DEFAULT_EDIT_FEEL_ID === "balanced" },
    { id: "X04027", name: "非法档回默认", check: () => K.findEditFeel("x").id === "balanced" },
    { id: "X04028", name: "CJK 全角配对补全", check: () => skip === "\u0000skip" && pe.openStack.length === 1 && pe.openStack[0] === "（" },
    { id: "X04029", name: "关闭档不补全", check: () => off.type("（") === "" && off.openStack.length === 0 },
    { id: "X04030", name: "智能缩进档位", check: () => K.nextIndent(4, "brace-open") === 8 && K.nextIndent(8, "brace-close") === 4 && K.nextIndent(4, "brace-open", 99) === 8 + 4 && K.nextIndent(0, "brace-open", 2) === 2 },
    { id: "X04035", name: "配对栈上限钳制", check: () => { const q = new K.PairEngine(true); for (let i = 0; i < 80; i++) q.type("（"); return q.clamped >= 1 && q.openStack.length <= 64; } },
    { id: "X04050", name: "回滚净身", check: () => { pe.reset(); return pe.openStack.length === 0 && pe.clamped === 0; } },
  ];
}

/* -------- 族0163 代码输入 2.0 -------- */
export function checkX0163(): CheckEntry[] {
  const ct = new K.CompletionTrigger("balanced", ["getValue", "getValueOr", "setState", "subscribe"], 100);
  const manual = new K.CompletionTrigger("manual", ["getValue"]);
  return [
    { id: "X04051", name: "代码档位 5 档", check: () => K.CODE_INPUT_PROFILES.length === 5 && K.DEFAULT_CODE_INPUT_ID === "balanced" },
    { id: "X04052", name: "非法档回默认", check: () => new K.CompletionTrigger("x").profileId === "balanced" },
    { id: "X04053", name: "前缀建议排序", check: () => { const s = ct.suggest("get"); return s.length === 2 && s[0]!.label === "getValue" && s[0]!.score >= s[1]!.score; } },
    { id: "X04054", name: "手动档不触发", check: () => !manual.canTrigger(0) && ct.canTrigger(0) && !ct.canTrigger(50) && ct.canTrigger(200) },
    { id: "X04055", name: "Tab 跳格", check: () => ct.nextTabStop(5) === 8 && ct.nextTabStop(0) === 4 && manual.nextTabStop(5) === -1 },
    { id: "X04056", name: "建议数钳制", check: () => ct.suggest("s", 0).length === 1 && ct.suggest("s", 999).length <= 64 },
    { id: "X04070", name: "快照迁移往返", check: () => { const q = new K.CompletionTrigger(); return q.restore(ct.snapshot()) && q.profileId === "balanced" && !q.restore("{bad"); } },
  ];
}

/* -------- 族0164 跨窗输入 2.0 -------- */
export function checkX0164(): CheckEntry[] {
  const r = new X.CrossWindowRouter("balanced");
  r.setFocus("win-a");
  r.route(0);
  r.setHover("win-b");
  r.route(10);
  r.setFocus("win-b");
  const cost = r.route(25);
  return [
    { id: "X04076", name: "跨窗档位 5 档", check: () => X.CROSS_WINDOW_PROFILES.length === 5 && X.DEFAULT_CROSS_WINDOW_ID === "balanced" },
    { id: "X04077", name: "非法档回默认", check: () => new X.CrossWindowRouter("x").profileId === "balanced" },
    { id: "X04078", name: "悬停输入路由", check: () => r.routed[1]!.target === "hover" && r.routed[1]!.window === "win-b" },
    { id: "X04079", name: "粘贴目标按档解析", check: () => r.pasteTargets().includes("win-b") && new X.CrossWindowRouter("strict").pasteTargets().length === 0 },
    { id: "X04080", name: "切换成本可测", check: () => cost.window === "win-b" && r.focusSwitchCost() !== null },
    { id: "X04081", name: "无焦点窗口丢弃", check: () => { const q = new X.CrossWindowRouter("strict"); const o = q.route(0); return o.target === "dropped" && o.window === null; } },
    { id: "X04100", name: "回滚净身", check: () => { r.reset(); return r.routed.length === 0 && r.clamped === 0; } },
  ];
}

/* -------- 族0165 输入无障碍 2.0 -------- */
export function checkX0165(): CheckEntry[] {
  const sk = new X.StickyKeys(true);
  sk.press("shift");
  sk.press("ctrl");
  const combo = sk.commit("a");
  const bf = new X.BounceFilter(80);
  return [
    { id: "X04101", name: "无障碍档位 5 档", check: () => X.INPUT_A11Y_PROFILES.length === 5 && X.DEFAULT_INPUT_A11Y_ID === "balanced" },
    { id: "X04102", name: "非法档回默认", check: () => X.findInputA11y("x").id === "balanced" },
    { id: "X04103", name: "粘滞键组合", check: () => combo === "ctrl+shift+a" && X.StickyKeys.shiftAsk(5) && !X.StickyKeys.shiftAsk(4) },
    { id: "X04104", name: "筛选键防抖", check: () => bf.feed(0) && !bf.feed(10) && bf.feed(100) && bf.rejected === 1 && bf.accepted === 2 },
    { id: "X04105", name: "按键回显读屏", check: () => X.keyEcho("enter", ["shift"]) === "上档，回车" && X.keyEcho("x").includes("X") },
    { id: "X04106", name: "单手镜像映射", check: () => X.handMirror("p", "left") === "o" && X.handMirror("o", "right") === "p" && X.handMirror("q", "left") === "q" },
    { id: "X04125", name: "非法修饰键钳制", check: () => { const q = new X.StickyKeys(true); q.press("meta"); return q.clamped === 1 && q.latched.size === 0; } },
  ];
}

/* -------- 族0166 触控板手感 2.0 -------- */
export function checkX0166(): CheckEntry[] {
  const g = new P.TouchpadGestureRecognizer("full");
  g.sample(0, 0, 2, 0);
  g.sample(10, 0, 2, 10);
  g.sample(30, 0, 2, 20);
  const gesture = g.finish();
  const tp = new P.TapPressure();
  return [
    { id: "X04126", name: "触控板档位 5 档", check: () => P.TOUCHPAD_PROFILES.length === 5 && P.DEFAULT_TOUCHPAD_ID === "balanced" },
    { id: "X04127", name: "非法档回默认", check: () => new P.TouchpadGestureRecognizer("x").profileId === "balanced" },
    { id: "X04128", name: "双指滑动识别", check: () => gesture === "two-finger-scroll" && g.fired.length <= g.maxGestures },
    { id: "X04129", name: "阈值内不触发", check: () => { const q = new P.TouchpadGestureRecognizer(); q.sample(0, 0, 2, 0); q.sample(5, 5, 2, 10); return q.finish() === null; } },
    { id: "X04130", name: "惯性速度钳制", check: () => g.inertiaVelocity() >= 0 && g.inertiaVelocity() <= 20 },
    { id: "X04131", name: "压力迟滞判定", check: () => tp.feed(50) && !tp.feed(30) && !tp.feed(20) && tp.feed(60) },
    { id: "X04150", name: "手指数钳制", check: () => { const q = new P.TouchpadGestureRecognizer(); q.sample(0, 0, 9, 0); return q.clamped === 1; } },
  ];
}

/* -------- 族0167 鼠标手感 2.0 -------- */
export function checkX0167(): CheckEntry[] {
  const dbl = new P.DoubleClickWindow(450);
  const first = dbl.click(0);
  const second = dbl.click(200);
  const third = dbl.click(1000);
  return [
    { id: "X04151", name: "鼠标档位 5 档", check: () => P.MOUSE_PROFILES.length === 5 && P.DEFAULT_MOUSE_ID === "balanced" },
    { id: "X04152", name: "非法档回默认", check: () => P.findMouse("x").id === "balanced" },
    { id: "X04153", name: "加速度曲线单调", check: () => P.mouseAccel(100, "high") > P.mouseAccel(100, "medium") && P.mouseAccel(100, "medium") > P.mouseAccel(100, "off") && P.mouseAccel(-100, "high") < 0 },
    { id: "X04154", name: "双击窗口判定", check: () => first === 1 && second === 2 && third === 1 },
    { id: "X04155", name: "滚轮行数分档", check: () => P.wheelLines(3, true) === 3 && P.wheelLines(3, false) === 9 && P.wheelLines(99, true) === 12 },
    { id: "X04156", name: "摇一摇找指针", check: () => P.shakeFind([50, -50, 60, -60, 10]) && !P.shakeFind([50, 60, 70]) },
    { id: "X04175", name: "位移输入钳制", check: () => P.mouseAccel(99999, "off") === 4096 && P.mouseAccel(-99999, "off") === -4096 },
  ];
}

/* -------- 族0168 语音输入 2.0 -------- */
export function checkX0168(): CheckEntry[] {
  const s = new S.VoiceSession("balanced");
  s.start();
  s.feed("今天天气，句号 很好", 0);
  s.feed("", 2100);
  return [
    { id: "X04176", name: "语音档位 5 档", check: () => S.VOICE_PROFILES.length === 5 && S.DEFAULT_VOICE_ID === "balanced" },
    { id: "X04177", name: "非法档回默认", check: () => new S.VoiceSession("x").profileId === "balanced" },
    { id: "X04178", name: "口头标点替换", check: () => s.text.includes("，。") && S.VOICE_PUNCT_COMMANDS.length >= 6 },
    { id: "X04179", name: "静音自动收束", check: () => s.phase === "done" && s.codes.includes("VC-402") },
    { id: "X04180", name: "暂停续录", check: () => { const q = new S.VoiceSession(); q.start(); q.pause(); const ok = q.resume(); return ok && q.phase === "listening" && !new S.VoiceSession().resume(); } },
    { id: "X04181", name: "叙事码有下一步", check: () => S.VOICE_NARRATIVES.length >= 4 && S.findVoiceNarrative("VC-401").next.length > 0 && S.findVoiceNarrative("VC-9").code === "VC-000" },
    { id: "X04200", name: "静音上限钳制", check: () => { const q = new S.VoiceSession("push"); q.start(); q.feed("", -5); return q.silenceMs === 0; } },
  ];
}

/* -------- 族0169 手写输入 2.0 -------- */
export function checkX0169(): CheckEntry[] {
  const stroke: S.Stroke = [{ x: 0, y: 0 }, { x: 10, y: 40 }, { x: 20, y: 80 }, { x: 90, y: 85 }];
  const fp = S.strokeFingerprint(stroke);
  const lib = [
    { char: "人", fp: 0b101000101 },
    { char: "二", fp: 0b000101000 },
  ];
  const cands = S.handwritingCandidates(stroke, lib, 2);
  return [
    { id: "X04201", name: "手写档位 5 档", check: () => S.HANDWRITE_PROFILES.length === 5 && S.DEFAULT_HANDWRITE_ID === "balanced" },
    { id: "X04202", name: "非法档回默认", check: () => S.findHandwrite("x").id === "balanced" },
    { id: "X04203", name: "轨迹归一化", check: () => { const n = S.normalizeStroke(stroke); return n[0]!.x === 0 && n[0]!.y === 0 && n.every((p) => p.x >= 0 && p.x <= 100 && p.y >= 0 && p.y <= 100); } },
    { id: "X04204", name: "指纹九宫格", check: () => fp > 0 && fp <= 511 },
    { id: "X04205", name: "候选按距离排序", check: () => cands.length === 2 && cands[0]!.score >= cands[1]!.score },
    { id: "X04206", name: "笔迹美化简化", check: () => S.beautifyStroke(stroke, 3).length < stroke.length && S.beautifyStroke([{ x: 0, y: 0 }, { x: 1, y: 1 }], 3).length === 2 },
    { id: "X04225", name: "空轨迹安全", check: () => S.normalizeStroke([]).length === 0 && S.strokeFingerprint([]) === 0 },
  ];
}

/* -------- 族0170 表情符号 2.0 -------- */
export function checkX0170(): CheckEntry[] {
  const p = new S.EmojiPanel(8);
  p.pick("😀");
  p.pick("🚀");
  p.pick("😀");
  return [
    { id: "X04226", name: "表情目录分组", check: () => S.EMOJI_CATALOG.length >= 12 && new Set(S.EMOJI_CATALOG.map((e) => e.group)).size === 6 },
    { id: "X04227", name: "关键词搜索", check: () => p.search("笑").some((e) => e.ch === "😀") && p.search("coffee").some((e) => e.ch === "☕") },
    { id: "X04228", name: "分组过滤", check: () => p.byGroup("食物").every((e) => e.group === "食物") && p.byGroup("食物").length >= 2 },
    { id: "X04229", name: "最近使用 LRU", check: () => p.recents[0] === "😀" && p.recents.length === 2 },
    { id: "X04230", name: "上限裁剪", check: () => { const q = new S.EmojiPanel(4); for (const e of S.EMOJI_CATALOG) q.pick(e.ch); return q.recents.length === 4; } },
    { id: "X04231", name: "未知字符钳制", check: () => { q_pick(p, "🜛"); return p.clamped === 1; } },
    { id: "X04250", name: "肤色档钳制", check: () => p.skinTier(false, 3) === 0 && p.skinTier(true, 9) === 5 && p.skinTier(true, 2) === 2 },
  ];
}

function q_pick(p: S.EmojiPanel, ch: string): void {
  p.pick(ch);
}
