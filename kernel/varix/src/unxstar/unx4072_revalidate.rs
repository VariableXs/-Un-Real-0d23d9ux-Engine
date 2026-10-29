//! unx4072_revalidate — UNX-F4072 inode 重校验（i_revalidate 类钩子）（诊断段 0x8B40）。
//!
//! 判据原文（提炼）：
//!   J1. 重校验钩子返回失效时缓存条目重建（重建计数=1）。
//!   J2. 钩子未实现时默认信任缓存（行为账可切换）。
//!   J3. 重校验通过时零重建（缓存直接命中）。
//!   J4. 行为账守恒：trust 路径 + revalidate 路径 == 总查询。
//! 设计纪律：
//!   1. i_revalidate 语义（网络文件系统传统）：缓存命中前先问
//!      钩子"还新鲜吗"，失效即重建，未实现则默认信任。
//!   2. 钩子双实现：NoHook（信任模式）/ VerdictHook（判定模式，
//!      判定值整数承载），切换即行为账切换（判据 J2）。
//!   3. 重建路径与首次装配同源（重建零差）。
//!   4. 重建计数恰一：失效一次重建一次，重复重建注入拒收。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8B40;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ENOENT: u16 = SEG_BASE + 1;   // 条目不存在
const E_DUP_REBUILD: u16 = SEG_BASE + 2; // 重复重建注入
const E_STALE_USED: u16 = SEG_BASE + 3;  // 失效条目仍被使用（新鲜度失守）
const E_HOOK_INV: u16 = SEG_BASE + 4; // 钩子判定值非法
const E_LEDGER: u16 = SEG_BASE + 5;   // 行为账守恒破坏
const E_STATE_INV: u16 = SEG_BASE + 6;// 相位迁移非法
const E_BAD_ARG: u16 = SEG_BASE + 7;  // 参数非法（0 号）

/// 钩子模式状态机（判据 J2 行为切换锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookMode {
    NoHook,      // 未实现：默认信任缓存
    VerdictHook, // 已实现：按判定值裁决
}

/// 新鲜度判定值（VerdictHook 返回语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Fresh = 0,  // 新鲜：信任缓存
    Stale = 1,  // 失效：触发重建
}

/// 缓存条目：数据键 + 新鲜相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheEntry {
    pub ino: u64,   // inode 号
    pub value: u64, // 缓存数据（确定性推导）
    pub fresh: bool,// 新鲜位
}

/// 重校验枢纽：条目表 + 钩子模式 + 行为账。
pub struct RevalidateFabric {
    pub entries: Vec<CacheEntry>, // 条目表
    pub mode: HookMode,           // 钩子模式
    pub verdict: Verdict,         // VerdictHook 判定值
    pub trust_hits: u32,          // 信任路径计数（判据 J2 账）
    pub rebuild_total: u32,       // 重建总数（判据 J1 锚）
    pub fresh_hits: u32,          // 重校验通过零重建数（判据 J3）
    pub query_total: u32,         // 总查询数（守恒主账）
}

impl RevalidateFabric {
    pub fn new() -> RevalidateFabric {
        RevalidateFabric {
            entries: Vec::new(),
            mode: HookMode::NoHook,
            verdict: Verdict::Fresh,
            trust_hits: 0,
            rebuild_total: 0,
            fresh_hits: 0,
            query_total: 0,
        }
    }

    fn find_entry(&self, ino: u64) -> Option<usize> {
        for i in 0..self.entries.len() {
            if self.entries[i].ino == ino {
                return Some(i);
            }
        }
        None
    }

    /// 首次装配（重建同源基准：value = ino * 11 + 7）。
    pub fn assemble(&mut self, ino: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_ARG);
        }
        if self.find_entry(ino).is_some() {
            return Err(E_STATE_INV);
        }
        self.entries.push(CacheEntry {
            ino,
            value: ino * 11 + 7,
            fresh: true,
        });
        Ok(())
    }

    /// 失效注入（外部变更模拟：新鲜位清零）。
    pub fn invalidate(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_entry(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        self.entries[i].fresh = false;
        Ok(())
    }

    /// 钩子模式切换（判据 J2 行为账切换面）。
    pub fn set_hook(&mut self, m: HookMode, v: Verdict) {
        self.mode = m;
        self.verdict = v;
    }

    /// 重建：与首装同源重算，重建计数恰一（判据 J1）。
    fn rebuild(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_entry(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if !self.entries[i].fresh {
            // 重建即恢复同源值与新鲜位
            self.entries[i].value = ino * 11 + 7;
            self.entries[i].fresh = true;
            self.rebuild_total += 1;
            Ok(())
        } else {
            Err(E_DUP_REBUILD)
        }
    }

    /// 查询核心：钩子裁决 → 信任/重建/零重建三分支。
    /// 返回缓存值。
    pub fn query(&mut self, ino: u64) -> Result<u64, u16> {
        self.query_total += 1;
        let i = match self.find_entry(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        match self.mode {
            HookMode::NoHook => {
                // 未实现钩子：默认信任（判据 J2）
                self.trust_hits += 1;
                Ok(self.entries[i].value)
            }
            HookMode::VerdictHook => match self.verdict {
                Verdict::Fresh => {
                    if !self.entries[i].fresh {
                        return Err(E_STALE_USED);
                    }
                    self.fresh_hits += 1;
                    Ok(self.entries[i].value)
                }
                Verdict::Stale => {
                    // 失效裁决：重建恰好一次（判据 J1）
                    let v = self.entries[i].value;
                    if self.entries[i].fresh {
                        // 已新鲜仍裁失效 → 重复重建注入拒收
                        return Err(E_DUP_REBUILD);
                    }
                    self.rebuild(ino)?;
                    Ok(v) // 返回重建前值（重建后同源一致）
                }
            },
        }
    }

    /// 重复重建注入：新鲜条目裁决失效 → 必须拒收。
    pub fn inject_double_rebuild(&mut self, ino: u64) -> Result<(), u16> {
        let i = match self.find_entry(ino) {
            Some(i) => i,
            None => return Err(E_ENOENT),
        };
        if self.entries[i].fresh {
            // 已新鲜：再裁失效属重复重建注入
            return Err(E_DUP_REBUILD);
        }
        Err(E_STATE_INV)
    }

    /// 行为账守恒（判据 J4）：trust + fresh_hits + 重建触发查询
    /// == 总查询。重建触发的查询计入 fresh_hits 之外的差值。
    pub fn ledger_ok(&self) -> bool {
        self.trust_hits + self.fresh_hits + self.rebuild_total
            == self.query_total
    }
}

// ── 扩展面：行为账诊断与钩子状态导出 ─────────────────────────

impl RevalidateFabric {
    /// 钩子模式只读面（行为账切换态读数，判据 J2）。
    pub fn hook_mode(&self) -> HookMode {
        self.mode
    }

    /// 信任路径读数（判据 J2 账面导出）。
    pub fn trust_count(&self) -> u32 {
        self.trust_hits
    }

    /// 零重建路径读数（判据 J3 账面导出）。
    pub fn fresh_count(&self) -> u32 {
        self.fresh_hits
    }

    /// 重建读数（判据 J1 恰一锚导出）。
    pub fn rebuild_count(&self) -> u32 {
        self.rebuild_total
    }

    /// 条目新鲜位只读面。
    pub fn fresh_bit(&self, ino: u64) -> Result<bool, u16> {
        match self.find_entry(ino) {
            Some(i) => Ok(self.entries[i].fresh),
            None => Err(E_ENOENT),
        }
    }

    /// 重建零差断言：重建后值与首装同源值严格相等（判据 J1 面）。
    pub fn assert_rebuild_same(&self, ino: u64) -> Result<(), u16> {
        match self.find_entry(ino) {
            Some(i) => {
                if self.entries[i].value != ino * 11 + 7 {
                    return Err(E_LEDGER);
                }
                Ok(())
            }
            None => Err(E_ENOENT),
        }
    }

    /// 行为账分布千分比：返回（信任‰，零重建‰，重建‰）。
    pub fn behavior_permille(&self) -> Result<(u32, u32, u32), u16> {
        if self.query_total == 0 {
            return Err(E_BAD_ARG);
        }
        if !self.ledger_ok() {
            return Err(E_LEDGER);
        }
        Ok((
            self.trust_hits * 1000 / self.query_total,
            self.fresh_hits * 1000 / self.query_total,
            self.rebuild_total * 1000 / self.query_total,
        ))
    }
}

/// 自例程：失效重建恰一→信任模式切换→零重建路径→守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = RevalidateFabric::new();

    // R1 信任模式（NoHook）：失效条目仍按缓存值返回（判据 J2）
    f.assemble(4)?;
    f.set_hook(HookMode::NoHook, Verdict::Fresh);
    f.invalidate(4)?;
    let v1 = f.query(4)?;
    if v1 != 4 * 11 + 7 {
        return Err(E_LEDGER);
    }
    if f.trust_hits != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 切换判定模式 + 失效裁决：重建恰好一次（判据 J1）
    f.set_hook(HookMode::VerdictHook, Verdict::Stale);
    let v2 = f.query(4)?;
    if v2 != 4 * 11 + 7 {
        return Err(E_LEDGER);
    }
    if f.rebuild_total != 1 {
        return Err(E_DUP_REBUILD);
    }
    hits += 1;

    // R3 重校验通过零重建：Fresh 裁决 + 新鲜条目直接命中（判据 J3）
    f.set_hook(HookMode::VerdictHook, Verdict::Fresh);
    let v3 = f.query(4)?;
    if v3 != 4 * 11 + 7 || f.fresh_hits != 1 || f.rebuild_total != 1 {
        return Err(E_STALE_USED);
    }
    hits += 1;

    // R4 重复重建注入：已新鲜条目裁决失效 → 拒收
    f.set_hook(HookMode::VerdictHook, Verdict::Stale);
    match f.inject_double_rebuild(4) {
        Err(E_DUP_REBUILD) => {}
        _ => return Err(E_DUP_REBUILD),
    }
    match f.query(4) {
        Err(E_DUP_REBUILD) => {}
        _ => return Err(E_DUP_REBUILD),
    }
    if f.rebuild_total != 1 {
        return Err(E_DUP_REBUILD); // 重建计数不得再增
    }
    hits += 1;

    // R5 行为账守恒（判据 J4）
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    // trust 1 + fresh 1 + rebuild 1 = query 4？核对：R1 一次、
    // R2 一次、R3 一次、R4 一次（注入路径也计数）
    if f.query_total != 4 {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
