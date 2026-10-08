/**
 * NOVA-200（新星计划）· W-001…W-200 功能注册表（S0 地基 · AI-01 路代建，十六路共享）。
 *
 * 单一事实源：200 项功能的元数据（编号/领域/默认态/参数 schema/changelog 字段）与开关状态。
 * - 状态持久化：localStorage 键空间 `nova.registry.v1`（仅本机，零网络）；
 * - 订阅通知：`subscribeNova`（NovaRuntime / Hub / 各域模块共用）；
 * - React 绑定：`novaSnapshot`（useSyncExternalStore 快照）；
 * - changelog 字段：每项一行中文能力说明，供 W-008 品牌星座的可交互星点展示。
 *
 * 纪律（实施总步骤 §8）：与 singularity 平行不合并 —— 键空间 `nova.*`、
 * 类名 `nova-`、事件 `nova://` 三前缀独立；默认态克制（重动效默认关）；
 * reduce-motion / safeMode / static 降级由各域模块实现并在描述注明。
 */

export type NovaDomainId =
  | "boot"
  | "windows"
  | "desktop"
  | "dock"
  | "input"
  | "files"
  | "tools"
  | "hardware"
  | "compat"
  | "privacy"
  | "eco"
  | "vision"
  | "sound"
  | "a11y"
  | "design"
  | "quality";

export interface NovaDomainDef {
  id: NovaDomainId;
  index: number;
  zh: string;
  en: string;
}

export type NovaParamValue = number | boolean | string;

export interface NovaParamDef {
  key: string;
  /** labels.ts 内的词条键（novaP_<key> 或 novaO_<opt>）。 */
  labelKey: string;
  type: "toggle" | "slider" | "select";
  default: NovaParamValue;
  min?: number;
  max?: number;
  step?: number;
  options?: Array<{ value: string; labelKey: string }>;
}

export interface NovaFeatureDef {
  /** W-001 … W-200 */
  id: string;
  domain: NovaDomainId;
  /** 默认开关（克制原则：氛围重动效默认关、观测类默认开）。 */
  on: boolean;
  params?: NovaParamDef[];
  /** 该功能有独立 overlay 工具窗（ai04:open-feature 的 feature id，nova- 前缀）。 */
  overlay?: string;
  /** 一行中文能力说明（W-008 品牌星座星点展示；同步自全景图交付物）。 */
  changelog: string;
}

export interface NovaFeatureState {
  on: boolean;
  params: Record<string, NovaParamValue>;
}

/** 十六域定义（与分工图 §0 一致：8×12 + 8×13 = 200）。 */
export const NOVA_DOMAINS: NovaDomainDef[] = [
  { id: "boot", index: 1, zh: "启动与品牌剧场", en: "Boot & Brand Theater" },
  { id: "windows", index: 2, zh: "窗口与空间", en: "Windows & Space" },
  { id: "desktop", index: 3, zh: "桌面设计与图标", en: "Desktop & Icons" },
  { id: "dock", index: 4, zh: "任务栏与开始菜单", en: "Taskbar & Start" },
  { id: "input", index: 5, zh: "键盘与输入手感", en: "Keyboard & Input" },
  { id: "files", index: 6, zh: "文件与数据能力", en: "Files & Data" },
  { id: "tools", index: 7, zh: "效率与工具中枢", en: "Tools & Flow" },
  { id: "hardware", index: 8, zh: "系统集成与硬件", en: "System & Hardware" },
  { id: "compat", index: 9, zh: "兼容性防线", en: "Compat Defense" },
  { id: "privacy", index: 10, zh: "安全与隐私", en: "Security & Privacy" },
  { id: "eco", index: 11, zh: "开放生态", en: "Open Ecosystem" },
  { id: "vision", index: 12, zh: "视觉、个性化与氛围", en: "Visual & Ambience" },
  { id: "sound", index: 13, zh: "声音与通知", en: "Sound & Notify" },
  { id: "a11y", index: 14, zh: "无障碍与本地化", en: "A11y & Locale" },
  { id: "design", index: 15, zh: "UI 设计与优化", en: "UI Design & Measure" },
  { id: "quality", index: 16, zh: "工程质量与收官", en: "Quality & Finale" },
];

const S = (
  key: string,
  min: number,
  max: number,
  step: number,
  def: number,
): NovaParamDef => ({ key, labelKey: `novaP_${key}`, type: "slider", default: def, min, max, step });

const T = (key: string, def: boolean): NovaParamDef => ({ key, labelKey: `novaP_${key}`, type: "toggle", default: def });

const SEL = (key: string, def: string, options: Array<[string, string]>): NovaParamDef => ({
  key,
  labelKey: `novaP_${key}`,
  type: "select",
  default: def,
  options: options.map(([value, labelKey]) => ({ value, labelKey: `novaO_${labelKey}` })),
});

/**
 * 200 项功能定义。changelog 为 W-008 星座展示用的一行说明。
 * 默认态：重动效/手感类默认关（W-013/014/016/017/023/024/039/048…）；
 * 观测/面板类默认开；纯工具 overlay 不自动激活；opt-in 类（语音/指纹）默认关。
 */
export const NOVA_FEATURES: NovaFeatureDef[] = [
  // ---- 域1 启动与品牌剧场（W-001…W-012）----
  { id: "W-001", domain: "boot", on: true, changelog: "按电源状态区分点火制式：熔炉/余烬/无点火" },
  { id: "W-002", domain: "boot", on: true, params: [S("diskFactor", 0, 100, 5, 50)], changelog: "启动第 1 秒打出耗时预报并诚实回写误差" },
  { id: "W-003", domain: "boot", on: true, changelog: "跳映只压缩纯视觉段并自报 SKIPPED CINEMATICS ONLY" },
  { id: "W-004", domain: "boot", on: true, params: [T("nightSilent", true)], changelog: "启动音按音量/时段双因子自适应" },
  { id: "W-005", domain: "boot", on: true, params: [S("staggerMs", 0, 400, 50, 150)], changelog: "多屏按真实挂载完成 150ms 顺次点亮接力" },
  { id: "W-006", domain: "boot", on: true, overlay: "nova-footprints", changelog: "最近 20 次启动耗时的色温年轮足迹墙" },
  { id: "W-007", domain: "boot", on: true, changelog: "关机流中的下次启动预演卡（自启清单+预估）" },
  { id: "W-008", domain: "boot", on: true, params: [S("windowSec", 2, 12, 1, 4)], changelog: "品牌字标化作可交互星座，星点展开本版能力" },
  { id: "W-009", domain: "boot", on: true, changelog: "25/50/75/100% 四个真实里程碑的上行微音" },
  { id: "W-010", domain: "boot", on: true, params: [SEL("rite", "strings", [["strings", "strings"], ["pendulum", "pendulum"], ["silence", "silence"]])], changelog: "关机/重启的 1.2s 下行告别音（三种制式）" },
  { id: "W-011", domain: "boot", on: true, changelog: "按上次退出方式差异化启动情绪（安抚/检视版）" },
  { id: "W-012", domain: "boot", on: true, changelog: "桌面就绪后的 COLD/FAST/RESUME/RECOVER 源流戳" },
  // ---- 域2 窗口与空间（W-013…W-025）----
  { id: "W-013", domain: "windows", on: false, params: [S("maxMass", 0, 100, 10, 60)], changelog: "窗口面积映射惯性质量：大窗沉稳小窗轻快" },
  { id: "W-014", domain: "windows", on: true, params: [S("dampPx", 2, 12, 1, 6)], changelog: "吸附前 6px 阻尼磁场与吸附线呼吸确认" },
  { id: "W-015", domain: "windows", on: true, overlay: "nova-genealogy", changelog: "进程父子树的窗口族谱视图" },
  { id: "W-016", domain: "windows", on: false, params: [S("maxDepth", 1, 8, 1, 5)], changelog: "z 序映射阴影深度的空气透视层" },
  { id: "W-017", domain: "windows", on: false, changelog: "每虚拟桌面专属色相，切换瞬间辉光 600ms 锚点" },
  { id: "W-018", domain: "windows", on: true, changelog: "Alt+小键盘 1-9 九宫格几何吸附与二次恢复" },
  { id: "W-019", domain: "windows", on: true, changelog: "窗口宽高 8px 网格取整与安全区对齐" },
  { id: "W-020", domain: "windows", on: true, params: [S("idleSec", 5, 60, 5, 10)], changelog: "上焦点窗口的呼吸回声召回微光" },
  { id: "W-021", domain: "windows", on: true, changelog: "按宽高比推荐镶嵌布局的马赛克顾问" },
  { id: "W-022", domain: "windows", on: true, changelog: "每窗口 3 仓位几何历史与菜单回滚" },
  { id: "W-023", domain: "windows", on: false, changelog: "拖入顶部轨道区变 72px 贴轨浮窗" },
  { id: "W-024", domain: "windows", on: true, changelog: "Ctrl+双击折叠全部非焦点窗为右缘竖带" },
  { id: "W-025", domain: "windows", on: true, changelog: "窗口浮动文字标签，Alt+Tab 同步显示" },
  // ---- 域3 桌面设计与图标（W-026…W-038）----
  { id: "W-026", domain: "desktop", on: false, changelog: "沿 6 条星座曲线排布图标并连星图细线" },
  { id: "W-027", domain: "desktop", on: true, changelog: "图标存在感随使用频率生长与半休眠" },
  { id: "W-028", domain: "desktop", on: true, changelog: "图标接触阴影随四季调色温（南半球反转）" },
  { id: "W-029", domain: "desktop", on: true, overlay: "nova-leaderboard", changelog: "本月图标点击 Top10 领奖台与沉睡王" },
  { id: "W-030", domain: "desktop", on: true, params: [S("idleMin", 5, 30, 5, 10)], changelog: "10 分钟无输入桌面进入柔化午憩态" },
  { id: "W-031", domain: "desktop", on: true, changelog: "7/30 天图标老化梯度与退休提案卡" },
  { id: "W-032", domain: "desktop", on: true, changelog: "图标基线 2px 微光地平线构图层" },
  { id: "W-033", domain: "desktop", on: true, overlay: "nova-photoalbum", changelog: "布局视觉快照相册与差异对比还原" },
  { id: "W-034", domain: "desktop", on: false, changelog: "晚间图标层 2px 光雪缓落绕流" },
  { id: "W-035", domain: "desktop", on: true, changelog: "图标点击声按 X 坐标声像定位" },
  { id: "W-036", domain: "desktop", on: true, changelog: "框选按 G 临时结组，开合解散会话级" },
  { id: "W-037", domain: "desktop", on: true, changelog: "拖拽按 R 显示最近邻间距标尺与 SNAP" },
  { id: "W-038", domain: "desktop", on: true, changelog: "图标首落点记忆与一键回原点" },
  // ---- 域4 任务栏与开始菜单（W-039…W-050）----
  { id: "W-039", domain: "dock", on: false, params: [S("driftPx", 1, 8, 1, 4)], changelog: "任务栏图标向指针 4px 群体磁漂" },
  { id: "W-040", domain: "dock", on: true, changelog: "开始菜单开发者/创作者/学生三角色预设" },
  { id: "W-041", domain: "dock", on: true, changelog: "图标占用率驱动的任务栏潮汐密度" },
  { id: "W-042", domain: "dock", on: true, changelog: "搜索首项零点击详情卡，信息先于点击" },
  { id: "W-043", domain: "dock", on: true, changelog: "悬停栏空白 800ms 浮出今日活动微史" },
  { id: "W-044", domain: "dock", on: true, params: [S("peekMs", 200, 1200, 100, 500)], changelog: "悬停运行图标弹全窗口实时微缩景" },
  { id: "W-045", domain: "dock", on: true, changelog: "任务栏两端 2+2 可拔插模块槽架构" },
  { id: "W-046", domain: "dock", on: true, changelog: "开始菜单昨日-今日差集延续分区" },
  { id: "W-047", domain: "dock", on: false, changelog: "32px 图标极限密度的任务栏锋锐制式" },
  { id: "W-048", domain: "dock", on: true, params: [S("arcMs", 120, 480, 60, 240)], changelog: "启动应用的图标→屏心抛物线弹道" },
  { id: "W-049", domain: "dock", on: true, changelog: "蓝牙手柄接入自动切手柄友好菜单布局" },
  { id: "W-050", domain: "dock", on: false, changelog: "多屏镜像任务栏选项，各屏同构同步" },
  // ---- 域5 键盘与输入手感（W-051…W-063）----
  { id: "W-051", domain: "input", on: true, changelog: "分应用输入节奏记忆，互不污染" },
  { id: "W-052", domain: "input", on: false, changelog: "按住 Shift 临时反转 Caps 的物理修正键" },
  { id: "W-053", domain: "input", on: true, overlay: "nova-rhythm", changelog: "30s 击键节奏可视化训练，训练即焚" },
  { id: "W-054", domain: "input", on: true, changelog: "长按 ./、/; 弹出标点池径向环" },
  { id: "W-055", domain: "input", on: true, params: [S("ghostN", 1, 5, 1, 3)], changelog: "输入框获焦浮出本地高频短语幽灵建议" },
  { id: "W-056", domain: "input", on: true, overlay: "nova-sandbox", changelog: "F1 长按反查任意组合的真实键位绑定" },
  { id: "W-057", domain: "input", on: true, changelog: "打字速度映射输入框外圈光环" },
  { id: "W-058", domain: "input", on: true, changelog: "80/100/120 WPM 三门槛的高手时刻庆典" },
  { id: "W-059", domain: "input", on: true, changelog: "输入流中断 3s 的走神召回光点" },
  { id: "W-060", domain: "input", on: true, changelog: "Ctrl+Shift+1/2/3 显式热剪贴板环槽" },
  { id: "W-061", domain: "input", on: true, params: [S("chars", 100, 900, 100, 300)], changelog: "长句连续输入的呼吸气泡健康提示" },
  { id: "W-062", domain: "input", on: true, overlay: "nova-zones", changelog: "手部 10 分区击键负载热区图" },
  { id: "W-063", domain: "input", on: true, changelog: "中英标点上下文智断与豁免名单" },
  // ---- 域6 文件与数据能力（W-064…W-076）----
  { id: "W-064", domain: "files", on: true, changelog: "写入中文件的 2s 脉搏微光活性可视化" },
  { id: "W-065", domain: "files", on: true, overlay: "nova-census", changelog: "目录类型构成/年龄/极值的人口普查报告" },
  { id: "W-066", domain: "files", on: true, changelog: "借出-归还闭环的文件借阅书架" },
  { id: "W-067", domain: "files", on: true, changelog: "文件体量映射名字字重的墨阶三档" },
  { id: "W-068", domain: "files", on: true, changelog: "目录 30 天增长外推与红色水位日期" },
  { id: "W-069", domain: "files", on: true, overlay: "nova-journal", changelog: "环境内移动/重命名的迁途轨迹链" },
  { id: "W-070", domain: "files", on: true, changelog: "标签交并差代数与代数文件夹" },
  { id: "W-071", domain: "files", on: true, changelog: "长文档预览末尾的幽灵页轮廓" },
  { id: "W-072", domain: "files", on: true, changelog: "传输速率联动的律动声与终止和弦" },
  { id: "W-073", domain: "files", on: true, changelog: "每日 00:00 数据增量 3s 子夜更钟卡" },
  { id: "W-074", domain: "files", on: true, overlay: "nova-seasonring", changelog: "目录全年创建月分布的 12 扇区季节环" },
  { id: "W-075", domain: "files", on: true, changelog: "回收站删除方与原路径的出身簿" },
  { id: "W-076", domain: "files", on: true, changelog: "最近 5 个真实浏览目录的动态浮列" },
  // ---- 域7 效率与工具中枢（W-077…W-089）----
  { id: "W-077", domain: "tools", on: true, overlay: "nova-timebox", changelog: "15min 网格拖刻时间块并联动番茄氛围" },
  { id: "W-078", domain: "tools", on: true, changelog: "任务栏 3 条微字要务带与完成划线" },
  { id: "W-079", domain: "tools", on: true, params: [S("everyMin", 20, 90, 5, 45)], changelog: "45min 眨眼提醒与 20-20-20 时辰簿盖章" },
  { id: "W-080", domain: "tools", on: true, overlay: "nova-table", changelog: "计算器/换算/便签/取色 2×2 多工具横桌" },
  { id: "W-081", domain: "tools", on: true, params: [S("hour", 19, 23, 1, 21), S("minute", 0, 59, 15, 30)], changelog: "21:30 今日收官复盘卡与明日提名" },
  { id: "W-082", domain: "tools", on: true, overlay: "nova-habit", changelog: "近 7 天鼠键配比环与可解释建议" },
  { id: "W-083", domain: "tools", on: true, changelog: "文件拖到工具图标按类型语义调用" },
  { id: "W-084", domain: "tools", on: true, changelog: "会议临前四项状态一屏会议快车道" },
  { id: "W-085", domain: "tools", on: true, overlay: "nova-shuttle", changelog: "昨日同一时刻的 72h 缓存对照回看" },
  { id: "W-086", domain: "tools", on: false, changelog: "F8 长按 10s 本地语音转文字钉（opt-in）" },
  { id: "W-087", domain: "tools", on: true, changelog: "选区数字就地运算与 5 步撤销" },
  { id: "W-088", domain: "tools", on: true, changelog: "双栏路径与视图状态一键互换座" },
  { id: "W-089", domain: "tools", on: false, overlay: "nova-scratch", changelog: "F9 呼出 15 分钟自焚的内存态备忘板" },
  // ---- 域8 系统集成与硬件（W-090…W-101）----
  { id: "W-090", domain: "hardware", on: false, params: [S("intensity", 0, 100, 10, 60)], changelog: "CPU 负载映射壁纸粒子潮汐密度" },
  { id: "W-091", domain: "hardware", on: true, overlay: "nova-battery", changelog: "电池月度故事卡与人话健康结论" },
  { id: "W-092", domain: "hardware", on: true, changelog: "麦克风启用瞬间的 120ms 木质风铃" },
  { id: "W-093", domain: "hardware", on: true, changelog: "任务栏顶端 1px 内存川流光带" },
  { id: "W-094", domain: "hardware", on: true, changelog: "分核 4px 波形柱的核心合唱谱" },
  { id: "W-095", domain: "hardware", on: true, changelog: "每物理盘读写活动的巡逻微闪灯" },
  { id: "W-096", domain: "hardware", on: true, params: [SEL("bolt", "thunder", [["thunder", "thunder"], ["waterfall", "waterfall"], ["silence", "silence"]])], changelog: "电源插拔瞬间的落雷/细流仪式微光" },
  { id: "W-097", domain: "hardware", on: true, overlay: "nova-panelhours", changelog: "每屏点亮工时簿与 OLED 烧屏提醒" },
  { id: "W-098", domain: "hardware", on: false, changelog: "风扇高速时提示音 +6dB 穿透补偿" },
  { id: "W-099", domain: "hardware", on: true, changelog: "USB 卡片追加协商/实际电流读数" },
  { id: "W-100", domain: "hardware", on: true, changelog: "硬件装机纪念日的蛋糕微标与健康语" },
  { id: "W-101", domain: "hardware", on: false, changelog: "环境分贝秒级伴飞的自适应音量" },
  // ---- 域9 兼容性防线（W-102…W-113）----
  { id: "W-102", domain: "compat", on: true, overlay: "nova-watch", changelog: "新应用 48h 观察簿与三级体检小结" },
  { id: "W-103", domain: "compat", on: true, changelog: "启动前导入表快扫的 DLL 缺失预言卡" },
  { id: "W-104", domain: "compat", on: true, changelog: "全屏卡死的三档安全逃生门序列" },
  { id: "W-105", domain: "compat", on: true, changelog: "组段吞字检测与白名单内组段接力" },
  { id: "W-106", domain: "compat", on: true, changelog: "缺失字体的 metric 相似替身演出" },
  { id: "W-107", domain: "compat", on: true, params: [S("freezeMs", 100, 600, 100, 300)], changelog: "DPI 变更瞬间冻结截图 300ms 防腐揭幕" },
  { id: "W-108", domain: "compat", on: true, overlay: "nova-hospital", changelog: "遗留应用三查三治的诊疗流程" },
  { id: "W-109", domain: "compat", on: true, changelog: "反作弊驱动加载时的全面礼让协议" },
  { id: "W-110", domain: "compat", on: true, overlay: "nova-forensics", changelog: "蓝屏 minidump 本地检尸与中立结论" },
  { id: "W-111", domain: "compat", on: true, changelog: "色彩模式切换 2s 插值转影零白闪" },
  { id: "W-112", domain: "compat", on: true, changelog: "同名双实例 GPU 争抢的仲裁建议卡" },
  { id: "W-113", domain: "compat", on: true, changelog: "系统大版本更新前的设置护城河备份" },
  // ---- 域10 安全与隐私（W-114…W-126）----
  { id: "W-114", domain: "privacy", on: true, overlay: "nova-checkup", changelog: "季度隐私三轴体检报告与整改入口" },
  { id: "W-115", domain: "privacy", on: true, changelog: "密码框三轨力度合奏与具体指导" },
  { id: "W-116", domain: "privacy", on: true, changelog: "摄像头双击戴上眼罩与已蒙眼徽标" },
  { id: "W-117", domain: "privacy", on: true, changelog: "敏感文件被访问当下的 2s 实时气泡" },
  { id: "W-118", domain: "privacy", on: true, changelog: "显式标记的阅后即焚剪贴板" },
  { id: "W-119", domain: "privacy", on: true, overlay: "nova-pulsewall", changelog: "后台应用三电平活动的心跳墙" },
  { id: "W-120", domain: "privacy", on: true, changelog: "插件信任分的透明衰减曲线" },
  { id: "W-121", domain: "privacy", on: false, changelog: "沙盒化隐私泄露剧场排演（教育向）" },
  { id: "W-122", domain: "privacy", on: true, overlay: "nova-dns", changelog: "DNS 请求白话账本与可疑红标" },
  { id: "W-123", domain: "privacy", on: false, changelog: "单向哈希使用指纹黑匣（opt-in 迁移比对）" },
  { id: "W-124", domain: "privacy", on: true, changelog: "安全事件的金边红字置顶通行权" },
  { id: "W-125", domain: "privacy", on: true, changelog: "文件 ACL 继承链的权限族谱视图" },
  { id: "W-126", domain: "privacy", on: true, changelog: "销毁动作 1.2s 蓄力焚毁仪式" },
  // ---- 域11 开放生态（W-127…W-138）----
  { id: "W-127", domain: "eco", on: true, overlay: "nova-genes", changelog: "插件能力基因逐项开关矩阵" },
  { id: "W-128", domain: "eco", on: true, overlay: "nova-radar", changelog: "生态健康六轴雷达与月度对比" },
  { id: "W-129", domain: "eco", on: true, changelog: "离线起草知情投递的社区声邮筒" },
  { id: "W-130", domain: "eco", on: true, changelog: "插件依赖冲突树与三种解法预估" },
  { id: "W-131", domain: "eco", on: true, overlay: "nova-lighthouse", changelog: "已装插件四维实测本地灯塔跑分" },
  { id: "W-132", domain: "eco", on: false, changelog: "配置打包二维码序列的零网络驿传" },
  { id: "W-133", domain: "eco", on: true, changelog: "API 网关限流配额的车厢载客可视化" },
  { id: "W-134", domain: "eco", on: true, changelog: "插件权限声明的白话翻译与风险色标" },
  { id: "W-135", domain: "eco", on: true, changelog: "主题 token 逐器官勾选移植预览" },
  { id: "W-136", domain: "eco", on: true, changelog: "官方策展与社区热度的市场双货架" },
  { id: "W-137", domain: "eco", on: true, changelog: "沙盒试用行为的逐动作剧本回放" },
  { id: "W-138", domain: "eco", on: true, changelog: "卸载插件的服务叙事讣告页" },
  // ---- 域12 视觉、个性化与氛围（W-139…W-151）----
  { id: "W-139", domain: "vision", on: false, changelog: "极光色相随周序 15° 巡回的桌面极光历" },
  { id: "W-140", domain: "vision", on: true, changelog: "壁纸钟爱/休眠/荣休三态生态位" },
  { id: "W-141", domain: "vision", on: false, params: [S("flock", 0, 200, 20, 120)], changelog: "壁纸粒子自主游动避让的水族馆模式" },
  { id: "W-142", domain: "vision", on: true, overlay: "nova-gallery", changelog: "图标 64px 大卡展览墙的美术馆鉴赏视图" },
  { id: "W-143", domain: "vision", on: true, changelog: "材质纤维/磨砂感的季节纹理插值" },
  { id: "W-144", domain: "vision", on: false, changelog: "光标 24px 静态伴飞柔光晕（深色定位性）" },
  { id: "W-145", domain: "vision", on: true, changelog: "壁纸↔声纹成套配对推荐与试听" },
  { id: "W-146", domain: "vision", on: true, changelog: "主题嗅觉隐喻的香水文学卡" },
  { id: "W-147", domain: "vision", on: true, changelog: "活跃动效层数计量与一键安静档" },
  { id: "W-148", domain: "vision", on: false, changelog: "720p 降采样最近邻放大的像素画制式" },
  { id: "W-149", domain: "vision", on: false, params: [SEL("frame", "soft", [["thin", "thin"], ["gallery", "gallery"], ["soft", "soft"], ["film", "film"], ["deckle", "deckle"], ["none", "none"]])], changelog: "屏幕四缘六制式装饰暗角画框" },
  { id: "W-150", domain: "vision", on: true, overlay: "nova-exposure", changelog: "每小时主色采样的 8760 帧长曝光色带" },
  { id: "W-151", domain: "vision", on: false, changelog: "真实月相映射环境月光层的月光舞台" },
  // ---- 域13 声音与通知（W-152…W-163）----
  { id: "W-152", domain: "sound", on: true, overlay: "nova-museum", changelog: "3 分钟系统音导览与就地改音的声纹馆" },
  { id: "W-153", domain: "sound", on: true, overlay: "nova-heat", changelog: "7×24 通知密度时间热图" },
  { id: "W-154", domain: "sound", on: false, changelog: "首解锁 90s 本地 TTS 晨间电台（opt-in）" },
  { id: "W-155", domain: "sound", on: true, changelog: "双耳平衡校准与 ±3dB 温和补偿" },
  { id: "W-156", domain: "sound", on: true, params: [S("gain", 0, 40, 5, 15)], changelog: "勿扰期间通知转一声轻雨滴" },
  { id: "W-157", domain: "sound", on: true, changelog: "全局音量晨昏三段自动曲线" },
  { id: "W-158", domain: "sound", on: true, changelog: "工作会话跨整点的 200ms 软钟声" },
  { id: "W-159", domain: "sound", on: true, changelog: "勿扰期通知的栏体微颤触觉回声" },
  { id: "W-160", domain: "sound", on: true, overlay: "nova-piano", changelog: "12 键系统音试奏的声纹钢琴" },
  { id: "W-161", domain: "sound", on: true, changelog: "最近 3 次系统声微史与一键重播" },
  { id: "W-162", domain: "sound", on: true, changelog: "多通知逐条聚光上演的队列剧场" },
  { id: "W-163", domain: "sound", on: true, changelog: "并发声频段冲突的 60ms 和声错峰" },
  // ---- 域14 无障碍与本地化（W-164…W-175）----
  { id: "W-164", domain: "a11y", on: false, changelog: "20 条封闭语法离线声令官（opt-in）" },
  { id: "W-165", domain: "a11y", on: false, params: [S("zoom", 110, 200, 10, 150)], changelog: "150% 布局级重排放大的一键大屋" },
  { id: "W-166", domain: "a11y", on: false, changelog: "色觉自测后的主题源配方改写" },
  { id: "W-167", domain: "a11y", on: true, changelog: "系统声三通道转译的听障听诊器" },
  { id: "W-168", domain: "a11y", on: false, changelog: "1.3× 大卡双确认的长者模式套装" },
  { id: "W-169", domain: "a11y", on: true, changelog: "中英 50+ 规则的温柔语法墙建议" },
  { id: "W-170", domain: "a11y", on: false, changelog: "金额/日期/电话三制式 TTS 报数" },
  { id: "W-171", domain: "a11y", on: true, changelog: "键位方案中心对称的左右手镜像生成" },
  { id: "W-172", domain: "a11y", on: true, changelog: "界面字体栈逐级体检与修复建议" },
  { id: "W-173", domain: "a11y", on: true, changelog: "Ctrl+Alt+D 就地白话术语卡 300 条" },
  { id: "W-174", domain: "a11y", on: false, changelog: "界面中文拼音上标与悬停朗读" },
  { id: "W-175", domain: "a11y", on: false, changelog: "ARIA 附 Unicode 盲文徽章双通道" },
  // ---- 域15 UI 设计与优化（W-176…W-187）----
  { id: "W-176", domain: "design", on: false, overlay: "nova-philab", changelog: "黄金分割/三分线/根矩形美学参考网格" },
  { id: "W-177", domain: "design", on: false, overlay: "nova-symphony", changelog: "间距节奏五线谱与跑调标记" },
  { id: "W-178", domain: "design", on: true, changelog: "动效时长/曲线/位移三元体检成绩单" },
  { id: "W-179", domain: "design", on: false, changelog: "三代 Windows 风格即时换装考古馆" },
  { id: "W-180", domain: "design", on: false, overlay: "nova-xray", changelog: "实现网格列/槽/边距的 X 光透视" },
  { id: "W-181", domain: "design", on: true, overlay: "nova-turntable", changelog: "字重/字距/行高三旋钮排印唱机" },
  { id: "W-182", domain: "design", on: true, changelog: "图标前景配重与重心偏移天平体检" },
  { id: "W-183", domain: "design", on: true, changelog: "光标五场景形影一致性巡检" },
  { id: "W-184", domain: "design", on: true, changelog: "界面点击密度匿名热图（30 天滚动）" },
  { id: "W-185", domain: "design", on: true, changelog: "无人注视循环动画的保释降频复庭" },
  { id: "W-186", domain: "design", on: true, changelog: "页面信息密度三维湿度计打分" },
  { id: "W-187", domain: "design", on: true, changelog: "WCAG AA 全文对比度红框哨兵巡检" },
  // ---- 域16 工程质量与收官（W-188…W-200）----
  { id: "W-188", domain: "quality", on: true, changelog: "空载/启动/动效三轴 A–G 能效标签" },
  { id: "W-189", domain: "quality", on: true, changelog: "空闲 30min 工作集换出的睡眠孵化器" },
  { id: "W-190", domain: "quality", on: false, changelog: "确定性崩溃逐帧单步重放器（dev）" },
  { id: "W-191", domain: "quality", on: true, changelog: "GPU 帧耗时墨沉积诊断砚" },
  { id: "W-192", domain: "quality", on: false, changelog: "分身 A/B 同负载的军棋推演战报" },
  { id: "W-193", domain: "quality", on: true, changelog: "跨版本空载内存最低水位地平线" },
  { id: "W-194", domain: "quality", on: true, changelog: "同源码双构建哈希树可复现指纹" },
  { id: "W-195", domain: "quality", on: false, changelog: "零设置零历史首跑视角白纸沙盒" },
  { id: "W-196", domain: "quality", on: true, changelog: "晴/多云/雨/风暴的性能气象播报" },
  { id: "W-197", domain: "quality", on: true, overlay: "nova-zodiac", changelog: "12 指标映射星座亮暗的工程星象盘" },
  { id: "W-198", domain: "quality", on: true, changelog: "发版里程碑墙与全屏礼炮仪式" },
  { id: "W-199", domain: "quality", on: true, changelog: "全指标历史最佳的不朽档案馆" },
  { id: "W-200", domain: "quality", on: true, overlay: "nova-battlepass", changelog: "测试用例里程碑徽章战绩册" },
];

// ---------------------------------------------------------------------------
// 状态存储（订阅 + localStorage 持久化；键空间 nova.registry 独立于 singularity）
// ---------------------------------------------------------------------------

const STORAGE_KEY = "nova.registry.v1";

type Listener = () => void;
const listeners = new Set<Listener>();

function defaultState(): Record<string, NovaFeatureState> {
  const st: Record<string, NovaFeatureState> = {};
  for (const f of NOVA_FEATURES) {
    const params: Record<string, NovaParamValue> = {};
    for (const p of f.params ?? []) params[p.key] = p.default;
    st[f.id] = { on: f.on, params };
  }
  return st;
}

function coerce(raw: unknown): Record<string, NovaFeatureState> | null {
  if (typeof raw !== "object" || raw === null) return null;
  const out: Record<string, NovaFeatureState> = {};
  const known = new Map(NOVA_FEATURES.map((f) => [f.id, f]));
  for (const [id, val] of Object.entries(raw as Record<string, unknown>)) {
    const def = known.get(id);
    if (!def || typeof val !== "object" || val === null) continue;
    const v = val as { on?: unknown; params?: unknown };
    const params: Record<string, NovaParamValue> = {};
    for (const p of def.params ?? []) {
      const pv = (v.params as Record<string, unknown> | undefined)?.[p.key];
      if (typeof pv === typeof p.default) params[p.key] = pv as NovaParamValue;
      else params[p.key] = p.default;
    }
    out[id] = { on: v.on === true, params };
  }
  // 补齐缺失项（升级容错）
  for (const f of NOVA_FEATURES) if (!out[f.id]) out[f.id] = defaultState()[f.id]!;
  return out;
}

let state: Record<string, NovaFeatureState> = defaultState();

function persist(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    /* 无 localStorage（测试环境）如实跳过 */
  }
}

function load(): void {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const parsed = coerce(JSON.parse(raw));
    if (parsed) state = parsed;
  } catch {
    /* 损坏数据：回默认（诚实降级，不抛错） */
  }
}

if (typeof window !== "undefined") load();

function emit(): void {
  for (const fn of listeners) fn();
}

/** 订阅状态变化（返回反订阅）。 */
export function subscribeNova(fn: Listener): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function getNovaState(): Record<string, NovaFeatureState> {
  return state;
}

export function setNovaOn(id: string, on: boolean): void {
  if (!state[id] || state[id].on === on) return;
  state = { ...state, [id]: { ...state[id], on } };
  persist();
  emit();
}

export function setNovaParam(id: string, key: string, value: NovaParamValue): void {
  const cur = state[id];
  if (!cur || cur.params[key] === value) return;
  state = { ...state, [id]: { ...cur, params: { ...cur.params, [key]: value } } };
  persist();
  emit();
}

export function resetNovaDomain(domain: NovaDomainId): void {
  const next = { ...state };
  for (const f of NOVA_FEATURES) {
    if (f.domain !== domain) continue;
    next[f.id] = defaultState()[f.id]!;
  }
  state = next;
  persist();
  emit();
}

export function resetNovaAll(): void {
  state = defaultState();
  persist();
  emit();
}

// ---------------------------------------------------------------------------
// 只读便捷读取
// ---------------------------------------------------------------------------

/** 功能是否启用（未知 id 一律 false，安全默认）。 */
export function novaOn(id: string): boolean {
  return state[id]?.on === true;
}

/** 读取参数（带默认回退；类型不匹配回退默认值）。 */
export function novaParam(id: string, key: string): NovaParamValue {
  const def = NOVA_FEATURES.find((f) => f.id === id);
  const p = def?.params?.find((x) => x.key === key);
  const v = state[id]?.params[key];
  if (v === undefined) return p?.default ?? false;
  return v;
}

export function novaNum(id: string, key: string): number {
  const v = novaParam(id, key);
  return typeof v === "number" ? v : 0;
}

export function novaStr(id: string, key: string): string {
  const v = novaParam(id, key);
  return typeof v === "string" ? v : "";
}

export function novaBool(id: string, key: string): boolean {
  const v = novaParam(id, key);
  return v === true;
}

/** 某域是否有任一功能启用（模块整体挂载判据）。 */
export function novaDomainActive(domain: NovaDomainId): boolean {
  return NOVA_FEATURES.some((f) => f.domain === domain && state[f.id]?.on);
}

export function featuresOfNovaDomain(domain: NovaDomainId): NovaFeatureDef[] {
  return NOVA_FEATURES.filter((f) => f.domain === domain);
}

/**
 * 全局运动降级（各域模块共读；与全局 reduce-motion 令牌同源语义）。
 * App 以 String(bool) 写入 data-reduce-motion，另接受 safeMode / static。
 */
export function novaMotionOK(): boolean {
  if (typeof document === "undefined") return false;
  const ds = document.documentElement.dataset;
  return ds.reduceMotion !== "true" && ds.safeMode !== "true" && ds.staticMode !== "true";
}

/** 统计（hub 首页卡片用）。 */
export function novaStats(): { total: number; on: number } {
  let on = 0;
  for (const f of NOVA_FEATURES) if (state[f.id]?.on) on++;
  return { total: NOVA_FEATURES.length, on };
}

/** React 绑定（useSyncExternalStore 快照）。 */
let cachedSnapshot: Record<string, NovaFeatureState> | null = null;
export function novaSnapshot(): Record<string, NovaFeatureState> {
  if (cachedSnapshot !== state) cachedSnapshot = state;
  return cachedSnapshot;
}
