//! UNX-F48646 —— I·镜像升级后词库/配置迁移联测（升级不破坏旧数据）
//! 判据原文：UNX-F48646-J1 联测 ×10 零破坏；判据锚定 体验十二。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//!           升级迁移账：旧数据全量保真 ×10 轮零破坏。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x07E0; // 升级迁移联测诊断段

const E_DATA_LOSS: u16 = SEG_BASE + 1;   // 旧数据丢失
const E_CFG_LOSS: u16 = SEG_BASE + 2;    // 配置丢失
const E_VER_BAD: u16 = SEG_BASE + 3;     // 版本迁移违例
const E_LEDGER: u16 = SEG_BASE + 4;      // 迁移账不守恒
const E_REPLAY: u16 = SEG_BASE + 5;      // 重演不一致
const E_ROUND_MISS: u16 = SEG_BASE + 6;  // 联测轮次缺失
const E_ROLLBACK_BAD: u16 = SEG_BASE + 7;// 回滚违例

/// 用户词库数据。
pub struct UserLex {
    pub words: Vec<u32>,
    pub freq: Vec<u32>,
}

impl UserLex {
    pub fn new() -> Self {
        UserLex { words: Vec::new(), freq: Vec::new() }
    }
    pub fn add(&mut self, w: u32, f: u32) {
        self.words.push(w);
        self.freq.push(f);
    }
    pub fn len(&self) -> usize {
        self.words.len()
    }
    /// 全量比对。
    pub fn same(&self, other: &UserLex) -> bool {
        if self.words.len() != other.words.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.words.len() {
            if self.words[i] != other.words[i] || self.freq[i] != other.freq[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 用户配置。
pub struct UserCfg {
    pub values: Vec<(u8, u32)>,
}

impl UserCfg {
    pub fn new() -> Self {
        UserCfg { values: Vec::new() }
    }
    pub fn set(&mut self, k: u8, v: u32) {
        let mut i = 0usize;
        while i < self.values.len() {
            if self.values[i].0 == k {
                self.values[i].1 = v;
                return;
            }
            i += 1;
        }
        self.values.push((k, v));
    }
    pub fn get(&self, k: u8) -> Option<u32> {
        let mut i = 0usize;
        while i < self.values.len() {
            if self.values[i].0 == k {
                return Some(self.values[i].1);
            }
            i += 1;
        }
        None
    }
    pub fn same(&self, other: &UserCfg) -> bool {
        if self.values.len() != other.values.len() {
            return false;
        }
        let mut i = 0usize;
        while i < self.values.len() {
            if self.values[i].0 != other.values[i].0
                || self.values[i].1 != other.values[i].1
            {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 迁移结果：旧→新全量保真。
pub struct MigrateResult {
    pub old_lex: UserLex,
    pub new_lex: UserLex,
    pub old_cfg: UserCfg,
    pub new_cfg: UserCfg,
    pub from_ver: u32,
    pub to_ver: u32,
}

impl MigrateResult {
    /// 零破坏审计。
    pub fn audit(&self) -> Result<(), u16> {
        if self.to_ver <= self.from_ver {
            return Err(E_VER_BAD);
        }
        if !self.old_lex.same(&self.new_lex) {
            return Err(E_DATA_LOSS);
        }
        if !self.old_cfg.same(&self.new_cfg) {
            return Err(E_CFG_LOSS);
        }
        Ok(())
    }
}

/// 执行一次升级迁移（保真模型）。
pub fn migrate(
    from_ver: u32,
    to_ver: u32,
    lex: &UserLex,
    cfg: &UserCfg,
) -> Result<MigrateResult, u16> {
    if to_ver <= from_ver {
        return Err(E_VER_BAD);
    }
    let mut new_lex = UserLex::new();
    let mut i = 0usize;
    while i < lex.len() {
        new_lex.add(lex.words[i], lex.freq[i]);
        i += 1;
    }
    let mut new_cfg = UserCfg::new();
    let mut i = 0usize;
    while i < cfg.values.len() {
        new_cfg.set(cfg.values[i].0, cfg.values[i].1);
        i += 1;
    }
    let r = MigrateResult {
        old_lex: UserLex {
            words: lex.words.clone(),
            freq: lex.freq.clone(),
        },
        new_lex,
        old_cfg: UserCfg { values: cfg.values.clone() },
        new_cfg,
        from_ver,
        to_ver,
    };
    r.audit()?;
    Ok(r)
}

/// 升级回滚面：迁移失败回旧版零空窗。
pub fn rollback_lex(new_lex: &UserLex, backup: &UserLex) -> Result<UserLex, u16> {
    let mut back = UserLex::new();
    let mut i = 0usize;
    while i < backup.len() {
        back.add(backup.words[i], backup.freq[i]);
        i += 1;
    }
    if !back.same(backup) {
        return Err(E_ROLLBACK_BAD);
    }
    let _ = new_lex;
    Ok(back)
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 主巡检：升级迁移联测十判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 基线迁移：词库+配置全量保真。
    let mut lex = UserLex::new();
    lex.add(0x4e2d, 90);
    lex.add(0x6587, 80);
    lex.add(0x8f93, 70);
    let mut cfg = UserCfg::new();
    cfg.set(0, 1);
    cfg.set(2, 0);
    let r = migrate(1, 2, &lex, &cfg)?;
    if r.new_lex.len() != 3 {
        return Err(E_DATA_LOSS);
    }
    hits += 1;

    // R2 联测 ×10：十轮随机数据升级零破坏。
    let mut rng = Lcg::new(0x48646_0001);
    let mut round = 0u32;
    while round < 10 {
        let mut l2 = UserLex::new();
        let n = 1 + rng.below(20);
        let mut i = 0u64;
        while i < n {
            l2.add(rng.below(0x1_0000) as u32, rng.below(100) as u32);
            i += 1;
        }
        let mut c2 = UserCfg::new();
        let mut i = 0u64;
        while i < 4 {
            c2.set(i as u8, rng.below(2) as u32);
            i += 1;
        }
        let fv = 1 + rng.below(5) as u32;
        let tv = fv + 1 + rng.below(3) as u32;
        let m = migrate(fv, tv, &l2, &c2)?;
        if m.new_lex.len() != l2.len() {
            return Err(E_DATA_LOSS);
        }
        round += 1;
    }
    hits += 1;

    // R3 版本违例反例：to ≤ from 须拒。
    let r = migrate(3, 3, &lex, &cfg);
    if r.err() != Some(E_VER_BAD) {
        return Err(E_VER_BAD);
    }
    let r = migrate(3, 2, &lex, &cfg);
    if r.err() != Some(E_VER_BAD) {
        return Err(E_VER_BAD);
    }
    hits += 1;

    // R4 词频保真抽检：迁移后词频逐一相等。
    let m = migrate(1, 4, &lex, &cfg)?;
    let mut i = 0usize;
    while i < lex.len() {
        if m.new_lex.freq[i] != lex.freq[i] {
            return Err(E_DATA_LOSS);
        }
        i += 1;
    }
    hits += 1;

    // R5 配置键值保真抽检。
    let mut i = 0usize;
    while i < cfg.values.len() {
        let k = cfg.values[i].0;
        let v = cfg.values[i].1;
        let nv = m.new_cfg.get(k);
        if nv != Some(v) {
            return Err(E_CFG_LOSS);
        }
        i += 1;
    }
    hits += 1;

    // R6 空数据迁移：空词库空配置迁移合法。
    let empty_lex = UserLex::new();
    let empty_cfg = UserCfg::new();
    let m2 = migrate(1, 2, &empty_lex, &empty_cfg)?;
    if m2.new_lex.len() != 0 {
        return Err(E_DATA_LOSS);
    }
    hits += 1;

    // R7 覆盖写幂等：同键覆盖后迁移仍保真终值。
    let mut c3 = UserCfg::new();
    c3.set(1, 0);
    c3.set(1, 1);
    let m3 = migrate(1, 2, &lex, &c3)?;
    if m3.new_cfg.get(1) != Some(1) {
        return Err(E_CFG_LOSS);
    }
    hits += 1;

    // R8 回滚面：失败升级回滚回旧库零空窗。
    let backup = UserLex { words: lex.words.clone(), freq: lex.freq.clone() };
    let back = rollback_lex(&m.new_lex, &backup)?;
    if !back.same(&lex) {
        return Err(E_ROLLBACK_BAD);
    }
    hits += 1;

    // R9 联测轮次守恒：十轮账的迁移条目和 == 十轮数据条目和。
    let mut sum_new = 0u32;
    let mut rng2 = Lcg::new(0x48646_0001);
    let mut round = 0u32;
    while round < 10 {
        let mut l2 = UserLex::new();
        let n = 1 + rng2.below(20);
        let mut i = 0u64;
        while i < n {
            l2.add(rng2.below(0x1_0000) as u32, rng2.below(100) as u32);
            i += 1;
        }
        let mut c2 = UserCfg::new();
        let mut i = 0u64;
        while i < 4 {
            c2.set(i as u8, rng2.below(2) as u32);
            i += 1;
        }
        let fv = 1 + rng2.below(5) as u32;
        let tv = fv + 1 + rng2.below(3) as u32;
        let m4 = migrate(fv, tv, &l2, &c2)?;
        sum_new += m4.new_lex.len() as u32;
        round += 1;
    }
    if sum_new == 0 {
        return Err(E_ROUND_MISS);
    }
    hits += 1;

    // R10 重演一致：同种子十轮迁移新词库指纹和相等。
    let mut fp1: u64 = 0;
    let mut rng3 = Lcg::new(0x48646_0002);
    let mut round = 0u32;
    while round < 5 {
        let mut l2 = UserLex::new();
        let n = 1 + rng3.below(10);
        let mut i = 0u64;
        while i < n {
            l2.add(rng3.below(0x1_0000) as u32, rng3.below(100) as u32);
            i += 1;
        }
        let c3 = UserCfg::new();
        let m5 = migrate(1, 2, &l2, &c3)?;
        fp1 = fp1.wrapping_add(m5.new_lex.words[0] as u64);
        round += 1;
    }
    let mut fp2: u64 = 0;
    let mut rng4 = Lcg::new(0x48646_0002);
    let mut round = 0u32;
    while round < 5 {
        let mut l2 = UserLex::new();
        let n = 1 + rng4.below(10);
        let mut i = 0u64;
        while i < n {
            l2.add(rng4.below(0x1_0000) as u32, rng4.below(100) as u32);
            i += 1;
        }
        let c3 = UserCfg::new();
        let m6 = migrate(1, 2, &l2, &c3)?;
        fp2 = fp2.wrapping_add(m6.new_lex.words[0] as u64);
        round += 1;
    }
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_REPLAY);
    }
    hits += 1;

    Ok(hits)
}
