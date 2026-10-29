//! UNX-F10178 多核 tick 同步正反双判据（同步达成与偏移越限误用受控）（0x10960 段）
//!
//! 锚点：UNX-F10178-J1 同步达成（多核 tick 偏移在限内 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：核网格（每核独立 tick 计数器）+ 同步核（窗口末求最大偏移，
//! 带内 Ok / 越限拒收并标记）+ 归零核（resync 对齐中位）+ 同步账本 +
//! 折叠指纹 + 注入核（幽灵核/零步进/越限偏移/伪造同步账/印后同步）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x10960 独占 0x20）。
pub const SEG_BASE: u32 = 0x10960;
pub const TS_GHOST: u32 = SEG_BASE + 0x01; // 幽灵核
pub const TS_ZERO: u32 = SEG_BASE + 0x02; // 零步进
pub const TS_SKEW: u32 = SEG_BASE + 0x03; // 偏移越限
pub const TS_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const TS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍同步

/// 偏移容忍带（tick）。
pub const TOL: u64 = 8;

/// 单核 tick 计数器。
#[derive(Clone, Copy)]
pub struct CoreTick {
    pub core: u32,
    pub ticks: u64,
}

/// 一次同步窗口的账本留痕。
#[derive(Clone)]
pub struct SyncRec {
    pub seq: u32,
    pub window: u32,
    /// 窗口末最大偏移。
    pub max_off: u64,
    /// 是否越限（越限账为拒收留痕）。
    pub skewed: bool,
    pub note: String,
}

impl SyncRec {
    /// 账目合法性：留痕非空；带内账偏移必在容忍带内。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (self.skewed || self.max_off <= TOL)
    }

    pub fn render_line(&self) -> String {
        format!(
            "S{}|w{}|off{}|sk{}|{}",
            self.seq, self.window, self.max_off, self.skewed as u8, self.note
        )
    }
}

/// 同步结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncRes {
    InBand { max_off: u64 },
    Skewed { max_off: u64 },
}

/// 核网格：多核 tick 计数 + 窗口同步。
pub struct TickMesh {
    pub cores: Vec<CoreTick>,
    pub ledger: Vec<SyncRec>,
    pub seq_next: u32,
    pub windows: u32,
    pub sealed: bool,
}

impl TickMesh {
    pub fn new(n: u32) -> TickMesh {
        let mut cores = Vec::new();
        let mut i = 0;
        while i < n {
            cores.push(CoreTick { core: i, ticks: 0 });
            i += 1;
        }
        TickMesh { cores, ledger: Vec::new(), seq_next: 1, windows: 0, sealed: false }
    }

    fn idx_of(&self, core: u32) -> Option<usize> {
        self.cores.iter().position(|c| c.core == core)
    }

    /// 单核推进（零步进/幽灵核必拒）。
    pub fn advance(&mut self, core: u32, n: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(TS_FROZEN);
        }
        if n == 0 {
            return Err(TS_ZERO);
        }
        let i = match self.idx_of(core) {
            Some(i) => i,
            None => return Err(TS_GHOST),
        };
        self.cores[i].ticks += n;
        Ok(self.cores[i].ticks)
    }

    /// 当前最大偏移（max - min）。
    pub fn max_offset(&self) -> u64 {
        if self.cores.is_empty() {
            return 0;
        }
        let mut mx = self.cores[0].ticks;
        let mut mn = self.cores[0].ticks;
        for c in &self.cores {
            if c.ticks > mx {
                mx = c.ticks;
            }
            if c.ticks < mn {
                mn = c.ticks;
            }
        }
        mx - mn
    }

    /// 窗口同步核：带内落账放行；越限落拒收账并回报 Skewed。
    pub fn sync_window(&mut self, note: &str) -> Result<SyncRes, u32> {
        if self.sealed {
            return Err(TS_FROZEN);
        }
        if note.is_empty() {
            return Err(TS_EMPTY_NOTE);
        }
        if self.cores.is_empty() {
            return Err(TS_GHOST);
        }
        let off = self.max_offset();
        let skewed = off > TOL;
        self.windows += 1;
        self.ledger.push(SyncRec {
            seq: self.seq_next,
            window: self.windows,
            max_off: off,
            skewed,
            note: note.to_string(),
        });
        self.seq_next += 1;
        if skewed {
            Err(TS_SKEW)
        } else {
            Ok(SyncRes::InBand { max_off: off })
        }
    }

    /// 归零核：全体核对齐到最小值（偏移归零）。
    pub fn resync(&mut self) {
        if self.cores.is_empty() {
            return;
        }
        let mut mn = self.cores[0].ticks;
        for c in &self.cores {
            if c.ticks < mn {
                mn = c.ticks;
            }
        }
        for c in &mut self.cores {
            c.ticks = mn;
        }
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（67 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10960;
        for c in &self.cores {
            h = h.wrapping_mul(67).wrapping_add(c.core);
            h = h.wrapping_mul(67).wrapping_add(c.ticks as u32);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(67).wrapping_add(r.max_off as u32);
            h = h.wrapping_mul(67).wrapping_add(r.skewed as u32);
            for by in r.note.bytes() {
                h = h.wrapping_mul(67).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut m = TickMesh::new(4);
    // 1) 幽灵核推进必拒。
    got[0] = m.advance(99, 1, ).err() == Some(TS_GHOST);
    // 2) 零步进必拒。
    got[1] = m.advance(0, 0).err() == Some(TS_ZERO);
    // 3) 越限偏移必拒（单核独走 100 tick > TOL 8）。
    let _ = m.advance(0, 100);
    got[2] = m.sync_window("inj-skew").err() == Some(TS_SKEW);
    // 4) 伪造带内账（偏移 99 却标带内）必判非法。
    let forged = SyncRec {
        seq: 1,
        window: 1,
        max_off: 99,
        skewed: false,
        note: "inj-fake-inband".to_string(),
    };
    got[3] = !forged.ok();
    // 5) 印账后同步必抓。
    m.seal();
    got[4] = m.sync_window("inj-frozen").err() == Some(TS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10178_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10178");

    // J1 同步达成：小偏移窗口带内放行。
    let j1 = {
        let mut m = TickMesh::new(4);
        let _ = m.advance(0, 3);
        let _ = m.advance(1, 5);
        let r = m.sync_window("w1");
        r == Ok(SyncRes::InBand { max_off: 5 })
    };
    s.add("J1-同步达成", j1, "四核偏移 5 落在带内放行");

    // J2 越限受控：偏移超带必拒且账面如实标 skewed。
    let j2 = {
        let mut m = TickMesh::new(2);
        let _ = m.advance(1, 9);
        let e = m.sync_window("sk");
        e == Err(TS_SKEW) && m.ledger[0].skewed && m.ledger[0].max_off == 9
    };
    s.add("J2-越限受控", j2, "偏移 9>8 必拒且账面标记");

    // J3 归零收敛：resync 后偏移归零再同步带内。
    let j3 = {
        let mut m = TickMesh::new(3);
        let _ = m.advance(0, 50);
        let _ = m.advance(1, 80);
        let _ = m.sync_window("sk");
        m.resync();
        m.max_offset() == 0 && m.sync_window("ok").is_ok()
    };
    s.add("J3-归零收敛", j3, "resync 偏移归零恢复带内");

    // J4 边界含边：偏移恰为 TOL 放行、TOL+1 拒。
    let j4 = {
        let mut m = TickMesh::new(2);
        let _ = m.advance(1, TOL);
        let a = m.sync_window("edge");
        let mut m2 = TickMesh::new(2);
        let _ = m2.advance(1, TOL + 1);
        let b = m2.sync_window("edge+");
        a.is_ok() && b == Err(TS_SKEW)
    };
    s.add("J4-边界含边", j4, "TOL 放行、TOL+1 拒收");

    // J5 幽灵核/零步进双拒。
    let j5 = {
        let mut m = TickMesh::new(2);
        m.advance(9, 1).err() == Some(TS_GHOST) && m.advance(0, 0).err() == Some(TS_ZERO)
    };
    s.add("J5-幽灵零步", j5, "幽灵核与零步进必拒");

    // J6 账本完整：带内/越限混排全合法且窗口号递增。
    let j6 = {
        let mut m = TickMesh::new(2);
        let _ = m.advance(0, 2);
        let _ = m.sync_window("w1");
        let _ = m.advance(0, 20);
        let _ = m.sync_window("w2");
        m.ledger.len() == 2
            && m.ledger_ok()
            && m.seq_unique()
            && m.ledger[0].window == 1
            && m.ledger[1].window == 2
    };
    s.add("J6-账本完整", j6, "窗口号递增且逐条合法");

    // J7 空网格拒：无核网格同步必拒（防零核误报同步成功）。
    let j7 = {
        let mut m = TickMesh::new(0);
        m.sync_window("empty").err() == Some(TS_GHOST)
    };
    s.add("J7-空网格拒", j7, "零核网格同步必拒");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut m = TickMesh::new(2);
        let _ = m.advance(0, 1);
        let _ = m.sync_window("w");
        let f1 = m.fold();
        let f2 = m.fold();
        m.seal();
        f1 == f2 && m.sync_window("post").err() == Some(TS_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10178_tick_flow() {
        let mut m = TickMesh::new(3);
        let _ = m.advance(0, 10);
        let _ = m.advance(1, 12);
        let _ = m.advance(2, 11);
        assert_eq!(m.max_offset(), 2);
        assert_eq!(m.sync_window("w"), Ok(SyncRes::InBand { max_off: 2 }));
        let _ = m.advance(2, 100);
        assert_eq!(m.sync_window("x"), Err(TS_SKEW));
        m.resync();
        assert_eq!(m.max_offset(), 0);
        assert_eq!(m.advance(5, 1), Err(TS_GHOST));
    }

    #[test]
    fn unx10178_checks_green() {
        let s = run_unx10178_checks();
        assert!(s.all_passed(), "UNX-F10178 判据未全绿");
    }
}
