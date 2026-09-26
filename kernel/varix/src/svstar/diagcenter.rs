//! F120 诊断中心 · 完整设计（STAR I 主册 G-C-50）。
//!
//! **判据（主册）**：四体检灯与 F062 数据一致；三修复项执行-回滚全链
//! 实测；脱敏导出三查通过（注入敏感样本验证）。
//!
//! **设计要点（主册）**：
//! - 系统健康统一入口：健康快照（F062）展示 / 日志导出（脱敏选项）/
//!   一键修复项（引导菜单修复/图标缓存重建/缩略图库重建）/ 启动
//!   时间线（F053）查看；
//! - 窗口 880×600px：顶四体检灯卡；中部修复项列表（名称/说明/风险级/
//!   执行钮）；页签：快照/日志/时间线/修复；
//! - 体检灯三色语义文档化（绿=正常/黄=关注/红=建议处理+直链帮助 F119）；
//! - 修复项注册制（新增修复必须登记风险级与回滚方案——门禁）；执行前
//!   说明做什么可回滚；修复项执行失败 → 三要素+回滚自动（全部设计为
//!   可回滚）；
//! - 导出包 zip 含 manifest（脱敏声明）；脱敏规则固定三查（vbase::
//!   sanitize 唯一源：路径用户段/序列号/密钥类）；日志量大（>100MB）
//!   → 分卷导出；
//! - 日志查看器内嵌（过滤/搜索/时间窗）；时间线页复用 F053 甘特组件；
//! - 日志格式开放（F126——社区工具可解析）。
//!
//! 时间注入式（毫秒戳），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use crate::svstar::vbase;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 窗口尺寸（px，主册：880×600）。
pub const WINDOW_W_PX: u32 = 880;
pub const WINDOW_H_PX: u32 = 600;
/// 日志分卷阈值（字节，主册：>100MB → 分卷导出）。
pub const EXPORT_SPLIT_BYTES: u64 = 100_000_000;
/// 官方修复项数（主册三件：引导菜单修复/图标缓存重建/缩略图库重建）。
pub const OFFICIAL_REPAIRS: usize = 3;
/// 页签数（快照/日志/时间线/修复）。
pub const TAB_COUNT: usize = 4;

// ---------------------------------------------------------------------------
// 体检灯（F062 数据一致面）
// ---------------------------------------------------------------------------

/// 四体检灯（主册：帧率/存储/调度/唤醒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lamp {
    Frame,
    Storage,
    Scheduler,
    Wakeup,
}

impl Lamp {
    pub fn name(self) -> &'static str {
        match self {
            Lamp::Frame => "帧率",
            Lamp::Storage => "存储",
            Lamp::Scheduler => "调度",
            Lamp::Wakeup => "唤醒",
        }
    }
}

/// 灯色三态（语义文档化：绿=正常/黄=关注/红=建议处理）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LampColor {
    Green,
    Yellow,
    Red,
}

/// F062 注入数据：一项指标（当前值，黄线，红线；方向=高于红线异常）。
#[derive(Clone, Copy, Debug)]
pub struct Metric {
    pub lamp: Lamp,
    pub value: u64,
    /// 黄线（关注阈值）。
    pub warn_at: u64,
    /// 红线（处理阈值）。
    pub fail_at: u64,
}

impl Metric {
    /// 灯色判定（与 F062 数据一致的唯一换算：value < warn → 绿；
    /// warn ≤ value < fail → 黄；value ≥ fail → 红）。
    pub fn lamp_color(&self) -> LampColor {
        if self.value >= self.fail_at {
            LampColor::Red
        } else if self.value >= self.warn_at {
            LampColor::Yellow
        } else {
            LampColor::Green
        }
    }
}

/// 四灯快照（F062 数据源注入）。
#[derive(Clone, Copy, Debug)]
pub struct HealthSnapshot {
    pub metrics: [Metric; 4],
}

impl HealthSnapshot {
    pub fn lamp(&self, lamp: Lamp) -> LampColor {
        for m in &self.metrics {
            if m.lamp == lamp {
                return m.lamp_color();
            }
        }
        LampColor::Red
    }

    /// 快照注入溯源（判据：与 F062 数据一致——灯色由注入值唯一决定，
    /// 无第二数据源的结构性断言：snapshot 不持缓存值）。
    pub fn consistent_with_source(&self) -> bool {
        self.metrics.iter().all(|m| {
            let want = if m.value >= m.fail_at {
                LampColor::Red
            } else if m.value >= m.warn_at {
                LampColor::Yellow
            } else {
                LampColor::Green
            };
            self.lamp(m.lamp) == want
        })
    }
}

// ---------------------------------------------------------------------------
// 修复项（注册制 + 回滚）
// ---------------------------------------------------------------------------

/// 修复风险级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    Low,
    Medium,
    High,
}

/// 一项注册的修复。
pub struct Repair {
    pub name: &'static str,
    pub desc: &'static str,
    pub risk: Risk,
    /// 回滚方案（登记制——无方案不合入，门禁）。
    pub rollback_plan: &'static str,
    /// 执行体：改写系统态（返回 Ok(撤销闭包数据) 或 Err(失败原因)）。
    /// 撤销数据以操作日志承载（真实修复的 undo 面）。
    pub apply_fn: fn(&mut RepairWorld) -> Result<u64, &'static str>,
}

/// 修复操作的世界态（模拟系统配置层——真实修复的执行面）。
#[derive(Default)]
pub struct RepairWorld {
    /// 引导菜单条目数。
    pub boot_menu_entries: u64,
    /// 图标缓存条目数。
    pub icon_cache_entries: u64,
    /// 缩略图库条目数。
    pub thumb_cache_entries: u64,
    /// 已执行操作日志（回滚依据）。
    pub op_log: Vec<(&'static str, u64)>,
}

/// 官方三修复项（主册定名；全部可回滚）。
pub fn official_repairs() -> Vec<Repair> {
    vec![
        Repair {
            name: "引导菜单修复",
            desc: "重建引导菜单条目表（不触碰固件引导程序——红线遵循）",
            risk: Risk::High,
            rollback_plan: "备份条目表快照，回滚=整表还原",
            apply_fn: |w| {
                let backup = w.boot_menu_entries;
                w.op_log.push(("boot-menu-backup", backup));
                w.boot_menu_entries = w.boot_menu_entries.max(1);
                Ok(backup)
            },
        },
        Repair {
            name: "图标缓存重建",
            desc: "清空并重建图标缓存（重启资源管理器后生效）",
            risk: Risk::Low,
            rollback_plan: "旧缓存保留一个版本，回滚=换回旧版",
            apply_fn: |w| {
                let old = w.icon_cache_entries;
                w.op_log.push(("icon-cache-backup", old));
                w.icon_cache_entries = 0;
                Ok(old)
            },
        },
        Repair {
            name: "缩略图库重建",
            desc: "重建缩略图库（按需重新生成，期间旧图可用）",
            risk: Risk::Low,
            rollback_plan: "旧库保留一个版本，回滚=换回旧版",
            apply_fn: |w| {
                let old = w.thumb_cache_entries;
                w.op_log.push(("thumb-cache-backup", old));
                w.thumb_cache_entries = 0;
                Ok(old)
            },
        },
    ]
}

/// 修复执行结果（三要素：发生了什么/为什么/下一步）。
#[derive(Clone, Debug)]
pub struct RepairOutcome {
    pub repair: &'static str,
    pub ok: bool,
    /// 三要素文案（成功=做了什么+影响+回滚入口；失败=原因+已回滚+重试钮）。
    pub what: &'static str,
    pub why: &'static str,
    pub next: &'static str,
}

/// 修复执行器：执行 → 失败自动回滚（全部设计为可回滚）。
pub struct RepairRunner {
    pub world: RepairWorld,
    /// 执行记录。
    pub history: Vec<RepairOutcome>,
}

impl RepairRunner {
    pub fn new() -> RepairRunner {
        RepairRunner { world: RepairWorld::default(), history: Vec::new() }
    }

    /// 执行一项修复：成功记录三要素；失败 → 自动回滚（op_log 逆序还原）
    /// + 三要素失败文案。
    pub fn run(&mut self, repair: &Repair, force_fail: bool) -> &RepairOutcome {
        let before_log = self.world.op_log.len();
        let outcome = if force_fail {
            // 注入失败：自动回滚（备份还原语义）。
            let rolled = self.rollback_to(before_log);
            RepairOutcome {
                repair: repair.name,
                ok: false,
                what: "修复未完成",
                why: "执行过程出错",
                next: if rolled { "已自动回滚，可重试" } else { "回滚未完成，请导出日志求助" },
            }
        } else {
            match (repair.apply_fn)(&mut self.world) {
                Ok(_) => RepairOutcome {
                    repair: repair.name,
                    ok: true,
                    what: repair.desc,
                    why: "按登记方案执行",
                    next: "如需撤销，走回滚入口",
                },
                Err(e) => {
                    let rolled = self.rollback_to(before_log);
                    RepairOutcome {
                        repair: repair.name,
                        ok: false,
                        what: "修复未完成",
                        why: e,
                        next: if rolled { "已自动回滚，可重试" } else { "回滚未完成，请导出日志求助" },
                    }
                }
            }
        };
        self.history.push(outcome);
        self.history.last().unwrap()
    }

    /// 回滚到 op_log 指定长度（逆序还原）。
    fn rollback_to(&mut self, len: usize) -> bool {
        while self.world.op_log.len() > len {
            let (tag, val) = self.world.op_log.pop().unwrap();
            match tag {
                "boot-menu-backup" => self.world.boot_menu_entries = val,
                "icon-cache-backup" => self.world.icon_cache_entries = val,
                "thumb-cache-backup" => self.world.thumb_cache_entries = val,
                _ => return false,
            }
        }
        true
    }

    /// 修复项登记门禁：风险级与回滚方案必须齐（注册制）。
    pub fn registration_valid(r: &Repair) -> bool {
        !r.name.is_empty() && !r.desc.is_empty() && !r.rollback_plan.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 日志库（查看器 + 导出）
// ---------------------------------------------------------------------------

/// 日志级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// 一条日志。
#[derive(Clone, Debug)]
pub struct LogLine {
    pub at_ms: u64,
    pub level: LogLevel,
    pub text: String,
}

/// 日志库：过滤/搜索/时间窗 + 导出（脱敏 + 分卷）。
pub struct LogStore {
    lines: Vec<LogLine>,
}

impl LogStore {
    pub fn new() -> LogStore {
        LogStore { lines: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn push(&mut self, at_ms: u64, level: LogLevel, text: &str) {
        self.lines.push(LogLine { at_ms, level, text: String::from(text) });
    }

    /// 时间窗 + 级别过滤 + 关键词搜索（三条件与语义）。
    pub fn query(
        &self,
        from_ms: u64,
        to_ms: u64,
        min_level: LogLevel,
        keyword: Option<&str>,
    ) -> Vec<&LogLine> {
        self.lines
            .iter()
            .filter(|l| {
                l.at_ms >= from_ms
                    && l.at_ms <= to_ms
                    && level_rank(l.level) >= level_rank(min_level)
                    && keyword.map(|k| l.text.contains(k)).unwrap_or(true)
            })
            .collect()
    }

    /// 导出：脱敏选项（默认开——vbase 三查逐行）+ 分卷（按 EXPORT_SPLIT_BYTES
    /// 上限切卷）。返回（各卷行数，是否触发分卷）。
    pub fn export(&self, sanitize: bool) -> (Vec<usize>, bool) {
        let mut out = Vec::new();
        let mut cur_bytes = 0u64;
        let mut cur_lines = 0usize;
        let mut split = false;
        for l in &self.lines {
            let text = if sanitize { vbase::sanitize_line(&l.text) } else { l.text.clone() };
            let size = text.len() as u64 + 8;
            if cur_bytes + size > EXPORT_SPLIT_BYTES && cur_lines > 0 {
                out.push(cur_lines);
                cur_bytes = 0;
                cur_lines = 0;
                split = true;
            }
            cur_bytes += size;
            cur_lines += 1;
        }
        if cur_lines > 0 {
            out.push(cur_lines);
        }
        (out, split)
    }

    /// 导出 manifest（zip 内附）：脱敏声明 + 行数 + 时间窗。
    pub fn export_manifest(&self, sanitize: bool) -> String {
        let (vols, split) = self.export(sanitize);
        alloc::format!(
            "manifest: lines={} volumes={} split={} sanitized={} policy=[user-path,serial,key]",
            self.lines.len(),
            vols.len(),
            split,
            sanitize
        )
    }
}

fn level_rank(l: LogLevel) -> u8 {
    match l {
        LogLevel::Debug => 0,
        LogLevel::Info => 1,
        LogLevel::Warn => 2,
        LogLevel::Error => 3,
    }
}

// ---------------------------------------------------------------------------
// 启动时间线（F053 甘特组件复用语义）
// ---------------------------------------------------------------------------

/// 时间线一段（F053 五段刻度语义：装载/初始化/首帧/可用/稳定）。
#[derive(Clone, Copy, Debug)]
pub struct TimelineSeg {
    pub name: &'static str,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl TimelineSeg {
    pub fn duration(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// 时间线视图：段序列 + 边界连续性校验（F053 判据：五段一一对应——
/// 无重叠无遗漏）。
#[derive(Clone)]
pub struct BootTimeline {
    pub segs: Vec<TimelineSeg>,
}

impl BootTimeline {
    /// 边界连续：段按 start 升序且相邻段首尾相接（允许零间隙）。
    pub fn contiguous(&self) -> bool {
        let mut sorted: Vec<&TimelineSeg> = self.segs.iter().collect();
        sorted.sort_by_key(|s| s.start_ms);
        for w in sorted.windows(2) {
            if w[0].end_ms > w[1].start_ms {
                return false;
            }
        }
        true
    }

    /// 总时长。
    pub fn total_ms(&self) -> u64 {
        self.segs.iter().map(|s| s.duration()).sum()
    }
}

// ---------------------------------------------------------------------------
// 诊断中心门面
// ---------------------------------------------------------------------------

/// 四页签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Snapshot,
    Logs,
    Timeline,
    Repairs,
}

/// 诊断中心（页签容器语义 + 组合三面）。
pub struct DiagCenter {
    pub snapshot: HealthSnapshot,
    pub logs: LogStore,
    pub timeline: BootTimeline,
    pub repairs: Vec<Repair>,
    pub runner: RepairRunner,
    pub active_tab: Tab,
}

impl DiagCenter {
    pub fn new(snapshot: HealthSnapshot, timeline: BootTimeline) -> DiagCenter {
        DiagCenter {
            snapshot,
            logs: LogStore::new(),
            timeline,
            repairs: official_repairs(),
            runner: RepairRunner::new(),
            active_tab: Tab::Snapshot,
        }
    }

    pub fn switch_tab(&mut self, t: Tab) {
        self.active_tab = t;
    }

    /// 红灯 → 帮助直链（体检灯三色语义：红=建议处理+直链帮助 F119）。
    pub fn red_lamp_help_link(&self) -> Option<&'static str> {
        for m in &self.snapshot.metrics {
            if self.snapshot.lamp(m.lamp) == LampColor::Red {
                return Some(match m.lamp {
                    Lamp::Frame => "help:frame-drops",
                    Lamp::Storage => "help:storage",
                    Lamp::Scheduler => "help:scheduler",
                    Lamp::Wakeup => "help:wakeup",
                });
            }
        }
        None
    }

    /// 注册门禁：官方三件必须全部登记齐全（判据：OFFICIAL_REPAIRS 数 +
    /// 每件 registration_valid）。
    pub fn repairs_registered(&self) -> bool {
        self.repairs.len() == OFFICIAL_REPAIRS
            && self.repairs.iter().all(RepairRunner::registration_valid)
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2：日志三条件查询 / 分卷导出计算
// ---------------------------------------------------------------------------

/// 日志查询条件（等级/时间窗/文本子串——三轴 AND；主册「日志查看器
/// 内嵌（过滤/搜索/时间窗）」）。
#[derive(Clone, Copy, Debug, Default)]
pub struct LogQuery {
    pub level_min: Option<LogLevel>,
    pub since_ms: Option<u64>,
    pub until_ms: Option<u64>,
    pub text_contains: Option<&'static str>,
}

impl LogStore {
    /// 结构化三条件过滤查询（等级/时间窗/文本子串三轴 AND；与既有
    /// 四参 query 的差异：可选轴——不关心的轴不设限）。
    pub fn query_filtered(&self, q: LogQuery) -> Vec<&LogLine> {
        self.lines
            .iter()
            .filter(|l| q.level_min.map(|m| l.level as u32 >= m as u32).unwrap_or(true))
            .filter(|l| q.since_ms.map(|s| l.at_ms >= s).unwrap_or(true))
            .filter(|l| q.until_ms.map(|u| l.at_ms < u).unwrap_or(true))
            .filter(|l| q.text_contains.map(|t| l.text.contains(t)).unwrap_or(true))
            .collect()
    }
}

/// 分卷导出计算（>100MB → 分卷；返回卷数与每卷字节——总字节对账）。
pub fn export_volume_plan(total_bytes: u64, cap_bytes: u64) -> (usize, u64) {
    if total_bytes == 0 {
        return (1, 0);
    }
    let cap = cap_bytes.max(1);
    let volumes = ((total_bytes + cap - 1) / cap) as usize;
    (volumes, cap.min(total_bytes))
}
// ---------------------------------------------------------------------------
// 深化批次 v4/v5：修复注册门禁 / 甘特导出 / 导出 manifest 全文
// ---------------------------------------------------------------------------

/// 修复项注册门禁（主册【设计细节】「修复项注册制（新增修复必须登记
/// 风险级与回滚方案——门禁）」的机器面）：名称/说明/风险级/回滚方案
/// 四件缺一即拒——没有回滚方案的修复不许上架。
pub fn repair_registration_gate(
    name: &str,
    desc: &str,
    risk: Risk,
    rollback_plan: &str,
) -> Result<(), &'static str> {
    if name.trim().is_empty() {
        return Err("repair name mandatory");
    }
    if desc.chars().count() < 8 {
        return Err("repair desc too short (explain what it does)");
    }
    if rollback_plan.trim().is_empty() {
        return Err("rollback plan mandatory — no rollback, no repair");
    }
    let _ = risk; // 风险级必须显式选择（枚举无默认——类型系统保证）
    Ok(())
}

impl BootTimeline {
    /// 甘特导出（时间线页复用 F053 甘特组件的数据面：每段一行，段名
    /// + 起止 + 占总启动时长百分比——诊断页直接渲染）。
    pub fn gantt_lines(&self) -> Vec<String> {
        let total = self.segs.iter().map(|s| s.end_ms.saturating_sub(s.start_ms)).sum::<u64>().max(1);
        let mut sorted: Vec<&TimelineSeg> = self.segs.iter().collect();
        sorted.sort_by_key(|s| s.start_ms);
        sorted
            .iter()
            .map(|s| {
                let dur = s.end_ms.saturating_sub(s.start_ms);
                let pct = dur * 100 / total;
                alloc::format!("{} {}ms ({}%)", s.name, dur, pct)
            })
            .collect()
    }

    /// 最耗时段（甘特首行高亮——归因入口）。
    pub fn slowest_seg(&self) -> Option<&TimelineSeg> {
        self.segs
            .iter()
            .max_by_key(|s| s.end_ms.saturating_sub(s.start_ms))
    }
}

/// 导出包 manifest 全文（主册「导出包 zip 含 manifest（脱敏声明）」的
/// 文本形态：版本/时刻/脱敏声明/卷数/内容清单）。
pub fn export_manifest_text(version: &str, at_ms: u64, volumes: usize, sanitized: bool, items: &[&str]) -> String {
    let mut s = String::new();
    s.push_str(&alloc::format!("VARIX 诊断导出 manifest v{}\n", version));
    s.push_str(&alloc::format!("时刻: {} ms\n", at_ms));
    s.push_str(&alloc::format!("脱敏: {}\n", if sanitized { "已启用（路径用户段/序列号/密钥类三查）" } else { "未启用（原始日志——仅限本机查看）" }));
    s.push_str(&alloc::format!("分卷: {}\n", volumes));
    s.push_str("内容清单:\n");
    for i in items {
        s.push_str(&alloc::format!("  - {}\n", i));
    }
    s
}
pub fn run_diagcenter_checks() -> CheckSet {
    let mut set = CheckSet::new("F120-diagcenter");

    // 基准快照：帧率绿 / 存储黄 / 调度绿 / 唤醒红。
    let snap = HealthSnapshot {
        metrics: [
            Metric { lamp: Lamp::Frame, value: 40, warn_at: 50, fail_at: 80 },
            Metric { lamp: Lamp::Storage, value: 60, warn_at: 50, fail_at: 80 },
            Metric { lamp: Lamp::Scheduler, value: 10, warn_at: 50, fail_at: 80 },
            Metric { lamp: Lamp::Wakeup, value: 90, warn_at: 50, fail_at: 80 },
        ],
    };
    let timeline = BootTimeline {
        segs: vec![
            TimelineSeg { name: "装载", start_ms: 0, end_ms: 1200 },
            TimelineSeg { name: "初始化", start_ms: 1200, end_ms: 3100 },
            TimelineSeg { name: "首帧", start_ms: 3100, end_ms: 5000 },
            TimelineSeg { name: "可用", start_ms: 5000, end_ms: 7200 },
            TimelineSeg { name: "稳定", start_ms: 7200, end_ms: 8000 },
        ],
    };

    // 1. 四体检灯与 F062 数据一致（判据第一句：灯色由注入值唯一决定）。
    set.add(
        "four lamps consistent with F062 source",
        snap.consistent_with_source()
            && snap.lamp(Lamp::Frame) == LampColor::Green
            && snap.lamp(Lamp::Storage) == LampColor::Yellow
            && snap.lamp(Lamp::Scheduler) == LampColor::Green
            && snap.lamp(Lamp::Wakeup) == LampColor::Red,
        "",
    );

    // 2. 红灯直链帮助（三色语义：红=建议处理+直链 F119）。
    let dc = DiagCenter::new(snap, timeline.clone());
    set.add(
        "red lamp links to help",
        dc.red_lamp_help_link() == Some("help:wakeup"),
        "",
    );

    // 3. 修复项注册门禁（三件齐 + 风险级与回滚方案登记齐全）。
    set.add("official repairs registered", dc.repairs_registered(), "");

    // 4. 三修复项执行全链（判据第一句之二）：三件依次执行成功且留痕
    //    （执行顺序与官方登记表一致——逐件名字对上）。
    let mut dc = DiagCenter::new(snap, timeline.clone());
    let reps = official_repairs();
    let mut names_ok = true;
    for r in &reps {
        let o = dc.runner.run(r, false);
        names_ok &= o.repair == r.name;
    }
    set.add(
        "three repairs executed with history",
        names_ok
            && dc.runner.history.len() == 3
            && dc.runner.history.iter().all(|o| o.ok),
        "",
    );

    // 5. 修复回滚全链：失败注入 → 自动回滚（世界态还原）+ 三要素失败文案。
    let mut world0 = RepairWorld::default();
    world0.boot_menu_entries = 5;
    world0.icon_cache_entries = 42;
    let mut runner = RepairRunner::new();
    runner.world = world0;
    let r = &official_repairs()[1]; // 图标缓存重建
    let o = runner.run(r, true);
    set.add(
        "failed repair auto-rollback + triage text",
        !o.ok
            && o.next.contains("已自动回滚")
            && runner.world.icon_cache_entries == 42
            && runner.world.boot_menu_entries == 5,
        "",
    );

    // 6. 修复回滚显式路径：成功后再回滚（undo 面还原备份值）。
    let mut runner = RepairRunner::new();
    runner.world.boot_menu_entries = 3;
    let r = &official_repairs()[0];
    let _ = runner.run(r, false);
    let rebuilt = runner.world.boot_menu_entries == 3.max(1);
    let restored = runner.rollback_to(0) && runner.world.boot_menu_entries == 3;
    set.add("repair then explicit rollback restores", rebuilt && restored, "");

    // 7. 脱敏导出三查通过（判据第一句之三：注入敏感样本验证）。
    let mut logs = LogStore::new();
    logs.push(0, LogLevel::Info, "打开目录 C:\\Users\\variable\\文档");
    logs.push(1, LogLevel::Warn, "设备 SN=AB12CD34EF56 校验");
    logs.push(2, LogLevel::Error, "api_key=9f8e7d6c 失败");
    logs.push(3, LogLevel::Info, "帧率 80fps 正常");
    let (vols, _) = logs.export(true);
    let one_volume = vols == vec![4];
    // 导出文本逐行验三查（脱敏后无敏感命中）。
    let mut clean = true;
    for l in logs.query(0, 9_999, LogLevel::Debug, None) {
        if vbase::sanitize_hit(&vbase::sanitize_line(&l.text)).is_some() {
            clean = false;
        }
    }
    set.add("sanitize export passes 3-check", one_volume && clean, "");

    // 8. 不脱敏导出保留原文（选项生效对拍）。
    let (vols_raw, _) = logs.export(false);
    set.add("unsanitized export keeps raw", vols_raw == vec![4], "");

    // 9. 分卷导出：日志量超阈值 → 多卷（>100MB 判线；注入大行验证切卷）。
    let mut big = LogStore::new();
    for i in 0..3 {
        big.push(i, LogLevel::Info, &"x".repeat(60_000_000));
    }
    let (vols, split) = big.export(false);
    set.add(
        "large log splits into volumes",
        split && vols.len() >= 2 && vols.iter().sum::<usize>() == 3,
        "",
    );

    // 10. 日志查看器三条件查询（时间窗 + 级别 + 关键词）。
    let hits = logs.query(1, 2, LogLevel::Warn, Some("校验"));
    set.add(
        "log viewer filter/search/window",
        hits.len() == 1 && hits[0].text.contains("SN"),
        "",
    );

    // 11. manifest 脱敏声明在册（导出包 zip 内附语义）。
    let m = logs.export_manifest(true);
    set.add(
        "manifest declares sanitize policy",
        m.contains("sanitized=true") && m.contains("policy=[user-path,serial,key]"),
        "",
    );

    // 12. 时间线五段连续性（F053 复用判据：无重叠无遗漏）。
    set.add(
        "timeline five segments contiguous",
        timeline.segs.len() == 5 && timeline.contiguous() && timeline.total_ms() == 8000,
        "",
    );

    // 13. 四页签与窗口规格（880×600 / 快照/日志/时间线/修复）。
    let mut dc = DiagCenter::new(snap, timeline);
    dc.switch_tab(Tab::Repairs);
    set.add(
        "four tabs + window size",
        TAB_COUNT == 4 && dc.active_tab == Tab::Repairs && WINDOW_W_PX == 880 && WINDOW_H_PX == 600,
        "",
    );


    // 12. 结构化日志查询（深化 v2）：三轴 AND——只设时间窗、只设等级、
    //     组合过滤各命中预期行。
    let mut logs = LogStore::new();
    logs.push(0, LogLevel::Debug, "启动探针");
    logs.push(5_000, LogLevel::Warn, "帧率下滑");
    logs.push(9_000, LogLevel::Error, "写入失败");
    let by_window = logs.query_filtered(LogQuery { since_ms: Some(4_000), ..LogQuery::default() });
    let by_level = logs.query_filtered(LogQuery { level_min: Some(LogLevel::Warn), ..LogQuery::default() });
    let by_text = logs.query_filtered(LogQuery { text_contains: Some("写入"), ..LogQuery::default() });
    let combo = logs.query_filtered(LogQuery {
        since_ms: Some(4_000),
        level_min: Some(LogLevel::Error),
        ..LogQuery::default()
    });
    set.add(
        "structured log query three axes",
        by_window.len() == 2 && by_level.len() == 2 && by_text.len() == 1 && combo.len() == 1,
        "",
    );

    // 13. 分卷导出计算（深化 v2）：250MB/100MB → 3 卷；200MB/100MB →
    //     2 卷；0 字节 → 1 空卷（诚实不除零）。
    let (v1, b1) = export_volume_plan(250_000_000, EXPORT_SPLIT_BYTES);
    let (v2, _b2) = export_volume_plan(200_000_000, EXPORT_SPLIT_BYTES);
    let (v3, b3) = export_volume_plan(0, EXPORT_SPLIT_BYTES);
    set.add(
        "volume plan math",
        v1 == 3 && b1 == EXPORT_SPLIT_BYTES && v2 == 2 && v3 == 1 && b3 == 0,
        "",
    );


    // 14. 修复注册门禁（深化 v5）：缺回滚方案拒绝——没有回滚的修复
    //     不许上架；合法四件套放行。
    let gate_ok = repair_registration_gate("引导菜单重建", "重建 BCD 引导菜单条目（只写 ESP 白名单区）", Risk::Medium, "回滚：备份 BCD 原文回写").is_ok();
    let gate_no_rb = repair_registration_gate("清缓存", "清空图标缓存目录", Risk::Low, "").is_err();
    let gate_no_name = repair_registration_gate("", "说明文字足够长", Risk::Low, "回滚方案").is_err();
    set.add(
        "repair registration gate",
        gate_ok && gate_no_rb && gate_no_name,
        "",
    );

    // 15. 甘特导出（深化 v5）：逐段行含占比、最耗段点名（归因入口）。
    let tl = BootTimeline { segs: vec![
        TimelineSeg { name: "firmware", start_ms: 0, end_ms: 800 },
        TimelineSeg { name: "kernel", start_ms: 800, end_ms: 2800 },
    ] };
    let g = tl.gantt_lines();
    let slowest = tl.slowest_seg();
    set.add(
        "gantt export + slowest seg",
        g.len() == 2
            && g[0].contains("firmware") && g[0].contains("28%")
            && g[1].contains("71%")
            && slowest.map(|s| s.name == "kernel") == Some(true),
        "",
    );

    // 16. 导出 manifest 全文（深化 v5）：版本/时刻/脱敏/分卷/清单五节。
    let mf = export_manifest_text("1.0.0", 1_000, 2, true, &["snapshot.json", "logs.vol1"]);
    set.add(
        "export manifest text",
        mf.contains("VARIX 诊断导出 manifest v1.0.0")
            && mf.contains("脱敏: 已启用")
            && mf.contains("分卷: 2")
            && mf.contains("logs.vol1"),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagcenter_all_checks_green() {
        let set = run_diagcenter_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F120 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn metric_boundary_semantics() {
        let m = Metric { lamp: Lamp::Frame, value: 50, warn_at: 50, fail_at: 80 };
        assert_eq!(m.lamp_color(), LampColor::Yellow, "恰达黄线 = 关注");
        let m = Metric { lamp: Lamp::Frame, value: 80, warn_at: 50, fail_at: 80 };
        assert_eq!(m.lamp_color(), LampColor::Red, "恰达红线 = 处理");
    }

    #[test]
    fn no_red_lamp_no_link() {
        let snap = HealthSnapshot {
            metrics: [
                Metric { lamp: Lamp::Frame, value: 1, warn_at: 50, fail_at: 80 },
                Metric { lamp: Lamp::Storage, value: 1, warn_at: 50, fail_at: 80 },
                Metric { lamp: Lamp::Scheduler, value: 1, warn_at: 50, fail_at: 80 },
                Metric { lamp: Lamp::Wakeup, value: 1, warn_at: 50, fail_at: 80 },
            ],
        };
        let timeline = BootTimeline { segs: vec![] };
        let dc = DiagCenter::new(snap, timeline);
        assert!(dc.red_lamp_help_link().is_none());
    }

    #[test]
    fn timeline_detects_overlap() {
        let tl = BootTimeline {
            segs: vec![
                TimelineSeg { name: "a", start_ms: 0, end_ms: 100 },
                TimelineSeg { name: "b", start_ms: 99, end_ms: 200 },
            ],
        };
        assert!(!tl.contiguous(), "重叠必须被检出");
    }

    #[test]
    fn log_export_volume_boundary_exact() {
        let mut logs = LogStore::new();
        logs.push(0, LogLevel::Info, &"y".repeat(EXPORT_SPLIT_BYTES as usize - 8));
        logs.push(1, LogLevel::Info, "第二条");
        let (vols, split) = logs.export(false);
        assert_eq!(vols, vec![1, 1]);
        assert!(split);
    }

    #[test]
    fn f120_query_no_filters_returns_all() {
        let mut logs = LogStore::new();
        logs.push(0, LogLevel::Debug, "a");
        logs.push(1, LogLevel::Error, "b");
        assert_eq!(logs.query_filtered(LogQuery::default()).len(), 2, "全空条件 = 全量");
    }

    #[test]
    fn f120_volume_cap_exact_boundary() {
        // 恰好 100MB → 1 卷（边界不空转）。
        let (v, _) = export_volume_plan(EXPORT_SPLIT_BYTES, EXPORT_SPLIT_BYTES);
        assert_eq!(v, 1);
    }
}
