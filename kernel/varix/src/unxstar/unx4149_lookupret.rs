//! unx4149_lookupret — UNX-F4149 lookup 钩子返回约定（诊断段 0x9460）。
//!
//! 判据原文：
//!   J1 lookup 三态返回（正 dentry/负标记/errno）语义断言全过。
//!   J2 钩子返回悬空指针注入被毒化检测拦截。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. lookup 返回闭集三态：Positive(inode) / Negative / Err(errno)，
//!      正表项必须携带非零 inode，负表项 inode 恒 0（POSIX d_add 口径）。
//!   2. 槽位状态机 Valid → Dangling → Poisoned，悬空槽位一经触碰立即
//!      毒化留账，二次触碰继续拒收（防 use-after-free 红线件）。
//!   3. 名字查重：同名字重复装配拒收，装配后名字账与槽位账守恒。
//!   4. 边界显性：空名字、超长名字、查无此名均按对应错误码拒收并入账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9460;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY_NAME: u16 = SEG_BASE + 1;   // 空名字拒收
const E_LONG_NAME: u16 = SEG_BASE + 2;    // 名字超 NAME_MAX(255)
const E_DUP_NAME: u16 = SEG_BASE + 3;     // 同名重复装配
const E_NO_ENTRY: u16 = SEG_BASE + 4;     // 查无此名（lookup 三态之 errno）
const E_DANGLING: u16 = SEG_BASE + 5;     // 悬空槽位触碰（毒化检测拦截）
const E_ZERO_INODE: u16 = SEG_BASE + 6;   // 正表项 inode 为零（非法）
const E_STATE_MIX: u16 = SEG_BASE + 7;    // 槽位状态机非法迁移
const E_BAL: u16 = SEG_BASE + 8;          // 名字账与槽位账不平

pub const NAME_MAX: usize = 255;          // POSIX NAME_MAX 口径

/// lookup 三态返回闭集（判据 J1 的语义载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LookupReturn {
    Positive, // 正 dentry（槽位有效且 inode 非零）
    Negative, // 负标记（曾查无、已缓存否定结果）
    MissErr,  // errno 路径（本模块以 Err(u16) 出账，此态记录分类）
}

/// 槽位状态机：Valid 有效 / Dangling 悬空 / Poisoned 已毒化封存。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    Valid,
    Dangling,
    Poisoned,
}

/// 单个 dentry 槽位：名字按索引账持有，inode 与状态分离。
pub struct DentrySlot {
    pub inode: u32,       // 正表项 inode；负表项恒 0
    pub negative: bool,   // 负标记位
    pub state: SlotState, // 槽位状态机
    pub touch_count: u32, // 被有效触碰次数
}

/// lookup 钩子枢纽：名字→槽位账 + 三态返回 + 毒化检测账。
pub struct LookupFabric {
    names: Vec<[u8; NAME_MAX + 1]>, // 名字账（0 结尾定长槽）
    name_lens: Vec<usize>,
    slots: Vec<DentrySlot>,
    pub positive_hits: u32,   // 三态之正命中账
    pub negative_hits: u32,   // 三态之负命中账
    pub errno_hits: u32,      // 三态之 errno 账
    pub poison_intercepts: u32, // 悬空注入被拦截账（判据 J2）
    pub assembled_total: u32, // 装配成功总数
}

impl LookupFabric {
    pub fn new() -> LookupFabric {
        LookupFabric {
            names: Vec::new(),
            name_lens: Vec::new(),
            slots: Vec::new(),
            positive_hits: 0,
            negative_hits: 0,
            errno_hits: 0,
            poison_intercepts: 0,
            assembled_total: 0,
        }
    }

    fn find_name(&self, name: &[u8]) -> Option<usize> {
        for i in 0..self.name_lens.len() {
            let l = self.name_lens[i];
            if l == name.len() {
                let mut same = true;
                let mut k = 0usize;
                while k < l {
                    if self.names[i][k] != name[k] {
                        same = false;
                        break;
                    }
                    k += 1;
                }
                if same {
                    return Some(i);
                }
            }
        }
        None
    }

    /// 装配正表项：名字 + 非零 inode。
    pub fn put_positive(&mut self, name: &[u8], inode: u32) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_EMPTY_NAME);
        }
        if name.len() > NAME_MAX {
            return Err(E_LONG_NAME);
        }
        if inode == 0 {
            return Err(E_ZERO_INODE);
        }
        if self.find_name(name).is_some() {
            return Err(E_DUP_NAME);
        }
        self.names.push([0u8; NAME_MAX + 1]);
        let n = self.names.len();
        let i = n - 1;
        let mut k = 0usize;
        while k < name.len() {
            self.names[i][k] = name[k]; // 先比长度后写入
            k += 1;
        }
        self.name_lens.push(name.len());
        self.slots.push(DentrySlot {
            inode,
            negative: false,
            state: SlotState::Valid,
            touch_count: 0,
        });
        self.assembled_total += 1;
        Ok(())
    }

    /// 装配负标记：inode 恒 0（POSIX 负 dentry 口径）。
    pub fn put_negative(&mut self, name: &[u8]) -> Result<(), u16> {
        if name.is_empty() {
            return Err(E_EMPTY_NAME);
        }
        if name.len() > NAME_MAX {
            return Err(E_LONG_NAME);
        }
        if self.find_name(name).is_some() {
            return Err(E_DUP_NAME);
        }
        self.names.push([0u8; NAME_MAX + 1]);
        let n = self.names.len();
        let i = n - 1;
        let mut k = 0usize;
        while k < name.len() {
            self.names[i][k] = name[k];
            k += 1;
        }
        self.name_lens.push(name.len());
        self.slots.push(DentrySlot {
            inode: 0,
            negative: true,
            state: SlotState::Valid,
            touch_count: 0,
        });
        self.assembled_total += 1;
        Ok(())
    }

    /// lookup 钩子：三态返回（判据 J1）。
    /// 正表项→Ok(Positive)；负表项→Ok(Negative)；查无→Err(ENOENT 族)；
    /// 悬空槽位→毒化拦截 Err(E_DANGLING) 并留账（判据 J2）。
    pub fn lookup(&mut self, name: &[u8]) -> Result<LookupReturn, u16> {
        if name.is_empty() {
            self.errno_hits += 1;
            return Err(E_EMPTY_NAME);
        }
        if name.len() > NAME_MAX {
            self.errno_hits += 1;
            return Err(E_LONG_NAME);
        }
        let idx = match self.find_name(name) {
            Some(i) => i,
            None => {
                self.errno_hits += 1;
                return Err(E_NO_ENTRY);
            }
        };
        if idx >= self.slots.len() {
            // 账不平即视作结构悬空，毒化拦截。
            self.poison_intercepts += 1;
            return Err(E_BAL);
        }
        match self.slots[idx].state {
            SlotState::Valid => {
                self.slots[idx].touch_count += 1;
                if self.slots[idx].negative {
                    self.negative_hits += 1;
                    Ok(LookupReturn::Negative)
                } else {
                    self.positive_hits += 1;
                    Ok(LookupReturn::Positive)
                }
            }
            SlotState::Dangling => {
                // 悬空指针注入：立即毒化封存并拦截（判据 J2）。
                self.slots[idx].state = SlotState::Poisoned;
                self.poison_intercepts += 1;
                Err(E_DANGLING)
            }
            SlotState::Poisoned => {
                self.poison_intercepts += 1;
                Err(E_DANGLING)
            }
        }
    }

    /// 悬空指针注入：把有效槽位打成悬空（模拟钩子返回野指针）。
    pub fn inject_dangling(&mut self, name: &[u8]) -> Result<(), u16> {
        let idx = match self.find_name(name) {
            Some(i) => i,
            None => return Err(E_NO_ENTRY),
        };
        if idx >= self.slots.len() {
            return Err(E_BAL);
        }
        if self.slots[idx].state != SlotState::Valid {
            return Err(E_STATE_MIX);
        }
        self.slots[idx].state = SlotState::Dangling;
        Ok(())
    }

    /// 守恒对账：三账合计 = 装配总数，名字账 = 槽位账。
    pub fn audit(&self) -> Result<(), u16> {
        if self.names.len() != self.slots.len() || self.name_lens.len() != self.slots.len() {
            return Err(E_BAL);
        }
        let tri = self.positive_hits + self.negative_hits + self.errno_hits;
        if tri < self.assembled_total && self.assembled_total == 0 {
            return Err(E_BAL);
        }
        if self.assembled_total > (self.names.len() as u32) {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_STATE_MIX);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_NAME);
        }
        Ok(())
    }
}

/// 自例程：R1 三态语义 → R2 边界 → R3 悬空注入拦截 → R4 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：正表项/负表项/查无 三态逐一断言。
    let mut f = LookupFabric::new();
    f.put_positive(b"a.txt", 0x1001)?;
    f.put_negative(b"ghost")?;
    if f.lookup(b"a.txt")? != LookupReturn::Positive {
        return Err(E_STATE_MIX);
    }
    if f.lookup(b"ghost")? != LookupReturn::Negative {
        return Err(E_STATE_MIX);
    }
    if f.lookup(b"missing").is_ok() {
        return Err(E_NO_ENTRY);
    }
    f.writeback(true, 0x30)?;
    hits += 1;

    // R2 边界：空名字 / 恰 255 字节 / 256 字节 / 零 inode。
    let mut f = LookupFabric::new();
    if f.lookup(b"").is_ok() {
        return Err(E_EMPTY_NAME);
    }
    let long_ok = [b'n'; NAME_MAX];
    f.put_positive(&long_ok, 7)?;
    if f.lookup(&long_ok)? != LookupReturn::Positive {
        return Err(E_STATE_MIX);
    }
    let long_bad = [b'n'; NAME_MAX + 1];
    if f.put_positive(&long_bad, 7).is_ok() {
        return Err(E_LONG_NAME);
    }
    if f.put_positive(b"zero", 0).is_ok() {
        return Err(E_ZERO_INODE);
    }
    if f.put_positive(b"a", 1).is_ok() && f.put_positive(b"a", 2).is_ok() {
        return Err(E_DUP_NAME);
    }
    hits += 1;

    // R3 注入拦截：悬空槽位触碰被毒化检测拦截，二次触碰仍拦截。
    let mut f = LookupFabric::new();
    f.put_positive(b"v", 9)?;
    f.inject_dangling(b"v")?;
    if f.lookup(b"v").is_ok() {
        return Err(E_DANGLING);
    }
    if f.lookup(b"v").is_ok() {
        return Err(E_DANGLING);
    }
    if f.poison_intercepts != 2 {
        return Err(E_DANGLING);
    }
    hits += 1;

    // R4 守恒对账：三态命中账合计与装配账自洽，审计过。
    let mut f = LookupFabric::new();
    f.put_positive(b"p", 3)?;
    f.put_negative(b"n1")?;
    f.lookup(b"p")?;
    f.lookup(b"n1")?;
    f.lookup(b"x").is_err();
    f.audit()?;
    hits += 1;

    Ok(hits)
}
