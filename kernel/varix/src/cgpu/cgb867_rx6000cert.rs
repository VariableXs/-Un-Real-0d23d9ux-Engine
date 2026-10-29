//! cgb867_rx6000cert —— CGPU-F0867 RX 6000 主流档认证。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0867 RX 6000 主流档认证  <SEG 0x76E0>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0867；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（RX 6000 主流档认证，整数账面）：
//!   1. 认证项闭集八项（写死）：RDNA2 指令集 / 无限缓存合规 /
//!      SAM 支持面 / 显示链路 / 电源治理 / 显存训练 / 驱动白名
//!      单 / 基准达标；项号 0..=7；
//!   2. SKU 闭集（写死）：6950XT / 6900XT / 6800XT / 6800 /
//!      6750XT / 6700XT / 6650XT / 6600 / 6500XT / 6400；越界红
//!      码（XC_BAD_SKU）；
//!   3. 主流档门槛（写死）：CU ≥ 36 且 IC ≥ 64MiB；6600 及以下
//!      （IC < 64）不收，红码（XC_TIER_MISMATCH）；
//!   4. 认证状态机：Submitted → Testing → Pass / Fail；Fail 重测
//!      上限 2 次；
//!   5. 认证账（定容 20 台）守恒：送测数 = 过证数 + 挂证数 + 测
//!      中数；漂移红码（XC_CERT_DRIFT）；
//!   6. 出证判据：八项全 Pass；SAM 支持面项（项 2）必须过（主流
//!      档硬性项）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x76E0 段为本模块独占（SEG_BASE=0x76E0），判别式
//!   SEG_BASE+1..+N；与 0x76C0/0x7700 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const R6_VERSION: &str = "cgb867_rx6000cert-v1";

/// 诊断段基址（0x76E0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x76E0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x76E1 认证项号越界（0..=7 之外）。
pub const XC_BAD_ITEM: u16 = SEG_BASE + 1;
/// 0x76E2 认证状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x76E3 认证账漂移。
pub const XC_CERT_DRIFT: u16 = SEG_BASE + 3;
/// 0x76E4 SKU 越界。
pub const XC_BAD_SKU: u16 = SEG_BASE + 4;
/// 0x76E5 档位不符。
pub const XC_TIER_MISMATCH: u16 = SEG_BASE + 5;
/// 0x76E6 重测超限。
pub const XC_RETEST_EXHAUSTED: u16 = SEG_BASE + 6;
/// 0x76E7 认证未齐即出证。
pub const XC_CERT_INCOMPLETE: u16 = SEG_BASE + 7;
/// 0x76E8 设备号越界。
pub const XC_BAD_DEVICE: u16 = SEG_BASE + 8;
/// 0x76E9 认证账满（20）。
pub const XC_CERT_FULL: u16 = SEG_BASE + 9;
/// 0x76EA 硬性项未过即出证。
pub const XC_HARDITEM_MISS: u16 = SEG_BASE + 10;

/// 认证项数（写死：8）。
pub const ITEM_CAP: usize = 8;
/// 认证账容量（写死：20）。
pub const CERT_CAP: usize = 20;
/// 重测上限（写死：2）。
pub const RETEST_MAX: u32 = 2;
/// 硬性项号（SAM 支持面）。
pub const HARD_ITEM: usize = 2;

/// SKU 闭集。
pub const S_6950XT: u8 = 0;
pub const S_6900XT: u8 = 1;
pub const S_6800XT: u8 = 2;
pub const S_6800: u8 = 3;
pub const S_6750XT: u8 = 4;
pub const S_6700XT: u8 = 5;
pub const S_6650XT: u8 = 6;
pub const S_6600: u8 = 7;
pub const S_6500XT: u8 = 8;
pub const S_6400: u8 = 9;
/// SKU 上限。
pub const SKU_CAP: u8 = 9;

/// SKU CU 数（写死）。
pub const SKU_CU: [u32; 10] = [80, 72, 72, 60, 40, 40, 32, 28, 16, 12];
/// SKU IC 容量（MiB，写死）。
pub const SKU_IC: [u32; 10] = [128, 128, 128, 128, 96, 96, 32, 32, 16, 16];

/// 主流档门槛判定。
pub fn mainstream_eligible(sku: u8) -> Result<bool, u16> {
    if sku > SKU_CAP {
        return Err(XC_BAD_SKU);
    }
    let cu = SKU_CU[sku as usize];
    let ic = SKU_IC[sku as usize];
    Ok(cu >= 36 && ic >= 64)
}

/// SKU 名（闭集原文）。
pub fn sku_name(sku: u8) -> &'static str {
    match sku {
        S_6950XT => "6950XT",
        S_6900XT => "6900XT",
        S_6800XT => "6800XT",
        S_6800 => "6800",
        S_6750XT => "6750XT",
        S_6700XT => "6700XT",
        S_6650XT => "6650XT",
        S_6600 => "6600",
        S_6500XT => "6500XT",
        _ => "6400",
    }
}

/// 认证项状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CState {
    /// 已送测。
    Submitted,
    /// 测中。
    Testing,
    /// 过。
    Pass,
    /// 挂。
    Fail,
}

/// 单台送测设备。
#[derive(Clone, Debug)]
pub struct CertDevice {
    /// 签名（去重键）。
    pub sig: String,
    /// SKU 号。
    pub sku: u8,
    /// 八项状态。
    pub items: [CState; ITEM_CAP],
    /// 各项重测计数。
    pub retests: [u32; ITEM_CAP],
}

impl CertDevice {
    /// 八项全 Pass。
    pub fn all_pass(&self) -> bool {
        let mut i = 0;
        while i < ITEM_CAP {
            if self.items[i] != CState::Pass {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// RX 6000 主流档认证册。
pub struct Rx6000Cert {
    /// 送测账。
    pub rows: Vec<CertDevice>,
    /// 送测数。
    pub submitted: u64,
    /// 过证数。
    pub passed: u64,
    /// 挂证数。
    pub failed: u64,
    /// 测中数。
    pub inflight: u64,
    /// 出证数。
    pub issued: u64,
    /// 累计红码。
    pub red_count: u64,
}

impl Rx6000Cert {
    /// 空册。
    pub fn new() -> Self {
        Rx6000Cert {
            rows: Vec::new(),
            submitted: 0,
            passed: 0,
            failed: 0,
            inflight: 0,
            issued: 0,
            red_count: 0,
        }
    }

    /// 送测。
    pub fn submit(&mut self, sig: &str, sku: u8) -> Result<usize, u16> {
        if sku > SKU_CAP {
            self.red_count += 1;
            return Err(XC_BAD_SKU);
        }
        if !mainstream_eligible(sku)? {
            self.red_count += 1;
            return Err(XC_TIER_MISMATCH);
        }
        if self.has_sig(sig) {
            self.red_count += 1;
            return Err(XC_DUP_DEVICE_RO);
        }
        if self.rows.len() == CERT_CAP {
            self.red_count += 1;
            return Err(XC_CERT_FULL);
        }
        self.rows.push(CertDevice {
            sig: String::from(sig),
            sku,
            items: [CState::Submitted; ITEM_CAP],
            retests: [0; ITEM_CAP],
        });
        self.submitted += 1;
        self.inflight += 1;
        Ok(self.rows.len() - 1)
    }

    /// 签名查重。
    pub fn has_sig(&self, sig: &str) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].sig == sig {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 项开测。
    pub fn test(&mut self, idx: usize, item: usize) -> Result<(), u16> {
        if idx >= self.rows.len() {
            self.red_count += 1;
            return Err(XC_BAD_DEVICE);
        }
        if item >= ITEM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_ITEM);
        }
        let cur = self.rows[idx].items[item];
        match cur {
            CState::Submitted => {}
            CState::Fail => {
                if self.rows[idx].retests[item] + 1 > RETEST_MAX {
                    self.red_count += 1;
                    return Err(XC_RETEST_EXHAUSTED);
                }
                self.rows[idx].retests[item] += 1;
                self.failed -= 1;
                self.inflight += 1;
            }
            _ => {
                self.red_count += 1;
                return Err(XC_BAD_TRANS);
            }
        }
        self.rows[idx].items[item] = CState::Testing;
        Ok(())
    }

    /// 项结测。
    pub fn conclude(&mut self, idx: usize, item: usize, ok: bool) -> Result<(), u16> {
        if idx >= self.rows.len() {
            self.red_count += 1;
            return Err(XC_BAD_DEVICE);
        }
        if item >= ITEM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_ITEM);
        }
        if self.rows[idx].items[item] != CState::Testing {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        self.rows[idx].items[item] = if ok { CState::Pass } else { CState::Fail };
        self.inflight -= 1;
        if ok {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        Ok(())
    }

    /// 账守恒。
    pub fn balanced(&self) -> bool {
        self.submitted == self.passed + self.failed + self.inflight
    }

    /// 出证（全 Pass + 硬性项）。
    pub fn issue(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.rows.len() {
            self.red_count += 1;
            return Err(XC_BAD_DEVICE);
        }
        if !self.balanced() {
            self.red_count += 1;
            return Err(XC_CERT_DRIFT);
        }
        if !self.rows[idx].all_pass() {
            self.red_count += 1;
            return Err(XC_CERT_INCOMPLETE);
        }
        if self.rows[idx].items[HARD_ITEM] != CState::Pass {
            self.red_count += 1;
            return Err(XC_HARDITEM_MISS);
        }
        self.issued += 1;
        Ok(())
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("rx6000cert devices=");
        s += &self.rows.len().to_string();
        s += " issued=";
        s += &self.issued.to_string();
        s += " pass=";
        s += &self.passed.to_string();
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_CERT_DRIFT);
        }
        Ok(())
    }
}

/// 重复送测的独立错误码（0x76EB）。
pub const XC_DUP_DEVICE_RO: u16 = SEG_BASE + 11;

/// 自例程：送测 → 全项过证 → 硬性项负例 → 审计 + 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut c = Rx6000Cert::new();
    let a = c.submit("oem-6800xt-01", S_6800XT)?;
    let b = c.submit("oem-6700xt-01", S_6700XT)?;
    // 负例：SKU 越界。
    if c.submit("bad", 10) != Err(XC_BAD_SKU) {
        return Err(XC_BAD_SKU);
    }
    // 负例：6600（CU=28）不达门槛。
    if c.submit("rx6600", S_6600) != Err(XC_TIER_MISMATCH) {
        return Err(XC_TIER_MISMATCH);
    }
    // 负例：重复。
    if c.submit("oem-6800xt-01", S_6800XT) != Err(XC_DUP_DEVICE_RO) {
        return Err(XC_DUP_DEVICE_RO);
    }
    // 门槛面复核。
    if mainstream_eligible(S_6950XT)? != true {
        return Err(XC_TIER_MISMATCH);
    }
    if mainstream_eligible(S_6500XT)? != false {
        return Err(XC_TIER_MISMATCH);
    }
    // 全项链 a。
    let mut i = 0;
    while i < ITEM_CAP {
        c.test(a, i)?;
        c.conclude(a, i, true)?;
        i += 1;
    }
    c.issue(a)?;
    c.audit()?;
    // b 链：硬性项先挂两次再过。
    let mut i2 = 0;
    while i2 < ITEM_CAP {
        if i2 != HARD_ITEM {
            c.test(b, i2)?;
            c.conclude(b, i2, true)?;
        }
        i2 += 1;
    }
    c.test(b, HARD_ITEM)?;
    c.conclude(b, HARD_ITEM, false)?;
    c.test(b, HARD_ITEM)?;
    c.conclude(b, HARD_ITEM, false)?;
    c.test(b, HARD_ITEM)?;
    c.conclude(b, HARD_ITEM, true)?;
    c.issue(b)?;
    c.audit()?;
    // 负例：硬性项挂时出证。
    let mut d = Rx6000Cert::new();
    let e = d.submit("hardmiss", S_6800)?;
    let mut i3 = 0;
    while i3 < ITEM_CAP {
        d.test(e, i3)?;
        d.conclude(e, i3, i3 != HARD_ITEM)?;
        i3 += 1;
    }
    if d.issue(e) != Err(XC_CERT_INCOMPLETE) {
        return Err(XC_HARDITEM_MISS);
    }
    // 负例：重测超限。
    let mut f = Rx6000Cert::new();
    let g = f.submit("flaky", S_6800)?;
    let mut k = 0;
    while k <= RETEST_MAX {
        f.test(g, 0)?;
        f.conclude(g, 0, false)?;
        k += 1;
    }
    if f.test(g, 0) != Err(XC_RETEST_EXHAUSTED) {
        return Err(XC_RETEST_EXHAUSTED);
    }
    // 负例：设备号越界。
    if d.issue(99) != Err(XC_BAD_DEVICE) {
        return Err(XC_BAD_DEVICE);
    }
    if c.summary().len() == 0 || c.issued != 2 {
        return Err(XC_CERT_INCOMPLETE);
    }
    Ok(0)
}
