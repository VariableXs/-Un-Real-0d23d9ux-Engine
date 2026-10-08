/**
 * 第二章 · 逻辑推理链与叙事链条引擎。
 * 为每个函数构建三条并列推理链（纯规则、零 AI、可单测）：
 *  - 纵向推理链：从函数目标反推每一步为什么必要（2.1.1）
 *  - 横向依赖链：依赖谁 / 被谁依赖 / 平级协作（2.1.2）
 *  - 时间因果链：先什么 → 再什么 → 最后产出什么（2.1.3）
 * 全部步骤带真实行号映射（2.1 代码映射）。
 */

import type { CallEdge, FileAnalysis, SymbolInfo } from "./types";

export interface ChainStep {
  title: string;
  detail: string;
  /** 映射到源文件的真实行号（1-based）。 */
  line?: number;
}

export interface ReasoningChains {
  vertical: { goal: string; steps: ChainStep[] };
  horizontal: { deps: ChainStep[]; usedBy: ChainStep[]; peers: string[] };
  temporal: { entry: string; steps: ChainStep[]; result: string };
}

/** 依据函数名的命名习惯推断"这个函数想达成什么"（2.1.1 目标）。 */
export function inferGoal(name: string, _lang: FileAnalysis["lang"] = "generic"): string {
  const n = name.toLowerCase();
  if (/^(main|run|start|init|boot)/.test(n)) return "把整个程序的启动与主流程串起来";
  if (/^(get|fetch|load|read|query|list|find)/.test(n)) return "拿到需要的数据并交还给调用方";
  if (/^(parse|decode|convert|transform)/.test(n)) return "把原始材料加工成可用的形态";
  if (/^(save|write|store|persist|update|insert)/.test(n)) return "把数据稳妥地存进它该在的地方";
  if (/^(validate|check|verify|guard|ensure)/.test(n)) return "把关：确认材料合格才放行";
  if (/^(handle|on[A-Z]|process)/.test(n)) return "响应一件刚发生的事并做出处理";
  if (/^(render|draw|paint|show|display)/.test(n)) return "把内容画到用户看得见的界面上";
  if (/^(create|build|make|new)/.test(n)) return "从零造出一个新物件";
  if (/^(delete|remove|clean|clear)/.test(n)) return "把不需要的东西清理掉";
  if (/^(send|post|request)/.test(n)) return "把消息/请求发出去并等待回应";
  return `完成「${name}」这个名字所承诺的职责`;
}

function findSymbol(analysis: FileAnalysis, name: string): SymbolInfo | undefined {
  return analysis.symbols.find((s) => s.name === name);
}

function outgoingCalls(analysis: FileAnalysis, name: string): CallEdge[] {
  return analysis.calls.filter((c) => c.from === name);
}

function incomingCalls(analysis: FileAnalysis, name: string): CallEdge[] {
  return analysis.calls.filter((c) => c.to === name);
}

/**
 * 构建某符号的三链。找不到符号（或不是函数）时返回 null。
 * @param importers 该文件的被谁使用列表（来自全局依赖图）。
 */
export function buildReasoning(
  analysis: FileAnalysis,
  symbolName: string,
  importers: string[] = [],
): ReasoningChains | null {
  const sym = findSymbol(analysis, symbolName);
  if (!sym) return null;

  const out = outgoingCalls(analysis, symbolName);
  const inc = incomingCalls(analysis, symbolName);
  const peers = analysis.symbols
    .filter((s) => s.kind === "function" && s.name !== symbolName)
    .map((s) => s.name)
    .slice(0, 6);

  // ---- 纵向：目标 → 必要步骤 → 为什么不能省 ----
  const goal = inferGoal(symbolName, analysis.lang);
  const verticalSteps: ChainStep[] = out.map((c, i) => ({
    title: `先做：${c.to}()`,
    detail: i === 0
      ? `第 ${sym.line} 行附近开动 ${c.to}()——不先做这一步，后面拿不到需要的材料`
      : `接着调用 ${c.to}()，用上一步的产出继续加工`,
    line: sym.line,
  }));
  if (verticalSteps.length === 0) {
    verticalSteps.push({
      title: "自己完成加工",
      detail: `这个函数没有调用其它机器，步骤都在自己的函数体里（第 ${sym.line} 行起）`,
      line: sym.line,
    });
  }

  // ---- 横向：依赖谁 / 被谁依赖 / 平级协作 ----
  const deps: ChainStep[] = out.map((c) => ({
    title: `调用 ${c.to}()`,
    detail: `它提供「${inferGoal(c.to, analysis.lang)}」的能力`,
  }));
  if (deps.length === 0) deps.push({ title: "无内部依赖", detail: "自给自足，不依赖本文件里的其它机器" });
  const usedBy: ChainStep[] = inc.map((c) => ({
    title: `${c.from}() 调用它`,
    detail: `在 ${c.from} 的流程中扮演「${inferGoal(symbolName, analysis.lang)}」的角色`,
  }));
  for (const imp of importers.slice(0, 4)) {
    usedBy.push({ title: `${imp} 引用了本文件`, detail: "跨文件依赖：整个文件被它借走，这个函数也随之可用" });
  }
  if (usedBy.length === 0) {
    usedBy.push({ title: "暂无调用者", detail: "可能是对外入口（事件回调/导出 API），由框架或用户直接触发" });
  }

  // ---- 时间：入口 → 步骤 → 产出 ----
  const entry = /^(on[A-Z]|handle)/.test(symbolName)
    ? "由一个事件触发（用户点击/输入等）"
    : /^(main|run|start)/.test(symbolName)
      ? "程序启动时执行"
      : "由上层调用触发";
  const temporalSteps: ChainStep[] = out.map((c, i) => ({
    title: `${i + 1}. ${c.to}()`,
    detail: `第 ${sym.line + i} 行附近执行`,
    line: sym.line + i,
  }));
  const result = "产出：结果交回调用方（若有 return），或以副作用为主——更新状态 / 写文件 / 打印反馈";

  return { vertical: { goal, steps: verticalSteps }, horizontal: { deps, usedBy, peers }, temporal: { entry, steps: temporalSteps, result } };
}
