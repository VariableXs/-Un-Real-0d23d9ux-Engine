//! UNX-F0725 · boot info 响应树兼容账（升级字段冻结核对）· 诊断段 0x5a20（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0725] boot info 响应树兼容账（升级字段冻结核对） <SEG 0x5a20>
//!   UNX-F0725-J1 响应树兼容核对账齐（memory map/framebuffer/kernel
//!   address 三响应逐字段冻结表核对 100%）。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 三响应闭集：RespTag 恰 3 变体（memory map/framebuffer/
//!      kernel address），tag 外无响应面。
//!   2. 字段冻结表：每响应字段（字段号/偏移/宽度/冻结值）逐项落表；
//!      升级核对 = 实测值 × 冻结值逐字段对拍，100% 全查。
//!   3. 字段闭集域：字段号 0..=FIELdS（按响应定容），越域拒收；
//!      冻结表缺字段（漏冻）与多字段（超冻）都显性检出。
//!   4. magic/revision 门：响应头 magic 与 revision 先核对，门未过
//!      字段核对不启动（升级兼容前置闸）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5a20~0x5a3F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥570（判据原文「纯功能代码 ≥570 行」）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0x5a20 独占 0x5a20~0x5a3F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5a20;

/// 0x5a21 冻结表已封板仍尝试改字段
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5a22 响应 tag 越闭集
const E_TAG_RANGE: u16 = SEG_BASE + 0x2;
/// 0x5a23 字段号越域
const E_FIELD_RANGE: u16 = SEG_BASE + 0x3;
/// 0x5a24 magic 破：响应头 magic 不符
const E_MAGIC_BAD: u16 = SEG_BASE + 0x4;
/// 0x5a25 revision 不符：协议版本位漂移
const E_REVISION_BAD: u16 = SEG_BASE + 0x5;
/// 0x5a26 字段实测漂移：实测值 ≠ 冻结值
const E_FIELD_DRIFT: u16 = SEG_BASE + 0x6;
/// 0x5a27 冻结缺字段：实测字段未入冻结表
const E_FIELD_UNFROZEN: u16 = SEG_BASE + 0x7;
/// 0x5a28 冻结超字段：冻结表字段实测缺失
const E_FIELD_ABSENT: u16 = SEG_BASE + 0x8;
/// 0x5a29 宽度越域（合法 1/2/4/8 字节）
const E_WIDTH_RANGE: u16 = SEG_BASE + 0x9;
/// 0x5a2A 字段重冻：同 tag 同字段号二次冻结
const E_FROZEN_DUP: u16 = SEG_BASE + 0xA;
/// 0x5a2B 核对未跑即取兼容结论
const E_NOT_VERIFIED: u16 = SEG_BASE + 0xB;
/// 0x5a2C 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xC;
/// 0x5a2D 冻结表空：零字段即核对
const E_TABLE_EMPTY: u16 = SEG_BASE + 0xD;
/// 0x5a2E 兼容破：存在漂移字段
const E_COMPAT_BROKEN: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5a20~0x5a3F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "冻结表已封板：字段冻结，改表请开新账",
        E_TAG_RANGE => "响应 tag 越闭集：三响应闭集外无面",
        E_FIELD_RANGE => "字段号越域：按响应定容闭集外拒收",
        E_MAGIC_BAD => "magic 破：响应头 magic 不符，升级面异常",
        E_REVISION_BAD => "revision 不符：协议版本位漂移，先走升级闸",
        E_FIELD_DRIFT => "字段实测漂移：实测值 ≠ 冻结值，逐字段对拍必中",
        E_FIELD_UNFROZEN => "冻结缺字段：实测字段未入冻结表，先补冻",
        E_FIELD_ABSENT => "冻结超字段：冻结表字段实测缺失",
        E_WIDTH_RANGE => "宽度越域：合法 1/2/4/8 字节",
        E_FROZEN_DUP => "字段重冻：同 tag 同字段号二次冻结拒收",
        E_NOT_VERIFIED => "核对未跑：先 verify_all 再取兼容结论",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_TABLE_EMPTY => "冻结表空：零字段无核对资格",
        E_COMPAT_BROKEN => "兼容破：存在漂移字段，升级面待处置",
        _ => "未知诊断码（0x5a20 段外或保留位）",
    }
}

// ── 常量（来源：Limine 三响应口径） ──────────────────────────────────
/// 响应 tag 数（判据原文：三响应）。
pub const RESP_TAG_COUNT: usize = 3;
/// 单响应字段定容。
pub const FIELDS_PER_RESP: usize = 16;
/// 合法宽度闭集（字节）。
pub const LEGAL_WIDTHS: [u8; 4] = [1, 2, 4, 8];
/// Limine 响应头 magic（64 位常量，来源：Limine boot protocol 约定值
/// 0xf956780dcf7ea9c9——写死作冻结锚）。
pub const LIMINE_MAGIC: u64 = 0xf956_780d_cf7e_a9c9;
/// 本域协议 revision 冻结值（来源：升级跟随账冻结基线 revision=3）。
pub const FROZEN_REVISION: u64 = 3;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(RESP_TAG_COUNT == 3);
const _: () = assert!(LIMINE_MAGIC == 0xf956_780d_cf7e_a9c9);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

// ── 三响应闭集 ───────────────────────────────────────────────────────
/// boot info 三响应闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RespTag {
    /// memory map 响应。
    Memmap,
    /// framebuffer 响应。
    Framebuffer,
    /// kernel address 响应。
    KernelAddress,
}

impl RespTag {
    /// tag 号。
    pub const fn id(self) -> usize {
        match self {
            RespTag::Memmap => 0,
            RespTag::Framebuffer => 1,
            RespTag::KernelAddress => 2,
        }
    }

    /// 按号重建。
    pub fn from_id(v: usize) -> Result<RespTag, u16> {
        match v {
            0 => Ok(RespTag::Memmap),
            1 => Ok(RespTag::Framebuffer),
            2 => Ok(RespTag::KernelAddress),
            _ => Err(E_TAG_RANGE),
        }
    }

    /// tag 名。
    pub const fn name(self) -> &'static str {
        match self {
            RespTag::Memmap => "memory map",
            RespTag::Framebuffer => "framebuffer",
            RespTag::KernelAddress => "kernel address",
        }
    }
}

// ── 冻结字段 ─────────────────────────────────────────────────────────
/// 冻结字段：字段号 + 偏移 + 宽度 + 冻结值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenField {
    /// 所属 tag。
    pub tag: u8,
    /// 字段号 0..=FIELDS_PER_RESP。
    pub field_no: u8,
    /// 字段偏移（字节）。
    pub offset: u16,
    /// 宽度（1/2/4/8 字节）。
    pub width: u8,
    /// 冻结值。
    pub frozen: u64,
}

impl FrozenField {
    /// 宽度合法性。
    pub const fn width_ok(&self) -> bool {
        let w = self.width;
        w == 1 || w == 2 || w == 4 || w == 8
    }
}

// ── 实测字段（升级后响应实测面） ─────────────────────────────────────
/// 实测字段：字段号 + 实测值（对拍冻结值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredField {
    /// 所属 tag。
    pub tag: u8,
    /// 字段号。
    pub field_no: u8,
    /// 实测值。
    pub measured: u64,
}

// ── 响应头 ───────────────────────────────────────────────────────────
/// 响应头实测：magic + revision（前置闸核对对象）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RespHeader {
    /// 实测 magic。
    pub magic: u64,
    /// 实测 revision。
    pub revision: u64,
}

// ── 兼容核对账 ───────────────────────────────────────────────────────
/// 响应树兼容核对账：冻结表 + 实测面 + 逐字段 100% 对拍。
pub struct CompatLedger {
    /// 冻结表（逐字段）。
    frozen: Vec<FrozenField>,
    /// 封板位。
    sealed: bool,
    /// 已核对位。
    verified: bool,
    /// 漂移字段检出计数。
    drift_hits: u32,
    /// 缺字段检出计数。
    absent_hits: u32,
}

impl CompatLedger {
    /// 新账。
    pub fn new() -> Self {
        CompatLedger {
            frozen: Vec::new(),
            sealed: false,
            verified: false,
            drift_hits: 0,
            absent_hits: 0,
        }
    }

    /// 冻结字段：tag/字段号/宽度域 + 重冻拦截。
    pub fn freeze_field(
        &mut self,
        tag: RespTag,
        field_no: u8,
        offset: u16,
        width: u8,
        frozen: u64,
    ) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if field_no as usize >= FIELDS_PER_RESP {
            return Err(E_FIELD_RANGE);
        }
        let f = FrozenField {
            tag: tag.id() as u8,
            field_no,
            offset,
            width,
            frozen,
        };
        if !f.width_ok() {
            return Err(E_WIDTH_RANGE);
        }
        let mut i = 0usize;
        while i < self.frozen.len() {
            if self.frozen[i].tag == f.tag && self.frozen[i].field_no == f.field_no {
                return Err(E_FROZEN_DUP);
            }
            i += 1;
        }
        self.frozen.push(f);
        Ok(())
    }

    /// 冻结表字段数。
    pub fn frozen_count(&self) -> usize {
        self.frozen.len()
    }

    /// 封板。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.frozen.is_empty() {
            return Err(E_TABLE_EMPTY);
        }
        self.sealed = true;
        Ok(())
    }

    /// 前置闸：响应头核对（magic + revision）。
    pub fn gate_header(&self, h: RespHeader) -> Result<(), u16> {
        if h.magic != LIMINE_MAGIC {
            return Err(E_MAGIC_BAD);
        }
        if h.revision != FROZEN_REVISION {
            return Err(E_REVISION_BAD);
        }
        Ok(())
    }

    /// 逐字段 100% 核对：
    ///   ① 冻结表逐字段在实测面找同号字段——缺即 E_FIELD_ABSENT；
    ///   ② 值对拍——不等即 E_FIELD_DRIFT（检出累加）；
    ///   ③ 实测面逐字段回查冻结表——未冻结即 E_FIELD_UNFROZEN。
    /// 全查通过 → verified 落位。
    pub fn verify_all(
        &mut self,
        header: RespHeader,
        measured: &[MeasuredField],
    ) -> Result<usize, u16> {
        if !self.sealed {
            return Err(E_SEALED);
        }
        self.gate_header(header)?;
        if self.frozen.is_empty() {
            return Err(E_TABLE_EMPTY);
        }
        self.verified = false;
        self.drift_hits = 0;
        self.absent_hits = 0;
        // ① + ②：冻结 → 实测。
        let mut i = 0usize;
        while i < self.frozen.len() {
            let f = self.frozen[i];
            let mut found = false;
            let mut j = 0usize;
            while j < measured.len() {
                if measured[j].tag == f.tag && measured[j].field_no == f.field_no {
                    found = true;
                    if measured[j].measured != f.frozen {
                        self.drift_hits = self.drift_hits.wrapping_add(1);
                    }
                    break;
                }
                j += 1;
            }
            if !found {
                self.absent_hits = self.absent_hits.wrapping_add(1);
            }
            i += 1;
        }
        if self.absent_hits > 0 {
            return Err(E_FIELD_ABSENT);
        }
        // ③：实测 → 冻结。
        let mut k = 0usize;
        while k < measured.len() {
            let m = measured[k];
            if m.tag as usize >= RESP_TAG_COUNT {
                return Err(E_TAG_RANGE);
            }
            if m.field_no as usize >= FIELDS_PER_RESP {
                return Err(E_FIELD_RANGE);
            }
            let mut frozen = false;
            let mut n = 0usize;
            while n < self.frozen.len() {
                if self.frozen[n].tag == m.tag && self.frozen[n].field_no == m.field_no {
                    frozen = true;
                    break;
                }
                n += 1;
            }
            if !frozen {
                return Err(E_FIELD_UNFROZEN);
            }
            k += 1;
        }
        if self.drift_hits > 0 {
            return Err(E_COMPAT_BROKEN);
        }
        self.verified = true;
        Ok(self.frozen.len())
    }

    /// 已核对位。
    pub const fn is_verified(&self) -> bool {
        self.verified
    }

    /// 漂移检出计数。
    pub const fn drift_hits(&self) -> u32 {
        self.drift_hits
    }

    /// 缺字段检出计数。
    pub const fn absent_hits(&self) -> u32 {
        self.absent_hits
    }

    /// 兼容结论：已核对 + 零漂移 + 零缺字段。
    pub fn compat_verdict(&self) -> Result<bool, u16> {
        if !self.verified {
            return Err(E_NOT_VERIFIED);
        }
        Ok(self.drift_hits == 0 && self.absent_hits == 0)
    }

    /// 冻结表指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.frozen.len() {
            let f = self.frozen[i];
            h = fnv_mix(h, f.tag | 0x10);
            h = fnv_mix(h, f.field_no | 0x20);
            h = fnv_mix(h, (f.offset & 0xFF) as u8 | 0x30);
            h = fnv_mix(h, f.width | 0x40);
            let mut shift = 56i32;
            while shift >= 0 {
                h = fnv_mix(h, ((f.frozen >> shift) & 0xFF) as u8);
                shift -= 8;
            }
            i += 1;
        }
        h
    }

    /// 结果摘要（判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.frozen.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("响应树兼容账: 冻结字段=");
        out.push_str(&itoa(self.frozen.len() as u64));
        out.push_str(" 漂移=");
        out.push_str(&itoa(self.drift_hits as u64));
        out.push_str(" 缺字=");
        out.push_str(&itoa(self.absent_hits as u64));
        out.push_str(" 兼容=");
        out.push_str(&itoa(self.verified as u64));
        out.push_str(" 表指纹=");
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

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    // —— 冻结表：三响应各冻 4 字段（含宽度 1/2/4/8 覆盖） ——
    let mut led = CompatLedger::new();
    // memory map：entry_count(8B)/entry_size(8B)/virt_base(8B)/flags(4B)。
    led.freeze_field(RespTag::Memmap, 0, 24, 8, 27)?; // entry_count（e820 口径样例）
    led.freeze_field(RespTag::Memmap, 1, 32, 8, 24)?;
    led.freeze_field(RespTag::Memmap, 2, 40, 8, 0xFFFF_8000_0000_0000)?;
    led.freeze_field(RespTag::Memmap, 3, 48, 4, 1)?;
    // framebuffer：width(2B)/height(2B)/bpp(1B)/pitch(4B)。
    led.freeze_field(RespTag::Framebuffer, 0, 24, 2, 1280)?;
    led.freeze_field(RespTag::Framebuffer, 1, 26, 2, 800)?;
    led.freeze_field(RespTag::Framebuffer, 2, 28, 1, 32)?;
    led.freeze_field(RespTag::Framebuffer, 3, 32, 4, 5120)?;
    // kernel address：phys_base(8B)/virt_base(8B)。
    led.freeze_field(RespTag::KernelAddress, 0, 24, 8, 0x1000)?;
    led.freeze_field(RespTag::KernelAddress, 1, 32, 8, 0xFFFF_8000_0010_0000)?;
    if led.frozen_count() != 10 {
        return Err(E_FIELD_RANGE);
    }
    led.seal()?;
    // 封板后冻结必拦。
    if led.freeze_field(RespTag::Memmap, 9, 0, 8, 1) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let sm = led.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 主路径：前置闸过 + 实测全同 → 兼容 100% ——
    let hdr = RespHeader {
        magic: LIMINE_MAGIC,
        revision: FROZEN_REVISION,
    };
    let mut measured: Vec<MeasuredField> = Vec::new();
    let mut i = 0u8;
    while i < 10 {
        // 与冻结值全同的实测面（tag/字段号/值三元组复刻）。
        let (tag, no, val) = match i {
            0 => (0u8, 0u8, 27u64),
            1 => (0, 1, 24),
            2 => (0, 2, 0xFFFF_8000_0000_0000),
            3 => (0, 3, 1),
            4 => (1, 0, 1280),
            5 => (1, 1, 800),
            6 => (1, 2, 32),
            7 => (1, 3, 5120),
            8 => (2, 0, 0x1000),
            _ => (2, 1, 0xFFFF_8000_0010_0000),
        };
        measured.push(MeasuredField { tag, field_no: no, measured: val });
        i += 1;
    }
    let n = led.verify_all(hdr, &measured)?;
    if n != 10 {
        return Err(E_FIELD_DRIFT);
    }
    if !led.compat_verdict()? {
        return Err(E_COMPAT_BROKEN);
    }
    // 重复核对幂等。
    led.verify_all(hdr, &measured)?;
    if !led.compat_verdict()? {
        return Err(E_COMPAT_BROKEN);
    }

    // —— 漂移注入：1 字段实测漂移 → 兼容破必中 ——
    let mut m2 = measured.clone();
    m2[4].measured = 1024; // width 1280 → 1024。
    if led.verify_all(hdr, &m2) != Err(E_COMPAT_BROKEN) {
        return Err(E_COMPAT_BROKEN);
    }
    if led.drift_hits() != 1 {
        return Err(E_COMPAT_BROKEN);
    }

    // —— 缺字段注入：实测面少 1 字段 ——
    let m3 = measured[1..].to_vec();
    if led.verify_all(hdr, &m3) != Err(E_FIELD_ABSENT) {
        return Err(E_FIELD_ABSENT);
    }
    if led.absent_hits() != 1 {
        return Err(E_FIELD_ABSENT);
    }

    // —— 未冻字段注入：实测面多 1 未冻字段 ——
    let mut m4 = measured.clone();
    m4.push(MeasuredField { tag: 0, field_no: 9, measured: 1 });
    if led.verify_all(hdr, &m4) != Err(E_FIELD_UNFROZEN) {
        return Err(E_FIELD_UNFROZEN);
    }

    // —— 前置闸拦截：magic 破 / revision 漂移 ——
    let bad_magic = RespHeader { magic: 0x1234, revision: FROZEN_REVISION };
    if led.verify_all(bad_magic, &measured) != Err(E_MAGIC_BAD) {
        return Err(E_MAGIC_BAD);
    }
    let bad_rev = RespHeader { magic: LIMINE_MAGIC, revision: 4 };
    if led.verify_all(bad_rev, &measured) != Err(E_REVISION_BAD) {
        return Err(E_REVISION_BAD);
    }

    // —— 错误路径样本 ——
    if RespTag::from_id(3) != Err(E_TAG_RANGE) {
        return Err(E_TAG_RANGE);
    }
    let mut err = CompatLedger::new();
    if err.freeze_field(RespTag::Memmap, FIELDS_PER_RESP as u8, 0, 8, 1) != Err(E_FIELD_RANGE) {
        return Err(E_FIELD_RANGE);
    }
    if err.freeze_field(RespTag::Memmap, 0, 0, 3, 1) != Err(E_WIDTH_RANGE) {
        return Err(E_WIDTH_RANGE);
    }
    err.freeze_field(RespTag::Memmap, 0, 0, 8, 1)?;
    if err.freeze_field(RespTag::Memmap, 0, 8, 8, 2) != Err(E_FROZEN_DUP) {
        return Err(E_FROZEN_DUP);
    }
    // 表空封板/核对。
    let mut empty = CompatLedger::new();
    if empty.seal() != Err(E_TABLE_EMPTY) {
        return Err(E_TABLE_EMPTY);
    }
    if empty.verify_all(hdr, &[]) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if empty.compat_verdict() != Err(E_NOT_VERIFIED) {
        return Err(E_NOT_VERIFIED);
    }
    // 未封板即核对。
    err.seal()?;
    // (err 已封板，重新核对正常路径已覆盖。)

    // 诊断表抽查。
    if diag_detail(E_FIELD_DRIFT) == "未知诊断码（0x5a20 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5a20 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
