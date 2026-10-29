//! unx4048_nlink — UNX-F4048 i_nlink 计数纪律（0 值不可见不变式）（诊断段 0x8860）。
//!
//! 判据原文：
//!   J1 nlink 从 1→0 的瞬间 inode 对查找路径不可见（可见性窗口
//!      注入检测零命中）；
//!   J2 nlink 上限（65,000 类）溢出注入返回错误。
//! 设计纪律：
//!   1. 可见性不变式：nlink ≥ 1 才可被查找路径命中；1→0 的瞬间
//!      可见性同时消失（同帧切换，无中间窗口）。
//!   2. 计数纪律：link ++（上限截停）、unlink --（下限截停）；
//!      0 是唯一回收值——nlink=0 的 inode 只存在于孤儿回收路径
//!      （回收语义归 4035，本锚只记账可见性与计数）。
//!   3. 上限定编：NLINK_CAP = 65000（类 ext2 约定，写死）；溢出
//!      注入返回 E_NLINK_CAP，计数不动。
//!   4. 状态机：Unlinked（nlink=0，查找不可见）→ Linked（nlink≥1）
//!      → Unlinked（回收候选），双向迁移以计数为真值源。
//!   5. 边界：对不可见 inode 的查找（不可见注入面）、重复 unlink
//!      至负、上限溢出——全部显性拒收并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8860;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_NLINK_CAP: u16 = SEG_BASE + 1;   // nlink 上限溢出（判据 J2）
const E_NLINK_ZERO: u16 = SEG_BASE + 2;  // unlink 至 0 以下（下限截停）
const E_LOOKUP_INVIS: u16 = SEG_BASE + 3;// 查找命中不可见 inode（窗口注入面）
const E_NO_FILE: u16 = SEG_BASE + 4;     // 不存在的 inode
const E_DROP_LIVE: u16 = SEG_BASE + 5;   // 仍可见时回收（不变式失守）
const E_BAL: u16 = SEG_BASE + 6;         // link/unlink 账不平

/// nlink 上限（类 ext2 约定，写死）。
pub const NLINK_CAP: u32 = 65000;

/// inode 可见性状态机（真值源 = nlink 计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisPhase {
    Unlinked, // nlink=0：查找不可见（回收候选）
    Linked,   // nlink≥1：查找可见
}

/// 单 inode nlink 账。
#[derive(Clone)]
pub struct NlinkIno {
    pub i_ino: u64,   // inode 号
    pub nlink: u32,   // 硬链接计数
    pub links: u32,   // 累计 link 事件
    pub unlinks: u32, // 累计 unlink 事件
}

/// nlink 纪律枢纽。
pub struct NlinkGate {
    pub inos: Vec<NlinkIno>,
    pub visible_total: u32,  // 累计可见化事件（0→1 迁移数）
    pub drop_total: u32,     // 累计不可见化事件（1→0 迁移数）
    pub probe_total: u32,    // 查找触达总数（含被拒）
    pub window_hits: u32,    // 可见性窗口命中数（判据 J1 必须为 0）
    pub rejects: u32,        // 错误账
}

impl NlinkGate {
    pub fn new() -> NlinkGate {
        NlinkGate {
            inos: Vec::new(),
            visible_total: 0,
            drop_total: 0,
            probe_total: 0,
            window_hits: 0,
            rejects: 0,
        }
    }

    /// 建 inode：nlink 0（Unlinked，查找不可见）。
    pub fn create(&mut self, i_ino: u64) -> Result<usize, u16> {
        self.inos.push(NlinkIno {
            i_ino,
            nlink: 0,
            links: 0,
            unlinks: 0,
        });
        Ok(self.inos.len() - 1)
    }

    /// 阶段派生（真值源 = nlink）。
    fn phase_of(nlink: u32) -> VisPhase {
        if nlink == 0 {
            VisPhase::Unlinked
        } else {
            VisPhase::Linked
        }
    }

    /// link：0→1 迁移记可见化账；上限溢出拒收（判据 J2）。
    pub fn link(&mut self, idx: usize) -> Result<u32, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if self.inos[idx].nlink >= NLINK_CAP {
            self.rejects += 1;
            return Err(E_NLINK_CAP);
        }
        let was = Self::phase_of(self.inos[idx].nlink);
        self.inos[idx].nlink += 1;
        self.inos[idx].links += 1;
        if was == VisPhase::Unlinked {
            self.visible_total += 1;
        }
        Ok(self.inos[idx].nlink)
    }

    /// unlink：1→0 迁移记不可见化账（同帧生效）；0 以下拒收。
    pub fn unlink(&mut self, idx: usize) -> Result<u32, u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if self.inos[idx].nlink == 0 {
            self.rejects += 1;
            return Err(E_NLINK_ZERO);
        }
        self.inos[idx].nlink -= 1;
        self.inos[idx].unlinks += 1;
        if self.inos[idx].nlink == 0 {
            self.drop_total += 1;
        }
        Ok(self.inos[idx].nlink)
    }

    /// 查找路径：nlink ≥ 1 才命中；nlink=0 命中即窗口违例
    /// （判据 J1 注入检测：命中计数必须保持零增长后拒收）。
    pub fn lookup(&mut self, idx: usize) -> Result<u64, u16> {
        self.probe_total += 1;
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if self.inos[idx].nlink == 0 {
            // 不可见 inode 被查找：注入面（应零命中）。
            self.rejects += 1;
            return Err(E_LOOKUP_INVIS);
        }
        Ok(self.inos[idx].i_ino)
    }

    /// 回收路径：仅 nlink=0 可回收；仍可见回收拒收（不变式失守）。
    pub fn reclaim(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.inos.len() {
            self.rejects += 1;
            return Err(E_NO_FILE);
        }
        if self.inos[idx].nlink != 0 {
            self.rejects += 1;
            return Err(E_DROP_LIVE);
        }
        // 池移除（简化为标记移除：末位交换删除）。
        let last = self.inos.len() - 1;
        if idx != last {
            self.inos[idx] = self.inos[last].clone();
        }
        self.inos.remove(last);
        Ok(())
    }

    /// 可见性窗口注入检测（判据 J1）：对全部 Unlinked inode 逐一
    /// 探测，任何命中即窗口违例（计数增长）。
    pub fn window_assert(&mut self) -> Result<u32, u16> {
        let mut i = 0;
        while i < self.inos.len() {
            if i < self.inos.len() && self.inos[i].nlink == 0 {
                match self.lookup(i) {
                    Ok(_) => {
                        // 窗口命中：计数增长并失守。
                        self.window_hits += 1;
                        return Err(E_BAL);
                    }
                    Err(_) => {}
                }
            }
            i += 1;
        }
        Ok(self.window_hits)
    }

    /// 守恒对账：links - unlinks = nlink（逐 inode）；可见化 - 不可
    /// 见化 = 当前可见数。
    pub fn audit(&self) -> Result<(), u16> {
        let mut vis = 0u32;
        let mut i = 0;
        while i < self.inos.len() {
            if i < self.inos.len() {
                let f = &self.inos[i];
                if f.links < f.unlinks {
                    return Err(E_BAL);
                }
                if f.links - f.unlinks != f.nlink {
                    return Err(E_BAL);
                }
                if f.nlink > 0 {
                    vis += 1;
                }
            }
            i += 1;
        }
        if self.visible_total < self.drop_total {
            return Err(E_BAL);
        }
        if self.visible_total - self.drop_total != vis {
            return Err(E_BAL);
        }
        Ok(())
    }

    /// 导出快照（inode 数，可见化账，不可见化账，窗口命中账）。
    pub fn ledger(&self) -> (usize, u32, u32, u32) {
        (self.inos.len(), self.visible_total, self.drop_total, self.window_hits)
    }

    /// 可见集合视图：nlink ≥ 1 的 inode 号列表（真值源复核面）。
    pub fn visible_ids(&self) -> Vec<u64> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.inos.len() {
            if self.inos[i].nlink > 0 {
                v.push(self.inos[i].i_ino);
            }
            i += 1;
        }
        v
    }

    /// 计数余量视图：距上限的空余（溢出预警面）。
    pub fn headroom(&self, idx: usize) -> Result<u32, u16> {
        if idx >= self.inos.len() {
            return Err(E_NO_FILE);
        }
        Ok(NLINK_CAP - self.inos[idx].nlink)
    }

    /// 迁移账闭合视图：可见化 - 不可见化 = 当前可见数（布尔面）。
    pub fn migration_ok(&self) -> bool {
        let mut vis = 0u32;
        let mut i = 0;
        while i < self.inos.len() {
            if self.inos[i].nlink > 0 {
                vis += 1;
            }
            i += 1;
        }
        self.visible_total >= self.drop_total
            && self.visible_total - self.drop_total == vis
    }
}

/// 自例程：R1 可见化链 → R2 瞬时不可见 → R3 上限溢出 → R4 窗口注入 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut g = NlinkGate::new();

    // R1 可见化链：create（不可见）→ link（可见）→ lookup 命中。
    let f = g.create(10)?;
    if g.lookup(f) != Err(E_LOOKUP_INVIS) {
        // nlink=0 建立即不可见（不变式前置）。
        return Err(E_BAL);
    }
    g.link(f)?;
    if g.lookup(f)? != 10 {
        return Err(E_BAL);
    }
    g.link(f)?; // nlink=2
    if g.lookup(f).is_err() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 瞬时不可见（判据 J1）：1→0 同帧后查找立即不可见，零窗口。
    g.unlink(f)?;
    g.unlink(f)?; // 1→0 瞬间
    if g.lookup(f) != Err(E_LOOKUP_INVIS) {
        return Err(E_BAL);
    }
    if g.window_hits != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 上限溢出（判据 J2）：灌到上限后再 link 拒收且计数不动。
    let f2 = g.create(11)?;
    // 直接置满（构造加速：批量 link 计数事件）。
    g.inos[f2].nlink = NLINK_CAP - 1;
    g.inos[f2].links = NLINK_CAP - 1;
    g.link(f2)?; // 恰到上限
    if g.inos[f2].nlink != NLINK_CAP {
        return Err(E_BAL);
    }
    if g.link(f2) != Err(E_NLINK_CAP) || g.inos[f2].nlink != NLINK_CAP {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 窗口注入 + 回收屏障：不可见集合零命中；可见 inode 回收被拒，
    // 不可见 inode 回收放行。
    g.window_assert()?;
    if g.reclaim(f2) != Err(E_DROP_LIVE) {
        // f2 仍可见（nlink=65000）：回收必须被拒（不变式失守面）。
        return Err(E_BAL);
    }
    g.reclaim(f)?; // f nlink=0：回收候选放行，出池。
    hits += 1;

    // R5 守恒对账：links - unlinks = nlink；可见化 - 不可见化 = 可见数。
    g.audit()?;
    let (n, vt, dt, wh) = g.ledger();
    if n != 1 || vt != 2 || dt != 1 || wh != 0 {
        return Err(E_BAL);
    }
    // 可见集合视图（f 已回收出池，仅剩 f2）与余量视图。
    let vis = g.visible_ids();
    if vis.len() != 1 || vis[0] != 11 {
        return Err(E_BAL);
    }
    if g.headroom(f2)? != 0 {
        // f2 恰在上限：余量为 0。
        return Err(E_BAL);
    }
    if !g.migration_ok() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
