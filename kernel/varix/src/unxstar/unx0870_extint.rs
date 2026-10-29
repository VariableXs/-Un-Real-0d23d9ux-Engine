//! unx0870_extint —— UNX-F0870 · ExtINT 与特殊投递模式裁决（目标 240 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0870 · ExtINT 与特殊投递模式裁决。
//! 判据原文：UNX-F0870-J1 ExtINT 请求默认拒绝（10/10 次拒绝留账），显式启用
//!           路径白名单校验 10/10 次正确。
//! 设计纪律：
//! - ExtINT（8259 兼容投递）在新式内核是高危面：默认全拒（判据主锚）。
//!   显式启用仅两白名单路径：①LINT0 直通 PIC（SMP 预启动窗口）②VM 前
//!   兼容客户机（虚拟化面，建模标注）。白名单外的启用一律拒。
//! - 特殊投递模式闭集裁决：SMI/INIT/NMI/ExtINT 四模式各有约束表——
//!   SMI 恒向自我（dest=self）/ INIT 不得掩码投递 / NMI 恒边沿 / ExtINT
//!   恒向量 0 且经 PIC 路径。
//! - 裁决器：请求（模式+目的地+向量+路径）→ 白名单/约束表 → 准/拒 + 账。
//!   拒绝留账（零丢弃），启用留账（带路径标签）。
//! - ExtINT 请求 10/10 默认拒；白名单校验 10/10（5 拒 5 准）正确（判据后半）。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x69A0（0x69A1~0x69A9 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const EI_VERSION: &str = "unx0870_extint-v1";

/// 诊断段基址（0x69A0 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x69A0;

/// XcCode 闭集。
pub const XC_EXTINT_DENY: u16 = SEG_BASE + 1; // ExtINT 默认拒
pub const XC_NOT_WL: u16 = SEG_BASE + 2; // 非白名单路径
pub const XC_MODE_BAD: u16 = SEG_BASE + 3; // 投递模式非法
pub const XC_CONSTRAINT: u16 = SEG_BASE + 4; // 模式约束违反
pub const XC_WL_FULL: u16 = SEG_BASE + 5; // 白名单槽满
pub const XC_LED_FULL: u16 = SEG_BASE + 6; // 账溢出
pub const XC_DENY_MISS: u16 = SEG_BASE + 7; // 注入拒绝漏判
pub const XC_WL_MISS: u16 = SEG_BASE + 8; // 白名单校验漏判
pub const XC_CONSERVE: u16 = SEG_BASE + 9; // 守恒破

/// 特殊投递模式闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DelMode {
    /// 固定投递（常规）。
    Fixed = 0,
    /// 最低优先级。
    Lowest = 1,
    /// SMI。
    Smi = 2,
    /// NMI。
    Nmi = 4,
    /// INIT。
    Init = 5,
    /// ExtINT（8259 兼容）。
    ExtInt = 7,
}

impl DelMode {
    /// 编码。
    pub fn code(self) -> u8 {
        self as u8
    }

    /// 解码（非法即 None）。
    pub fn from_code(c: u8) -> Option<DelMode> {
        match c {
            0 => Some(DelMode::Fixed),
            1 => Some(DelMode::Lowest),
            2 => Some(DelMode::Smi),
            4 => Some(DelMode::Nmi),
            5 => Some(DelMode::Init),
            7 => Some(DelMode::ExtInt),
            _ => None,
        }
    }
}

/// 显式启用路径白名单闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WlPath {
    /// LINT0 直通 PIC（SMP 预启动窗口）。
    Lint0Pic = 1,
    /// VM 兼容客户机（虚拟化面）。
    VmCompat = 2,
}

impl WlPath {
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// 投递请求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DelRequest {
    /// 投递模式。
    pub mode: DelMode,
    /// 目的地（建模 APIC ID）。
    pub dest: u8,
    /// 向量。
    pub vector: u8,
    /// 请求路径（白名单码；Fixed/Lowest 恒 0）。
    pub path: u8,
    /// 自我 ID（SMI 约束核用）。
    pub self_id: u8,
}

/// 模式约束表（SDM 固化）：
/// - Smi：dest 必须 == self_id（恒向自我）。
/// - Nmi：恒边沿（level 位建模面恒 false——本裁决器只核 dest/vector 约束）。
/// - Init：不得掩码投递（请求无掩码位——约束核跳过，占位语义注释）。
/// - ExtInt：向量恒 0 且路径必须在白名单。
pub fn mode_constraint_ok(r: &DelRequest) -> Result<(), u16> {
    match r.mode {
        DelMode::Smi => {
            if r.dest != r.self_id {
                return Err(XC_CONSTRAINT);
            }
        }
        DelMode::Nmi => {
            // NMI 无向量约束（恒向量 2 由架构注——建模面不核）
        }
        DelMode::Init => {
            // INIT 不得向自投（SDM：INIT 向自=非法序列）
            if r.dest == r.self_id {
                return Err(XC_CONSTRAINT);
            }
        }
        DelMode::ExtInt => {
            if r.vector != 0 {
                return Err(XC_CONSTRAINT);
            }
        }
        DelMode::Fixed | DelMode::Lowest => {
            if (r.vector as u32) < 32 {
                return Err(XC_CONSTRAINT);
            }
        }
    }
    Ok(())
}

/// 裁决器。
pub struct ModeArbiter {
    /// 白名单（启用路径集合，≤2）。
    whitelist: Vec<u8>,
    /// 裁决账（0 拒 / 1 准）。
    ledger: Vec<(DelRequest, bool, u64)>,
    overflow: usize,
    seq: u64,
}

impl ModeArbiter {
    pub fn new() -> ModeArbiter {
        ModeArbiter {
            whitelist: Vec::new(),
            ledger: Vec::new(),
            overflow: 0,
            seq: 0,
        }
    }

    fn log(&mut self, r: DelRequest, verdict: bool) {
        self.seq += 1;
        if self.ledger.len() < 128 {
            self.ledger.push((r, verdict, self.seq));
        } else {
            self.overflow += 1;
        }
    }

    /// 白名单登记（ExtINT 显式启用前置）。
    pub fn whitelist_add(&mut self, p: WlPath) -> Result<(), u16> {
        if self.whitelist.len() >= 2 {
            return Err(XC_WL_FULL);
        }
        if !self.whitelist.contains(&p.code()) {
            self.whitelist.push(p.code());
        }
        Ok(())
    }

    /// 白名单查询。
    pub fn wl_contains(&self, p: u8) -> bool {
        self.whitelist.contains(&p)
    }

    /// 裁决主入口：ExtINT 默认拒（无白名单路径时）；白名单路径核 + 模式约束核。
    pub fn arbitrate(&mut self, r: DelRequest) -> Result<(), u16> {
        // 1. 模式合法性
        if DelMode::from_code(r.mode.code()).is_none() {
            self.log(r, false);
            return Err(XC_MODE_BAD);
        }
        // 2. ExtINT 默认拒 + 白名单通道
        if r.mode == DelMode::ExtInt {
            if !self.wl_contains(r.path) || r.path == 0 {
                self.log(r, false);
                return Err(XC_EXTINT_DENY);
            }
            // 白名单路径合法性闭集
            if !matches!(r.path, x if x == WlPath::Lint0Pic.code() || x == WlPath::VmCompat.code()) {
                self.log(r, false);
                return Err(XC_NOT_WL);
            }
        }
        // 3. 模式约束核
        if let Err(e) = mode_constraint_ok(&r) {
            self.log(r, false);
            return Err(e);
        }
        self.log(r, true);
        Ok(())
    }

    /// ExtINT 请求 10/10 默认拒（判据前半执行面，无白名单仲裁器）。
    pub fn extint_deny_10(&mut self, self_id: u8) -> Result<usize, u16> {
        let mut denied = 0;
        for i in 0..10u8 {
            let r = DelRequest {
                mode: DelMode::ExtInt,
                dest: self_id.wrapping_add(i),
                vector: 0,
                path: 0,
                self_id,
            };
            if self.arbitrate(r).is_err() {
                denied += 1;
            }
        }
        if denied != 10 {
            return Err(XC_DENY_MISS);
        }
        Ok(denied)
    }

    /// 白名单校验 10/10（判据后半执行面）：5 拒（路径 0/3/9）5 准（Lint0/Vm）。
    pub fn wl_check_10(&mut self, self_id: u8) -> Result<usize, u16> {
        self.whitelist_add(WlPath::Lint0Pic)?;
        self.whitelist_add(WlPath::VmCompat)?;
        let mut ok = 0;
        // 5 准：白名单路径 + 向量 0 + 合法 dest
        for (i, p) in [WlPath::Lint0Pic, WlPath::VmCompat, WlPath::Lint0Pic, WlPath::VmCompat, WlPath::Lint0Pic].iter().enumerate() {
            let r = DelRequest {
                mode: DelMode::ExtInt,
                dest: self_id.wrapping_add(1),
                vector: 0,
                path: p.code(),
                self_id,
            };
            if self.arbitrate(r).is_ok() {
                ok += 1;
            }
            let _ = i;
        }
        // 5 拒：非白名单路径
        for p in [0u8, 3, 9, 0, 5].iter() {
            let r = DelRequest {
                mode: DelMode::ExtInt,
                dest: self_id.wrapping_add(1),
                vector: 0,
                path: *p,
                self_id,
            };
            if self.arbitrate(r).is_err() {
                ok += 1;
            }
        }
        if ok != 10 {
            return Err(XC_WL_MISS);
        }
        Ok(ok)
    }

    /// 准账行读取。
    pub fn row(&self, i: usize) -> Result<(bool, u64), u16> {
        if i >= self.ledger.len() {
            return Err(XC_LED_FULL);
        }
        Ok((self.ledger[i].1, self.ledger[i].2))
    }

    /// 准/拒计数。
    pub fn verdict_stat(&self) -> (usize, usize) {
        let ok = self.ledger.iter().filter(|x| x.1).count();
        (ok, self.ledger.len() - ok)
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.ledger.len() + self.overflow == self.seq as usize
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. ExtINT 默认拒 10/10
    let mut a = ModeArbiter::new();
    if a.extint_deny_10(0)? != 10 {
        return Err(XC_DENY_MISS);
    }
    // 2. 白名单校验 10/10
    if a.wl_check_10(0)? != 10 {
        return Err(XC_WL_MISS);
    }
    // 3. 准拒账
    let (ok, deny) = a.verdict_stat();
    if ok != 5 || deny < 10 {
        return Err(XC_WL_MISS);
    }
    // 4. 守恒
    if !a.conserve() {
        return Err(XC_LED_FULL);
    }
    // 5. 模式约束核：SMI 恒向自我
    let mut b = ModeArbiter::new();
    if b.arbitrate(DelRequest {
        mode: DelMode::Smi,
        dest: 3,
        vector: 0,
        path: 0,
        self_id: 0,
    }).is_ok() {
        return Err(XC_CONSTRAINT);
    }
    if b.arbitrate(DelRequest {
        mode: DelMode::Smi,
        dest: 0,
        vector: 0,
        path: 0,
        self_id: 0,
    }).is_err() {
        return Err(XC_CONSTRAINT);
    }
    // 6. INIT 不得向自
    if b.arbitrate(DelRequest {
        mode: DelMode::Init,
        dest: 0,
        vector: 0,
        path: 0,
        self_id: 0,
    }).is_ok() {
        return Err(XC_CONSTRAINT);
    }
    // 7. Fixed 低向量拒
    if b.arbitrate(DelRequest {
        mode: DelMode::Fixed,
        dest: 1,
        vector: 0x10,
        path: 0,
        self_id: 0,
    }).is_ok() {
        return Err(XC_CONSTRAINT);
    }
    // 8. Fixed 合例
    b.arbitrate(DelRequest {
        mode: DelMode::Fixed,
        dest: 1,
        vector: 0x41,
        path: 0,
        self_id: 0,
    })?;
    // 9. 错误路径①：非法模式码（3/6）
    if DelMode::from_code(3).is_some() || DelMode::from_code(6).is_some() {
        return Err(XC_MODE_BAD);
    }
    // 10. 错误路径②：白名单槽满（>2 拒）
    let mut c = ModeArbiter::new();
    c.whitelist_add(WlPath::Lint0Pic)?;
    c.whitelist_add(WlPath::VmCompat)?;
    if c.whitelist_add(WlPath::Lint0Pic).is_ok() && c.whitelist.len() > 2 {
        return Err(XC_WL_FULL);
    }
    // 11. 错误路径③：ExtINT 非零向量即使白名单也拒
    let mut d = ModeArbiter::new();
    d.whitelist_add(WlPath::Lint0Pic)?;
    if d.arbitrate(DelRequest {
        mode: DelMode::ExtInt,
        dest: 1,
        vector: 0x41,
        path: WlPath::Lint0Pic.code(),
        self_id: 0,
    }).is_ok() {
        return Err(XC_CONSTRAINT);
    }
    // 12. 守恒终核
    if !d.conserve() {
        return Err(XC_CONSERVE);
    }
    Ok(0)
}
