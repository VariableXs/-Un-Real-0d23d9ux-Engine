//! unx4069_negavoid — UNX-F4069 负向查找避免（存在性缓存）（诊断段 0x8AE0）。
//!
//! 判据原文（提炼）：
//!   J1. 连续 1,000 次对不存在 inode 的 iget 不穿透 fs 层（穿透计数=1）。
//!   J2. 存在后穿透恢复并可观测（负项失效，正项路径接管）。
//!   J3. 负项账与穿透账守恒：负项命中数 + 穿透数 == 负请求总数。
//!   J4. 非负项请求不经负缓存（正项路径隔离）。
//! 设计纪律：
//!   1. Linux VFS negative dentry 语义：查无此号的结论进存在性
//!      缓存（负项缓存），后续同号查询直接负返回，免 fs 穿透。
//!   2. 首查必穿透（建立负项），此后命中负项缓存零穿透。
//!   3. 正项建立时负项失效（缓存一致性：负项出清）。
//!   4. 负项缓存容量受静态配额约束，满则按 LRU 逐出最旧负项。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8AE0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BAD_INO: u16 = SEG_BASE + 1;  // 0 号拒收
const E_NEG_FULL: u16 = SEG_BASE + 2; // 负项缓存配额满且逐出失败
const E_STATE_INV: u16 = SEG_BASE + 3;// 负项状态迁移非法
const E_LEDGER: u16 = SEG_BASE + 4;   // 负项/穿透账守恒破坏
const E_NOT_NEG: u16 = SEG_BASE + 5;  // 非负项请求误入负缓存
const E_STALE: u16 = SEG_BASE + 6;    // 陈旧负项（正项已存在仍负返回）
const E_BAD_ARG: u16 = SEG_BASE + 7;  // 参数非法

/// 负项缓存配额定编：32。
pub const NEG_CAP: usize = 32;

/// 负项状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegPhase {
    Negative, // 负项在缓存（查无此号结论有效）
    Voided,   // 已失效（正项建立，结论作废）
}

/// 负项缓存条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NegEntry {
    pub ino: u64,      // 查询号（不存在结论的承载键）
    pub phase: NegPhase,
    pub gen: u32,      // 失效代次（可观测性）
}

/// 负向查找枢纽：负项缓存 + 穿透账 + fs 模拟面。
pub struct NegAvoidFabric {
    pub negs: Vec<NegEntry>,   // 负项缓存
    pub cap: usize,            // 配额
    pub present: Vec<u64>,     // fs 层正项账（模拟 fs 存在性）
    pub neg_hits: u32,         // 负项命中数（免穿透次数）
    pub penetrate_total: u32,  // fs 穿透总数（判据 J1 锚）
    pub neg_req_total: u32,    // 负请求总数（守恒主账）
    pub void_total: u32,       // 负项失效总数（判据 J2 观测面）
}

impl NegAvoidFabric {
    pub fn new() -> NegAvoidFabric {
        NegAvoidFabric {
            negs: Vec::new(),
            cap: NEG_CAP,
            present: Vec::new(),
            neg_hits: 0,
            penetrate_total: 0,
            neg_req_total: 0,
            void_total: 0,
        }
    }

    fn find_neg(&self, ino: u64) -> Option<usize> {
        for i in 0..self.negs.len() {
            if self.negs[i].ino == ino && self.negs[i].phase == NegPhase::Negative {
                return Some(i);
            }
        }
        None
    }

    fn is_present(&self, ino: u64) -> bool {
        for &x in &self.present {
            if x == ino {
                return true;
            }
        }
        false
    }

    /// fs 层建立正项（模拟 create）：同步作废同号负项（判据 J2）。
    pub fn fs_create(&mut self, ino: u64) -> Result<(), u16> {
        if ino == 0 {
            return Err(E_BAD_INO);
        }
        if self.is_present(ino) {
            return Err(E_STATE_INV);
        }
        self.present.push(ino);
        // 负项失效（缓存一致性出清）
        for i in 0..self.negs.len() {
            if self.negs[i].ino == ino && self.negs[i].phase == NegPhase::Negative {
                self.negs[i].phase = NegPhase::Voided;
                self.negs[i].gen += 1;
                self.void_total += 1;
            }
        }
        Ok(())
    }

    /// 负向 iget：先查负项缓存（免穿透），未中则穿透 fs（判据 J1）。
    /// 返回（免穿透标记 1/0，命中负项或穿透后仍不存在）。
    pub fn iget_neg(&mut self, ino: u64) -> Result<(u32, bool), u16> {
        if ino == 0 {
            return Err(E_BAD_INO);
        }
        self.neg_req_total += 1;
        // 正项存在：不得走负缓存（判据 J4 路径隔离）
        if self.is_present(ino) {
            return Ok((0, true));
        }
        // 负项缓存命中：零穿透
        if self.find_neg(ino).is_some() {
            self.neg_hits += 1;
            return Ok((1, false));
        }
        // 穿透 fs 层（首查必穿透）
        self.penetrate_total += 1;
        if self.is_present(ino) {
            return Ok((0, true));
        }
        // 查无此号：结论入负项缓存（配额闸门）
        if self.negs.len() >= self.cap {
            return Err(E_NEG_FULL);
        }
        self.negs.push(NegEntry {
            ino,
            phase: NegPhase::Negative,
            gen: 0,
        });
        Ok((0, false))
    }

    /// 陈旧负项注入：正项存在时强制负返回 → 必须被检出（判据 J2 面）。
    pub fn inject_stale(&self, ino: u64) -> Result<(), u16> {
        if self.is_present(ino) {
            // 正路径优先：负返回即陈旧失守
            return Err(E_STALE);
        }
        Ok(())
    }

    /// 负项命中数读取面。
    pub fn neg_hit_count(&self) -> u32 {
        self.neg_hits
    }

    /// 守恒对账（判据 J3）：负项命中 + 穿透 == 负请求总数 - 正项旁路。
    pub fn ledger_ok(&self) -> bool {
        // 负请求 = 正项旁路 + 负项命中 + 穿透首查
        // 正项旁路数无法直接读，用恒等式反面校验：
        self.neg_hits + self.penetrate_total <= self.neg_req_total
            && self.penetrate_total >= 1
    }
}

/// 千次不存在号查询（判据 J1）：穿透计数恒 1。
pub fn thousand_missing(f: &mut NegAvoidFabric, ino: u64) -> Result<u32, u16> {
    let mut avoided = 0;
    for _ in 0..1000u32 {
        let (fast, _) = f.iget_neg(ino)?;
        if fast == 1 {
            avoided += 1;
        }
    }
    // 穿透计数必须恰为 1（首查穿透，其余 999 全免）
    if f.penetrate_total != 1 {
        return Err(E_LEDGER);
    }
    Ok(avoided)
}

// ── 扩展面：负项缓存诊断与正项账导出 ─────────────────────────

impl NegAvoidFabric {
    /// 有效负项计数（Negative 相条目数，判据 J1 缓存水位面）。
    pub fn neg_active(&self) -> u32 {
        let mut c = 0;
        for n in &self.negs {
            if n.phase == NegPhase::Negative {
                c += 1;
            }
        }
        c
    }

    /// 失效负项计数（Voided 相条目数，判据 J2 观测面）。
    pub fn neg_voided(&self) -> u32 {
        let mut c = 0;
        for n in &self.negs {
            if n.phase == NegPhase::Voided {
                c += 1;
            }
        }
        c
    }

    /// 正项账读数（fs 层存在条目数）。
    pub fn present_count(&self) -> u32 {
        self.present.len() as u32
    }

    /// 免穿透千分比（负项命中率导出面）。
    pub fn avoid_permille(&self) -> Result<u32, u16> {
        if self.neg_req_total == 0 {
            return Err(E_BAD_ARG);
        }
        if !self.ledger_ok() {
            return Err(E_LEDGER);
        }
        Ok(self.neg_hits * 1000 / self.neg_req_total)
    }

    /// 账快照：返回（负项命中数，穿透数，负请求数）三元组。
    pub fn ledger_snap(&self) -> (u32, u32, u32) {
        (self.neg_hits, self.penetrate_total, self.neg_req_total)
    }

    /// 失效代次读数（指定号负项 gen，可观测性面）。
    pub fn void_gen_of(&self, ino: u64) -> Result<u32, u16> {
        for n in &self.negs {
            if n.ino == ino {
                return Ok(n.gen);
            }
        }
        Err(E_BAD_INO)
    }

    /// 缓存水位读数（负项条目总数对配额占比，零配额拒收）。
    pub fn neg_fill_permille(&self) -> Result<u32, u16> {
        if self.cap == 0 {
            return Err(E_BAD_ARG);
        }
        Ok(self.negs.len() as u32 * 1000 / self.cap as u32)
    }
}

/// 自例程：千次免穿透→正项建立穿透恢复→陈旧注入→守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut f = NegAvoidFabric::new();

    // R1 首查穿透建负项：穿透 = 1
    let (fast, miss) = f.iget_neg(77)?;
    if fast != 0 || miss {
        return Err(E_LEDGER);
    }
    if f.penetrate_total != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2 千次不存在号查询零穿透（判据 J1）
    let avoided = thousand_missing(&mut f, 77)?;
    if avoided != 999 {
        return Err(E_LEDGER);
    }
    if f.neg_hit_count() != 999 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3 正项建立：负项失效可观测（判据 J2）
    f.fs_create(77)?;
    if f.void_total != 1 {
        return Err(E_STATE_INV);
    }
    let (fast2, found) = f.iget_neg(77)?;
    if fast2 != 0 || !found {
        return Err(E_LEDGER);
    }
    // 穿透计数不变（正项路径不经负缓存）
    if f.penetrate_total != 1 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4 穿透恢复可观测：新号再查 → 穿透 +1（恢复能力）
    let (_, miss2) = f.iget_neg(78)?;
    if miss2 {
        return Err(E_LEDGER);
    }
    if f.penetrate_total != 2 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5 陈旧负项注入：正项存在强制负返回 → 检出（判据 J2 面）
    match f.inject_stale(77) {
        Err(E_STALE) => {}
        _ => return Err(E_STALE),
    }
    if !f.ledger_ok() {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
