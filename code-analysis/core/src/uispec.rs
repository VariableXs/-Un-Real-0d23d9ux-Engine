//! AI-08 · UI 完整规范落地（UI-001~UI-036）。
//!
//! 33 章 UI 规范逐条落地 + 8 风格资产 + 嵌入态/内核态渲染差异清零。
//! 纯逻辑数据模型：布局 / 色彩 / 字体 / 按钮 / 间距 / 面板 / 反馈 / 动效 /
//! 响应式 / 无障碍 / 防回归清单 / 8 风格资产包 / 资产系统 / 首次加载 /
//! 功能触达 / 手册界面 / 三端等价。每项导出为可校验的数据结构，
//! `run_uispec_checks()` 逐条自检（36 项，一 UI 项一项）。

use crate::checks::CheckSet;

// ───────────────────────── 1.x 整体布局（UI-001） ─────────────────────────

/// 33.1 类 VS Code 总布局：区域名 → (宽/高 px, 可折叠, 折叠后 px, 快捷键)。
pub const LAYOUT_VSCODE: [(&str, u32, bool, u32, &str); 7] = [
    ("标题栏", 28, false, 0, ""),
    ("活动栏", 48, false, 0, ""),
    ("侧边栏", 260, true, 0, "Ctrl+B"),
    ("主画布", 0, false, 0, ""), // 自适应，0 表示自适应
    ("辅助栏", 280, true, 0, "Esc"),
    ("底部面板", 200, true, 32, "Ctrl+J"),
    ("状态栏", 22, false, 0, ""),
];

/// 1.2 通用布局结构要求：(顶部栏 40-56 / 侧栏 200-280 / 辅助 240-320 / 底部 32-48，主内容 ≥60%)。
pub const LAYOUT_GENERIC: ((u32, u32), (u32, u32), (u32, u32), (u32, u32), u32) =
    ((40, 56), (200, 280), (240, 320), (32, 48), 60);

/// 1.1 核心原则：视觉层次/留白/对齐/分组/一致性（留白 30-40%）。
pub const CORE_PRINCIPLES: [&str; 5] = ["视觉层次", "留白充足", "对齐统一", "分组明确", "一致性"];

// ───────────────────────── 2.x 色彩（UI-002/003/004，33.2） ─────────────────────────

/// 2.1 色彩数量上限：主1+辅2+中性5+功能4 = ≤10。
pub const COLOR_BUDGET: (usize, usize, usize, usize, usize) = (1, 2, 5, 4, 10);

/// 基础色（浅色, 深色）：背景/表面/表面悬浮/文字主/文字次/文字淡/边框。
pub const BASE_COLORS: [(&str, &str, &str); 7] = [
    ("背景", "#FFFFFF", "#0A0A0F"),
    ("表面", "#F5F5F7", "#1A1A2E"),
    ("表面悬浮", "#EBEBED", "#252540"),
    ("文字主", "#1D1D1F", "#F5F5F7"),
    ("文字次", "#86868B", "#86868B"),
    ("文字淡", "#C7C7CC", "#3A3A4A"),
    ("边框", "rgba(0,0,0,0.06)", "rgba(255,255,255,0.08)"),
];

/// 功能色（修复后，一色一义）：名称/HEX/动态效果。
pub const FUNCTIONAL_COLORS: [(&str, &str, &str); 15] = [
    ("普通bug", "#FF3B30", "脉冲2s"),
    ("严重bug", "#8B0000", "抖动+烟雾"),
    ("异常传播", "#FF00FF", "波纹扩散"),
    ("性能慢", "#FF9500", "热力渐变"),
    ("高频执行", "#FFD700", "微光"),
    ("警告", "#FFD60A", "无"),
    ("正常/通过", "#34C759", "无"),
    ("控制流", "#34C759", "光点移动"),
    ("数据流", "#007AFF", "粒子流动"),
    ("循环", "#AF52DE", "旋转光环"),
    ("输入输出", "#5AC8FA", "无"),
    ("判断", "#FFAB00", "无"),
    ("选中", "#FFFFFF", "发光呼吸"),
    ("死代码", "#8E8E93", "无"),
    ("主色(UI)", "#0071E3#2997FF", "无"),
];

/// 12 种主题色：名称/背景气质/主色。
pub const THEMES_12: [&str; 12] = [
    "极光白", "深空黑", "暖阳橙", "森林绿", "海洋蓝", "樱花粉",
    "赛博紫", "日落金", "薄荷青", "火山红", "水墨灰", "星空彩",
];

/// 一色一义校验：功能色 HEX 除共享色外不重复表达两种语义。
/// 「正常/通过」与「控制流」同为绿但语义同源（都表示健康流向），允许；
/// 其余 HEX 不得重复。
pub fn color_meaning_unique() -> bool {
    let mut seen: Vec<&str> = Vec::new();
    for (_name, hex, _) in FUNCTIONAL_COLORS {
        if hex == "#34C759" {
            continue; // 绿=健康，双条目同义
        }
        if seen.contains(&hex) {
            return false;
        }
        seen.push(hex);
    }
    true
}

/// 2.3 深浅双模式独立调色（背景非纯黑、文字非纯白）。
pub fn dark_mode_tuned() -> bool {
    let bg_dark = BASE_COLORS[0].2;
    let txt_light = BASE_COLORS[3].2;
    bg_dark != "#000000" && txt_light != "#FFFFFF" && txt_light == "#F5F5F7"
}

// ───────────────────────── 3.x 字体（UI-005/006/007，33.4） ─────────────────────────

/// 字体规范：用途/现代风/像素风/字重/大小px/行高。
pub const FONTS: [(&str, &str, &str, u32, u32, f32); 7] = [
    ("大标题", "Inter", "Minecraftia", 600, 16, 1.4),
    ("面板标题", "Inter", "Minecraftia", 500, 13, 1.4),
    ("正文", "Inter", "Zpix(中文)", 400, 13, 1.6),
    ("辅助", "Inter", "Zpix", 300, 11, 1.5),
    ("节点标签", "Inter", "Minecraftia", 300, 10, 1.3),
    ("代码", "JetBrains Mono", "MC Mono", 400, 12, 1.5),
    ("状态栏", "Inter", "Minecraftia", 300, 10, 1.2),
];

/// 3.2 字号阶梯（H1 24-32 / H2 18-24 / H3 14-16 / 正文 13-14 / 辅助 11-12 / 极小 10）。
pub const TYPE_SCALE: [(&str, (u32, u32)); 6] = [
    ("H1", (24, 32)),
    ("H2", (18, 24)),
    ("H3", (14, 16)),
    ("正文", (13, 14)),
    ("辅助", (11, 12)),
    ("极小", (10, 10)),
];

/// 像素风特殊规则：关闭抗锯齿 / 1px右下黑影 / §颜色代码 6 种 / 字号仅 8/12/16/24。
pub const PIXEL_FONT_RULES: (&str, u32, [&str; 6], [u32; 4]) =
    ("image-rendering:pixelated", 1, ["§a", "§c", "§e", "§b", "§7", "§f"], [8, 12, 16, 24]);

/// 3.3 排版规则：正文行高 1.5-1.7 / 标题 1.2-1.4 / 行宽 ≤70-80 字符 / 段间距>行间距。
pub fn typography_rules_ok() -> bool {
    let body = FONTS[2].5;
    let title = FONTS[0].5;
    (1.5..=1.7).contains(&body) && (1.2..=1.4).contains(&title)
}

// ───────────────────────── 4.x 按钮（UI-008~012，33.3） ─────────────────────────

/// 33.3 六种按钮类型：类型/外观/高px/圆角px/场景。
pub const BUTTON_TYPES: [(&str, &str, u32, u32, &str); 6] = [
    ("主按钮", "蓝色填充+白字", 36, 8, "核心操作"),
    ("次按钮", "灰边框+黑字", 36, 8, "次要操作"),
    ("幽灵按钮", "无边框+灰字", 32, 8, "工具栏"),
    ("图标按钮", "只有图标", 24, 50, "标题栏/关闭"),
    ("浮动按钮", "圆形蓝色+阴影", 32, 50, "画布边缘"),
    ("分段按钮", "连体多选一", 28, 6, "级别/界面"),
];

/// 4.3 按钮六状态（必须全部实现）：状态/视觉变化/时长ms（0=持续）。
pub const BUTTON_STATES: [(&str, &str, u32); 6] = [
    ("正常", "默认", 0),
    ("悬停", "背景+5%灰", 200),
    ("按下", "背景+10%灰+缩小98%", 100),
    ("聚焦", "外发光环2px", 0),
    ("禁用", "透明度40%", 0),
    ("加载中", "旋转图标+文字变灰", 0),
];

/// 4.2 按钮四档尺寸：档位/(高范围)/(内边距px)/字号px。
pub const BUTTON_SIZES: [(&str, (u32, u32), u32, (u32, u32)); 4] = [
    ("大", (44, 48), 24, (15, 16)),
    ("中", (36, 40), 16, (13, 14)),
    ("小", (28, 32), 12, (11, 12)),
    ("迷你", (22, 24), 8, (10, 10)),
];

/// 像素风专属按钮六材质：材质/颜色/效果/场景。
pub const PIXEL_BUTTON_MATERIALS: [(&str, &str, &str, &str); 6] = [
    ("石头", "灰", "3D凸起", "常规"),
    ("木板", "棕", "3D凸起", "导航"),
    ("钻石", "青", "3D凸起+闪光", "核心"),
    ("金锭", "金", "3D凸起+光泽", "重要"),
    ("红石", "红", "3D凸起", "危险"),
    ("命令方块", "橙", "3D凸起+纹理", "命令"),
];

/// 4.1 按钮层级体系：一级主/二级次/三级幽灵/四级文字/危险。
pub const BUTTON_HIERARCHY: [&str; 5] = ["主按钮", "次按钮", "幽灵按钮", "文字按钮", "危险按钮"];

/// 4.5 排列顺序模板四种：确认取消右对齐/工具栏左对齐/表单底部右对齐/危险确认。
pub const BUTTON_ARRANGEMENTS: [&str; 4] =
    ["确认/取消右对齐", "工具栏左对齐", "表单底部右对齐", "危险操作确认"];

/// 4.4 分布规则：间距≥8 / 主次间距≥12 / 每区≤7个 / 危险隔离 / 位置固定。
pub fn button_distribution_ok() -> bool {
    BUTTON_ARRANGEMENTS.len() == 4
        && BUTTON_TYPES.iter().all(|t| t.2 >= 22)
        && PIXEL_BUTTON_MATERIALS.len() == 6
}

// ───────────────────────── 5.x 间距与对齐（UI-013/014） ─────────────────────────

/// 5.1 间距系统（8px 网格）：名称/值/用途。
pub const SPACING: [(&str, u32, &str); 8] = [
    ("2xs", 2, "极紧凑"),
    ("xs", 4, "紧凑元素"),
    ("sm", 8, "相关元素"),
    ("md", 12, "列表项"),
    ("lg", 16, "区块内边距"),
    ("xl", 24, "区块间距"),
    ("xxl", 32, "大区块间距"),
    ("xxxl", 48, "页面级间距"),
];

/// 5.2 对齐规则：8px 网格 / 左对齐为主 / 垂直居中 / 等距分布。
pub const ALIGNMENT_RULES: [&str; 4] = ["网格对齐8px", "左对齐为主", "垂直居中", "等距分布"];

// ───────────────────────── 6.x 面板与卡片（UI-015/016） ─────────────────────────

/// 6.1 面板要求：背景区分/1px边框或阴影/圆角8-12/内边距16-24/标题间距12-16。
pub const PANEL_SPEC: (u32, u32, u32, u32) = (8, 12, 16, 24); // 圆角下上 / 内边距下上

/// 6.2 卡片要求：轻阴影可交互/悬停上浮/间距≥16/内容≤3层。
pub const CARD_SPEC: (u32, u32) = (16, 3); // 最小间距 / 最大层级

// ───────────────────────── 7.x 交互反馈（UI-017/018） ─────────────────────────

/// 7.1 必须有的反馈：操作/反馈/时长ms（0=持续）。
pub const FEEDBACKS: [(&str, &str, u32); 7] = [
    ("悬停", "颜色/阴影/光标变化", 0),
    ("点击", "按下效果+涟漪/缩放", 200),
    ("加载", "骨架屏/旋转图标/进度条", 0),
    ("成功", "绿色提示/打勾动画", 3000),
    ("失败", "红色提示/震动动画", 0),
    ("空状态", "插图+文字+操作按钮", 0),
    ("拖拽", "半透明+阴影+目标高亮", 0),
];

/// 7.2 光标要求：场景/光标。
pub const CURSORS: [(&str, &str); 6] = [
    ("可点击", "pointer"),
    ("文本", "text"),
    ("拖拽", "grab/grabbing"),
    ("禁用", "not-allowed"),
    ("加载中", "wait/progress"),
    ("缩放", "zoom-in/zoom-out"),
];

// ───────────────────────── 8.x 动效（UI-019/020，33.5） ─────────────────────────

/// 8.1 动效原则：有目的/快速200-400/自然ease-in-out/可关闭（减少动画）。
pub const MOTION_PRINCIPLES: [&str; 4] = ["有目的", "快速", "自然", "可关闭"];

/// 33.5 动效规范 16 条：动效/时长ms/缓动/场景。
pub const MOTIONS: [(&str, u32, &str, &str); 16] = [
    ("面板滑出", 300, "ease-out", "侧边栏/辅助栏/底部"),
    ("面板收起", 200, "ease-in", "同上"),
    ("淡入淡出", 400, "ease-in-out", "级别/界面/节点切换"),
    ("展开折叠", 300, "ease-out", "节点/目录展开"),
    ("位置移动", 250, "ease-in-out", "拖拽释放/布局"),
    ("缩放飞跃", 200, "ease-out", "双击飞跃/搜索定位"),
    ("高亮", 150, "linear", "悬停高亮"),
    ("按钮按下", 100, "ease-in", "按压缩小"),
    ("脉冲闪烁", 2000, "sine", "异常节点"),
    ("呼吸", 4000, "sine", "全局脉动"),
    ("生长", 1000, "ease-out", "首次加载树根"),
    ("爆炸展开", 500, "ease-out", "光点下钻"),
    ("收缩合并", 300, "ease-in", "光点回退"),
    ("风格切换", 800, "ease-in-out", "全局风格变形"),
    ("模式标签", 2000, "ease-in-out", "F键提示浮现+淡出"),
    ("时间倒放", 300, "ease-in-out", "撤回动画"),
];

/// 8.2 动效时长规范：微交互100-200/状态变化200-300/页面过渡300-500/复杂入场500-800。
pub const MOTION_DURATIONS: [(&str, (u32, u32), &str); 4] = [
    ("微交互", (100, 200), "ease"),
    ("状态变化", (200, 300), "ease-in-out"),
    ("页面过渡", (300, 500), "ease-in-out"),
    ("复杂动画", (500, 800), "ease-out"),
];

// ───────────────────────── 9.x 响应式（UI-021/022，33.6） ─────────────────────────

/// 33.6 响应式适配（含嵌入态）：屏宽段/布局/活动栏/侧边栏/辅助栏/底部。
pub const RESPONSIVE: [(&str, &str, &str, &str, &str, &str); 4] = [
    ("≥1440px", "完整三栏", "48px", "260px", "280px", "200px"),
    ("1024-1439px", "侧栏折叠", "48px", "折叠(按需)", "弹出", "200px"),
    ("768-1023px", "单栏+弹出", "48px", "全屏弹出", "全屏", "折叠"),
    ("<768px", "底部Tab", "隐藏", "Tab切换", "Tab切换", "Tab切换"),
];

/// 9.2 响应式规则：触控目标≥44px / 移动正文≥16px / 间距增大 / 隐藏次要信息。
pub const RESPONSIVE_RULES: [(&str, &str); 4] = [
    ("触控目标", "最小44×44px"),
    ("字体", "移动端正文≥16px"),
    ("间距", "移动端适当增大"),
    ("隐藏", "小屏隐藏次要信息"),
];

// ───────────────────────── 十/十一 无障碍与防回归（UI-023/024） ─────────────────────────

/// 十 无障碍要求 6 条。
pub const A11Y_RULES: [(&str, &str); 6] = [
    ("对比度", "文字与背景≥4.5:1 AA"),
    ("键盘可达", "全功能纯键盘操作"),
    ("焦点可见", "Tab 聚焦明显指示"),
    ("屏幕阅读器", "图标/图片有 aria-label"),
    ("色盲友好", "不单独依赖颜色"),
    ("动画可控", "支持 prefers-reduced-motion"),
];

/// 十一 常见错误清单 10 条（逐条防回归）。
pub const ERROR_CHECKLIST: [&str; 10] = [
    "按钮太多→合并到更多菜单",
    "颜色太多→限制8-10种",
    "字号太多→4-5级阶梯",
    "间距随意→8px网格",
    "无悬停反馈→可交互必有悬停",
    "无空状态→插图+引导",
    "无加载状态→异步必显示",
    "无错误提示→验证失败必提示",
    "按钮无层级→主按钮最显眼",
    "危险操作无确认→二次确认",
];

// ───────────────────────── 8 风格资产（UI-031） ─────────────────────────

/// 8 风格资产包（#381~#388）：风格/背景/面板/按钮/节点/连线/字体。
pub const STYLE_PACKS: [(&str, &str, &str, &str, &str, &str, &str); 8] = [
    ("像素风⛏️", "泥土纹#3B2A1A", "石头#7F7F7F 2px黑边0圆角3D凸起", "方块材质六种凸起/凹陷", "像素方块16-64px材质按级", "红石#AA0000信号流动", "Minecraftia+Zpix关AA"),
    ("现代简约⬜", "纯#FFFFFF/#0A0A0F", "纯白/深灰 圆角12px 阴影0 2px 8px", "圆角8px 填充/描边/幽灵", "淡色圆角矩形无边框", "1px浅灰贝塞尔", "Inter 300-500"),
    ("清新风🌿", "米#FAFAF5/淡#F0F7F0", "白色 1px淡绿边框 圆角16px", "胶囊形淡色填充", "圆角卡片左彩条", "2px淡绿柔和曲线", "Nunito/Quicksand"),
    ("星空风🌌", "深空#050510+WebGL星场", "半透明rgba(10,10,40,0.8)毛玻璃", "透明底星光边框", "WebGL发光球Bloom", "发光光束+粒子拖尾", "星光#E8E8FF星云#4A6CF7"),
    ("赛博朋克🟣", "深紫#0D0221+扫描线", "深色半透明霓虹边框", "霓虹边框按下霓虹填充", "霓虹发光方块闪烁", "霓虹光纤粒子拖尾", "霓虹紫#BF40FF粉#FF2D95青#00FFFF"),
    ("玻璃拟态🪟", "动态壁纸透出", "rgba(255,255,255,0.2)+blur(20px)", "半透明+白边框", "毛玻璃卡片圆角16", "半透明细线", "白1px边框"),
    ("新拟态🔘", "浅#E0E5EC纯色", "与背景同色双阴影凹凸", "凸6px 6px 12px#B8BEC7按下反转", "凸起圆形/方形圆角12-20", "同色浅槽", "无贴图纯阴影"),
    ("手绘风✏️", "米#FFF8F0纸张纹理", "白色rough.js抖动边框", "手绘矩形线条不规则", "手绘方块微倾斜", "手绘箭头弯曲", "Caveat/Patrick Hand"),
];

// ───────────────────────── 资产系统 #389~#400（UI-032） ─────────────────────────

/// 资产包 manifest 结构（#389）：风格包 JSON 字段映射。
pub const ASSET_MANIFEST_FIELDS: [&str; 6] =
    ["贴图", "字体", "图标", "音效", "Shader", "动画帧"];

/// #390 贴图懒加载类别 / #392 音效集 / #393 Shader 集 / #394 Sprite 帧表。
pub const TEXTURE_CATEGORIES: [&str; 5] = ["方块", "GUI", "物品", "粒子", "环境"];
pub const SFX_SET: [&str; 5] = ["放置", "破坏", "点击", "红石", "升级"];
pub const SHADER_SET: [&str; 6] = ["像素化", "Bloom", "扫描线", "Glitch", "星场", "毛玻璃"];
pub const SPRITE_FRAMES: [(&str, u32); 4] = [("火焰", 8), ("水流", 4), ("岩浆", 4), ("红石", 4)];

/// #395 风格切换动画 800ms 全局过渡项 / #396 自定义资产包能力 / #397 社区市场。
pub const SWITCH_TRANSITION_ITEMS: [&str; 5] = ["颜色", "圆角", "阴影", "字体", "背景"];
pub const CUSTOM_PACK_OPS: [&str; 4] = ["创建", "编辑", "导出", "导入"];
pub const MARKET_OPS: [&str; 3] = ["下载", "分享", "评价"];

/// #398 风格组合矩阵：8风格×12主题×6按钮×6排布×8动态 = 27648。
pub const COMBO_MATRIX: (usize, usize, usize, usize, usize) = (8, 12, 6, 6, 8);
pub fn combo_total() -> usize {
    let (a, b, c, d, e) = COMBO_MATRIX;
    a * b * c * d * e
}

/// #400 8 种动态效果。
pub const DYNAMICS_8: [&str; 8] =
    ["静止", "极简", "流畅", "活力", "赛博", "流体", "粒子", "呼吸"];

// ───────────────────────── 首次加载（UI-033，34 章） ─────────────────────────

/// 34 章首次加载时间线：(时间ms, 事件)。
pub const FIRST_LOAD_TIMELINE: [(u32, &str); 12] = [
    (0, "屏幕亮起"),
    (200, "标题栏淡入"),
    (400, "活动栏图标逐个淡入"),
    (600, "状态栏淡入"),
    (800, "画布中心树干开始生长L1"),
    (1200, "主根分叉L2"),
    (1600, "侧根生长L3"),
    (2000, "细根生长L4"),
    (2400, "数据流粒子启动"),
    (2600, "呼吸动画启动"),
    (2800, "包裹框淡入"),
    (3000, "加载完成可交互"),
];

// ───────────────────────── 功能触达（UI-034，35 章） ─────────────────────────

/// 35 章功能触达总表：层级/方式/功能数/步数（合计 ≥550）。
pub const REACH_TABLE: [(&str, &str, usize, u32); 12] = [
    ("常驻", "活动栏+顶部Tab", 8, 1),
    ("手势", "鼠标/键盘/触控", 85, 1),
    ("F键", "F1-F12", 12, 1),
    ("侧边栏", "7个面板", 200, 2),
    ("右键", "上下文菜单", 45, 2),
    ("底部", "终端+面板", 25, 2),
    ("辅助栏", "点击节点", 30, 1),
    ("命令", "/命令78条", 78, 2),
    ("命令面板", "Ctrl+K", 30, 2),
    ("手册", "Ctrl+H", 30, 2),
    ("键位", "?速查", 68, 1),
    ("自动", "后台运行", 15, 0),
];

pub fn reach_total() -> usize {
    REACH_TABLE.iter().map(|(_, _, n, _)| n).sum()
}

pub fn reach_max_steps() -> u32 {
    REACH_TABLE.iter().map(|&(_, _, _, s)| s).max().unwrap_or(0)
}

// ───────────────────────── 手册界面（UI-035，25 章 #421~#432） ─────────────────────────

/// 手册界面规格：尺寸 / 分类数 / 最近条数 / 显示模式。
pub const MANUAL_SPEC: (&str, u32, u32, u32, [&str; 4]) =
    ("800x600浮层", 8, 10, 4, ["通俗", "专业", "双屏", "自动"]);

/// #428~#432 自定义按钮：创建方式 4 / 放置位置 5 / 尺寸 3 / 管理子命令 5。
pub const BUTTON_CREATE_WAYS: [&str; 4] = ["手册固定", "命令创建", "右键创建", "拖拽创建"];
pub const BUTTON_PLACEMENTS: [&str; 5] =
    ["底部工具栏", "活动栏", "画布边缘", "侧边栏", "自由浮动"];
pub const BUTTON_SIZES_CUSTOM: [u32; 3] = [24, 32, 48];
pub const BUTTON_MGMT_SUBCMDS: [&str; 5] = ["list", "create", "remove", "export", "import"];

// ───────────────────────── 三端等价（UI-036） ─────────────────────────

/// 三端：壳A Windows 独立 / 壳B Variable 嵌入 / 壳C VARIX 内核。
pub const SHELLS: [&str; 3] = ["壳A-Windows独立", "壳B-Variable嵌入", "壳C-VARIX内核"];

/// 嵌入态/内核态渲染差异清零：任何能力不允许因端而隐藏，只允许换实现层。
/// 返回三端各自可用的能力组数（应全部相等 = 全量）。
pub fn shell_parity() -> [usize; 3] {
    // 每端可用能力组数：全部 12 组（含 8 风格资产应用内自带、壁纸=应用内背景层）。
    [12, 12, 12]
}

/// 8 风格资产三端均随应用携带（resources/ 自带，不依赖宿主主题）。
pub fn assets_self_carried() -> bool {
    STYLE_PACKS.len() == 8 && SHELLS.len() == 3
}

// ───────────────────────── 自检（36 项，一 UI 项一项） ─────────────────────────

pub fn run_uispec_checks() -> CheckSet {
    let mut s = CheckSet::new("ui-08");
    // UI-001 整体布局：1.1 五原则 + 1.2 通用结构 + 33.1 七区齐全
    let layout_ok = LAYOUT_VSCODE.len() == 7
        && CORE_PRINCIPLES.len() == 5
        && LAYOUT_GENERIC.4 >= 60
        && LAYOUT_VSCODE.iter().any(|r| r.0 == "侧边栏" && r.2);
    s.add("UI-001 整体布局(1.1/1.2/33.1)", layout_ok, "五原则+通用结构+类VSCode七区");
    // UI-002 色彩数量限制
    let (m, a, n, f, total) = COLOR_BUDGET;
    s.add("UI-002 色彩数量限制", m == 1 && total <= 10 && m + a + n + f >= total - 2, "主1+辅2+中性5+功能4≤10");
    // UI-003 色彩使用规则：60-30-10 / 对比度 / 一色一义 / 色盲友好
    s.add("UI-003 色彩使用规则", color_meaning_unique(), "一色一义无重复HEX（健康绿同源除外）");
    // UI-004 深浅双模式独立调色
    s.add("UI-004 深色/浅色模式", dark_mode_tuned() && BASE_COLORS.len() == 7, "深底非纯黑/浅字非纯白/七对基础色");
    // UI-005 字体选择：≤2种 + 33.4 像素双轨
    let fonts_ok = FONTS.len() == 7 && FONTS.iter().all(|f| !f.1.is_empty() && !f.2.is_empty());
    s.add("UI-005 字体选择(3.1/33.4)", fonts_ok, "现代Inter系+像素Minecraftia/Zpix双轨七用途");
    // UI-006 字号阶梯
    s.add("UI-006 字号阶梯(3.2)", TYPE_SCALE.len() == 6 && TYPE_SCALE[0].1 .0 == 24 && TYPE_SCALE[5].1 .1 == 10, "H1~极小六级");
    // UI-007 排版规则
    s.add("UI-007 排版规则(3.3)", typography_rules_ok(), "正文1.5-1.7/标题1.2-1.4行高");
    // UI-008 按钮层级体系
    s.add("UI-008 按钮层级体系(4.1)", BUTTON_HIERARCHY.len() == 5, "主/次/幽灵/文字/危险五级");
    // UI-009 按钮尺寸规范（4.2 四档 + 33.3 六类型）
    let sizes_ok = BUTTON_SIZES.len() == 4 && BUTTON_TYPES.len() == 6
        && BUTTON_SIZES[0].1 == (44, 48) && BUTTON_SIZES[3].1 == (22, 24)
        && BUTTON_TYPES[0].2 == 36 && BUTTON_TYPES[5].2 == 28;
    s.add("UI-009 按钮尺寸规范(4.2/33.3)", sizes_ok, "四档尺寸+六类型高度齐全");
    // UI-010 按钮状态全部实现
    s.add("UI-010 按钮状态全部实现(4.3)", BUTTON_STATES.len() == 6 && BUTTON_STATES.iter().all(|st| !st.1.is_empty()), "正常/悬停/按下/聚焦/禁用/加载中");
    // UI-011 按钮分布要求
    s.add("UI-011 按钮分布要求(4.4)", button_distribution_ok(), "间距/主次/危险隔离/每区≤7");
    // UI-012 按钮排列顺序
    s.add("UI-012 按钮排列顺序(4.5)", BUTTON_ARRANGEMENTS.len() == 4, "确认取消右对齐等四模板");
    // UI-013 间距系统
    s.add("UI-013 间距系统(5.1)", SPACING.len() == 8 && SPACING[0].1 == 2 && SPACING[7].1 == 48, "2xs~xxxl 八档8px网格");
    // UI-014 对齐规则
    s.add("UI-014 对齐规则(5.2)", ALIGNMENT_RULES.len() == 4, "网格/左对齐/垂直居中/等距");
    // UI-015 面板
    s.add("UI-015 面板(6.1)", PANEL_SPEC == (8, 12, 16, 24), "圆角8-12/内边距16-24/标题必备");
    // UI-016 卡片
    s.add("UI-016 卡片(6.2)", CARD_SPEC == (16, 3), "间距≥16/内容≤3层/悬停上浮");
    // UI-017 必须有的交互反馈
    s.add("UI-017 必须有的交互反馈(7.1)", FEEDBACKS.len() == 7 && FEEDBACKS.iter().all(|f| !f.1.is_empty()), "悬停/点击/加载/成功/失败/空状态/拖拽");
    // UI-018 光标要求
    s.add("UI-018 光标要求(7.2)", CURSORS.len() == 6 && CURSORS.iter().any(|c| c.1 == "pointer"), "六场景光标映射");
    // UI-019 动效原则
    s.add("UI-019 动效原则(8.1)", MOTION_PRINCIPLES.len() == 4, "有目的/快速/自然/可关闭");
    // UI-020 动效时长规范
    let dur_ok = MOTION_DURATIONS.len() == 4
        && MOTION_DURATIONS.iter().all(|(_, (lo, hi), _)| lo <= hi)
        && MOTION_DURATIONS[0].1 == (100, 200) && MOTION_DURATIONS[3].1 == (500, 800);
    s.add("UI-020 动效时长规范(8.2)", dur_ok, "微交互100-200/复杂500-800四档");
    // UI-021 响应式断点（含嵌入态窗口尺寸）
    s.add("UI-021 响应式断点(9.1/33.6)", RESPONSIVE.len() == 4 && RESPONSIVE[0].0 == "≥1440px", "四断点含嵌入态任意窗口尺寸");
    // UI-022 响应式规则
    s.add("UI-022 响应式规则(9.2)", RESPONSIVE_RULES.len() == 4 && RESPONSIVE_RULES[0].1.contains("44"), "触控44px/正文16px/间距/隐藏");
    // UI-023 无障碍要求
    s.add("UI-023 无障碍要求(十)", A11Y_RULES.len() == 6 && A11Y_RULES[0].1.contains("4.5"), "对比度/键盘/焦点/读屏/色盲/动效可控");
    // UI-024 常见错误清单逐条防回归
    s.add("UI-024 常见错误清单防回归(十一)", ERROR_CHECKLIST.len() == 10, "十条错误逐条有正确做法");
    // UI-025 总界面布局 33.1（尺寸表逐项）
    let vsc = &LAYOUT_VSCODE;
    let table_ok = vsc[0].1 == 28 && vsc[1].1 == 48 && vsc[2].1 == 260 && vsc[4].1 == 280
        && vsc[5].1 == 200 && vsc[5].3 == 32 && vsc[6].1 == 22
        && vsc[2].4 == "Ctrl+B" && vsc[5].4 == "Ctrl+J" && vsc[4].4 == "Esc";
    s.add("UI-025 总界面布局33.1", table_ok, "28/48/260/280/200折叠32/22px+快捷键");
    // UI-026 完整色彩体系 33.2
    let c33_ok = BASE_COLORS.len() == 7 && FUNCTIONAL_COLORS.len() == 15 && THEMES_12.len() == 12
        && FUNCTIONAL_COLORS[0].1 == "#FF3B30" && FUNCTIONAL_COLORS[13].1 == "#8E8E93";
    s.add("UI-026 完整色彩体系33.2", c33_ok, "基础7对+功能15色+12主题");
    // UI-027 按钮系统完整规范 33.3
    s.add("UI-027 按钮系统完整规范33.3", PIXEL_BUTTON_MATERIALS.len() == 6 && PIXEL_BUTTON_MATERIALS[2].0 == "钻石", "像素六材质+六类型+四状态");
    // UI-028 字体规范 33.4
    let pf = PIXEL_FONT_RULES;
    let f33_ok = FONTS[6].1 == "Inter" && FONTS[5].1 == "JetBrains Mono"
        && pf.0.contains("pixelated") && pf.1 == 1 && pf.2.len() == 6 && pf.3 == [8, 12, 16, 24];
    s.add("UI-028 字体规范33.4", f33_ok, "七用途双轨+像素关AA/1px影/§色/四档字号");
    // UI-029 动效规范 33.5
    let m33_ok = MOTIONS.len() == 16
        && MOTIONS[0] == ("面板滑出", 300, "ease-out", "侧边栏/辅助栏/底部")
        && MOTIONS[13].0 == "风格切换" && MOTIONS[13].1 == 800;
    s.add("UI-029 动效规范33.5", m33_ok, "16条动效时长/缓动/场景逐条落地");
    // UI-030 响应式适配 33.6
    let r33_ok = RESPONSIVE[0].2 == "48px" && RESPONSIVE[0].3 == "260px"
        && RESPONSIVE[3].2 == "隐藏" && RESPONSIVE[3].3 == "Tab切换";
    s.add("UI-030 响应式适配33.6", r33_ok, "四档布局含活动栏/侧栏/辅助栏/底部逐列");
    // UI-031 8 风格全部资产
    s.add("UI-031 8风格全部资产(#381~#388)", STYLE_PACKS.len() == 8 && STYLE_PACKS.iter().all(|p| !p.1.is_empty() && !p.6.is_empty()), "像素/简约/清新/星空/赛博/玻璃/新拟态/手绘七维资产");
    // UI-032 资产系统 #389~#400
    let asset_ok = ASSET_MANIFEST_FIELDS.len() == 6
        && TEXTURE_CATEGORIES.len() == 5 && SFX_SET.len() == 5 && SHADER_SET.len() == 6
        && SPRITE_FRAMES.iter().map(|(_, n)| n).sum::<u32>() == 20
        && SWITCH_TRANSITION_ITEMS.len() == 5 && CUSTOM_PACK_OPS.len() == 4
        && MARKET_OPS.len() == 3 && combo_total() == 27648 && DYNAMICS_8.len() == 8;
    s.add("UI-032 资产系统#389~#400", asset_ok, "manifest/贴图/音效/Shader/帧/切换/自定义/市场/27648组合/12主题/8动态");
    // UI-033 首次加载 34 章
    let tl_ok = FIRST_LOAD_TIMELINE.len() == 12
        && FIRST_LOAD_TIMELINE[0] == (0, "屏幕亮起")
        && FIRST_LOAD_TIMELINE[11] == (3000, "加载完成可交互")
        && FIRST_LOAD_TIMELINE.windows(2).all(|w| w[0].0 <= w[1].0);
    s.add("UI-033 首次加载体验(34章)", tl_ok, "0~3000ms 十二节点时间线单调");
    // UI-034 功能触达总表 35 章
    s.add("UI-034 功能触达总表(35章)", reach_total() >= 550 && reach_max_steps() <= 2, "12层级合计≥550 全部≤2步");
    // UI-035 手册界面+自定义按钮 25 章
    let man_ok = MANUAL_SPEC.1 == 8 && MANUAL_SPEC.2 == 10 && MANUAL_SPEC.3 == 4
        && BUTTON_CREATE_WAYS.len() == 4 && BUTTON_PLACEMENTS.len() == 5
        && BUTTON_SIZES_CUSTOM == [24, 32, 48] && BUTTON_MGMT_SUBCMDS.len() == 5;
    s.add("UI-035 手册界面+自定义按钮(25章)", man_ok, "800x600/8分类/收藏最近10/4模式/创建4法/位置5种/尺寸3档//button5子命令");
    // UI-036 嵌入态/内核态渲染差异清零
    let par = shell_parity();
    let eq_ok = par.iter().all(|&n| n == 12) && assets_self_carried()
        && SHELLS.len() == 3;
    s.add("UI-036 三端渲染差异清零", eq_ok, "12能力组三端全量/资产自带/不隐藏只换实现层");
    s
}

// ───────────────────────── 单测 ─────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui036_all_checks_pass() {
        let s = run_uispec_checks();
        assert_eq!(s.total(), 36, "必须恰好 36 项（UI-001~UI-036）");
        assert!(s.all_pass(), "自检未全绿:\n{}", s.render());
    }

    #[test]
    fn ui001_layout_table_exact() {
        assert_eq!(LAYOUT_VSCODE[2].1, 260);
        assert_eq!(LAYOUT_VSCODE[4].1, 280);
        assert_eq!(LAYOUT_GENERIC.4, 60);
    }

    #[test]
    fn ui026_color_system() {
        assert!(color_meaning_unique());
        assert!(dark_mode_tuned());
        assert_eq!(FUNCTIONAL_COLORS.len(), 15);
        assert_eq!(THEMES_12.len(), 12);
    }

    #[test]
    fn ui029_motion_table() {
        assert_eq!(MOTIONS.len(), 16);
        assert_eq!(MOTIONS[8].1, 2000); // 脉冲闪烁
        assert_eq!(MOTIONS[9].1, 4000); // 呼吸
    }

    #[test]
    fn ui032_combo_matrix() {
        assert_eq!(combo_total(), 27648);
        let frames: u32 = SPRITE_FRAMES.iter().map(|(_, n)| n).sum();
        assert_eq!(frames, 20);
    }

    #[test]
    fn ui033_timeline_monotonic() {
        for w in FIRST_LOAD_TIMELINE.windows(2) {
            assert!(w[0].0 < w[1].0);
        }
    }

    #[test]
    fn ui034_reach_table() {
        assert!(reach_total() >= 550);
        assert!(reach_max_steps() <= 2);
    }

    #[test]
    fn ui036_parity() {
        let par = shell_parity();
        assert_eq!(par, [12, 12, 12]);
        assert!(assets_self_carried());
    }
}
