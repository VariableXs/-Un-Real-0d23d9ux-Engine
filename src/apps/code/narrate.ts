﻿/**
 * 第四章：通俗语言翻译引擎。把"符号/文件/流程"翻译成生活化语言：
 * 文件 = 厨房里的菜品清单，函数 = 机器，import = 借工具。
 * 所有词典查询走 dictionaries.ts；查不到的词原样保留并上报（8.2）。
 */

import type { FileAnalysis, ProjectDetect, ProjectScanResult, ScanEntry } from "./types";
import { describeDir } from "./dictionaries";
import { describeFileDeep, describeFunctionDeep, type NarrateStyle } from "./deepDescribe";
import { domainNarrative, plainEnglishFunction } from "./english";

export type Lang = "zh" | "zh-TW" | "en";

const ROLE_LABEL: Record<FileAnalysis["role"], [string, string]> = {
  entry: ["启动入口 —— 程序从这里开始运行", "Entry point — the program starts running here"],
  source: ["源代码 —— 实现功能的具体代码", "Source code — the code that does the work"],
  config: ["配置文件 —— 项目里各种开关和参数", "Config file — the project's switches and knobs"],
  doc: ["说明文档 —— 给人看的项目介绍", "Docs — writing for humans, not machines"],
  asset: ["静态资源 —— 图片、字体等素材", "Asset — images, fonts and other material"],
};

export function roleLabel(role: FileAnalysis["role"], lang: Lang): string {
  const [zh, en] = ROLE_LABEL[role];
  return lang === "en" ? en : zh;
}

/** 项目根节点的一句话通俗解释（spec 5.1 "是什么"分支）。 */
export function purposeSentence(detect: ProjectDetect, scan: ProjectScanResult, lang: Lang): string {
  const nFiles = scan.entries.filter((e) => e.kind === "file").length;
  const nDirs = scan.entries.filter((e) => e.kind === "dir").length;
  const stack = detect.stack.length > 0 ? detect.stack.join(" + ") : lang !== "en" ? "通用代码" : "plain code";
  const patterns = detect.patterns && detect.patterns.length > 0
    ? (lang !== "en" ? `识别到架构模式：${detect.patterns.join("、")}。` : ` Patterns: ${detect.patterns.join(", ")}.`)
    : "";
  // v3.0 2.1 自适应逻辑链：按识别出的领域加载对应的因果叙事链。
  const chain = domainNarrative(detect.domainKey ?? detect.domain, lang);
  return lang !== "en"
    ? `这个项目是${detect.label}。它属于：${detect.domain}。主要由 ${stack} 写成，共有 ${nFiles} 个文件、${nDirs} 个文件夹。${patterns}${chain}`
    : `This project is ${detect.label}. Domain: ${detect.domain}. Written mainly in ${stack}, with ${nFiles} files in ${nDirs} folders.${patterns} ${chain}`;
}

export function techStackSentence(detect: ProjectDetect, lang: Lang): string {
  if (detect.stack.length === 0) return lang !== "en" ? "暂未识别出框架" : "No framework detected";
  const list = detect.stack.join("、");
  return lang !== "en"
    ? `用到的工具和骨架：${list}。框架就像"已经搭好的骨架，我们只需要往里填东西"。`
    : `Tools & frameworks: ${list}. Frameworks are pre-built skeletons you fill in.`;
}

export function evidenceSentence(detect: ProjectDetect, lang: Lang): string {
  return detect.evidence.map((e) => (lang !== "en" ? `• ${e}` : `• ${e}`)).join(lang !== "en" ? "\n" : "\n");
}

/** 目录节点：用生活化比喻描述（spec 5.2）。 */
export function dirDescription(entry: ScanEntry, lang: Lang): string {
  return describeDir(entry.name, lang);
}

/** 文件节点主标题下的一句话定位 —— 走五维深度翻译，杜绝模板句（二章 2.3）。 */
export function fileOneLiner(analysis: FileAnalysis, lang: Lang, style: NarrateStyle = "metaphor"): string {
  const name = analysis.relPath.split("/").pop() ?? analysis.relPath;
  const deep = describeFileDeep(analysis, style);
  if (deep.trim().length > 0) return deep;
  return lang !== "en" ? `${name}：${roleLabel(analysis.role, lang)}。共 ${analysis.loc} 行。` : `${name}: ${roleLabel(analysis.role, lang)}. ${analysis.loc} lines.`;
}

/** 文件信息卡"主要功能列表"（spec 5.2），每台机器一句独有五维翻译。 */
export function abilityList(analysis: FileAnalysis, lang: Lang, max = 6, style: NarrateStyle = "metaphor"): string[] {
  const out: string[] = [];
  const fns = analysis.symbols.filter((s) => s.kind === "function");
  const classes = analysis.symbols.filter((s) => s.kind === "class");
  if (lang !== "en") {
    for (const c of classes.slice(0, 2)) {
      out.push(`图纸《${c.name}》—— 用它可以造出很多个同款物件（第 ${c.line} 行）`);
    }
    for (const f of fns.slice(0, max - out.length)) {
      out.push(`${describeFunctionDeep(analysis, f, style)}（第 ${f.line} 行）`);
    }
    if (out.length === 0) out.push("这个文件主要是声明和配置，没有定义独立的机器或图纸。");
  } else {
    for (const c of classes.slice(0, 2)) out.push(`Blueprint "${c.name}" — stamps out identical objects (line ${c.line})`);
    for (const f of fns.slice(0, max - out.length)) {
      out.push(`${describeFunctionDeep(analysis, f, "engineering")} (line ${f.line})`);
    }
    if (out.length === 0) out.push("This file is mostly declarations and configuration.");
  }
  return out;
}

/** 信息卡"依赖关系"：它借用了谁（import 列表，通俗化为"借工具"）。 */
export function depsSentence(analysis: FileAnalysis, lang: Lang, max = 8): string[] {
  if (analysis.imports.length === 0) {
    return [lang !== "en" ? "不依赖其他模块，自己是独立的。" : "No dependencies — fully self-contained."];
  }
  return analysis.imports.slice(0, max).map((imp) => {
    const names = imp.names.length > 0 ? `（用到了 ${imp.names.slice(0, 4).join("、")}）` : "";
    return lang !== "en" ? `借了工具箱 ${imp.from}${names}` : `Borrows from ${imp.from}${names}`;
  });
}

/** "怎么运行"分支：长链条流程的每一步描述（spec 5.3）。 */
export function flowStepDescription(
  analysis: FileAnalysis,
  isEntry: boolean,
  lang: Lang,
): string {
  const name = analysis.relPath.split("/").pop() ?? analysis.relPath;
  const head = lang !== "en"
    ? (isEntry ? `启动：打开 ${name}` : `接着：进入 ${name}`)
    : (isEntry ? `Start: open ${name}` : `Then: enter ${name}`);
  const fns = analysis.symbols.filter((s) => s.kind === "function").slice(0, 3).map((s) => s.name);
  const tail = fns.length > 0
    ? (lang !== "en" ? `这里会开动这些机器：${fns.join("、")}。` : `These machines run here: ${fns.join(", ")}.`)
    : (lang !== "en" ? "这里主要是准备和声明。" : "Mostly preparation here.");
  return `${head}，${tail}`;
}

/** 把一个 UIR 函数翻译成大白话（spec 4.2 函数/参数/返回值；v3.0 2.2 英文走 Plain English）。 */
export function describeFunction(name: string, params: string[], lang: Lang): string {
  if (lang !== "en") {
    const p = params.length > 0 ? `放进去的原材料：${params.join("、")}` : "不需要放东西进去";
    return `机器 ${name}()：一台加工机器。${p}，加工完给你返回成品。`;
  }
  return plainEnglishFunction(name, params);
}

/** 文件选择优先级：入口 > 配置 > 文档 > 小源码文件（用于信息卡兜底）。 */
export function entryDescription(detect: ProjectDetect, lang: Lang): string {
  if (detect.entryCandidates.length === 0) {
    return lang !== "en" ? "没有找到明显的启动入口（可能是个工具库）。" : "No obvious entry point (probably a library).";
  }
  const list = detect.entryCandidates.join(" / ");
  return lang !== "en"
    ? `程序从这里开始运行：${list}。就像做菜：先开火（启动），再按菜谱一步步做（主流程）。`
    : `The program starts here: ${list}. Like cooking: turn on the heat, then follow the recipe step by step.`;
}
