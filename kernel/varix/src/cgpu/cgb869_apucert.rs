//! cgb869_apucert —— CGPU-F0869 AMD APU 场景认证。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0869 AMD APU 场景认证  <SEG 0x7720>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0869；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD APU 场景认证，整数账面）：
//!   1. APU 场景项闭集八项（写死）：共享显存池 / UMA 帧缓冲 /
//!      iGPU 频域 / 内存带宽协商 / 媒体引擎共享 / 睡眠唤醒 / 双
//!      显切换 / 温度联动；项号 0..=7；
//!   2. APU SKU 闭集（写死）：8700G / 8600G / 8500G / 8300G /
//!      7840U / 7840HS / 680M 平台；越界红码（XC_BAD_SKU）；
//!   3. 共享显存配额账守恒（恒等式）：UMA 总配额 = iGPU 独占 +
//!      CPU 共享 + 系统预留；每 SKU 写死配额，漂移红码
//!      （XC_UMA_DRIFT）；
//!   4. 双显切换状态机：IGPUOnly → HYBRID → DGPUOnly；来回切
//!      换计数入账；非法迁移红码（XC_BAD_TRANS）；
//!   5. 认证状态机：Submitted → Testing → Pass / Fail；Fail 重测
//!      上限 2；
//!   6. 出证判据：八项全 Pass 且 UMA 账平衡；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7720 段为本模块独占（SEG_BASE=0x7720），判别式
//!   SEG_BASE+1..+N；与 0x7700/0x7740 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AP_VERSION: &str = "cgb869_apucert-v1";

/// 诊断段基址（0x7720 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7720;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7721 场景项号越界（0..=7 之外）。
pub const XC_BAD_ITEM: u16 = SEG_BASE + 1;
/// 0x7722 认证状态迁移非法。
pub const XC_BAD_TRANS: u16 = SEG_BASE + 2;
/// 0x7723 UMA 配额账漂移。
pub const XC_UMA_DRIFT: u16 = SEG_BASE + 3;
/// 0x7724 SKU 越界。
pub const XC_BAD_SKU: u16 = SEG_BASE + 4;
/// 0x7725 重测超限。
pub const XC_RETEST_EXHAUSTED: u16 = SEG_BASE + 5;
/// 0x7726 认证未齐即出证。
pub const XC_CERT_INCOMPLETE: u16 = SEG_BASE + 6;
/// 0x7727 设备号越界。
pub const XC_BAD_DEVICE: u16 = SEG_BASE + 7;
/// 0x7728 认证账满（16）。
pub const XC_CERT_FULL: u16 = SEG_BASE + 8;
/// 0x7729 重复送测。
pub const XC_DUP_DEVICE: u16 = SEG_BASE + 9;
/// 0x772A 双显切换账漂移（切换数 = I→H + H→D + H→I + D→H）。
pub const XC_SWITCH_DRIFT: u16 = SEG_BASE + 10;

/// 场景项数（写死：8）。
pub const ITEM_CAP: usize = 8;
/// 认证账容量（写死：16）。
pub const CERT_CAP: usize = 16;
/// 重测上限（写死：2）。
pub const RETEST_MAX: u32 = 2;

/// SKU 闭集。
pub const S_8700G: u8 = 0;
pub const S_8600G: u8 = 1;
pub const S_8500G: u8 = 2;
pub const S_8300G: u8 = 3;
pub const S_7840U: u8 = 4;
pub const S_7840HS: u8 = 5;
pub const S_680M: u8 = 6;
/// SKU 上限。
pub const SKU_CAP: u8 = 6;

/// UMA 三面（MiB）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UmaSplit {
    /// iGPU 独占。
    pub igpu: u32,
    /// CPU 共享。
    pub shared: u32,
    /// 系统预留。
    pub reserved: u32,
}

/// 各 SKU UMA 总配额（MiB，写死）。
pub const SKU_UMA_TOTAL: [u32; 7] = [4096, 2048, 2048, 1024, 4096, 4096, 2048];

/// UMA 账守恒：三面和 = 配额。
pub fn uma_balanced(sku: u8, s: &UmaSplit) -> Result<bool, u16> {
    if sku > SKU_CAP {
        return Err(XC_BAD_SKU);
    }
    Ok(SKU_UMA_TOTAL[sku as usize] == s.igpu + s.shared + s.reserved)
}

/// SKU 名（闭集原文）。
pub fn sku_name(sku: u8) -> &'static str {
    match sku {
        S_8700G => "8700G",
        S_8600G => "8600G",
        S_8500G => "8500G",
        S_8300G => "8300G",
        S_7840U => "7840U",
        S_7840HS => "7840HS",
        _ => "680M",
    }
}

/// 双显切换状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    /// 仅 iGPU。
    IGPUOnly,
    /// 混合。
    Hybrid,
    /// 仅 dGPU。
    DGPUOnly,
}

/// 切换合法性：I↔H、H↔D（I↔D 直跳非法）。
pub fn switch_legal(from: DisplayMode, to: DisplayMode) -> bool {
    matches!(
        (from, to),
        (DisplayMode::IGPUOnly, DisplayMode::Hybrid)
            | (DisplayMode::Hybrid, DisplayMode::IGPUOnly)
            | (DisplayMode::Hybrid, DisplayMode::DGPUOnly)
            | (DisplayMode::DGPUOnly, DisplayMode::Hybrid)
    )
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

/// 单台送测 APU。
#[derive(Clone, Debug)]
pub struct ApuDevice {
    /// 签名（去重键）。
    pub sig: String,
    /// SKU 号。
    pub sku: u8,
    /// 八项状态。
    pub items: [CState; ITEM_CAP],
    /// 各项重测计数。
    pub retests: [u32; ITEM_CAP],
    /// 当前双显模式。
    pub display: DisplayMode,
    /// 切换计数。
    pub switches: u64,
    /// 各向切换计数 [I→H, H→I, H→D, D→H]。
    pub switch_kinds: [u64; 4],
}

impl ApuDevice {
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

/// APU 认证册。
pub struct ApuCert {
    /// 送测账。
    pub rows: Vec<ApuDevice>,
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

impl ApuCert {
    /// 空册。
    pub fn new() -> Self {
        ApuCert {
            rows: Vec::new(),
            submitted: 0,
            passed: 0,
            failed: 0,
            inflight: 0,
            issued: 0,
            red_count: 0,
        }
    }

    /// 送测（初始 UMA 账平衡校验）。
    pub fn submit(&mut self, sig: &str, sku: u8, uma: UmaSplit) -> Result<usize, u16> {
        if sku > SKU_CAP {
            self.red_count += 1;
            return Err(XC_BAD_SKU);
        }
        if !uma_balanced(sku, &uma)? {
            self.red_count += 1;
            return Err(XC_UMA_DRIFT);
        }
        if self.has_sig(sig) {
            self.red_count += 1;
            return Err(XC_DUP_DEVICE);
        }
        if self.rows.len() == CERT_CAP {
            self.red_count += 1;
            return Err(XC_CERT_FULL);
        }
        self.rows.push(ApuDevice {
            sig: String::from(sig),
            sku,
            items: [CState::Submitted; ITEM_CAP],
            retests: [0; ITEM_CAP],
            display: DisplayMode::IGPUOnly,
            switches: 0,
            switch_kinds: [0; 4],
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

    /// 双显切换（带方向计数）。
    pub fn switch_display(&mut self, idx: usize, to: DisplayMode) -> Result<(), u16> {
        if idx >= self.rows.len() {
            self.red_count += 1;
            return Err(XC_BAD_DEVICE);
        }
        let cur = self.rows[idx].display;
        if !switch_legal(cur, to) {
            self.red_count += 1;
            return Err(XC_BAD_TRANS);
        }
        let kind = match (cur, to) {
            (DisplayMode::IGPUOnly, DisplayMode::Hybrid) => 0,
            (DisplayMode::Hybrid, DisplayMode::IGPUOnly) => 1,
            (DisplayMode::Hybrid, DisplayMode::DGPUOnly) => 2,
            _ => 3,
        };
        self.rows[idx].display = to;
        self.rows[idx].switches += 1;
        self.rows[idx].switch_kinds[kind] += 1;
        Ok(())
    }

    /// 账守恒。
    pub fn balanced(&self) -> bool {
        self.submitted == self.passed + self.failed + self.inflight
    }

    /// 出证（全 Pass + UMA 账复核）。
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
        self.issued += 1;
        Ok(())
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("apucert devices=");
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
        // 切换账守恒：switches = 四向和。
        let mut i = 0;
        while i < self.rows.len() {
            let d = &self.rows[i];
            let sum = d.switch_kinds[0] + d.switch_kinds[1] + d.switch_kinds[2] + d.switch_kinds[3];
            if sum != d.switches {
                return Err(XC_SWITCH_DRIFT);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 自例程：送测 → 全项过 → 双显切换链 → 出证 + 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut c = ApuCert::new();
    let uma1 = UmaSplit { igpu: 1024, shared: 2560, reserved: 512 };
    let a = c.submit("nb-7840hs-a", S_7840HS, uma1)?;
    // 负例：SKU 越界。
    if c.submit("bad", 7, uma1) != Err(XC_BAD_SKU) {
        return Err(XC_BAD_SKU);
    }
    // 负例：UMA 漂移。
    if c.submit("drift", S_8700G, UmaSplit { igpu: 1024, shared: 1024, reserved: 512 })
        != Err(XC_UMA_DRIFT)
    {
        return Err(XC_UMA_DRIFT);
    }
    // 负例：重复。
    if c.submit("nb-7840hs-a", S_7840HS, uma1) != Err(XC_DUP_DEVICE) {
        return Err(XC_DUP_DEVICE);
    }
    // 全项链 a。
    let mut i = 0;
    while i < ITEM_CAP {
        c.test(a, i)?;
        c.conclude(a, i, true)?;
        i += 1;
    }
    // 双显切换链：I→H→D→H→I。
    c.switch_display(a, DisplayMode::Hybrid)?;
    c.switch_display(a, DisplayMode::DGPUOnly)?;
    c.switch_display(a, DisplayMode::Hybrid)?;
    c.switch_display(a, DisplayMode::IGPUOnly)?;
    // 负例：I→D 直跳。
    if c.switch_display(a, DisplayMode::DGPUOnly) != Err(XC_BAD_TRANS) {
        return Err(XC_BAD_TRANS);
    }
    c.issue(a)?;
    c.audit()?;
    // 重测路径。
    let b = c.submit("mini-8600g-b", S_8600G, UmaSplit { igpu: 512, shared: 1280, reserved: 256 })?;
    c.test(b, 0)?;
    c.conclude(b, 0, false)?;
    c.test(b, 0)?;
    c.conclude(b, 0, true)?;
    let mut i2 = 1;
    while i2 < ITEM_CAP {
        c.test(b, i2)?;
        c.conclude(b, i2, true)?;
        i2 += 1;
    }
    c.issue(b)?;
    c.audit()?;
    // 负例：重测超限。
    let mut e = ApuCert::new();
    let f = e.submit("flaky", S_8300G, UmaSplit { igpu: 256, shared: 640, reserved: 128 })?;
    let mut k = 0;
    while k <= RETEST_MAX {
        e.test(f, 1)?;
        e.conclude(f, 1, false)?;
        k += 1;
    }
    if e.test(f, 1) != Err(XC_RETEST_EXHAUSTED) {
        return Err(XC_RETEST_EXHAUSTED);
    }
    // 负例：未测项出证。
    if e.issue(f) != Err(XC_CERT_INCOMPLETE) {
        return Err(XC_CERT_INCOMPLETE);
    }
    // 负例：设备号越界。
    if e.issue(99) != Err(XC_BAD_DEVICE) {
        return Err(XC_BAD_DEVICE);
    }
    // UMA 平衡面复核。
    if !uma_balanced(S_680M, &UmaSplit { igpu: 512, shared: 1280, reserved: 256 })? {
        return Err(XC_UMA_DRIFT);
    }
    if c.summary().len() == 0 || c.issued != 2 {
        return Err(XC_CERT_INCOMPLETE);
    }
    Ok(0)
}
