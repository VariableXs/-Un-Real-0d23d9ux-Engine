/**
 * U3-v4 双语标签词典（AI-U3 · labels）。
 *
 * 纪律唯一源（入格宪章十四章 + F140 本地化开放 + B-1104 零硬编码文案）：
 * 「双语切换全界面走查（30 关键页零漏翻零硬编码）」——v4 引擎群与新实验室
 * 面板的全部用户可见文案入本词典，禁止组件内散写中文字面量。
 *
 * 与全局 i18n（src/i18n/dictionaries.ts）的分工：全局词典收系统级键；
 * 本词典收 U3 域内键（引擎名/组名/状态文案），结构对齐（zh/en 双键），
 * 导出 selector 供组件按当前语言取词。
 */

export type U3Lang = "zh" | "en";

export interface U3LabelPair {
  zh: string;
  en: string;
}

/** 引擎群标签（v4 十三引擎 + v5 三装配引擎）。 */
export const U3_ENGINE_LABELS: Readonly<Record<string, U3LabelPair>> = {
  lumapick:   { zh: "壁纸亮度采样引擎", en: "Wallpaper Luma Sampler" },
  gridlab:    { zh: "图标网格规划器", en: "Icon Grid Planner" },
  pinvault:   { zh: "PIN 与动态锁引擎", en: "PIN & Dynamic Lock" },
  guestbox:   { zh: "访客沙盒账引擎", en: "Guest Sandbox Ledger" },
  capguard:   { zh: "截图防护裁决器", en: "Capture Guard Adjudicator" },
  shredplan:  { zh: "粉碎计划器", en: "Shred Planner" },
  vxcrypt2:   { zh: "加密容器引擎", en: "Encryption Container" },
  findrepl:   { zh: "查找替换引擎", en: "Find & Replace" },
  bannerpack: { zh: "横幅布局引擎", en: "Banner Layout" },
  hotkeymap:  { zh: "快捷键映射引擎", en: "Hotkey Mapping" },
  sysdiag:    { zh: "系统诊断引擎", en: "System Diagnostics" },
  explog:     { zh: "体验日志引擎", en: "Experience Log" },
  walkcheck:  { zh: "十二查对账引擎", en: "Twelve-Query Reconciler" },
  despaint:   { zh: "桌面实绘引擎", en: "Desktop Paint Engine" },
  expui:      { zh: "资源管理器装配引擎", en: "Explorer Assembly Engine" },
  lockmount:  { zh: "锁屏横幅挂接引擎", en: "Lock & Banner Mount Engine" },
  shellbar:   { zh: "shell 层装配引擎", en: "Shell Bar Engine" },
  dictwalk:   { zh: "交互词典走查引擎", en: "Dict Walk Engine" },
  kernelbridge: { zh: "内核域桥接引擎", en: "Kernel Bridge Engine" },
};

/** 面板区组标签。 */
export const U3_LAB_LABELS: Readonly<Record<string, U3LabelPair>> = {
  labTitle:    { zh: "U3-v4 引擎实验室", en: "U3-v4 Engine Lab" },
  labIntro:    { zh: "十三引擎实时运行——每条自检都是可执行判据，红=缺陷显性", en: "13 engines live — every self-check is an executable criterion; red = visible defect" },
  runAll:      { zh: "执行引擎群总自检", en: "Run all engine self-checks" },
  engineGroup: { zh: "引擎群自检", en: "Engine self-checks" },
  walkGroup:   { zh: "十二查对账", en: "Twelve-query reconciliation" },
  logGroup:    { zh: "体验日志实况", en: "Experience log live" },
  allGreen:    { zh: "全绿", en: "ALL GREEN" },
  hasRed:      { zh: "有红", en: "HAS RED" },
  pending:     { zh: "真机走查 pending", en: "Manual walk pending" },
  sampleLabel: { zh: "两行封顶样张", en: "Two-line wrap sample" },
  rageHint:    { zh: "点击下方按钮记录交互事件（狂点会被指纹器捕获）", en: "Click below to log events (rage clicks get fingerprinted)" },
  signals:     { zh: "挫败信号", en: "Frustration signals" },
  none:        { zh: "暂无", en: "None" },
  deskGroup:   { zh: "桌面实绘装配区", en: "Desktop paint assembly" },
  expGroup:    { zh: "资源管理器装配区", en: "Explorer assembly" },
  lockGroup:   { zh: "锁屏横幅挂接区", en: "Lock & banner mount" },
  shellTitle:  { zh: "shell 层活体", en: "Shell bar live" },
  dictGroup:   { zh: "交互词典走查区", en: "Interaction dictionary walk" },
  bridgeGroup: { zh: "内核域桥接对账区", en: "Kernel bridge reconciliation" },
};

/** 取词 selector（缺键显性回退键名——零静默漏翻）。 */
export function u3Label(key: string, lang: U3Lang, dict: Readonly<Record<string, U3LabelPair>>): string {
  const pair = dict[key];
  if (!pair) return key;
  return pair[lang];
}

/** 双语覆盖自检（B-1104 语言面：缺 zh 缺 en 都是缺陷）。 */
export function labelsSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  const allDicts = [U3_ENGINE_LABELS, U3_LAB_LABELS];
  checks.push({
    name: "labels 双语零缺键",
    pass: allDicts.every((d) => Object.values(d).every((p) => p.zh.length > 0 && p.en.length > 0)),
  });
  checks.push({
    name: "labels 十九引擎在册",
    pass: Object.keys(U3_ENGINE_LABELS).length === 19,
  });
  // 缺键显性回退：不存在的键返回键名（不静默给空串）
  checks.push({ name: "labels 缺键显性回退", pass: u3Label("no-such-key", "zh", U3_LAB_LABELS) === "no-such-key" });
  return checks;
}
