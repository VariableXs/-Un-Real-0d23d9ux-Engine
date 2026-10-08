// ⚠️ 自动生成（tools/gen-completion.cjs）—— 请勿手工编辑；
// 更新 docs/五路进度总览.md 后运行 `node tools/gen-completion.cjs` 重新生成。
// 数据口径：分工图全量任务 × 进度总览已交付 ID 集（未出现在进度文档 = 如实灰色）。
export interface CompletionTask {
  id: string;
  name: string;
  domain: string;
  group: string;
  stage: string;
  delivered: boolean;
}

export interface CompletionGroup {
  group: string;
  total: number;
  done: number;
}

export const COMPLETION_GENERATED_AT = "2026-09-08";
export const COMPLETION_TOTAL = 355;
export const COMPLETION_DONE = 213;
export const COMPLETION_GROUPS: CompletionGroup[] = [
  {
    "group": "AI-01",
    "total": 16,
    "done": 0
  },
  {
    "group": "AI-02",
    "total": 17,
    "done": 0
  },
  {
    "group": "AI-03",
    "total": 16,
    "done": 0
  },
  {
    "group": "AI-04",
    "total": 22,
    "done": 10
  },
  {
    "group": "AI-05",
    "total": 17,
    "done": 0
  },
  {
    "group": "AI-06",
    "total": 12,
    "done": 0
  },
  {
    "group": "AI-07",
    "total": 16,
    "done": 16
  },
  {
    "group": "AI-08",
    "total": 11,
    "done": 11
  },
  {
    "group": "AI-09",
    "total": 16,
    "done": 0
  },
  {
    "group": "AI-10",
    "total": 24,
    "done": 24
  },
  {
    "group": "AI-11",
    "total": 23,
    "done": 23
  },
  {
    "group": "AI-12",
    "total": 16,
    "done": 16
  },
  {
    "group": "AI-13",
    "total": 24,
    "done": 24
  },
  {
    "group": "AI-14",
    "total": 16,
    "done": 16
  },
  {
    "group": "AI-15",
    "total": 19,
    "done": 19
  },
  {
    "group": "AI-16",
    "total": 16,
    "done": 10
  },
  {
    "group": "AI-17",
    "total": 24,
    "done": 12
  },
  {
    "group": "AI-18",
    "total": 24,
    "done": 24
  },
  {
    "group": "AI-19",
    "total": 8,
    "done": 8
  },
  {
    "group": "AI-20",
    "total": 18,
    "done": 0
  }
];

export const COMPLETION_TASKS: CompletionTask[] = [
  {
    "id": "Z-01",
    "name": "官方图标网格对齐工程（Icon Grid Fidelity）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-02",
    "name": "Fluent 控件规格复刻（Control Metrics）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-03",
    "name": "Segoe UI Variable 字体链（Typography Chain）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-04",
    "name": "2K/4K 清晰度管线（Hi-DPI Clarity Pipeline）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-05",
    "name": "焦点环与选择态规范（Focus & Selection States）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-06",
    "name": "系统光标接管与对齐（Cursor Alignment）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-07",
    "name": "文案规范对齐（UI Copy Style Guide）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-08",
    "name": "全局键位注册表与冲突仲裁（Keymap Registry & Arbitration）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-09",
    "name": "系统组合键让位协议（System Combo Yield Protocol）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-10",
    "name": "三级作用域分层（Scope Layering）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-11",
    "name": "全键盘导航网格（Full Keyboard Grid）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-12",
    "name": "上下文键位速查浮层（Contextual Keymap Overlay）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-13",
    "name": "命令提示条（Command Hint Bar）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-14",
    "name": "键位方案管理（Keymap Profiles）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-15",
    "name": "应用适配等级库（App Compat Matrix）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-16",
    "name": "遗留协议兼容 Shim（Legacy Protocol Shim）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-17",
    "name": "IME 深度兼容（IME Compatibility）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-18",
    "name": "全屏与独占模式协议（Fullscreen & Exclusive Protocol）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-19",
    "name": "多屏混合 DPI 兼容（Mixed-DPI Handling）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-20",
    "name": "远程与虚拟宿主模式（Remote & VM Host Profile）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-21",
    "name": "慢速设备模式（Slow Device Mode）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-22",
    "name": "时钟中心（Clock Hub）",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-23",
    "name": "天气信息卡（Weather Card）",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-24",
    "name": "字符与 Emoji 面板（Chars & Emoji Panel）",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-25",
    "name": "放大镜与取色器（Magnifier & Color Picker）",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-26",
    "name": "换算中心（Converter Hub）",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-27",
    "name": "系统信息面板（System Info Panel）",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-28",
    "name": "运行对话框（Run Dialog）",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-29",
    "name": "预览格式扩展（Preview Formats +）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-30",
    "name": "打开方式管理面板（Open-With Manager）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-31",
    "name": "位置侧栏与快速跳转（Places Sidebar）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-32",
    "name": "批量重命名工具（Batch Rename）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-33",
    "name": "重复文件报告器（Duplicate Reporter）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-34",
    "name": "空间分析器（Space Analyzer）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-35",
    "name": "发送到菜单（SendTo Extensions）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-36",
    "name": "窗口不透明度与置顶微控（Opacity & Topmost Fine Control）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-37",
    "name": "窗口几何记忆（Geometry Memory）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-38",
    "name": "滚轮窗口行为（Wheel Behaviors）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-39",
    "name": "标题栏自定义（Titlebar Options）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-40",
    "name": "窗口布局快照（Layout Snapshots）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-41",
    "name": "鼠标手势最小集（Minimal Gestures）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-42",
    "name": "虚拟桌面切换增强（Desktop Switcher +）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-43",
    "name": "逐应用音量记忆（Per-App Volume Memory）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-44",
    "name": "勿扰日程（DND Schedule）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-45",
    "name": "系统声音方案（Sound Schemes）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-46",
    "name": "音频设备快切（Audio Device Quick Switch）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-47",
    "name": "通知存档与搜索（Notification Archive）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-48",
    "name": "麦克风使用指示（Mic Usage Indicator）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-49",
    "name": "提醒中心（Reminder Hub）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-50",
    "name": "主题令牌开放规范（Theme Token Spec）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-51",
    "name": "用户数据开放导出（User Data Export）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-52",
    "name": "本地事件流接口（Local Event Stream）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-53",
    "name": "本地只读状态 API（Read-only Status API）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-54",
    "name": "布局与配置分享格式（Config Share Format）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-55",
    "name": "开放数据连接器（Data Connectors）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-56",
    "name": "扩展兼容性承诺与版本矩阵（Ecosystem Compatibility Matrix）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-57",
    "name": "安全模式启动（Safe Boot Mode）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-58",
    "name": "UI 线程健康面板（UI Thread Health Panel）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-59",
    "name": "空闲渲染冻结（Idle Render Freeze）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-60",
    "name": "内存压力自适应（Memory Pressure Adaptation）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-61",
    "name": "增量更新通道（Update Channels & Delta）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-62",
    "name": "本地统计面板（Local Usage Stats）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-63",
    "name": "回归基线扩展（Regression Baseline +）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-64",
    "name": "CJK 排版精修（CJK Typography）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-65",
    "name": "拖动清晰度策略（Drag Clarity）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-66",
    "name": "图标加载零闪烁（Icon Zero-Flicker）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-67",
    "name": "滚动与动效一致性（Scroll & Motion Consistency）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-68",
    "name": "主题切换零闪白（Theme Cross-Fade）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": false
  },
  {
    "id": "Z-69",
    "name": "边缘热区自定义（Edge Hotspots）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "Z-70",
    "name": "帮助中心（Help Center）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "APEX-70",
    "delivered": true
  },
  {
    "id": "U-01",
    "name": "启动剧场（Boot Cinema）",
    "domain": "启动与品牌剧场",
    "group": "AI-16",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-02",
    "name": "大气字标与字形资产（Wordmark & Glyph Assets）",
    "domain": "启动与品牌剧场",
    "group": "AI-16",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-03",
    "name": "胶囊进度条（Capsule Progress）",
    "domain": "启动与品牌剧场",
    "group": "AI-16",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-04",
    "name": "文件流带（File Stream Ribbon）",
    "domain": "启动与品牌剧场",
    "group": "AI-16",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-05",
    "name": "启动交响（Boot Symphony）",
    "domain": "启动与品牌剧场",
    "group": "AI-16",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-06",
    "name": "仪式编排器（Ceremony Director）",
    "domain": "启动与品牌剧场",
    "group": "AI-16",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-07",
    "name": "设计令牌 2.0 与裸色值清零（Tokens 2.0 & De-bare-value）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-08",
    "name": "材质引擎（Material Engine）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-09",
    "name": "光影系统（Light & Shadow Language）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-10",
    "name": "图标语言 2.0（Icon Language 2.0）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-11",
    "name": "微交互精修（Micro-interaction Polish）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-12",
    "name": "布局响应式重构（Responsive Reflow）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-13",
    "name": "桌面 Profiles（Desktop Profiles）",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-14",
    "name": "智能窗口吸附 2.0（Smart Snap 2.0）",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-15",
    "name": "任务栏进化（Taskbar Evolution）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-16",
    "name": "文件管理器 2.0（Explorer 2.0）",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-17",
    "name": "全局拖放总线（Global Drop Bus）",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-18",
    "name": "迷你应用框架（Mini Apps Framework）",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-19",
    "name": "启动加速流水线（Boot Pipeline Acceleration）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-20",
    "name": "内存守护（Memory Warden）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-21",
    "name": "渲染帧预算器（Frame Budget Governor）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-22",
    "name": "IO 治理（IO Governance）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-23",
    "name": "崩溃叙事（Crash Narratives）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-24",
    "name": "基准与回归门禁（Bench 2.0 & Perf Gate）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-25",
    "name": "版本时光机（Version Time Machine）",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-26",
    "name": "全局文件标签系统（Global File Tags）",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-27",
    "name": "回收站 2.0（Recycle Bin 2.0）",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-28",
    "name": "传输指挥台（Transfer Command Center）",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-29",
    "name": "存档柜（Archive Vault）",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-30",
    "name": "数据血缘（Data Lineage）",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-31",
    "name": "隐私仪表盘（Privacy Dashboard）",
    "domain": "安全与隐私",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-32",
    "name": "应用防火墙 2.0（App Firewall 2.0）",
    "domain": "安全与隐私",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-33",
    "name": "诱饵文件系统（Canary Files）",
    "domain": "安全与隐私",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-34",
    "name": "紧急擦拭（Panic Protocol）",
    "domain": "安全与隐私",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-35",
    "name": "信任链中心（Trust Chain Center）",
    "domain": "安全与隐私",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-36",
    "name": "隐身会话（Incognito Sessions）",
    "domain": "安全与隐私",
    "group": "AI-10",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-37",
    "name": "协议中枢（Deep Link Hub）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-38",
    "name": "脚本安全屋（Script Safehouse）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-39",
    "name": "资源包格式（.vxs Packs）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-40",
    "name": "无障碍 2.0（Accessibility 2.0）",
    "domain": "无障碍与本地化",
    "group": "AI-19",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-41",
    "name": "多语言中心 2.0（i18n Center 2.0）",
    "domain": "无障碍与本地化",
    "group": "AI-19",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-42",
    "name": "诊断导出包（Diagnostic Bundle）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-43",
    "name": "多显示器编排 2.0（Multi-Monitor 2.0）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-44",
    "name": "外设快捷层（Peripheral Shortcut Layer）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-45",
    "name": "音频路由器（Audio Router）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-46",
    "name": "色彩与时辰（Color & Circadian）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-47",
    "name": "无线中心（Wireless Center）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-48",
    "name": "性能模式切换器（Performance Modes）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-49",
    "name": "氛围音景引擎（Soundscape Engine）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-50",
    "name": "焦点舱 2.0（Focus Cabin 2.0）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-51",
    "name": "通知交互进化（Notification Interactivity）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-52",
    "name": "声景反馈系统（Sonic Feedback System）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-53",
    "name": "环境辉光（Ambient Glow）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-54",
    "name": "节律助手（Rhythm Assistant）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-55",
    "name": "空状态设计系统（Empty States）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-56",
    "name": "错误叙事 2.0（Error Narratives）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-57",
    "name": "引导体系（Onboarding System）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "U-58",
    "name": "键盘全景（Keyboard Everywhere）",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-59",
    "name": "触控支持基础（Touch Foundations）",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "ASCENT-60",
    "delivered": false
  },
  {
    "id": "U-60",
    "name": "品质关卡（Quality Gates）",
    "domain": "工程质量、性能与收官",
    "group": "AI-17",
    "stage": "ASCENT-60",
    "delivered": true
  },
  {
    "id": "N-01",
    "name": "窗口时间机器（Window Timeline）",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "NEXT-40",
    "delivered": false
  },
  {
    "id": "N-02",
    "name": "舞台管理器（Window Stage Manager）",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "NEXT-40",
    "delivered": false
  },
  {
    "id": "N-03",
    "name": "窗口规则引擎（Window Rules Engine）",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "NEXT-40",
    "delivered": false
  },
  {
    "id": "N-04",
    "name": "任意窗口画中画（Universal PiP）",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "NEXT-40",
    "delivered": false
  },
  {
    "id": "N-05",
    "name": "工作区场景（Workspace Scenes）",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "NEXT-40",
    "delivered": false
  },
  {
    "id": "N-06",
    "name": "动效编排系统（Motion Orchestrator）",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "NEXT-40",
    "delivered": false
  },
  {
    "id": "N-07",
    "name": "壁纸工坊 2.0（Wallpaper Studio）",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "NEXT-40",
    "delivered": false
  },
  {
    "id": "N-08",
    "name": "主题工坊（Theme Studio）",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "NEXT-40",
    "delivered": false
  },
  {
    "id": "N-09",
    "name": "桌面小组件系统（Widget Board + SDK）",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "NEXT-40",
    "delivered": false
  },
  {
    "id": "N-10",
    "name": "锁屏与仪式屏（Lock Screen）",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-11",
    "name": "实况场景组件（Living Scene Widgets）",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-12",
    "name": "图标包系统（Icon Pack System）",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-13",
    "name": "命令面板（Command Palette）",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-14",
    "name": "统一搜索 2.0（Unified Search）",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-15",
    "name": "剪贴板历史中心（Clipboard History）",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-16",
    "name": "截图标注与贴图（Snip & Pin）",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-17",
    "name": "快捷键中心（Shortcuts Hub）",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-18",
    "name": "自动化宏引擎（Macro Engine）",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-19",
    "name": "性能 HUD 悬浮窗（Perf HUD）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-20",
    "name": "电源与电池管家（Power Manager）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-21",
    "name": "网络指挥台（Network Command Center）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-22",
    "name": "存储健康仪表盘（Storage Health）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-23",
    "name": "设备与外设中心（Device Center）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-24",
    "name": "预测性启动预热（Predictive Launch）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-25",
    "name": "健康自愈中心（Self-Healing Center）",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-26",
    "name": "插件运行时 2.0（Plugin Runtime 2.0）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-27",
    "name": "插件市场（Marketplace & Registry）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-28",
    "name": "开放 IPC API 网关（Open API Gateway）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-29",
    "name": "variable-cli（命令行控制台）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-30",
    "name": "浏览器伴侣扩展（Browser Companion）",
    "domain": "开放生态",
    "group": "AI-14",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-31",
    "name": "本地使用洞察（Local Insights）（AI-1）",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-32",
    "name": "智能通知整理（Smart Notifications）（AI-4）",
    "domain": "声音与通知",
    "group": "AI-16",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-33",
    "name": "情绪引擎（Mood Engine）（AI-4）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-35",
    "name": "多环境分身（Multi-Instance Sandbox）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "N-36",
    "name": "跨设备接力（Session Relay）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "NEXT-40",
    "delivered": true
  },
  {
    "id": "M-01",
    "name": "摇一摇最小化（Aero Shake）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-02",
    "name": "窗口卷帘（Roll-Up）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-03",
    "name": "最小化窗口抽屉（Minimized Drawer）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-04",
    "name": "窗口体检与无响应标记（Window Health）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-05",
    "name": "跨屏摆渡走廊（Monitor Ferry）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-06",
    "name": "窗口挂起与恢复（Suspend / Resume）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-07",
    "name": "对齐参考线（Alignment Guides）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-08",
    "name": "精炼 Alt+Tab（Alt+Tab Refinement）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-09",
    "name": "悬停聚焦（X-Mouse，可选）",
    "domain": "窗口与空间管理",
    "group": "AI-01",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-10",
    "name": "跳转列表（Jump Lists）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-11",
    "name": "托盘收纳抽屉（Tray Overflow Drawer）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-12",
    "name": "时钟多时区与细节（Clock Details）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-13",
    "name": "任务栏等待态规范（Launch Pending State）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-14",
    "name": "托盘 IM 未读聚合（IM Unread Aggregate）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-15",
    "name": "任务栏空区菜单定制（Taskbar Blank Menu）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-16",
    "name": "任务栏时钟的媒体呼吸（Media Breathing，可选）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-17",
    "name": "任务栏便签速贴（Quick Sticky）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-18",
    "name": "外设音量滚轮规范（Volume Wheel Etiquette）",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-19",
    "name": "右键菜单自定义编辑器（Context Menu Editor）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-20",
    "name": "同名操作选择记忆（Conflict Choice Memory）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-21",
    "name": "校验和工具（Checksum Utility）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-22",
    "name": "空格快速预览（Quick Look）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-23",
    "name": "压缩包目录浏览（Zip Browse-Only）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-24",
    "name": "目录置顶书签条（Places Pins）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-25",
    "name": "文件锁定侦探（File Lock Detective）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-26",
    "name": "环境回收站安全网（Delete Safety Net）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-27",
    "name": "目录监控哨兵（Folder Sentinel）",
    "domain": "文件与数据能力",
    "group": "AI-09",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-28",
    "name": "键位使用统计（Keymap Telemetry，本地）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-29",
    "name": "长按加速曲线（Key Repeat Profiling）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-30",
    "name": "鼠标侧键可编程（XButton Programming）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-31",
    "name": "启动槽可视化分配（Launch Slots Editor）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-32",
    "name": "每窗口输入法状态（Per-Window IME State）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-33",
    "name": "按键回显（Key Cast Overlay）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-34",
    "name": "Esc 层级语义规范（Esc Semantics）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-35",
    "name": "滚轮语义全局规范（Wheel Semantics）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-36",
    "name": "键位变更影响预览（Keymap Change Preview）",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-37",
    "name": "图标缓存校验与自愈（Icon Cache Integrity）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-38",
    "name": "UWP 应用识别增强（UWP Awareness）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-39",
    "name": "提权应用协作提示（Elevation Etiquette）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-40",
    "name": "高刷自适应时长（High-Refresh Timing）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-41",
    "name": "外设驱动软件共存协议（Peripheral Driver Coexistence）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-42",
    "name": "多实例应用任务栏区分（Instance Badging）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-43",
    "name": "便携路径漂移自愈（Portable Path Healing）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-44",
    "name": "嵌入应用崩溃善后（Embed Crash Aftercare）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-45",
    "name": "输入设备热插拔稳定（Input Hot-Plug Stability）",
    "domain": "兼容性防线",
    "group": "AI-12",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-46",
    "name": "日志轮转与配额（Log Rotation Quota）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-47",
    "name": "设置迁移预检（Settings Migration Preflight）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-48",
    "name": "数据库紧凑会话（DB Compaction Session）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-49",
    "name": "图标缓存 LRU 治理（Icon Cache LRU）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-50",
    "name": "事件风暴削峰（Event Storm Shedding）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-51",
    "name": "长跑浸泡测试基建（Soak Test Harness）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-52",
    "name": "冷启动 A/B 对照（Boot A/B Comparator）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-53",
    "name": "崩溃转储收集与符号化（Crash Dump Pipeline）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-54",
    "name": "资源公平调度（Resource Fairness）",
    "domain": "工程质量、性能与收官",
    "group": "AI-13",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-55",
    "name": "路由注册公开表（Route Registry Spec）",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-56",
    "name": "设置项自动文档（Settings Auto-Doc）",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-57",
    "name": "本地出站桥（Local Webhook Bridge，opt-in）",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-58",
    "name": "插件开发热重载（Plugin Dev Reload）",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-59",
    "name": "第三方嵌入声明协议（Embed Manifest Protocol）",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-60",
    "name": "测试钩子规范（Test Hooks Standard）",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-61",
    "name": "变更日志自动化（Changelog Automation）",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-62",
    "name": "社区翻译工作台格式（i18n Crowd Format）",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-63",
    "name": "资源包安全扫描（Pack Safety Scan）",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-64",
    "name": "壁纸主色主题采样（Wallpaper Accent Sampling）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-65",
    "name": "昼夜壁纸组（Day-Around Wallpaper Set）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-66",
    "name": "音量淡变防爆音（Volume Fade Guard）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-67",
    "name": "桌面纯净模式（Pure Mode）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-68",
    "name": "屏保时钟（Screensaver Clock）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-69",
    "name": "今日简报卡（Daily Briefing Card）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-70",
    "name": "壁纸快捷操作（Wallpaper Context Actions）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-71",
    "name": "悬停延迟全局面板（Hover Latency Controls）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-72",
    "name": "环境氛围会话恢复（Ambient Session Restore）",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-73",
    "name": "系统辅助功能桥（Accessibility Bridge）",
    "domain": "无障碍与本地化",
    "group": "AI-19",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-74",
    "name": "系统高对比度跟随（HC System Follow）",
    "domain": "无障碍与本地化",
    "group": "AI-19",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-75",
    "name": "动效时长缩放（Motion Duration Scale）",
    "domain": "无障碍与本地化",
    "group": "AI-19",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-76",
    "name": "屏幕阅读器标注审计（ARIA Audit）",
    "domain": "无障碍与本地化",
    "group": "AI-19",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-77",
    "name": "简繁转换用户词表（S2T User Lexicon）",
    "domain": "无障碍与本地化",
    "group": "AI-19",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-78",
    "name": "区域格式跟随（Locale Format Follow）",
    "domain": "无障碍与本地化",
    "group": "AI-19",
    "stage": "SUMMIT-90",
    "delivered": true
  },
  {
    "id": "M-79",
    "name": "前端错误聚合看板（FE Error Board）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-80",
    "name": "IPC 调用追踪（IPC Trace View）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-81",
    "name": "设置漂移测试基建（Settings Drift Tests）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-82",
    "name": "视觉回归多主题矩阵（Visual Matrix Regression）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-83",
    "name": "键位审计 CI 门禁（Keymap CI Gate）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-84",
    "name": "性能影响声明（Perf Impact Statement）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-85",
    "name": "依赖审计自动化（Dependency Audit Automation）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-86",
    "name": "文档链接检查（Doc Link Checker）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-87",
    "name": "发版演练脚本（Release Rehearsal）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-88",
    "name": "全局 Tooltip 规范（Tooltip Standard）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-89",
    "name": "单位与数字规范（Units & Numbers Standard）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "M-90",
    "name": "跨午夜会话正确性（Midnight Rollover Correctness）",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "SUMMIT-90",
    "delivered": false
  },
  {
    "id": "V-01",
    "name": "桌面图标排列系统",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-02",
    "name": "系统桌面图标管理",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-03",
    "name": "桌面图标锁定",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-04",
    "name": "桌面双击空白动作",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-05",
    "name": "图标标签可读性自适应",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-06",
    "name": "桌面图标密度档位",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-07",
    "name": "桌面敲字定位图标",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-08",
    "name": "回收站图标状态徽标",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-09",
    "name": "新建菜单模板中心",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-10",
    "name": "图标让位 FLIP 微动效",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-11",
    "name": "开始菜单字母索引条",
    "domain": "任务栏与开始菜单",
    "group": "AI-04",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-12",
    "name": "「最近添加」与「高频使用」自动分组",
    "domain": "任务栏与开始菜单",
    "group": "AI-04",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-13",
    "name": "固定应用文件夹",
    "domain": "任务栏与开始菜单",
    "group": "AI-04",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-14",
    "name": "开始菜单右键高级操作",
    "domain": "任务栏与开始菜单",
    "group": "AI-04",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-15",
    "name": "任务栏图标中键新开实例",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-16",
    "name": "拖到任务栏图标打开",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-17",
    "name": "任务栏溢出折叠区",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-18",
    "name": "运行指示样式三选",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-19",
    "name": "关机前会话清单",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-20",
    "name": "电源菜单增强",
    "domain": "任务栏与开始菜单",
    "group": "AI-03",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-21",
    "name": "经典系统菜单复刻",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-22",
    "name": "失联窗口救援",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-23",
    "name": "调整大小实时几何提示",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-24",
    "name": "窗口多选编组操作",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-25",
    "name": "焦点历史回溯",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-26",
    "name": "窗口位置互换",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-27",
    "name": "嵌入窗口焦点联动",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-28",
    "name": "窗口分布小地图",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-29",
    "name": "窗口色带标记",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-30",
    "name": "拖拽中断与回弹",
    "domain": "窗口与空间管理",
    "group": "AI-02",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-31",
    "name": "文件夹视图记忆",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-32",
    "name": "即时过滤与命中高亮",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-33",
    "name": "复制为路径常驻",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-34",
    "name": "拖拽计数徽标与落点确认",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-35",
    "name": "两文件属性对比",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-36",
    "name": "长路径与特殊名防呆",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-37",
    "name": "按类型智能选取",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-38",
    "name": "预览锁定",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-39",
    "name": "地址栏模糊跳转",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-40",
    "name": "压缩包提取向导",
    "domain": "文件与数据能力",
    "group": "AI-10",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-41",
    "name": "内联计算",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-42",
    "name": "屏幕取字 OCR",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-43",
    "name": "二维码速递",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-44",
    "name": "纯文本净化粘贴",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-45",
    "name": "命令面板宏收藏",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-46",
    "name": "全局速记",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-47",
    "name": "搜索历史隐私开关",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-48",
    "name": "运行框自动补全",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-49",
    "name": "时间戳速插",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-50",
    "name": "键位速查表导出",
    "domain": "效率与工具中枢",
    "group": "AI-07",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-51",
    "name": "亮度音量微步进",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-52",
    "name": "系统可靠性时间线",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-53",
    "name": "端口占用侦探",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-54",
    "name": "临时保持唤醒",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-55",
    "name": "大文件雷达",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-56",
    "name": "环境运行时长与重启建议",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-57",
    "name": "进程优先级预设",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-58",
    "name": "更新闲时下载",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-59",
    "name": "断电恢复自检",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-60",
    "name": "启动项耗时归因",
    "domain": "系统集成与硬件",
    "group": "AI-11",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-61",
    "name": "鼠标手感面板",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-62",
    "name": "指针方案管理",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-63",
    "name": "指针轨迹",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-64",
    "name": "点击涟漪反馈",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-65",
    "name": "大写锁定全局提示",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-66",
    "name": "按键重映射",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-67",
    "name": "触控板自然滚动方向",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-68",
    "name": "打字音效可选",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-69",
    "name": "指针精确模式",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-70",
    "name": "拖拽阈值与防手滑",
    "domain": "键盘与输入手感",
    "group": "AI-06",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-71",
    "name": "本地壁纸精选轮换",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-72",
    "name": "壁纸饱和度与明度调节",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-73",
    "name": "农历节气与节日",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-74",
    "name": "界面密度档位",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-75",
    "name": "界面几何风格",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-76",
    "name": "界面字体偏好",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-77",
    "name": "焦点环样式选择",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-78",
    "name": "全局 UI 图标尺寸档位",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-79",
    "name": "系统模式与应用模式深浅分离",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-80",
    "name": "强调色对比度守护",
    "domain": "视觉、个性化与氛围",
    "group": "AI-18",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-81",
    "name": "开放安装器",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-82",
    "name": "环境变量编辑器",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-83",
    "name": "计划任务工坊",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-84",
    "name": "文件关联快照与还原",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-85",
    "name": "卸载善后报告",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-86",
    "name": "启动项延迟编排",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-87",
    "name": "服务依赖图",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-88",
    "name": "CLI 交互式教程",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-89",
    "name": "配置对比工具",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-90",
    "name": "沙盒试用面板",
    "domain": "开放生态",
    "group": "AI-15",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-91",
    "name": "演示模式",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-92",
    "name": "关机倒计时取消",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-93",
    "name": "Windows 偏好搬家向导",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-94",
    "name": "键位体检医生",
    "domain": "键盘与输入手感",
    "group": "AI-05",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-95",
    "name": "卸载器数据抉择",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-96",
    "name": "桌面归档建议",
    "domain": "桌面与图标表达",
    "group": "AI-04",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-97",
    "name": "右键打印",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-98",
    "name": "打印队列查看器",
    "domain": "效率与工具中枢",
    "group": "AI-08",
    "stage": "化境",
    "delivered": true
  },
  {
    "id": "V-99",
    "name": "依赖诚实声明页 v2",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "化境",
    "delivered": false
  },
  {
    "id": "V-100",
    "name": "收官毕业页",
    "domain": "工程质量、性能与收官",
    "group": "AI-20",
    "stage": "化境",
    "delivered": false
  }
];
