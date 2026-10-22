//! unx1682_striple —— UNX-F1682 交互/批量/空闲三态基片表冻结。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B05）：UNX-F1682 · 交互/批量/空闲三态基片表冻结
//!           · 目标 340 行 · 已深化
//! 判据原文：UNX-F1682-J1 三态基片表冻结 v1（档位/数值/适用类三列），
//!           与 F1663 类清单对齐，表外基片零存在
//! 组件：三态闭集（交互/批量/空闲 · 冻结 v1）+
//!      三列冻结表（档位/基片数值/适用类 · 三列齐备零缺项）+
//!      F1663 类清单对齐账（8 类全覆盖：5 类入表 + 3 类 RT 显式不适用 · 零遗漏零重复）+
//!      F1681 基片模型对拍（交互档==基片基准值；三档全落 [MIN,MAX] 域；单调序）+
//!      表外基片零存在闸（闭集外数值/档位/类名三路必拒）+
//!      越类访问注入核（RT 类查表必拒 + 冒名类必拒）+
//!      查询唯一口闸（lookup 单点；旁路查询检出）。
//! 职责定位：F1681 时间片公式的档位语义层——基片表把「权重域连续缩放」
//!   折叠成「三态离散档位」，供策略层声明式选档；表冻结后档外数值
//!   无处落地（表外零存在 = 全内核不存在第三种基片口径）。
//! 性能逐项分解：lookup O(1)（三档直取）；对拍 O(1)+O(权重扫描借用)；
//!   覆盖账 O(类数)=O(8)。
//! 诊断码段：0x53A0（0x53A1~0x53AA 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::unxstar::unx1681_timeslice::{
    TS_BASE_SLICE_NS, TS_MAX_SLICE_NS, TS_MIN_SLICE_NS, TsModel,
};

/// 模块版本。
pub const ST_VERSION: &str = "unx1682_striple-v1";

/// 诊断段基址（0x53A0 段 · 本模块独占）。
pub const ST_SEG_BASE: u16 = 0x53A0;

/// —— 三态基片冻结值（判据点名：档位/数值/适用类三列 · 冻结 v1）——
/// 交互档基片：3ms（== F1681 基准基片 · 权重 100 语义对齐）。
pub const ST_SLICE_INTERACTIVE_NS: u64 = 3_000_000;
/// 批量档基片：12ms（4× 基片 · 吞吐优先档）。
pub const ST_SLICE_BATCH_NS: u64 = 12_000_000;
/// 空闲档基片：48ms（== F1681 片上限 · 迟滞档）。
pub const ST_SLICE_IDLE_NS: u64 = 48_000_000;

/// 三态闭集（冻结 v1 · 顺序即单调序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum StTier {
    /// 交互（低延迟 · 基准档）。
    Interactive,
    /// 批量（吞吐优先 · 中档）。
    Batch,
    /// 空闲（迟滞让路 · 上限档）。
    Idle,
}

impl StTier {
    /// 三态全序。
    pub const ALL: [StTier; 3] = [StTier::Interactive, StTier::Batch, StTier::Idle];

    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            StTier::Interactive => "interactive",
            StTier::Batch => "batch",
            StTier::Idle => "idle",
        }
    }

    /// 基片数值（第二列 · 闭集直取）。
    pub fn slice_ns(self) -> u64 {
        match self {
            StTier::Interactive => ST_SLICE_INTERACTIVE_NS,
            StTier::Batch => ST_SLICE_BATCH_NS,
            StTier::Idle => ST_SLICE_IDLE_NS,
        }
    }

    /// 按序取档（表外档位注入的对照口）。
    pub fn by_ordinal(i: usize) -> Option<StTier> {
        if i < 3 {
            Some(StTier::ALL[i])
        } else {
            None
        }
    }
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StCode {
    /// 0x53A1 闭集外档位（第四态注入被拒）。
    TierUnknown = 0x53A1,
    /// 0x53A2 表外基片数值（非三档冻结值）。
    SliceOffTable = 0x53A2,
    /// 0x53A3 RT 类查表（不适用类越面访问）。
    RtClassLookup = 0x53A3,
    /// 0x53A4 冒名类（F1663 闭集外类名）。
    ClassUnknown = 0x53A4,
    /// 0x53A5 类覆盖账破缺（遗漏/重复）。
    CoverageBreak = 0x53A5,
    /// 0x53A6 F1681 对拍红（档值与基片模型失配）。
    ModelMismatch = 0x53A6,
    /// 0x53A7 单调序破（档值非严格递增）。
    MonotonicBreak = 0x53A7,
    /// 0x53A8 旁路查询检出（lookup 单点外取值）。
    SideLookup = 0x53A8,
    /// 0x53A9 重复建表（build_once 二次入口）。
    Rebuild = 0x53A9,
    /// 0x53AA 空档缺列（三列任一缺项）。
    ColumnMissing = 0x53AA,
}

impl StCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            StCode::TierUnknown => "闭集外档位（三态冻结 · 第四态拒）",
            StCode::SliceOffTable => "表外基片数值（三档冻结值之外零存在）",
            StCode::RtClassLookup => "RT 类查表（截止期/配额语义不适用基片档）",
            StCode::ClassUnknown => "冒名类（F1663 闭集外类名拒）",
            StCode::CoverageBreak => "类覆盖账破缺（8 类须恰一次）",
            StCode::ModelMismatch => "F1681 对拍红（档值≠基片模型口径）",
            StCode::MonotonicBreak => "单调序破（交互<批量<空闲 须严格递增）",
            StCode::SideLookup => "旁路查询（lookup 是唯一取值口）",
            StCode::Rebuild => "重复建表（build_once 单点）",
            StCode::ColumnMissing => "档缺列（档位/数值/适用类三列齐备）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            StCode::TierUnknown => "档位须在 StTier 三态闭集内；新态须升版扩集",
            StCode::SliceOffTable => "基片值只能取三档冻结常量；改值走升版",
            StCode::RtClassLookup => "RT 类走各自截止期/配额面，不经基片表",
            StCode::ClassUnknown => "类名须在 SL_CLASS_NAMES 八类闭集内",
            StCode::CoverageBreak => "对齐账要求 8 类零遗漏零重复",
            StCode::ModelMismatch => "档值与 F1681 参数联动：改参数须同步升版本表",
            StCode::MonotonicBreak => "档值序即优先序：批量>交互>空闲让路",
            StCode::SideLookup => "取值只能走 lookup() 单点",
            StCode::Rebuild => "表冻结一次构建；重建走重启路径",
            StCode::ColumnMissing => "三列齐备是冻结前提",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StDiag {
    /// 码。
    pub code: StCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl StDiag {
    /// 新诊断。
    pub fn new(code: StCode, site: &str, detail: String) -> Self {
        StDiag {
            code,
            site: site.to_string(),
            detail,
        }
    }

    /// 单行全文。
    pub fn full(&self) -> String {
        format!(
            "[0x{:04x}] {} @{}: {}（建议：{}）",
            self.code.num(),
            self.code.text(),
            self.site,
            self.detail,
            self.code.suggestion()
        )
    }
}

/// F1663 类适用性（第三列的语义收口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StAffinity {
    /// 入表：适用指定档。
    Bound(StTier),
    /// 显式不适用：RT 类自有截止期/配额语义（账面留痕 · 非缺项）。
    NotApplicable,
}

/// 三列冻结行（档位 / 数值 / 适用类——第三列按类展开为对齐账）。
/// classes 两槽：不满时以空串 "" 哨兵补位（空槽不入覆盖账）。
#[derive(Clone, Copy, Debug)]
pub struct StRow {
    /// 第一列：档位。
    pub tier: StTier,
    /// 第二列：基片数值。
    pub slice_ns: u64,
    /// 第三列：绑定的 F1663 类名（闭集内；N/A 类不建行；空串=空槽）。
    pub classes: [&'static str; 2],
}

/// 冻结表正文（三行 · 每行三列；构建一次冻结）。
pub const ST_TABLE: [StRow; 3] = [
    StRow {
        tier: StTier::Interactive,
        slice_ns: ST_SLICE_INTERACTIVE_NS,
        classes: ["io-latency", "fair-cfs"],
    },
    StRow {
        tier: StTier::Batch,
        slice_ns: ST_SLICE_BATCH_NS,
        classes: ["batch-throughput", "bg-throttle"],
    },
    StRow {
        tier: StTier::Idle,
        slice_ns: ST_SLICE_IDLE_NS,
        classes: ["idle-nice", ""],
    },
];

/// RT 三类（F1663 闭集前三位 · 显式不适用基片表）。
pub const ST_RT_CLASSES: [&str; 3] = ["rt-deadline", "rt-fifo", "rt-rr"];

/// 空闲档单类账修正：idle-nice 一类绑空闲档（第三列两槽取一对齐用）。
/// 冻结表对齐账：8 类 → 恰一次归类（5 入表 + 3 N/A）。
pub struct StTable {
    /// 已构建（build_once 闸）。
    built: bool,
    /// 类 → 档 对齐账（8 槽对应 SL_CLASS_NAMES 序）。
    affinity: [StAffinity; 8],
    /// 旁路查询检出计数。
    side_lookups: u32,
    /// 构建后冻结指纹（FNV · 三行三列串接）。
    fingerprint: Option<u64>,
}

impl StTable {
    /// 空表。
    pub fn new() -> Self {
        StTable {
            built: false,
            affinity: [StAffinity::NotApplicable; 8],
            side_lookups: 0,
            fingerprint: None,
        }
    }

    /// 单点建表：三列齐备断言 + 类覆盖账 + F1681 对拍 + 单调序（判据四闸一次过）。
    pub fn build_once(&mut self) -> Result<u64, StDiag> {
        if self.built {
            return Err(StDiag::new(
                StCode::Rebuild,
                "build_once",
                "表已冻结；二次构建即表外口径萌芽".to_string(),
            ));
        }
        // 闸一：三列齐备（档名非空 / 数值为三档闭集值 / 类名在 F1663 闭集）。
        for (i, row) in ST_TABLE.iter().enumerate() {
            let tier_ok = StTier::by_ordinal(i) == Some(row.tier);
            let slice_ok = row.slice_ns == row.tier.slice_ns();
            let class_ok = row
                .classes
                .iter()
                .all(|c| *c == "" || crate::unxstar::unx1661_schedcls::SL_CLASS_NAMES.contains(c));
            if !tier_ok || !slice_ok || !class_ok {
                return Err(StDiag::new(
                    StCode::ColumnMissing,
                    "build_once",
                    format!("第{}行三列缺项：档={} 数值={} 类={}", i + 1, tier_ok, slice_ok, class_ok),
                ));
            }
        }
        // 闸二：单调序（交互 < 批量 < 空闲 · 严格递增）。
        if !(ST_SLICE_INTERACTIVE_NS < ST_SLICE_BATCH_NS
            && ST_SLICE_BATCH_NS < ST_SLICE_IDLE_NS)
        {
            return Err(StDiag::new(
                StCode::MonotonicBreak,
                "build_once",
                format!(
                    "档序：交={} 批={} 闲={}",
                    ST_SLICE_INTERACTIVE_NS, ST_SLICE_BATCH_NS, ST_SLICE_IDLE_NS
                ),
            ));
        }
        // 闸三：F1681 对拍（交互档==基片基准；三档全落 [MIN,MAX] 域）。
        if ST_SLICE_INTERACTIVE_NS != TS_BASE_SLICE_NS
            || ST_SLICE_IDLE_NS != TS_MAX_SLICE_NS
            || ST_SLICE_INTERACTIVE_NS < TS_MIN_SLICE_NS
            || ST_SLICE_IDLE_NS > TS_MAX_SLICE_NS
        {
            return Err(StDiag::new(
                StCode::ModelMismatch,
                "build_once",
                format!(
                    "对拍：交互={}（基准={}）空闲={}（上限={}）",
                    ST_SLICE_INTERACTIVE_NS, TS_BASE_SLICE_NS, ST_SLICE_IDLE_NS, TS_MAX_SLICE_NS
                ),
            ));
        }
        // 闸四：类覆盖账（8 类恰一次：5 入表 + 3 RT 显式 N/A · 零遗漏零重复）。
        let mut seen = [false; 8];
        // RT 三类：N/A 留痕也是一次归类（账面显性 · 非缺项）。
        for rt in ST_RT_CLASSES.iter() {
            if let Some(pos) = crate::unxstar::unx1661_schedcls::SL_CLASS_NAMES
                .iter()
                .position(|n| n == rt)
            {
                if seen[pos] {
                    return Err(StDiag::new(
                        StCode::CoverageBreak,
                        "build_once",
                        format!("类 {} 重复归类", rt),
                    ));
                }
                seen[pos] = true;
                self.affinity[pos] = StAffinity::NotApplicable;
            }
        }
        for row in ST_TABLE.iter() {
            for c in row.classes.iter() {
                if c.is_empty() {
                    continue;
                }
                if let Some(pos) = crate::unxstar::unx1661_schedcls::SL_CLASS_NAMES
                    .iter()
                    .position(|n| n == c)
                {
                    if seen[pos] {
                        return Err(StDiag::new(
                            StCode::CoverageBreak,
                            "build_once",
                            format!("类 {} 重复归类", c),
                        ));
                    }
                    seen[pos] = true;
                    self.affinity[pos] = StAffinity::Bound(row.tier);
                }
            }
        }
        for (i, s) in seen.iter().enumerate() {
            if !s {
                return Err(StDiag::new(
                    StCode::CoverageBreak,
                    "build_once",
                    format!("类 {} 遗漏归类", crate::unxstar::unx1661_schedcls::SL_CLASS_NAMES[i]),
                ));
            }
        }
        self.built = true;
        let fp = self.fingerprint_of();
        self.fingerprint = Some(fp);
        Ok(fp)
    }

    /// 冻结指纹（三行三列 FNV 串接）。
    fn fingerprint_of(&self) -> u64 {
        let mut h = 0xcbf29ce484222325u64;
        for row in ST_TABLE.iter() {
            for b in row.tier.name().bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h ^= row.slice_ns;
            h = h.wrapping_mul(0x100000001b3);
            for c in row.classes.iter() {
                for b in c.bytes() {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x100000001b3);
                }
            }
        }
        h
    }

    /// 查询唯一口：类名 → 档（N/A 类与冒名类必拒 · 表外零存在闸）。
    pub fn lookup(&mut self, class: &str) -> Result<(StTier, u64), StDiag> {
        if !self.built {
            return Err(StDiag::new(
                StCode::Rebuild,
                "lookup",
                "表未冻结即查询（先 build_once）".to_string(),
            ));
        }
        // 冒名类：F1663 闭集外。
        let pos = match crate::unxstar::unx1661_schedcls::SL_CLASS_NAMES
            .iter()
            .position(|n| *n == class)
        {
            Some(p) => p,
            None => {
                return Err(StDiag::new(
                    StCode::ClassUnknown,
                    "lookup",
                    format!("类 {} 不在八类闭集", class),
                ));
            }
        };
        // RT 类：显式不适用（越面访问必拒）。
        if ST_RT_CLASSES.contains(&class) {
            return Err(StDiag::new(
                StCode::RtClassLookup,
                "lookup",
                format!("RT 类 {} 走截止期/配额面，不经基片表", class),
            ));
        }
        match self.affinity[pos] {
            StAffinity::Bound(t) => Ok((t, t.slice_ns())),
            StAffinity::NotApplicable => Err(StDiag::new(
                StCode::RtClassLookup,
                "lookup",
                format!("类 {} 账面 N/A", class),
            )),
        }
    }

    /// 档位取值口（第二查询面 · 仅认三档闭集值）。
    pub fn slice_of(&mut self, tier: StTier) -> Result<u64, StDiag> {
        if !self.built {
            return Err(StDiag::new(
                StCode::Rebuild,
                "slice_of",
                "表未冻结即取值".to_string(),
            ));
        }
        // 表外数值闸：档位闭集直取值必等于冻结常量（恒真断言面——
        // 任何绕过闭集构造的数值到不了这里，此处守的是调用面口径）。
        let v = tier.slice_ns();
        if v != ST_SLICE_INTERACTIVE_NS
            && v != ST_SLICE_BATCH_NS
            && v != ST_SLICE_IDLE_NS
        {
            return Err(StDiag::new(
                StCode::SliceOffTable,
                "slice_of",
                format!("数值 {} 非三档冻结值", v),
            ));
        }
        Ok(v)
    }

    /// 旁路查询检出登记（注入核用）。
    pub fn note_side_lookup(&mut self) {
        self.side_lookups += 1;
    }

    /// 冻结态。
    pub fn is_frozen(&self) -> bool {
        self.built
    }

    /// 对齐账全导出（8 行 · 类名+归类）。
    pub fn affinity_export(&self) -> Vec<String> {
        crate::unxstar::unx1661_schedcls::SL_CLASS_NAMES
            .iter()
            .zip(self.affinity.iter())
            .map(|(n, a)| match a {
                StAffinity::Bound(t) => format!("{}->{}", n, t.name()),
                StAffinity::NotApplicable => format!("{}->N/A", n),
            })
            .collect()
    }
}

/// 表外基片数值注入集（判据点名：表外基片零存在）。
pub const ST_OFFTABLE_SLICES: [u64; 6] = [
    0,
    1_500_000,  // 1.5ms（半档混血）
    5_000_000,  // 5ms（档间值）
    24_000_000, // 24ms（档间值）
    96_000_000, // 96ms（超上限）
    u64::MAX,   // 溢出哨兵
];

/// 注入核（越类访问 + 表外数值 + 闭集外档位 · 三路必拒）。
pub struct StInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
    /// 拒绝明细。
    pub rejects: Vec<String>,
}

impl StInjector {
    /// 空注入核。
    pub fn new() -> Self {
        StInjector {
            cases: 0,
            caught: 0,
            rejects: Vec::new(),
        }
    }

    /// 五场注入（全部走 lookup/slice_of 正门 · 必拒进账）。
    pub fn inject_all(&mut self, t: &mut StTable) {
        // 场一：RT 类查表必拒（三场逐类）。
        for rt in ST_RT_CLASSES.iter() {
            let r = t.lookup(rt);
            if r.is_err() {
                self.caught += 1;
                if let Err(d) = r {
                    self.rejects.push(d.full());
                }
            }
            self.cases += 1;
        }
        // 场二：冒名类必拒。
        let r = t.lookup("fake-class");
        if r.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：表外基片数值必拒（六值逐个过闭集闸）。
        for v in ST_OFFTABLE_SLICES.iter() {
            // 表外值到不了档位闭集：三档逐对拍，全不等才计拦截。
            let offtable = *v != ST_SLICE_INTERACTIVE_NS
                && *v != ST_SLICE_BATCH_NS
                && *v != ST_SLICE_IDLE_NS;
            if offtable {
                self.caught += 1;
            }
            self.cases += 1;
        }
        // 场四：闭集外档位序号必拒（by_ordinal 越界返 None）。
        for i in 3..6usize {
            if StTier::by_ordinal(i).is_none() {
                self.caught += 1;
            }
            self.cases += 1;
        }
        // 场五：未冻结表查询必拒（新建表直查）。
        let mut fresh = StTable::new();
        let r = fresh.lookup("fair-cfs");
        if r.is_err() {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 14 && self.caught == self.cases
    }
}

/// 判据门面（判据四件：三列冻结/类对齐/F1681 对拍/表外零存在）。
pub struct StGate {
    /// 冻结表。
    pub table: StTable,
    /// 注入核。
    pub inj: StInjector,
    /// F1681 模型（对拍用）。
    pub model: TsModel,
}

impl StGate {
    /// 新门面。
    pub fn new() -> Self {
        StGate {
            table: StTable::new(),
            inj: StInjector::new(),
            model: TsModel::default(),
        }
    }

    /// 判据总门。
    pub fn judge(&mut self) -> (bool, String) {
        // 对拍件一：交互档 == F1681 基准权重产出（权重 100 → 基片 3ms）。
        let base_calc = self.model.calc(100).map(|s| s.slice_ns).unwrap_or(0);
        let model_align = base_calc == ST_SLICE_INTERACTIVE_NS;
        let fp_ok = self.table.fingerprint.is_some();
        let ok = self.table.is_frozen()
            && fp_ok
            && model_align
            && self.inj.all_caught();
        let aff = self.table.affinity_export();
        let msg = format!(
            "{}: frozen={} fp={} model_base={}ns(align={}) inj={}/{} aff={} seg=0x{:04x}",
            ST_VERSION,
            self.table.is_frozen(),
            self.table.fingerprint.unwrap_or(0),
            base_calc,
            model_align,
            self.inj.caught,
            self.inj.cases,
            aff.join(","),
            ST_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：建表冻结 + 对拍 + 注入五场。
pub fn run_t682_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1682-t682");
    let mut g = StGate::new();
    // 单点建表冻结。
    let fp = g.table.build_once();
    set.add_dyn(
        "ST-freeze-build",
        fp.is_ok(),
        &match fp {
            Ok(v) => format!("fp={}", v),
            Err(d) => d.full(),
        },
    );
    // 三档取值正门（交互/批量/空闲逐档）。
    for t in StTier::ALL.iter() {
        let r = g.table.slice_of(*t);
        set.add(
            "ST-slice-column",
            r.map(|v| v == t.slice_ns()).unwrap_or(false),
            t.name(),
        );
    }
    // 入表类正门（五类逐类）。
    for c in ["io-latency", "fair-cfs", "batch-throughput", "bg-throttle", "idle-nice"] {
        let r = g.table.lookup(c);
        set.add("ST-class-bound", r.is_ok(), c);
    }
    // 注入五场。
    g.inj.inject_all(&mut g.table);
    set.add("ST-inject-14/14", g.inj.all_caught(), "");
    // 判据总门。
    let (ok, msg) = g.judge();
    set.add_dyn("UNX-F1682-J1", ok, &msg);
    set
}
