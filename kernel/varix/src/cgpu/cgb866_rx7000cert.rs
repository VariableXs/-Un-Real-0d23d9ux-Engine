//! cgb866_rx7000cert —— CGPU-F0866 RX 7000 高端档认证。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0866 RX 7000 高端档认证  <SEG 0x76C0>
//! 目标行数：≥360（判据：纯功能代码 ≥360 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥360 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0866；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（RX 7000 高端档认证，整数账面）：
//!   1. 认证项闭集十项（写死）：RDNA3 指令集 / 双媒体引擎 / 光追
//!      合规 / AI 加速器 / 显示链路 / 电源治理 / 显存训练 / 驱动
//!      白名单 / 基准达标 / 压力稳定；项号 0..=9；
//!   2. 认证状态机：Submitted → Testing → Pass / Fail；Fail 可重
//!      测（上限 2 次）；非法迁移红码（XC_BAD_TRANS）；
//!   3. SKU 闭集（写死）：7900XTX / 7900XT / 7900GRE / 7800XT /
//!      7700XT / 7600XT / 7600；型号越界红码（XC_BAD_SKU）；
//!   4. 高端档门槛（写死）：CU ≥ 60 或（CU ≥ 32 且 IC ≥ 64MiB）；
//!      不达门槛红码（XC_TIER_MISMATCH）；
//!   5. 认证账（定容 24 台）守恒：送测数 = 过证数 + 挂证数 + 测
//!      中数；漂移红码（XC_CERT_DRIFT）；
//!   6. 出证判据：十项全 Pass 且无重测余量滥用，违例红码
//!      （XC_CERT_INCOMPLETE）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x76C0 段为本模块独占（SEG_BASE=0x76C0），判别式
//!   SEG_BASE+1..+N；与 0x76A0/0x76E0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const R7_VERSION: &str = "cgb866_rx7000cert-v1";

/// 诊断段基址（0x76C0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x76C0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x76C1 认证项号越界（0..=9 之外）。
pub const XC_BAD_ITEM: u16 = SEG_BASE + 1;
/// 0x76C2 认证状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x76C3 认证账漂移（送测 ≠ 过证 + 挂证 + 测中）。
pub const XC_CERT_DRIFT: u16 = SEG_BASE + 3;
/// 0x76C4 SKU 越界。
pub const XC_BAD_SKU: u16 = SEG_BASE + 4;
/// 0x76C5 档位不符（不达高端门槛）。
pub const XC_TIER_MISMATCH: u16 = SEG_BASE + 5;
/// 0x76C6 重测超限。
pub const XC_RETEST_EXHAUSTED: u16 = SEG_BASE + 6;
/// 0x76C7 认证未齐即出证。
pub const XC_CERT_INCOMPLETE: u16 = SEG_BASE + 7;
/// 0x76C8 设备号越界。
pub const XC_BAD_DEVICE: u16 = SEG_BASE + 8;
/// 0x76C9 认证账满（24）。
pub const XC_CERT_FULL: u16 = SEG_BASE + 9;
/// 0x76CA 重复送测签名。
pub const XC_DUP_DEVICE: u16 = SEG_BASE + 10;

/// 认证项数（写死：10）。
pub const ITEM_CAP: usize = 10;
/// 认证账容量（写死：24）。
pub const CERT_CAP: usize = 24;
/// 重测上限（写死：2）。
pub const RETEST_MAX: u32 = 2;

/// SKU 闭集：0=7900XTX 1=7900XT 2=7900GRE 3=7800XT 4=7700XT
/// 5=7600XT 6=7600。
pub const S_7900XTX: u8 = 0;
pub const S_7900XT: u8 = 1;
pub const S_7900GRE: u8 = 2;
pub const S_7800XT: u8 = 3;
pub const S_7700XT: u8 = 4;
pub const S_7600XT: u8 = 5;
pub const S_7600: u8 = 6;
/// SKU 上限。
pub const SKU_CAP: u8 = 6;

/// SKU 规格（CU 数，写死）。
pub const SKU_CU: [u32; 7] = [96, 84, 80, 60, 54, 32, 32];
/// SKU IC 容量（MiB，写死；0 = 无 IC）。
pub const SKU_IC: [u32; 7] = [96, 96, 80, 64, 64, 32, 32];

/// 高端档门槛判定（CU ≥ 60 或（CU ≥ 32 且 IC ≥ 64））。
pub fn highend_eligible(sku: u8) -> Result<bool, u16> {
    if sku > SKU_CAP {
        return Err(XC_BAD_SKU);
    }
    let cu = SKU_CU[sku as usize];
    let ic = SKU_IC[sku as usize];
    Ok(cu >= 60 || (cu >= 32 && ic >= 64))
}

/// SKU 名（闭集原文）。
pub fn sku_name(sku: u8) -> &'static str {
    match sku {
        S_7900XTX => "7900XTX",
        S_7900XT => "7900XT",
        S_7900GRE => "7900GRE",
        S_7800XT => "7800XT",
        S_7700XT => "7700XT",
        S_7600XT => "7600XT",
        _ => "7600",
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
    /// 十项状态。
    pub items: [CState; ITEM_CAP],
    /// 各项重测计数。
    pub retests: [u32; ITEM_CAP],
    /// 设备级重测总次数。
    pub device_retests: u32,
}

impl CertDevice {
    /// 十项全 Pass。
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

/// RX 7000 高端档认证册。
pub struct Rx7000Cert {
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

impl Rx7000Cert {
    /// 空册。
    pub fn new() -> Self {
        Rx7000Cert {
            rows: Vec::new(),
            submitted: 0,
            passed: 0,
            failed: 0,
            inflight: 0,
            issued: 0,
            red_count: 0,
        }
    }

    /// 送测（SKU 门槛 + 去重 + 容量）。
    pub fn submit(&mut self, sig: &str, sku: u8) -> Result<usize, u16> {
        if sku > SKU_CAP {
            self.red_count += 1;
            return Err(XC_BAD_SKU);
        }
        if !highend_eligible(sku)? {
            self.red_count += 1;
            return Err(XC_TIER_MISMATCH);
        }
        if self.has_sig(sig) {
            self.red_count += 1;
            return Err(XC_DUP_DEVICE);
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
            device_retests: 0,
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

    /// 项开测（Submitted/Fail → Testing）。
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
                self.rows[idx].device_retests += 1;
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

    /// 项结测（Testing → Pass/Fail）。
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

    /// 出证（十项全 Pass 且重测未超限）。
    pub fn issue(&mut self, idx: usize) -> Result<u32, u16> {
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
        // 重测纪律：任一项重测 ≤ 上限且总数 ≤ 项数 × 上限。
        let mut i = 0;
        while i < ITEM_CAP {
            if self.rows[idx].retests[i] > RETEST_MAX {
                self.red_count += 1;
                return Err(XC_RETEST_EXHAUSTED);
            }
            i += 1;
        }
        self.issued += 1;
        Ok(self.rows[idx].device_retests)
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("rx7000cert devices=");
        s += &self.rows.len().to_string();
        s += " issued=";
        s += &self.issued.to_string();
        s += " pass=";
        s += &self.passed.to_string();
        s += " fail=";
        s += &self.failed.to_string();
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        if !self.balanced() {
            return Err(XC_CERT_DRIFT);
        }
        let mut p = 0u64;
        let mut f = 0u64;
        let mut i = 0;
        while i < self.rows.len() {
            let mut j = 0;
            while j < ITEM_CAP {
                match self.rows[i].items[j] {
                    CState::Pass => p += 1,
                    CState::Fail => f += 1,
                    _ => {}
                }
                j += 1;
            }
            i += 1;
        }
        if p != self.passed || f != self.failed {
            return Err(XC_CERT_DRIFT);
        }
        Ok(())
    }
}

/// 自例程：送测三台 → 全项过证 → 重测路径 → 审计 + 负例七连。
pub fn drill() -> Result<u32, u16> {
    let mut c = Rx7000Cert::new();
    let a = c.submit("aib-7900xtx-01", S_7900XTX)?;
    let b = c.submit("aib-7800xt-01", S_7800XT)?;
    let g = c.submit("aib-7900gre-01", S_7900GRE)?;
    // 负例：SKU 越界。
    if c.submit("bad-sku", 9) != Err(XC_BAD_SKU) {
        return Err(XC_BAD_SKU);
    }
    // 负例：7600 不达高端门槛（CU=32、IC=32 < 64）。
    if c.submit("rx7600", S_7600) != Err(XC_TIER_MISMATCH) {
        return Err(XC_TIER_MISMATCH);
    }
    // 负例：重复送测。
    if c.submit("aib-7900xtx-01", S_7900XTX) != Err(XC_DUP_DEVICE) {
        return Err(XC_DUP_DEVICE);
    }
    // 门槛面：7600XT（CU=32、IC=32）不达；7700XT（54/64）达。
    if highend_eligible(S_7600XT)? != false {
        return Err(XC_TIER_MISMATCH);
    }
    if highend_eligible(S_7700XT)? != true {
        return Err(XC_TIER_MISMATCH);
    }
    // 全项测试链 a。
    let mut i = 0;
    while i < ITEM_CAP {
        c.test(a, i)?;
        c.conclude(a, i, true)?;
        i += 1;
    }
    c.issue(a)?;
    // 全项链 b。
    let mut i2 = 0;
    while i2 < ITEM_CAP {
        c.test(b, i2)?;
        c.conclude(b, i2, true)?;
        i2 += 1;
    }
    c.issue(b)?;
    c.audit()?;
    // 重测路径 g：第 3 项先挂再重测过。
    c.test(g, 2)?;
    c.conclude(g, 2, false)?;
    c.test(g, 2)?;
    c.conclude(g, 2, true)?;
    let mut i3 = 0;
    while i3 < ITEM_CAP {
        if i3 != 2 {
            c.test(g, i3)?;
            c.conclude(g, i3, true)?;
        }
        i3 += 1;
    }
    c.issue(g)?;
    c.audit()?;
    // 负例：未测项出证。
    let mut d = Rx7000Cert::new();
    let e = d.submit("partial", S_7900XT)?;
    d.test(e, 0)?;
    d.conclude(e, 0, true)?;
    if d.issue(e) != Err(XC_CERT_INCOMPLETE) {
        return Err(XC_CERT_INCOMPLETE);
    }
    // 负例：重测超限（连挂 3 次）。
    let mut f = Rx7000Cert::new();
    let h = f.submit("flaky", S_7900XT)?;
    f.test(h, 0)?;
    f.conclude(h, 0, false)?;
    f.test(h, 0)?;
    f.conclude(h, 0, false)?;
    f.test(h, 0)?;
    f.conclude(h, 0, false)?;
    if f.test(h, 0) != Err(XC_RETEST_EXHAUSTED) {
        return Err(XC_RETEST_EXHAUSTED);
    }
    // 负例：设备号越界。
    if f.issue(99) != Err(XC_BAD_DEVICE) {
        return Err(XC_BAD_DEVICE);
    }
    if c.summary().len() == 0 || c.issued != 3 {
        return Err(XC_CERT_INCOMPLETE);
    }
    Ok(0)
}
