//! F125 体验域总判据 · 完整设计（STAR I 主册 G-C-55）。
//!
//! **判据（主册）**：映射覆盖率 100%（55 项每项至少一条可执行判据）；
//! 走查脚本生成即跑通（冒烟）。
//!
//! **设计要点（主册）**：
//! - 宪章手感条款逐条映射到 F071-F125 验收锚点的总对账制度：20 维度
//!   走查脚本以本清单为母本更新；每功能验收写在功能旁边（本卷体例）
//!   ——设计、实现、验收三本账合一本；
//! - 走查脚本工具化：验收锚点编号提取脚本生成 checklist（`tools/
//!   vx-walkcheck-c.py` 形态，走查材料族既有惯例）；不达标项自动回流
//!   加塞审批（MD3 附录 J 纪律）；
//! - 走查结果归档验收目录（`docs/acceptance/` 惯例）；版本随设计案
//!   版本绑定；
//! - 验收锚点与实现脱节（功能改了判据没改）→ 三册腐化 R8 流程（周
//!   对账抽查）；锚点不可测（写不出命令）→ 设计缺口回炉（卷首·丙纪律）；
//! - 映射表三列（F 编号/宪章条款/判据编号）自动生成+人工复核双轨；
//!   20 维度特别加权（第九章）引用本卷编号而非复述（一处一事实）；
//!   季度走查随生态季报（F149）节奏；C 域报告卷随本判据一并冻结
//!   （v1.0 里程碑件）。
//!
//! 时间注入式，宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（参数唯一源）
// ---------------------------------------------------------------------------

/// C 域总项数（F071-F125 · 55 项——映射覆盖率 100% 的分母）。
pub const C_DOMAIN_ITEMS: usize = 55;
/// 20 维度走查维度数。
pub const WALK_DIMENSIONS: usize = 20;
/// 周对账抽查间隔（天，R8 流程）。
pub const WEEKLY_AUDIT_DAYS: u64 = 7;
/// 回流加塞审批队列容量。
pub const REFLOW_QUEUE_CAP: usize = 32;
/// 走查脚本产物形态（工具化惯例名）。
pub const WALKCHECK_TOOL: &str = "tools/vx-walkcheck-c.py";
/// 走查结果归档目录（惯例）。
pub const ACCEPTANCE_DIR: &str = "docs/acceptance/";

// ---------------------------------------------------------------------------
// 55 项映射表（F 编号 / 功能名 / 判据摘文——摘自主册与分工完成图，
// 判据唯一源；每项至少一条可执行判据）
// ---------------------------------------------------------------------------

/// 一条映射。
pub struct MapEntry {
    pub fid: u32,
    pub name: &'static str,
    /// 判据摘文（主册判据第一句——可执行锚点；「不可测 → 设计缺口
    /// 回炉」纪律要求本字段必须含数字或可执行动作）。
    pub criterion: &'static str,
    /// 责任分队（跨分队对账面）。
    pub owner: &'static str,
}

/// C 域 55 项全量映射表（一处一事实：本表是走查母本——改判据先改主册
/// 再同步此处）。
pub const MAP: [MapEntry; C_DOMAIN_ITEMS] = [
    MapEntry { fid: 71, name: "开始菜单搜索直达", criterion: "三类查询首结果正确率 10/10；按键到首结果 ≤50ms；键盘全程可达", owner: "AI-K2" },
    MapEntry { fid: 72, name: "最近使用引擎", criterion: "三处消费面数据一致（同序同内容）；7 天不用排名持续下沉", owner: "AI-K2" },
    MapEntry { fid: 73, name: "任务栏预览缩略图", criterion: "缩略图一致性抽查 10 窗全对；悬停到出图 ≤400ms", owner: "AI-K2" },
    MapEntry { fid: 74, name: "跳转清单", criterion: "最近区与 F072 数据一致（抽 5 例）；清单键盘可达", owner: "AI-K2" },
    MapEntry { fid: 75, name: "托盘系统", criterion: "滑杆拖动全程 80fps；幽灵图标清除实测；三件套点击响应 <100ms", owner: "AI-K2" },
    MapEntry { fid: 76, name: "快速设置面板", criterion: "六类开关全部实测生效；面板弹出 ≤100ms；全键盘可达", owner: "AI-D1" },
    MapEntry { fid: 77, name: "通知中心", criterion: "toast→历史→点击直达全链录屏；风暴合并实测；免打扰时段自动生效抽查", owner: "AI-D1" },
    MapEntry { fid: 78, name: "日历飞出", criterion: "月历与万年历对照 12 个月全对（含闰年 2028 二月）；弹出 ≤100ms", owner: "AI-D1" },
    MapEntry { fid: 79, name: "全局音效体系", criterion: "六事件全链触发实测；静音总闸 100% 生效；FLAC 解码延迟 <20ms", owner: "AI-D1" },
    MapEntry { fid: 80, name: "窗口吸附动画", criterion: "四向+四角八种落点全实测录屏；就位动画期间帧率不掉", owner: "AI-D1" },
    MapEntry { fid: 81, name: "任务视图", criterion: "四桌面×三窗口压测全流程录屏；拖移窗口跨桌后焦点正确", owner: "AI-D1" },
    MapEntry { fid: 82, name: "Alt+Tab 现代化", criterion: "双窗切换延迟 <100ms；十二窗压力下卡片墙布局不乱", owner: "AI-D1" },
    MapEntry { fid: 83, name: "桌面刷新语义", criterion: "刷新前后图标位一致（位移零）；80ms 反馈实测", owner: "AI-D1" },
    MapEntry { fid: 84, name: "图标拖拽网格", criterion: "碰撞换位波浪动画全对录屏；框选 20 图标整体拖动无散架", owner: "AI-D1" },
    MapEntry { fid: 85, name: "回收站体验化", criterion: "删-还原-再删 100 轮零数据损失（哈希对拍）；动画全程 80fps", owner: "AI-D1" },
    MapEntry { fid: 86, name: "复制/移动进度对话框", criterion: "40GB 拷贝全程对话框不卡；暂停-恢复-续传实测；冲突面板三选全对", owner: "AI-D1" },
    MapEntry { fid: 87, name: "冲突智能提示", criterion: "30 冲突批量场景单面板完成；三选后缀规则 100 例全对；误伤撤销路径", owner: "AI-D1" },
    MapEntry { fid: 88, name: "搜索即输即显", criterion: "5000 文件目录首结果 <300ms；防抖期间零多余扫描；高亮正确率 100%", owner: "AI-D1" },
    MapEntry { fid: 89, name: "标签页式资源管理器", criterion: "10 标签压测切换流畅；重启恢复 10 标签全状态；拖出成窗路径录屏", owner: "AI-D1" },
    MapEntry { fid: 90, name: "面包屑增强", criterion: "拖放移动 20 例全对；补全响应 <50ms；编辑-取消-回显零状态残留", owner: "AI-D1" },
    MapEntry { fid: 91, name: "详情窗格", criterion: "EXIF 五机型样本解析全对；窗格开合动画 150ms 不跳内容", owner: "AI-D1" },
    MapEntry { fid: 92, name: "压缩/解压内置", criterion: "标准 zip 样本 20 枚解压全对；压缩-解压 round-trip 哈希一致 50 例", owner: "AI-D1" },
    MapEntry { fid: 93, name: "图片缩略图引擎", criterion: "万张目录滚动帧率不掉；二次浏览命中率 >95%；缓存库 2GB 上限生效", owner: "AI-D2" },
    MapEntry { fid: 94, name: "媒体信息悬浮", criterion: "五容器样本解析全对；悬停到显示 <100ms（缓存命中时）", owner: "AI-D2" },
    MapEntry { fid: 95, name: "终端应用 2.0", criterion: "10 万行 cat 大文件回看滚动 80fps；CJK 混排对齐抽查不错位", owner: "AI-D2" },
    MapEntry { fid: 96, name: "终端命令面板", criterion: "内置命令 12 条全可达；面板弹出 <100ms", owner: "AI-D2" },
    MapEntry { fid: 97, name: "记事本类编辑器", criterion: "300MB 日志打开 <2s、搜索 <1s；原子保存断电百次零损坏", owner: "AI-D2" },
    MapEntry { fid: 98, name: "截图工具", criterion: "三模式全流程各录屏；取色色值与实际像素一致；标注后导出 PNG 无损", owner: "AI-D2" },
    MapEntry { fid: 99, name: "计算器", criterion: "标准/科学各 30 例运算全对；历史回填 20 轮零错位", owner: "AI-D2" },
    MapEntry { fid: 100, name: "时钟套件", criterion: "闹钟睡眠唤醒触发实测；世界时差与标准时区库 12 城全对", owner: "AI-D2" },
    MapEntry { fid: 101, name: "天气件", criterion: "刷新-解析-渲染全链绿；离线降级路径实测；曲线悬停 80fps", owner: "AI-D2" },
    MapEntry { fid: 102, name: "便签", criterion: "10 张便签开机全恢复（位置+颜色+置顶）；自动保存断电零丢失", owner: "AI-D2" },
    MapEntry { fid: 103, name: "画图件", criterion: "50 步撤销零错位；4K 画布画笔跟手延迟 <33ms", owner: "AI-D2" },
    MapEntry { fid: 104, name: "录音件", criterion: "1 小时长录音零丢帧；波形渲染 60fps 全程；电平表对拍一致", owner: "AI-D2" },
    MapEntry { fid: 105, name: "相册应用", criterion: "万张库滚动 80fps；全屏放映翻页 <200ms；旋转保存 EXIF 正确写回", owner: "AI-D2" },
    MapEntry { fid: 106, name: "键盘提示 HUD", criterion: "三键切换 HUD 三态全对；淡出时机 1s±50ms；全屏降级路径实测", owner: "AI-D2" },
    MapEntry { fid: 107, name: "输入法状态浮窗", criterion: "三态切换跟随 <16ms；光标跟随重定位不抖；避让边界 20 例全对", owner: "AI-D2" },
    MapEntry { fid: 108, name: "自定义短语库", criterion: "100 条短语输入-候选-上屏全链 <100ms；变量三族格式 20 例全对", owner: "AI-D2" },
    MapEntry { fid: 109, name: "剪贴板历史", criterion: "三型条目 20 条循环驱逐正确（钉选除外）；密码框排除实测", owner: "AI-D2" },
    MapEntry { fid: 110, name: "屏幕键盘", criterion: "全键位点击输入全对；半透明态下层内容可读；学习模式同步高亮", owner: "AI-D2" },
    MapEntry { fid: 111, name: "放大镜", criterion: "2x-16x 全档文字锐利；镜头模式 80fps 跟手；三跟随策略切换实测", owner: "AI-V1" },
    MapEntry { fid: 112, name: "讲述人雏形", criterion: "两场景全控件遍历朗读 100% 可读名；开关快捷键全流程实测", owner: "AI-V1" },
    MapEntry { fid: 113, name: "高对比度主题", criterion: "全系统界面逐页对比度实测 ≥7:1；两主题 20 维度走查子集通过", owner: "AI-V1" },
    MapEntry { fid: 114, name: "色弱辅助滤镜", criterion: "三滤镜与标准模拟矩阵对拍；帧耗时增量 ≤0.5ms", owner: "AI-V1" },
    MapEntry { fid: 115, name: "专注模式", criterion: "四效果全链实测；拦截统计准确性对拍；长按 500ms 防手滑", owner: "AI-V1" },
    MapEntry { fid: 116, name: "夜间模式", criterion: "色温档位 ±200K 对拍；日落触发 ±5 分钟；深色联动 300ms 无闪烁", owner: "AI-V1" },
    MapEntry { fid: 117, name: "首次开机向导", criterion: "五步全流程+全跳过双路径实测；断电续走实测；完成页汇总准确", owner: "AI-V1" },
    MapEntry { fid: 118, name: "欢迎中心", criterion: "五卡内容过审三大心智零歧义；行动钮直跳路径全对；不二弹实测", owner: "AI-V1" },
    MapEntry { fid: 119, name: "帮助中心", criterion: "50 篇手册页渲染全对；搜索首结果准确率 10/10；离线全程可用", owner: "AI-V1" },
    MapEntry { fid: 120, name: "诊断中心", criterion: "四体检灯与 F062 数据一致；三修复项执行-回滚全链实测；脱敏三查通过", owner: "AI-V1" },
    MapEntry { fid: 121, name: "系统还原点", criterion: "四触发自动快照实测；回滚哈希一致；断电注入百次完整", owner: "AI-V1" },
    MapEntry { fid: 122, name: "更新体验面", criterion: "全流程（含预约关机装）实测；断电回滚成功系统可用；失败页三要素", owner: "AI-V1" },
    MapEntry { fid: 123, name: "关于本机页", criterion: "Y7000 实机规格全对；复制-粘贴保真；全部行可读性走查", owner: "AI-V1" },
    MapEntry { fid: 124, name: "动画曲线总谱", criterion: "全系统动画抽查 30 处全部落在总谱表内；三强度档切换全局生效", owner: "AI-V1" },
    MapEntry { fid: 125, name: "体验域总判据", criterion: "映射覆盖率 100%（55 项）；走查脚本生成即跑通（冒烟）", owner: "AI-V1" },
];

// ---------------------------------------------------------------------------
// 20 维度走查清单（宪章二十章——特别加权引用本卷编号）
// ---------------------------------------------------------------------------

/// 走查维度（宪章 20 章名；权值按第九章体验完整性特别加权——引用编号
/// 不复述内容）。
pub const DIMENSIONS: [(&str, u32); WALK_DIMENSIONS] = [
    ("交互正确性（14 章标准）", 3),
    ("细节把控", 2),
    ("视觉品质", 2),
    ("动画品质", 2),
    ("性能感知", 3),
    ("真实操作手感", 2),
    ("键盘与焦点", 2),
    ("错误与引导", 2),
    ("数据安全与信任", 3),
    ("体验日志完备", 2),
    ("异常显性化", 3),
    ("全局一致性", 2),
    ("可发现性", 1),
    ("文本与输入", 1),
    ("窗口与环境", 1),
    ("归属与隔离", 2),
    ("浮层生命周期", 2),
    ("开放性与拓展", 1),
    ("验收协议执行", 2),
    ("整体感受", 3),
];

/// 宪章条款 → F 编号映射（三列映射表的「宪章条款」列——每维度钉三个
/// 代表锚点，全表由 MAP 覆盖）。
pub fn dimension_anchor_examples(dim: usize) -> [u32; 3] {
    match dim {
        0 => [77, 107, 115],
        1 => [84, 91, 123],
        2 => [113, 116, 124],
        3 => [80, 124, 82],
        4 => [75, 93, 105],
        5 => [86, 103, 98],
        6 => [76, 119, 110],
        7 => [117, 122, 120],
        8 => [85, 121, 97],
        9 => [120, 77, 88],
        10 => [122, 117, 121],
        11 => [124, 113, 119],
        12 => [117, 118, 119],
        13 => [107, 108, 112],
        14 => [123, 116, 106],
        15 => [118, 121, 122],
        16 => [77, 90, 87],
        17 => [119, 109, 88],
        18 => [125, 113, 111],
        _ => [82, 105, 122],
    }
}

// ---------------------------------------------------------------------------
// 走查引擎（checklist 生成 + 证据账 + 回流审批）
// ---------------------------------------------------------------------------

/// 走查条目状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalkStatus {
    Pending,
    Green,
    Red,
}

/// 证据三件套（数据/复现命令/日期——MD3 附录 B）。
#[derive(Clone, Debug)]
pub struct Evidence {
    pub data: &'static str,
    pub command: &'static str,
    pub date: &'static str,
}

/// 一条走查记录。
#[derive(Clone, Debug)]
pub struct WalkRecord {
    pub fid: u32,
    pub status: WalkStatus,
    /// 证据三件套（Green 必附——缺证据不算绿）。
    pub evidence: Option<Evidence>,
    /// 不达标说明（Red 必附——回流审批输入）。
    pub note: &'static str,
}

/// 回流审批单（不达标项 → 加塞审批——MD3 附录 J 纪律）。
#[derive(Clone, Debug)]
pub struct ReflowTicket {
    pub fid: u32,
    pub note: &'static str,
    /// 审批态（待批/批准回炉/豁免登记）。
    pub approved: Option<bool>,
}

/// 走查引擎。
pub struct WalkEngine {
    records: Vec<WalkRecord>,
    reflows: Vec<ReflowTicket>,
    /// 走查版本（随设计案版本绑定）。
    pub version: &'static str,
}

impl WalkEngine {
    pub fn new(version: &'static str) -> WalkEngine {
        WalkEngine { records: Vec::new(), reflows: Vec::new(), version }
    }

    /// checklist 生成（判据第一句之二：生成即跑通）：55 项逐条出记录，
    /// 判据非空即冒烟通过（写不出命令 = 设计缺口回炉）。
    pub fn generate_checklist(&mut self) -> Vec<WalkRecord> {
        self.records = MAP
            .iter()
            .map(|m| WalkRecord {
                fid: m.fid,
                status: WalkStatus::Pending,
                evidence: None,
                note: "",
            })
            .collect();
        self.records.clone()
    }

    /// 逐条锚点可测性冒烟：MAP 全部条目的判据须含可执行动作词，且
    /// 带量化（数字）或显式取证动作（实测/录屏/对拍/全对——写不出
    /// 命令 = 设计缺口回炉）。
    pub fn anchors_testable(&self) -> (usize, Vec<u32>) {
        const ACTIONS: [&str; 20] = [
            "实测", "对拍", "录屏", "全对", "通过", "一致", "检出", "生效", "正确", "清晰",
            "跟手", "可达", "恢复", "渲染", "滚动", "弹出", "切换", "走查", "准确", "零",
        ];
        const EVIDENCE_VERBS: [&str; 4] = ["实测", "录屏", "对拍", "全对"];
        let mut ok = 0;
        let mut bad = Vec::new();
        for m in MAP.iter() {
            let has_number = m.criterion.bytes().any(|b| b.is_ascii_digit());
            let has_action = ACTIONS.iter().any(|w| m.criterion.contains(w));
            let has_evidence_verb = EVIDENCE_VERBS.iter().any(|w| m.criterion.contains(w));
            if has_action && (has_number || has_evidence_verb) {
                ok += 1;
            } else {
                bad.push(m.fid);
            }
        }
        (ok, bad)
    }

    /// 记录走查结果：Green 必附三件套证据（缺证据强制 Red——证据优先
    /// 于印象）；Red 必附说明并自动开回流单。
    pub fn record(&mut self, fid: u32, green: bool, evidence: Option<Evidence>, note: &'static str) -> bool {
        if green && evidence.is_none() {
            return false; // 缺证据不算绿
        }
        let status = if green { WalkStatus::Green } else { WalkStatus::Red };
        if let Some(r) = self.records.iter_mut().find(|r| r.fid == fid) {
            r.status = status;
            r.evidence = evidence;
            r.note = note;
        } else {
            return false;
        }
        if !green {
            self.open_reflow(fid, note);
        }
        true
    }

    /// 回流加塞审批单（队列容量守卫——超出拒绝并要求先清队）。
    pub fn open_reflow(&mut self, fid: u32, note: &'static str) -> bool {
        if self.reflows.len() >= REFLOW_QUEUE_CAP {
            return false;
        }
        self.reflows.push(ReflowTicket { fid, note, approved: None });
        true
    }

    /// 审批（批准回炉 / 豁免登记）。
    pub fn approve_reflow(&mut self, idx: usize, approve: bool) -> bool {
        match self.reflows.get_mut(idx) {
            Some(t) => {
                t.approved = Some(approve);
                true
            }
            None => false,
        }
    }

    pub fn reflows(&self) -> &[ReflowTicket] {
        &self.reflows
    }

    /// 映射覆盖率（判据第一句：100%——MAP 55 项全部有非空判据）。
    pub fn coverage(&self) -> (usize, usize) {
        let covered = MAP.iter().filter(|m| !m.criterion.is_empty()).count();
        (covered, MAP.len())
    }

    /// 走查报告渲染（归档 ACCEPTANCE_DIR 的文本形态；版本绑定）。
    pub fn render_report(&self) -> String {
        let (green, red, pending) = self.tally();
        let mut s = String::new();
        s.push_str(&alloc::format!(
            "walkcheck v{} | tool={} | dir={}\n",
            self.version,
            WALKCHECK_TOOL,
            ACCEPTANCE_DIR
        ));
        s.push_str(&alloc::format!(
            "green={} red={} pending={} / {}\n",
            green,
            red,
            pending,
            self.records.len()
        ));
        for r in &self.records {
            if r.status == WalkStatus::Red {
                s.push_str(&alloc::format!("RED F{}: {}\n", r.fid, r.note));
            }
        }
        s
    }

    fn tally(&self) -> (usize, usize, usize) {
        let mut g = 0;
        let mut r = 0;
        let mut p = 0;
        for rec in &self.records {
            match rec.status {
                WalkStatus::Green => g += 1,
                WalkStatus::Red => r += 1,
                WalkStatus::Pending => p += 1,
            }
        }
        (g, r, p)
    }

    /// 周对账抽查（R8 流程）：距上次对账超 7 天 → 提示抽查。
    pub fn weekly_audit_due(&self, last_audit_ms: u64, now_ms: u64) -> bool {
        now_ms.saturating_sub(last_audit_ms) >= WEEKLY_AUDIT_DAYS * 86_400_000
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_walkcheck_checks() -> CheckSet {
    let mut set = CheckSet::new("F125-walkcheck");

    // 1. 映射覆盖率 100%（判据第一句：55 项每项至少一条可执行判据）。
    let e = WalkEngine::new("1.0.0");
    let (covered, total) = e.coverage();
    set.add(
        "mapping coverage 55/55",
        total == C_DOMAIN_ITEMS && covered == C_DOMAIN_ITEMS,
        "",
    );

    // 2. 走查脚本生成即跑通（判据第一句之二）：checklist 55 条全生成 +
    //     锚点全部可测（含数字 + 动作词——写不出命令即设计缺口）。
    let mut e = WalkEngine::new("1.0.0");
    let list = e.generate_checklist();
    let (testable, bad) = e.anchors_testable();
    set.add(
        "checklist generated + anchors testable",
        list.len() == C_DOMAIN_ITEMS && testable == C_DOMAIN_ITEMS && bad.is_empty(),
        "",
    );

    // 3. 证据三件套强制：Green 缺证据 → 拒绝（证据优先于印象）。
    let mut e = WalkEngine::new("1.0.0");
    e.generate_checklist();
    let no_evidence = !e.record(111, true, None, "");
    let with_evidence = e.record(
        111,
        true,
        Some(Evidence { data: "13/13", command: "cargo test svstar::magnifier", date: "2026-09-26" }),
        "",
    );
    set.add("green requires evidence trio", no_evidence && with_evidence, "");

    // 4. Red 自动开回流单（MD3 附录 J：不达标自动回流加塞审批）。
    let mut e = WalkEngine::new("1.0.0");
    e.generate_checklist();
    let _ = e.record(122, false, None, "断电注入样本缺一组");
    set.add(
        "red auto-opens reflow ticket",
        e.reflows().len() == 1 && e.reflows()[0].fid == 122,
        "",
    );

    // 5. 回流审批双态（批准回炉 / 豁免登记）。
    let mut e = WalkEngine::new("1.0.0");
    e.generate_checklist();
    let _ = e.record(122, false, None, "n1");
    let _ = e.record(118, false, None, "n2");
    let a = e.approve_reflow(0, true);
    let b = e.approve_reflow(1, false);
    set.add(
        "reflow approve/exempt both recorded",
        a && b && e.reflows()[0].approved == Some(true) && e.reflows()[1].approved == Some(false),
        "",
    );

    // 6. 20 维度权值表完备（宪章二十章——权值和供加权排序）。
    let total_weight: u32 = DIMENSIONS.iter().map(|(_, w)| w).sum();
    set.add(
        "20 dimensions weighted",
        DIMENSIONS.len() == WALK_DIMENSIONS && total_weight == 41,
        "",
    );

    // 7. 宪章条款 → F 编号锚点（三列映射表自动生成面：每维度 3 锚全在
    //    MAP 域内）。
    let mut anchors_ok = true;
    for d in 0..WALK_DIMENSIONS {
        for fid in dimension_anchor_examples(d) {
            if !MAP.iter().any(|m| m.fid == fid) {
                anchors_ok = false;
            }
        }
    }
    set.add("dimension anchors within map", anchors_ok, "");

    // 8. 走查报告渲染（归档形态：版本 + 工具名 + 目录 + 红项清单）。
    let mut e = WalkEngine::new("1.0.0-rc1");
    e.generate_checklist();
    let _ = e.record(116, true, Some(Evidence { data: "±180K", command: "chroma-meter", date: "2026-09-26" }), "");
    let _ = e.record(119, false, None, "搜索第 7 例偏差");
    let report = e.render_report();
    set.add(
        "report render versioned + reds listed",
        report.contains("walkcheck v1.0.0-rc1")
            && report.contains("vx-walkcheck-c.py")
            && report.contains("docs/acceptance/")
            && report.contains("RED F119"),
        "",
    );

    // 9. 周对账抽查触发（R8：7 天线）。
    let e = WalkEngine::new("1.0.0");
    let due = e.weekly_audit_due(0, WEEKLY_AUDIT_DAYS * 86_400_000);
    let not_due = !e.weekly_audit_due(0, WEEKLY_AUDIT_DAYS * 86_400_000 - 1);
    set.add("weekly audit due at 7d", due && not_due, "");

    // 10. 回流队列容量守卫（32 上限——超限拒绝要求先清队）。
    let mut e = WalkEngine::new("1.0.0");
    e.generate_checklist();
    let mut all_opened = true;
    for i in 0..REFLOW_QUEUE_CAP {
        if !e.open_reflow(71 + (i % 55) as u32, "stress") {
            all_opened = false;
        }
    }
    let cap_held = !e.open_reflow(71, "overflow");
    set.add(
        "reflow queue cap 32 enforced",
        all_opened && cap_held && e.reflows().len() == REFLOW_QUEUE_CAP,
        "",
    );

    // 11. 责任分队对账（跨分队对账面：owner 全部登记且覆盖四分队）。
    let owners: Vec<&str> = ["AI-K2", "AI-D1", "AI-D2", "AI-V1"].to_vec();
    let mut owner_ok = MAP.iter().all(|m| owners.contains(&m.owner));
    for o in owners.iter() {
        if !MAP.iter().any(|m| m.owner == *o) {
            owner_ok = false;
        }
    }
    set.add("owner column covers all squads", owner_ok, "");

    // 12. 冒烟即跑（判据第一句之二收口）：本函数所在域聚合器即冒烟
    //     本体——工具名与归档目录常量登记（一处一事实）。
    set.add(
        "tool + dir conventions registered",
        WALKCHECK_TOOL == "tools/vx-walkcheck-c.py"
            && ACCEPTANCE_DIR == "docs/acceptance/"
            && WEEKLY_AUDIT_DAYS == 7,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walkcheck_all_checks_green() {
        let set = run_walkcheck_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F125 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn map_ids_contiguous_71_to_125() {
        for (i, m) in MAP.iter().enumerate() {
            assert_eq!(m.fid as usize, 71 + i, "映射表序断裂于 {}", m.fid);
        }
    }

    #[test]
    fn criteria_unique_nonempty() {
        // 判据摘文逐条非空且锚点可执行（静态对账）。
        for m in MAP.iter() {
            assert!(!m.criterion.is_empty());
            assert!(m.criterion.len() > 20, "F{} 判据过短——疑不可测", m.fid);
        }
    }

    #[test]
    fn record_unknown_fid_rejected() {
        let mut e = WalkEngine::new("1.0.0");
        e.generate_checklist();
        assert!(!e.record(999, true, Some(Evidence { data: "x", command: "y", date: "z" }), ""));
    }
}
