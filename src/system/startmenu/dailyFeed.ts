/**
 * 开始菜单「今日面板」本地内容源（零网络、零外部数据）。
 *
 * 用途：新版开始菜单右侧两张信息卡（功能问答 / 今日提示）与底部推荐搜索的内容来源。
 * 诚实边界：这里**没有**任何联网取回的新闻、壁纸或时讯；全部条目都是本引擎自身
 * 真实存在的功能问答与提示，按本地日期轮换，离线可用、可预测、可测试。
 */

export interface DailyQuiz {
  q: string;
  options: string[];
  /** 正确项下标 */
  answer: number;
  /** 答对后的说明（也是本引擎的真实行为） */
  note: string;
}

export interface DailyTip {
  title: string;
  note: string;
}

/** 本引擎功能问答库（不会随机：按日期轮换，保证同一天内稳定）。 */
const QUIZ_BANK: DailyQuiz[] = [
  {
    q: "想把任务栏停靠到屏幕左侧，去哪一页改？",
    options: ["设置 › 外观", "设置 › 系统 › 声音", "桌面右键菜单"],
    answer: 0,
    note: "外观页的「窗口与任务栏」卡片里可切换任务栏四向停靠（下 / 左 / 右 / 上）。",
  },
  {
    q: "开始菜单里直接敲字母，会发生什么？",
    options: ["打开全局搜索", "跳到对应首字母的应用", "切换主题"],
    answer: 1,
    note: "已固定应用支持字母索引与拼音首字母直达，命中项会高亮，回车即可启动。",
  },
  {
    q: "「每日自动换」壁纸的图片来自哪里？",
    options: ["在线图库", "本机指定目录的缓存池", "引擎内置素材包"],
    answer: 1,
    note: "只读取本机目录，零网络：在外观页选定图片目录后即可参与每日轮换。",
  },
  {
    q: "性能档位选择「自动」时由谁决定档位？",
    options: ["由引擎按当前负载自动挑选", "始终锁最高档", "锁最低档省电"],
    answer: 0,
    note: "自动档会结合当前负载与安全模式挑选合适档位；也可手动覆盖成固定档。",
  },
  {
    q: "窗口控制按钮能不能换成左侧（macOS 风格）？",
    options: ["可以，外观页可切换", "只能右侧", "需要重装"],
    answer: 0,
    note: "外观页「窗口与任务栏」里提供「窗口控制按钮位置」：mac / windows 两种布局即时生效。",
  },
  {
    q: "任务栏正在运行的应用下面那点标记，可以换样式吗？",
    options: ["不可以", "可以，三选：圆点 / 下划线 / 胶囊", "只有圆点"],
    answer: 1,
    note: "「运行指示样式」提供圆点、下划线、胶囊三种，改完立即生效，无需重启。",
  },
  {
    q: "设置中心里页数很多，怎么快速找到某一页？",
    options: ["只能一页页翻", "用标题栏中间的搜索框过滤", "记住顺序"],
    answer: 1,
    note: "设置窗口顶部的搜索框会实时过滤左侧导航；清空后列表与原来逐项一致。",
  },
  {
    q: "多块显示器可以分别指定壁纸吗？",
    options: ["可以，外观页支持逐屏设置", "只能全部相同", "不支持多屏"],
    answer: 0,
    note: "检测到多显示器时，外观页会列出每块屏幕，可单独设置，也可一键应用到全部。",
  },
];

/** 今日提示库（同样按日期轮换）。 */
const TIP_BANK: DailyTip[] = [
  {
    title: "用字母跳转开始菜单",
    note: "打开开始菜单后直接敲字母（或拼音首字母），命中项会高亮，回车即可启动。",
  },
  {
    title: "设置也能搜",
    note: "设置窗口标题栏中间就是搜索框，输入关键字即可过滤全部设置页。",
  },
  {
    title: "壁纸全部来自本机",
    note: "图片、视频、Shader 与每日自动换都只读本机资源，全程零网络。",
  },
  {
    title: "一次改完窗口手感",
    note: "窗口控制按钮位置、任务栏停靠、运行指示样式都在「外观」页，改完立即生效。",
  },
  {
    title: "让引擎自己挑档位",
    note: "性能档位选「自动」后，引擎会按当前负载与安全模式挑选合适档位。",
  },
  {
    title: "桌面图标大小",
    note: "外观页可在小 / 中 / 大之间切换桌面图标尺寸（32 / 48 / 64）。",
  },
];

/** 本地日期序号（本地日历日，忽略时分秒；跨时区不会跳变）。 */
export function daySerial(d: Date): number {
  const t = Date.UTC(d.getFullYear(), d.getMonth(), d.getDate());
  return Math.floor(t / 86_400_000);
}

/** 稳定取模（负数也落到 [0, n)）。 */
export function modIndex(serial: number, n: number): number {
  if (n <= 0) return 0;
  return ((serial % n) + n) % n;
}

/** 当天问答（同一天内恒定，跨天轮换）。 */
export function dailyQuiz(d: Date = new Date()): DailyQuiz {
  return QUIZ_BANK[modIndex(daySerial(d), QUIZ_BANK.length)]!;
}

/** 当天提示（用不同偏移，避免与问答同频）。 */
export function dailyTip(d: Date = new Date()): DailyTip {
  return TIP_BANK[modIndex(daySerial(d) + 3, TIP_BANK.length)]!;
}

/** 日期短标签：`9月14日`（en 用 `Sep 14`）。 */
export function dayLabel(d: Date, lang: string): string {
  if (lang === "en") {
    const M = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    return `${M[d.getMonth()]} ${d.getDate()}`;
  }
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}

/** 星期标签（zh 短式 / en 短式）。 */
export function weekdayLabel(d: Date, lang: string): string {
  const zh = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
  const en = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
  const list = lang === "en" ? en : zh;
  return list[d.getDay()] ?? "";
}

/** 推荐搜索词：候选去重后按日期轮换起点取 n 个（不随机，同一天内稳定）。 */
export function pickTrending(candidates: string[], d: Date = new Date(), n = 4): string[] {
  const uniq: string[] = [];
  for (const c of candidates) {
    const s = c.trim();
    if (s && !uniq.includes(s)) uniq.push(s);
  }
  if (uniq.length <= n) return uniq;
  const start = modIndex(daySerial(d), uniq.length);
  const out: string[] = [];
  for (let i = 0; i < n; i++) out.push(uniq[(start + i) % uniq.length]!);
  return out;
}
