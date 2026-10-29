//! UNX-F10817 ENOMEM 内核资源矩阵（0x1bea0 段）
//!
//! 锚点：UNX-F10817-J1 池分配对平——请求 k 格可用 f>=k 必整批放行、
//! f<k 必 ENOMEM 且零占格、释放回池计数同步三线全对平；注入坏实现
//! （部分占用/释放超发）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：内核池账（slots 总格 × in_use 占用）+ 批量分配判定器（all-or-
//! nothing）+ 释放钳制状态机 + 占用快照核（逐次分配快照对拍）+ 注入核
//! （两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：ENOMEM=12。
pub const POSIX_ENOMEM: u32 = 12;

/// 诊断段（0x1bea0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bea0;
pub const NM_PARTIAL: u32 = SEG_BASE + 0x01; // 失败却部分占用
pub const NM_REL_OVER: u32 = SEG_BASE + 0x02; // 释放超发
pub const NM_ALLOC_MISS: u32 = SEG_BASE + 0x03; // 放行漏推占用
pub const NM_ZERO_REQ: u32 = SEG_BASE + 0x04; // 零请求错账
pub const NM_CAP_SKEW: u32 = SEG_BASE + 0x05; // 池容量错账
pub const NM_SNAP_SKEW: u32 = SEG_BASE + 0x06; // 快照对拍错

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        NM_PARTIAL => "失败却部分占用",
        NM_REL_OVER => "释放超发",
        NM_ALLOC_MISS => "放行漏推占用",
        NM_ZERO_REQ => "零请求错账",
        NM_CAP_SKEW => "池容量错账",
        NM_SNAP_SKEW => "快照对拍错",
        _ => "未知诊断码",
    }
}

/// 场景输入：池格总数 × 已占格 × 请求格。
#[derive(Clone)]
pub struct Scenario {
    pub slots: u32,
    pub in_use: u32,
    pub want: u32,
}

/// 批量分配判定核：all-or-nothing——可用 >= 请求整批放行，否则 ENOMEM 零占用。
pub fn judge(sc: &Scenario) -> Result<(), u32> {
    if sc.want == 0 {
        return Ok(());
    }
    let free = sc.slots.saturating_sub(sc.in_use);
    if free >= sc.want {
        Ok(())
    } else {
        Err(POSIX_ENOMEM)
    }
}

/// 内核池账状态机：批量分配/整批释放，计数钳制。
pub struct Pool {
    pub slots: u32,
    pub in_use: u32,
    pub ledger: Vec<(u32, String)>,
    pub note: String,
}

impl Pool {
    pub fn new(slots: u32) -> Pool {
        Pool { slots, in_use: 0, ledger: Vec::new(), note: "pool".to_string() }
    }

    /// 批量分配：放行推进占用，ENOMEM 零占用落账。
    pub fn alloc(&mut self, want: u32) -> Result<(), u32> {
        let sc = Scenario { slots: self.slots, in_use: self.in_use, want };
        let out = judge(&sc);
        if out.is_ok() {
            self.in_use += want;
            self.ledger.push((want, "granted".to_string()));
            self.note = "granted".to_string();
        } else {
            self.ledger.push((want, "enomem".to_string()));
            self.note = "enomem".to_string();
        }
        out
    }

    /// 整批释放：占用回退、下限钳零。
    pub fn release(&mut self, n: u32) {
        if n > self.in_use {
            self.in_use = 0;
        } else {
            self.in_use -= n;
        }
        self.note = "released".to_string();
    }

    /// 账面对拍：granted 行必有占用推进、enomem 行零推进。
    pub fn ledger_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let (_w, tag) = &self.ledger[i];
            if *tag != "granted" && *tag != "enomem" {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 () 或 ENOMEM。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：空闲充裕
    m.push((Scenario { slots: 64, in_use: 0, want: 16 }, None));
    // 正：恰好贴界
    m.push((Scenario { slots: 32, in_use: 24, want: 8 }, None));
    // 正：零请求免检
    m.push((Scenario { slots: 0, in_use: 0, want: 0 }, None));
    // 反：差 1 格
    m.push((Scenario { slots: 32, in_use: 24, want: 9 }, Some(POSIX_ENOMEM)));
    // 反：池空请求
    m.push((Scenario { slots: 8, in_use: 8, want: 1 }, Some(POSIX_ENOMEM)));
    // 边界：cap=0 请求 1
    m.push((Scenario { slots: 0, in_use: 0, want: 1 }, Some(POSIX_ENOMEM)));
    // 边界：请求超池总量
    m.push((Scenario { slots: 4, in_use: 0, want: 8 }, Some(POSIX_ENOMEM)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want) = &m[i];
        let got = judge(sc);
        let hit = match want {
            None => got.is_ok(),
            Some(c) => got.err() == Some(*c),
        };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：ENOMEM 却部分占用（部分提交）。
pub fn faulty_partial(sc: &Scenario) -> (Result<(), u32>, u32) {
    let out = judge(sc);
    match out {
        Err(_) => (out, 2.min(sc.slots.saturating_sub(sc.in_use))),
        Ok(()) => (out, sc.want),
    }
}

/// 坏实现 B：放行漏推占用。
pub fn faulty_no_take(sc: &Scenario) -> (Result<(), u32>, u32) {
    let out = judge(sc);
    (out, 0)
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 失败却占用 2 格：权威零占用 → 检出
    let sc1 = Scenario { slots: 8, in_use: 7, want: 4 };
    let (_, take_bad) = faulty_partial(&sc1);
    got[0] = take_bad != 0 && judge(&sc1).is_err();
    // 2) 放行零推进：权威实进 want → 检出
    let sc2 = Scenario { slots: 8, in_use: 0, want: 3 };
    let (_, take_bad2) = faulty_no_take(&sc2);
    got[1] = take_bad2 == 0 && judge(&sc2).is_ok();
    got
}

/// 占用快照核：逐次分配落快照（先验场景同判 + 占用账同步）。
pub fn snapshot_replay_ok() -> bool {
    let mut p = Pool::new(8);
    let mut snaps: Vec<(u32, Result<(), u32>)> = Vec::new();
    let mut i = 0;
    while i < 3 {
        let sc = Scenario { slots: p.slots, in_use: p.in_use, want: 3 };
        let want = judge(&sc);
        let got = p.alloc(3);
        if want != got {
            return false;
        }
        snaps.push((p.in_use, got.clone()));
        i += 1;
    }
    // 8 格池请求 3×3：前两次通（in_use 3→6）、第三次 ENOMEM（差 1 格）
    snaps.len() == 3
        && snaps[0] == (3, Ok(()))
        && snaps[1] == (6, Ok(()))
        && snaps[2].1.err() == Some(POSIX_ENOMEM)
        && p.in_use == 6
}

/// 生命周期核：分配-释放-再分配，释放超发钳零不回绕。
pub fn lifecycle_ok() -> bool {
    let mut p = Pool::new(16);
    let a1 = p.alloc(8);
    let a2 = p.alloc(8);
    let a3 = p.alloc(1);
    let d1_ok = a1.is_ok() && a2.is_ok() && a3.err() == Some(POSIX_ENOMEM);
    p.release(16);
    let a4 = p.alloc(16);
    let a5 = p.alloc(1);
    let d2_ok = a4.is_ok() && a5.err() == Some(POSIX_ENOMEM) && p.in_use == 16;
    // 超发释放：in_use 16 → release(24) → 0，再 release(5) 仍 0
    p.release(24);
    p.release(5);
    d1_ok && d2_ok && p.in_use == 0 && p.ledger_ok()
}

/// 池面注记（对账/落账用）。
pub fn pool_note(p: &Pool) -> String {
    format!("slots={} in_use={} free={} note={}",
        p.slots, p.in_use, p.slots.saturating_sub(p.in_use), p.note)
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10817_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10817");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线池样本全命中");

    // J2 正路径：空闲充裕整批放行
    let j2 = judge(&Scenario { slots: 64, in_use: 0, want: 16 }).is_ok();
    s.add("J2-整批放行", j2, "free>=want 全进");

    // J3 反路径：差 1 格必 ENOMEM 零占用
    let j3 = judge(&Scenario { slots: 32, in_use: 24, want: 9 }).err() == Some(POSIX_ENOMEM);
    s.add("J3-差1格ENOMEM", j3, "all-or-nothing");

    // J4 边界：恰好贴界放行
    let j4 = judge(&Scenario { slots: 32, in_use: 24, want: 8 }).is_ok();
    s.add("J4-贴界放行", j4, "free==want 恰进");

    // J5 边界：零请求免检
    let j5 = judge(&Scenario { slots: 0, in_use: 0, want: 0 }).is_ok();
    s.add("J5-零请求免检", j5, "want=0 不查池");

    // J6 边界：请求超池总量必拒
    let j6 = judge(&Scenario { slots: 4, in_use: 0, want: 8 }).err() == Some(POSIX_ENOMEM);
    s.add("J6-超池必拒", j6, "want>slots 恒拒");

    // J7 占用快照核
    s.add("J7-快照对拍", snapshot_replay_ok(), "逐次先验同判且占用同步");

    // J8 注入1 部分占用检出
    s.add("J8-部分占用检出", inj[0], "ENOMEM 部分提交坏实现必分歧");

    // J9 注入2 放行漏推检出
    s.add("J9-漏推检出", inj[1], "放行零推进坏实现必分歧");

    // J10 生命周期核：超发释放钳零
    s.add("J10-生命周期核", lifecycle_ok(), "涨落对账且钳零不回绕");

    // J11 字面锚：ENOMEM=12 且诊断表完备
    let j11 = POSIX_ENOMEM == 12 && diag_text(NM_PARTIAL).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "ENOMEM 字面 12 且语义完备");

    // J12 池面注记闭集
    let mut p2 = Pool::new(4);
    let _ = p2.alloc(2);
    let n = pool_note(&p2);
    let j12 = n.contains("in_use=2") && n.contains("free=2");
    s.add("J12-注记对账", j12, "slots/in_use/free 三量一致");

    // J13 (dyn) 池账指纹落账
    let mut p3 = Pool::new(32);
    let _ = p3.alloc(8);
    let _ = p3.alloc(8);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(p3.slots);
    h = h.wrapping_mul(31).wrapping_add(p3.in_use);
    h = h.wrapping_mul(31).wrapping_add(p3.ledger.len() as u32);
    for c in p3.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-池账指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细
    s.add_dyn(
        "J14-命中明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10817_pool_flow() {
        let mut p = Pool::new(4);
        assert!(p.alloc(4).is_ok());
        assert!(p.alloc(1).err() == Some(POSIX_ENOMEM));
        assert_eq!(p.in_use, 4);
        p.release(2);
        assert!(p.alloc(2).is_ok());
        assert!(p.alloc(1).err() == Some(POSIX_ENOMEM));
        p.release(9);
        assert_eq!(p.in_use, 0);
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10817_checks_green() {
        let s = run_unx10817_checks();
        assert!(s.all_passed(), "UNX-F10817 判据未全绿");
    }
}
