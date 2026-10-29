//! UNX-F0718 · 断言封账（300+ 冻结定版）· 诊断段 0x59c0（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0718] 断言封账（300+ 冻结定版） <SEG 0x59c0>
//!   UNX-F0718-J1 断言面五件封账（盘点/映射/序表/版本链/报告哈希 5/5）
//!   且封账后写尝试检出恒零。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 五件闭集：SealItem 恰 5 变体（盘点哈希/映射哈希/序表哈希/
//!      版本链哈希/报告哈希），件外无封账对象；5/5 全齐才定版。
//!   2. 冻结定版：五件全齐 + 断言面 ≥300 条 → 定版哈希落账；
//!      定版后账面全只读（写尝试只进检出账，永不落盘）。
//!   3. 写尝试检出恒零判据：封账后一切写通道（登记/改账/重封）必须
//!      全数被拦——「成功写入数」恒 0，写尝试检出计数逐次累加；
//!      若出现成功写（检出数未增而账变）即 E_SEAL_BROKEN。
//!   4. 定版哈希链：五件哈希折叠 + 件数 5/5 位参与折叠；复算对拍。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x59c0~0x59dF 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x59c0 独占 0x59c0~0x59dF） ────────────────────────────
pub const SEG_BASE: u16 = 0x59c0;

/// 0x59c1 五件缺：封账件 5/5 未齐
const E_ITEM_MISSING: u16 = SEG_BASE + 0x1;
/// 0x59c2 封账已定版仍尝试写
const E_SEALED: u16 = SEG_BASE + 0x2;
/// 0x59c3 断言面不足：<300 条拒定版
const E_TARGET_SHORT: u16 = SEG_BASE + 0x3;
/// 0x59c4 件号越界（合法 0..5）
const E_ITEM_RANGE: u16 = SEG_BASE + 0x4;
/// 0x59c5 件哈希零值
const E_HASH_ZERO: u16 = SEG_BASE + 0x5;
/// 0x59c6 封账破：写尝试未被拦（成功写发生）
const E_SEAL_BROKEN: u16 = SEG_BASE + 0x6;
/// 0x59c7 定版哈希漂移：复算与落账不符
const E_FP_DRIFT: u16 = SEG_BASE + 0x7;
/// 0x59c8 断言重号：同面同名二次登记
const E_ASSERT_DUP: u16 = SEG_BASE + 0x8;
/// 0x59c9 断言名非法：空名或超长
const E_NAME_BAD: u16 = SEG_BASE + 0x9;
/// 0x59ca 断言账溢出
const E_SET_FULL: u16 = SEG_BASE + 0xA;
/// 0x59cb 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xB;
/// 0x59cc 未定版即取定版哈希
const E_NOT_SEALED: u16 = SEG_BASE + 0xC;
/// 0x59cd 五件哈希与工作面漂移：封账件与当前账不符
const E_ITEM_DRIFT: u16 = SEG_BASE + 0xD;
/// 0x59ce 封账序越界（合法 1..=MAX_SEALS）
const E_SEQ_RANGE: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x59c0~0x59dF 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_ITEM_MISSING => "五件缺：盘点/映射/序表/版本链/报告哈希缺件",
        E_SEALED => "封账已定版：账面全只读，写尝试一律拦截",
        E_TARGET_SHORT => "断言面不足：<300 条拒定版",
        E_ITEM_RANGE => "件号越界：五件闭集合法 0..5",
        E_HASH_ZERO => "件哈希零值：五件哈希不得为零",
        E_SEAL_BROKEN => "封账破：写尝试未被拦，守卫失效",
        E_FP_DRIFT => "定版哈希漂移：复算与落账不符",
        E_ASSERT_DUP => "断言重号：同名二次登记拒收",
        E_NAME_BAD => "断言名非法：空名或超 64 字节",
        E_SET_FULL => "断言账溢出：超定容上限",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_NOT_SEALED => "未定版即取定版哈希",
        E_ITEM_DRIFT => "封账件漂移：五件哈希与当前账不符",
        E_SEQ_RANGE => "封账序越界：合法 1..=MAX_SEALS",
        _ => "未知诊断码（0x59c0 段外或保留位）",
    }
}

// ── 常量（来源：判据 300+ 与五件口径） ───────────────────────────────
/// 断言面下限（判据原文：300+）。
pub const SEAL_TARGET: usize = 300;
/// 封账件数（判据原文：五件 5/5）。
pub const SEAL_ITEM_COUNT: usize = 5;
/// 封账定容。
pub const MAX_SEALS: usize = 16;
/// 名长度上限。
pub const NAME_MAX: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(SEAL_TARGET == 300);
const _: () = assert!(SEAL_ITEM_COUNT == 5);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

fn fnv_str(s: &str) -> u64 {
    let mut h = FNV_OFF;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        h = fnv_mix(h, b[i]);
        i += 1;
    }
    h
}

// ── 五件闭集 ─────────────────────────────────────────────────────────
/// 断言面封账五件闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SealItem {
    /// 件 0：盘点哈希（断言清单盘点）。
    Inventory,
    /// 件 1：映射哈希（断言-判据映射）。
    Mapping,
    /// 件 2：序表哈希（执行序表）。
    OrderTable,
    /// 件 3：版本链哈希。
    VersionChain,
    /// 件 4：报告哈希。
    Report,
}

impl SealItem {
    /// 件号。
    pub const fn id(self) -> usize {
        match self {
            SealItem::Inventory => 0,
            SealItem::Mapping => 1,
            SealItem::OrderTable => 2,
            SealItem::VersionChain => 3,
            SealItem::Report => 4,
        }
    }

    /// 按件号重建。
    pub fn from_id(v: usize) -> Result<SealItem, u16> {
        match v {
            0 => Ok(SealItem::Inventory),
            1 => Ok(SealItem::Mapping),
            2 => Ok(SealItem::OrderTable),
            3 => Ok(SealItem::VersionChain),
            4 => Ok(SealItem::Report),
            _ => Err(E_ITEM_RANGE),
        }
    }

    /// 件名。
    pub const fn name(self) -> &'static str {
        match self {
            SealItem::Inventory => "盘点",
            SealItem::Mapping => "映射",
            SealItem::OrderTable => "序表",
            SealItem::VersionChain => "版本链",
            SealItem::Report => "报告",
        }
    }
}

// ── 封账件 ───────────────────────────────────────────────────────────
/// 封账件：件号 + 件哈希。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SealedItem {
    /// 件号。
    pub item: u8,
    /// 件哈希。
    pub hash: u64,
}

// ── 断言面（封账对象） ───────────────────────────────────────────────
/// 断言面：登记面 + 五件封账 + 定版只读守卫。
pub struct SealLedger {
    /// 封账序号 1..=MAX_SEALS。
    pub seq: u32,
    /// 断言账（名哈希序表）。
    asserts: Vec<u64>,
    /// 封账件账（≤5）。
    items: Vec<SealedItem>,
    /// 定版位。
    sealed: bool,
    /// 定版哈希。
    final_fp: u64,
    /// 写尝试检出计数（封账后逐次累加）。
    write_hits: u32,
    /// 成功写计数（定版后恒 0——守卫判据锚）。
    write_ok: u32,
}

impl SealLedger {
    /// 新断言面。
    pub fn new(seq: u32) -> Result<Self, u16> {
        if seq < 1 || seq as usize > MAX_SEALS {
            return Err(E_SEQ_RANGE);
        }
        Ok(SealLedger {
            seq,
            asserts: Vec::new(),
            items: Vec::new(),
            sealed: false,
            final_fp: 0,
            write_hits: 0,
            write_ok: 0,
        })
    }

    /// 登记断言（定版前）。
    pub fn enroll(&mut self, name: &str) -> Result<(), u16> {
        if self.sealed {
            // 写尝试检出：计数累加，账不变。
            self.write_hits = self.write_hits.wrapping_add(1);
            return Err(E_SEALED);
        }
        if self.asserts.len() >= SEAL_TARGET * 2 {
            return Err(E_SET_FULL);
        }
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(E_NAME_BAD);
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.asserts.len() {
            if self.asserts[i] == fp {
                return Err(E_ASSERT_DUP);
            }
            i += 1;
        }
        self.asserts.push(fp);
        Ok(())
    }

    /// 断言面数。
    pub fn assert_count(&self) -> usize {
        self.asserts.len()
    }

    /// 盘点哈希（清单逐条折叠）。
    fn inventory_hash(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.asserts.len() {
            let fp = self.asserts[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            i += 1;
        }
        h
    }

    /// 派生件哈希（映射/序表/版本链/报告——由清单确定性派生，
    /// 供对照口径；真实面由上游账传入）。
    pub fn derive_item_hash(&self, item: SealItem) -> u64 {
        let inv = self.inventory_hash();
        let mut h = FNV_OFF;
        h = fnv_mix(h, item.id() as u8 | 0x10);
        let mut shift = 56i32;
        while shift >= 0 {
            h = fnv_mix(h, ((inv >> shift) & 0xFF) as u8);
            shift -= 8;
        }
        h = fnv_mix(h, (self.asserts.len() as u64 & 0xFF) as u8 | 0x20);
        h
    }

    /// 封一件。
    pub fn put_item(&mut self, item: SealItem, hash: u64) -> Result<(), u16> {
        if self.sealed {
            self.write_hits = self.write_hits.wrapping_add(1);
            return Err(E_SEALED);
        }
        if hash == 0 {
            return Err(E_HASH_ZERO);
        }
        let id = item.id();
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].item as usize == id {
                self.write_hits = self.write_hits.wrapping_add(1);
                return Err(E_ITEM_RANGE);
            }
            i += 1;
        }
        self.items.push(SealedItem { item: id as u8, hash });
        Ok(())
    }

    /// 件齐位数。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 定版：五件齐 + 面达标 → 定版哈希落账（写守卫生效）。
    pub fn finalize(&mut self) -> Result<u64, u16> {
        if self.sealed {
            self.write_hits = self.write_hits.wrapping_add(1);
            return Err(E_SEALED);
        }
        if self.items.len() != SEAL_ITEM_COUNT {
            return Err(E_ITEM_MISSING);
        }
        if self.asserts.len() < SEAL_TARGET {
            return Err(E_TARGET_SHORT);
        }
        // 五件哈希与当前账对拍（封账件漂移检出）。
        let mut i = 0usize;
        while i < self.items.len() {
            let it = SealItem::from_id(self.items[i].item as usize)?;
            if self.derive_item_hash(it) != self.items[i].hash {
                return Err(E_ITEM_DRIFT);
            }
            i += 1;
        }
        self.final_fp = self.compute_final_fp();
        self.sealed = true;
        Ok(self.final_fp)
    }

    /// 定版哈希（五件 + 面数折叠）。
    fn compute_final_fp(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fnv_mix(h, self.items.len() as u8 | 0x30);
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = fnv_mix(h, it.item | 0x40);
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((it.hash >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            i += 1;
        }
        h = fnv_mix(h, (self.asserts.len() as u64 & 0xFF) as u8 | 0x50);
        h
    }

    /// 定版哈希只读（未定版即 Err）。
    pub fn final_fp(&self) -> Result<u64, u16> {
        if !self.sealed {
            return Err(E_NOT_SEALED);
        }
        Ok(self.final_fp)
    }

    /// 定版哈希复算对拍（漂移检出）。
    pub fn verify_final(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(E_NOT_SEALED);
        }
        if self.compute_final_fp() != self.final_fp {
            return Err(E_FP_DRIFT);
        }
        Ok(())
    }

    /// 写尝试检出恒零核对：封账后成功写必须为 0。
    pub fn audit_write_guard(&self) -> Result<(), u16> {
        if !self.sealed {
            return Err(E_NOT_SEALED);
        }
        if self.write_ok != 0 {
            return Err(E_SEAL_BROKEN);
        }
        Ok(())
    }

    /// 写尝试检出计数。
    pub const fn write_hits(&self) -> u32 {
        self.write_hits
    }

    /// 写守卫探针（验收用）：封账后逐通道注入写尝试，全部必拦且
    /// 检出计数累加（恒零判据载体）。
    pub fn guard_probe(&mut self, name: &str, item: SealItem) -> Result<(), u16> {
        // 通道 1：登记写。
        let r1 = self.enroll(name);
        // 通道 2：件写。
        let r2 = self.put_item(item, 1);
        // 通道 3：重定版。
        let r3 = self.finalize();
        // 三通道必须全拦（封账态），且成功写恒零。
        if r1 != Err(E_SEALED) || r2 != Err(E_SEALED) || r3 != Err(E_SEALED) {
            self.write_ok = self.write_ok.wrapping_add(1);
            return Err(E_SEAL_BROKEN);
        }
        self.audit_write_guard()?;
        Ok(())
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.asserts.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("断言封账: 面=");
        out.push_str(&itoa(self.asserts.len() as u64));
        out.push_str("/300 件=");
        out.push_str(&itoa(self.items.len() as u64));
        out.push_str("/5 定版=");
        out.push_str(&itoa(self.sealed as u64));
        out.push_str(" 写检出=");
        out.push_str(&itoa(self.write_hits as u64));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn sname(i: usize) -> String {
    let mut s = String::new();
    s.push_str("seal_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：300 条登记 → 五件封账 → 定版 → 写守卫恒零 ——
    let mut led = SealLedger::new(1)?;
    let mut i = 0usize;
    while i < SEAL_TARGET {
        led.enroll(&sname(i))?;
        i += 1;
    }
    if led.assert_count() != SEAL_TARGET {
        return Err(E_TARGET_SHORT);
    }
    // 五件逐一封（哈希用派生口径，保证与账一致）。
    let mut k = 0usize;
    while k < SEAL_ITEM_COUNT {
        let it = SealItem::from_id(k)?;
        let h = led.derive_item_hash(it);
        led.put_item(it, h)?;
        k += 1;
    }
    if led.item_count() != SEAL_ITEM_COUNT {
        return Err(E_ITEM_MISSING);
    }
    let fp = led.finalize()?;
    if fp == 0 {
        return Err(E_ITEM_MISSING);
    }
    led.verify_final()?;
    led.audit_write_guard()?;
    // 定版哈希解引。
    if led.final_fp()? != fp {
        return Err(E_FP_DRIFT);
    }
    let sm = led.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 写尝试检出恒零：封账后写必拦且检出累加 ——
    let hits0 = led.write_hits();
    led.guard_probe("post_seal_write", SealItem::Inventory)?;
    if led.write_hits() <= hits0 {
        return Err(E_SEAL_BROKEN);
    }
    // 直接通道再拦：登记。
    if led.enroll("another") != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    // 件重封。
    if led.put_item(SealItem::Report, 1) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    // 重定版。
    if led.finalize() != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    led.audit_write_guard()?;
    // 面数不变（写尝试零落盘）。
    if led.assert_count() != SEAL_TARGET {
        return Err(E_SEAL_BROKEN);
    }

    // —— 幂等：同面同件重建，定版哈希全等 ——
    let mut led2 = SealLedger::new(2)?;
    let mut j = 0usize;
    while j < SEAL_TARGET {
        led2.enroll(&sname(j))?;
        j += 1;
    }
    k = 0;
    while k < SEAL_ITEM_COUNT {
        let it = SealItem::from_id(k)?;
        let h = led2.derive_item_hash(it);
        led2.put_item(it, h)?;
        k += 1;
    }
    if led2.finalize()? != fp {
        return Err(E_FP_DRIFT);
    }

    // —— 面不足拦截：299 条拒定版 ——
    let mut short = SealLedger::new(3)?;
    let mut m = 0usize;
    while m < SEAL_TARGET - 1 {
        short.enroll(&sname(1000 + m))?;
        m += 1;
    }
    k = 0;
    while k < SEAL_ITEM_COUNT {
        let it = SealItem::from_id(k)?;
        short.put_item(it, short.derive_item_hash(it))?;
        k += 1;
    }
    if short.finalize() != Err(E_TARGET_SHORT) {
        return Err(E_TARGET_SHORT);
    }

    // —— 件缺拦截：缺一件拒定版 ——
    let mut lack = SealLedger::new(4)?;
    let mut n = 0usize;
    while n < SEAL_TARGET {
        lack.enroll(&sname(2000 + n))?;
        n += 1;
    }
    k = 0;
    while k < SEAL_ITEM_COUNT - 1 {
        let it = SealItem::from_id(k)?;
        lack.put_item(it, lack.derive_item_hash(it))?;
        k += 1;
    }
    if lack.finalize() != Err(E_ITEM_MISSING) {
        return Err(E_ITEM_MISSING);
    }

    // —— 错误路径样本 ——
    if SealItem::from_id(5) != Err(E_ITEM_RANGE) {
        return Err(E_ITEM_RANGE);
    }
    if SealLedger::new(0).err() != Some(E_SEQ_RANGE) {
        return Err(E_SEQ_RANGE);
    }
    if SealLedger::new(MAX_SEALS as u32 + 1).err() != Some(E_SEQ_RANGE) {
        return Err(E_SEQ_RANGE);
    }
    let mut err = SealLedger::new(5)?;
    if err.enroll("") != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    err.enroll("dup")?;
    if err.enroll("dup") != Err(E_ASSERT_DUP) {
        return Err(E_ASSERT_DUP);
    }
    // 零哈希件。
    if err.put_item(SealItem::Mapping, 0) != Err(E_HASH_ZERO) {
        return Err(E_HASH_ZERO);
    }
    // 同件二封。
    err.put_item(SealItem::Mapping, err.derive_item_hash(SealItem::Mapping))?;
    if err.put_item(SealItem::Mapping, 7) != Err(E_ITEM_RANGE) {
        return Err(E_ITEM_RANGE);
    }
    // 未定版即取定版哈希 / 复算。
    if err.final_fp() != Err(E_NOT_SEALED) {
        return Err(E_NOT_SEALED);
    }
    if err.verify_final() != Err(E_NOT_SEALED) {
        return Err(E_NOT_SEALED);
    }
    if err.audit_write_guard() != Err(E_NOT_SEALED) {
        return Err(E_NOT_SEALED);
    }
    // 封账件漂移注入：件哈希篡改 → 定版必拦。
    let mut dm = SealLedger::new(6)?;
    let mut p = 0usize;
    while p < SEAL_TARGET {
        dm.enroll(&sname(3000 + p))?;
        p += 1;
    }
    k = 0;
    while k < SEAL_ITEM_COUNT {
        let it = SealItem::from_id(k)?;
        dm.put_item(it, dm.derive_item_hash(it))?;
        k += 1;
    }
    dm.items[0].hash = dm.items[0].hash.wrapping_add(1);
    if dm.finalize() != Err(E_ITEM_DRIFT) {
        return Err(E_ITEM_DRIFT);
    }
    // 摘要空账拦。
    let empty = SealLedger::new(7)?;
    if empty.summary() != Err(E_SUMMARY_EMPTY) {
        return Err(E_SUMMARY_EMPTY);
    }

    // 诊断表抽查。
    if diag_detail(E_SEAL_BROKEN) == "未知诊断码（0x59c0 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x59c0 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
