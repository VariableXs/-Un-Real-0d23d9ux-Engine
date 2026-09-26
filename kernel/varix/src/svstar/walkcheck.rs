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

use crate::svstar::vbase;
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
// 深化批次 v2 · 一：判据锚号与三列映射表（自动生成 + 人工复核双轨）
// ---------------------------------------------------------------------------

/// 判据锚号生成（F 编号 → 主册 G-C-xx 规范锚）。
///
/// 一处一事实：锚号规则唯一源。主册 C-9 段「深化设计报告（F071-F125）
/// 中 G-C-41 = F111、G-C-55 = F125，即锚号 = G-C-(fid-70)（两位补零）。
/// 三列映射表的「判据编号」列由此自动生成——改主册锚号先改本函数。
pub fn criterion_anchor(fid: u32) -> String {
    debug_assert!((71..=125).contains(&fid), "锚号规则只覆盖 C 域 F071-F125");
    alloc::format!("G-C-{:02}", fid - 70)
}

/// 判据锚号往返校验（F111 → G-C-41、F125 → G-C-55——主册两处已知锚点）。
pub fn criterion_anchor_known_pairs() -> [(u32, &'static str); 2] {
    [(111, "G-C-41"), (125, "G-C-55")]
}

/// 三列映射表行（F 编号 / 宪章条款 / 判据编号——主册 G-C-55 设计细节
/// 「映射表三列」）。
///
/// - `charter`：宪章条款名（20 维度名，引用 DIMENSIONS 不复述内容——
///   特别加权「引用本卷编号」纪律的列表达）；
/// - `anchor`：判据编号（criterion_anchor 自动生成）；
/// - `reviewed`：人工复核标记（双轨制的「人工」半边——自动生成后必须
///   逐行复核才算走查母本就绪）。
#[derive(Clone, Debug)]
pub struct ColumnRow {
    pub fid: u32,
    pub charter: &'static str,
    pub anchor: String,
    pub reviewed: bool,
}

/// 三列映射表自动生成：逐 MAP 条目按 F→维度归属表展开宪章条款列，
/// 判据编号列由 criterion_anchor 生成。每 F 至少一行（覆盖率 100% 的
/// 结构对账面），reviewed 初始为 false 待人工复核。
pub fn auto_columns() -> Vec<ColumnRow> {
    let mut rows = Vec::new();
    for m in MAP.iter() {
        let dims = dimensions_of(m.fid);
        if dims.is_empty() {
            // 无维度归属 = 三册腐化信号——结构上不可能（归属表自检兜底），
            // 兜底行挂「验收协议执行」并拒绝复核位。
            rows.push(ColumnRow {
                fid: m.fid,
                charter: DIMENSIONS[18].0,
                anchor: criterion_anchor(m.fid),
                reviewed: false,
            });
            continue;
        }
        for &d in dims {
            rows.push(ColumnRow {
                fid: m.fid,
                charter: DIMENSIONS[d].0,
                anchor: criterion_anchor(m.fid),
                reviewed: false,
            });
        }
    }
    rows
}

/// 三列映射表人工复核（双轨制第二轨：逐行盖章；返回盖章后待复数）。
pub fn review_columns(rows: &mut [ColumnRow], reviewed_fids: &[u32]) -> usize {
    for r in rows.iter_mut() {
        if reviewed_fids.contains(&r.fid) {
            r.reviewed = true;
        }
    }
    rows.iter().filter(|r| !r.reviewed).count()
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 二：F→维度全量归属表（20 维度走查的全量锚点面）
// ---------------------------------------------------------------------------

/// F→20 维度归属表（每 F 至少一维度；20 维度每维度至少一锚）。
///
/// 归属依据 = 主册各功能【验收判据】与【交互设计】段落落点（一处一
/// 事实：本表是「宪章条款 → F 编号」三列映射的展开源；改归属先改
/// 主册判据再同步此处）。索引 = DIMENSIONS 下标。
fn dims_of_entry(fid: u32) -> &'static [usize] {
    match fid {
        // F071-F075 任务栏族：交互正确性/性能/手感
        71 => &[0, 4, 6],
        72 => &[4, 11],
        73 => &[2, 4],
        74 => &[6, 11],
        75 => &[4, 5],
        // F076-F082 快速面板与窗口族
        76 => &[6, 12],
        77 => &[0, 16, 9],
        78 => &[3, 14],
        79 => &[3, 5],
        80 => &[3, 4],
        81 => &[0, 4],
        82 => &[0, 4, 15],
        // F083-F088 桌面操作族
        83 => &[1, 5],
        84 => &[1, 5],
        85 => &[8, 4],
        86 => &[0, 8],
        87 => &[7, 0],
        88 => &[4, 12],
        // F089-F092 资源管理器族
        89 => &[0, 14],
        90 => &[0, 13],
        91 => &[1, 2],
        92 => &[8],
        // F093-F096 媒体与终端族
        93 => &[4, 2],
        94 => &[1, 5],
        95 => &[13, 4],
        96 => &[6, 12],
        // F097-F102 应用族
        97 => &[8, 4],
        98 => &[5, 2],
        99 => &[1, 13],
        100 => &[8, 14],
        101 => &[7, 4],
        102 => &[8, 12],
        // F103-F106 创作族
        103 => &[5, 4],
        104 => &[4, 3],
        105 => &[4, 2],
        106 => &[14, 0],
        // F107-F110 输入族
        107 => &[13, 0],
        108 => &[13, 1],
        109 => &[8, 10],
        110 => &[6, 17],
        // F111-F115 辅助族（V1 域前半）
        111 => &[14, 4, 6],
        112 => &[6, 7],
        113 => &[2, 18],
        114 => &[2, 10],
        115 => &[0, 10],
        // F116-F120 服务族（V1 域中段）
        116 => &[14, 2],
        117 => &[12, 7],
        118 => &[12, 11],
        119 => &[7, 17],
        120 => &[10, 9, 7],
        // F121-F125 兜底族（V1 域后段）
        121 => &[8, 10],
        122 => &[10, 7],
        123 => &[14, 1],
        124 => &[3, 11],
        _ => &[18, 19], // F125 总判据自身：验收协议 + 整体感受
    }
}

/// 公开归属查询（切片常量化——调用方零拷贝）。
pub fn dimensions_of(fid: u32) -> &'static [usize] {
    dims_of_entry(fid)
}

/// 维度全量锚点展开（某维度下全部归属 F——三列映射与维度评分共用）。
pub fn dimension_anchors_full(dim: usize) -> Vec<u32> {
    let mut v = Vec::new();
    for m in MAP.iter() {
        if dimensions_of(m.fid).contains(&dim) {
            v.push(m.fid);
        }
    }
    v
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 三：回流单完整状态机（MD3 附录 J 纪律的闭环）
// ---------------------------------------------------------------------------

/// 回流单阶段（开单 → 审批 → 回炉 → 复测 → 关闭；驳回/撤回双出线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReflowStage {
    /// 已提交待批。
    Submitted,
    /// 批准进入回炉队列。
    Approved,
    /// 驳回（豁免登记或证据补齐）。
    Denied,
    /// 回炉施工中。
    InBacklog,
    /// 复测通过关闭。
    Closed,
    /// 提交者撤回。
    Withdrawn,
}

impl ReflowStage {
    pub fn tag(self) -> &'static str {
        match self {
            ReflowStage::Submitted => "submitted",
            ReflowStage::Approved => "approved",
            ReflowStage::Denied => "denied",
            ReflowStage::InBacklog => "in-backlog",
            ReflowStage::Closed => "closed",
            ReflowStage::Withdrawn => "withdrawn",
        }
    }

    /// 终态判定（终态单不再流转——台账冻结）。
    pub fn terminal(self) -> bool {
        matches!(self, ReflowStage::Closed | ReflowStage::Denied | ReflowStage::Withdrawn)
    }
}

/// 回流完整单据（审计字段齐：开单人/审批人/双时间戳）。
#[derive(Clone, Debug)]
pub struct ReflowFull {
    pub fid: u32,
    pub note: &'static str,
    pub stage: ReflowStage,
    pub opened_ms: u64,
    pub decided_ms: Option<u64>,
    pub reviewer: &'static str,
}

/// 回流审批台（MD3 附录 J：不达标项自动回流加塞审批——完整闭环）。
pub struct ReflowDesk {
    queue: Vec<ReflowFull>,
}

impl ReflowDesk {
    pub fn new() -> ReflowDesk {
        ReflowDesk { queue: Vec::new() }
    }

    /// 开单（自动入口：walkcheck Red 即开单——stage=Submitted）。
    pub fn open(&mut self, fid: u32, note: &'static str, now_ms: u64) -> Option<usize> {
        if self.queue.len() >= REFLOW_QUEUE_CAP {
            return None; // 容量守卫同 v1
        }
        self.queue.push(ReflowFull {
            fid,
            note,
            stage: ReflowStage::Submitted,
            opened_ms: now_ms,
            decided_ms: None,
            reviewer: "",
        });
        Some(self.queue.len() - 1)
    }

    /// 审批：Submitted → Approved / Denied（审批人 + 时间戳强制——谁批的
    /// 何时批的永久留痕）。
    pub fn decide(&mut self, idx: usize, approve: bool, reviewer: &'static str, now_ms: u64) -> bool {
        match self.queue.get_mut(idx) {
            Some(t) if t.stage == ReflowStage::Submitted => {
                t.stage = if approve { ReflowStage::Approved } else { ReflowStage::Denied };
                t.decided_ms = Some(now_ms);
                t.reviewer = reviewer;
                true
            }
            _ => false,
        }
    }

    /// 撤回：Submitted → Withdrawn（提交者反悔出线）。
    pub fn withdraw(&mut self, idx: usize) -> bool {
        match self.queue.get_mut(idx) {
            Some(t) if t.stage == ReflowStage::Submitted => {
                t.stage = ReflowStage::Withdrawn;
                true
            }
            _ => false,
        }
    }

    /// 派工：Approved → InBacklog（回炉队列就位）。
    pub fn dispatch(&mut self, idx: usize) -> bool {
        match self.queue.get_mut(idx) {
            Some(t) if t.stage == ReflowStage::Approved => {
                t.stage = ReflowStage::InBacklog;
                true
            }
            _ => false,
        }
    }

    /// 复测关闭：InBacklog → Closed（复测必须过原判据——本函数不判
    /// 判据，由调用方以 WalkEngine.record 复绿后调用）。
    pub fn close(&mut self, idx: usize, now_ms: u64) -> bool {
        match self.queue.get_mut(idx) {
            Some(t) if t.stage == ReflowStage::InBacklog => {
                t.stage = ReflowStage::Closed;
                t.decided_ms = Some(now_ms);
                true
            }
            _ => false,
        }
    }

    /// 未批超时清单（开单超 7 天仍 Submitted——审批台积压告警）。
    pub fn overdue(&self, now_ms: u64) -> Vec<usize> {
        let deadline = WEEKLY_AUDIT_DAYS * 86_400_000;
        self.queue
            .iter()
            .enumerate()
            .filter(|(_, t)| t.stage == ReflowStage::Submitted && now_ms.saturating_sub(t.opened_ms) >= deadline)
            .map(|(i, _)| i)
            .collect()
    }

    /// 台账快照（阶段统计——六态计数）。
    pub fn stage_tally(&self) -> [usize; 6] {
        let mut t = [0usize; 6];
        for q in &self.queue {
            t[match q.stage {
                ReflowStage::Submitted => 0,
                ReflowStage::Approved => 1,
                ReflowStage::Denied => 2,
                ReflowStage::InBacklog => 3,
                ReflowStage::Closed => 4,
                ReflowStage::Withdrawn => 5,
            }] += 1;
        }
        t
    }

    pub fn queue(&self) -> &[ReflowFull] {
        &self.queue
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl Default for ReflowDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 四：走查会话（审计三字段 + 并行隔离）
// ---------------------------------------------------------------------------

/// 一次走查会话（走查人/版本/起止时间戳三审计字段；引擎独立——并行
/// 会话各持引擎互不串写，会话收口时显式合并结果）。
pub struct WalkSession {
    pub id: u32,
    pub walker: &'static str,
    pub started_ms: u64,
    pub closed_ms: Option<u64>,
    pub engine: WalkEngine,
}

impl WalkSession {
    /// 开会话（版本随设计案版本绑定——主册数据与存储条款）。
    pub fn open(id: u32, walker: &'static str, version: &'static str, started_ms: u64) -> WalkSession {
        let mut engine = WalkEngine::new(version);
        let _ = engine.generate_checklist();
        WalkSession { id, walker, started_ms, closed_ms: None, engine }
    }

    /// 收会话（结束时间戳落定；收口后拒绝再记——审计完整性）。
    pub fn close(&mut self, now_ms: u64) -> bool {
        if self.closed_ms.is_some() {
            return false;
        }
        self.closed_ms = Some(now_ms);
        true
    }

    /// 收口后记录拒绝（会话终态保护——防止收口后偷改结果）。
    pub fn record_guarded(&mut self, fid: u32, green: bool, evidence: Option<Evidence>, note: &'static str) -> bool {
        if self.closed_ms.is_some() {
            return false;
        }
        self.engine.record(fid, green, evidence, note)
    }

    /// 会话小结（审计行：id/走查人/起止/绿红计数）。
    pub fn summary(&self) -> String {
        let (g, r, p) = self.tally();
        alloc::format!(
            "session#{} by {} v{}: green={} red={} pending={}",
            self.id,
            self.walker,
            self.engine.version,
            g,
            r,
            p
        )
    }

    fn tally(&self) -> (usize, usize, usize) {
        let mut g = 0;
        let mut r = 0;
        let mut p = 0;
        for rec in &self.engine.records {
            match rec.status {
                WalkStatus::Green => g += 1,
                WalkStatus::Red => r += 1,
                WalkStatus::Pending => p += 1,
            }
        }
        (g, r, p)
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 五：维度评分与热力（20 维度特别加权的量化面）
// ---------------------------------------------------------------------------

/// 维度得分规则（钉死唯一源）：
/// - 3 = 该维度全量锚点全部 Green（有证据）；
/// - 1 = 部分 Green 零 Red（未走完但无失败）；
/// - 0 = 存在任一 Red，或全 Pending。
pub fn dimension_score(engine: &WalkEngine, dim: usize) -> u32 {
    let anchors = dimension_anchors_full(dim);
    let total = anchors.len();
    let mut green = 0usize;
    let mut red = 0usize;
    for fid in &anchors {
        if let Some(r) = engine.records.iter().find(|r| r.fid == *fid) {
            match r.status {
                WalkStatus::Green => green += 1,
                WalkStatus::Red => red += 1,
                WalkStatus::Pending => {}
            }
        }
    }
    if red > 0 {
        0
    } else if green == total && total > 0 {
        3
    } else if green > 0 {
        1
    } else {
        0
    }
}

/// 全维度评分（20 项）。
pub fn dimension_scores(engine: &WalkEngine) -> Vec<(usize, u32)> {
    (0..WALK_DIMENSIONS).map(|d| (d, dimension_score(engine, d))).collect()
}

/// 加权总分（∑ score/3 × weight × 100 / ∑ weight——万分比整数语义）。
pub fn weighted_total_bp(scores: &[(usize, u32)]) -> u32 {
    let total_weight: u32 = DIMENSIONS.iter().map(|(_, w)| w).sum();
    let earned: u32 = scores
        .iter()
        .map(|(d, s)| s * DIMENSIONS[*d].1)
        .sum::<u32>();
    earned * 10_000 / (total_weight * 3)
}

/// 维度热力渲染（文本形态：█=3 ▓=1 ░=0；一行一维度带权值）。
pub fn heat_render(scores: &[(usize, u32)]) -> String {
    let mut s = String::new();
    for (d, score) in scores {
        let bar = match score {
            3 => "███",
            1 => "▓░░",
            _ => "░░░",
        };
        s.push_str(&alloc::format!("D{:02} {} w{} {}\n", d, bar, DIMENSIONS[*d].1, DIMENSIONS[*d].0));
    }
    s
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 六：checklist 导出（MD / JSON 双形态——F126 格式同源）
// ---------------------------------------------------------------------------

/// checklist MD 导出（归档 docs/acceptance/ 的文本形态：表格 + 版本头）。
pub fn export_checklist_md(engine: &WalkEngine) -> String {
    let mut s = String::new();
    s.push_str(&alloc::format!("# 走查 checklist v{}（tool={}）\n\n", engine.version, WALKCHECK_TOOL));
    s.push_str("| F 编号 | 功能 | 判据摘文 | 状态 |\n| --- | --- | --- | --- |\n");
    for r in &engine.records {
        let m = MAP.iter().find(|m| m.fid == r.fid);
        let (name, crit) = match m {
            Some(m) => (m.name, m.criterion),
            None => ("?", "?"),
        };
        let st = match r.status {
            WalkStatus::Green => "✅",
            WalkStatus::Red => "❌",
            WalkStatus::Pending => "⬜",
        };
        s.push_str(&alloc::format!("| F{} | {} | {} | {} |\n", r.fid, name, crit, st));
    }
    s
}

/// checklist JSON 导出（vbase::JsonObj 唯一 JSON 面——F126 checklist
/// schema 同源；每条目 fid/name/criterion/status 四字段）。
pub fn export_checklist_json(engine: &WalkEngine) -> String {
    let mut items: Vec<String> = Vec::new();
    for r in &engine.records {
        let mut o = vbase::JsonObj::new();
        o.num_field("fid", r.fid as u64);
        let name = MAP.iter().find(|m| m.fid == r.fid).map(|m| m.name).unwrap_or("?");
        o.str_field("name", name);
        let crit = MAP.iter().find(|m| m.fid == r.fid).map(|m| m.criterion).unwrap_or("");
        o.str_field("criterion", crit);
        o.str_field("status", match r.status {
            WalkStatus::Green => "green",
            WalkStatus::Red => "red",
            WalkStatus::Pending => "pending",
        });
        items.push(o.finish());
    }
    let mut root = vbase::JsonObj::new();
    root.str_field("tool", WALKCHECK_TOOL);
    root.str_field("version", engine.version);
    root.str_field("dir", ACCEPTANCE_DIR);
    root.raw_array_field("items", &items);
    root.finish()
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 七：锚点提取器（脚本工具化的内核面）
// ---------------------------------------------------------------------------

/// 提取锚点类别（量化数字 / F 引用 / 取证动词——工具化三型）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorKind {
    /// 量化锚（数字 + 单位后缀，如「≤50ms」「10/10」）。
    Quantity,
    /// F 编号引用（F\d{3}——跨项依赖对账面）。
    FRef,
    /// 取证动词（实测/录屏/对拍/全对——证据形态标注）。
    Evidence,
}

impl AnchorKind {
    pub fn tag(self) -> &'static str {
        match self {
            AnchorKind::Quantity => "quantity",
            AnchorKind::FRef => "f-ref",
            AnchorKind::Evidence => "evidence",
        }
    }
}

/// 一条提取结果。
#[derive(Clone, Debug)]
pub struct ExtractedAnchor {
    pub kind: AnchorKind,
    pub text: String,
}

/// 从判据文本提取可执行锚点（`vx-walkcheck-c.py` 形态的内核面）：
/// 逐字符扫描提取数字段（含紧随的 ASCII 单位字母与 %、/）、F 引用与
/// 取证动词。提取为空 = 锚点不可测（设计缺口回炉的结构信号）。
pub fn extract_anchors(criterion: &str) -> Vec<ExtractedAnchor> {
    let bytes = criterion.as_bytes();
    let mut out: Vec<ExtractedAnchor> = Vec::new();
    let mut i = 0usize;
    // 数字段提取：数字连续段 + 紧随单位（ASCII 字母/%/. 与中文单位字）。
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || bytes[i] == b'.'
                    || bytes[i] == b'/'
                    || bytes[i] == b'%'
                    || bytes[i] == b'x'
                    || bytes[i] == b':')
            {
                i += 1;
            }
            while i < bytes.len() && bytes[i] & 0xC0 == 0x80 {
                i += 1;
            }
            while i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] & 0xC0 == 0x80) {
                while i < bytes.len() && bytes[i] & 0xC0 == 0x80 {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'%' {
                    i += 1;
                    break;
                }
                if i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(ExtractedAnchor {
                kind: AnchorKind::Quantity,
                text: String::from(&criterion[start..i]),
            });
        } else if bytes[i] == b'F' && i + 2 < bytes.len() && bytes[i + 1].is_ascii_digit() {
            let start = i;
            let mut j = i + 1;
            let mut digits = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() && digits < 3 {
                j += 1;
                digits += 1;
            }
            if digits == 3 {
                out.push(ExtractedAnchor {
                    kind: AnchorKind::FRef,
                    text: String::from(&criterion[start..j]),
                });
                i = j;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    for verb in ["实测", "录屏", "对拍", "全对"] {
        if criterion.contains(verb) {
            out.push(ExtractedAnchor { kind: AnchorKind::Evidence, text: String::from(verb) });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 八：R8 周对账台账（三册腐化防线）
// ---------------------------------------------------------------------------

/// 一条周对账记录（R8 流程：日期/抽查数/脱节数/处置动作）。
#[derive(Clone, Copy, Debug)]
pub struct AuditEntry {
    pub date_ms: u64,
    pub sampled: usize,
    pub drifted: usize,
    pub action: &'static str,
}

/// 判据摘要（判据文本 SHA-256 hex 前 16 字符——脱节检测的指纹面）。
pub fn criterion_digest(fid: u32) -> String {
    let m = MAP.iter().find(|m| m.fid == fid);
    match m {
        Some(m) => {
            let d = vbase::sha256(m.criterion.as_bytes());
            let full = vbase::hex32_str(&d);
            String::from(&full[..16])
        }
        None => String::from("unknown"),
    }
}

/// 周对账台账（抽查判据指纹与登记值不一致 = 三册腐化——R8 检出）。
pub struct AuditLedger {
    entries: Vec<AuditEntry>,
}

impl AuditLedger {
    pub fn new() -> AuditLedger {
        AuditLedger { entries: Vec::new() }
    }

    /// 记录一条对账（容量 52 = 一年周记录；满后滚动丢最旧）。
    pub fn record(&mut self, date_ms: u64, sampled: usize, drifted: usize, action: &'static str) {
        if self.entries.len() >= 52 {
            self.entries.remove(0);
        }
        self.entries.push(AuditEntry { date_ms, sampled, drifted, action });
    }

    /// 脱节抽查：逐 (fid, 登记指纹) 对拍当前指纹，返回脱节清单。
    pub fn drift_scan(fids: &[(u32, &str)]) -> Vec<u32> {
        let mut drifted = Vec::new();
        for &(fid, expected) in fids {
            let now = criterion_digest(fid);
            if now != expected {
                drifted.push(fid);
            }
        }
        drifted
    }

    /// 脱节率（万分比——R8 报告指标）。
    pub fn drift_rate_bp(&self) -> u32 {
        let total: usize = self.entries.iter().map(|e| e.sampled).sum();
        let bad: usize = self.entries.iter().map(|e| e.drifted).sum();
        if total == 0 {
            0
        } else {
            (bad * 10_000 / total) as u32
        }
    }

    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }
}

impl Default for AuditLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 九：季度走查节奏（F149 生态季报联动）与冻结面
// ---------------------------------------------------------------------------

/// 季度窗（主册：季度走查随生态季报 F149 节奏——90 天窗）。
pub const QUARTER_DAYS: u64 = 90;

/// 季度走查到期判定（距上次全量走查超 90 天 → 到期）。
pub fn quarter_due(last_full_walk_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(last_full_walk_ms) >= QUARTER_DAYS * 86_400
}

/// C 域报告卷冻结面（v1.0 里程碑件：判据面冻结——冻结后判据文本与
/// 版本不可变；走查记录仍可继续——冻结的是「账本」不是「走查动作」）。
pub struct Freeze {
    pub frozen: bool,
    pub version: &'static str,
    pub frozen_ms: Option<u64>,
}

impl Freeze {
    pub fn new() -> Freeze {
        Freeze { frozen: false, version: "", frozen_ms: None }
    }

    /// 冻结（版本强制非空——无版本的冻结是假冻结）。
    pub fn freeze(&mut self, version: &'static str, now_ms: u64) -> bool {
        if version.is_empty() {
            return false;
        }
        self.frozen = true;
        self.version = version;
        self.frozen_ms = Some(now_ms);
        true
    }

    /// 冻结态下版本变更拒绝（里程碑件不可暗中改版）。
    pub fn version_change_allowed(&self, new_version: &'static str) -> bool {
        !self.frozen || new_version == self.version
    }
}

impl Default for Freeze {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v4 · 一：55 项复现命令侧表（十二查·台账与证据的命令面）
// ---------------------------------------------------------------------------

/// 55 项复现命令（fid → 复现口令——十二查第 10 条「数据、复现命令、
/// 日期三件齐」的命令半边；与 MAP 按 fid join 生成完整走查材料。命令
/// 以各分队域隔离测试口令为准；跨分队项指向其登记的验收口令形态）。
pub const WALK_COMMANDS: [(&str, &str); C_DOMAIN_ITEMS] = [
    ("F071", "AI-K2 域聚合测试 k2star::f071_search"),
    ("F072", "AI-K2 域聚合测试 k2star::f072_recent"),
    ("F073", "AI-K2 域聚合测试 k2star::f073_thumbs"),
    ("F074", "AI-K2 域聚合测试 k2star::f074_jump"),
    ("F075", "AI-K2 域聚合测试 k2star::f075_tray"),
    ("F076", "AI-D1 域聚合测试 d1star::f076_quick"),
    ("F077", "AI-D1 域聚合测试 d1star::f077_notify"),
    ("F078", "AI-D1 域聚合测试 d1star::f078_calendar"),
    ("F079", "AI-D1 域聚合测试 d1star::f079_sounds"),
    ("F080", "AI-D1 域聚合测试 d1star::f080_snap"),
    ("F081", "AI-D1 域聚合测试 d1star::f081_taskview"),
    ("F082", "AI-D1 域聚合测试 d1star::f082_alttab"),
    ("F083", "AI-D1 域聚合测试 d1star::f083_refresh"),
    ("F084", "AI-D1 域聚合测试 d1star::f084_grid"),
    ("F085", "AI-D1 域聚合测试 d1star::f085_recycle"),
    ("F086", "AI-D1 域聚合测试 d1star::f086_copydlg"),
    ("F087", "AI-D1 域聚合测试 d1star::f087_conflict"),
    ("F088", "AI-D1 域聚合测试 d1star::f088_search"),
    ("F089", "AI-D1 域聚合测试 d1star::f089_tabs"),
    ("F090", "AI-D1 域聚合测试 d1star::f090_breadcrumb"),
    ("F091", "AI-D1 域聚合测试 d1star::f091_detail"),
    ("F092", "AI-D1 域聚合测试 d1star::f092_zip"),
    ("F093", "AI-D2 域聚合测试 d2star::f093_thumbs"),
    ("F094", "AI-D2 域聚合测试 d2star::f094_media"),
    ("F095", "AI-D2 域聚合测试 d2star::f095_terminal"),
    ("F096", "AI-D2 域聚合测试 d2star::f096_palette"),
    ("F097", "AI-D2 域聚合测试 d2star::f097_editor"),
    ("F098", "AI-D2 域聚合测试 d2star::f098_shot"),
    ("F099", "AI-D2 域聚合测试 d2star::f099_calc"),
    ("F100", "AI-D2 域聚合测试 d2star::f100_clock"),
    ("F101", "AI-D2 域聚合测试 d2star::f101_weather"),
    ("F102", "AI-D2 域聚合测试 d2star::f102_sticky"),
    ("F103", "AI-D2 域聚合测试 d2star::f103_paint"),
    ("F104", "AI-D2 域聚合测试 d2star::f104_record"),
    ("F105", "AI-D2 域聚合测试 d2star::f105_gallery"),
    ("F106", "AI-D2 域聚合测试 d2star::f106_hud"),
    ("F107", "AI-D2 域聚合测试 d2star::f107_imewin"),
    ("F108", "AI-D2 域聚合测试 d2star::f108_phrases"),
    ("F109", "AI-D2 域聚合测试 d2star::f109_cliphist"),
    ("F110", "AI-D2 域聚合测试 d2star::f110_osk"),
    ("F111", "cargo test svstar::magnifier"),
    ("F112", "cargo test svstar::narrator"),
    ("F113", "cargo test svstar::highcontrast"),
    ("F114", "cargo test svstar::colorfilter"),
    ("F115", "cargo test svstar::focusmode"),
    ("F116", "cargo test svstar::nightlight"),
    ("F117", "cargo test svstar::oobe"),
    ("F118", "cargo test svstar::welcome"),
    ("F119", "cargo test svstar::helpcenter"),
    ("F120", "cargo test svstar::diagcenter"),
    ("F121", "cargo test svstar::restorept"),
    ("F122", "cargo test svstar::updateux"),
    ("F123", "cargo test svstar::aboutpage"),
    ("F124", "cargo test svstar::motioncore"),
    ("F125", "cargo test svstar::walkcheck"),
];

/// 命令侧表完整性对账（join 键全命中——55 条逐一可查，无缺号无错号）。
pub fn commands_join_complete() -> bool {
    MAP.iter().all(|m| {
        let key = alloc::format!("F{:03}", m.fid);
        WALK_COMMANDS.iter().any(|(k, _)| *k == key.as_str())
    })
}

/// 完整 checklist 导出（MAP 判据 × WALK_COMMANDS 复现口令 × owner 三列
/// join——走查人拿到的材料一行一案，证据三件套的命令槽直接可填）。
pub fn export_checklist_full_md(engine: &WalkEngine) -> String {
    let mut s = String::new();
    s.push_str(&alloc::format!(
        "# 走查 checklist 完整版 v{}（MAP × 命令 × 责任）\n\n",
        engine.version
    ));
    s.push_str("| F 编号 | 判据摘文 | 复现口令 | 责任分队 | 状态 |\n| --- | --- | --- | --- | --- |\n");
    for r in &engine.records {
        let m = match MAP.iter().find(|m| m.fid == r.fid) {
            Some(m) => m,
            None => continue,
        };
        let key = alloc::format!("F{:03}", m.fid);
        let cmd = WALK_COMMANDS
            .iter()
            .find(|(k, _)| *k == key.as_str())
            .map(|(_, c)| *c)
            .unwrap_or("（缺命令——设计缺口）");
        let st = match r.status {
            WalkStatus::Green => "✅",
            WalkStatus::Red => "❌",
            WalkStatus::Pending => "⬜",
        };
        s.push_str(&alloc::format!(
            "| F{} | {} | {} | {} | {} |\n",
            r.fid, m.criterion, cmd, m.owner, st
        ));
    }
    s
}

/// 日期槽（证据三件套第三件：对账批次日期——批次唯一源常量）。
pub const AUDIT_BATCH_DATE: &str = "2026-09-26";
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

    // 13. 判据锚号生成器（深化 v2）：F111→G-C-41、F125→G-C-55 双已知锚
    //     对拍 + 全域往返（71..=125 生成不为空）。
    let pairs = criterion_anchor_known_pairs();
    let anchor_ok = pairs
        .iter()
        .all(|(fid, want)| &criterion_anchor(*fid) == want)
        && (71..=125).all(|f| !criterion_anchor(f).is_empty());
    set.add("criterion anchor G-C-xx round-trip", anchor_ok, "");

    // 14. 三列映射自动生成 + 人工复核双轨：全 F 覆盖（每 F ≥1 行）+
    //     复核盖章后待复数归零。
    let mut cols = auto_columns();
    let all_covered = (71u32..=125).all(|f| cols.iter().any(|r| r.fid == f));
    let pending_before = review_columns(&mut cols, &[]);
    let covered_fids: Vec<u32> = cols.iter().map(|r| r.fid).collect();
    let pending_after = review_columns(&mut cols, &covered_fids);
    set.add(
        "three-column map auto-gen + review dual-track",
        all_covered && pending_before == cols.len() && pending_after == 0,
        "",
    );

    // 15. F→维度归属全量：20 维度每维度 ≥1 锚（无空维度）且每 F ≥1 维度。
    let no_empty_dim = (0..WALK_DIMENSIONS).all(|d| !dimension_anchors_full(d).is_empty());
    let every_f_mapped = MAP.iter().all(|m| !dimensions_of(m.fid).is_empty());
    set.add("dimension ownership full coverage", no_empty_dim && every_f_mapped, "");

    // 16. 回流单完整状态机全链：open→decide(approve)→dispatch→close 绿；
    //     deny 与 withdraw 分支各落终态；终态再流转拒绝。
    let mut desk = ReflowDesk::new();
    let i0 = desk.open(122, "断电样本缺", 0).unwrap();
    let i1 = desk.open(118, "直跳路径偏差", 0).unwrap();
    let i2 = desk.open(117, "汇总文案错", 0).unwrap();
    let c1 = desk.decide(i0, true, "reviewer-A", 1000);
    let d1 = desk.dispatch(i0);
    let cl1 = desk.close(i0, 2000);
    let d2 = desk.decide(i1, false, "reviewer-B", 1000);
    let w1 = desk.withdraw(i2);
    let terminal_locked = !desk.decide(i1, true, "x", 3000) && !desk.close(i1, 3000);
    set.add(
        "reflow full state machine chain",
        c1 && d1 && cl1 && d2 && w1 && terminal_locked,
        "",
    );

    // 17. 回流超时提醒：开单 7 天未批 → overdue 命中；批准后不再提醒。
    let mut desk = ReflowDesk::new();
    let _ = desk.open(121, "n", 0);
    let overdue_at_7d = desk.overdue(WEEKLY_AUDIT_DAYS * 86_400_000).len() == 1;
    let _ = desk.decide(0, true, "r", 1);
    let cleared_after_decision = desk.overdue(WEEKLY_AUDIT_DAYS * 86_400_000).is_empty();
    set.add("reflow overdue 7d alert", overdue_at_7d && cleared_after_decision, "");

    // 18. 走查会话审计：收口后记录拒绝 + 小结含走查人与计数。
    let mut s1 = WalkSession::open(1, "walker-A", "1.0.0", 0);
    let _ = s1.record_guarded(111, true, Some(Evidence { data: "d", command: "c", date: "2026-09-26" }), "");
    let closed = s1.close(5000);
    let refused = !s1.record_guarded(112, true, None, "");
    let sum = s1.summary();
    set.add(
        "walk session audit + close guard",
        closed && refused && sum.contains("walker-A") && sum.contains("green=1"),
        "",
    );

    // 19. 维度评分规则：全绿 3 / 部分绿 1 / 有红 0 / 全 pending 0。
    let mut e = WalkEngine::new("1.0.0");
    e.generate_checklist();
    let score_pending = dimension_score(&e, 4);
    let _ = e.record(75, true, Some(Evidence { data: "x", command: "y", date: "z" }), "");
    let score_partial = dimension_score(&e, 4);
    let _ = e.record(93, false, None, "bad");
    let score_red = dimension_score(&e, 4);
    set.add(
        "dimension score rule 3/1/0",
        score_pending == 0 && score_partial == 1 && score_red == 0,
        "",
    );

    // 20. 加权总分：全 Green 会话 → 满分 10000bp；空会话 → 0。
    let mut full = WalkEngine::new("1.0.0");
    full.generate_checklist();
    for m in MAP.iter() {
        let _ = full.record(m.fid, true, Some(Evidence { data: "x", command: "y", date: "z" }), "");
    }
    let scores_full = dimension_scores(&full);
    let bp_full = weighted_total_bp(&scores_full);
    let empty = WalkEngine::new("1.0.0");
    let bp_empty = weighted_total_bp(&dimension_scores(&empty));
    set.add("weighted total bp full=10000 empty=0", bp_full == 10_000 && bp_empty == 0, "");

    // 21. checklist 双形态导出：MD 表格含状态列与 55 行；JSON 含 tool/
    //     version/items 且状态字段合法。
    let md = export_checklist_md(&e);
    let js = export_checklist_json(&e);
    set.add(
        "checklist export md + json",
        md.contains("| F71 |") && md.matches("| F").count() >= C_DOMAIN_ITEMS
            && js.contains("\"tool\":\"tools/vx-walkcheck-c.py\"")
            && js.contains("\"version\":\"1.0.0\"")
            && js.contains("\"status\":\"red\"")
            && js.contains("\"status\":\"green\""),
        "",
    );

    // 22. 锚点提取器三型：量化（50ms/10/10）、F 引用（F072）、取证动词。
    let a1 = extract_anchors("按键到首结果 ≤50ms；与 F072 数据一致；实测录屏 10/10");
    let has_qty = a1.iter().any(|a| a.kind == AnchorKind::Quantity && a.text.contains("50"));
    let has_fref = a1.iter().any(|a| a.kind == AnchorKind::FRef && a.text == "F072");
    let has_evi = a1.iter().any(|a| a.kind == AnchorKind::Evidence);
    set.add("anchor extractor three kinds", has_qty && has_fref && has_evi, "");

    // 23. R8 周对账：指纹登记一致 → 零脱节；指纹错 → 检出并计入脱节率。
    let digest_f111 = criterion_digest(111);
    let clean = AuditLedger::drift_scan(&[(111, digest_f111.as_str())]).is_empty();
    let drifted_bad = AuditLedger::drift_scan(&[(111, "ffffffffffffffffffffffffffffffff")]);
    let mut ledger = AuditLedger::new();
    ledger.record(0, 10, 1, "回炉 1 项");
    ledger.record(1, 10, 0, "零脱节");
    set.add(
        "r8 drift scan + ledger rate",
        clean && drifted_bad == vec![111] && ledger.drift_rate_bp() == 500,
        "",
    );

    // 24. 季度窗与冻结面：90 天到期；冻结后版本变更拒绝、同版放行；
    //     空版本冻结拒绝。
    let due = quarter_due(0, QUARTER_DAYS * 86_400);
    let not_due = !quarter_due(0, QUARTER_DAYS * 86_400 - 1);
    let mut fz = Freeze::new();
    let bad_freeze = !fz.freeze("", 0);
    let ok_freeze = fz.freeze("1.0.0", 1000);
    let ver_locked = !fz.version_change_allowed("1.1.0") && fz.version_change_allowed("1.0.0");
    set.add(
        "quarter cadence + freeze discipline",
        due && not_due && bad_freeze && ok_freeze && ver_locked,
        "",
    );


    // 25. 复现命令侧表（深化 v4）：55 条 join 键全命中 + 全 V1 项指向
    //     svstar 模块口令（跨分队项指向其登记口令形态）。
    let join_ok = commands_join_complete();
    let v1_cmd_ok = WALK_COMMANDS.iter().all(|(k, c)| {
        let fnum: u32 = k[1..].parse().unwrap_or(0);
        if (111..=125).contains(&fnum) {
            c.starts_with("cargo test svstar::")
        } else {
            c.contains("域聚合测试")
        }
    });
    set.add("walk commands 55 join + v1 routed", join_ok && v1_cmd_ok, "");

    // 26. 完整 checklist 导出（深化 v4）：四列齐（判据×命令×责任×状态）
    //     55 行全生成；缺命令行不存在。
    let mut e = WalkEngine::new("1.1.0");
    e.generate_checklist();
    let _ = e.record(111, true, Some(Evidence { data: "18/18", command: "cargo test svstar::magnifier", date: AUDIT_BATCH_DATE }), "");
    let full = export_checklist_full_md(&e);
    set.add(
        "full checklist md join map+cmd+owner",
        full.contains("复现口令")
            && full.contains("cargo test svstar::magnifier")
            && full.contains("AI-K2 域聚合测试 k2star::f071_search")
            && full.contains("✅")
            && full.matches("\n| F").count() >= C_DOMAIN_ITEMS,
        "",
    );

    // 27. 证据三件套日期槽（深化 v4）：批次日期常量唯一源在册。
    set.add("audit batch date registered", AUDIT_BATCH_DATE == "2026-09-26", "");

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

    #[test]
    fn f125_anchor_rule_matches_master_book() {
        // 主册两处已知锚点往返 + 锚号格式（G-C- 两位）。
        assert_eq!(criterion_anchor(111), "G-C-41");
        assert_eq!(criterion_anchor(125), "G-C-55");
        assert_eq!(criterion_anchor(71), "G-C-01");
        for f in 71..=125 {
            let a = criterion_anchor(f);
            assert!(a.starts_with("G-C-") && a.len() == 6, "锚号格式 G-C-xx");
        }
    }

    #[test]
    fn f125_column_rows_anchor_column_consistent() {
        // 三列映射的判据编号列与锚号生成器一致（一处一事实对拍）。
        for r in auto_columns() {
            assert_eq!(r.anchor, criterion_anchor(r.fid));
        }
    }

    #[test]
    fn f125_dimension_coverage_symmetric() {
        // 每维度锚点都在 MAP 域内且归属函数对每个 MAP 元素都有产出。
        for d in 0..WALK_DIMENSIONS {
            for fid in dimension_anchors_full(d) {
                assert!((71..=125).contains(&fid));
            }
        }
        assert!(dimension_anchors_full(19).contains(&125), "整体感受维度必含 F125 自身");
    }

    #[test]
    fn f125_reflow_unknown_stage_transitions_rejected() {
        let mut desk = ReflowDesk::new();
        // 空desk 上未开单的流转全部拒绝。
        assert!(desk.dispatch(0) == false);
        assert!(desk.close(0, 1) == false);
        assert!(desk.withdraw(0) == false);
        // 未决单不能直接 close（必须先批准派工）。
        let i = desk.open(111, "n", 0).unwrap();
        assert!(!desk.close(i, 1));
        assert!(desk.decide(i, true, "r", 2));
        // Withdrawn 单是终态。
        let j = desk.open(112, "n2", 0).unwrap();
        assert!(desk.withdraw(j));
        assert!(!desk.decide(j, true, "r", 3));
        assert!(desk.queue()[i].stage == ReflowStage::Approved);
        assert!(desk.queue()[j].stage.terminal());
    }

    #[test]
    fn f125_session_parallel_isolation() {
        // 双会话并行：各自引擎记录互不影响。
        let mut a = WalkSession::open(7, "A", "1.0.0", 0);
        let mut b = WalkSession::open(8, "B", "1.0.0", 0);
        let ev = Some(Evidence { data: "d", command: "c", date: "2026-09-26" });
        assert!(a.record_guarded(111, true, ev.clone(), ""));
        assert!(b.record_guarded(112, true, ev, ""));
        assert!(b.engine.records.iter().find(|r| r.fid == 111).unwrap().status == WalkStatus::Pending);
        assert!(a.engine.records.iter().find(|r| r.fid == 112).unwrap().status == WalkStatus::Pending);
    }

    #[test]
    fn f125_extractor_no_false_frefs() {
        // 「F1」「F12」不误报；无锚文本返回空（不可测结构信号）。
        assert!(extract_anchors("只有 F1 和 F12 字样").iter().all(|a| a.kind != AnchorKind::FRef));
        assert!(extract_anchors("无锚点文本").is_empty());
        let q = extract_anchors("80fps 跟手");
        assert!(q.iter().any(|a| a.kind == AnchorKind::Quantity && a.text.starts_with("80")));
    }

    #[test]
    fn f125_ledger_rolling_cap_52() {
        let mut ledger = AuditLedger::new();
        for i in 0..55u64 {
            ledger.record(i * 1000, 1, 0, "weekly");
        }
        assert_eq!(ledger.entries().len(), 52, "周台账一年滚动窗");
        assert_eq!(ledger.entries()[0].date_ms, 3 * 1000, "最旧三条被滚动逐出");
    }

    #[test]
    fn f125_heat_render_bar_shapes() {
        let mut e = WalkEngine::new("1.0.0");
        e.generate_checklist();
        for m in MAP.iter() {
            let _ = e.record(m.fid, true, Some(Evidence { data: "x", command: "y", date: "z" }), "");
        }
        let scores = dimension_scores(&e);
        let heat = heat_render(&scores);
        assert!(heat.contains("███"), "全绿维度热力条");
        assert_eq!(heat.lines().count(), WALK_DIMENSIONS);
    }

    #[test]
    fn f125_commands_dedup_and_coverage() {
        // 55 条命令无重复键（join 键唯一）。
        for i in 0..WALK_COMMANDS.len() {
            for j in (i + 1)..WALK_COMMANDS.len() {
                assert_ne!(WALK_COMMANDS[i].0, WALK_COMMANDS[j].0);
            }
        }
        assert_eq!(WALK_COMMANDS.len(), 55);
        assert!(commands_join_complete());
    }

    #[test]
    fn f125_full_md_pending_rows_have_box() {
        let mut e = WalkEngine::new("1.0.0");
        e.generate_checklist();
        let md = export_checklist_full_md(&e);
        assert!(md.contains("⬜"), "未走条目带待走标记");
    }
}
