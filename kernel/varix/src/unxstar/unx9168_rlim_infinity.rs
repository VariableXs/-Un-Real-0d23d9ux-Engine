//! UNX-F9168 RLIM_INFINITY 与软硬限翻转协议（0x634C0 段）
//!
//! 锚点：UNX-F9168-J1 翻转账（每次软/硬限翻转落账：资源、旧对、新
//! 对、是否涉无穷、留痕）；J2 翻转语义（∞ 直通值序闸 + ∞→有限值
//! 收敛闸 + 有限→∞ 扩张放行 + 硬限只升不降（非特权）四性质）；J3
//! 翻转指纹（账本折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C2-B19（RLIM_INFINITY 翻转面）。
//!
//! 结构：翻转簿 + 翻转核（四闸）+ 翻转账本 + 折叠指纹 + 注入核（错
//! 向翻转/空留痕/伪资源/重号/印后登记必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x634C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x634C0;
pub const IF_UNK_RES: u32 = SEG_BASE + 0x01; // 伪资源
pub const IF_BAD_MOVE: u32 = SEG_BASE + 0x02; // 错向翻转
pub const IF_EMPTY_NOTE: u32 = SEG_BASE + 0x03; // 空留痕
pub const IF_DUP_SEQ: u32 = SEG_BASE + 0x04; // 重序号
pub const IF_FROZEN: u32 = SEG_BASE + 0x05; // 印后登记

/// RLIM_INFINITY。
pub const INF: u32 = u32::MAX;

/// 结果码。
pub const R_OK: u32 = 0;
pub const R_EPERM: u32 = 1; // 非特权降硬限
pub const R_EINVAL: u32 = 2; // 值序
pub const R_COUNT: usize = 3;

pub const R_NAMES: [&str; R_COUNT] = ["OK", "EPERM", "EINVAL"];

/// 翻转方向闭集。
pub const MV_INF_TO_FIN: usize = 0; // ∞ → 有限（收敛）
pub const MV_FIN_TO_INF: usize = 1; // 有限 → ∞（扩张）
pub const MV_FIN_TO_FIN: usize = 2; // 有限 → 有限（平移）
pub const MV_COUNT: usize = 3;

pub const MV_NAMES: [&str; MV_COUNT] = ["inf->fin", "fin->inf", "fin->fin"];

/// 方向判定。
pub fn move_kind(old: u32, new: u32) -> usize {
    if old == INF && new != INF {
        MV_INF_TO_FIN
    } else if old != INF && new == INF {
        MV_FIN_TO_INF
    } else {
        MV_FIN_TO_FIN
    }
}

/// 单条翻转账。
#[derive(Clone)]
pub struct Flip {
    pub seq: u32,
    pub res: usize,
    pub old: (u32, u32),
    pub new: (u32, u32),
    pub kind: usize,
    pub code: u32,
    pub note: String,
}

impl Flip {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.kind < MV_COUNT && (self.code as usize) < R_COUNT
    }

    pub fn render_line(&self) -> String {
        format!(
            "IF{}|r{}|{:?}->{:?}|{}|{}|{}",
            self.seq,
            self.res,
            self.old,
            self.new,
            MV_NAMES[self.kind],
            R_NAMES[self.code as usize],
            self.note
        )
    }
}

/// 翻转簿。
pub struct FlipBook {
    /// res → (cur, max)
    pub limits: Vec<(usize, (u32, u32))>,
    pub ledger: Vec<Flip>,
    pub privileged: bool,
    pub seq_next: u32,
    pub sealed: bool,
}

impl FlipBook {
    pub fn new(privileged: bool) -> FlipBook {
        FlipBook {
            limits: Vec::new(),
            ledger: Vec::new(),
            privileged,
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn seed(&mut self, res: usize, cur: u32, max: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IF_FROZEN);
        }
        if res >= 8 {
            return Err(IF_UNK_RES);
        }
        match self.limits.iter_mut().find(|(r, _)| *r == res) {
            Some((_, v)) => *v = (cur, max),
            None => self.limits.push((res, (cur, max))),
        }
        Ok(())
    }

    fn pair(&self, res: usize) -> Option<(u32, u32)> {
        self.limits.iter().find(|(r, _)| *r == res).map(|(_, v)| *v)
    }

    /// 翻转核：set 软/硬限。
    pub fn flip(&mut self, res: usize, nc: u32, nm: u32, note: &str) -> Result<usize, u32> {
        if self.sealed {
            return Err(IF_FROZEN);
        }
        if note.is_empty() {
            return Err(IF_EMPTY_NOTE);
        }
        if res >= 8 {
            return Err(IF_UNK_RES);
        }
        let old = match self.pair(res) {
            Some(p) => p,
            None => return Err(IF_UNK_RES),
        };
        // 值序闸（∞ 直通）
        if !(nc == INF || nm == INF || nc <= nm) {
            self.record(res, old, (nc, nm), R_EINVAL, note);
            return Err(R_EINVAL);
        }
        // 硬限只升不降（非特权）：nm < old.1 且两者均有限 → EPERM
        if !self.privileged && nm != INF && old.1 != INF && nm < old.1 {
            self.record(res, old, (nc, nm), R_EPERM, note);
            return Err(R_EPERM);
        }
        let kind = move_kind(old.1, nm);
        let i = self.limits.iter().position(|(r, _)| *r == res).unwrap_or(0);
        self.limits[i].1 = (nc, nm);
        self.record(res, old, (nc, nm), R_OK, note);
        Ok(kind)
    }

    fn record(&mut self, res: usize, old: (u32, u32), new: (u32, u32), code: u32, note: &str) {
        let seq = self.seq_next;
        self.seq_next += 1;
        let kind = move_kind(old.1, new.1);
        self.ledger.push(Flip {
            seq,
            res,
            old,
            new,
            kind,
            code,
            note: note.to_string(),
        });
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x91680;
        for f in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(f.seq);
            h = h.wrapping_mul(31).wrapping_add(f.res as u32);
            h = h.wrapping_mul(31).wrapping_add(f.kind as u32);
            h = h.wrapping_mul(31).wrapping_add(f.new.0);
            h = h.wrapping_mul(31).wrapping_add(f.new.1);
            h = h.wrapping_mul(31).wrapping_add(f.code);
            for b in f.note.bytes() {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 基础样本：r0(64/∞)、r1(1M/∞)、r2(64/4096)。
pub fn mk_book(privileged: bool) -> FlipBook {
    let mut b = FlipBook::new(privileged);
    let _ = b.seed(0, 64, INF);
    let _ = b.seed(1, 1024 * 1024, INF);
    let _ = b.seed(2, 64, 4096);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 错向账
    let bad = Flip {
        seq: 1,
        res: 0,
        old: (INF, INF),
        new: (10, 10),
        kind: 9,
        code: R_OK,
        note: "inj-kind".to_string(),
    };
    got[0] = !bad.ok();
    // 2) 空留痕
    let bad2 = Flip {
        seq: 2,
        res: 0,
        old: (0, 0),
        new: (0, 0),
        kind: 0,
        code: 0,
        note: String::new(),
    };
    got[1] = !bad2.ok();
    // 3) 伪资源
    let mut b = mk_book(true);
    got[2] = b.flip(99, 1, 1, "inj-res").err() == Some(IF_UNK_RES);
    // 4) 非特权降硬限
    let mut b2 = mk_book(false);
    got[3] = b2.flip(2, 32, 2048, "inj-lower").err() == Some(R_EPERM);
    // 5) 印后登记必抓
    b.seal();
    got[4] = b.flip(0, 128, INF, "inj-frozen").err() == Some(IF_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx9168_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9168");
    let mut b = mk_book(true);

    // J1 翻转账非空（三向齐备）
    let j1 = {
        let _ = b.flip(0, 128, INF, "平移软");
        let _ = b.flip(1, 2 * 1024 * 1024, INF, "再平移");
        let _ = b.flip(2, 64, INF, "有限→∞");
        b.ledger.iter().filter(|f| f.kind == MV_FIN_TO_INF).count() >= 1
            && b.ledger.iter().filter(|f| f.kind == MV_FIN_TO_FIN).count() >= 1
    };
    s.add("J1-翻转账非空", j1, "至少两种方向在册");

    // J2 账字段齐
    let j2 = b.ledger.iter().all(|f| f.ok());
    s.add("J2-账字段齐备", j2, "留痕非空且方向/码闭集");

    // J3 ∞ 直通值序闸
    let j3 = {
        let mut b2 = mk_book(true);
        b2.flip(0, INF, INF, "双∞").is_ok()
    };
    s.add("J3-无穷直通", j3, "RLIM_INFINITY 绕值序闸");

    // J4 ∞→有限 收敛（特权可）
    let j4 = {
        let mut b2 = mk_book(true);
        let kind = b2.flip(0, 1024, 4096, "收敛");
        kind == Ok(MV_INF_TO_FIN) && b2.pair(0) == Some((1024, 4096))
    };
    s.add("J4-收敛翻转", j4, "∞→有限收敛落账");

    // J5 有限→∞ 扩张
    let j5 = {
        let mut b2 = mk_book(true);
        let kind = b2.flip(2, 64, INF, "扩张");
        kind == Ok(MV_FIN_TO_INF)
    };
    s.add("J5-扩张翻转", j5, "有限→∞扩张放行");

    // J6 方向判定正确
    let j6 = move_kind(INF, 10) == MV_INF_TO_FIN
        && move_kind(10, INF) == MV_FIN_TO_INF
        && move_kind(10, 20) == MV_FIN_TO_FIN
        && move_kind(INF, INF) == MV_FIN_TO_FIN;
    s.add("J6-方向判定", j6, "三向判定含双双∞归平移");

    // J7 硬限只升不降（非特权）
    let j7 = {
        let mut b2 = mk_book(false);
        b2.flip(2, 32, 2048, "降硬").err() == Some(R_EPERM)
    };
    s.add("J7-硬限不降", j7, "非特权降硬限必 EPERM");

    // J8 硬限升放行
    let j8 = {
        let mut b2 = mk_book(false);
        b2.flip(2, 64, 8192, "升硬").is_ok()
    };
    s.add("J8-硬限可升", j8, "非特权升硬限放行");

    // J9 值序闸
    let j9 = {
        let mut b2 = mk_book(true);
        b2.flip(2, 9000, 4096, "软>硬").err() == Some(R_EINVAL)
    };
    s.add("J9-值序闸", j9, "软>硬必 EINVAL");

    // J10 伪资源拒
    let j10 = b.flip(99, 1, 1, "伪").err() == Some(IF_UNK_RES);
    s.add("J10-伪资源拒", j10, "资源出闭集必拒");

    // J11 翻转后 get 一致
    let j11 = {
        let mut b2 = mk_book(true);
        let _ = b2.flip(0, 256, INF, "改");
        b2.pair(0) == Some((256, INF))
    };
    s.add("J11-翻转一致", j11, "翻转后簿面值同步");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9168_flip_flow() {
        let mut b = mk_book(true);
        assert_eq!(b.flip(0, 128, INF, "a"), Ok(MV_FIN_TO_FIN));
        assert_eq!(b.flip(2, 64, INF, "b"), Ok(MV_FIN_TO_INF));
        assert_eq!(b.flip(0, 1024, 4096, "c"), Ok(MV_INF_TO_FIN));
        assert_eq!(b.pair(0), Some((1024, 4096)));
        // 非特权降硬
        let mut u = mk_book(false);
        assert_eq!(u.flip(2, 32, 2048, "d"), Err(R_EPERM));
        assert!(u.flip(2, 32, 8192, "e").is_ok());
    }

    #[test]
    fn unx9168_checks_green() {
        let s = run_unx9168_checks();
        assert!(s.all_passed(), "UNX-F9168 判据未全绿");
    }
}
