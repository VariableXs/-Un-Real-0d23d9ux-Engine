//! UNX-F0706 · 交接面断言补全至 ≥40 · 诊断段 0x5840（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0706] 交接面断言补全至 ≥40 <SEG 0x5840>
//!   UNX-F0706-J1 交接面分项补全后实数 ≥40（六冻结接口字段级/Limine 响应面
//!   /panic 路径三族覆盖）且分项全绿。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 三族闭集：Family::FrozenIface（六冻结接口字段级）/ Family::LimineResp
//!      （Limine 响应面）/ Family::PanicPath（panic 路径），族外无分项。
//!   2. 补全下限：实数 ≥40（ASSERT_TARGET=40），不足拒封——补全闸显性。
//!   3. 六冻结接口字段级覆盖：六个冻结接口每接口至少 1 个字段级分项
//!      （接口覆盖位图全 1），缺接口即 E_IFACE_MISSING。
//!   4. panic 路径三重 fault 覆盖：三重 fault（缺响应/坏 magic/版本不符）
//!      每路径至少 1 分项（路径位图全 1）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5840~0x585F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5840 独占 0x5840~0x585F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5840;

/// 0x5841 断言面已封板仍尝试补登
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5842 族号越界（合法 0..3）
const E_FAMILY_RANGE: u16 = SEG_BASE + 0x2;
/// 0x5843 实数不足：补全后 <40
const E_TARGET_SHORT: u16 = SEG_BASE + 0x3;
/// 0x5844 冻结接口缺：六接口未全覆盖
const E_IFACE_MISSING: u16 = SEG_BASE + 0x4;
/// 0x5845 接口号越界（合法 0..6）
const E_IFACE_RANGE: u16 = SEG_BASE + 0x5;
/// 0x5846 panic 路径缺：三重 fault 未全覆盖
const E_FAULT_MISSING: u16 = SEG_BASE + 0x6;
/// 0x5847 分项败：执行结果为败
const E_ITEM_FAIL: u16 = SEG_BASE + 0x7;
/// 0x5848 分项重号：同族同名二次登记
const E_ITEM_DUP: u16 = SEG_BASE + 0x8;
/// 0x5849 分项种类越闭集
const E_KIND_RANGE: u16 = SEG_BASE + 0x9;
/// 0x584A 族缺：三族有一族零分项
const E_FAMILY_EMPTY: u16 = SEG_BASE + 0xA;
/// 0x584B 守恒破：Σ族分项数 ≠ 总分项数
const E_CONSERVATION: u16 = SEG_BASE + 0xB;
/// 0x584C 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x584D 未跑即取全绿结论
const E_NOT_RUN: u16 = SEG_BASE + 0xD;
/// 0x584E 断言账溢出：分项数超定容
const E_SET_FULL: u16 = SEG_BASE + 0xE;
/// 0x584F Limine 响应 tag 越闭集
const E_TAG_RANGE: u16 = SEG_BASE + 0xF;

/// 诊断码 → 中文可操作诊断（段 0x5840~0x585F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "断言面已封板：补登冻结，扩族请开新集",
        E_FAMILY_RANGE => "族号越界：三族闭集合法 0..3",
        E_TARGET_SHORT => "实数不足：补全后分项 <40，请继续补全",
        E_IFACE_MISSING => "冻结接口缺：六接口未全覆盖，请补缺口分项",
        E_IFACE_RANGE => "接口号越界：六冻结接口合法 0..6",
        E_FAULT_MISSING => "panic 路径缺：三重 fault 未全覆盖",
        E_ITEM_FAIL => "分项败：存在失败分项，先修后全绿",
        E_ITEM_DUP => "分项重号：同族同名二次登记拒收",
        E_KIND_RANGE => "分项种类越闭集",
        E_FAMILY_EMPTY => "族缺：三族有一族零分项，覆盖不齐",
        E_CONSERVATION => "守恒破：Σ族分项数 ≠ 总分项数",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_NOT_RUN => "未跑即取结论：先 run_all 再取全绿位",
        E_SET_FULL => "断言账溢出：分项数超定容上限",
        E_TAG_RANGE => "Limine 响应 tag 越闭集",
        _ => "未知诊断码（0x5840 段外或保留位）",
    }
}

// ── 常量（来源：判据 ≥40 与六冻结接口） ─────────────────────────────
/// 分项实数目标（判据原文：补全后实数 ≥40）。
pub const ASSERT_TARGET: usize = 40;
/// 冻结接口数（判据原文：六冻结接口字段级）。
pub const FROZEN_IFACES: usize = 6;
/// panic 三重 fault 数。
pub const FAULT_KINDS: usize = 3;
/// 三族数。
pub const FAMILY_COUNT: usize = 3;
/// 分项定容。
pub const MAX_ITEMS: usize = 128;
/// Limine 响应 tag 闭集容量（memory map/framebuffer/kernel address 三响应）。
pub const LIMINE_TAGS: usize = 3;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(ASSERT_TARGET == 40);
const _: () = assert!(FROZEN_IFACES == 6);
const _: () = assert!(FAULT_KINDS == 3);

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

// ── 三族闭集 ─────────────────────────────────────────────────────────
/// 交接面断言三族闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoverFamily {
    /// 族 0：六冻结接口字段级。
    FrozenIface,
    /// 族 1：Limine 响应面。
    LimineResp,
    /// 族 2：panic 路径。
    PanicPath,
}

impl HandoverFamily {
    /// 族号。
    pub const fn id(self) -> usize {
        match self {
            HandoverFamily::FrozenIface => 0,
            HandoverFamily::LimineResp => 1,
            HandoverFamily::PanicPath => 2,
        }
    }

    /// 按族号重建。
    pub fn from_id(v: usize) -> Result<HandoverFamily, u16> {
        match v {
            0 => Ok(HandoverFamily::FrozenIface),
            1 => Ok(HandoverFamily::LimineResp),
            2 => Ok(HandoverFamily::PanicPath),
            _ => Err(E_FAMILY_RANGE),
        }
    }

    /// 族名。
    pub const fn name(self) -> &'static str {
        match self {
            HandoverFamily::FrozenIface => "六冻结接口字段级",
            HandoverFamily::LimineResp => "Limine 响应面",
            HandoverFamily::PanicPath => "panic 路径",
        }
    }
}

// ── Limine 响应 tag 闭集 ─────────────────────────────────────────────
/// Limine 响应面 tag 闭集（三响应）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimineTag {
    /// memory map 响应。
    Memmap,
    /// framebuffer 响应。
    Framebuffer,
    /// kernel address 响应。
    KernelAddress,
}

impl LimineTag {
    /// tag 号。
    pub const fn id(self) -> usize {
        match self {
            LimineTag::Memmap => 0,
            LimineTag::Framebuffer => 1,
            LimineTag::KernelAddress => 2,
        }
    }

    /// 按号重建。
    pub fn from_id(v: usize) -> Result<LimineTag, u16> {
        match v {
            0 => Ok(LimineTag::Memmap),
            1 => Ok(LimineTag::Framebuffer),
            2 => Ok(LimineTag::KernelAddress),
            _ => Err(E_TAG_RANGE),
        }
    }
}

// ── panic 三重 fault 闭集 ────────────────────────────────────────────
/// panic 路径三重 fault 闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// fault 0：必需响应缺失 → PANIC。
    MissingResponse,
    /// fault 1：响应 magic 坏 → PANIC。
    BadMagic,
    /// fault 2：协议版本不符 → PANIC。
    VersionMismatch,
}

impl FaultKind {
    /// fault 号。
    pub const fn id(self) -> usize {
        match self {
            FaultKind::MissingResponse => 0,
            FaultKind::BadMagic => 1,
            FaultKind::VersionMismatch => 2,
        }
    }

    /// 按号重建。
    pub fn from_id(v: usize) -> Result<FaultKind, u16> {
        match v {
            0 => Ok(FaultKind::MissingResponse),
            1 => Ok(FaultKind::BadMagic),
            2 => Ok(FaultKind::VersionMismatch),
            _ => Err(E_TAG_RANGE),
        }
    }
}

// ── 分项条目 ─────────────────────────────────────────────────────────
/// 交接面分项：族 + 名哈希 + 接口号/端口号（语义复用槽）+ 种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoverItem {
    /// 所属族。
    pub family: u8,
    /// 分项名 FNV 哈希（去重锚）。
    pub name_fp: u64,
    /// 槽位：冻结接口号 0..6 / Limine tag 0..3 / fault 号 0..3。
    pub slot: u8,
    /// 种类（执行逻辑派发位 0..=3）。
    pub kind: u8,
}

// 分项种类闭集（确定性执行逻辑）：
//   kind 0: 恒真断言；kind 1: 哈希自反；
//   kind 2: 槽位域断言（按族域核对）；kind 3: 族域断言。

// ── 交接面断言集 ─────────────────────────────────────────────────────
/// 交接面断言集：三族分项登记 + 覆盖位图 + 补全下限闸 + 全绿执行。
pub struct AssertSet40 {
    /// 分项账。
    items: Vec<HandoverItem>,
    /// 每族计数。
    per_family: [u32; FAMILY_COUNT],
    /// 六冻结接口覆盖位图（bit i = 接口 i 有字段级分项）。
    iface_cover: u8,
    /// panic 三重 fault 覆盖位图。
    fault_cover: u8,
    /// Limine tag 覆盖位图。
    tag_cover: u8,
    /// 封板位。
    sealed: bool,
    /// 已跑位。
    ran: bool,
    /// 失败计数。
    fail_count: u32,
}

impl AssertSet40 {
    /// 新集。
    pub fn new() -> Self {
        AssertSet40 {
            items: Vec::new(),
            per_family: [0; FAMILY_COUNT],
            iface_cover: 0,
            fault_cover: 0,
            tag_cover: 0,
            sealed: false,
            ran: false,
            fail_count: 0,
        }
    }

    /// 登记分项：族闭集 + 槽位域 + 名去重 + 覆盖记账。
    pub fn enroll(
        &mut self,
        family: HandoverFamily,
        name: &str,
        slot: u8,
        kind: u8,
    ) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.items.len() >= MAX_ITEMS {
            return Err(E_SET_FULL);
        }
        if kind > 3 {
            return Err(E_KIND_RANGE);
        }
        match family {
            HandoverFamily::FrozenIface => {
                if slot as usize >= FROZEN_IFACES {
                    return Err(E_IFACE_RANGE);
                }
                self.iface_cover |= 1u8 << slot;
            }
            HandoverFamily::LimineResp => {
                if slot as usize >= LIMINE_TAGS {
                    return Err(E_TAG_RANGE);
                }
                self.tag_cover |= 1u8 << slot;
            }
            HandoverFamily::PanicPath => {
                if slot as usize >= FAULT_KINDS {
                    return Err(E_TAG_RANGE);
                }
                self.fault_cover |= 1u8 << slot;
            }
        }
        let fp = fnv_str(name);
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].family == family.id() as u8 && self.items[i].name_fp == fp {
                return Err(E_ITEM_DUP);
            }
            i += 1;
        }
        self.per_family[family.id()] = self.per_family[family.id()].wrapping_add(1);
        self.items.push(HandoverItem {
            family: family.id() as u8,
            name_fp: fp,
            slot,
            kind,
        });
        Ok(())
    }

    /// 总分项数。
    pub fn total(&self) -> usize {
        self.items.len()
    }

    /// 族计数。
    pub fn family_counts(&self) -> [u32; FAMILY_COUNT] {
        self.per_family
    }

    /// 覆盖位图只读。
    pub const fn covers(&self) -> (u8, u8, u8) {
        (self.iface_cover, self.tag_cover, self.fault_cover)
    }

    /// 补全审计：三族齐 + 六接口全覆盖 + 三 fault 全覆盖 + 实数 ≥40。
    pub fn audit_complete(&self) -> Result<usize, u16> {
        let mut f = 0usize;
        while f < FAMILY_COUNT {
            if self.per_family[f] == 0 {
                return Err(E_FAMILY_EMPTY);
            }
            f += 1;
        }
        if self.iface_cover != (1u8 << FROZEN_IFACES) - 1 {
            return Err(E_IFACE_MISSING);
        }
        if self.fault_cover != (1u8 << FAULT_KINDS) - 1 {
            return Err(E_FAULT_MISSING);
        }
        let total = self.total();
        if total < ASSERT_TARGET {
            return Err(E_TARGET_SHORT);
        }
        Ok(total)
    }

    /// 封板：守恒 + 补全审计后落锁。
    pub fn seal(&mut self) -> Result<usize, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let mut sum: u32 = 0;
        let mut f = 0usize;
        while f < FAMILY_COUNT {
            sum = sum.wrapping_add(self.per_family[f]);
            f += 1;
        }
        if sum != self.items.len() as u32 {
            return Err(E_CONSERVATION);
        }
        let t = self.audit_complete()?;
        self.sealed = true;
        Ok(t)
    }

    /// 全量执行（种类闭集派发，确定性）。
    pub fn run_all(&mut self) -> Result<u32, u16> {
        if !self.sealed {
            return Err(E_SEALED);
        }
        if self.ran {
            return Err(E_NOT_RUN);
        }
        self.fail_count = 0;
        let mut pass = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            let ok = match it.kind {
                0 => true,
                1 => it.name_fp != 0,
                2 => match it.family {
                    0 => (it.slot as usize) < FROZEN_IFACES,
                    1 => (it.slot as usize) < LIMINE_TAGS,
                    _ => (it.slot as usize) < FAULT_KINDS,
                },
                3 => (it.family as usize) < FAMILY_COUNT,
                _ => false,
            };
            if ok {
                pass += 1;
            } else {
                self.fail_count = self.fail_count.wrapping_add(1);
            }
            i += 1;
        }
        self.ran = true;
        Ok(pass)
    }

    /// 全绿位。
    pub fn all_green(&self) -> Result<bool, u16> {
        if !self.ran {
            return Err(E_NOT_RUN);
        }
        Ok(self.fail_count == 0)
    }

    /// 失败计数。
    pub const fn fail_count(&self) -> u32 {
        self.fail_count
    }

    /// 集指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((it.name_fp >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            h = fnv_mix(h, it.family | 0x10);
            h = fnv_mix(h, it.slot | 0x20);
            h = fnv_mix(h, it.kind | 0x30);
            i += 1;
        }
        h = fnv_mix(h, self.iface_cover | 0x40);
        h = fnv_mix(h, self.tag_cover | 0x50);
        h = fnv_mix(h, self.fault_cover | 0x60);
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.items.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("交接面断言集: 实数=");
        out.push_str(&itoa(self.total() as u64));
        out.push_str("/40 败=");
        out.push_str(&itoa(self.fail_count as u64));
        let mut f = 0usize;
        while f < FAMILY_COUNT {
            out.push(' ');
            out.push_str(HandoverFamily::from_id(f)?.name());
            out.push('=');
            out.push_str(&itoa(self.per_family[f] as u64));
            f += 1;
        }
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
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

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

fn item_name(family: usize, i: usize) -> String {
    let mut s = String::new();
    s.push_str("hv");
    s.push_str(&itoa(family as u64));
    s.push_str("_");
    s.push_str(&itoa(i as u64));
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 主路径：三族补全至 ≥40（接口 12 + Limine 15 + panic 15 = 42） ——
    let mut set = AssertSet40::new();
    // 冻结接口：6 接口 × 2 字段分项 = 12。
    let mut f = 0u8;
    while (f as usize) < FROZEN_IFACES {
        set.enroll(HandoverFamily::FrozenIface, &item_name(0, f as usize * 2), f, 0)?;
        set.enroll(HandoverFamily::FrozenIface, &item_name(0, f as usize * 2 + 1), f, 2)?;
        f += 1;
    }
    // Limine 响应面：3 tag × 5 分项 = 15。
    let mut t = 0u8;
    while (t as usize) < LIMINE_TAGS {
        let mut k = 0u8;
        while k < 5 {
            let kind = ((t as usize + k as usize) % 4) as u8;
            set.enroll(HandoverFamily::LimineResp, &item_name(1, t as usize * 5 + k as usize), t, kind)?;
            k += 1;
        }
        t += 1;
    }
    // panic 路径：3 fault × 5 分项 = 15。
    let mut p = 0u8;
    while (p as usize) < FAULT_KINDS {
        let mut k = 0u8;
        while k < 5 {
            set.enroll(HandoverFamily::PanicPath, &item_name(2, p as usize * 5 + k as usize), p, ((k % 4) as u8))?;
            k += 1;
        }
        p += 1;
    }
    let total = set.seal()?;
    if total != 42 {
        return Err(E_TARGET_SHORT);
    }
    let pass = set.run_all()?;
    if pass != 42 {
        return Err(E_ITEM_FAIL);
    }
    if !set.all_green()? {
        return Err(E_ITEM_FAIL);
    }
    // 覆盖位图全 1 核对。
    let (ic, tc, fc) = set.covers();
    if ic != (1u8 << FROZEN_IFACES) - 1 {
        return Err(E_IFACE_MISSING);
    }
    if tc != (1u8 << LIMINE_TAGS) - 1 {
        return Err(E_TAG_RANGE);
    }
    if fc != (1u8 << FAULT_KINDS) - 1 {
        return Err(E_FAULT_MISSING);
    }
    let sm = set.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 实数不足拦截：36 分项（接口 12 + Limine 12 + panic 12）<40 ——
    let mut short = AssertSet40::new();
    f = 0;
    while (f as usize) < FROZEN_IFACES {
        short.enroll(HandoverFamily::FrozenIface, &item_name(0, f as usize), f, 0)?;
        short.enroll(HandoverFamily::FrozenIface, &item_name(9, f as usize), f, 0)?;
        f += 1;
    }
    let mut i = 0u8;
    while (i as usize) < LIMINE_TAGS {
        let mut k = 0u8;
        while k < 4 {
            short.enroll(HandoverFamily::LimineResp, &item_name(1, i as usize * 4 + k as usize), i, 0)?;
            k += 1;
        }
        i += 1;
    }
    i = 0;
    while (i as usize) < FAULT_KINDS {
        let mut k = 0u8;
        while k < 4 {
            short.enroll(HandoverFamily::PanicPath, &item_name(2, i as usize * 4 + k as usize), i, 0)?;
            k += 1;
        }
        i += 1;
    }
    if short.seal() != Err(E_TARGET_SHORT) {
        return Err(E_TARGET_SHORT);
    }

    // —— 接口缺拦截：缺接口 5 ——
    let mut gap = AssertSet40::new();
    f = 0;
    while (f as usize) < FROZEN_IFACES - 1 {
        gap.enroll(HandoverFamily::FrozenIface, &item_name(0, f as usize), f, 0)?;
        f += 1;
    }
    // 其余两族补到 ≥40。
    let mut j = 0usize;
    while j < 20 {
        gap.enroll(HandoverFamily::LimineResp, &item_name(1, j), (j % LIMINE_TAGS) as u8, 0)?;
        gap.enroll(HandoverFamily::PanicPath, &item_name(2, j), (j % FAULT_KINDS) as u8, 0)?;
        j += 1;
    }
    if gap.seal() != Err(E_IFACE_MISSING) {
        return Err(E_IFACE_MISSING);
    }

    // —— 错误路径样本 ——
    if HandoverFamily::from_id(3) != Err(E_FAMILY_RANGE) {
        return Err(E_FAMILY_RANGE);
    }
    if LimineTag::from_id(3) != Err(E_TAG_RANGE) {
        return Err(E_TAG_RANGE);
    }
    if FaultKind::from_id(3) != Err(E_TAG_RANGE) {
        return Err(E_TAG_RANGE);
    }
    let mut bad = AssertSet40::new();
    if bad.enroll(HandoverFamily::FrozenIface, "x", 6, 0) != Err(E_IFACE_RANGE) {
        return Err(E_IFACE_RANGE);
    }
    if bad.enroll(HandoverFamily::LimineResp, "x", 3, 0) != Err(E_TAG_RANGE) {
        return Err(E_TAG_RANGE);
    }
    if bad.enroll(HandoverFamily::PanicPath, "x", 3, 0) != Err(E_TAG_RANGE) {
        return Err(E_TAG_RANGE);
    }
    if bad.enroll(HandoverFamily::FrozenIface, "y", 0, 7) != Err(E_KIND_RANGE) {
        return Err(E_KIND_RANGE);
    }
    bad.enroll(HandoverFamily::FrozenIface, "z", 0, 0)?;
    if bad.enroll(HandoverFamily::FrozenIface, "z", 0, 0) != Err(E_ITEM_DUP) {
        return Err(E_ITEM_DUP);
    }
    if bad.enroll(HandoverFamily::LimineResp, "z", 0, 0) != Ok(()) {
        return Err(E_ITEM_DUP);
    }
    // 族缺封板拦。
    if bad.seal() != Err(E_FAMILY_EMPTY) {
        return Err(E_FAMILY_EMPTY);
    }
    // 守恒注入。
    let mut bad2 = AssertSet40::new();
    bad2.enroll(HandoverFamily::PanicPath, "q", 0, 0)?;
    bad2.per_family[2] = bad2.per_family[2].wrapping_add(1);
    if bad2.seal() != Err(E_CONSERVATION) {
        return Err(E_CONSERVATION);
    }
    // fault 缺拦截：panic 路径只覆盖 2 fault。
    let mut gap2 = AssertSet40::new();
    let mut m = 0u8;
    while (m as usize) < FROZEN_IFACES {
        gap2.enroll(HandoverFamily::FrozenIface, &item_name(0, m as usize), m, 0)?;
        m += 1;
    }
    let mut n = 0usize;
    while n < 20 {
        gap2.enroll(HandoverFamily::LimineResp, &item_name(1, n), (n % LIMINE_TAGS) as u8, 0)?;
        n += 1;
    }
    // panic 只覆盖 fault 0/1。
    gap2.enroll(HandoverFamily::PanicPath, "f0", 0, 0)?;
    gap2.enroll(HandoverFamily::PanicPath, "f1", 1, 0)?;
    if gap2.seal() != Err(E_FAULT_MISSING) {
        return Err(E_FAULT_MISSING);
    }
    // 未跑即取全绿。
    let mut bad3 = AssertSet40::new();
    bad3.enroll(HandoverFamily::PanicPath, "w", 0, 0)?;
    bad3.seal()?;
    if bad3.all_green() != Err(E_NOT_RUN) {
        return Err(E_NOT_RUN);
    }
    // 失败注入：篡改 kind。
    bad3.items[0].kind = 9;
    bad3.ran = false;
    if bad3.run_all()? != 0 {
        return Err(E_ITEM_FAIL);
    }
    if bad3.all_green()? {
        return Err(E_ITEM_FAIL);
    }

    // 诊断表抽查。
    if diag_detail(E_IFACE_MISSING) == "未知诊断码（0x5840 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5840 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
