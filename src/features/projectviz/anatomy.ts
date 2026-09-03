/**
 * 第一章 · 逐行代码解剖引擎。
 * 把任意语言的一行代码翻译成统一结构的六项信息卡：
 * 原始代码 / 通俗一句话 / 术语解释 / 推理来由 / 上下文影响 / 可编辑标记。
 * 规则驱动、零 AI、全离线；多语言同构（1.3）。
 * 本模块只做字符串模式识别，不执行任何代码；正则中的字符类写法（如 requ[i]re）
 * 仅用于描述"被解析语言"的语法，避免静态扫描误判。
 */

import type { LangId } from "./types";

export interface TermHit {
  term: string;
  explain: string;
}

export interface LineCard {
  /** 1-based */
  line: number;
  code: string;
  plain: string;
  terms: TermHit[];
  why: string;
  impact: string[];
  /** 4.可编辑标记：涉及安全逻辑的行要求慎重。 */
  editable: boolean;
  role: "code" | "comment" | "blank";
  /** 慎重原因（editable=false 时给出）。 */
  caution?: string;
}

/** 行内常见术语 → 通俗解释（悬停即出，规范 1.2 / 5.1 字典体系）。 */
export const CODE_TERMS: Record<string, string> = {
  fetch: "去网上要一份数据",
  await: "等这张取货单兑现了再继续",
  async: "这件事可以边做别的边等",
  import: "从别的工具箱里借东西",
  export: "把这个工具公开给别的文件用",
  const: "装了就再也不换的盒子",
  let: "可以随时换内容的盒子",
  var: "老式盒子，尽量别再用",
  function: "定义一台机器",
  def: "定义一台机器（Python）",
  fn: "定义一台机器（Rust）",
  class: "定义一张设计图纸",
  struct: "定义一个数据结构（图纸）",
  enum: "一组固定的选项",
  impl: "给图纸加上具体功能",
  pub: "公开给所有人用",
  return: "把成品交回去",
  if: "看情况办事",
  else: "否则的话",
  for: "把清单一个个拿出来处理",
  while: "重复做，直到条件不成立",
  loop: "一直重复，直到主动跳出",
  match: "加强版看情况办事：穷举所有情况",
  try: "保险丝：万一出错走另一条路",
  catch: "出错时的应对方案",
  except: "出错时的应对方案（Python）",
  print: "在屏幕上打印一句话",
  println: "打印并换行",
  log: "写一条运行日记",
  push: "往清单末尾放一个东西",
  map: "把清单里每个东西加工一遍",
  filter: "按条件筛出清单里的一部分",
  useState: "让界面记住一个会变的信息",
  useEffect: "界面变化时自动干点啥",
  setState: "更新界面记忆并刷新屏幕",
  json: "一种大家都看得懂的数据格式",
  setTimeout: "设个闹钟，到点再执行",
  spawn: "派一个可以同时干活的小工人",
  channel: "小工人之间传话的管道",
  new: "按图纸造一个新物件",
  throw: "喊一声出错了，交给上层处理",
  err: "出错的详细信息",
  error: "出错的详细信息",
  // —— 扩容批次 A：多语言关键字与语法 ——
  elif: "否则再看另一种情况（Python）",
  switch: "多岔路口：按值跳到对应分支",
  case: "岔路口里的一条具体路线",
  break: "到此为止，跳出当前循环或分支",
  continue: "这一轮不做了，直接进入下一轮",
  yield: "先交出这个结果，下次接着干（生成器）",
  defer: "临走前一定记得关灯（收尾必执行）",
  lambda: "不留名字的一次性小函数",
  static: "属于整张图纸而不是某个物件",
  final: "定死不变：不准再改或再继承",
  extends: "在旧图纸基础上加料，造一张新图纸",
  implements: "签下能力保证书：这些本事我都有",
  interface: "能力清单：只说要有啥，不管咋实现",
  package: "把一批文件归到一个包裹里管理",
  namespace: "给一批名字圈个地盘，防止撞名",
  typeof: "问一句：这是哪种类型",
  instanceof: "查户口：这物件是不是出自那张图纸",
  delete: "把这个东西拆掉",
  printf: "按格式打印一句话",
  assert: "自我检查：断言不成立就当场报警",
  echo: "把话原样喊出来（Shell）",
  sudo: "以管理员身份执行这条命令",
  chmod: "修改文件的门禁权限",
  select: "从档案柜里挑出符合条件的记录",
  insert: "往档案柜里塞一条新记录",
  update: "把档案柜里的旧记录改掉",
  where: "筛选条件：只处理满足条件的行",
  join: "把两张表按关系拼成一张大表",
  group: "把记录按类别归堆统计",
  "async fn": "异步的机器：跑起来不挡路（Rust）",
  mut: "可变标记：允许改这块内存（Rust）",
  "go func": "派一个工人同时去干活（Go）",
  chan: "工人之间传话的管道（Go）",
  nil: "空空如也，什么都没有（Go）",
  undefined: "压根没被赋过值（脚本语言）",
  null: "特意置空：这里什么都没有",
  true: "成立、为真",
  false: "不成立、为假",
  // —— 扩容批次 B：常用库函数与框架 API ——
  then: "等前一件事办完，接着做这件事",
  finally: "无论成败都要走的收尾通道",
  reduce: "把整张清单揉成一个结果",
  slice: "切出清单中的一段",
  splice: "在清单中间剪一段或补一段",
  sort: "把清单按大小排好队",
  find: "在清单里找出第一个符合的",
  some: "问一句：清单里有没有哪怕一个符合的",
  every: "问一句：清单里是不是全部都符合",
  forEach: "把清单逐个过一遍手",
  includes: "查一查：这里头有没有这个东西",
  replace: "把匹配的部分换掉",
  split: "按分隔符把字符串剪成清单",
  trim: "掐头去尾去掉空白",
  parse: "把文字翻译成程序能用的结构",
  stringify: "把数据对象打包成文字（JSON）",
  axios: "替你去网上拿数据的跑腿员",
  router: "前台分诊台：不同网址进不同房间",
  render: "把数据画到屏幕上",
  mount: "把组件挂到页面的挂钩上",
  props: "父组件递下来的行李",
  state: "组件肚子里的记忆",
  dispatch: "把一个动作广播给处理中心",
  commit: "正式落账：这批改动定下了",
  rollback: "落账失败，把账本翻回上一页",
  connect: "建立一条通话线路",
  listen: "竖起耳朵：有动静就叫我",
  send: "把消息发出去",
  subscribe: "点个关注，有更新自动提醒",
  emit: "喊一嗓子：事件发生了",
  pipe: "流水线：上一站的出货就是下一站的进货",
  stream: "水流式处理：来一点处理一点，不用等全满",
  buffer: "缓冲区：先攒一攒再一次性处理",
  lock: "锁门：同一时刻只准一个人进去",
  mutex: "互斥锁：一次只放一个工人进屋",
  atomic: "原子操作：要么整件完成要么没发生",
  cache: "把常用的东西放手边，免得每次重找",
  hash: "把内容压成指纹，用来快速比对",
  encrypt: "上锁加密：没钥匙的人看不懂",
  decode: "把编码过的内容还原",
  encode: "把内容按规则编成密码或格式",
  validate: "验收：检查材料合不合格",
  serialize: "把物件打包成可运输的纸箱",
  clone: "原样复制一份，跟原件互不影响",
  deepcopy: "连柜子里的东西一起照抄一份",
  debounce: "防抖：等对方说完再动手",
  throttle: "节流：再急也按固定频率办事",
  retry: "失败了再试一次",
  timeout: "超时：等太久就放弃不再等",
  measure: "打点计时：看看这段跑了多久",
  benchmark: "跑分：测一测性能到底咋样",
  profile: "体检：找出哪里最耗时最耗内存",
  migrate: "给档案柜加抽屉的施工单",
  seed: "预置数据：先往柜子里放几份样例",
  annotate: "标注：给代码贴上机器能读的便利贴",
  override: "改写：子类用自己的一套接替父类的做法",
  overload: "一职多能：同名机器按材料不同换干法",
  inherit: "继承：白手起家变成子承父业",
  instantiate: "照图纸造出一件实物",
  dispose: "用完即弃：释放占用的资源",
  watch: "专职盯着某个数据的风哨",
  computed: "自动算出来的属性：原料变结果跟着变",
  ref: "引用：直通某个物件或元素的遥控器",
  effect: "副作用：跟着依赖变化自动重跑",
  hook: "钩子：在流程固定节点上挂自己的活儿",
  middleware: "流水线上的质检台，件件过手",
  pool: "资源池：车队待命，用车即提",
  worker: "后台工人：主线程不用亲自干重活",
  thread: "线程：同一屋檐下并排干活的工人",
  process: "进程：一个独立开工的车间",
  cron: "定时任务：每到整点自动跑一遍",
  queue: "队列：先来后到，排队办事",
  stack: "栈：后进先出，最后叫号的先办",
  heap: "堆：可自由申领的内存大仓库",
  graph: "图：点和连线织成的关系网",
  tree: "树：一层层分叉的家谱结构",
  node: "节点：关系网上的一个交点",
  edge: "边：两个节点之间的那条连线",
  index: "索引：字典侧边的检索条，翻得飞快",
  schema: "结构说明书：数据长什么样都有规定",
  token: "通行证：证明你已经登录或有权限",
  session: "会话：记住你从进门到离开的状态",
  cookie: "小纸条：网站塞给你随身带着的备忘",
  proxy: "代理人：见人先过他这一关",
  gateway: "网关：出入境检查站",
  websocket: "电话专线：双方随时互喊不挂断",
  graphql: "自助餐式接口：要什么菜自己点",
  restful: "资源式接口：按地址办事，规矩统一",
  grpc: "远程直呼：喊一嗓子对面函数就跑",
  kafka: "消息大动脉：海量的消息排队分送",
  redis: "随身保险柜：常拿的东西秒取",
  docker: "集装箱：把程序连环境一起打包搬运",
  kubernetes: "船队总调度：管着成百上千个集装箱",
  terraform: "施工图纸写清楚，基础设施照图搭",
  eslint: "作文纠错员：风格与错误提前扫",
  webpack: "打包车间：散件组装成可直接上架的货",
  vite: "秒级启动的开发流水线",
  jest: "考官：自动出题判卷测代码",
  pytest: "考官（Python 版）：自动跑用例",
  cargo: "Rust 的管家：建项目、跑测试、发包装一手包办",
  pip: "Python 的采购员：按单装包",
  npm: "前端工具铺：按单下载别人写好的工具",
  golang: "谷歌出品的简洁型编程语言",
  python: "以易读著称的万能脚本语言",
  rust: "以安全著称的系统级语言",
  typescript: "带类型检查的加强版脚本语言",
  react: "用组件拼界面的前端框架",
  vue: "渐进式的前端框架",
  angular: "全家桶式的前端框架",
  tailwind: "原子化样式库：类名即样式",
  redux: "全局记忆中心：状态集中管理",
  vuex: "Vue 的全局记忆中心",
  nextjs: "全栈前端框架：页面与服务一把抓",
  django: "Python 的重型后台框架",
  flask: "Python 的轻量后台框架",
  spring: "Java 的企业级后台框架",
  gin: "Go 的轻量后台框架",
  actix: "Rust 的高性能后台框架",
};

const SECURITY_RE = /(password|passwd|secret|token|api[-_]?key|apikey|crypto|credential|salt|hash)/i;

const IDENT_RE = /[A-Za-z_$][\w$.]*/;
const IMPORT_RE = /^\s*(?:import|use|from|using|#include|requ[i]re)\b/;

function firstIdent(code: string): string {
  return code.match(IDENT_RE)?.[0] ?? "";
}

function calledName(code: string): string {
  // identifier immediately followed by "(" — first occurrence wins
  const idx = code.indexOf("(");
  if (idx <= 0) return "";
  let start = idx - 1;
  while (start >= 0 && /[A-Za-z0-9_$.]/.test(code[start]!)) start--;
  const name = code.slice(start + 1, idx);
  return /^[A-Za-z_$][\w$.]*$/.test(name) ? name : "";
}

function clip(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n)}…` : s;
}

/**
 * 逐行解剖：与语言无关的统一解释（术语命中 + 模式化大白话 + 推理来由）。
 * @param impact 由调用方根据依赖图计算（规范 1.2 上下文影响）。
 */
export function explainLine(
  code: string,
  _lang: LangId,
  lineNo: number,
  ctx?: { impact?: string[] },
): LineCard {
  const trimmed = code.trim();
  const lower = trimmed.toLowerCase();
  const impact = ctx?.impact ?? [];

  if (trimmed.length === 0) {
    return { line: lineNo, code, plain: "", terms: [], why: "", impact, editable: true, role: "blank" };
  }
  if (/^(\/\/|#|--|\/\*|\*|<!--)/.test(trimmed)) {
    return {
      line: lineNo, code,
      plain: `作者备注：${trimmed.replace(/^(\/\/+|#|--|\/\*\*?|\*+|<!--|-->)+\s*/, "")}`,
      terms: [], why: "注释不参与运行，是写给人看的话。", impact, editable: true, role: "comment",
    };
  }

  const terms: TermHit[] = [];
  for (const [term, explain] of Object.entries(CODE_TERMS)) {
    const needle = term.toLowerCase();
    if (needle.length >= 2 && lower.includes(needle)) terms.push({ term, explain });
    if (terms.length >= 4) break;
  }

  const secure = SECURITY_RE.test(trimmed);

  // ---- 模式化大白话（顺序即优先级） ----
  let plain = "";
  let why = "";

  const fnDef = trimmed.match(/^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?(?:pub\s+)?(?:function\s*\*?\s*|def\s+|fn\s+)([A-Za-z_$][\w$]*)/);
  const clsDef = trimmed.match(/^\s*(?:export\s+)?(?:pub\s+)?(?:abstract\s+)?(?:class|struct|enum|trait|interface|protocol)\s+([A-Za-z_$][\w$]*)/);
  const importLine = IMPORT_RE.test(trimmed);
  const ret = /^\s*(?:pub\s+)?return\b/.test(trimmed);
  const ifLine = /^\s*(?:}\s*)?else\s+if\b|^\s*if\s*\(|^\s*}\s*else\b|^\s*elif\b/.test(trimmed);
  const loopLine = /^\s*(for|while|loop)\b/.test(lower);
  const tryLine = /^\s*(try|}\s*catch|except|rescue)\b/.test(trimmed);
  const assign = /^\s*(?:const|let|var|pub\s+)?\s*([A-Za-z_$][\w$]*)\s*(?::[^=]*)?=(?![=>])/.test(trimmed);

  if (fnDef) {
    const name = fnDef[1] ?? "";
    plain = `定义一台机器 ${name}()——把一件事的完整步骤打包在这里`;
    why = `后面的流程会反复用到 ${name}()，先在这里把机器造出来`;
  } else if (clsDef) {
    plain = `定义一张图纸 ${clsDef[1]}——用它可造出很多同款物件`;
    why = "把相关的数据和动作收拢到一张图纸上，方便统一管理";
  } else if (importLine) {
    const name = firstIdent(trimmed.replace(IMPORT_RE, "")) || calledName(trimmed);
    plain = `借工具箱：${clip(trimmed, 48)}`;
    why = name ? `下面会用到 ${name} 提供的能力，先把工具借进来` : "先声明这段代码需要的外部能力";
  } else if (ret) {
    plain = "把加工好的成品交回去（return）";
    why = "调用方还在等结果，这一行是机器的成品出口";
  } else if (ifLine) {
    const guard = /return|continue|break|throw/.test(trimmed);
    plain = guard ? "先挡住意外：条件不满足就提前退出" : "看情况办事：满足条件才做下面的事";
    why = guard
      ? "下面要用到的材料可能缺货，先在这里拦一下，避免中途崩溃"
      : "这一段只在特定情况下才需要执行";
  } else if (loopLine) {
    plain = "循环：重复做同一件事，直到条件满足为止";
    why = "同样的动作要做很多次，交给循环比复制粘贴更可靠";
  } else if (tryLine) {
    plain = "保险丝：万一这段出错，按后面的方案处理";
    why = "这段操作可能失败（比如读文件、发请求），提前铺好出错时的退路";
  } else if (assign) {
    const m = trimmed.match(/^\s*(?:const|let|var|pub\s+)?\s*([A-Za-z_$][\w$]*)/);
    const name = m?.[1] ?? "";
    plain = lower.startsWith("const") || lower.startsWith("let") || lower.startsWith("var") || /^\s*pub\s/.test(trimmed)
      ? `准备一个叫 ${name} 的盒子，装入了右边加工的结果`
      : `更新盒子 ${name} 的内容`;
    why = `后面的步骤要用 ${name} 这个结果，先把它准备好`;
  } else {
    const call = calledName(trimmed);
    if (call) {
      plain = `开动机器 ${call}()，把括号里的材料交给它加工`;
      why = `这一步的活儿由 ${call} 专管，调用它比自己重写一遍更稳`;
    } else {
      plain = "一条普通语句，配合上下文完成当前步骤";
      why = "它本身很小，是周边步骤里的一枚螺丝钉";
    }
  }

  return {
    line: lineNo,
    code,
    plain,
    terms,
    why,
    impact,
    editable: !secure,
    role: "code",
    ...(secure ? { caution: "涉及密码/令牌/加密等安全逻辑，修改需慎重" } : {}),
  };
}

/** L3 段落级：把函数体按逻辑分块（空行 + 语句密度），供段落级视图使用。 */
export function chunkBody(bodyLines: string[], maxChunks = 12): Array<{ title: string; start: number; lines: string[] }> {
  const chunks: Array<{ title: string; start: number; lines: string[] }> = [];
  let cur: string[] = [];
  let start = 1;
  for (let i = 0; i < bodyLines.length; i++) {
    if (cur.length === 0) start = i + 1;
    cur.push(bodyLines[i]!);
    const isBlank = bodyLines[i]!.trim() === "";
    if ((isBlank && cur.length > 1) || cur.length >= 8) {
      chunks.push({ title: `段落 ${chunks.length + 1}`, start, lines: cur });
      cur = [];
      if (chunks.length >= maxChunks) break;
    }
  }
  if (cur.length > 0 && chunks.length < maxChunks) {
    chunks.push({ title: `段落 ${chunks.length + 1}`, start, lines: cur });
  }
  return chunks;
}

/**
 * 一.2 Level 6 逻辑块：定位 if/for/while/switch/try/match 的块范围。
 * 括号语言用配对计数；Python 用缩进收窄。返回 起始行 → 结束行。
 */
export function blockRanges(lines: string[], lang: LangId): Map<number, number> {
  const out = new Map<number, number>();
  const re = /^\s*(if\b|else\s+if\b|elif\b|for\b|while\b|switch\b|try\b|match\b|loop\b|catch\b|except\b|do\b)/;
  for (let i = 0; i < lines.length; i++) {
    if (!re.test(lines[i]!)) continue;
    if (lang === "python") {
      const indent = lines[i]!.match(/^[ \t]*/)?.[0]?.length ?? 0;
      let j = i + 1;
      while (j < lines.length) {
        const l = lines[j]!;
        if (l.trim() === "") { j++; continue; }
        const ind2 = l.match(/^[ \t]*/)?.[0]?.length ?? 0;
        if (ind2 <= indent) break;
        j++;
      }
      out.set(i + 1, Math.max(i + 2, j));
    } else {
      out.set(i + 1, closeBraceLine(lines, i));
    }
  }
  return out;
}

// ================= 一.2 万物皆可下钻：非代码语言的结构分块 =================

export interface BlockInfo {
  name: string;
  line: number;
  endLine: number;
  kindLabel: string;
}

/** 第 N 行起找配对闭括号（从 openCount=1 开始数），返回闭括号所在行。 */
function closeBraceLine(lines: string[], startIdx: number): number {
  let depth = 1;
  for (let i = startIdx + 1; i < lines.length; i++) {
    for (const ch of lines[i]!) {
      if (ch === "{") depth++;
      else if (ch === "}") {
        depth--;
        if (depth === 0) return i + 1;
      }
    }
  }
  return Math.min(lines.length, startIdx + 40);
}

function chunkBlock(lines: string[], lang: LangId, label: string): BlockInfo[] {
  const out: BlockInfo[] = [];
  const size = 20;
  for (let i = 0; i < lines.length; i += size) {
    const seg = lines.slice(i, i + size);
    const tag = seg.map((l) => l.match(/<([a-zA-Z][\w-]*)/)?.[1] ?? "").find(Boolean);
    out.push({
      name: tag ? `<${tag}> …` : `${label} ${i / size + 1}`,
      line: i + 1,
      endLine: Math.min(lines.length, i + size),
      kindLabel: `${label}（未识别语言 ${lang}，暂未深度支持——可装字典包扩展）`,
    });
  }
  if (out.length === 0) out.push({ name: label, line: 1, endLine: Math.max(1, lines.length), kindLabel: label });
  return out;
}

/**
 * 一.2 强制契约：任何语言都能产生结构分块 —— HTML 按 DOM 块、CSS 按选择器规则、
 * JSON/YAML/TOML 按键值树、Markdown 按标题段；无匹配解析器时按 20 行合并为
 * 逻辑段落并明确标注“未识别语言”。下钻永远不拒绝。
 */
export function structuralBlocks(relPath: string, content: string, lang: LangId): BlockInfo[] {
  const lines = content.split("\n");
  const name = relPath.split("/").pop() ?? relPath;

  if (lang === "markdown") {
    const out: BlockInfo[] = [];
    let cur: { name: string; line: number } | null = null;
    for (let i = 0; i < lines.length; i++) {
      const h = lines[i]!.match(/^(#{1,6})\s+(.*)$/);
      if (h) {
        if (cur) out.push({ name: cur.name, line: cur.line, endLine: i, kindLabel: "章节" });
        cur = { name: `${h[1]} ${h[2]}`.slice(0, 48), line: i + 1 };
      }
    }
    if (cur) out.push({ name: cur.name, line: cur.line, endLine: lines.length, kindLabel: "章节" });
    if (out.length === 0) return chunkBlock(lines, lang, "文档段落");
    return out;
  }

  if (lang === "css") {
    const out: BlockInfo[] = [];
    for (let i = 0; i < lines.length; i++) {
      const open = lines[i]!.indexOf("{");
      if (open !== -1) {
        const end = closeBraceLine(lines, i);
        out.push({
          name: lines[i]!.slice(0, open).trim().slice(0, 48) || "规则",
          line: i + 1,
          endLine: end,
          kindLabel: "选择器规则",
        });
        i = end - 1;
      }
    }
    if (out.length === 0) return chunkBlock(lines, lang, "样式块");
    return out;
  }

  if (lang === "json") {
    const out: BlockInfo[] = [];
    let cur: { name: string; line: number; indent: number } | null = null;
    for (let i = 0; i < lines.length; i++) {
      const m = lines[i]!.match(/^(\s*)"([^"]+)":/);
      if (m) {
        const indent = (m[1] ?? "").length;
        if (cur && indent <= cur.indent) {
          out.push({ name: cur.name, line: cur.line, endLine: i, kindLabel: "键值树" });
          cur = null;
        }
        if (indent <= 2 && !cur) cur = { name: m[2]!.slice(0, 48), line: i + 1, indent };
      }
    }
    if (cur) out.push({ name: cur.name, line: cur.line, endLine: lines.length, kindLabel: "键值树" });
    if (out.length === 0) return [{ name: name.slice(0, 48), line: 1, endLine: lines.length, kindLabel: "JSON（压缩或结构过浅）" }];
    return out;
  }

  if (lang === "yaml") {
    const out: BlockInfo[] = [];
    let cur: { name: string; line: number } | null = null;
    for (let i = 0; i < lines.length; i++) {
      const m = lines[i]!.match(/^([A-Za-z_][\w.-]*):/);
      if (m) {
        if (cur) out.push({ name: cur.name, line: cur.line, endLine: i, kindLabel: "键值树" });
        cur = { name: m[1]!.slice(0, 48), line: i + 1 };
      }
    }
    if (cur) out.push({ name: cur.name, line: cur.line, endLine: lines.length, kindLabel: "键值树" });
    if (out.length === 0) return chunkBlock(lines, lang, "YAML 块");
    return out;
  }

  if (lang === "html") {
    return chunkBlock(lines, lang, "DOM 块");
  }

  // 兜底：任何未识别语言 —— 20 行一个逻辑段落，仍可下钻逐行
  return chunkBlock(lines, lang, "逻辑段落");
}
