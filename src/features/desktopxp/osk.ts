/**
 * F110 屏幕键盘 · 前端逻辑（C 桌面体验域·后段 · AI-D2）。
 *
 * 判据（主册 G-C-40）：全键位点击输入全对；半透明态下层内容可读；
 * 学习模式同步高亮实测。
 *
 * 移植声明：`kernel/varix/src/stard/osk.rs` 的 TS 同构移植——104 键等效
 * 布局（功能排 13/主区 61/导航区 13/小键盘 17）、符号层 Shift⊕Caps、
 * 修饰键粘滞、学习模式、多点并发 held 集，逐项对齐。码位唯一。
 */

/** 窗口尺寸（px）。 */
export const WINDOW_W_PX = 900;
export const WINDOW_H_PX = 320;
/** 触屏标准热区（px）。 */
export const KEY_MIN_PX = 44;
/** 按键字体（px）。 */
export const KEY_FONT_PX = 16;
/** 透明度默认（90% 不透明——30% 可读下限红线在渲染层校验）。 */
export const OPACITY_DEFAULT = 90;
/** 可读下限（低于 30% 视为不可读——诚实拒绝）。 */
export const OPACITY_FLOOR = 30;
/** 104 键等效总数。 */
export const FULL_LAYOUT_KEYS = 104;
/** 小键盘码位段起点（104 键布局最后 17 键——numpad 门按码位段判定，
 *  与内核模型面同语义：小键盘数字键 base 是裸数字，与主区同形不同位）。 */
export const NUMPAD_BASE_CODE = FULL_LAYOUT_KEYS - 17 + 1;

export type KeyKind =
  | "char" | "shift" | "ctrl" | "alt" | "win"
  | "capsLock" | "numLock" | "scrollLock" | "tab" | "enter" | "backspace"
  | "space" | "esc" | "fnRow" | "arrow" | "menu";

/** 一枚虚拟键。 */
export interface VKey {
  code: number;
  kind: KeyKind;
  /** 主层符号（Shift 前）。 */
  base: string;
  /** 符号层符号（Shift 态翻转显示——"" 表示无变化）。 */
  shifted: string;
  w: number;
  h: number;
}

/** 当前显示符号（Shift⊕Caps 组合态）。 */
export function keyLabel(k: VKey, shift: boolean, caps: boolean): string {
  const letters = /^[a-z]$/.test(k.base);
  const active = letters ? shift !== caps : shift; // 字母键 Caps 翻转与 Shift 取异或
  const shiftedDisplay = letters ? k.base.toUpperCase() : k.shifted;
  return active && shiftedDisplay !== "" ? shiftedDisplay : k.base;
}

/** 热区达标（触屏 ≥44px）。 */
export function touchOk(k: VKey): boolean {
  return k.w >= KEY_MIN_PX && k.h >= KEY_MIN_PX;
}

const ROW_NUM: Array<[string, string]> = [
  ["`", "~"], ["1", "!"], ["2", "@"], ["3", "#"], ["4", "$"], ["5", "%"], ["6", "^"], ["7", "&"],
  ["8", "*"], ["9", "("], ["0", ")"], ["-", "_"], ["=", "+"],
];
const ROW_Q: Array<[string, string]> = [
  ["q", "Q"], ["w", "W"], ["e", "E"], ["r", "R"], ["t", "T"], ["y", "Y"], ["u", "U"], ["i", "I"],
  ["o", "O"], ["p", "P"], ["[", "{"], ["]", "}"],
];
const ROW_A: Array<[string, string]> = [
  ["a", "A"], ["s", "S"], ["d", "D"], ["f", "F"], ["g", "G"], ["h", "H"], ["j", "J"], ["k", "K"],
  ["l", "L"], [";", ":"], ["'", "\""],
];
const ROW_Z: Array<[string, string]> = [
  ["z", "Z"], ["x", "X"], ["c", "C"], ["v", "V"], ["b", "B"], ["n", "N"], ["m", "M"],
  [",", "<"], [".", ">"], ["/", "?"],
];

function mk(code: { v: number }, kind: KeyKind, base: string, shifted: string, w: number): VKey {
  return { code: code.v++, kind, base, shifted, w: Math.max(w, KEY_MIN_PX), h: KEY_MIN_PX };
}

/** 104 键布局（等效——功能排/主区/编辑导航区/小键盘全位，码位唯一）。 */
export function fullLayout(): VKey[] {
  const v: VKey[] = [];
  const c = { v: 1 };
  // 功能排：Esc + F1-F12（13）。
  v.push(mk(c, "esc", "Esc", "", 60));
  for (let i = 1; i <= 12; i++) v.push(mk(c, "fnRow", `F${i}`, "", 44));
  // 主区第一排：数字 13 + Backspace（14）。
  for (const [b, s] of ROW_NUM) v.push(mk(c, "char", b, s, 44));
  v.push(mk(c, "backspace", "⌫", "", 88));
  // 主区第二排：Tab + 12 + \ + Enter（15）。
  v.push(mk(c, "tab", "Tab", "", 66));
  for (const [b, s] of ROW_Q) v.push(mk(c, "char", b, s, 44));
  v.push(mk(c, "char", "\\", "|", 44));
  v.push(mk(c, "enter", "Enter", "", 88));
  // 主区第三排：Caps + 11（12）。
  v.push(mk(c, "capsLock", "Caps", "", 78));
  for (const [b, s] of ROW_A) v.push(mk(c, "char", b, s, 44));
  // 主区第四排：Shift + 10 + Shift（12）。
  v.push(mk(c, "shift", "Shift", "", 100));
  for (const [b, s] of ROW_Z) v.push(mk(c, "char", b, s, 44));
  v.push(mk(c, "shift", "Shift", "", 100));
  // 主区底排：Ctrl Win Alt Space Alt Win Menu Ctrl（8）。
  v.push(mk(c, "ctrl", "Ctrl", "", 60));
  v.push(mk(c, "win", "Win", "", 52));
  v.push(mk(c, "alt", "Alt", "", 52));
  v.push(mk(c, "space", "", "", 248));
  v.push(mk(c, "alt", "Alt", "", 52));
  v.push(mk(c, "win", "Win", "", 52));
  v.push(mk(c, "menu", "Menu", "", 52));
  v.push(mk(c, "ctrl", "Ctrl", "", 60));
  // 编辑导航区：PrtSc ScrLk Pause / Ins Home PgUp / Del End PgDn / 方向（13）。
  v.push(mk(c, "fnRow", "PrtSc", "", 44));
  v.push(mk(c, "scrollLock", "Scr", "", 44));
  v.push(mk(c, "fnRow", "Pause", "", 44));
  v.push(mk(c, "fnRow", "Ins", "", 44));
  v.push(mk(c, "fnRow", "Home", "", 44));
  v.push(mk(c, "fnRow", "PgUp", "", 44));
  v.push(mk(c, "fnRow", "Del", "", 44));
  v.push(mk(c, "fnRow", "End", "", 44));
  v.push(mk(c, "fnRow", "PgDn", "", 44));
  for (const base of ["←", "↑", "↓", "→"]) v.push(mk(c, "arrow", base, "", 44));
  // 小键盘：NumLock / * - + 789 456 123 0 . Enter（17）。
  v.push(mk(c, "numLock", "Num", "", 44));
  v.push(mk(c, "char", "n/", "", 44));
  v.push(mk(c, "char", "n*", "", 44));
  v.push(mk(c, "char", "n-", "", 44));
  for (const b of ["7", "8", "9"]) v.push(mk(c, "char", b, "", 44));
  v.push(mk(c, "char", "n+", "", 44));
  for (const b of ["4", "5", "6"]) v.push(mk(c, "char", b, "", 44));
  for (const b of ["1", "2", "3"]) v.push(mk(c, "char", b, "", 44));
  v.push(mk(c, "enter", "nEnter", "", 44));
  v.push(mk(c, "char", "n0", "", 44));
  v.push(mk(c, "char", "n.", "", 44));
  return v;
}

/** 紧凑布局（九宫联想前瞻——8 组字母分组）。 */
export function compactLayout(): VKey[] {
  const groups = ["abc", "def", "ghi", "jkl", "mno", "pqrs", "tuv", "wxyz"];
  const v: VKey[] = [];
  let code = 500;
  for (const g of groups) {
    v.push({ code: code++, kind: "char", base: g, shifted: "", w: 88, h: 88 });
  }
  return v;
}

/** 键盘状态机。 */
export class OnScreenKb {
  full = true; // true=全布局 false=紧凑
  shift = false;
  caps = false;
  numLock = true;
  scrollLock = false;
  ctrl = false;
  alt = false;
  opacity = OPACITY_DEFAULT;
  clickThrough = false;
  alwaysOnTop = true;
  /** 学习模式：物理键按下 → 虚拟键同步高亮。 */
  learning = true;
  highlightCode: number | null = null;
  keyEvents = 0;
  /** 多点并发：当前按住的键集（触屏前瞻 F063）。 */
  held: number[] = [];
  /** 音效回显（F079）——由调用方触发。 */
  soundEnabled = true;

  keys(): VKey[] {
    return this.full ? fullLayout() : compactLayout();
  }

  /** 虚拟键按下 → 上屏字符与状态迁移。返回上屏文本（无则空串）。 */
  press(code: number): string {
    const k = this.keys().find((x) => x.code === code);
    if (!k) return "";
    this.keyEvents += 1;
    if (!this.held.includes(code)) this.held.push(code);
    switch (k.kind) {
      case "char": {
        // 小键盘按码位段判定（NUMPAD_BASE_CODE 之后）——数字键 base 与主区同形。
        const isNumpad = code >= NUMPAD_BASE_CODE && /^\d$/.test(k.base);
        const effective = isNumpad && !this.numLock ? "" : keyLabel(k, this.shift, this.caps);
        if (this.shift) {
          this.shift = false; // 单发弹起：符号层输入一次后 Shift 自动弹起
        }
        return effective;
      }
      case "space":
        return " ";
      case "enter":
        return "\n";
      case "backspace":
        return "\b";
      case "shift":
        this.shift = !this.shift;
        return "";
      case "ctrl":
        this.ctrl = !this.ctrl;
        return "";
      case "alt":
        this.alt = !this.alt;
        return "";
      case "capsLock":
        this.caps = !this.caps;
        return "";
      case "numLock":
        this.numLock = !this.numLock;
        return "";
      case "scrollLock":
        this.scrollLock = !this.scrollLock;
        return "";
      default:
        return ""; // 功能排/方向/菜单：修饰面由系统层接管（模型面同语义）
    }
  }

  /** 松开（多点并发 held 集维护）。 */
  release(code: number): void {
    this.held = this.held.filter((c) => c !== code);
  }

  /** 学习模式：物理键按下 → 虚拟键同步高亮（KeyboardEvent.code → 码位）。
   *  映射主区字符与功能键；映射不到返回 null（不误亮）。 */
  highlightFromPhysical(e: { code: string; key: string }): number | null {
    if (!this.learning) return null;
    const keys = fullLayout();
    if (e.code === "Space") return keys.find((k) => k.kind === "space")?.code ?? null;
    const byBase = (b: string): number | null => keys.find((k) => k.base === b)?.code ?? null;
    if (e.code.startsWith("Key")) return byBase(e.code.slice(3).toLowerCase());
    if (e.code.startsWith("Digit")) return byBase(e.code.slice(5));
    const direct: Record<string, string> = {
      Space: " ", Enter: "Enter", Backspace: "⌫", Tab: "Tab", Escape: "Esc",
      CapsLock: "Caps", NumLock: "Num", ScrollLock: "Scr",
      ShiftLeft: "Shift", ShiftRight: "Shift", ControlLeft: "Ctrl", ControlRight: "Ctrl",
      AltLeft: "Alt", AltRight: "Alt", MetaLeft: "Win", MetaRight: "Win",
      ArrowLeft: "←", ArrowUp: "↑", ArrowDown: "↓", ArrowRight: "→",
      Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Backslash: "\\",
      Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/", Backquote: "`",
    };
    const d = direct[e.code];
    if (d !== undefined) return byBase(d);
    if (e.code.startsWith("F") && /^F\d{1,2}$/.test(e.code)) return byBase(e.code);
    if (e.code.startsWith("Numpad")) {
      const np: Record<string, string> = { Divide: "n/", Multiply: "n*", Subtract: "n-", Add: "n+", Enter: "nEnter", Decimal: "n." };
      const n = e.code.slice(6);
      const b = /^\d$/.test(n) ? n : np[n];
      if (b) {
        const pad = keys.slice(NUMPAD_BASE_CODE - 1); // 小键盘码位段（数字键 base 与主区同形）
        return pad.find((k) => k.base === b)?.code ?? null;
      }
    }
    return null;
  }

  /** 透明度设置（可读下限 30% 红线——低于即诚实拒绝并保持原值）。 */
  setOpacity(pct: number): boolean {
    const v = Math.round(pct);
    if (v < OPACITY_FLOOR || v > 100) return false;
    this.opacity = v;
    return true;
  }

  /** 屏边吸附：靠近屏缘 24px 内吸附到缘（拖拽松手时调用）。 */
  static snapToEdge(x: number, y: number, screenW: number, screenH: number, winW = WINDOW_W_PX, winH = WINDOW_H_PX): { x: number; y: number; snapped: boolean } {
    const EDGE = 24;
    let nx = x;
    let ny = y;
    let snapped = false;
    if (x <= EDGE) { nx = 0; snapped = true; }
    else if (x + winW >= screenW - EDGE) { nx = screenW - winW; snapped = true; }
    if (y <= EDGE) { ny = 0; snapped = true; }
    else if (y + winH >= screenH - EDGE) { ny = screenH - winH; snapped = true; }
    return { x: Math.max(0, Math.min(nx, screenW - winW)), y: Math.max(0, Math.min(ny, screenH - winH)), snapped };
  }
}
