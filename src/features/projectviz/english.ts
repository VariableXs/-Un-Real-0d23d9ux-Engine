/**
 * v3.0 ch.2 · English ecosystem + unified dictionary hub helpers.
 *
 * 2.2 语言感知智能分词: CamelCase / snake_case / kebab-case identifiers are
 * split into semantic words, then matched against a bilingual programming
 * verb dictionary to produce PLAIN ENGLISH narratives — code is explained
 * with everyday phrasing, not literal translation.
 *
 * 2.1 多类型项目字典: each detected project domain (web/desktop/data/cli/
 * mobile/game) owns a causal narrative chain ("requests → parsing → storage")
 * shared with the fate engine's cause→effect chain model.
 */

/** Split any identifier into lowercase semantic words. */
export function splitIdentifier(name: string): string[] {
  return name
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2") // camelCase / PascalCase
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2") // HTTPServer → HTTP Server
    .split(/[\s_\-.:]+/)
    .map((w) => w.trim().toLowerCase())
    .filter(Boolean);
}

/** Programming verbs → plain-English gerund templates (%s = the object). */
const VERBS: [string, (obj: string) => string][] = [
  ["fetch", (o) => `Getting the ${o} from its source`],
  ["get", (o) => `Getting the ${o}`],
  ["read", (o) => `Reading the ${o}`],
  ["load", (o) => `Loading the ${o}`],
  ["parse", (o) => `Breaking the ${o} down into useful pieces`],
  ["analyze", (o) => `Taking a close look at the ${o}`],
  ["save", (o) => `Storing the ${o} for later`],
  ["write", (o) => `Writing the ${o} down`],
  ["send", (o) => `Sending the ${o} on its way`],
  ["update", (o) => `Updating the ${o} to the latest state`],
  ["delete", (o) => `Removing the ${o}`],
  ["remove", (o) => `Removing the ${o}`],
  ["create", (o) => `Creating a new ${o}`],
  ["build", (o) => `Building the ${o} piece by piece`],
  ["handle", (o) => `Responding when the ${o} happens`],
  ["validate", (o) => `Double-checking the ${o} before use`],
  ["check", (o) => `Checking the ${o}`],
  ["render", (o) => `Drawing the ${o} on screen`],
  ["draw", (o) => `Drawing the ${o}`],
  ["init", (o) => `Setting up the ${o} before anything else`],
  ["setup", (o) => `Setting up the ${o}`],
  ["compute", (o) => `Crunching the numbers for the ${o}`],
  ["calc", (o) => `Crunching the numbers for the ${o}`],
  ["convert", (o) => `Turning the ${o} into another format`],
  ["transform", (o) => `Reshaping the ${o}`],
  ["merge", (o) => `Combining the ${o} into one`],
  ["retry", (o) => `Trying the ${o} again after a failure`],
  ["detect", (o) => `Spotting the ${o} automatically`],
  ["crawl", (o) => `Collecting the ${o} page by page`],
  ["request", (o) => `Asking the server for the ${o}`],
];

const FALLBACK = (verb: string, obj: string): string => `Working on the ${obj} (${verb})`;

/**
 * fetchUserData → "Getting the user data from its source."
 * parse_html_report → "Breaking the html report down into useful pieces."
 */
export function plainEnglishFunction(name: string, params: string[] = []): string {
  const words = splitIdentifier(name);
  if (words.length === 0) return `A helper called ${name}.`;
  const verb = words[0]!;
  const obj = words.slice(1).join(" ") || "requested work";
  const entry = VERBS.find(([v]) => verb.startsWith(v));
  const core = entry ? entry[1](obj) : FALLBACK(verb, obj);
  const extra = params.length > 0 ? ` Input: ${params.join(", ")}.` : "";
  return `${core}.${extra}`;
}

/** Plain-English one-liner for a file name (spec 2.2 examples). */
export function plainEnglishFile(fileName: string): string {
  const stem = fileName.replace(/\.[^.]+$/, "");
  const words = splitIdentifier(stem).filter((w) => !["index", "main", "mod", "app"].includes(w));
  const topic = words.length > 0 ? words.join(" ") : stem;
  return `This file takes care of everything about the ${topic}.`;
}

/**
 * 2.1 · per-domain causal chains (unified with the fate engine's
 * cause → effect chain model). Keyed by detect.ts domain ids.
 */
const DOMAIN_CHAINS: Record<string, { zh: string[]; en: string[] }> = {
  web: {
    zh: ["用户打开页面", "框架把界面拼装出来", "界面发请求要数据", "数据填进界面", "用户看到结果"],
    en: ["the user opens a page", "the framework assembles the UI", "the UI asks for data", "data fills the view", "the user sees the result"],
  },
  desktop: {
    zh: ["程序启动", "读入配置和本地数据", "把界面画到窗口", "响应用户操作", "把改动写回磁盘"],
    en: ["the app starts", "settings and local data load", "the window is drawn", "user actions are handled", "changes are saved back to disk"],
  },
  data: {
    zh: ["拿到原始数据", "网络请求抓取内容", "解析并清洗数据", "计算统计指标", "输出结果或图表"],
    en: ["raw data arrives", "content is fetched over the network", "data is parsed and cleaned", "numbers are crunched", "results or charts come out"],
  },
  cli: {
    zh: ["命令行启动", "解析命令与参数", "读取输入来源", "执行核心逻辑", "打印结果到终端"],
    en: ["the command starts", "flags and arguments are parsed", "the input source is read", "the core logic runs", "results print to the terminal"],
  },
  mobile: {
    zh: ["应用唤醒", "恢复上次界面状态", "渲染界面与动画", "处理触摸与手势", "同步数据到服务端"],
    en: ["the app wakes up", "the last screen state restores", "views and animations render", "touches and gestures are handled", "data syncs to the server"],
  },
  game: {
    zh: ["游戏初始化", "加载资源与场景", "每帧循环：输入→模拟→渲染", "实体间发生碰撞与事件", "状态推进并存档"],
    en: ["the game initializes", "assets and scenes load", "each frame: input → simulation → render", "entities collide and events fire", "state advances and saves"],
  },
};

/** 一句话因果链（2.1 自适应逻辑链），直接可展示。 */
export function domainNarrative(domain: string, lang: "zh" | "en"): string {
  const chain = DOMAIN_CHAINS[domain];
  if (!chain) return "";
  const steps = lang === "zh" ? chain.zh.join(" → ") : chain.en.join(" → ");
  return lang === "zh"
    ? `它的运行逻辑链：${steps}。`
    : `How it runs, step by step: ${steps}.`;
}
