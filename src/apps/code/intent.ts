/**
 * 第六章 反向生成模式：大白话 → 代码。
 * 意图识别（动词+名词）→ 先讲通俗步骤 → 生成对应技术栈的代码模板 →
 * 告诉用户粘贴到哪个文件。模板覆盖首批高频意图：按钮/提示框、请求接口、
 * 保存数据、新增函数；未命中时给出通用引导。
 */

import type { IntentPlan, ProjectDetect } from "./types";

interface IntentRule {
  id: string;
  keywords: RegExp;
  /** Guard: only fires when the project type matches (or "*"). */
  fits: (detect: ProjectDetect) => boolean;
  build: (detect: ProjectDetect, m: RegExpMatchArray | null, text: string) => Omit<IntentPlan, "matched" | "unknownTerms">;
}

function primaryLangOf(detect: ProjectDetect): "ts" | "js" | "python" | "rust" {
  if (detect.primaryLang === "python") return "python";
  if (detect.primaryLang === "rust") return "rust";
  if (detect.primaryLang === "js" || detect.typeId === "node") return "js";
  return "ts";
}

function pascal(name: string): string {
  const cleaned = name.replace(/[^\w]+/g, " ").trim();
  if (!cleaned) return "Feature";
  const p = cleaned.split(/\s+/).map((s) => s.charAt(0).toUpperCase() + s.slice(1)).join("");
  return /^[A-Za-z]/.test(p) ? p : `My${p}`;
}

function camel(name: string): string {
  const p = pascal(name);
  return p.charAt(0).toLowerCase() + p.slice(1);
}

const RULES: IntentRule[] = [
  {
    id: "button-alert",
    keywords: /(按钮|点击|点一下|单击).{0,12}(弹|提示|显示|你好|hello)|(button|click).{0,16}(alert|dialog|hello|message)/i,
    fits: (d) => d.typeId === "react" || d.typeId === "next" || d.typeId === "vue",
    build: (d, m, _text) => {
      const feat = (m?.[0] ?? "hello button").slice(0, 12);
      const name = pascal(`${feat} button`);
      const isVue = d.typeId === "vue";
      const code = isVue
        ? `<script setup lang="ts">\nfunction ${camel(name)}(): void {\n  window.alert("你好 / Hello!");\n}\n</script>\n\n<template>\n  <button @click="${camel(name)}">点我 / Click me</button>\n</template>\n`
        : `export function ${name}(): React.JSX.Element {\n  const onClick = (): void => {\n    window.alert("你好 / Hello!");\n  };\n  return (\n    <button type="button" onClick={onClick}>\n      点我 / Click me\n    </button>\n  );\n}\n`;
      return {
        title: "做一个点击后弹提示的按钮",
        steps: [
          "第一步：在页面上放一个按钮（就像在桌上放一个门铃）",
          "第二步：告诉这个按钮“被按的时候要做什么”（接一根电线）",
          "第三步：让它做的事情就是弹出“你好”的提示框（门铃响了）",
        ],
        code,
        codeLang: isVue ? "vue" : "tsx",
        targetFile: isVue ? `src/components/${name}.vue` : `src/components/${name}.tsx`,
        explanation: "按钮组件是“积木”：先造一块新积木，再把它摆进页面里。onClick 就是那根电线。",
        anchorFile: "src/components",
      };
    },
  },
  {
    id: "fetch-data",
    keywords: /(请求|获取|拉取|调用).{0,10}(接口|数据|api|列表)|(fetch|http|request).{0,12}(data|api)/i,
    fits: (d) => ["react", "next", "vue", "node", "python"].includes(d.typeId),
    build: (d) => {
      const lang = primaryLangOf(d);
      if (lang === "python") {
        return {
          title: "请求一个接口拿数据",
          steps: [
            "第一步：带上地址去接口门口敲门（requests.get）",
            "第二步：对方把 JSON 表格递给你",
            "第三步：检查回执没问题后，取出里面的内容",
          ],
          code: `import requests\n\ndef fetch_data(url: str) -> dict:\n    resp = requests.get(url, timeout=10)\n    resp.raise_for_status()  # 回执不对就报错\n    return resp.json()\n`,
          codeLang: "python",
          targetFile: "src/fetch_data.py",
          explanation: "requests 就像替你跑腿的助手：你给它地址，它把回复原封不动带回来。",
        };
      }
      return {
        title: "请求一个接口拿数据",
        steps: [
          "第一步： async 边做A边做B——发起请求的同时界面不用卡住",
          "第二步： await 等这张“取货单”兑现",
          "第三步：拿到 JSON 表格，交给界面显示",
        ],
        code: `export async function fetchData(url: string): Promise<unknown> {\n  const resp = await fetch(url); // 递一张取货单\n  if (!resp.ok) throw new Error("请求失败 / Request failed");\n  return await resp.json();      // 兑现：拿到 JSON 表格\n}\n`,
        codeLang: "ts",
        targetFile: "src/api/fetchData.ts",
        explanation: "fetch 是浏览器自带的传话筒；await 表示“等对方回复再继续”。",
      };
    },
  },
  {
    id: "save-data",
    keywords: /(保存|存储|记住).{0,10}(数据|内容|设置|用户)|(save|store|persist)/i,
    fits: (d) => ["react", "next", "vue", "node", "python"].includes(d.typeId),
    build: (d) => {
      const lang = primaryLangOf(d);
      if (lang === "python") {
        return {
          title: "把数据保存成文件",
          steps: ["第一步：把内容装进一个字典", "第二步：打开（或新建）一个文件柜抽屉", "第三步：把内容写进去并锁好"],
          code: `import json\n\ndef save_data(path: str, data: dict) -> None:\n    with open(path, "w", encoding="utf-8") as f:\n        json.dump(data, f, ensure_ascii=False, indent=2)\n`,
          codeLang: "python",
          targetFile: "src/save_data.py",
          explanation: "json.dump 就像把一张表格平整地放进抽屉；ensure_ascii=False 让中文原样保存。",
        };
      }
      return {
        title: "把数据保存到本地",
        steps: [
          "第一步：给数据起一个钥匙名（key）",
          "第二步：JSON.stringify 把内容变成一串文字",
          "第三步：放进 localStorage 这个随身口袋，下次开门就能取",
        ],
        code: `export function saveLocal(key: string, data: unknown): void {\n  try {\n    localStorage.setItem(key, JSON.stringify(data));\n  } catch (e) {\n    console.error("保存失败 / Save failed", e);\n  }\n}\n\nexport function loadLocal<T>(key: string, fallback: T): T {\n  const raw = localStorage.getItem(key);\n  if (!raw) return fallback;\n  try {\n    return JSON.parse(raw) as T;\n  } catch {\n    return fallback;\n  }\n}\n`,
        codeLang: "ts",
        targetFile: "src/lib/localStore.ts",
        explanation: "localStorage 是浏览器送的随身口袋：关掉页面东西还在，但只存在这台电脑上。",
      };
    },
  },
  {
    id: "add-function",
    keywords: /(新增|加一个|添加|写一个).{0,10}(函数|方法|机器)|(add|create).{0,10}(function|method)/i,
    fits: () => true,
    build: (d, _m, text) => {
      const lang = primaryLangOf(d);
      const name = camel(text.replace(/[^\w\u4e00-\u9fa5 ]/g, "").split(/\s+/).slice(-2).join(" ") || "myTask");
      if (lang === "python") {
        return {
          title: "新增一台机器（函数）",
          steps: ["第一步：用 def 定义一台新机器", "第二步：写清楚它接收的原材料（参数）", "第三步：在机器里加工，最后 return 交出成品"],
          code: `def ${name}(x: int) -> int:\n    """大白话：这台机器把原材料加工成成品。"""\n    result = x * 2  # TODO: 换成你的加工逻辑\n    return result\n`,
          codeLang: "python",
          targetFile: "src/tasks.py",
          explanation: "函数是一台机器：def 造机器，参数是原材料，return 是成品出口。",
        };
      }
      if (lang === "rust") {
        return {
          title: "新增一台机器（函数）",
          steps: ["第一步：用 fn 定义一台新机器", "第二步：声明原材料的类型（Rust 要验收型号）", "第三步：加工后把成品交回去"],
          code: `pub fn ${name}(x: i32) -> i32 {\n    let result = x * 2; // TODO: 换成你的加工逻辑\n    result\n}\n`,
          codeLang: "rust",
          targetFile: "src/lib.rs",
          explanation: "fn 造机器；Rust 的编译器像严格的验收员，型号（类型）必须写清楚。",
        };
      }
      return {
        title: "新增一台机器（函数）",
        steps: ["第一步：想好机器叫什么、吃什么原料", "第二步：写函数签名（名字 + 参数）", "第三步：加工，然后 return 成品"],
        code: `export function ${name}(input: number): number {\n  const result = input * 2; // TODO: 换成你的加工逻辑\n  return result;\n}\n`,
        codeLang: "ts",
        targetFile: "src/lib/tasks.ts",
        explanation: "函数是一台机器：参数是原材料，返回值是成品。",
      };
    },
  },
];

/** 意图识别 + 通俗步骤 + 代码生成（spec 6.2 全流程）。 */
export function planIntent(text: string, detect: ProjectDetect, lang: "zh" | "zh-TW" | "en"): IntentPlan {
  for (const rule of RULES) {
    const m = text.match(rule.keywords);
    if (m && rule.fits(detect)) {
      const plan = rule.build(detect, m, text);
      return { matched: true, ...plan, unknownTerms: [] };
    }
  }
  const unknownTerms = lang !== "en"
    ? ["还没学会这个需求对应的模板"]
    : ["no template learned for this request yet"];
  return {
    matched: false,
    title: lang !== "en" ? "暂时听不懂这个需求" : "Cannot understand this request yet",
    steps: lang !== "en"
      ? [
          "我可以听懂这些说法：做一个按钮并弹出提示 / 请求接口拿数据 / 保存数据 / 新增函数。",
          "你也可以换一种说法，比如“我想做一个按钮，点了之后弹出你好”。",
        ]
      : [
          "Understood requests: a button that pops a message / fetch API data / save data / add a function.",
          "Try rephrasing, e.g. \"I want a button that shows hello when clicked\".",
        ],
    code: "",
    codeLang: "text",
    targetFile: "",
    explanation: "",
    unknownTerms,
  };
}
