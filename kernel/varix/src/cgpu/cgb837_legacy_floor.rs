//! cgb837_legacy_floor — CGPU-F0837 老旧 Intel 支持下限。
//!
//! 判据原文（验收行摘录）：老旧 Intel 支持下限：最低特征基线（EU ≥ 6、
//! 带宽 ≥ 4GB/s、单解码位——不足即断供）；降级策略表（超出基线的老旧
//! 设备按能力逐项降级：关媒体编码、关功耗画像、保遥测）；断供台账（低
//! 于基线设备入断供账、申报断供码）；下限守恒账（登记数 = 在服数 + 断
//! 供数）。判据：基线三检、降级表、断供台账、守恒。
//!
//! 诊断段 0x7320（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；定容设备账；drill() 自例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb837-v1";

/// 诊断段基（本单独占 0x7320）。
pub const SEG_BASE: u16 = 0x7320;

/// EU 不足（< 6——断供）。
pub const XC_EU_TOO_LOW: u16 = SEG_BASE + 1;
/// 带宽不足（< 4GB/s——断供）。
pub const XC_BW_TOO_LOW: u16 = SEG_BASE + 2;
/// 解码位缺失——断供。
pub const XC_NO_DECODE: u16 = SEG_BASE + 3;
/// 设备账满（定容耗尽）。
pub const XC_DEV_FULL: u16 = SEG_BASE + 4;
/// 设备重登。
pub const XC_DEV_DUP: u16 = SEG_BASE + 5;
/// 降级项未知（降级表外的能力项）。
pub const XC_DEGRADE_UNKNOWN: u16 = SEG_BASE + 6;
/// 台账守恒破坏（登记数 ≠ 在服+断供）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 7;

/// 最低基线（判据"最低特征基线"）。
pub const EU_FLOOR: u32 = 6;
/// 基线带宽。
pub const BW_FLOOR_GBS: u32 = 4;
/// 基线解码位。
pub const DECODE_BIT: u32 = 0b0001;
/// 设备账定容。
pub const DEV_CAP: usize = 24;

/// 降级能力项闭集（判据"降级策略表"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cap {
    /// 媒体编码（老旧默认关）。
    MediaEncode,
    /// 功耗画像（老旧默认关）。
    PowerProfile,
    /// 双精度（老旧禁用）。
    DoublePipe,
    /// 遥测（保底能力——永不降级）。
    Telemetry,
}

impl Cap {
    /// 项名。
    pub fn name(self) -> &'static str {
        match self {
            Cap::MediaEncode => "media_encode",
            Cap::PowerProfile => "power_profile",
            Cap::DoublePipe => "double_pipe",
            Cap::Telemetry => "telemetry",
        }
    }
    /// 保底项判定（判据"保遥测"——保底项降级即表错）。
    pub fn is_floor(self) -> bool {
        self == Cap::Telemetry
    }
}

/// 送检特征。
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    /// EU 数。
    pub eus: u32,
    /// 带宽（GB/s）。
    pub bandwidth_gbs: u32,
    /// 媒体位图。
    pub media_bits: u32,
}

/// 基线三检（判据"不足即断供"——返回断供码）。
pub fn baseline_check(s: &Spec) -> Result<(), u16> {
    if s.eus < EU_FLOOR {
        return Err(XC_EU_TOO_LOW);
    }
    if s.bandwidth_gbs < BW_FLOOR_GBS {
        return Err(XC_BW_TOO_LOW);
    }
    if s.media_bits & DECODE_BIT == 0 {
        return Err(XC_NO_DECODE);
    }
    Ok(())
}

/// 设备条目。
#[derive(Clone, Debug)]
pub struct Dev {
    /// PCI ID。
    pub dev_id: u16,
    /// 特征。
    pub spec: Spec,
    /// 在服否（false = 已断供）。
    pub serving: bool,
    /// 生效降级项。
    pub degraded: Vec<Cap>,
}

/// 老旧支持下限账。
pub struct LegacyBook {
    /// 账名。
    pub name: String,
    /// 设备账（定容）。
    pub devs: Vec<Dev>,
    /// 断供台账（断供码 → 设备 ID，与 devs 中 serving=false 对位）。
    pub cut_list: Vec<(u16, u16)>,
}

/// 守恒恒等式：devs.len() == serving 计数 + cut_list.len()（断供数）。
/// 每次登记/断供后 verify() 复核。
impl LegacyBook {
    /// 构造。
    pub fn new(name: &str) -> Self {
        LegacyBook { name: name.to_string(), devs: Vec::new(), cut_list: Vec::new() }
    }

    /// 登记老旧设备（判据：基线不足 → 断供入台账；足够 → 在服 + 默认降级表）。
    pub fn register(&mut self, dev_id: u16, s: Spec) -> Result<bool, u16> {
        if self.devs.len() >= DEV_CAP {
            return Err(XC_DEV_FULL);
        }
        for d in self.devs.iter() {
            if d.dev_id == dev_id {
                return Err(XC_DEV_DUP);
            }
        }
        match baseline_check(&s) {
            Ok(()) => {
                // 在服：挂默认降级表（关编码/关画像/禁双精度/保遥测）。
                let degraded = vec![Cap::MediaEncode, Cap::PowerProfile, Cap::DoublePipe];
                self.devs.push(Dev { dev_id, spec: s, serving: true, degraded });
                self.verify()?;
                Ok(true)
            }
            Err(code) => {
                // 断供入台账（判据"申报断供码"）。
                self.devs.push(Dev { dev_id, spec: s, serving: false, degraded: Vec::new() });
                self.cut_list.push((dev_id, code));
                self.verify()?;
                Ok(false)
            }
        }
    }

    /// 在服计数。
    pub fn serving_count(&self) -> usize {
        let mut n = 0;
        for d in self.devs.iter() {
            if d.serving {
                n += 1;
            }
        }
        n
    }

    /// 断供计数。
    pub fn cut_count(&self) -> usize {
        self.cut_list.len()
    }

    /// 查断供码。
    pub fn cut_code(&self, dev_id: u16) -> Result<u16, u16> {
        for (id, code) in self.cut_list.iter() {
            if *id == dev_id {
                return Ok(*code);
            }
        }
        Err(XC_DEV_DUP)
    }

    /// 能力查询（降级表命中 → 不具备）。
    pub fn capable(&self, dev_id: u16, c: Cap) -> Result<bool, u16> {
        let mut found = false;
        let mut degraded = false;
        for d in self.devs.iter() {
            if d.dev_id == dev_id {
                found = true;
                if !d.serving {
                    return Ok(false);
                }
                for g in d.degraded.iter() {
                    if *g == c {
                        degraded = true;
                        break;
                    }
                }
                break;
            }
        }
        if !found {
            return Err(XC_DEGRADE_UNKNOWN);
        }
        if c.is_floor() && degraded {
            // 保底项被降级——表错。
            return Err(XC_DEGRADE_UNKNOWN);
        }
        Ok(!degraded)
    }

    /// 手工降级（表内项可加，保底项拒）。
    pub fn degrade(&mut self, dev_id: u16, c: Cap) -> Result<(), u16> {
        if c.is_floor() {
            return Err(XC_DEGRADE_UNKNOWN);
        }
        for d in self.devs.iter_mut() {
            if d.dev_id == dev_id {
                if !d.serving {
                    return Err(XC_DEGRADE_UNKNOWN);
                }
                for g in d.degraded.iter() {
                    if *g == c {
                        return Err(XC_DEV_DUP);
                    }
                }
                d.degraded.push(c);
                return Ok(());
            }
        }
        Err(XC_DEGRADE_UNKNOWN)
    }

    /// 守恒校验（判据"下限守恒账"）。
    pub fn verify(&self) -> Result<(), u16> {
        let sv = self.serving_count();
        let cut = self.cut_count();
        if sv + cut != self.devs.len() {
            return Err(XC_LEDGER_DRIFT);
        }
        // 台账与明细对位复核。
        let mut cut_ids = 0;
        for d in self.devs.iter() {
            if !d.serving {
                cut_ids += 1;
            }
        }
        if cut_ids != cut {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb837[");
        s.push_str(&self.name);
        s.push_str("] devs=");
        s.push_str(&self.devs.len().to_string());
        s.push_str(" serving=");
        s.push_str(&self.serving_count().to_string());
        s.push_str(" cut=");
        s.push_str(&self.cut_count().to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut b = LegacyBook::new("w3-drill-837");

    // 负样本 1：基线三检逐项断供。
    let low_eu = Spec { eus: 4, bandwidth_gbs: 8, media_bits: DECODE_BIT };
    match b.register(0x0101, low_eu) {
        Ok(false) => {}
        Ok(true) => return Err(XC_EU_TOO_LOW),
        Err(other) => return Err(other),
    }
    if b.cut_code(0x0101)? != XC_EU_TOO_LOW {
        return Err(XC_LEDGER_DRIFT);
    }
    let low_bw = Spec { eus: 8, bandwidth_gbs: 2, media_bits: DECODE_BIT };
    match b.register(0x0102, low_bw) {
        Ok(false) => {}
        Ok(true) => return Err(XC_BW_TOO_LOW),
        Err(other) => return Err(other),
    }
    if b.cut_code(0x0102)? != XC_BW_TOO_LOW {
        return Err(XC_LEDGER_DRIFT);
    }
    let nodec = Spec { eus: 8, bandwidth_gbs: 8, media_bits: 0 };
    match b.register(0x0103, nodec) {
        Ok(false) => {}
        Ok(true) => return Err(XC_NO_DECODE),
        Err(other) => return Err(other),
    }
    if b.cut_code(0x0103)? != XC_NO_DECODE {
        return Err(XC_LEDGER_DRIFT);
    }

    // 正样本：基线以上在服（默认降级表生效）。
    let ok = Spec { eus: 12, bandwidth_gbs: 16, media_bits: DECODE_BIT | 0b0010 };
    if !b.register(0x0201, ok)? {
        return Err(XC_LEDGER_DRIFT);
    }
    // 在服但无编码能力（默认降级命中）。
    if b.capable(0x0201, Cap::MediaEncode)? {
        return Err(XC_LEDGER_DRIFT);
    }
    if b.capable(0x0201, Cap::PowerProfile)? {
        return Err(XC_LEDGER_DRIFT);
    }
    if b.capable(0x0201, Cap::DoublePipe)? {
        return Err(XC_LEDGER_DRIFT);
    }
    // 保底遥测必须具备。
    if !b.capable(0x0201, Cap::Telemetry)? {
        return Err(XC_LEDGER_DRIFT);
    }

    // 负样本 2：重复登记。
    match b.register(0x0201, ok) {
        Err(code) if code == XC_DEV_DUP => {}
        Ok(_) => return Err(XC_DEV_DUP),
        Err(other) => return Err(other),
    }
    // 负样本 3：查未登记设备能力。
    match b.capable(0x9999, Cap::Telemetry) {
        Err(code) if code == XC_DEGRADE_UNKNOWN => {}
        Ok(_) => return Err(XC_DEGRADE_UNKNOWN),
        Err(other) => return Err(other),
    }
    // 负样本 4：对断供设备查能力（一律不具备，但降级表为空——走 serving=false 路径）。
    if b.capable(0x0101, Cap::Telemetry)? {
        return Err(XC_LEDGER_DRIFT);
    }
    // 负样本 5：保底项手工降级——应拒。
    match b.degrade(0x0201, Cap::Telemetry) {
        Err(code) if code == XC_DEGRADE_UNKNOWN => {}
        Ok(_) => return Err(XC_DEGRADE_UNKNOWN),
        Err(other) => return Err(other),
    }
    // 手工降级表内新项（重登项已默认全关——手工加重复项应拒）。
    match b.degrade(0x0201, Cap::MediaEncode) {
        Err(code) if code == XC_DEV_DUP => {}
        Ok(_) => return Err(XC_DEV_DUP),
        Err(other) => return Err(other),
    }

    // 再登一台在服设备走手工降级成功路径。
    let ok2 = Spec { eus: 6, bandwidth_gbs: 4, media_bits: DECODE_BIT };
    if !b.register(0x0202, ok2)? {
        return Err(XC_LEDGER_DRIFT);
    }
    b.degrade(0x0202, Cap::MediaEncode)?;
    if b.capable(0x0202, Cap::MediaEncode)? {
        return Err(XC_LEDGER_DRIFT);
    }
    if !b.capable(0x0202, Cap::Telemetry)? {
        return Err(XC_LEDGER_DRIFT);
    }

    // 守恒终检。
    b.verify()?;
    if b.serving_count() != 2 || b.cut_count() != 3 || b.devs.len() != 5 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
