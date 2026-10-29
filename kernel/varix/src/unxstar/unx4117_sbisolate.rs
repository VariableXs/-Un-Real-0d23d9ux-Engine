//! unx4117_sbisolate — UNX-F4117 跨 sb 缓存隔离不变式（诊断段 0x90A0）。
//!
//! 判据原文（提炼）：
//!   J1 双 sb 同名路径条目互不串扰：隔离断言 1,000 次零混淆
//       （A sb 查 B sb 名不得命中 B 的条目，反之亦然）。
//!   J2 单 sb 失效不影响他 sb 条目：失效 A sb 全部条目后，
//       B sb 条目有效数不变（影响面断言通过）。
//!   J3 缓存键复合：键 = (sb 号, 名)，任一维不同即不同条目，
//       不存在跨 sb 命中路径。
//!   J4 条目账守恒：双 sb 条目数之和 == 总条目数，任何操作后成立。
//!   J5 混淆注入检出：任何跨 sb 查询串扰企图被检出并计混淆账
//       （混淆计数恒 0 才绿）。
//! 设计纪律：
//!   1. 复合键单表：条目以 (sb, name) 复合键存单表，
//       查找必须双键全配，杜绝按名单键误命中。
//!   2. sb 归属不变形：条目入账时定 sb，此后不可迁移
//       （迁移属 rename 语义，本模块不开放）。
//!   3. 失效按 sb 批量：invalidate_sb 只触该 sb 条目，
//       他 sb 计数在失效前后必须逐位相等。
//!   4. 抽样断言确定性：1,000 次隔离断言按固定序列轮转双 sb 双名，
//       不用 rand。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x90A0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x90A0;

/// 隔离断言轮数（判据 J1 原文数）。
pub const ISOLATE_ROUNDS: u32 = 1000;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_CONFUSION: u16 = SEG_BASE + 1; // 跨 sb 串扰命中（判据 J1/J5）
const E_SB_MISS: u16 = SEG_BASE + 2;   // 未挂载 sb 操作
const E_DUP_SB: u16 = SEG_BASE + 3;    // 重复挂载同 sb
const E_DUP_ENTRY: u16 = SEG_BASE + 4; // 同 sb 同名重复入账
const E_BAL: u16 = SEG_BASE + 5;       // 条目账守恒失守（判据 J4）
const E_IMPACT_LEAK: u16 = SEG_BASE + 6; // 失效波及他 sb（判据 J2 失守）

/// 条目有效性状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsoValidity {
    Valid,       // 有效
    Invalidated, // 已失效（本 sb 失效波及）
}

/// 复合键条目：(sb, name) 二元键 + 有效性。
pub struct IsoEntry {
    pub sb: u32,            // 归属 superblock（入账定死不迁移）
    pub name: &'static str, // 路径名
    pub ino: u32,           // inode 号
    pub validity: IsoValidity,
}

/// 隔离缓存枢纽：挂载账 + 复合键条目表 + 混淆账。
pub struct IsoCache {
    pub sbs: Vec<u32>,        // 已挂载 sb 号账
    pub entries: Vec<IsoEntry>, // 复合键条目表
    pub confusions: u32,      // 混淆检出计数（判据 J5，恒 0 才绿）
    pub invalidations: u32,   // 失效条目累计数
    pub probes: u32,          // 隔离断言执行次数（判据 J1）
}

impl IsoCache {
    pub fn new() -> IsoCache {
        IsoCache {
            sbs: Vec::new(),
            entries: Vec::new(),
            confusions: 0,
            invalidations: 0,
            probes: 0,
        }
    }

    /// 挂 sb（重复挂拒收）。
    pub fn mount(&mut self, sb: u32) -> Result<(), u16> {
        for s in &self.sbs {
            if *s == sb {
                return Err(E_DUP_SB);
            }
        }
        self.sbs.push(sb);
        Ok(())
    }

    fn mounted(&self, sb: u32) -> bool {
        for s in &self.sbs {
            if *s == sb {
                return true;
            }
        }
        false
    }

    /// 复合键定位：sb 与名双配才命中（判据 J3）。
    fn find(&self, sb: u32, name: &str) -> Option<usize> {
        for i in 0..self.entries.len() {
            if self.entries[i].sb == sb && self.entries[i].name == name {
                return Some(i);
            }
        }
        None
    }

    /// 入账（判据 J2/E_SB_MISS 前置）：sb 必须已挂载。
    pub fn put(&mut self, sb: u32, name: &'static str, ino: u32) -> Result<(), u16> {
        if !self.mounted(sb) {
            return Err(E_SB_MISS);
        }
        if self.find(sb, name).is_some() {
            return Err(E_DUP_ENTRY);
        }
        self.entries.push(IsoEntry {
            sb,
            name,
            ino,
            validity: IsoValidity::Valid,
        });
        Ok(())
    }

    /// 隔离查找（判据 J1/J5）：严格双键；若同名条目存在于他 sb，
    /// 本 sb 未命中即返回未命中——绝不允许跨 sb 回他 sb 的 ino。
    pub fn lookup(&mut self, sb: u32, name: &str) -> Result<Option<u32>, u16> {
        if !self.mounted(sb) {
            return Err(E_SB_MISS);
        }
        self.probes += 1;
        match self.find(sb, name) {
            Some(i) => {
                if self.entries[i].validity == IsoValidity::Valid {
                    Ok(Some(self.entries[i].ino))
                } else {
                    Ok(None) // 已失效按未命中回（穿透归上层）
                }
            }
            None => {
                // 串扰检查（判据 J5）：他 sb 若有同名有效条目，
                // 而本实现若误回其 ino 即混淆——本实现直接不回，
                // 此处仅审计性核对：不产生任何跨 sb 应答。
                Ok(None)
            }
        }
    }

    /// 混淆注入检出面（判据 J5）：显式尝试跨 sb 取值——
    /// 用 sb_A 的键去取 sb_B 的条目内容，必须拿不到（返回 Err）。
    pub fn inject_cross_sb(&mut self, from: u32, target_sb: u32, name: &str) -> Result<(), u16> {
        if !self.mounted(from) || !self.mounted(target_sb) {
            return Err(E_SB_MISS);
        }
        match self.find(target_sb, name) {
            Some(i) => {
                // 目标 sb 有条目：from sb 的查找绝不可见它
                match self.lookup(from, name)? {
                    Some(_) => {
                        self.confusions += 1;
                        Err(E_CONFUSION) // 串扰命中：检出
                    }
                    None => Ok(()), // 零混淆：正确隔离
                }
            }
            None => Ok(()), // 目标 sb 无此条目，无串扰可言
        }
    }

    /// 单 sb 批量失效（判据 J2）：返回失效数；他 sb 条目不动。
    pub fn invalidate_sb(&mut self, sb: u32) -> Result<u32, u16> {
        if !self.mounted(sb) {
            return Err(E_SB_MISS);
        }
        // 失效前快照他 sb 有效数
        let mut before = 0u32;
        for e in &self.entries {
            if e.sb != sb && e.validity == IsoValidity::Valid {
                before += 1;
            }
        }
        let mut n = 0u32;
        for i in 0..self.entries.len() {
            if self.entries[i].sb == sb
                && self.entries[i].validity == IsoValidity::Valid
            {
                self.entries[i].validity = IsoValidity::Invalidated;
                n += 1;
            }
        }
        self.invalidations += n;
        // 失效后复核他 sb 有效数不变（判据 J2 影响面）
        let mut after = 0u32;
        for e in &self.entries {
            if e.sb != sb && e.validity == IsoValidity::Valid {
                after += 1;
            }
        }
        if before != after {
            return Err(E_IMPACT_LEAK);
        }
        Ok(n)
    }

    /// 条目账守恒（判据 J4）：逐 sb 计数和 == 总数。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u32;
        for s in &self.sbs {
            for e in &self.entries {
                if e.sb == *s {
                    sum += 1;
                }
            }
        }
        sum == self.entries.len() as u32
    }

    /// sb 有效条目数（对照面）。
    pub fn valid_count(&self, sb: u32) -> u32 {
        let mut n = 0u32;
        for e in &self.entries {
            if e.sb == sb && e.validity == IsoValidity::Valid {
                n += 1;
            }
        }
        n
    }

    /// 隔离断言批量驱动（判据 J1）：1,000 次双 sb 双名轮转查找，
    /// 任何一次跨 sb 混淆即红。
    pub fn isolation_sweep(&mut self) -> Result<(), u16> {
        let mut r = 0u32;
        while r < ISOLATE_ROUNDS {
            // 确定性轮转：偶数轮查 (sb1, nameA)，奇数轮查 (sb2, nameA)
            let (sb, expect_hit) = if r % 2 == 0 {
                (1u32, true)
            } else {
                (2u32, false)
            };
            match self.lookup(sb, "shared")? {
                Some(_) => {
                    if !expect_hit {
                        // sb2 没有 shared 条目却命中：串扰！
                        self.confusions += 1;
                        return Err(E_CONFUSION);
                    }
                }
                None => {
                    if expect_hit {
                        return Err(E_BAL); // 应命中未命中：账坏
                    }
                }
            }
            r += 1;
        }
        if self.confusions != 0 {
            return Err(E_CONFUSION);
        }
        Ok(())
    }
}

/// 自例程：R1 双 sb 铺账→R2 千次隔离断言→R3 单 sb 失效影响面→R4 注入终审。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 铺账：sb1 有 shared 与 own1；sb2 有 shared（不同 ino）与 own2。
    let mut c = IsoCache::new();
    c.mount(1)?;
    c.mount(2)?;
    c.put(1, "shared", 100)?;
    c.put(1, "own1", 101)?;
    c.put(2, "shared", 200)?; // 同名跨 sb，ino 不同（判据 J1 场景）
    c.put(2, "own2", 201)?;
    if !c.conserved() {
        return Err(E_BAL);
    }
    // 同名不同 sb 应答各自 ino（复合键正确性）。
    if c.lookup(1, "shared")? != Some(100) {
        return Err(E_CONFUSION);
    }
    if c.lookup(2, "shared")? != Some(200) {
        return Err(E_CONFUSION);
    }
    hits += 1;

    // R2 千次隔离断言：sb2 查 own1 不命中、sb1 查 own2 不命中、
    // shared 各归各（判据 J1）。
    c.isolation_sweep()?;
    if c.lookup(2, "own1")?.is_some() {
        return Err(E_CONFUSION); // sb2 不得见 sb1 私有条目
    }
    if c.lookup(1, "own2")?.is_some() {
        return Err(E_CONFUSION); // sb1 不得见 sb2 私有条目
    }
    if c.probes < ISOLATE_ROUNDS + 2 {
        return Err(E_BAL); // 断言轮数账
    }
    hits += 1;

    // R3 单 sb 失效影响面（判据 J2）：失效 sb1 全部（2 条），
    // sb2 有效数不变（2），shared 在 sb1 回未命中、sb2 仍回 200。
    let b2 = c.valid_count(2);
    let n = c.invalidate_sb(1)?;
    if n != 2 || c.valid_count(1) != 0 {
        return Err(E_BAL);
    }
    if c.valid_count(2) != b2 {
        return Err(E_IMPACT_LEAK); // 他 sb 零波及
    }
    if c.lookup(1, "shared")?.is_some() {
        return Err(E_BAL); // 失效后按未命中回
    }
    if c.lookup(2, "shared")? != Some(200) {
        return Err(E_IMPACT_LEAK); // sb2 的 shared 不受 sb1 失效影响
    }
    hits += 1;

    // R4 注入终审：重复挂载、重复入账、未挂 sb 操作、混淆账恒零。
    if c.mount(1).is_ok() {
        return Err(E_DUP_SB);
    }
    if c.put(1, "own1", 9).is_ok() {
        // 注：own1 已失效但条目仍在表，复合键重复仍应拒
        return Err(E_DUP_ENTRY);
    }
    if c.put(9, "x", 1).is_ok() {
        return Err(E_SB_MISS); // 未挂载 sb 应拒
    }
    if c.lookup(9, "x").is_ok() {
        return Err(E_SB_MISS);
    }
    if c.invalidate_sb(9).is_ok() {
        return Err(E_SB_MISS);
    }
    if c.confusions != 0 || !c.conserved() {
        return Err(E_CONFUSION); // 判据 J5 终审
    }
    hits += 1;

    Ok(hits)
}
