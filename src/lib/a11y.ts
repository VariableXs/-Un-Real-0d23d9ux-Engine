/**
 * AI-19 无障碍与本地化组 — 无障碍 2.0 工具库（U-40 / M-73 前端侧）。
 *
 * 覆盖：
 * - aria-live 通知区（announce）：状态变化对屏幕阅读器可闻；
 * - 焦点跟随提示（startFocusAnnouncer）：Tab 移动时读出目标控件名称；
 * - 色觉模拟器（CVD_FILTERS / applyCvdFilter）：四类色觉滤镜，
 *   供开发者/用户检验配色对色觉障碍用户的可读性；
 * - 粘滞键序列累积器（StickyModifiers，M-73）：系统粘滞键开启时
 *   支持「分步修饰键」输入（Ctrl 松开后再按 Alt 再按 E）。
 *
 * 红线（承 ASCENT U-40 / SUMMIT M-73 口径）：
 * - 不实现自家屏幕阅读器（系统职责）；
 * - 一切工具零副作用默认（滤镜/播报全部 opt-in）。
 */

export type CvdKind = "off" | "protanopia" | "deuteranopia" | "tritanopia" | "achromatopsia";

/**
 * M-73/M-74 运行时桥接态（App.tsx 探针轮询写入，主题/动效层读取）：
 * - hcOverride：系统高对比度开启且用户开了跟随 → 主题层临时切 HC；
 * - narrator：讲述人运行 → 动效自动降级（reduceMotion 运行时叠加）；
 * - stickyKeys / filterKeys：系统粘滞/筛选键开启（键位解析层参考）。
 */
export const a11yRuntime = {
  hcOverride: false,
  narrator: false,
  stickyKeys: false,
  filterKeys: false,
};

/** 四类色觉模拟滤镜（近似模拟；开发者向预览，非医疗精确）。 */
export const CVD_FILTERS: Record<CvdKind, string> = {
  off: "none",
  protanopia: "url(#cvd-protanopia)",
  deuteranopia: "url(#cvd-deuteranopia)",
  tritanopia: "url(#cvd-tritanopia)",
  achromatopsia: "grayscale(1)",
};

/**
 * 色觉模拟所需的 SVG feColorMatrix 定义（protan/deuteran/tritan）。
 * 注入文档一次（id 幂等），供 CSS filter: url(#cvd-*) 引用。
 */
export function ensureCvdSvgFilters(doc: Document = document): void {
  const ID = "cvd-filters-root";
  if (doc.getElementById(ID)) return;
  const svg = doc.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.id = ID;
  svg.setAttribute("width", "0");
  svg.setAttribute("height", "0");
  svg.setAttribute("aria-hidden", "true");
  svg.style.position = "absolute";
  svg.style.pointerEvents = "none";
  svg.innerHTML = `
    <filter id="cvd-protanopia"><feColorMatrix type="matrix" values="
      0.567 0.433 0 0 0
      0.558 0.442 0 0 0
      0 0.242 0.758 0 0
      0 0 0 1 0"/></filter>
    <filter id="cvd-deuteranopia"><feColorMatrix type="matrix" values="
      0.625 0.375 0 0 0
      0.7 0.3 0 0 0
      0 0.3 0.7 0 0
      0 0 0 1 0"/></filter>
    <filter id="cvd-tritanopia"><feColorMatrix type="matrix" values="
      0.95 0.05 0 0 0
      0 0.433 0.567 0 0
      0 0.475 0.525 0 0
      0 0 0 1 0"/></filter>`;
  doc.body.appendChild(svg);
}

/** 应用/清除色觉模拟滤镜（root 上的 data-cvd 属性 + CSS 规则联动）。 */
export function applyCvdFilter(root: HTMLElement, kind: CvdKind): void {
  if (kind === "off") {
    delete root.dataset.cvd;
    root.style.filter = "";
    return;
  }
  ensureCvdSvgFilters(root.ownerDocument ?? document);
  root.dataset.cvd = kind;
  root.style.filter = CVD_FILTERS[kind];
}

// ---------- aria-live 通知（M-76：状态变化 aria-live=polite 通知） ----------

const LIVE_REGIONS: Record<"polite" | "assertive", HTMLElement | null> = {
  polite: null,
  assertive: null,
};

function liveRegion(kind: "polite" | "assertive", doc: Document): HTMLElement {
  const existing = LIVE_REGIONS[kind];
  if (existing && existing.isConnected) return existing;
  const el = doc.createElement("div");
  el.id = `a11y-live-${kind}`;
  el.setAttribute("aria-live", kind);
  el.setAttribute("role", kind === "assertive" ? "alert" : "status");
  el.style.position = "fixed";
  el.style.width = "1px";
  el.style.height = "1px";
  el.style.overflow = "hidden";
  el.style.clip = "rect(0 0 0 0)";
  el.style.whiteSpace = "nowrap";
  doc.body.appendChild(el);
  LIVE_REGIONS[kind] = el;
  return el;
}

/** 对屏幕阅读器播报一条状态变化（M-76：默认 polite 不打断）。 */
export function announce(message: string, assertive = false): void {
  if (typeof document === "undefined" || !message) return;
  const el = liveRegion(assertive ? "assertive" : "polite", document);
  // 先清空再写入，保证相同文案连续播报也能触发。
  el.textContent = "";
  window.setTimeout(() => {
    el.textContent = message;
  }, 30);
}

// ---------- 焦点跟随提示（U-40：Tab 移动时目标控件名称读出） ----------

/** 取一个元素的可读名称（aria-label > aria-labelledby > title > 可见文本）。 */
export function controlName(el: Element): string {
  const label = el.getAttribute("aria-label");
  if (label?.trim()) return label.trim();
  const labelledby = el.getAttribute("aria-labelledby");
  if (labelledby && el.ownerDocument) {
    const refs = labelledby
      .split(/\s+/)
      .map((id) => el.ownerDocument?.getElementById(id)?.textContent?.trim() ?? "")
      .filter(Boolean);
    if (refs.length) return refs.join(" ");
  }
  const title = el.getAttribute("title");
  if (title?.trim()) return title.trim();
  const text = (el as HTMLElement).innerText ?? el.textContent ?? "";
  const t = text.replace(/\s+/g, " ").trim();
  return t.slice(0, 80);
}

/**
 * 焦点跟随提示：Tab 导航移动时读出目标控件名称
 * （复用 aria-live polite 通道，与系统 Narrator 兼容模式互补）。
 * 返回清理函数。
 */
export function startFocusAnnouncer(doc: Document = document): () => void {
  const onFocus = (e: FocusEvent): void => {
    const el = e.target;
    if (!(el instanceof Element)) return;
    const name = controlName(el);
    if (name) announce(name);
  };
  doc.addEventListener("focusin", onFocus, true);
  return () => doc.removeEventListener("focusin", onFocus, true);
}

// ---------- 粘滞键序列累积器（M-73：分步修饰键输入） ----------

export type ModifierName = "ctrl" | "alt" | "shift" | "meta";

const MODIFIER_KEYS: Record<string, ModifierName> = {
  Control: "ctrl",
  Alt: "alt",
  AltGraph: "alt",
  Shift: "shift",
  Meta: "meta",
};

/**
 * 粘滞序列累积器：系统粘滞键开启时，修饰键按下即「锁存」，
 * 非修饰键到达时返回（锁存 ∪ 实时）修饰键集合并清空锁存。
 * 验收口径（M-73）：逐个按 Ctrl→Alt→E 成功触发组合。
 */
export class StickyModifiers {
  private latched = new Set<ModifierName>();

  /** 喂入一次 keydown；返回是否为修饰键。 */
  feed(key: string): boolean {
    const mod = MODIFIER_KEYS[key];
    if (mod) {
      this.latched.add(mod);
      return true;
    }
    return false;
  }

  /** 非修饰键到达：取走（锁存 ∪ 实时事件修饰键）。 */
  take(e: { ctrlKey: boolean; altKey: boolean; shiftKey: boolean; metaKey: boolean }): Set<ModifierName> {
    const out = new Set(this.latched);
    if (e.ctrlKey) out.add("ctrl");
    if (e.altKey) out.add("alt");
    if (e.shiftKey) out.add("shift");
    if (e.metaKey) out.add("meta");
    this.latched.clear();
    return out;
  }

  /** 显式清空（Esc / 失焦时防误触发）。 */
  reset(): void {
    this.latched.clear();
  }

  /** 当前锁存态（只读展示用）。 */
  current(): ReadonlySet<ModifierName> {
    return this.latched;
  }

  /** 修饰键集合 → 常规事件布尔四元组（供既有快捷键解析层复用）。 */
  static toFlags(mods: Set<ModifierName>): { ctrl: boolean; alt: boolean; shift: boolean; meta: boolean } {
    return {
      ctrl: mods.has("ctrl"),
      alt: mods.has("alt"),
      shift: mods.has("shift"),
      meta: mods.has("meta"),
    };
  }
}

/**
 * M-73 桥接判定：系统粘滞键开启时，把分步输入的修饰键并入
 * 键盘事件的四元组（例如分步 Ctrl→Alt→E 等价 Ctrl+Alt+E）。
 */
export function withStickyModifiers(e: KeyboardEvent, sticky: StickyModifiers): { ctrl: boolean; alt: boolean; shift: boolean; meta: boolean } {
  if (MODIFIER_KEYS[e.key]) {
    sticky.feed(e.key);
    return { ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, meta: e.metaKey };
  }
  return StickyModifiers.toFlags(sticky.take(e));
}
