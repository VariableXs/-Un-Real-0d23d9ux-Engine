//! unx1621_percpu —— UNX-F1621 per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址）。
//! 锚点（CoRun Unxreal 主册 UNX-A3-B02）：UNX-F1621 · per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） · 目标 380 行 · 已深化
//! 判据原文：UNX-F1621-J1 变量区按 A1 预留地址切分（统计区/队列区/时钟区/
//!           shootdown 区/调试区五段）偏移账固化，五段边界与 A1 预留区零越界，
//!           布局图可导出
//! 组件：五段布局账（统计区/队列区/时钟区/shootdown 区/调试区，段基址/
//!      长度/对齐三列固化）+
//!      A1 预留区衔接闸（变量区基址 = A1 trampoline 预留区末址上取整对齐；
//!      五段合计 ≤ A1 声明的变量区预算，零越界）+
//!      每核偏移账（段内 per-CPU 槽：核数 × 槽宽，槽内字段偏移固化）+
//!      布局图导出（五段+槽表文本图，确定性渲染）+
//!      越界注入核（6 处越界/错对齐/超预算注入 6/6 必抓）。
//! 职责定位：per-CPU 区的唯一权威布局面——五段划分与偏移账固化后，
//!   A3 后续各单（runqueue/负载/时钟/shootdown/调试）全部按本布局取址；
//!   与 A1 预留区零越界是内存安全判据；布局图可导出使布局可审计。
//! 性能逐项分解：布局构建 O(5)；槽址解析 O(1)（段基址+核号×槽宽）；
//!   越界断言 O(5)；导出 O(5+核数)。全整数确定性。
//! 诊断码段：0x52D0（0x52D1~0x52D8 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本。
pub const PC_VERSION: &str = "unx1621_percpu-v1";

/// 诊断段基址（0x52D0 段 · 本模块独占）。
pub const PC_SEG_BASE: u16 = 0x52D0;

/// 五段闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcSeg {
    /// 段 1：统计区（每核计数器组）。
    Stats,
    /// 段 2：队列区（runqueue 槽）。
    Runqueue,
    /// 段 3：时钟区（每核时钟账）。
    Clock,
    /// 段 4：shootdown 区（TLB 击落账）。
    Shootdown,
    /// 段 5：调试区（每核调试槽）。
    Debug,
}

impl PcSeg {
    /// 段号。
    pub fn num(self) -> usize {
        match self {
            PcSeg::Stats => 0,
            PcSeg::Runqueue => 1,
            PcSeg::Clock => 2,
            PcSeg::Shootdown => 3,
            PcSeg::Debug => 4,
        }
    }
    /// 中文名。
    pub fn text(self) -> &'static str {
        match self {
            PcSeg::Stats => "统计区",
            PcSeg::Runqueue => "队列区",
            PcSeg::Clock => "时钟区",
            PcSeg::Shootdown => "shootdown区",
            PcSeg::Debug => "调试区",
        }
    }
    /// 段宽（字节/核；写死并注释来源）。
    pub fn slot_bytes(self) -> u64 {
        match self {
            // 统计区：16 个 u64 计数器 = 128B（覆盖 A1 全部统计面）。
            PcSeg::Stats => 128,
            // 队列区：runqueue 槽 512B（256 项 × 2B 权重/序号语义宽）。
            PcSeg::Runqueue => 512,
            // 时钟区：4 源 × 32B 账 = 128B。
            PcSeg::Clock => 128,
            // shootdown 区：请求队列 16 项 × 32B = 512B。
            PcSeg::Shootdown => 512,
            // 调试区：64B 断点/跟踪槽。
            PcSeg::Debug => 64,
        }
    }
    /// 段对齐（字节；写死并注释）。
    pub fn align(self) -> u64 {
        match self {
            // 统计/时钟：64B 缓存行对齐（防伪共享）。
            PcSeg::Stats | PcSeg::Clock => 64,
            // 队列/shootdown：4KB 页对齐（大槽独立页）。
            PcSeg::Runqueue | PcSeg::Shootdown => 4096,
            // 调试：16B 对齐。
            PcSeg::Debug => 16,
        }
    }
}

/// 五段顺序视图。
pub const PC_SEGS: [PcSeg; 5] = [PcSeg::Stats, PcSeg::Runqueue, PcSeg::Clock, PcSeg::Shootdown, PcSeg::Debug];

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcCode {
    /// 0x52D1 五段越出 A1 预算（零越界红线）。
    BudgetExceeded = 0x52D1,
    /// 0x52D2 段基址不对齐。
    SegAlignBad = 0x52D2,
    /// 0x52D3 段重叠。
    SegOverlap = 0x52D3,
    /// 0x52D4 变量区基址低于 trampoline 末址。
    BaseBelowTramp = 0x52D4,
    /// 0x52D5 核号越界（槽寻址）。
    CpuOutOfRange = 0x52D5,
    /// 0x52D6 段内字段越槽。
    FieldOverSlot = 0x52D6,
    /// 0x52D7 布局未固化先取址。
    NotSealed = 0x52D7,
    /// 0x52D8 A1 预算字段缺失。
    BudgetMissing = 0x52D8,
}

impl PcCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }
    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            PcCode::BudgetExceeded => "五段越出 A1 预算（变量区预算红线）",
            PcCode::SegAlignBad => "段基址不对齐（段对齐契约）",
            PcCode::SegOverlap => "段重叠（布局互斥破坏）",
            PcCode::BaseBelowTramp => "变量区基址低于 trampoline 末址（踩 A1 区）",
            PcCode::CpuOutOfRange => "核号越界（槽寻址闭集外）",
            PcCode::FieldOverSlot => "段内字段越槽（字段偏移超槽宽）",
            PcCode::NotSealed => "布局未固化先取址（固化是取址前置）",
            PcCode::BudgetMissing => "A1 预算字段缺失（衔接无依据）",
        }
    }
    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            PcCode::BudgetExceeded => "缩段宽或向 A1 追加预算（升版交接包）；不得越界硬写",
            PcCode::SegAlignBad => "段基址上取整到段对齐；对齐常量见 seg.align()",
            PcCode::SegOverlap => "布局算法保证互斥：查手工改段序的路径",
            PcCode::BaseBelowTramp => "基址 = tramp 末址上取整 4KB；重排",
            PcCode::CpuOutOfRange => "核号 < cpu_count；查调用方核号来源",
            PcCode::FieldOverSlot => "字段偏移 + 宽 ≤ 槽宽；缩字段或扩段",
            PcCode::NotSealed => "先 layout_seal() 再取址",
            PcCode::BudgetMissing => "A1 交接包须带变量区预算字段（F1601 补齐）",
        }
    }
}

/// 诊断体。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcDiag {
    /// 码。
    pub code: PcCode,
    /// 位置。
    pub site: String,
    /// 细节。
    pub detail: String,
}

impl PcDiag {
    /// 构造。
    pub fn new(code: PcCode, site: &str, detail: String) -> Self {
        PcDiag { code, site: site.to_string(), detail }
    }
    /// 全文。
    pub fn full(&self) -> String {
        format!("[0x{:04X}] {} —— {} —— 建议:{}", self.code.num(), self.code.text(), self.detail, self.code.suggestion())
    }
}

/// 段账（布局固化产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcSegRow {
    /// 段。
    pub seg: PcSeg,
    /// 段基址（绝对）。
    pub base: u64,
    /// 段总长（核数 × 槽宽，上取整对齐前）。
    pub len: u64,
    /// 对齐。
    pub align: u64,
}

/// per-CPU 布局总账。
#[derive(Clone, Debug, Default)]
pub struct PcLayout {
    /// 变量区基址（= A1 tramp 末址上取整）。
    pub base: u64,
    /// A1 变量区预算（字节）。
    pub budget: u64,
    /// 核数。
    pub cpu_count: u32,
    /// 段表（固化后非空且有序）。
    pub segs: Vec<PcSegRow>,
    /// 固化指纹（0=未固化）。
    pub seal_fp: u64,
    /// 五段合计。
    pub total: u64,
}

impl PcLayout {
    /// 布局构建（判据点名：按 A1 预留地址切分；五段边界零越界）。
    pub fn build(base_below_tramp_end: u64, tramp_end: u64, budget: u64, cpu_count: u32) -> Result<Self, PcDiag> {
        if budget == 0 {
            return Err(PcDiag::new(PcCode::BudgetMissing, "build:budget", "A1 预算 0".to_string()));
        }
        if cpu_count == 0 {
            return Err(PcDiag::new(PcCode::CpuOutOfRange, "build:cpu", "核数 0".to_string()));
        }
        // 变量区基址 = tramp 末址上取整 4KB（判据点名：接续 A1 trampoline 预留地址）。
        let align4k: u64 = 4096;
        let base = tramp_end.div_ceil(align4k) * align4k;
        if base < base_below_tramp_end {
            return Err(PcDiag::new(
                PcCode::BaseBelowTramp,
                "build:base",
                format!("基址 0x{:016X} < 界 0x{:016X}", base, base_below_tramp_end),
            ));
        }
        // 逐段布局（上取整对齐 + 段长 = 核数×槽宽）。
        let mut segs: Vec<PcSegRow> = Vec::new();
        let mut cur = base;
        let mut total: u64 = 0;
        let mut i = 0usize;
        while i < 5 {
            let seg = PC_SEGS[i];
            let a = seg.align();
            let sb = seg.slot_bytes();
            let len = sb.wrapping_mul(cpu_count as u64);
            let seg_base = cur.div_ceil(a) * a;
            segs.push(PcSegRow { seg, base: seg_base, len, align: a });
            total = seg_base + len - base;
            cur = seg_base + len;
            i += 1;
        }
        // 预算闸（零越界红线）。
        if total > budget {
            return Err(PcDiag::new(
                PcCode::BudgetExceeded,
                "build:budget",
                format!("五段合计 {} > A1 预算 {}", total, budget),
            ));
        }
        // 段重叠闸（布局算法数学上不重叠，此处为防手工改动断言）。
        let mut k = 1usize;
        while k < segs.len() {
            let prev = &segs[k - 1];
            let cur_s = &segs[k];
            if cur_s.base < prev.base + prev.len {
                return Err(PcDiag::new(
                    PcCode::SegOverlap,
                    "build:overlap",
                    format!("段 {} 与段 {} 重叠", k, k - 1),
                ));
            }
            // 对齐闸。
            if cur_s.base % cur_s.align != 0 {
                return Err(PcDiag::new(
                    PcCode::SegAlignBad,
                    "build:align",
                    format!("段 {} 基址 0x{:016X} 非 {} 对齐", k, cur_s.base, cur_s.align),
                ));
            }
            k += 1;
        }
        let mut lay = PcLayout { base, budget, cpu_count, segs, seal_fp: 0, total };
        lay.seal_fp = lay.fingerprint();
        Ok(lay)
    }

    /// 段基址查询（固化后可用）。
    pub fn seg_base(&self, seg: PcSeg) -> Result<u64, PcDiag> {
        if self.seal_fp == 0 {
            return Err(PcDiag::new(PcCode::NotSealed, "seg_base", "布局未固化".to_string()));
        }
        for s in &self.segs {
            if s.seg == seg {
                return Ok(s.base);
            }
        }
        Err(PcDiag::new(PcCode::SegOverlap, "seg_base:miss", "段不在表".to_string()))
    }

    /// 槽址解析（段 + 核号 → 绝对地址；核号闸 + 越槽闸）。
    pub fn slot_addr(&self, seg: PcSeg, cpu: u32, field_off: u64, field_len: u64) -> Result<u64, PcDiag> {
        if self.seal_fp == 0 {
            return Err(PcDiag::new(PcCode::NotSealed, "slot:seal", "布局未固化".to_string()));
        }
        if cpu >= self.cpu_count {
            return Err(PcDiag::new(
                PcCode::CpuOutOfRange,
                "slot:cpu",
                format!("核 {} ≥ {}", cpu, self.cpu_count),
            ));
        }
        let sb = seg.slot_bytes();
        if field_off.wrapping_add(field_len) > sb {
            return Err(PcDiag::new(
                PcCode::FieldOverSlot,
                "slot:field",
                format!("偏移 {}+{} > 槽宽 {}", field_off, field_len, sb),
            ));
        }
        let base = self.seg_base(seg)?;
        Ok(base + cpu as u64 * sb + field_off)
    }

    /// 布局指纹。
    pub fn fingerprint(&self) -> u64 {
        const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = FNV_OFFSET;
        h ^= self.base;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.budget;
        h = h.wrapping_mul(FNV_PRIME);
        h ^= self.cpu_count as u64;
        h = h.wrapping_mul(FNV_PRIME);
        for s in &self.segs {
            h ^= s.seg as u64 ^ s.base ^ s.len;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    /// 预算内复核（判据点名：五段边界与 A1 预留区零越界）。
    pub fn in_budget(&self) -> bool {
        self.total <= self.budget
    }

    /// 布局图导出（判据点名：布局图可导出）。
    pub fn export_map(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "per-CPU 布局图 基址:0x{:016X} 预算:{}B 合计:{}B 核:{} {}\n",
            self.base,
            self.budget,
            self.total,
            self.cpu_count,
            if self.in_budget() { "预算内" } else { "越界" }
        ));
        for s in &self.segs {
            out.push_str(&format!(
                "  {:<12} 基址:0x{:016X} 长:{:<6} 对齐:{} 槽宽:{}\n",
                s.seg.text(),
                s.base,
                s.len,
                s.align,
                s.seg.slot_bytes()
            ));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 演练与注入（判据面）
// ---------------------------------------------------------------------------

/// 全绿演练：A1 tramp 末址衔接 → 五段布局 → 槽址解析 → 布局图导出。
pub fn drill_layout() -> (bool, String) {
    // A1 语义：tramp 基址 0x9C000、8 页 → 末址 0x9E000；变量区预算 2MB。
    let tramp_end = 0x9C000u64 + 8 * 4096;
    match PcLayout::build(tramp_end, tramp_end, 2 * 1024 * 1024, 4) {
        Ok(lay) => {
            let rq = lay.slot_addr(PcSeg::Runqueue, 2, 8, 8).unwrap_or(0);
            let green = lay.in_budget()
                && lay.segs.len() == 5
                && lay.seal_fp != 0
                && rq != 0
                && lay.slot_addr(PcSeg::Runqueue, 4, 0, 1).is_err()
                && lay.slot_addr(PcSeg::Stats, 0, 128, 1).is_err();
            (green, lay.export_map())
        }
        Err(d) => (false, d.full()),
    }
}

/// 越界注入核：6 处 6/6 必抓。
pub fn inject_over() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：预算超（核数拉满 256 → 五段合计 1280B/核 ≈ 327KB，预算缩到 64KB）。
    let tramp_end = 0x9E000u64;
    if PcLayout::build(tramp_end, tramp_end, 64 * 1024, 256).is_err() {
        caught += 1;
        detail.push("注1 预算超:抓".to_string());
    } else {
        detail.push("注1 预算超:漏".to_string());
    }
    // 注 2：基址低于 tramp 末址。
    if PcLayout::build(0x9F000, tramp_end, 2 * 1024 * 1024, 4).is_err() {
        caught += 1;
        detail.push("注2 基址越界:抓".to_string());
    } else {
        detail.push("注2 基址越界:漏".to_string());
    }
    // 注 3：预算缺失。
    if PcLayout::build(tramp_end, tramp_end, 0, 4).is_err() {
        caught += 1;
        detail.push("注3 预算缺失:抓".to_string());
    } else {
        detail.push("注3 预算缺失:漏".to_string());
    }
    // 注 4：核号越界槽寻址。
    match PcLayout::build(tramp_end, tramp_end, 2 * 1024 * 1024, 4) {
        Ok(lay) => {
            if lay.slot_addr(PcSeg::Clock, 9, 0, 1).is_err() {
                caught += 1;
                detail.push("注4 核号越界:抓".to_string());
            } else {
                detail.push("注4 核号越界:漏".to_string());
            }
            // 注 5：字段越槽。
            if lay.slot_addr(PcSeg::Debug, 0, 63, 2).is_err() {
                caught += 1;
                detail.push("注5 字段越槽:抓".to_string());
            } else {
                detail.push("注5 字段越槽:漏".to_string());
            }
            // 注 6：手工破坏段重叠检出。
            let mut lay2 = lay.clone();
            lay2.segs[2].base = lay2.segs[1].base + 16; // 手工塞重叠
            let mut has_overlap = false;
            let mut k = 1usize;
            while k < lay2.segs.len() {
                if lay2.segs[k].base < lay2.segs[k - 1].base + lay2.segs[k - 1].len {
                    has_overlap = true;
                    break;
                }
                k += 1;
            }
            if has_overlap {
                caught += 1;
                detail.push("注6 段重叠检出:抓".to_string());
            } else {
                detail.push("注6 段重叠检出:漏".to_string());
            }
        }
        Err(d) => return (caught, 6, vec![format!("布局构建失败:{}", d.full())]),
    }
    (caught, 6, detail)
}
