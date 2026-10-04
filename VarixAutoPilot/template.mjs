/**
 * VarixAutoPilot · 模板渲染（纯函数，无副作用）
 *
 * 单独成文件的原因：`server.mjs` 一import 就会 listen，
 * 单元测试 import 它会把测试进程挂住。纯函数必须可独立测。
 */

/**
 * 渲染模板。
 *
 * ★ 系统内置变量（自动注入，不需要人填）★
 * 早先把 {{时间}} 当普通变量，结果"未填就拦"把它一起拦了——
 * 但时间戳是**系统该给的**，让人手填时间戳是设计错误。
 * 现在内置变量在渲染时自动注入，与用户变量分开：
 * 用户只填业务信息，系统填环境信息。
 *
 * 未提供的用户键**保留占位符原样**（不替换成空串）——
 * 空段落会让人以为"这里没什么要说的"，而实际上是没配。
 *
 * @param {string} tpl 含 {{键}} 的模板
 * @param {Record<string, unknown>} vars 变量表（用户填的）
 * @returns {{text: string, missing: string[]}}
 */
export function renderTemplate(tpl, vars) {
  const missing = [];
  const now = new Date();
  const p2 = (n) => String(n).padStart(2, '0');
  const sysVars = {
    时间: `${now.getFullYear()}-${p2(now.getMonth() + 1)}-${p2(now.getDate())} ` +
          `${p2(now.getHours())}:${p2(now.getMinutes())}:${p2(now.getSeconds())}`,
  };
  const text = String(tpl == null ? '' : tpl).replace(
    /\{\{\s*([^{}]+?)\s*\}\}/g,
    (_, k) => {
      const key = k.trim();
      if (Object.prototype.hasOwnProperty.call(sysVars, key)) return sysVars[key];
      const v = vars ? vars[key] : undefined;
      // 数字 0 是有效内容，不能与"未填"混为一谈
      if (v === undefined || v === null || v === '') {
        if (!missing.includes(key)) missing.push(key);
        return `{{${key}}}`;
      }
      return String(v);
    }
  );
  return { text, missing };
}
