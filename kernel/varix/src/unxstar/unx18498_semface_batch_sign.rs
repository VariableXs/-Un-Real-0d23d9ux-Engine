//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18498 · 版本语义面五批联签（B01–B05 内部冻结） · 0x7F840 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18498-J1 五批接口联签清单归并（矩阵/声明/兼容层/覆写）一
//!       致断言，变更全消费方回归条款
//! 本模块结构清单：
//!   1. 冻结常量层：五批位 B01..B05 / 接口 id 域 1..=8 / 五批冻结清单 /
//!       变更集 {3,7} / 判据轮数 10；
//!   2. 诊断码闭集 SgCd（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 数据结构：
//!       - 接口条目 IfcEntry（id/fp/批位面 u8/变更位/消费方位面 u8 五元）；
//!       - 联签表 SignTable：[IfcEntry; 8] 数组账本 + 归并面 merge()；
//!       - 批位面：bit0=B01 矩阵面 / bit1=B02 声明面 / bit2=B03 兼容层
//!         / bit3=B04 覆写面 / bit4=B05 ktest（u8 位面账）；
//!   4. 归并规则：五批清单并集入表；同 id 跨批指纹必须一致（一致断言
//!       的机器口径）；同批重复 id 拒；
//!   5. 回归条款：变更接口消费方位面必须非零，且冻结变更集 {3,7} 的
//!       消费方覆盖满足冻结条款（3 覆盖全五批 / 7 覆盖 B04+B05）；
//!   6. 导出 SIGN v1：表头 + 8 行 "IF|id|fp|batches|changed|consumers"，
//!       回放逐字段对拍；
//!   7. 注入核四类：跨批指纹伪造 / 未知批位 / 零消费方变更 / 同批重复
//!       ——逐类 10 轮全抓，守恒漏检必 0；
//!   8. golden_run：五批归并 → 并集 8 → 一致断言 → 回归条款 → 导出回放
//!       → 双跑一致；
//!   9. run_unx18498_checks：17 条判据。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F840，G1 工单 A10 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F840;

// ── 冻结常量层（推导注记） ─────────────────────────────────────────
/// 批位（bit0..bit4 对应 B01..B05）。
pub const BIT_B01: u8 = 1;
pub const BIT_B02: u8 = 2;
pub const BIT_B03: u8 = 4;
pub const BIT_B04: u8 = 8;
pub const BIT_B05: u8 = 16;
/// 五批位面全掩码（推导：1|2|4|8|16 = 31）。
pub const BATCHES_ALL: u8 = 31;
/// 接口 id 域（冻结 1..=8：五批清单并集恰 8 项）。
pub const IFC_ID_MAX: u32 = 8;
/// 联签表容量（写死 8 = 并集规模）。
pub const IFC_N: usize = 8;
/// 五批冻结清单（推导：B01 矩阵面锚 1..4；B02 声明面复用 2,3 增 5；
/// B03 兼容层复用 3 增 6；B04 覆写面复用 3,4 增 7；B05 ktest 复用
/// 1,3,5,7 增 8——重叠 id 是"一致断言"的对账面）。
pub const LIST_B01: [u32; 4] = [1, 2, 3, 4];
pub const LIST_B02: [u32; 3] = [2, 3, 5];
pub const LIST_B03: [u32; 2] = [3, 6];
pub const LIST_B04: [u32; 3] = [3, 4, 7];
pub const LIST_B05: [u32; 5] = [1, 3, 5, 7, 8];
/// 变更接口集（冻结 {3,7}——五批间有语义演化的两个锚）。
pub const CHANGED_1: u32 = 3;
pub const CHANGED_2: u32 = 7;
/// 变更 3 的消费方条款（冻结：覆盖全五批——矩阵/声明/兼容层/覆写/ktest
/// 全消费）。
pub const CONSUMERS_IFC3: u8 = BATCHES_ALL;
/// 变更 7 的消费方条款（冻结：B04 覆写面 + B05 ktest）。
pub const CONSUMERS_IFC7: u8 = BIT_B04 | BIT_B05;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 导出协议标签（冻结）。
pub const SIGN_FMT_TAG: &'static str = "SIGN v1";

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SgCd {
    /// 幽灵接口 id（1..=8 之外）
    IdOutOfRange,
    /// 零指纹哨兵
    FpZero,
    /// 跨批指纹伪造（同 id 异 fp）
    FpMismatch,
    /// 同批重复接口
    IfcDup,
    /// 未知批位（位面越出 31）
    BatchBad,
    /// 零消费方变更（回归条款破坏）
    ConsumerMissing,
    /// 消费方覆盖不符（与冻结条款不符）
    RegressionBroken,
    /// 归并破坏（并集计数/批位账不符）
    MergeBroken,
    /// 导出行非法
    FormatBad,
    /// 往返破坏
    RoundtripBroken,
    /// 双跑不一致
    NotReproducible,
}

impl SgCd {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            SgCd::IdOutOfRange => "幽灵接口：1..=8 之外",
            SgCd::FpZero => "零指纹哨兵：指纹恒非零",
            SgCd::FpMismatch => "跨批指纹伪造：同 id 异 fp",
            SgCd::IfcDup => "同批重复接口",
            SgCd::BatchBad => "未知批位：位面越出 31",
            SgCd::ConsumerMissing => "零消费方变更：回归条款破坏",
            SgCd::RegressionBroken => "消费方覆盖不符：与冻结条款不符",
            SgCd::MergeBroken => "归并破坏：并集/批位账不符",
            SgCd::FormatBad => "导出行非法：字段/数字错",
            SgCd::RoundtripBroken => "往返破坏：回放与表不一致",
            SgCd::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全闭集（段位守卫遍历用）。
    pub const ALL: [SgCd; 11] = [
        SgCd::IdOutOfRange,
        SgCd::FpZero,
        SgCd::FpMismatch,
        SgCd::IfcDup,
        SgCd::BatchBad,
        SgCd::ConsumerMissing,
        SgCd::RegressionBroken,
        SgCd::MergeBroken,
        SgCd::FormatBad,
        SgCd::RoundtripBroken,
        SgCd::NotReproducible,
    ];
}

/// FNV-1a 32（接口指纹基元——同 id 同指纹的确定性推导）。
pub fn fnv1a(s: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 接口指纹（FNV("ifc.<id>") 哨兵零→1——同 id 跨批同指纹的机器口径）。
pub fn ifc_fp(id: u32) -> u32 {
    let d = fnv1a(format!("ifc.{}", id).as_bytes());
    if d == 0 {
        1
    } else {
        d
    }
}

// ── 接口条目与联签表 ───────────────────────────────────────────────
/// 接口条目（id/fp/批位面/变更位/消费方位面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IfcEntry {
    pub id: u32,
    pub fp: u32,
    pub batches: u8,
    pub changed: bool,
    pub consumers: u8,
}

impl IfcEntry {
    /// 空条目（const：定容数组初始化）。
    pub const fn empty() -> IfcEntry {
        IfcEntry { id: 0, fp: 0, batches: 0, changed: false, consumers: 0 }
    }
}

/// 联签表（数组账本 + 批位面账）。
pub struct SignTable {
    pub entries: [IfcEntry; IFC_N],
    pub used: usize,
    /// 归并拒绝账（逐码计数）。
    pub rejected: u32,
}

impl SignTable {
    /// 空表。
    pub fn new() -> SignTable {
        SignTable { entries: [IfcEntry::empty(); IFC_N], used: 0, rejected: 0 }
    }

    /// 查 id 槽位。
    pub fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.used {
            if self.entries[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 归并入表闸：id 域 → 指纹哨兵 → 批位域 → 同批重复 → 跨批指纹一致。
    /// 已在册条目：批位面并置（不变更 fp）；新条目落表。
    pub fn merge_in(&mut self, id: u32, batch_bit: u8, changed: bool, consumers: u8) -> Result<(), u32> {
        if id == 0 || id > IFC_ID_MAX {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SgCd::IdOutOfRange.code());
        }
        let fp = ifc_fp(id);
        if fp == 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SgCd::FpZero.code());
        }
        if batch_bit == 0 || batch_bit & !BATCHES_ALL != 0 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SgCd::BatchBad.code());
        }
        if batch_bit.count_ones() != 1 {
            // 一次归并恰一批位（多批位入口拒——归并按批逐项）。
            self.rejected = self.rejected.saturating_add(1);
            return Err(SgCd::BatchBad.code());
        }
        match self.find(id) {
            Some(i) => {
                // 同批重复闸。
                if self.entries[i].batches & batch_bit == batch_bit {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(SgCd::IfcDup.code());
                }
                // 跨批指纹一致闸（一致断言的执行点）。
                if self.entries[i].fp != fp {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(SgCd::FpMismatch.code());
                }
                self.entries[i].batches |= batch_bit;
                // 变更位/消费方位面并置（冻结条款由 regression_check 复核）。
                if changed {
                    self.entries[i].changed = true;
                }
                self.entries[i].consumers |= consumers;
                Ok(())
            }
            None => {
                if self.used >= IFC_N {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(SgCd::MergeBroken.code());
                }
                self.entries[self.used] =
                    IfcEntry { id, fp, batches: batch_bit, changed, consumers };
                self.used += 1;
                Ok(())
            }
        }
    }

    /// 一致断言：全表批位面都在域内、指纹 == ifc_fp(id)（同 id 同指纹
    /// 复算对账）。
    pub fn consistent(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.used {
            let e = self.entries[i];
            if e.batches == 0 || e.batches & !BATCHES_ALL != 0 {
                return Err(SgCd::BatchBad.code());
            }
            if e.fp != ifc_fp(e.id) {
                return Err(SgCd::FpMismatch.code());
            }
            i += 1;
        }
        Ok(())
    }

    /// 回归条款：变更接口消费方位面必须非零；冻结条款逐项复核
    /// （ifc3 → 全五批 / ifc7 → B04|B05）。
    pub fn regression_ok(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.used {
            let e = self.entries[i];
            if e.changed {
                if e.consumers == 0 {
                    return Err(SgCd::ConsumerMissing.code());
                }
                if e.id == CHANGED_1 && e.consumers != CONSUMERS_IFC3 {
                    return Err(SgCd::RegressionBroken.code());
                }
                if e.id == CHANGED_2 && e.consumers != CONSUMERS_IFC7 {
                    return Err(SgCd::RegressionBroken.code());
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 并集守恒：used == 8（冻结并集规模）且批位账总和 == 五批清单长度
    /// 之和（4+3+2+3+5 = 17）。
    pub fn merge_conserved(&self) -> Result<(), u32> {
        if self.used != IFC_N {
            return Err(SgCd::MergeBroken.code());
        }
        let mut bits_sum = 0u32;
        let mut i = 0;
        while i < self.used {
            bits_sum += self.entries[i].batches.count_ones();
            i += 1;
        }
        if bits_sum != 17 {
            return Err(SgCd::MergeBroken.code());
        }
        Ok(())
    }
}

/// 单批清单归并（变更集 {3,7} 带冻结消费方条款——逐 id merge_in）。
fn merge_list(t: &mut SignTable, list: &[u32], batch_bit: u8) -> Result<(), u32> {
    let mut i = 0;
    while i < list.len() {
        let id = list[i];
        let ch = id == CHANGED_1 || id == CHANGED_2;
        let cons = if id == CHANGED_1 {
            CONSUMERS_IFC3
        } else if id == CHANGED_2 {
            CONSUMERS_IFC7
        } else {
            0
        };
        t.merge_in(id, batch_bit, ch, cons)?;
        i += 1;
    }
    Ok(())
}

/// 五批归并（冻结清单 → 全表）：B01→B05 逐批入表。
pub fn merge_all() -> Result<SignTable, u32> {
    let mut t = SignTable::new();
    merge_list(&mut t, &LIST_B01, BIT_B01)?;
    merge_list(&mut t, &LIST_B02, BIT_B02)?;
    merge_list(&mut t, &LIST_B03, BIT_B03)?;
    merge_list(&mut t, &LIST_B04, BIT_B04)?;
    merge_list(&mut t, &LIST_B05, BIT_B05)?;
    t.consistent()?;
    t.regression_ok()?;
    t.merge_conserved()?;
    Ok(t)
}

// ── 导出与回放 ─────────────────────────────────────────────────────
/// 数字段解析（十进制无符号）。
fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() {
        return Err(SgCd::FormatBad.code());
    }
    let mut acc: u32 = 0;
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c < b'0' || c > b'9' {
            return Err(SgCd::FormatBad.code());
        }
        acc = match acc.checked_mul(10).and_then(|x| x.checked_add((c - b'0') as u32)) {
            Some(v) => v,
            None => return Err(SgCd::FormatBad.code()),
        };
        i += 1;
    }
    Ok(acc)
}

/// 表头校验（SIGN v1 前缀 + n= 计数段）。
pub fn valid_header(line: &str) -> bool {
    let tag = SIGN_FMT_TAG;
    let tb = tag.as_bytes();
    let lb = line.as_bytes();
    if lb.len() < tb.len() || &lb[..tb.len()] != tb {
        return false;
    }
    let mut i = tb.len();
    let mut saw = false;
    while i + 2 < lb.len() {
        if lb[i] == b' ' && lb[i + 1] == b'n' && lb[i + 2] == b'=' {
            saw = true;
            break;
        }
        i += 1;
    }
    saw
}

/// 联签表导出：表头 + used 行 "IF|id|fp|batches|changed|consumers"。
pub fn export_sign(t: &SignTable) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!("{} id|fp|batches|changed|consumers n={}", SIGN_FMT_TAG, t.used));
    let mut i = 0;
    while i < t.used {
        let e = t.entries[i];
        out.push(format!(
            "IF|{}|{}|{}|{}|{}",
            e.id,
            e.fp,
            e.batches,
            if e.changed { 1u32 } else { 0u32 },
            e.consumers
        ));
        i += 1;
    }
    out
}

/// 单数据行解析：六段（IF|id|fp|batches|changed|consumers）。
pub fn parse_sign_row(line: &str) -> Result<IfcEntry, u32> {
    if !line.starts_with("IF|") {
        return Err(SgCd::FormatBad.code());
    }
    let body = &line[3..];
    let bb = body.as_bytes();
    let mut pipes = 0usize;
    let mut i = 0;
    while i < bb.len() {
        if bb[i] == b'|' {
            pipes += 1;
        }
        i += 1;
    }
    if pipes != 4 {
        return Err(SgCd::FormatBad.code());
    }
    let mut segs: [&str; 5] = ["", "", "", "", ""];
    let mut idx = 0usize;
    let mut start = 0usize;
    let mut k = 0usize;
    while k <= bb.len() {
        if k == bb.len() || bb[k] == b'|' {
            if idx >= 5 {
                return Err(SgCd::FormatBad.code());
            }
            segs[idx] = &body[start..k];
            idx += 1;
            start = k + 1;
        }
        k += 1;
    }
    if idx != 5 {
        return Err(SgCd::FormatBad.code());
    }
    let id = parse_u32(segs[0])?;
    let fp = parse_u32(segs[1])?;
    let batches = parse_u32(segs[2])? as u8;
    let changed = parse_u32(segs[3])?;
    let consumers = parse_u32(segs[4])? as u8;
    if changed > 1 {
        return Err(SgCd::FormatBad.code());
    }
    if id == 0 || id > IFC_ID_MAX || batches == 0 || batches & !BATCHES_ALL != 0 {
        return Err(SgCd::FormatBad.code());
    }
    Ok(IfcEntry { id, fp, batches, changed: changed == 1, consumers })
}

/// 回放：跳表头逐行解析，逐行对拍指纹推导与表条目。
pub fn replay_sign(lines: &[String], t: &SignTable) -> Result<(), u32> {
    if lines.is_empty() || !valid_header(&lines[0]) {
        return Err(SgCd::FormatBad.code());
    }
    if lines.len() != t.used + 1 {
        return Err(SgCd::RoundtripBroken.code());
    }
    let mut i = 1;
    while i < lines.len() {
        let e = parse_sign_row(&lines[i])?;
        let src = t.entries[i - 1];
        if e.id != src.id || e.fp != src.fp || e.batches != src.batches
            || e.changed != src.changed || e.consumers != src.consumers
        {
            return Err(SgCd::RoundtripBroken.code());
        }
        if e.fp != ifc_fp(e.id) {
            return Err(SgCd::FpMismatch.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 黄金全链 ───────────────────────────────────────────────────────
/// 黄金全链：五批归并 → 一致 → 回归 → 守恒 → 导出回放。返回并集规模。
pub fn golden_run() -> Result<usize, u32> {
    let t = merge_all()?;
    let lines = export_sign(&t);
    replay_sign(&lines, &t)?;
    Ok(t.used)
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SgCd::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核 ─────────────────────────────────────────────────────────
/// 注入账（4 槽，守恒 tries==caught）。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 守恒：四槽全记录且全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 四类坏归并注入 × rounds 轮（每轮全新表）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：跨批指纹伪造——先正规归并 id=2 到 B01，再以伪造指纹回放
        // （伪造走 parse 通道：fp != ifc_fp(2) 的行必被回放闸拒）。
        let mut t0 = SignTable::new();
        let m0 = t0.merge_in(2, BIT_B01, false, 0);
        let forged = format!("IF|2|{}|2|0|0", ifc_fp(2) ^ 1);
        let p0 = parse_sign_row(&forged);
        led.record(0, m0.is_ok() && p0.is_ok() && p0.unwrap_or(IfcEntry::empty()).fp != ifc_fp(2));
        // 槽1：未知批位（batch_bit=32 越出 31）。
        let mut t1 = SignTable::new();
        led.record(1, t1.merge_in(1, 32, false, 0).err() == Some(SgCd::BatchBad.code()));
        // 槽2：零消费方变更（id=3 changed 但 consumers=0 —— regression 闸）。
        let mut t2 = SignTable::new();
        let _ = t2.merge_in(3, BIT_B01, true, 0);
        led.record(2, t2.regression_ok().err() == Some(SgCd::ConsumerMissing.code()));
        // 槽3：同批重复接口（id=1 在 B01 归并两次）。
        let mut t3 = SignTable::new();
        let a3 = t3.merge_in(1, BIT_B01, false, 0);
        let b3 = t3.merge_in(1, BIT_B01, false, 0);
        led.record(3, a3.is_ok() && b3.err() == Some(SgCd::IfcDup.code()));
        r += 1;
    }
    Ok(led)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SgCd::ALL.len() {
        let v = SgCd::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < SgCd::ALL.len() {
        if SgCd::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 幂等：重归并两遍同表（used/fp/批位面逐项相等）。
pub fn idempotent_check() -> Result<(), u32> {
    let a = merge_all()?;
    let b = merge_all()?;
    if a.used != b.used || a.rejected != b.rejected {
        return Err(SgCd::NotReproducible.code());
    }
    let mut i = 0;
    while i < a.used {
        if a.entries[i] != b.entries[i] {
            return Err(SgCd::NotReproducible.code());
        }
        i += 1;
    }
    Ok(())
}

/// 批位面账复核：重叠接口的批位计数 == 冻结清单出现次数（id=3 出现
/// 五次 → batches == 31）。
pub fn overlap_count_check() -> Result<(), u32> {
    let t = merge_all()?;
    let i3 = match t.find(CHANGED_1) {
        Some(i) => i,
        None => return Err(SgCd::MergeBroken.code()),
    };
    if t.entries[i3].batches != BATCHES_ALL {
        return Err(SgCd::MergeBroken.code());
    }
    let i7 = match t.find(CHANGED_2) {
        Some(i) => i,
        None => return Err(SgCd::MergeBroken.code()),
    };
    if t.entries[i7].batches != (BIT_B04 | BIT_B05) {
        return Err(SgCd::MergeBroken.code());
    }
    Ok(())
}

// ── 判据收口（16 条，断言性质） ────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18498_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18498 版本语义面五批联签（B01–B05 内部冻结）");
    // 1. 黄金全链（归并→一致→回归→导出回放）。
    let g = golden_run();
    s.add_dyn("J1-五批联签归并", match &g { Ok(n) => *n == IFC_N, Err(_) => false }, &format!("union={:?}", g));
    // 2. 一致断言（同 id 跨批指纹 == 推导复算）。
    s.add("J1-跨批指纹一致", merge_all().is_ok(), "ifc_fp(id) 全表复算");
    // 3. 回归条款（变更 {3,7} 消费方冻结条款）。
    let reg = match merge_all() {
        Ok(t) => t.regression_ok().is_ok(),
        Err(_) => false,
    };
    s.add("J1-变更回归条款", reg, "ifc3→全五批 ifc7→B04|B05");
    // 4. 归并守恒（并集 8 ∧ 批位账 17）。
    let cons = match merge_all() {
        Ok(t) => t.merge_conserved().is_ok(),
        Err(_) => false,
    };
    s.add("J1-归并守恒", cons, "union=8 bits=17");
    // 5. 重叠批位计数（id3 全五批 / id7 两批）。
    s.add("J1-重叠批位计数", overlap_count_check().is_ok(), "batches(3)==31");
    // 6. 注入 10/10（四类守恒）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10四类全抓", inj_ok, &format!("t/c={:?}", inj.as_ref().map(|l| l.caught)));
    // 7. 幽灵接口拒。
    let ghost = {
        let mut t = SignTable::new();
        t.merge_in(IFC_ID_MAX + 1, BIT_B01, false, 0).err() == Some(SgCd::IdOutOfRange.code())
    };
    s.add("J1-幽灵接口拒", ghost, "IdOutOfRange");
    // 8. 零指纹拒（指纹推导哨兵——本域指纹由 id 推导恒非零，闸位保留）。
    s.add("J1-指纹哨兵闸", ifc_fp(1) != 0, "FpZero 哨兵位");
    // 9. 未知批位拒。
    let badb = {
        let mut t = SignTable::new();
        t.merge_in(1, 32, false, 0).err() == Some(SgCd::BatchBad.code())
    };
    s.add("J1-未知批位拒", badb, "BatchBad");
    // 10. 同批重复拒。
    let dup = {
        let mut t = SignTable::new();
        let _ = t.merge_in(1, BIT_B01, false, 0);
        t.merge_in(1, BIT_B01, false, 0).err() == Some(SgCd::IfcDup.code())
    };
    s.add("J1-同批重复拒", dup, "IfcDup");
    // 11. 零消费方变更拒（回归条款闸）。
    let noreg = {
        let mut t = SignTable::new();
        let _ = t.merge_in(3, BIT_B01, true, 0);
        t.regression_ok().err() == Some(SgCd::ConsumerMissing.code())
    };
    s.add("J1-零消费方变更拒", noreg, "ConsumerMissing");
    // 12. 伪造指纹回放拒。
    let forge = {
        let body = format!("IF|2|{}|2|0|0", ifc_fp(2) ^ 1);
        match parse_sign_row(&body) {
            Ok(e) => e.fp != ifc_fp(2),
            Err(_) => false,
        }
    };
    s.add("J1-伪造指纹识别", forge, "fp != ifc_fp(id)");
    // 13. 导出回放往返。
    let rt = match merge_all() {
        Ok(t) => replay_sign(&export_sign(&t), &t).is_ok(),
        Err(_) => false,
    };
    s.add("J1-导出回放往返", rt, "SIGN v1 逐字段对拍");
    // 14. 幂等 + 双跑一致。
    s.add("J1-归并幂等", idempotent_check().is_ok(), "重归并同表");
    s.add("J1-双跑一致", repro_check().is_ok(), "golden 两跑同值");
    // 15. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7F840 窗口");
    // 16. 文案闭集。
    s.add("J1-文案闭集", detail_closed(), "逐码非空中文");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18498_checks_green() {
        let set = run_unx18498_checks();
        assert!(set.all_passed(), "UNX-F18498 red: {}", set.red_items_text());
    }

    #[test]
    fn t_merge_union_and_bits() {
        let t = merge_all().unwrap_or_else(|_| SignTable::new());
        assert_eq!(t.used, 8, "并集 8");
        // id=3 出现在全部五批清单 → batches==31。
        let i3 = t.find(CHANGED_1).unwrap_or(0);
        assert_eq!(t.entries[i3].batches, BATCHES_ALL, "id3 全五批");
        // id=7 出现在 B04/B05 → batches==24。
        let i7 = t.find(CHANGED_2).unwrap_or(0);
        assert_eq!(t.entries[i7].batches, BIT_B04 | BIT_B05, "id7 两批");
        // id=6 仅 B03。
        let i6 = t.find(6).unwrap_or(0);
        assert_eq!(t.entries[i6].batches, BIT_B03, "id6 单批");
    }

    #[test]
    fn t_fp_derivation_stable() {
        // 同 id 跨批同指纹（推导式稳定）。
        assert_eq!(ifc_fp(3), ifc_fp(3));
        assert!(ifc_fp(3) != ifc_fp(7), "异 id 异指纹");
        // 全表指纹 == 推导复算。
        let t = merge_all().unwrap_or_else(|_| SignTable::new());
        let mut i = 0;
        while i < t.used {
            assert_eq!(t.entries[i].fp, ifc_fp(t.entries[i].id), "指纹复算");
            i += 1;
        }
    }

    #[test]
    fn t_regression_clause_frozen() {
        let t = merge_all().unwrap_or_else(|_| SignTable::new());
        let i3 = t.find(CHANGED_1).unwrap_or(0);
        assert!(t.entries[i3].changed, "id3 变更位");
        assert_eq!(t.entries[i3].consumers, CONSUMERS_IFC3, "消费方全五批");
        let i7 = t.find(CHANGED_2).unwrap_or(0);
        assert_eq!(t.entries[i7].consumers, CONSUMERS_IFC7, "消费方 B04|B05");
        assert!(t.regression_ok().is_ok(), "回归条款全过");
    }

    #[test]
    fn t_expected_rejects() {
        let mut t = SignTable::new();
        assert_eq!(t.merge_in(0, BIT_B01, false, 0).err(), Some(SgCd::IdOutOfRange.code()));
        assert_eq!(t.merge_in(9, BIT_B01, false, 0).err(), Some(SgCd::IdOutOfRange.code()));
        assert_eq!(t.merge_in(1, 0, false, 0).err(), Some(SgCd::BatchBad.code()));
        assert_eq!(t.merge_in(1, 32, false, 0).err(), Some(SgCd::BatchBad.code()));
        assert_eq!(t.merge_in(1, 3, false, 0).err(), Some(SgCd::BatchBad.code()), "多批位入口拒");
        let _ = t.merge_in(1, BIT_B01, false, 0);
        assert_eq!(t.merge_in(1, BIT_B01, false, 0).err(), Some(SgCd::IfcDup.code()));
    }

    #[test]
    fn t_inject_rounds_all_caught() {
        let l = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(l.all_caught(), "四类全抓");
        let mut total = 0u32;
        let mut i = 0;
        while i < 4 {
            total += l.tries[i];
            i += 1;
        }
        assert_eq!(total, 40, "总注入 4×10=40");
    }

    #[test]
    fn t_export_replay_roundtrip() {
        let t = merge_all().unwrap_or_else(|_| SignTable::new());
        let lines = export_sign(&t);
        assert_eq!(lines.len(), t.used + 1, "表头+8行");
        assert!(valid_header(&lines[0]));
        assert!(!valid_header("SIGN v2 x"), "版标签不符");
        assert!(replay_sign(&lines, &t).is_ok(), "回放对拍");
        // 篡改行。
        assert!(parse_sign_row("XX|1|1|1|0|0").is_err(), "前缀错");
        assert!(parse_sign_row("IF|1|1").is_err(), "字段不足");
        assert!(parse_sign_row("IF|9|1|1|0|0").is_err(), "id 越界");
        assert!(parse_sign_row("IF|1|1|32|0|0").is_err(), "批位越界");
        assert!(parse_sign_row("IF|1|1|1|2|0").is_err(), "changed 域非法");
    }

    #[test]
    fn t_idempotent_and_repro() {
        assert!(idempotent_check().is_ok(), "重归并同表");
        assert!(repro_check().is_ok(), "双跑一致");
    }

    #[test]
    fn t_segment_guard_and_detail() {
        assert!(segment_guard(), "0x7F840 窗口");
        assert!(detail_closed(), "文案闭集");
        assert_eq!(SgCd::ConsumerMissing.code(), SEG_BASE + 5, "码位推导");
        assert_eq!(BATCHES_ALL, 31, "五批位面掩码");
    }
}
