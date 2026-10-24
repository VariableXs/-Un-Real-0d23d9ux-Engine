//! unx0354_aptimeline —— UNX-F0354 AP 启动时序账与零差异对接（F0203 白名单扩容）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0354 · AP 时序账与零差异对接 · 目标 240 行
//! 判据原文：UNX-F0354-J1 AP 时序字段入报告（volatile 白名单扩容
//!           走 ADR），50 次零差异复测过。
//! 组件：时序字段模型（字段名/偏移/位宽三元组，闭集管理）+
//!       volatile 白名单扩容账（新字段入白名单必须走 ADR：ADR
//!       编号 + 理由 + 双签，缺任一即拒——白名单不是自由扩的）+
//!       报告投影面（白名单字段按偏移序投影成报告行）+ 零差异
//!       复测器（同输入 50 次投影逐位一致：投影是纯函数的账面
//!       证明）+ ADR 溯源账（每个白名单字段可回查其 ADR 编号）。
//! 职责定位：AP 时序数据进入报告面的唯一合规通道——「哪些字段
//!   允许被报告」由 ADR 治理，「报告是否稳定可复现」由零差异复
//!   测证明。volatile 白名单扩容不靠口头约定靠账面，这正是
//!   F0203 白名单机制在 AP 域的落地。
//! 性能逐项分解：字段登记 O(1)；投影 O(字段数)；复测 O(50×
//!   字段数)（收口一次跑）。全整数确定性。
//! 诊断码段：0xE4B0（0xE4B1~0xE4B9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PT_VERSION: &str = "unx0354_aptimeline-v1";

/// 诊断段基址（0xE4B0 段 · 本模块独占）。
pub const PT_SEG_BASE: u16 = 0xE4B0;

/// XcCode 闭集（0xE4B1~0xE4B9）。
pub const XC_NO_ADR: u32 = 0xE4B1;
pub const XC_DUP_FIELD: u32 = 0xE4B2;
pub const XC_OFFSET_OOR: u32 = 0xE4B3;
pub const XC_WIDTH_OOR: u32 = 0xE4B4;
pub const XC_ZERO_DIFF_BROKEN: u32 = 0xE4B5;
pub const XC_PROJ_ORDER_BAD: u32 = 0xE4B6;
pub const XC_FIELD_MISS: u32 = 0xE4B7;
pub const XC_RETEST_SHORT: u32 = 0xE4B8;
pub const XC_WHITE_FULL: u32 = 0xE4B9;

/// 零差异复测次数（判据写死 50 次）。
pub const RETEST_ROUNDS: usize = 50;

/// 白名单容量。
pub const WHITELIST_CAP: usize = 32;

/// 时序字段三元组（字段名 / 报告偏移 / 位宽）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TimelineField {
    pub name: String,
    /// 报告区偏移（字节，必须 4 对齐——volatile 读对齐纪律）。
    pub offset: u32,
    /// 位宽（1/8/16/32/64 闭集）。
    pub width: u8,
    /// 入白名单的 ADR 编号（0 = 无 ADR = 拒收）。
    pub adr: u32,
}

impl TimelineField {
    /// 构造断言：偏移 4 对齐、位宽闭集、ADR 非零。
    pub fn new(name: &str, offset: u32, width: u8, adr: u32) -> Result<TimelineField, u16> {
        if adr == 0 {
            return Err(XC_NO_ADR as u16);
        }
        if offset % 4 != 0 {
            return Err(XC_OFFSET_OOR as u16);
        }
        match width {
            1 | 8 | 16 | 32 | 64 => {}
            _ => return Err(XC_WIDTH_OOR as u16),
        }
        Ok(TimelineField { name: name.to_string(), offset, width, adr })
    }
}

/// volatile 白名单（扩容走 ADR 治理）。
pub struct VolatileWhitelist {
    fields: Vec<TimelineField>,
}

impl VolatileWhitelist {
    pub fn new() -> VolatileWhitelist {
        VolatileWhitelist { fields: Vec::new() }
    }

    /// 扩容：同名字段重复拒；容量超限拒；无 ADR 拒（构造期已挡）。
    pub fn admit(&mut self, f: TimelineField) -> Result<(), u16> {
        if self.fields.iter().any(|e| e.name == f.name) {
            return Err(XC_DUP_FIELD as u16);
        }
        if self.fields.len() >= WHITELIST_CAP {
            return Err(XC_WHITE_FULL as u16);
        }
        self.fields.push(f);
        Ok(())
    }

    /// 白名单只读视图。
    pub fn fields(&self) -> &[TimelineField] {
        &self.fields
    }

    /// 报告投影：按偏移升序输出报告行（名@偏移:位宽）。
    /// 投影是纯函数——同白名单同输入必得同输出（零差异的来源）。
    pub fn project(&self) -> Result<Vec<String>, u16> {
        // 偏移排序（稳定插入排序：同偏移保持登记序——同偏移本身
        // 就是账面破，投影核会报 PROJ_ORDER_BAD）。
        let mut idx: Vec<usize> = Vec::new();
        for i in 0..self.fields.len() {
            idx.push(i);
        }
        for i in 1..idx.len() {
            let key = idx[i];
            let mut j = i;
            while j > 0 && self.fields[idx[j - 1]].offset > self.fields[key].offset {
                idx[j] = idx[j - 1];
                j -= 1;
            }
            idx[j] = key;
        }
        // 同偏移检测：两字段同偏移 = 投影歧义。
        for w in idx.windows(2) {
            if self.fields[w[0]].offset == self.fields[w[1]].offset {
                return Err(XC_PROJ_ORDER_BAD as u16);
            }
        }
        let mut out = Vec::new();
        for &i in idx.iter() {
            let f = &self.fields[i];
            out.push(format!("{}@{}:{}", f.name, f.offset, f.width));
        }
        Ok(out)
    }

    /// ADR 溯源：按字段名回查 ADR 编号。
    pub fn adr_of(&self, name: &str) -> Result<u32, u16> {
        match self.fields.iter().find(|f| f.name == name) {
            Some(f) => Ok(f.adr),
            None => Err(XC_FIELD_MISS as u16),
        }
    }
}

/// 零差异复测器：同白名单连跑 RETEST_ROUNDS 次投影，逐轮比对。
pub struct ZeroDiffRetester;

impl ZeroDiffRetester {
    pub fn retest(wl: &VolatileWhitelist) -> Result<(), u16> {
        let baseline = wl.project()?;
        for round in 1..RETEST_ROUNDS {
            let again = wl.project()?;
            if again != baseline {
                return Err(XC_ZERO_DIFF_BROKEN as u16 + round as u16);
            }
        }
        Ok(())
    }
}

/// 域自检入口：白名单治理/投影/零差异 三族。
pub fn run_unx0354_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0354-时序零差异");
    // 族一：ADR 治理闸。
    let mut wl = VolatileWhitelist::new();
    let f1 = TimelineField::new("ap_wake_tick", 0, 64, 2026_001);
    cs.add("合法字段收", f1.is_ok(), "对齐+闭集宽+ADR 齐");
    cs.add("无 ADR 拒", TimelineField::new("x", 4, 32, 0).is_err(), "白名单不自由扩");
    cs.add("失对齐拒", TimelineField::new("y", 5, 32, 1).is_err(), "偏移须 4 对齐");
    cs.add("位宽越闭集拒", TimelineField::new("z", 8, 12, 1).is_err(), "12 不在闭集");
    if let Ok(f) = f1 {
        let _ = wl.admit(f);
    }
    let f2 = TimelineField::new("ap_sentinel_tick", 8, 64, 2026_002);
    if let Ok(f) = f2 {
        let _ = wl.admit(f);
    }
    cs.add("同名字段拒", TimelineField::new("ap_wake_tick", 12, 32, 2026_003)
        .map(|f| wl.admit(f).is_err()).unwrap_or(false), "重名即拒");

    // 族二：投影（偏移升序 + 同偏移歧义检出）。
    let proj = wl.project();
    cs.add("投影升序", matches!(&proj, Ok(v) if v.len() == 2
        && v[0].contains("@0:") && v[1].contains("@8:")), "偏移序输出");
    let mut wl3 = VolatileWhitelist::new();
    if let Ok(fa) = TimelineField::new("a", 4, 32, 1) {
        let _ = wl3.admit(fa);
    }
    if let Ok(fb) = TimelineField::new("b", 4, 32, 2) {
        let _ = wl3.admit(fb);
    }
    cs.add("同偏移歧义拒", wl3.project().is_err(), "投影不许歧义");
    cs.add("ADR 溯源", wl.adr_of("ap_wake_tick") == Ok(2026_001), "按名回查 ADR");
    cs.add("溯源缺字段拒", wl.adr_of("ghost").is_err(), "不在册即拒");

    // 族三：零差异复测（50 轮全过判据）。
    cs.add("50 轮零差异", ZeroDiffRetester::retest(&wl).is_ok(), "判据 J1 复测线");
    cs.add("空白名单投影", VolatileWhitelist::new().project().map(|v| v.is_empty()).unwrap_or(false),
           "空投影合法（零字段）");
    cs
}

// ===== 扩展面：字段值快照账（投影值 + 快照防改 + 双拍一致）=====
/// 字段值快照：投影不只输出「名@偏移:位宽」结构行，还要装值。
/// 快照账把一次投影像的值序列 FNV 落档，双拍（间隔任意 tick）
/// 一致 = 报告面稳定；不一致 = volatile 读到了变化值——这正是
/// 白名单字段的语义：允许变化的是「值随时间变」，不允许的是
/// 「同拍重读不一致」。
pub struct FieldSnapshot {
    pub vals: Vec<u64>,
    pub sig: u64,
}

impl FieldSnapshot {
    /// 从白名单字段序列构造快照（读值函数由调用方注入——
    /// 本模块不做真实 MMIO 读，读函数纯化注入保持可测）。
    pub fn capture(wl: &VolatileWhitelist, reader: impl Fn(u32, u8) -> u64) -> Result<FieldSnapshot, u16> {
        let mut vals = Vec::new();
        let mut h: u64 = 0xcbf29ce484222325;
        for f in wl.fields().iter() {
            let v = reader(f.offset, f.width);
            vals.push(v);
            for &b in v.to_le_bytes().iter() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        Ok(FieldSnapshot { vals, sig: h })
    }

    /// 双拍一致：两次快照指纹相等（同拍重读一致红线）。
    pub fn double_tap_ok(a: &FieldSnapshot, b: &FieldSnapshot) -> Result<(), u16> {
        if a.vals.len() != b.vals.len() {
            return Err(1);
        }
        if a.sig != b.sig {
            return Err(2);
        }
        Ok(())
    }
}

/// 复测预算：50 轮零差异在报告预算内的占比核（每轮 O(字段数)，
/// 50 轮 × 32 字段 = 1600 次投影步，红线 4096 步）。
pub const RETEST_BUDGET_STEPS: u64 = 4096;

pub fn retest_budget_ok(fields: usize) -> Result<(), u16> {
    let steps = RETEST_ROUNDS as u64 * fields as u64;
    if steps > RETEST_BUDGET_STEPS {
        return Err(1);
    }
    Ok(())
}
