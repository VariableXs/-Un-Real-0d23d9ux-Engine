//! cgb836_uhd_cert — CGPU-F0836 UHD 基础档认证。
//!
//! 判据原文（验收行摘录）：UHD 基础档认证：下限特征门（EU 16~32、带宽
//! ≥ 12GB/s、解码位在册——三检）；能力封顶核验（不得声明高档专属特性：
//! 双精度/编解码高档位——声明即拒）；基本场景三关卡（内存分配/解码回
//! 路/遥测采样——全过才收口）；守恒账（关卡 = 过+挂+待）。判据：下限
//! 三检、封顶核验、三关卡、守恒。
//!
//! 诊断段 0x7300（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；定容关卡账；drill() 自例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb836-v1";

/// 诊断段基（本单独占 0x7300）。
pub const SEG_BASE: u16 = 0x7300;

/// EU 越上界（> 32——疑高档混检）。
pub const XC_EU_OVER: u16 = SEG_BASE + 1;
/// EU 不足（< 16）。
pub const XC_EU_UNDER: u16 = SEG_BASE + 2;
/// 带宽不足（< 12GB/s）。
pub const XC_BW_SHORT: u16 = SEG_BASE + 3;
/// 解码位缺失。
pub const XC_NO_DECODE: u16 = SEG_BASE + 4;
/// 高档特性越权声明（双精度/高档编码——封顶违约）。
pub const XC_TIER_CAP_VIOLATION: u16 = SEG_BASE + 5;
/// 守恒破坏。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 6;
/// 重复收敛。
pub const XC_ALREADY_DONE: u16 = SEG_BASE + 7;

/// UHD 特征区间。
pub const EU_LO: u32 = 16;
/// 特征上界。
pub const EU_HI: u32 = 32;
/// 带宽下限。
pub const BW_FLOOR_GBS: u32 = 12;
/// 解码位（bit0）。
pub const DECODE_BIT: u32 = 0b0001;

/// 送检特征（判据"能力封顶核验"的声明域）。
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// EU 数。
    pub eus: u32,
    /// 带宽（GB/s）。
    pub bandwidth_gbs: u32,
    /// 媒体位图。
    pub media_bits: u32,
    /// 双精度管道声明。
    pub dp_pipe: bool,
    /// 档内编码位声明（bit2）。
    pub enc_claim: bool,
}

/// 下限特征门（判据"下限三检"）。
pub fn spec_gate(s: &Spec) -> Result<(), u16> {
    if s.eus > EU_HI {
        return Err(XC_EU_OVER);
    }
    if s.eus < EU_LO {
        return Err(XC_EU_UNDER);
    }
    if s.bandwidth_gbs < BW_FLOOR_GBS {
        return Err(XC_BW_SHORT);
    }
    if s.media_bits & DECODE_BIT == 0 {
        return Err(XC_NO_DECODE);
    }
    Ok(())
}

/// 能力封顶核验（判据"不得声明高档专属特性——声明即拒"）。
/// UHD 档禁声明：双精度管道、高档编码位。
pub fn cap_check(s: &Spec) -> Result<(), u16> {
    if s.dp_pipe {
        return Err(XC_TIER_CAP_VIOLATION);
    }
    if s.enc_claim {
        return Err(XC_TIER_CAP_VIOLATION);
    }
    // 媒体位图不得含高档专属位（bit2 编码、bit3 去隔行声明越档）。
    if s.media_bits & 0b1100 != 0 {
        return Err(XC_TIER_CAP_VIOLATION);
    }
    Ok(())
}

/// 三关卡闭集（判据"基本场景三关卡"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    /// 内存分配。
    MemAlloc,
    /// 解码回路。
    DecodeLoop,
    /// 遥测采样。
    Telemetry,
}

impl Check {
    /// 序号。
    pub fn idx(self) -> usize {
        match self {
            Check::MemAlloc => 0,
            Check::DecodeLoop => 1,
            Check::Telemetry => 2,
        }
    }
    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            Check::MemAlloc => "mem_alloc",
            Check::DecodeLoop => "decode_loop",
            Check::Telemetry => "telemetry",
        }
    }
}

/// 关卡状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum St {
    /// 待跑。
    Todo,
    /// 过。
    Pass,
    /// 挂。
    Fail,
}

/// UHD 认证账。
pub struct UhdCert {
    /// 账名。
    pub name: String,
    /// 特征（门过后冻结）。
    pub spec: Spec,
    /// 三关卡状态。
    pub checks: [St; 3],
    /// 判定位。
    done: bool,
    /// 结果。
    pub passed: bool,
}

/// 守恒恒等式：三关卡状态数恒为 3，且 Σ(过+挂+待) == 3；判定后冻结。
impl UhdCert {
    /// 构造（双门前置：下限门 + 封顶核验）。
    pub fn new(name: &str, s: Spec) -> Result<Self, u16> {
        spec_gate(&s)?;
        cap_check(&s)?;
        Ok(UhdCert { name: name.to_string(), spec: s, checks: [St::Todo; 3], done: false, passed: false })
    }

    /// 关卡判定。
    pub fn judge(&mut self, c: Check, st: St) -> Result<(), u16> {
        if self.done {
            return Err(XC_ALREADY_DONE);
        }
        if st == St::Todo {
            return Err(XC_LEDGER_DRIFT);
        }
        if self.checks[c.idx()] != St::Todo {
            return Err(XC_LEDGER_DRIFT);
        }
        self.checks[c.idx()] = st;
        Ok(())
    }

    /// 三关卡通过计数。
    pub fn pass_count(&self) -> u32 {
        let mut n = 0;
        for s in self.checks.iter() {
            if *s == St::Pass {
                n += 1;
            }
        }
        n
    }

    /// 挂掉计数。
    pub fn fail_count(&self) -> u32 {
        let mut n = 0;
        for s in self.checks.iter() {
            if *s == St::Fail {
                n += 1;
            }
        }
        n
    }

    /// 收敛（判据"全过才收口"）。
    pub fn finalize(&mut self) -> Result<bool, u16> {
        if self.done {
            return Err(XC_ALREADY_DONE);
        }
        for s in self.checks.iter() {
            if *s == St::Todo {
                return Err(XC_LEDGER_DRIFT);
            }
        }
        self.done = true;
        self.passed = self.fail_count() == 0;
        Ok(self.passed)
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        if self.pass_count() + self.fail_count() != 3 {
            return Err(XC_LEDGER_DRIFT);
        }
        if !self.done {
            if self.pass_count() + self.fail_count() == 3 {
                // 未判定但全终态——允许（待 finalize），非漂移。
            }
        } else if !self.passed && self.fail_count() == 0 {
            return Err(XC_LEDGER_DRIFT);
        } else if self.passed && self.fail_count() != 0 {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb836[");
        s.push_str(&self.name);
        s.push_str("] pass=");
        s.push_str(&self.pass_count().to_string());
        s.push_str(" fail=");
        s.push_str(&self.fail_count().to_string());
        s.push_str(" done=");
        s.push_str(if self.done { "1" } else { "0" });
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    // 负样本 1：下限三检逐项挂。
    let over = Spec { eus: 64, bandwidth_gbs: 16, media_bits: DECODE_BIT, dp_pipe: false, enc_claim: false };
    match UhdCert::new("x", over) {
        Err(code) if code == XC_EU_OVER => {}
        Ok(_) => return Err(XC_EU_OVER),
        Err(other) => return Err(other),
    }
    let under = Spec { eus: 8, bandwidth_gbs: 16, media_bits: DECODE_BIT, dp_pipe: false, enc_claim: false };
    match UhdCert::new("x", under) {
        Err(code) if code == XC_EU_UNDER => {}
        Ok(_) => return Err(XC_EU_UNDER),
        Err(other) => return Err(other),
    }
    let bw = Spec { eus: 24, bandwidth_gbs: 8, media_bits: DECODE_BIT, dp_pipe: false, enc_claim: false };
    match UhdCert::new("x", bw) {
        Err(code) if code == XC_BW_SHORT => {}
        Ok(_) => return Err(XC_BW_SHORT),
        Err(other) => return Err(other),
    }
    let nodec = Spec { eus: 24, bandwidth_gbs: 16, media_bits: 0, dp_pipe: false, enc_claim: false };
    match UhdCert::new("x", nodec) {
        Err(code) if code == XC_NO_DECODE => {}
        Ok(_) => return Err(XC_NO_DECODE),
        Err(other) => return Err(other),
    }

    // 负样本 2：封顶违约——双精度声明。
    let dp = Spec { eus: 24, bandwidth_gbs: 16, media_bits: DECODE_BIT, dp_pipe: true, enc_claim: false };
    match UhdCert::new("x", dp) {
        Err(code) if code == XC_TIER_CAP_VIOLATION => {}
        Ok(_) => return Err(XC_TIER_CAP_VIOLATION),
        Err(other) => return Err(other),
    }
    // 负样本 3：封顶违约——高档编码声明。
    let enc = Spec { eus: 24, bandwidth_gbs: 16, media_bits: DECODE_BIT, dp_pipe: false, enc_claim: true };
    match UhdCert::new("x", enc) {
        Err(code) if code == XC_TIER_CAP_VIOLATION => {}
        Ok(_) => return Err(XC_TIER_CAP_VIOLATION),
        Err(other) => return Err(other),
    }
    // 负样本 4：封顶违约——媒体位含高档专属位。
    let hi_bits = Spec { eus: 24, bandwidth_gbs: 16, media_bits: 0b1101, dp_pipe: false, enc_claim: false };
    match UhdCert::new("x", hi_bits) {
        Err(code) if code == XC_TIER_CAP_VIOLATION => {}
        Ok(_) => return Err(XC_TIER_CAP_VIOLATION),
        Err(other) => return Err(other),
    }

    // 正样本：合规三关卡全过。
    let ok = Spec { eus: 24, bandwidth_gbs: 16, media_bits: DECODE_BIT, dp_pipe: false, enc_claim: false };
    let mut c = UhdCert::new("w3-drill-836", ok)?;
    // 收敛前缺关卡——应拒。
    match c.finalize() {
        Err(code) if code == XC_LEDGER_DRIFT => {}
        Ok(_) => return Err(XC_LEDGER_DRIFT),
        Err(other) => return Err(other),
    }
    c.judge(Check::MemAlloc, St::Pass)?;
    c.judge(Check::DecodeLoop, St::Pass)?;
    c.judge(Check::Telemetry, St::Pass)?;
    if c.pass_count() != 3 {
        return Err(XC_LEDGER_DRIFT);
    }
    if !c.finalize()? {
        return Err(XC_LEDGER_DRIFT);
    }
    // 负样本 5：判定后改判。
    match c.judge(Check::MemAlloc, St::Fail) {
        Err(code) if code == XC_ALREADY_DONE => {}
        Ok(_) => return Err(XC_ALREADY_DONE),
        Err(other) => return Err(other),
    }
    match c.finalize() {
        Err(code) if code == XC_ALREADY_DONE => {}
        Ok(_) => return Err(XC_ALREADY_DONE),
        Err(other) => return Err(other),
    }

    // 挂掉路径：解码回路挂 → 收敛拒绝。
    let mut c2 = UhdCert::new("w3-drill-836-fail", ok)?;
    c2.judge(Check::MemAlloc, St::Pass)?;
    c2.judge(Check::DecodeLoop, St::Fail)?;
    c2.judge(Check::Telemetry, St::Pass)?;
    if !c2.finalize()? {
        // 预期拒绝。
    } else {
        return Err(XC_LEDGER_DRIFT);
    }
    if c2.fail_count() != 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    c.verify()?;
    c2.verify()?;
    Ok(0)
}
