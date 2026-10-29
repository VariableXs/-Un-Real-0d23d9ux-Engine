//! unx48238_hotremap — UNX-F48238 切换热键重映射面（切换键可配置+系统冲突检测）（诊断段 0xBEA0）。
//!
//! 判据原文：
//!   J1 重映射 ×10 生效、冲突 ×10 检出显性提示；判据锚定 体验四 快捷键是承诺。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 重映射承诺：切换热键 (键,修饰) 可配置；重映射后旧键让位、
//!      新键生效，语义位不变（换键不换义）。
//!   2. 冲突检测闭式：新键与系统保留键表 + 已占键位比对，命中即拒
//!      并给出显性冲突对象（保留表名 或 已占语义名）——静默吞键=红线。
//!   3. ×10 生效判据：10 组重映射样本，逐组「重映射→事件→意图」
//!      全链核对生效；×10 冲突样本逐组检出且对象正确。
//!   4. 单键单义：同一 (键,修饰) 在热键表内唯一；卸载旧绑定后才能
//!      重新绑定（链式 swap 显性两步，不许隐式覆盖）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;      // vec! 宏（坑 2 三件套）
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBEA0;

const E_SYS_CONFLICT: u16 = SEG_BASE + 1; // 系统保留键冲突
const E_OCC_CONFLICT: u16 = SEG_BASE + 2; // 已占键位冲突
const E_NOT_BOUND: u16 = SEG_BASE + 3;    // 卸载不存在的绑定
const E_DUP_BOUND: u16 = SEG_BASE + 4;    // 重复绑定
const E_BAD_KEY: u16 = SEG_BASE + 5;      // 键面闭集外
const E_SEAL_MISMATCH: u16 = SEG_BASE + 6; // 封账指纹不符
const E_GUARD_UNBAL: u16 = SEG_BASE + 7;  // 热键账不平

/// 系统保留键表（冲突检测第一道）。
pub const SYS_RESERVED: [(&str, u8, u8); 6] = [
    ("Alt+Tab", 9, 4),      // (名, 键码, 修饰位图)
    ("Win 键", 91, 0),
    ("Ctrl+Alt+Del", 46, 6),
    ("PrintScreen", 55, 0),
    ("Alt+F4", 33, 4),
    ("Ctrl+Esc", 1, 2),
];

/// 热键语义闭集：中英切换 / 临时英文 / 候选显隐。
pub const S_TOGGLE: u8 = 1;
pub const S_TEMPEN: u8 = 2;
pub const S_CANDSHOW: u8 = 3;

/// 一条热键绑定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bind {
    pub key: u8,
    pub mods: u8,
    pub sem: u8,
}

/// 热键重映射总账。
#[derive(Debug)]
pub struct HotRemap {
    binds: Vec<Bind>,
    history: Vec<(u8, u8, u8, u8, u8)>, // (序, 旧key, 旧mods, 新key, 新mods)
    conflicts: Vec<(u8, u8, u16)>,      // (key, mods, 冲突码)
    seq: u32,
    sealed: Option<u32>,
}

impl HotRemap {
    pub fn new() -> Self {
        HotRemap {
            binds: vec![
                Bind { key: 1, mods: 0, sem: S_TEMPEN },  // 默认：左Shift=临时英文
                Bind { key: 3, mods: 0, sem: S_TOGGLE },  // 默认：CapsLock=中英切换
            ],
            history: Vec::new(),
            conflicts: Vec::new(),
            seq: 0,
            sealed: None,
        }
    }

    fn sys_conflict(key: u8, mods: u8) -> bool {
        SYS_RESERVED.iter().any(|(_, k, m)| *k == key && *m == mods)
    }

    /// 绑定新热键（首次）：冲突三查（系统/已占/闭集）。
    pub fn bind(&mut self, key: u8, mods: u8, sem: u8) -> Result<(), u16> {
        if key == 0 || key > 120 {
            return Err(E_BAD_KEY);
        }
        if Self::sys_conflict(key, mods) {
            self.conflicts.push((key, mods, E_SYS_CONFLICT));
            return Err(E_SYS_CONFLICT);
        }
        if self.binds.iter().any(|b| b.key == key && b.mods == mods) {
            self.conflicts.push((key, mods, E_OCC_CONFLICT));
            return Err(E_OCC_CONFLICT);
        }
        if self.binds.iter().any(|b| b.sem == sem) {
            // 单键单义：语义已有绑定 → 必须先卸载。
            self.conflicts.push((key, mods, E_OCC_CONFLICT));
            return Err(E_OCC_CONFLICT);
        }
        self.binds.push(Bind { key, mods, sem });
        Ok(())
    }

    /// 重映射（换键）：卸旧 + 绑新两步显性；历史留痕。
    pub fn remap(&mut self, sem: u8, new_key: u8, new_mods: u8) -> Result<(), u16> {
        if new_key == 0 || new_key > 120 {
            return Err(E_BAD_KEY);
        }
        if Self::sys_conflict(new_key, new_mods) {
            self.conflicts.push((new_key, new_mods, E_SYS_CONFLICT));
            return Err(E_SYS_CONFLICT);
        }
        let old = match self.binds.iter().find(|b| b.sem == sem) {
            Some(b) => *b,
            None => return Err(E_NOT_BOUND),
        };
        if self
            .binds
            .iter()
            .any(|b| b.key == new_key && b.mods == new_mods && b.sem != sem)
        {
            self.conflicts.push((new_key, new_mods, E_OCC_CONFLICT));
            return Err(E_OCC_CONFLICT);
        }
        let idx = self.binds.iter().position(|b| b.sem == sem).ok_or(E_NOT_BOUND)?;
        self.binds[idx] = Bind { key: new_key, mods: new_mods, sem };
        self.seq += 1;
        self.history.push((self.seq as u8, old.key, old.mods, new_key, new_mods));
        Ok(())
    }

    /// 卸载绑定。
    pub fn unbind(&mut self, sem: u8) -> Result<(), u16> {
        match self.binds.iter().position(|b| b.sem == sem) {
            Some(i) => {
                self.binds.remove(i);
                Ok(())
            }
            None => Err(E_NOT_BOUND),
        }
    }

    /// 事件 → 语义（查表生效判定）。
    pub fn feed(&self, key: u8, mods: u8) -> Result<u8, u16> {
        match self.binds.iter().find(|b| b.key == key && b.mods == mods) {
            Some(b) => Ok(b.sem),
            None => Err(E_NOT_BOUND),
        }
    }

    /// 冲突账导出。
    pub fn conflict_list(&self) -> Vec<(u8, u8, u16)> {
        self.conflicts.clone()
    }

    /// 审计：单键单义不变式 + 历史账与绑定账互恰。
    pub fn audit(&self) -> Result<(), u16> {
        for i in 0..self.binds.len() {
            for j in (i + 1)..self.binds.len() {
                if self.binds[i].key == self.binds[j].key && self.binds[i].mods == self.binds[j].mods {
                    return Err(E_DUP_BOUND);
                }
                if self.binds[i].sem == self.binds[j].sem {
                    return Err(E_DUP_BOUND);
                }
            }
        }
        if self.history.len() as u32 != self.seq {
            return Err(E_GUARD_UNBAL);
        }
        Ok(())
    }

    pub fn seal(&mut self) -> Result<u32, u16> {
        let mut h: u32 = 0x811C_9DC5;
        for b in &self.binds {
            h ^= (b.key as u32) | ((b.mods as u32) << 8) | ((b.sem as u32) << 16);
            h = h.wrapping_mul(0x0100_0193);
        }
        self.sealed = Some(h);
        Ok(h)
    }

    pub fn verify(&self, fp: u32) -> Result<(), u16> {
        match self.sealed {
            Some(s) if s == fp => Ok(()),
            _ => Err(E_SEAL_MISMATCH),
        }
    }

    pub fn binds(&self) -> usize {
        self.binds.len()
    }
}

/// 自例程判据：落实 J1「重映射 ×10 生效、冲突 ×10 检出」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：×10 重映射生效 —— 中英切换键连换 10 次，每次 feed 新键
    //        得 S_TOGGLE、旧键失位。
    let mut h = HotRemap::new();
    let mut cur_key = 3u8; // CapsLock 起点
    for i in 1..=10u8 {
        let nk = 20 + i * 2; // 22,24,...,40
        h.remap(S_TOGGLE, nk, 0)?;
        if h.feed(nk, 0)? != S_TOGGLE {
            return Err(E_GUARD_UNBAL);
        }
        if h.feed(cur_key, 0).is_ok() {
            return Err(E_GUARD_UNBAL); // 旧键必须让位
        }
        cur_key = nk;
    }
    h.audit()?;
    hits += 1;

    // R2 场景：×10 冲突检出 —— 6 系统保留键 + 4 已占键位逐组显性。
    let mut h2 = HotRemap::new();
    let sys_samples = [(9u8, 4u8), (91, 0), (46, 6), (55, 0), (33, 4), (1, 2)];
    for (k, m) in sys_samples {
        match h2.bind(k, m, S_CANDSHOW) {
            Err(E_SYS_CONFLICT) => {}
            _ => return Err(E_SYS_CONFLICT),
        }
    }
    // 已占键位：默认 Shift(1,0)/Caps(3,0) 再绑必拒。
    match h2.bind(1, 0, S_CANDSHOW) {
        Err(E_OCC_CONFLICT) => {}
        _ => return Err(E_OCC_CONFLICT),
    }
    match h2.bind(3, 0, S_CANDSHOW) {
        Err(E_OCC_CONFLICT) => {}
        _ => return Err(E_OCC_CONFLICT),
    }
    // 语义已占：新键绑已占语义必拒。
    match h2.bind(50, 0, S_TEMPEN) {
        Err(E_OCC_CONFLICT) => {}
        _ => return Err(E_OCC_CONFLICT),
    }
    // remap 到已占键位必拒。
    match h2.remap(S_TOGGLE, 1, 0) {
        Err(E_OCC_CONFLICT) => {}
        _ => return Err(E_OCC_CONFLICT),
    }
    if h2.conflict_list().len() != 10 {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R3 场景：冲突对象正确性 —— 系统冲突与占用冲突码分账可查。
    let cl = h2.conflict_list();
    if !cl.iter().any(|(_, _, c)| *c == E_SYS_CONFLICT) || !cl.iter().any(|(_, _, c)| *c == E_OCC_CONFLICT) {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R4 场景：换键不换义 —— 重映射后语义位不变（feed 值恒定）。
    let sem_before = h.feed(cur_key, 0)?;
    let nk = 60;
    h.remap(S_TOGGLE, nk, 0)?;
    if h.feed(nk, 0)? != sem_before {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R5 场景：系统保留键 remap 冲突 —— remap 目标落保留表必拒且绑定不动。
    let before = h.binds();
    match h.remap(S_TOGGLE, 9, 4) {
        Err(E_SYS_CONFLICT) => {}
        _ => return Err(E_SYS_CONFLICT),
    }
    if h.binds() != before {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R6 场景：卸载与重绑 —— unbind 后键位可复用。
    let mut h6 = HotRemap::new();
    h6.unbind(S_TEMPEN)?;
    match h6.unbind(S_TEMPEN) {
        Err(E_NOT_BOUND) => {}
        _ => return Err(E_NOT_BOUND),
    }
    h6.bind(1, 0, S_CANDSHOW)?; // 释放后可绑
    if h6.feed(1, 0)? != S_CANDSHOW {
        return Err(E_GUARD_UNBAL);
    }
    hits += 1;

    // R7 场景：键面闭集外拒收（0 与 121）。
    let mut h7 = HotRemap::new();
    match h7.bind(0, 0, S_CANDSHOW) {
        Err(E_BAD_KEY) => {}
        _ => return Err(E_BAD_KEY),
    }
    match h7.remap(S_TOGGLE, 121, 0) {
        Err(E_BAD_KEY) => {}
        _ => return Err(E_BAD_KEY),
    }
    hits += 1;

    // R8 场景：修饰区分 —— 同键不同修饰是不同绑定。
    let mut h8 = HotRemap::new();
    h8.bind(50, 0, S_CANDSHOW)?;
    h8.bind(50, 1, S_TEMPEN)?;
    if h8.feed(50, 0)? != S_CANDSHOW || h8.feed(50, 1)? != S_TEMPEN {
        return Err(E_GUARD_UNBAL);
    }
    h8.audit()?;
    hits += 1;

    // R9 场景：历史账守恒 —— 重映射历史逐条 (seq,old,new) 且计数一致。
    let h9n = h.history.len() as u32;
    if h9n != h.seq || h.seq < 11 {
        return Err(E_GUARD_UNBAL); // R1 10 次 + R4 1 次
    }
    h.audit()?;
    hits += 1;

    // R10 场景：封账与复核 —— 绑定账变更指纹必变。
    let fp = h.seal()?;
    h.verify(fp)?;
    h.remap(S_TOGGLE, 100, 0)?;
    let fp2 = h.seal()?;
    if fp == fp2 {
        return Err(E_SEAL_MISMATCH);
    }
    match h.verify(fp) {
        Err(E_SEAL_MISMATCH) => {}
        _ => return Err(E_SEAL_MISMATCH),
    }
    hits += 1;

    Ok(hits)
}
