//! cgb829_conformance — CGPU-F0829 Intel 一致性。
//!
//! 判据原文（验收行摘录）：Intel 一致性：差分对照表（同一输入经配置 A/B
//! 两路求值，结果对账）；逐字节比对（摘要不等即差分违例）；配置扰动单
//! 元（扰动项白名单——白名单外扰动即配置违约）；差分账守恒（用例数 =
//! 一致 + 违例 + 跳过）。判据：差分表、逐字节、扰动白名单、守恒账。
//!
//! 诊断段 0x7220（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；摘要用 FNV-1a 纯整数；定容用例账；drill()
//! 自例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb829-v1";

/// 诊断段基（本单独占 0x7220）。
pub const SEG_BASE: u16 = 0x7220;

/// 配置未就绪（未声明配置 A/B 即跑差分）。
pub const XC_CFG_NOT_SET: u16 = SEG_BASE + 1;
/// 扰动越权（白名单外扰动项）。
pub const XC_TAMPER_DENIED: u16 = SEG_BASE + 2;
/// 差分违例（两路摘要不等）。
pub const XC_DIFF_MISMATCH: u16 = SEG_BASE + 3;
/// 输入超容（差分输入超定容）。
pub const XC_INPUT_LONG: u16 = SEG_BASE + 4;
/// 用例账漂移（守恒破坏）。
pub const XC_LEDGER_DRIFT: u16 = SEG_BASE + 5;
/// 路径未知（求值路径号非 A/B）。
pub const XC_PATH_UNKNOWN: u16 = SEG_BASE + 6;

/// 输入定容。
pub const INPUT_CAP: usize = 32;
/// 用例账定容。
pub const CASE_CAP: usize = 32;

/// 配置项闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CfgKey {
    /// 内存域选择。
    DomainSel,
    /// tiling 模式。
    TileSel,
    /// 调度策略。
    SchedSel,
}

impl CfgKey {
    /// 键代号。
    pub fn code(self) -> u64 {
        match self {
            CfgKey::DomainSel => 1,
            CfgKey::TileSel => 2,
            CfgKey::SchedSel => 3,
        }
    }
}

/// 配置（键值对定容集）。
#[derive(Clone, Debug)]
pub struct Cfg {
    /// 键序列。
    pub keys: Vec<CfgKey>,
    /// 值序列（与键对位）。
    pub vals: Vec<u64>,
}

impl Cfg {
    /// 构造。
    pub fn new() -> Self {
        Cfg { keys: Vec::new(), vals: Vec::new() }
    }
    /// 置项（存在则改，不存在则增）。
    pub fn set(&mut self, k: CfgKey, v: u64) -> Result<(), u16> {
        for (i, kk) in self.keys.iter().enumerate() {
            if *kk == k {
                self.vals[i] = v;
                return Ok(());
            }
        }
        self.keys.push(k);
        self.vals.push(v);
        Ok(())
    }
    /// 取项（缺省 0）。
    pub fn get(&self, k: CfgKey) -> u64 {
        for (i, kk) in self.keys.iter().enumerate() {
            if *kk == k {
                return self.vals[i];
            }
        }
        0
    }
    /// 配置摘要（键值对 FNV 合成）。
    pub fn digest(&self) -> u64 {
        let mut acc = 0xcbf29ce484222325u64;
        for (i, kk) in self.keys.iter().enumerate() {
            acc ^= kk.code();
            acc = acc.wrapping_mul(0x100000001b3);
            acc ^= self.vals[i];
            acc = acc.wrapping_mul(0x100000001b3);
        }
        acc
    }
}

/// 求值路径 A/B（判据"配置 A/B 两路"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path {
    /// 路径 A：参考实现。
    PathA,
    /// 路径 B：被测实现。
    PathB,
}

/// 差分求值（两路模拟：同输入同配置摘要下结果必须一致）。
/// 模型：结果摘要 = FNV(输入) 异或 配置摘要 的低位折叠——两路算法相同，
/// 差分违例仅可能由配置扰动越权引入。
pub fn eval_path(p: Path, input: &[u8], cfg: &Cfg) -> Result<u64, u16> {
    let _ = match p {
        Path::PathA | Path::PathB => 0,
    };
    if input.len() > INPUT_CAP {
        return Err(XC_INPUT_LONG);
    }
    let mut acc = 0x84222325cbf29348u64;
    for b in input {
        acc ^= *b as u64;
        acc = acc.wrapping_mul(0x100000001b3);
    }
    Ok(acc ^ (cfg.digest() & 0xFF))
}

/// 扰动白名单（判据"扰动项白名单"——仅 TileSel 可在 B 路扰动）。
pub fn tamper_allowed(k: CfgKey) -> bool {
    k == CfgKey::TileSel
}

/// 差分用例账条目。
#[derive(Clone, Debug)]
pub struct Case {
    /// 用例名。
    pub name: String,
    /// 输入摘要。
    pub in_digest: u64,
    /// A 路结果。
    pub res_a: u64,
    /// B 路结果。
    pub res_b: u64,
    /// 一致否。
    pub same: bool,
}

/// 一致性差分器。
pub struct Differ {
    /// 差分器名。
    pub name: String,
    /// 配置 A。
    cfg_a: Option<Cfg>,
    /// 配置 B。
    cfg_b: Option<Cfg>,
    /// 用例账（定容）。
    pub cases: Vec<Case>,
    /// 一致计数。
    pub same_cnt: u64,
    /// 违例计数。
    pub diff_cnt: u64,
    /// 跳过计数。
    pub skip_cnt: u64,
}

/// 守恒恒等式：cases.len() == same_cnt + diff_cnt + skip_cnt。
/// 每条用例落账后 verify() 复核。
impl Differ {
    /// 构造。
    pub fn new(name: &str) -> Self {
        Differ {
            name: name.to_string(),
            cfg_a: None,
            cfg_b: None,
            cases: Vec::new(),
            same_cnt: 0,
            diff_cnt: 0,
            skip_cnt: 0,
        }
    }

    /// 声明配置 A。
    pub fn set_a(&mut self, c: Cfg) {
        self.cfg_a = Some(c);
    }
    /// 声明配置 B。
    pub fn set_b(&mut self, c: Cfg) {
        self.cfg_b = Some(c);
    }

    /// 跑一条差分用例。
    pub fn run_case(&mut self, name: &str, input: &[u8]) -> Result<bool, u16> {
        if self.cfg_a.is_none() || self.cfg_b.is_none() {
            return Err(XC_CFG_NOT_SET);
        }
        if input.len() > INPUT_CAP {
            return Err(XC_INPUT_LONG);
        }
        if self.cases.len() >= CASE_CAP {
            self.skip_cnt += 1;
            self.cases.push(Case {
                name: name.to_string(),
                in_digest: 0,
                res_a: 0,
                res_b: 0,
                same: false,
            });
            self.verify()?;
            return Err(XC_INPUT_LONG);
        }
        let ra = eval_path(Path::PathA, input, self.cfg_a.as_ref().ok_or(XC_CFG_NOT_SET)?)?;
        let rb = eval_path(Path::PathB, input, self.cfg_b.as_ref().ok_or(XC_CFG_NOT_SET)?)?;
        let mut in_acc = 0xcbf29ce484222325u64;
        for b in input {
            in_acc ^= *b as u64;
            in_acc = in_acc.wrapping_mul(0x100000001b3);
        }
        let same = ra == rb;
        self.cases.push(Case { name: name.to_string(), in_digest: in_acc, res_a: ra, res_b: rb, same });
        if same {
            self.same_cnt += 1;
        } else {
            self.diff_cnt += 1;
        }
        self.verify()?;
        Ok(same)
    }

    /// 扰动 B 路配置并重跑（白名单校验——判据"扰动白名单"）。
    pub fn tamper_and_run(&mut self, name: &str, k: CfgKey, v: u64, input: &[u8]) -> Result<bool, u16> {
        if !tamper_allowed(k) {
            return Err(XC_TAMPER_DENIED);
        }
        let mut cb = match &self.cfg_b {
            Some(c) => c.clone(),
            None => return Err(XC_CFG_NOT_SET),
        };
        cb.set(k, v)?;
        let saved = self.cfg_b.replace(cb);
        let r = self.run_case(name, input);
        self.cfg_b = saved;
        r
    }

    /// 摘要核验（判据"逐字节比对"的账面回读：两路结果逐位一致才一致）。
    pub fn audit_case(&self, idx: usize) -> Result<bool, u16> {
        if idx >= self.cases.len() {
            return Err(XC_PATH_UNKNOWN);
        }
        let c = &self.cases[idx];
        let same = c.res_a == c.res_b;
        if same != c.same {
            return Err(XC_DIFF_MISMATCH);
        }
        Ok(same)
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        let n = self.cases.len() as u64;
        if n != self.same_cnt + self.diff_cnt + self.skip_cnt {
            return Err(XC_LEDGER_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb829[");
        s.push_str(&self.name);
        s.push_str("] cases=");
        s.push_str(&self.cases.len().to_string());
        s.push_str(" same=");
        s.push_str(&self.same_cnt.to_string());
        s.push_str(" diff=");
        s.push_str(&self.diff_cnt.to_string());
        s.push_str(" skip=");
        s.push_str(&self.skip_cnt.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut d = Differ::new("w3-drill-829");

    // 负样本 1：配置未声明——应拒。
    match d.run_case("cold", b"in-1") {
        Err(code) if code == XC_CFG_NOT_SET => {}
        Ok(_) => return Err(XC_CFG_NOT_SET),
        Err(other) => return Err(other),
    }

    // 声明同摘要配置（保证一致基线）。
    let mut a = Cfg::new();
    a.set(CfgKey::DomainSel, 1)?;
    a.set(CfgKey::SchedSel, 2)?;
    let mut b = Cfg::new();
    b.set(CfgKey::DomainSel, 1)?;
    b.set(CfgKey::SchedSel, 2)?;
    d.set_a(a);
    d.set_b(b);

    // 正样本：三条一致用例。
    if d.run_case("case-1", b"in-1")? != true {
        return Err(XC_DIFF_MISMATCH);
    }
    if d.run_case("case-2", b"in-2-longer-input")? != true {
        return Err(XC_DIFF_MISMATCH);
    }
    let empty: Vec<u8> = Vec::new();
    if d.run_case("case-3", &empty)? != true {
        return Err(XC_DIFF_MISMATCH);
    }

    // 负样本 2：白名单外扰动——应拒。
    match d.tamper_and_run("t-denied", CfgKey::SchedSel, 9, b"in-t") {
        Err(code) if code == XC_TAMPER_DENIED => {}
        Ok(_) => return Err(XC_TAMPER_DENIED),
        Err(other) => return Err(other),
    }
    // 正样本：白名单内扰动（TileSel 改值——摘要变，但求值路径仍应给出一致账？不：
    // 扰动即制造差异——扰动用例的 B 路配置不同则摘要不同，跑出违例是预期行为。
    // 为保持"扰动只允许、不必然一致"的语义，这里只验证白名单放行且账守恒。
    let r = d.tamper_and_run("t-allowed", CfgKey::TileSel, 5, b"in-t");
    match r {
        Ok(_) => {}
        Err(XC_DIFF_MISMATCH) => {}
        Err(other) => return Err(other),
    }
    d.verify()?;

    // 负样本 3：输入超容——应拒。
    let big = [9u8; INPUT_CAP + 1];
    match d.run_case("big", &big) {
        Err(code) if code == XC_INPUT_LONG => {}
        Ok(_) => return Err(XC_INPUT_LONG),
        Err(other) => return Err(other),
    }

    // 账面回读核验。
    for i in 0..d.cases.len() {
        let same = d.audit_case(i)?;
        let c_same = d.cases[i].same;
        if same != c_same {
            return Err(XC_DIFF_MISMATCH);
        }
    }

    // 差分违例路径：把 B 配置改成不同摘要后跑普通用例——两路结果必异。
    let mut b2 = Cfg::new();
    b2.set(CfgKey::DomainSel, 1)?;
    b2.set(CfgKey::SchedSel, 3)?;
    d.set_b(b2);
    match d.run_case("case-diverge", b"in-1") {
        Ok(false) => {}
        Ok(true) => return Err(XC_DIFF_MISMATCH),
        Err(other) => return Err(other),
    }
    d.verify()?;
    if d.diff_cnt < 1 {
        return Err(XC_LEDGER_DRIFT);
    }
    Ok(0)
}
