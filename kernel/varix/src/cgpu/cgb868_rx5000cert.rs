//! cgb868_rx5000cert —— CGPU-F0868 RX 5000/Vega 基础档认证。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0868 RX 5000/Vega 基础档认证  <SEG 0x7700>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0868；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（RX 5000/Vega 基础档认证，整数账面）：
//!   1. 认证项闭集七项（写死）：GCN5/RDNA1 指令集 / 基础显存路
//!      径 / 显示链路 / 电源治理 / 驱动白名单 / 基准下限 / 崩溃
//!      率上界；项号 0..=6；
//!   2. SKU 闭集（写死）：5700XT / 5700 / 5600XT / 5500XT /
//!      Vega64 / Vega56 / Vega FE / R9 Fury；越界红码
//!      （XC_BAD_SKU）；
//!   3. 基础档门槛（写死）：GCN 代际 ≤ RDNA1 且 CU ≥ 36；全收
//!      但按代际走差异表（RDNA1 全项；GCN5 免项 6 崩溃率上界改
//!      宽限；Vega/Fury 免 SAM 类检查）；
//!   4. 认证状态机：Submitted → Testing → Pass / Fail；Fail 重测
//!      上限 2 次；
//!   5. 认证账（定容 20 台）守恒：送测数 = 过证数 + 挂证数 + 测
//!      中数；漂移红码（XC_CERT_DRIFT）；
//!   6. 出证判据：代际差异表生效后的必测项全 Pass；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7700 段为本模块独占（SEG_BASE=0x7700），判别式
//!   SEG_BASE+1..+N；与 0x76E0/0x7720 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const R5_VERSION: &str = "cgb868_rx5000cert-v1";

/// 诊断段基址（0x7700 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7700;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7701 认证项号越界（0..=6 之外）。
pub const XC_BAD_ITEM: u16 = SEG_BASE + 1;
/// 0x7702 认证状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x7703 认证账漂移。
pub const XC_CERT_DRIFT: u16 = SEG_BASE + 3;
/// 0x7704 SKU 越界。
pub const XC_BAD_SKU: u16 = SEG_BASE + 4;
/// 0x7705 档位不符（CU < 36）。
pub const XC_TIER_MISMATCH: u16 = SEG_BASE + 5;
/// 0x7706 重测超限。
pub const XC_RETEST_EXHAUSTED: u16 = SEG_BASE + 6;
/// 0x7707 认证未齐即出证。
pub const XC_CERT_INCOMPLETE: u16 = SEG_BASE + 7;
/// 0x7708 设备号越界。
pub const XC_BAD_DEVICE: u16 = SEG_BASE + 8;
/// 0x7709 认证账满（20）。
pub const XC_CERT_FULL: u16 = SEG_BASE + 9;
/// 0x770A 代际差异表越界。
pub const XC_BAD_GEN_TABLE: u16 = SEG_BASE + 10;

/// 认证项数（写死：7）。
pub const ITEM_CAP: usize = 7;
/// 认证账容量（写死：20）。
pub const CERT_CAP: usize = 20;
/// 重测上限（写死：2）。
pub const RETEST_MAX: u32 = 2;
/// CU 门槛（写死：36）。
pub const CU_FLOOR: u32 = 36;

/// SKU 闭集。
pub const S_5700XT: u8 = 0;
pub const S_5700: u8 = 1;
pub const S_5600XT: u8 = 2;
pub const S_5500XT: u8 = 3;
pub const S_VEGA64: u8 = 4;
pub const S_VEGA56: u8 = 5;
pub const S_VEGAFE: u8 = 6;
pub const S_FURY: u8 = 7;
/// SKU 上限。
pub const SKU_CAP: u8 = 7;

/// 代际闭集：0=RDNA1 1=GCN5 2=Vega 3=Fury。
pub const G_RDNA1: u8 = 0;
pub const G_GCN5: u8 = 1;
pub const G_VEGA: u8 = 2;
pub const G_FURY: u8 = 3;
/// 代际上限。
pub const GEN_CAP: u8 = 3;

/// SKU 规格：[CU, 代际]（写死）。
pub const SKU_CU: [u32; 8] = [40, 36, 36, 20, 64, 56, 64, 56];
pub const SKU_GEN: [u8; 8] = [
    G_RDNA1, G_RDNA1, G_RDNA1, G_RDNA1, G_VEGA, G_VEGA, G_VEGA, G_FURY,
];

/// 代际差异表：每代际必测项位图（bit = 项号；写死）。
/// RDNA1 全测（0..=6 → 0b1111111）；GCN5 免项 6（0b0111111）；
/// Vega 免项 6（同 GCN5）；Fury 免项 1/6（0b0111101）。
pub const GEN_REQUIRED: [[bool; ITEM_CAP]; 4] = [
    [true, true, true, true, true, true, true],
    [true, true, true, true, true, true, false],
    [true, true, true, true, true, true, false],
    [true, false, true, true, true, true, false],
];

/// 基础档资格判定。
pub fn basic_eligible(sku: u8) -> Result<bool, u16> {
    if sku > SKU_CAP {
        return Err(XC_BAD_SKU);
    }
    Ok(SKU_CU[sku as usize] >= CU_FLOOR)
}

/// SKU 名（闭集原文）。
pub fn sku_name(sku: u8) -> &'static str {
    match sku {
        S_5700XT => "5700XT",
        S_5700 => "5700",
        S_5600XT => "5600XT",
        S_5500XT => "5500XT",
        S_VEGA64 => "Vega64",
        S_VEGA56 => "Vega56",
        S_VEGAFE => "VegaFE",
        _ => "Fury",
    }
}

/// 代际名。
pub fn gen_name(g: u8) -> &'static str {
    match g {
        G_RDNA1 => "RDNA1",
        G_GCN5 => "GCN5",
        G_VEGA => "Vega",
        _ => "Fury",
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
    /// 七项状态。
    pub items: [CState; ITEM_CAP],
    /// 各项重测计数。
    pub retests: [u32; ITEM_CAP],
}

impl CertDevice {
    /// 必测项全 Pass（按代际差异表）。
    pub fn required_pass(&self) -> Result<bool, u16> {
        let gen = SKU_GEN[self.sku as usize];
        if gen > GEN_CAP {
            return Err(XC_BAD_GEN_TABLE);
        }
        let mut i = 0;
        while i < ITEM_CAP {
            if GEN_REQUIRED[gen as usize][i] && self.items[i] != CState::Pass {
                return Ok(false);
            }
            i += 1;
        }
        Ok(true)
    }
}

/// RX 5000/Vega 基础档认证册。
pub struct Rx5000Cert {
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

impl Rx5000Cert {
    /// 空册。
    pub fn new() -> Self {
        Rx5000Cert {
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
        if !basic_eligible(sku)? {
            self.red_count += 1;
            return Err(XC_TIER_MISMATCH);
        }
        if self.has_sig(sig) {
            self.red_count += 1;
            return Err(XC_DUP_DEV);
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

    /// 项开测（免测项拒绝开测）。
    pub fn test(&mut self, idx: usize, item: usize) -> Result<(), u16> {
        if idx >= self.rows.len() {
            self.red_count += 1;
            return Err(XC_BAD_DEVICE);
        }
        if item >= ITEM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_ITEM);
        }
        let gen = SKU_GEN[self.rows[idx].sku as usize];
        if !GEN_REQUIRED[gen as usize][item] {
            self.red_count += 1;
            return Err(XC_BAD_GEN_TABLE);
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

    /// 出证（必测项全过）。
    pub fn issue(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.rows.len() {
            self.red_count += 1;
            return Err(XC_BAD_DEVICE);
        }
        if !self.balanced() {
            self.red_count += 1;
            return Err(XC_CERT_DRIFT);
        }
        if !self.rows[idx].required_pass()? {
            self.red_count += 1;
            return Err(XC_CERT_INCOMPLETE);
        }
        self.issued += 1;
        Ok(())
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("rx5000cert devices=");
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

/// 重复送测错误码（0x770B）。
pub const XC_DUP_DEV: u16 = SEG_BASE + 11;

/// 自例程：三代际各送测 → 差异表出证 → 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut c = Rx5000Cert::new();
    let a = c.submit("oem-5700xt", S_5700XT)?;
    let b = c.submit("oem-vega64", S_VEGA64)?;
    let d = c.submit("oem-fury", S_FURY)?;
    // 负例：SKU 越界。
    if c.submit("bad", 8) != Err(XC_BAD_SKU) {
        return Err(XC_BAD_SKU);
    }
    // 负例：5500XT（CU=20）不达门槛。
    if c.submit("rx5500xt", S_5500XT) != Err(XC_TIER_MISMATCH) {
        return Err(XC_TIER_MISMATCH);
    }
    // 负例：重复。
    if c.submit("oem-5700xt", S_5700XT) != Err(XC_DUP_DEV) {
        return Err(XC_DUP_DEV);
    }
    // RDNA1 a 链：全七项。
    let mut i = 0;
    while i < ITEM_CAP {
        c.test(a, i)?;
        c.conclude(a, i, true)?;
        i += 1;
    }
    c.issue(a)?;
    c.audit()?;
    // Vega b 链：免项 6（测它会红）。
    if c.test(b, 6) != Err(XC_BAD_GEN_TABLE) {
        return Err(XC_BAD_GEN_TABLE);
    }
    let mut i2 = 0;
    while i2 < ITEM_CAP - 1 {
        c.test(b, i2)?;
        c.conclude(b, i2, true)?;
        i2 += 1;
    }
    c.issue(b)?;
    c.audit()?;
    // Fury d 链：免项 1/6。
    if c.test(d, 1) != Err(XC_BAD_GEN_TABLE) {
        return Err(XC_BAD_GEN_TABLE);
    }
    if c.test(d, 6) != Err(XC_BAD_GEN_TABLE) {
        return Err(XC_BAD_GEN_TABLE);
    }
    let mut i3 = 0;
    while i3 < ITEM_CAP {
        if i3 != 1 && i3 != 6 {
            c.test(d, i3)?;
            c.conclude(d, i3, true)?;
        }
        i3 += 1;
    }
    c.issue(d)?;
    c.audit()?;
    // 负例：Fury 项 1 未测出证。
    let mut e = Rx5000Cert::new();
    let f = e.submit("fury-miss", S_FURY)?;
    let mut i4 = 0;
    while i4 < ITEM_CAP {
        if i4 != 1 && i4 != 6 {
            e.test(f, i4)?;
            e.conclude(f, i4, true)?;
        }
        i4 += 1;
    }
    if e.issue(f) != Err(XC_CERT_INCOMPLETE) {
        return Err(XC_CERT_INCOMPLETE);
    }
    // 负例：重测超限。
    let mut g = Rx5000Cert::new();
    let h = g.submit("flaky-vega", S_VEGA64)?;
    let mut k = 0;
    while k <= RETEST_MAX {
        g.test(h, 3)?;
        g.conclude(h, 3, false)?;
        k += 1;
    }
    if g.test(h, 3) != Err(XC_RETEST_EXHAUSTED) {
        return Err(XC_RETEST_EXHAUSTED);
    }
    if c.summary().len() == 0 || c.issued != 3 {
        return Err(XC_CERT_INCOMPLETE);
    }
    Ok(0)
}
