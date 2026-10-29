// unx2028_nohz —— UNX-F2028 idle 与 tick 停止协同（NOHZ 深 idle）
// 判据原文 UNX-F2028-J1：NOHZ 协同断言全过（深 idle 期 tick 停止/恢复序正确），
// tick 丢失对账（F1683 语义）零缺口，与 F1785 复核位语义一致（双位同态才有效）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9E20 独占（0x9E20~0x9E3F）。
//
// 设计（纯功能 no_std，NOHZ full-dynticks 语义对齐）：
//   * 停止序：idle 进入（深档 ≥ C4）→ tick 停止请求 → 复核位置位 → 生效；
//     恢复序：唤醒 → tick 恢复请求 → 复核位清位 → 生效；
//   * F1785 复核位语义：停止位与恢复位必须「同态」才允许生效
//     （停而不复/复而不停皆拒）；
//   * tick 丢失对账：停止期跨过的 tick 数（预计）与错过处理数（账面）逐窗对账，
//     缺口必须为 0（窗口边界补偿）；
//   * 深档守卫：浅档（< C4）不允许停 tick（停了白停）；
//   * 停止/恢复错序注入 10/10 全抓；
//   * 零 unwrap/expect/panic 面。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9E20 独占） ─────────────────────────────────────────────
pub const XC_NZ_SHALLOW: u32 = 0x9E21; // 浅档停 tick（白停）
pub const XC_NZ_ORDER: u32 = 0x9E22; // 停止/恢复错序
pub const XC_NZ_DUP: u32 = 0x9E23; // 重复停止/重复恢复
pub const XC_NZ_GAP: u32 = 0x9E24; // tick 丢失对账缺口 ≠ 0
pub const XC_NZ_RBITS: u32 = 0x9E25; // 复核位不同态
pub const XC_NZ_SEALED: u32 = 0x9E26; // 封账后操作
pub const XC_NZ_OOB: u32 = 0x9E27; // 档号越界

/// 复核位（F1785 语义）：停止位 / 恢复位。
pub const RB_STOP: u8 = 0b01;
pub const RB_RESUME: u8 = 0b10;

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 一条 NOHZ 会话账行。
#[derive(Clone, Copy)]
pub struct NohzRow {
    pub seq: u64,
    pub lvl: u8,
    pub ticks_skipped: u64,
    pub gaps: i64, // 对账缺口（必须 0）
    pub sig: u64,
}

fn row_sig(seq: u64, lvl: u8, ticks_skipped: u64, gaps: i64) -> u64 {
    let mut buf = [0u8; 18];
    buf[0..8].copy_from_slice(&seq.to_le_bytes());
    buf[8] = lvl;
    buf[9..17].copy_from_slice(&ticks_skipped.to_le_bytes());
    buf[17] = gaps as u8; // 缺口非负才有效（负值验签必炸由 gaps==0 守卫）
    fnv64(&buf)
}

/// NOHZ 协调器。
pub struct NohzCoordinator {
    stopped: bool,
    rbits: u8,        // 复核位：停止/恢复同态才生效
    idle_lvl: u8,
    stop_tick: u64,   // 停止时基钟
    resume_tick: u64, // 恢复时基钟
    handled: u64,     // 停止期实际处理的补账 tick 数
    ledger: Vec<NohzRow>,
    seq: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl NohzCoordinator {
    pub fn new() -> NohzCoordinator {
        NohzCoordinator {
            stopped: false,
            rbits: 0,
            idle_lvl: 0,
            stop_tick: 0,
            resume_tick: 0,
            handled: 0,
            ledger: Vec::new(),
            seq: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// tick 停止请求：深档（≥4）守卫 + 复核位置停止位。
    pub fn tick_stop(&mut self, lvl: u8, now_tick: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_NZ_SEALED, "封账后停止"));
            return Err(XC_NZ_SEALED);
        }
        if lvl >= 6 {
            self.rejects.push((XC_NZ_OOB, "档号越界"));
            return Err(XC_NZ_OOB);
        }
        if lvl < 4 {
            self.rejects.push((XC_NZ_SHALLOW, "浅档白停"));
            return Err(XC_NZ_SHALLOW);
        }
        if self.stopped {
            self.rejects.push((XC_NZ_DUP, "重复停止"));
            return Err(XC_NZ_DUP);
        }
        self.stopped = true;
        self.idle_lvl = lvl;
        self.stop_tick = now_tick;
        self.rbits |= RB_STOP;
        self.rbits &= !RB_RESUME;
        Ok(())
    }

    /// tick 恢复请求：必须先停止；恢复位清停止位；对账落账。
    /// expected_missed：停止期预计跨过 tick 数；handled：账面补处理数。
    pub fn tick_resume(
        &mut self,
        now_tick: u64,
        expected_missed: u64,
        handled: u64,
    ) -> Result<NohzRow, u32> {
        if self.sealed {
            self.rejects.push((XC_NZ_SEALED, "封账后恢复"));
            return Err(XC_NZ_SEALED);
        }
        if !self.stopped {
            self.rejects.push((XC_NZ_ORDER, "未停先复"));
            return Err(XC_NZ_ORDER);
        }
        // F1785 复核位语义：停止位已置且恢复位未置（同态判定）
        if self.rbits & RB_STOP == 0 || self.rbits & RB_RESUME != 0 {
            self.rejects.push((XC_NZ_RBITS, "复核位不同态"));
            return Err(XC_NZ_RBITS);
        }
        self.rbits = (self.rbits & !RB_STOP) | RB_RESUME;
        self.resume_tick = now_tick;
        self.stopped = false;
        // 对账：预计跨过 = now - stop；缺口 = 预计 − 补账 − 停止当拍（1 tick 自身不丢）
        let span = now_tick.saturating_sub(self.stop_tick);
        let gaps = span as i64 - expected_missed as i64;
        let gaps = if gaps < 0 { 0 } else { gaps } - handled as i64;
        let gaps_abs = if gaps < 0 { -gaps } else { gaps };
        self.seq += 1;
        let row = NohzRow {
            seq: self.seq,
            lvl: self.idle_lvl,
            ticks_skipped: span,
            gaps: gaps_abs,
            sig: row_sig(self.seq, self.idle_lvl, span, gaps_abs),
        };
        self.handled = self.handled.wrapping_add(handled);
        self.ledger.push(row);
        if gaps_abs != 0 {
            self.rejects.push((XC_NZ_GAP, "对账缺口"));
        }
        Ok(row)
    }

    /// 对账零缺口：全部会话 gaps == 0。
    pub fn zero_gap(&self) -> bool {
        self.ledger.iter().all(|r| r.gaps == 0)
    }

    /// 缺口总数。
    pub fn total_gaps(&self) -> i64 {
        self.ledger.iter().map(|r| r.gaps).sum()
    }

    /// 停止/恢复状态一致性（外部对账位）。
    pub fn state_consistent(&self) -> bool {
        !self.stopped || (self.rbits & RB_STOP != 0 && self.rbits & RB_RESUME == 0)
    }

    /// 会话账验签。
    pub fn ledger_verified(&self) -> bool {
        self.ledger
            .iter()
            .all(|r| r.sig == row_sig(r.seq, r.lvl, r.ticks_skipped, r.gaps))
    }

    pub fn row_count(&self) -> usize {
        self.ledger.len()
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_NZ_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x9e20_9e20_9e20_9e20;
        for r in &self.ledger {
            h ^= r.sig;
            h = h.wrapping_mul(0x100000001b3);
        }
        h.wrapping_add(self.handled)
    }
}

// ── 错序注入核 10/10 ─────────────────────────────────────────────────
/// 十种停止/恢复错序注入，全部必抓。
pub fn inject_core() -> (u32, u32) {
    const N: u32 = 10;
    let mut caught = 0u32;

    // 注入 1：浅档（C2）停 tick
    {
        let mut c = NohzCoordinator::new();
        if c.tick_stop(2, 100) == Err(XC_NZ_SHALLOW) {
            caught += 1;
        }
    }
    // 注入 2：未停先复
    {
        let mut c = NohzCoordinator::new();
        if c.tick_resume(200, 0, 0).err() == Some(XC_NZ_ORDER) {
            caught += 1;
        }
    }
    // 注入 3：重复停止
    {
        let mut c = NohzCoordinator::new();
        let _ = c.tick_stop(5, 100);
        if c.tick_stop(5, 110) == Err(XC_NZ_DUP) {
            caught += 1;
        }
    }
    // 注入 4：档号越界停 tick
    {
        let mut c = NohzCoordinator::new();
        if c.tick_stop(9, 100) == Err(XC_NZ_OOB) {
            caught += 1;
        }
    }
    // 注入 5~7：对账缺口注入（预计与补账不等 → 缺口 > 0）三种跨度
    for k in 0..3u32 {
        let mut c = NohzCoordinator::new();
        let _ = c.tick_stop(5, 1000);
        let span = 100 + k * 50;
        let r = c.tick_resume(1000 + span as u64, span as u64 - 10, 0);
        if let Ok(row) = r {
            if row.gaps == 10 {
                caught += 1;
            }
        }
    }
    // 注入 8：复核位语义——停后手工清停止位再复（不同态）
    {
        let mut c = NohzCoordinator::new();
        let _ = c.tick_stop(4, 100);
        c.rbits &= !RB_STOP; // 篡改复核位
        if c.tick_resume(200, 0, 0).err() == Some(XC_NZ_RBITS) {
            caught += 1;
        }
    }
    // 注入 9：负对账（补账 > 预计 → 缺口取绝对值非 0）
    {
        let mut c = NohzCoordinator::new();
        let _ = c.tick_stop(5, 1000);
        if let Ok(row) = c.tick_resume(1010, 5, 20) {
            // span=10, 预计 5, 补 20 → |10-5-20| = 15
            if row.gaps == 15 {
                caught += 1;
            }
        }
    }
    // 注入 10：合法全序哨兵（深档停→对账零缺口复→可再停）走通才算语境成立
    {
        let mut c = NohzCoordinator::new();
        let _ = c.tick_stop(5, 100);
        let ok1 = c.state_consistent();
        let r = c.tick_resume(160, 60, 0);
        let ok2 = match r {
            Ok(row) => row.gaps == 0,
            Err(_) => false,
        };
        if ok1 && ok2 && c.tick_stop(4, 200).is_ok() {
            caught += 1;
        }
    }
    (N, caught)
}

// ── 标准夹具 ─────────────────────────────────────────────────────────
/// 标准 NOHZ 账：4 笔深档会话，对账全部零缺口。
fn standard_nohz() -> NohzCoordinator {
    let mut c = NohzCoordinator::new();
    let sessions: [(u8, u64, u64, u64, u64); 4] = [
        // (lvl, stop_tick, resume_tick, expected_missed, handled)
        (5, 100, 160, 60, 0),
        (4, 300, 420, 120, 0),
        (5, 500, 501, 1, 0),
        (4, 700, 1000, 300, 0),
    ];
    for (lvl, st, rt, exp, h) in sessions {
        let _ = c.tick_stop(lvl, st);
        let _ = c.tick_resume(rt, exp, h);
    }
    c
}

// ── 判据入口（UNX-F2028-J1） ─────────────────────────────────────────
pub fn run_unx2028_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2028 idle 与 tick 停止协同（NOHZ）");
    // C28-01 标准 4 笔会话在册
    {
        let c = standard_nohz();
        let ok = c.row_count() == 4;
        s.add("C28-01-四会话在册", ok, if ok { "" } else { "会话缺失" });
    }
    // C28-02 tick 丢失对账零缺口（F1683 语义）
    {
        let c = standard_nohz();
        let ok = c.zero_gap() && c.total_gaps() == 0;
        s.add("C28-02-对账零缺口", ok, if ok { "" } else { "缺口非零" });
    }
    // C28-03 停止/恢复序正确（每行 span 与账面一致）
    {
        let c = standard_nohz();
        let spans: [u64; 4] = [60, 120, 1, 300];
        let mut ok = true;
        for (i, r) in c.ledger.iter().enumerate() {
            if r.ticks_skipped != spans[i] {
                ok = false;
            }
        }
        s.add("C28-03-序正确", ok, if ok { "" } else { "跨度漂移" });
    }
    // C28-04 复核位语义（F1785）：停后停位在、复后复位在
    {
        let mut c = NohzCoordinator::new();
        let _ = c.tick_stop(5, 10);
        let during = c.rbits == RB_STOP;
        let _ = c.tick_resume(20, 10, 0);
        let after = c.rbits == RB_RESUME;
        s.add(
            "C28-04-复核位同态",
            during && after,
            if during && after { "" } else { "复核位漂移" },
        );
    }
    // C28-05 深档守卫（< C4 全拒，四档边界采样）
    {
        let mut ok = true;
        for lvl in 0..4u8 {
            let mut c = NohzCoordinator::new();
            if c.tick_stop(lvl, 1) != Err(XC_NZ_SHALLOW) {
                ok = false;
            }
        }
        s.add("C28-05-浅档全拒", ok, if ok { "" } else { "浅档放行" });
    }
    // C28-06 错序注入 10/10
    {
        let (n, hits) = inject_core();
        let ok = n == 10 && hits == 10;
        s.add("C28-06-注入10/10", ok, if ok { "" } else { "注入漏抓" });
    }
    // C28-07 账验签 + 会话可查
    {
        let c = standard_nohz();
        let ok = c.ledger_verified();
        s.add("C28-07-账验签", ok, if ok { "" } else { "验签破" });
    }
    // C28-08 封账后操作必拒
    {
        let mut c = standard_nohz();
        let _ = c.seal();
        let st = c.tick_stop(5, 2000);
        let rs = c.tick_resume(2100, 100, 0);
        let ok = st == Err(XC_NZ_SEALED) && rs.err() == Some(XC_NZ_SEALED) && c.reject_count() >= 2;
        s.add("C28-08-封账必拒", ok, if ok { "" } else { "封账失守" });
    }
    // C28-09 指纹一致 + 封账稳定
    {
        let f = || standard_nohz().fingerprint();
        let ok0 = f() == f();
        let mut c = standard_nohz();
        let pre = c.fingerprint();
        let sealed = c.seal();
        s.add(
            "C28-09-指纹封账",
            ok0 && sealed.is_ok() && c.fingerprint() == pre && c.seal().is_err(),
            if ok0 { "" } else { "指纹漂移" },
        );
    }
    // C28-10 缺口会话留痕：缺口非零的会话也在账（不吞账），rejects 同步留痕
    {
        let mut c = NohzCoordinator::new();
        let _ = c.tick_stop(5, 100);
        let r = c.tick_resume(200, 50, 0);
        let ok = match r {
            Ok(row) => row.gaps == 50 && c.reject_count() >= 1 && c.row_count() == 1,
            Err(_) => false,
        };
        s.add("C28-10-缺口留痕", ok, if ok { "" } else { "留痕失守" });
    }
    s
}
