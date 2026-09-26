/**
 * J 鼠标域 · 应用身份注册表（v6 · 深化批次六 · F605/F615/F616）。
 *
 * v4 的最丑角落（checklist F605 自记）：「应用覆盖编辑器要求手填
 * data-app-id——对普通用户是黑话」。而 v3 起全系统八窗口都声明了
 * data-app-id/data-app-class——身份就在 DOM 上，本模块把它变成可选清单：
 * 枚举当前系统所有已声明应用（id/类目/角色标签），覆盖编辑器从「手填
 * 黑话」变「从列表选」——普通用户不再需要知道任何实现细节。
 *
 * 数据口径（一处一事实）：清单=DOM [data-app-id] 实时枚举，非静态登记表
 * （新窗口挂载即自动出现，无同步维护成本）；data-app-label 缺省回退 id。
 */

export interface AppIdentity {
  id: string;
  /** 应用类目（F605 刻度语义解析用，缺省 document）。 */
  appClass: string;
  /** 展示名（data-app-label，缺省回退 id——诚实呈现不编造）。 */
  label: string;
}

/**
 * 枚举系统内已声明的应用身份（document 级扫描，去重保序）。
 * 非浏览器环境返回空数组（面板显式空态，不静默伪装）。
 */
export function enumerateAppIds(doc: Document = document): AppIdentity[] {
  if (typeof doc.querySelectorAll !== "function") return [];
  const seen = new Set<string>();
  const out: AppIdentity[] = [];
  for (const el of Array.from(doc.querySelectorAll("[data-app-id]"))) {
    const id = el.getAttribute("data-app-id");
    if (!id || seen.has(id)) continue;
    seen.add(id);
    out.push({
      id,
      appClass: el.getAttribute("data-app-class") ?? "document",
      label: el.getAttribute("data-app-label") ?? id,
    });
  }
  return out;
}

/**
 * 覆盖目标合法性校验（编辑器保存前调用）：id 必须在当前枚举内或用户显式
 * 「手填高级模式」——默认拦截拼写错误（选单里没有 = 打错了 = 不许存）。
 * @returns 错误清单（空 = 合法）
 */
export function validateAppOverrideTarget(id: string, known: AppIdentity[]): string[] {
  const trimmed = id.trim();
  if (!trimmed) return ["应用 id 不能为空"];
  if (!known.some((a) => a.id === trimmed)) {
    return [`「${trimmed}」不在当前系统已声明的应用清单中——请从列表选择（或确认窗口已打开过）`];
  }
  return [];
}
