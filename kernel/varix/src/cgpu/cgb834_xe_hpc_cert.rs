//! cgb834_xe_hpc_cert — CGPU-F0834 Xe 高性能档认证。
//!
//! 判据原文（验收行摘录）：Xe 高性能档认证：硬指标门（EU 数 ≥ 384、带
//! 宽 ≥ 256GB/s、双精度管道在册——三项不过即拒）；压力剖面账（连续负载
//! 帧账：满载频率保持率、降频窗口占比、温度墙触发数）；长稳门槛（最小
//! 压力帧数不足即不予认证）；认证判定账（指标项 = 过 + 挂，帧账 = 有效
//! + 无效）。判据：三项硬门、压力剖面、长稳门槛、守恒判定账。
//!
//! 诊断段 0x72C0（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；指标全整数；
//! 失败路径 Result<_, u16> 闭集错误码；定容帧账环形覆盖；drill() 自例程
//! 返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb834-v1";

/// 诊断段基（本单独占 0x72C0）。
pub const SEG_BASE: u16 = 0x72C0;

/// EU 数不足（硬门 1 挂）。
pub const XC_EU_SHORT: u16 = SEG_BASE + 1;
/// 带宽不足（硬门 2 挂）。
pub const XC_BW_SHORT: u16 = SEG_BASE + 2;
/// 双精度管道缺失（硬门 3 挂）。
pub const XC_NO_DP_PIPE: u16 = SEG_BASE + 3;
/// 长稳不足（压力帧数低于门槛）。
pub const XC_SOAK_SHORT: u16 = SEG_BASE + 4;
/// 帧读数非法（频率/温度越界——传感器违约）。
pub const XC_TELEM_INVALID: u16 = SEG_BASE + 5;
/// 判定账漂移（守恒破坏）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 6;
/// 重复认证（已判定后重复收敛）。
pub const XC_ALREADY_CERT: u16 = SEG_BASE + 7;

/// 硬门阈值（判据"三项硬门"）。
pub const EU_FLOOR: u32 = 384;
/// 硬门阈值。
pub const BW_FLOOR_GBS: u32 = 256;
/// 压力帧门槛（判据"长稳门槛"）。
pub const SOAK_MIN_FRAMES: usize = 12;
/// 帧账定容。
pub const FRAME_CAP: usize = 16;
/// 频率标称值（MHz，保持率分母）。
pub const NOMINAL_MHZ: u64 = 2400;
/// 温度墙（摄氏度）。
pub const TEMP_WALL_C: u64 = 95;
/// 频率保持率门槛（千分比：≥ 900 表示 ≥90%）。
pub const HOLD_MILLI: u64 = 900;
/// 降频窗口占比上限（千分比：≤ 50 即 ≤5%）。
pub const THROTTLE_MAX_MILLI: u64 = 50;

/// 压力帧读数。
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// 帧序。
    pub n: u64,
    /// 满载频率（MHz）。
    pub mhz: u64,
    /// 封装温度（摄氏度）。
    pub temp_c: u64,
    /// 本帧是否触发温度墙降频。
    pub wall: bool,
    /// 读数是否有效（无效帧入账剔除）。
    pub valid: bool,
}

/// 硬指标送检项。
#[derive(Clone, Copy, Debug)]
pub struct HardSpec {
    /// EU 数。
    pub eus: u32,
    /// 带宽（GB/s）。
    pub bandwidth_gbs: u32,
    /// 双精度管道在册。
    pub dp_pipe: bool,
}

/// 三项硬门判定（判据"三项不过即拒"）。
pub fn hard_gate(s: &HardSpec) -> Result<(), u16> {
    if s.eus < EU_FLOOR {
        return Err(XC_EU_SHORT);
    }
    if s.bandwidth_gbs < BW_FLOOR_GBS {
        return Err(XC_BW_SHORT);
    }
    if !s.dp_pipe {
        return Err(XC_NO_DP_PIPE);
    }
    Ok(())
}

/// 认证判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 待定：硬门过、压力账未齐。
    Pending,
    /// 认证通过。
    Certified,
    /// 认证拒绝。
    Rejected,
}

impl Verdict {
    /// 判定名。
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Pending => "pending",
            Verdict::Certified => "certified",
            Verdict::Rejected => "rejected",
        }
    }
}

/// Xe 高性能档认证器。
pub struct XeHpcCert {
    /// 认证器名。
    pub name: String,
    /// 硬门结果。
    pub hard_ok: bool,
    /// 帧账（定容环形）。
    ring: Vec<Frame>,
    /// 写指针。
    wpos: usize,
    /// 有效帧数。
    valid_n: usize,
    /// 无效帧数（守恒项）。
    pub invalid_n: u64,
    /// 温度墙触发总数（环形覆盖同步递减）。
    pub wall_cnt: u64,
    /// 判定。
    verdict: Verdict,
    /// 拒绝计数。
    pub reject: u64,
}

/// 守恒恒等式：valid_n + invalid_n == 总入账帧数；且 valid_n ≤ FRAME_CAP。
/// 环形覆盖丢弃旧帧时同步递减 wall_cnt，每步 push 后 verify() 复核。
impl XeHpcCert {
    /// 构造（Pending 态）。
    pub fn new(name: &str, s: &HardSpec) -> Result<Self, u16> {
        hard_gate(s)?;
        Ok(XeHpcCert {
            name: name.to_string(),
            hard_ok: true,
            ring: Vec::new(),
            wpos: 0,
            valid_n: 0,
            invalid_n: 0,
            wall_cnt: 0,
            verdict: Verdict::Pending,
            reject: 0,
        })
    }

    /// 判定只读。
    pub fn verdict(&self) -> Verdict {
        self.verdict
    }

    /// 有效帧数。
    pub fn frames(&self) -> usize {
        self.valid_n
    }

    /// 压力帧入账（判据"压力剖面账"）。
    pub fn push(&mut self, f: Frame) -> Result<(), u16> {
        if self.verdict != Verdict::Pending {
            self.reject += 1;
            return Err(XC_ALREADY_CERT);
        }
        if f.mhz == 0 || f.mhz > NOMINAL_MHZ || f.temp_c > 120 {
            self.invalid_n += 1;
            // 无效帧不入环形账（丢弃式入账，仅计数）。
            self.verify()?;
            return Err(XC_TELEM_INVALID);
        }
        if self.ring.len() < FRAME_CAP {
            self.ring.push(f);
            self.valid_n += 1;
        } else {
            // 满则覆盖最旧（环形），有效数守恒。
            let old = self.ring[self.wpos];
            if old.valid && old.wall {
                self.wall_cnt -= 1;
            }
            self.ring[self.wpos] = f;
            self.wpos = (self.wpos + 1) % FRAME_CAP;
        }
        if f.wall {
            self.wall_cnt += 1;
        }
        self.verify()?;
        Ok(())
    }

    /// 按时序读取有效帧。
    pub fn ordered(&self) -> Vec<Frame> {
        let mut out = Vec::new();
        if self.ring.is_empty() {
            return out;
        }
        let start = if self.ring.len() < FRAME_CAP { 0 } else { self.wpos };
        for i in 0..self.ring.len() {
            out.push(self.ring[(start + i) % FRAME_CAP]);
        }
        out
    }

    /// 频率保持率（千分比：平均频率 / 标称 × 1000）。
    pub fn hold_rate(&self) -> Result<u64, u16> {
        if self.valid_n < SOAK_MIN_FRAMES {
            return Err(XC_SOAK_SHORT);
        }
        let mut sum = 0u64;
        for f in self.ordered() {
            if f.valid {
                sum += f.mhz;
            }
        }
        Ok(sum * 1000 / (self.valid_n as u64 * NOMINAL_MHZ))
    }

    /// 降频窗口占比（千分比）。
    pub fn throttle_rate(&self) -> Result<u64, u16> {
        if self.valid_n < SOAK_MIN_FRAMES {
            return Err(XC_SOAK_SHORT);
        }
        Ok(self.wall_cnt * 1000 / self.valid_n as u64)
    }

    /// 收敛判定（判据"认证判定账"）。
    pub fn finalize(&mut self) -> Result<Verdict, u16> {
        if self.verdict != Verdict::Pending {
            return Err(XC_ALREADY_CERT);
        }
        if self.valid_n < SOAK_MIN_FRAMES {
            return Err(XC_SOAK_SHORT);
        }
        let hold = self.hold_rate()?;
        let thr = self.throttle_rate()?;
        if hold >= HOLD_MILLI && thr <= THROTTLE_MAX_MILLI {
            self.verdict = Verdict::Certified;
        } else {
            self.verdict = Verdict::Rejected;
        }
        Ok(self.verdict)
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        if self.valid_n > FRAME_CAP {
            return Err(XC_LEDGER_DRIFT);
        }
        let mut walls = 0u64;
        for f in self.ring.iter() {
            if f.valid && f.wall {
                walls += 1;
            }
        }
        if walls != self.wall_cnt {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb834[");
        s.push_str(&self.name);
        s.push_str("] verdict=");
        s.push_str(self.verdict.name());
        s.push_str(" valid=");
        s.push_str(&self.valid_n.to_string());
        s.push_str(" invalid=");
        s.push_str(&self.invalid_n.to_string());
        s.push_str(" walls=");
        s.push_str(&self.wall_cnt.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    // 负样本 1：三项硬门逐项挂。
    let s1 = HardSpec { eus: 128, bandwidth_gbs: 300, dp_pipe: true };
    match XeHpcCert::new("x1", &s1) {
        Err(code) if code == XC_EU_SHORT => {}
        Ok(_) => return Err(XC_EU_SHORT),
        Err(other) => return Err(other),
    }
    let s2 = HardSpec { eus: 512, bandwidth_gbs: 128, dp_pipe: true };
    match XeHpcCert::new("x2", &s2) {
        Err(code) if code == XC_BW_SHORT => {}
        Ok(_) => return Err(XC_BW_SHORT),
        Err(other) => return Err(other),
    }
    let s3 = HardSpec { eus: 512, bandwidth_gbs: 512, dp_pipe: false };
    match XeHpcCert::new("x3", &s3) {
        Err(code) if code == XC_NO_DP_PIPE => {}
        Ok(_) => return Err(XC_NO_DP_PIPE),
        Err(other) => return Err(other),
    }

    // 正样本：硬门全过 + 压力账（满频低温不降频——认证通过路径）。
    let s = HardSpec { eus: 512, bandwidth_gbs: 512, dp_pipe: true };
    let mut c = XeHpcCert::new("w3-drill-834", &s)?;
    // 负样本 2：长稳不足即收敛。
    match c.finalize() {
        Err(code) if code == XC_SOAK_SHORT => {}
        Ok(_) => return Err(XC_SOAK_SHORT),
        Err(other) => return Err(other),
    }
    // 负样本 3：无效读数（频率为 0）。
    for i in 0..3u64 {
        match c.push(Frame { n: i, mhz: 0, temp_c: 60, wall: false, valid: true }) {
            Err(code) if code == XC_TELEM_INVALID => {}
            Ok(_) => return Err(XC_TELEM_INVALID),
            Err(other) => return Err(other),
        }
    }
    if c.invalid_n != 3 {
        return Err(XC_LEDGER_DRIFT);
    }
    // 正样本：12 帧满载。
    for i in 0..12u64 {
        let mhz = NOMINAL_MHZ - (i % 3) * 40;
        c.push(Frame { n: 100 + i, mhz, temp_c: 78, wall: false, valid: true })?;
    }
    if c.frames() != 12 {
        return Err(XC_LEDGER_DRIFT);
    }
    let hold = c.hold_rate()?;
    if hold < HOLD_MILLI {
        return Err(XC_LEDGER_DRIFT);
    }
    let v = c.finalize()?;
    if v != Verdict::Certified {
        return Err(XC_LEDGER_DRIFT);
    }
    // 负样本 4：判定后重复入账/收敛。
    match c.push(Frame { n: 999, mhz: 2300, temp_c: 80, wall: false, valid: true }) {
        Err(code) if code == XC_ALREADY_CERT => {}
        Ok(_) => return Err(XC_ALREADY_CERT),
        Err(other) => return Err(other),
    }
    match c.finalize() {
        Err(code) if code == XC_ALREADY_CERT => {}
        Ok(_) => return Err(XC_ALREADY_CERT),
        Err(other) => return Err(other),
    }

    // 拒绝路径：温度墙频发 → 保持率不足。
    let mut c2 = XeHpcCert::new("w3-drill-834-hot", &s)?;
    for i in 0..12u64 {
        c2.push(Frame { n: i, mhz: 1600, temp_c: 100, wall: true, valid: true })?;
    }
    if c2.hold_rate()? >= HOLD_MILLI {
        return Err(XC_LEDGER_DRIFT);
    }
    let v2 = c2.finalize()?;
    if v2 != Verdict::Rejected {
        return Err(XC_LEDGER_DRIFT);
    }
    c2.verify()?;
    c.verify()?;
    if c.reject < 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
