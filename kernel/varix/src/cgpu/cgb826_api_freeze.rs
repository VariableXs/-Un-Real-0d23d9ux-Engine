//! cgb826_api_freeze — CGPU-F0826 Intel 适配层 API 冻结。
//!
//! 判据原文（验收行摘录）：Intel 适配层 API 冻结：签名指纹表（入口名+参
//! 数型别序列合成指纹）；冻结基线落账（指纹表快照不可变）；兼容性核验
//! （新增入口=可追加，改签名/删入口=破坏性）；变更审批账（破坏性变更须
//! 双键确认才放行）。判据：指纹表、冻结基线、兼容三判、双键审批。
//!
//! 诊断段 0x71C0（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；指纹纯整数合成；定容签名表；drill() 自例
//! 程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb826-v1";

/// 诊断段基（本单独占 0x71C0）。
pub const SEG_BASE: u16 = 0x71C0;

/// 入口重名（同名入口重复登记）。
pub const XC_DUP_ENTRY: u16 = SEG_BASE + 1;
/// 参数过多（参数型别序列超定容）。
pub const XC_SIG_TOO_LONG: u16 = SEG_BASE + 2;
/// 基线未冻结（冻结前核验兼容性）。
pub const XC_NOT_FROZEN: u16 = SEG_BASE + 3;
/// 重复冻结（基线已冻结再次冻结）。
pub const XC_ALREADY_FROZEN: u16 = SEG_BASE + 4;
/// 删改入口（冻结后删除或改签名已有入口——破坏性）。
pub const XC_BREAKING_CHANGE: u16 = SEG_BASE + 5;
/// 审批未备（破坏性变更无双键确认——不放行）。
pub const XC_APPROVAL_MISSING: u16 = SEG_BASE + 6;
/// 表漂移（签名表计数与账目不符——守恒破坏）。
pub const XC_TABLE_DRIFT: u16 = SEG_BASE + 7;

/// 签名表定容。
pub const SIG_CAP: usize = 24;
/// 参数序列定容。
pub const PARAM_CAP: usize = 8;

/// 参数型别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PType {
    /// 无符号 32 位。
    U32,
    /// 无符号 64 位。
    U64,
    /// 句柄。
    Handle,
    /// 字节缓冲。
    Buf,
}

impl PType {
    /// 型别代号（指纹合成用，闭集稳定值）。
    pub fn code(self) -> u64 {
        match self {
            PType::U32 => 1,
            PType::U64 => 2,
            PType::Handle => 3,
            PType::Buf => 4,
        }
    }
}

/// 入口签名。
#[derive(Clone, Debug)]
pub struct Sig {
    /// 入口名。
    pub name: String,
    /// 参数型别序列。
    pub params: Vec<PType>,
    /// 返回型别。
    pub ret: PType,
}

/// 签名指纹（判据"签名指纹表"：名哈希 + 型别序列编码）。
pub fn fingerprint(s: &Sig) -> Result<u64, u16> {
    if s.params.len() > PARAM_CAP {
        return Err(XC_SIG_TOO_LONG);
    }
    let mut acc = 5381u64;
    for b in s.name.as_bytes() {
        acc = acc.wrapping_mul(33).wrapping_add(*b as u64);
    }
    acc = acc.wrapping_mul(33).wrapping_add(s.ret.code());
    for p in s.params.iter() {
        acc = acc.wrapping_mul(31).wrapping_add(p.code());
    }
    Ok(acc)
}

/// 兼容性三判（判据"兼容三判"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Compat {
    /// 可追加：新入口。
    Append,
    /// 保留：同名同签名。
    Keep,
    /// 破坏性：同名异签名或已删。
    Breaking,
}

/// 兼容性核验结果。
#[derive(Clone, Debug)]
pub struct Verdict {
    /// 入口名。
    pub name: String,
    /// 判定。
    pub kind: Compat,
}

/// API 冻结账。
pub struct FreezeBook {
    /// 账名。
    pub name: String,
    /// 签名表（冻结后为基线）。
    sigs: Vec<Sig>,
    /// 指纹表（与 sigs 一一对应）。
    fps: Vec<u64>,
    /// 冻结位。
    frozen: bool,
    /// 累计追加。
    pub appended: u64,
    /// 累计破坏性放行（须双键）。
    pub breakouts: u64,
    /// 审批键 A 状态。
    key_a: bool,
    /// 审批键 B 状态。
    key_b: bool,
}

/// 守恒恒等式：sigs.len() == fps.len()，且表内指纹重算一致。
/// 每次登记/放行后 verify() 复核。
impl FreezeBook {
    /// 构造。
    pub fn new(name: &str) -> Self {
        FreezeBook {
            name: name.to_string(),
            sigs: Vec::new(),
            fps: Vec::new(),
            frozen: false,
            appended: 0,
            breakouts: 0,
            key_a: false,
            key_b: false,
        }
    }

    /// 登记入口（冻结前自由登记，重名拒）。
    pub fn register(&mut self, s: Sig) -> Result<u64, u16> {
        if self.frozen {
            return Err(XC_ALREADY_FROZEN);
        }
        if self.sigs.len() >= SIG_CAP {
            return Err(XC_SIG_TOO_LONG);
        }
        for e in self.sigs.iter() {
            if e.name == s.name {
                return Err(XC_DUP_ENTRY);
            }
        }
        let fp = fingerprint(&s)?;
        self.sigs.push(s);
        self.fps.push(fp);
        self.verify()?;
        Ok(fp)
    }

    /// 冻结基线（判据"冻结基线落账"——此后表只增不改）。
    pub fn freeze(&mut self) -> Result<usize, u16> {
        if self.frozen {
            return Err(XC_ALREADY_FROZEN);
        }
        if self.sigs.is_empty() {
            return Err(XC_NOT_FROZEN);
        }
        self.frozen = true;
        Ok(self.sigs.len())
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 表规模。
    pub fn len(&self) -> usize {
        self.sigs.len()
    }

    /// 查入口。
    fn find(&self, name: &str) -> Result<usize, u16> {
        for (i, s) in self.sigs.iter().enumerate() {
            if s.name == name {
                return Ok(i);
            }
        }
        Err(XC_BREAKING_CHANGE)
    }

    /// 审批键置位。
    pub fn approve(&mut self, which: u8) -> Result<(), u16> {
        if which == 1 {
            self.key_a = true;
        } else if which == 2 {
            self.key_b = true;
        } else {
            return Err(XC_APPROVAL_MISSING);
        }
        Ok(())
    }

    /// 审批键复位（放行后自动清空——一变一审）。
    fn clear_keys(&mut self) {
        self.key_a = false;
        self.key_b = false;
    }

    /// 兼容性核验（判据"兼容三判"）。
    pub fn check(&self, s: &Sig) -> Result<Verdict, u16> {
        if !self.frozen {
            return Err(XC_NOT_FROZEN);
        }
        match self.find(&s.name) {
            Ok(i) => {
                let old_fp = self.fps[i];
                let new_fp = fingerprint(s)?;
                if old_fp == new_fp {
                    Ok(Verdict { name: s.name.clone(), kind: Compat::Keep })
                } else {
                    Ok(Verdict { name: s.name.clone(), kind: Compat::Breaking })
                }
            }
            Err(_) => {
                if self.sigs.len() >= SIG_CAP {
                    return Err(XC_SIG_TOO_LONG);
                }
                Ok(Verdict { name: s.name.clone(), kind: Compat::Append })
            }
        }
    }

    /// 冻结后追加入口（可追加类直接放行）。
    pub fn append(&mut self, s: Sig) -> Result<u64, u16> {
        if !self.frozen {
            return Err(XC_NOT_FROZEN);
        }
        if self.sigs.len() >= SIG_CAP {
            return Err(XC_SIG_TOO_LONG);
        }
        let v = self.check(&s)?;
        if v.kind == Compat::Breaking {
            // 判据"破坏性变更须双键确认才放行"。
            if !self.key_a || !self.key_b {
                return Err(XC_APPROVAL_MISSING);
            }
            self.clear_keys();
            self.breakouts += 1;
            // 放行 = 原位替换签名。
            let i = self.find(&s.name)?;
            let fp = fingerprint(&s)?;
            self.fps[i] = fp;
            self.sigs[i] = s;
            self.verify()?;
            return Ok(fp);
        }
        if v.kind == Compat::Keep {
            let i = self.find(&s.name)?;
            return Ok(self.fps[i]);
        }
        // Append。
        if self.find(&s.name).is_ok() {
            return Err(XC_DUP_ENTRY);
        }
        let fp = fingerprint(&s)?;
        self.sigs.push(s);
        self.fps.push(fp);
        self.appended += 1;
        self.verify()?;
        Ok(fp)
    }

    /// 守恒校验（判据"表漂移"）。
    pub fn verify(&self) -> Result<(), u16> {
        if self.sigs.len() != self.fps.len() {
            return Err(XC_TABLE_DRIFT);
        }
        for (i, s) in self.sigs.iter().enumerate() {
            let fp = fingerprint(s)?;
            if fp != self.fps[i] {
                return Err(XC_TABLE_DRIFT);
            }
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb826[");
        s.push_str(&self.name);
        s.push_str("] frozen=");
        s.push_str(if self.frozen { "1" } else { "0" });
        s.push_str(" sigs=");
        s.push_str(&self.sigs.len().to_string());
        s.push_str(" append=");
        s.push_str(&self.appended.to_string());
        s.push_str(" brk=");
        s.push_str(&self.breakouts.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut bk = FreezeBook::new("w3-drill-826");

    // 负样本 1：冻结前核验——应拒。
    let s0 = Sig { name: "shm_alloc".to_string(), params: vec![PType::U32, PType::Buf], ret: PType::Handle };
    match bk.check(&s0) {
        Err(code) if code == XC_NOT_FROZEN => {}
        Ok(_) => return Err(XC_NOT_FROZEN),
        Err(other) => return Err(other),
    }

    // 登记 3 个入口。
    bk.register(Sig { name: "shm_alloc".to_string(), params: vec![PType::U32, PType::Buf], ret: PType::Handle })?;
    bk.register(Sig { name: "media_submit".to_string(), params: vec![PType::Handle, PType::U32], ret: PType::U32 })?;
    bk.register(Sig { name: "power_sample".to_string(), params: vec![], ret: PType::U64 })?;

    // 负样本 2：重名登记——应拒。
    match bk.register(Sig { name: "shm_alloc".to_string(), params: vec![], ret: PType::U32 }) {
        Err(code) if code == XC_DUP_ENTRY => {}
        Ok(_) => return Err(XC_DUP_ENTRY),
        Err(other) => return Err(other),
    }
    // 负样本 3：参数序列超容——应拒。
    let mut many: Vec<PType> = Vec::new();
    for _ in 0..PARAM_CAP + 1 {
        many.push(PType::U32);
    }
    match bk.register(Sig { name: "too_many".to_string(), params: many, ret: PType::U32 }) {
        Err(code) if code == XC_SIG_TOO_LONG => {}
        Ok(_) => return Err(XC_SIG_TOO_LONG),
        Err(other) => return Err(other),
    }

    // 冻结。
    let n = bk.freeze()?;
    if n != 3 || !bk.is_frozen() {
        return Err(XC_TABLE_DRIFT);
    }
    // 负样本 4：重复冻结——应拒。
    match bk.freeze() {
        Err(code) if code == XC_ALREADY_FROZEN => {}
        Ok(_) => return Err(XC_ALREADY_FROZEN),
        Err(other) => return Err(other),
    }

    // 正样本 1：Keep——同签名。
    let v1 = bk.check(&Sig { name: "shm_alloc".to_string(), params: vec![PType::U32, PType::Buf], ret: PType::Handle })?;
    if v1.kind != Compat::Keep {
        return Err(XC_TABLE_DRIFT);
    }
    // 正样本 2：Append——新入口。
    let v2 = bk.check(&Sig { name: "hybrid_route".to_string(), params: vec![PType::Handle], ret: PType::U32 })?;
    if v2.kind != Compat::Append {
        return Err(XC_TABLE_DRIFT);
    }
    bk.append(Sig { name: "hybrid_route".to_string(), params: vec![PType::Handle], ret: PType::U32 })?;
    if bk.len() != 4 || bk.appended != 1 {
        return Err(XC_TABLE_DRIFT);
    }

    // 破坏性：改 shm_alloc 签名。
    let v3 = bk.check(&Sig { name: "shm_alloc".to_string(), params: vec![PType::U64, PType::Buf], ret: PType::Handle })?;
    if v3.kind != Compat::Breaking {
        return Err(XC_TABLE_DRIFT);
    }
    // 负样本 5：无双键——应拒。
    bk.approve(1)?;
    match bk.append(Sig { name: "shm_alloc".to_string(), params: vec![PType::U64, PType::Buf], ret: PType::Handle }) {
        Err(code) if code == XC_APPROVAL_MISSING => {}
        Ok(_) => return Err(XC_APPROVAL_MISSING),
        Err(other) => return Err(other),
    }
    // 双键齐——放行。
    bk.approve(2)?;
    bk.append(Sig { name: "shm_alloc".to_string(), params: vec![PType::U64, PType::Buf], ret: PType::Handle })?;
    if bk.breakouts != 1 {
        return Err(XC_TABLE_DRIFT);
    }
    // 键已清——再变再审。
    match bk.append(Sig { name: "media_submit".to_string(), params: vec![PType::U32], ret: PType::U32 }) {
        Err(code) if code == XC_APPROVAL_MISSING => {}
        Ok(_) => return Err(XC_APPROVAL_MISSING),
        Err(other) => return Err(other),
    }
    bk.verify()?;
    if bk.len() != 4 {
        return Err(XC_TABLE_DRIFT);
    }
    Ok(0)
}
