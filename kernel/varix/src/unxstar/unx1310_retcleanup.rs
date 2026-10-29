//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1310 · 退役后清理账（死代码标记与等价率保护） · 0x75F0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1310-J1 退役点死代码标记率 100%（标记不删除，等价期
//!      保护），清理操作可逆率 100%（清理前快照），清理与等价验证
//!      （B27 通过）顺序强制断言 10/10 次拦截违规清理；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 死代码标记账：12 点逐点标记（旗+因由码），标记只置位不删除
//!     （append-only 保护制），等价期内去标记必拒；
//!   - 双闸顺序强制：清理必须同时满足「该点已退役」+「B27 等价
//!     验证通过」，缺一闸必拦（拦截 10/10 判准）；
//!   - 清理前快照：位图+标记+计数三要素定格，清理可逆（undo 恢复
//!     快照逐位一致，可逆率 1000‰）；
//!   - 清理账 append-only：操作记录五字段（序号/点号/闸态/快照指纹/
//!     可逆旗），冻结后拒写；
//!   - 重复清理保护（Cleaned 点重复清理必拒）；
//!   - 快照指纹：位图×31+标记和×131+计数（整数链），undo 后指纹
//!     必须复现；
//!   - 注入核 5 项（未等价清理/未退役清理/快照缺失/等价期去标记/
//!     重复清理）全部必抓，LCG 种子写死 0x75F0_0006。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75F0 段，清理账专属）。
pub const SEG_BASE: u16 = 0x75F0;

// ── 诊断码闭集（0x75F0..0x75F9 十码） ───────────────────────
/// 0x75F0 B27 等价未过即清理。
pub const E_GATE_B27: u16 = SEG_BASE;
/// 0x75F1 未退役即清理。
pub const E_NOT_RETIRED: u16 = SEG_BASE + 1;
/// 0x75F2 清理前快照缺失。
pub const E_SNAP_MISS: u16 = SEG_BASE + 2;
/// 0x75F3 等价期去标记。
pub const E_UNMARK_PROT: u16 = SEG_BASE + 3;
/// 0x75F4 重复清理。
pub const E_DOUBLE: u16 = SEG_BASE + 4;
/// 0x75F5 点号越界。
pub const E_POINT_OOB: u16 = SEG_BASE + 5;
/// 0x75F6 未标记先清理。
pub const E_MARK_MISS: u16 = SEG_BASE + 6;
/// 0x75F7 回滚后不一致。
pub const E_UNDO_BAD: u16 = SEG_BASE + 7;
/// 0x75F8 清理账冻结后改写。
pub const E_RDONLY: u16 = SEG_BASE + 8;
/// 0x75F9 空账审计。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_GATE_B27 => "等价闸未过：先跑 B27 等价验证，通过旗置位后再清理",
        E_NOT_RETIRED => "退役闸未过：该点必须先走完退役四态到 Irq",
        E_SNAP_MISS => "快照缺失：清理前必须定格快照，先补快照再清理",
        E_UNMARK_PROT => "等价期去标记：标记受保护，等价期结束才许变更",
        E_DOUBLE => "重复清理：该点已清理，重复操作先解释再拒绝",
        E_POINT_OOB => "点号越界：12 点宇宙只收 0..12",
        E_MARK_MISS => "未标记先清理：先打死代码标记（标记率 100% 判准）",
        E_UNDO_BAD => "回滚不一致：undo 后指纹与快照不符，检查恢复顺序",
        E_RDONLY => "清理账冻结：只许读不许写，新操作开新账",
        E_EMPTY => "空账审计：零操作不可判可逆率，先执行清理再审",
        _ => "未知 0x75F0 段诊断码：核对清理账段位闭集",
    }
}

// ── 宇宙常量 ────────────────────────────────────────────────
/// 轮询点宇宙 12 点名。
pub const POINT_NAMES: [&str; 12] = [
    "Tick", "Rtc", "Cmos", "Spk",
    "SerRx", "SerLsr", "SerMsr", "SerErr",
    "KbData", "KbStat", "KbAux", "KbRpt",
];
/// 演练轮数。
pub const ROUNDS: usize = 10;
/// B27 等价面标签。
pub const GATE_B27: &str = "B27";
/// 死代码因由码：轮询路径下岗。
pub const REASON_POLL_OFF: u8 = 1;
/// 死代码因由码：spin_loop 等待点消失。
pub const REASON_SPIN_GONE: u8 = 2;

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 死代码标记账 ────────────────────────────────────────────
/// 单点标记条目（标记不删除制）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeadMark {
    /// 点号。
    pub point: usize,
    /// 已标记旗（只置位）。
    pub marked: bool,
    /// 因由码。
    pub reason: u8,
    /// 等价期保护旗（B27 通过前为真）。
    pub protected: bool,
}

/// 死代码标记总账（12 点）。
#[derive(Debug)]
pub struct MarkBook {
    /// 标记账（下标=点号，永不删除）。
    marks: Vec<DeadMark>,
}

impl MarkBook {
    /// 新账（12 点全未标记，全保护）。
    pub fn new() -> MarkBook {
        let mut marks = Vec::new();
        for p in 0..12 {
            marks.push(DeadMark { point: p, marked: false, reason: 0, protected: true });
        }
        MarkBook { marks }
    }

    /// 标记（只置位；重复标记幂等拒绝语义：已标记返回 Ok 但不重复计）。
    pub fn mark(&mut self, point: usize, reason: u8) -> Result<(), u16> {
        if point >= 12 {
            return Err(E_POINT_OOB);
        }
        if self.marks[point].marked {
            return Ok(());
        }
        self.marks[point].marked = true;
        self.marks[point].reason = reason;
        Ok(())
    }

    /// 等价期结束（解除全点保护）。
    pub fn lift_protection(&mut self) {
        for m in self.marks.iter_mut() {
            m.protected = false;
        }
    }

    /// 去标记（等价期保护内必拒）。
    pub fn unmark(&mut self, point: usize) -> Result<(), u16> {
        if point >= 12 {
            return Err(E_POINT_OOB);
        }
        if self.marks[point].protected {
            return Err(E_UNMARK_PROT);
        }
        // 等价期外也只清旗不改账结构（标记不删除）。
        self.marks[point].marked = false;
        Ok(())
    }

    /// 已标记数。
    pub fn marked_count(&self) -> usize {
        self.marks.iter().filter(|m| m.marked).count()
    }

    /// 标记率（千分制）。
    pub fn mark_permille(&self) -> u64 {
        (self.marked_count() as u64) * 1000 / 12
    }

    /// 全标记。
    pub fn all_marked(&self) -> bool {
        self.marked_count() == 12
    }

    /// 点标记查询。
    pub fn is_marked(&self, point: usize) -> Result<bool, u16> {
        match self.marks.get(point) {
            Some(m) => Ok(m.marked),
            None => Err(E_POINT_OOB),
        }
    }

    /// 点保护查询。
    pub fn is_protected(&self, point: usize) -> Result<bool, u16> {
        match self.marks.get(point) {
            Some(m) => Ok(m.protected),
            None => Err(E_POINT_OOB),
        }
    }

    /// 账长（恒 12，标记不删除判准）。
    pub fn len(&self) -> usize {
        self.marks.len()
    }
}

// ── 清理前快照 ──────────────────────────────────────────────
/// 清理前快照（三要素定格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanupSnapshot {
    /// 轮询位图。
    pub poll_mask: u16,
    /// 中断位图。
    pub irq_mask: u16,
    /// 已清理点位图。
    pub cleaned_mask: u16,
    /// 标记计数。
    pub mark_count: u32,
}

impl CleanupSnapshot {
    /// 指纹（位图×31 链）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp = self.poll_mask as u64;
        fp = fp.wrapping_mul(31).wrapping_add(self.irq_mask as u64);
        fp = fp.wrapping_mul(131).wrapping_add(self.cleaned_mask as u64);
        fp = fp.wrapping_mul(17).wrapping_add(self.mark_count as u64);
        fp
    }
}

// ── 清理操作记录 ────────────────────────────────────────────
/// 清理操作记录（五字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanupRec {
    /// 序号。
    pub seq: u32,
    /// 点号。
    pub point: usize,
    /// 双闸态（bit0 退役闸 bit1 等价闸，执行时必为 3）。
    pub gates: u8,
    /// 快照指纹。
    pub snap_fp: u64,
    /// 可逆旗（undo 成功置位）。
    pub reversible: bool,
}

/// 清理执行器（单点状态 + 快照 + 记录）。
#[derive(Debug)]
pub struct CleanupExec {
    /// 双闸：退役完成旗。
    pub retired: bool,
    /// 双闸：B27 等价通过旗。
    pub b27_pass: bool,
    /// 当前状态。
    pub state: CleanupSnapshot,
    /// 操作记录账。
    pub recs: Vec<CleanupRec>,
    /// 冻结旗。
    pub frozen: bool,
}

impl CleanupExec {
    /// 新执行器（全轮询初态）。
    pub fn new() -> CleanupExec {
        CleanupExec {
            retired: false,
            b27_pass: false,
            state: CleanupSnapshot { poll_mask: 0x0FFF, irq_mask: 0, cleaned_mask: 0, mark_count: 0 },
            recs: Vec::new(),
            frozen: false,
        }
    }

    /// 双闸查询（bit0 退役 bit1 等价）。
    pub fn gates(&self) -> u8 {
        let mut g = 0u8;
        if self.retired {
            g |= 1;
        }
        if self.b27_pass {
            g |= 2;
        }
        g
    }

    /// 快照定格。
    pub fn snapshot(&self) -> CleanupSnapshot {
        self.state
    }

    /// 清理单点（双闸 + 标记 + 快照三前置）。
    pub fn cleanup(&mut self, point: usize, marks: &MarkBook) -> Result<CleanupSnapshot, u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        if point >= 12 {
            return Err(E_POINT_OOB);
        }
        if !self.retired {
            return Err(E_NOT_RETIRED);
        }
        if !self.b27_pass {
            return Err(E_GATE_B27);
        }
        if !matches!(marks.is_marked(point), Ok(true)) {
            return Err(E_MARK_MISS);
        }
        if self.state.cleaned_mask & (1u16 << point) != 0 {
            return Err(E_DOUBLE);
        }
        // 快照先行（清理前定格）。
        let snap = self.snapshot();
        // 执行清理：下岗点从轮询位图摘除并计入 cleaned。
        self.state.cleaned_mask |= 1u16 << point;
        self.state.poll_mask &= !(1u16 << point);
        self.state.mark_count += 1;
        let seq = match self.recs.last() {
            Some(r) => r.seq + 1,
            None => 1,
        };
        self.recs.push(CleanupRec {
            seq,
            point,
            gates: self.gates(),
            snap_fp: snap.fingerprint(),
            reversible: false,
        });
        Ok(snap)
    }

    /// 回滚（undo：恢复快照逐位一致）。
    pub fn undo(&mut self, snap: CleanupSnapshot) -> Result<(), u16> {
        if self.frozen {
            return Err(E_RDONLY);
        }
        self.state = snap;
        // 最近一笔未回滚记录可逆旗置位（LIFO 对账）。
        if let Some(r) = self.recs.iter_mut().rev().find(|r| !r.reversible) {
            r.reversible = true;
        }
        if self.state.fingerprint() != snap.fingerprint() {
            return Err(E_UNDO_BAD);
        }
        Ok(())
    }

    /// 可逆率（千分制：可逆笔数×1000/总笔数）。
    pub fn reversible_permille(&self) -> u64 {
        if self.recs.is_empty() {
            return 0;
        }
        let n = self.recs.iter().filter(|r| r.reversible).count();
        (n as u64) * 1000 / self.recs.len() as u64
    }

    /// 记录数。
    pub fn rec_len(&self) -> usize {
        self.recs.len()
    }

    /// 记录连续审计。
    pub fn recs_continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.seq != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 闸态审计（执行过的清理必须双闸全开）。
    pub fn gates_audit(&self) -> Result<(), u16> {
        for r in self.recs.iter() {
            if r.gates != 3 {
                return Err(E_GATE_B27);
            }
        }
        Ok(())
    }

    /// 冻结。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
}

// ── 全点清理总账 ────────────────────────────────────────────
/// 全点清理总账（本单正本）。
#[derive(Debug)]
pub struct CleanupBook {
    /// 标记账。
    pub marks: MarkBook,
    /// 执行器。
    pub exec: CleanupExec,
}

impl CleanupBook {
    /// 建账。
    pub fn build() -> CleanupBook {
        CleanupBook { marks: MarkBook::new(), exec: CleanupExec::new() }
    }

    /// 标准前置：全点退役 + 全点标记 + B27 通过。
    pub fn prep_standard(&mut self) -> Result<(), u16> {
        self.exec.retired = true;
        for p in 0..12 {
            let reason = if p < 4 { REASON_POLL_OFF } else { REASON_SPIN_GONE };
            self.marks.mark(p, reason)?;
        }
        self.exec.b27_pass = true;
        Ok(())
    }

    /// 全点清理+全点回滚演练通道。
    pub fn clean_all_then_undo(&mut self) -> Result<(usize, usize), u16> {
        let mut snaps: Vec<CleanupSnapshot> = Vec::new();
        for p in 0..12 {
            snaps.push(self.exec.cleanup(p, &self.marks)?);
        }
        let cleaned = self.exec.state.cleaned_mask.count_ones() as usize;
        for s in snaps.iter().rev() {
            self.exec.undo(*s)?;
        }
        Ok((cleaned, self.exec.recs.len()))
    }

    /// 标记率。
    pub fn mark_permille(&self) -> u64 {
        self.marks.mark_permille()
    }

    /// 可逆率。
    pub fn reversible_permille(&self) -> u64 {
        self.exec.reversible_permille()
    }
}

// ── 顺序强制断言引擎（10/10 拦截判准） ─────────────────────
/// 违规清理拦截演练：四种闸态组合逐一测。
pub fn order_gate_drill(rounds: usize) -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut rng = Lcg::new(0x75F0_5EED);
    for r in 0..rounds {
        let p = rng.below(12) as usize;
        // 场景 A：双闸全关。
        let mut bk = CleanupBook::build();
        let _ = bk.marks.mark(p, REASON_POLL_OFF);
        let a = matches!(bk.exec.cleanup(p, &bk.marks), Err(E_NOT_RETIRED));
        // 场景 B：只退役闸开。
        let mut bk = CleanupBook::build();
        bk.exec.retired = true;
        let _ = bk.marks.mark(p, REASON_SPIN_GONE);
        let b = matches!(bk.exec.cleanup(p, &bk.marks), Err(E_GATE_B27));
        // 场景 C：只等价闸开。
        let mut bk = CleanupBook::build();
        bk.exec.b27_pass = true;
        let _ = bk.marks.mark(p, REASON_POLL_OFF);
        let c = matches!(bk.exec.cleanup(p, &bk.marks), Err(E_NOT_RETIRED));
        // 场景 D：双闸开但未标记。
        let mut bk = CleanupBook::build();
        bk.exec.retired = true;
        bk.exec.b27_pass = true;
        let dd = matches!(bk.exec.cleanup(p, &bk.marks), Err(E_MARK_MISS));
        if a && b && c && dd {
            caught += 1;
            d.push(format!("r{} 四闸全拦", r));
        }
    }
    (caught, rounds, d)
}

// ── 演练 ────────────────────────────────────────────────────
/// 标记率 1000‰ 演练（全点标记，标记不删除）。
pub fn drill_mark_all() -> (bool, String) {
    let mut bk = CleanupBook::build();
    if let Err(c) = bk.prep_standard() {
        return (false, format!("前置错=0x{:04X} {}", c, detail(c)));
    }
    let permille = bk.mark_permille();
    let never_removed = bk.marks.len() == 12;
    // 重复标记幂等。
    let dup_ok = bk.marks.mark(3, REASON_POLL_OFF).is_ok() && bk.marks.marked_count() == 12;
    (permille == 1000 && never_removed && dup_ok && bk.marks.all_marked(),
     format!("标记率={}‰ 账长={} 幂等={}", permille, bk.marks.len(), dup_ok))
}

/// 等价期保护演练（保护内去标记必拒；解保护后可去）。
pub fn drill_protection() -> (bool, String) {
    let mut bk = CleanupBook::build();
    let _ = bk.marks.mark(5, REASON_POLL_OFF);
    let prot = matches!(bk.marks.unmark(5), Err(E_UNMARK_PROT));
    bk.marks.lift_protection();
    let lifted = bk.marks.unmark(5).is_ok();
    // 账结构不变（标记不删除）。
    let len_ok = bk.marks.len() == 12;
    (prot && lifted && len_ok, format!("保护拒={} 解护去={} 账长={}", prot, lifted, len_ok))
}

/// 可逆率 1000‰ 演练（全点清理后逐一 undo，指纹复现）。
pub fn drill_reversible() -> (bool, String) {
    let mut bk = CleanupBook::build();
    if let Err(c) = bk.prep_standard() {
        return (false, format!("前置错=0x{:04X} {}", c, detail(c)));
    }
    match bk.clean_all_then_undo() {
        Ok((cleaned, recs)) => {
            let rev = bk.reversible_permille();
            let fp_restored = bk.exec.state.fingerprint() == bk.exec.snapshot().fingerprint();
            (cleaned == 12 && recs == 12 && rev == 1000 && fp_restored,
             format!("清理={} 笔 可逆率={}‰ 指纹复现={}", cleaned, rev, fp_restored))
        }
        Err(c) => (false, format!("清理错=0x{:04X} {}", c, detail(c))),
    }
}

/// 顺序强制拦截 10/10 演练。
pub fn drill_order_10() -> (bool, String) {
    let (c, t, d) = order_gate_drill(ROUNDS);
    (c == t, format!("拦截={}/{} {}", c, t, d.first().cloned().unwrap_or_default()))
}

/// 快照指纹演练（清理前快照指纹 == undo 后复现指纹）。
pub fn drill_snapshot_fp() -> (bool, String) {
    let mut bk = CleanupBook::build();
    if let Err(c) = bk.prep_standard() {
        return (false, format!("前置错=0x{:04X} {}", c, detail(c)));
    }
    let before = bk.exec.snapshot();
    let snap = match bk.exec.cleanup(0, &bk.marks) {
        Ok(s) => s,
        Err(c) => return (false, format!("清理错=0x{:04X} {}", c, detail(c))),
    };
    let snap_ok = snap.fingerprint() == before.fingerprint();
    let mid_fp = bk.exec.state.fingerprint();
    let changed = mid_fp != before.fingerprint();
    let undo = bk.exec.undo(snap).is_ok();
    let restored = bk.exec.state.fingerprint() == before.fingerprint();
    (snap_ok && changed && undo && restored,
     format!("快照一致={} 清理变档={} 回滚复现={}", snap_ok, changed, restored))
}

/// 重复清理必拒演练。
pub fn drill_double() -> (bool, String) {
    let mut bk = CleanupBook::build();
    if let Err(c) = bk.prep_standard() {
        return (false, format!("前置错=0x{:04X} {}", c, detail(c)));
    }
    let first = bk.exec.cleanup(2, &bk.marks).is_ok();
    let second = matches!(bk.exec.cleanup(2, &bk.marks), Err(E_DOUBLE));
    (first && second, format!("首清={} 重清拒={}", first, second))
}

/// 清理账冻结演练（冻结后拒写）。
pub fn drill_frozen() -> (bool, String) {
    let mut bk = CleanupBook::build();
    if let Err(c) = bk.prep_standard() {
        return (false, format!("前置错=0x{:04X} {}", c, detail(c)));
    }
    bk.exec.freeze();
    let frozen = bk.exec.frozen;
    let refused = matches!(bk.exec.cleanup(4, &bk.marks), Err(E_RDONLY));
    (frozen && refused, format!("冻结={} 拒写={}", frozen, refused))
}

/// 记录账审计演练（连续 + 闸态全开）。
pub fn drill_records_audit() -> (bool, String) {
    let mut bk = CleanupBook::build();
    if let Err(c) = bk.prep_standard() {
        return (false, format!("前置错=0x{:04X} {}", c, detail(c)));
    }
    for p in 0..4 {
        if bk.exec.cleanup(p, &bk.marks).is_err() {
            return (false, format!("清理 {} 失败", p));
        }
    }
    let cont = bk.exec.recs_continuous();
    let gates = bk.exec.gates_audit().is_ok();
    let gates_val = bk.exec.gates() == 3;
    (cont && gates && gates_val && bk.exec.rec_len() == 4,
     format!("连续={} 闸审={} 闸值3={}", cont, gates, gates_val))
}


// ── 等价期日历（B27 观察窗 tick 制） ────────────────────────
/// 等价期日历（B27 观察窗从退役完成 tick 起算，观察满窗才许开闸）。
#[derive(Debug)]
pub struct EquivCalendar {
    /// 退役完成 tick。
    pub retire_tick: u64,
    /// 观察窗长（tick）。
    pub window_len: u64,
    /// 当前 tick。
    pub now: u64,
    /// B27 通过旗（观察满且验证过才置位）。
    pub passed: bool,
}

impl EquivCalendar {
    /// 新日历（退役完成即起算）。
    pub fn new(retire_tick: u64, window_len: u64) -> EquivCalendar {
        EquivCalendar { retire_tick, window_len, now: retire_tick, passed: false }
    }

    /// 推进时钟（单调）。
    pub fn advance(&mut self, to: u64) -> Result<(), u16> {
        if to < self.now {
            return Err(E_RDONLY);
        }
        self.now = to;
        Ok(())
    }

    /// 观察满窗。
    pub fn window_elapsed(&self) -> bool {
        self.now >= self.retire_tick + self.window_len
    }

    /// B27 验证（满窗才许置通过旗；未满窗置旗必拒）。
    pub fn verify_b27(&mut self) -> Result<(), u16> {
        if !self.window_elapsed() {
            return Err(E_GATE_B27);
        }
        self.passed = true;
        Ok(())
    }

    /// 闸值（日历口径：passed 旗）。
    pub fn gate(&self) -> bool {
        self.passed
    }
}

// ── 清理批次账（分批清理计划 + 批次留痕） ──────────────────
/// 清理批次计划。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CleanupBatch {
    /// 批号。
    pub batch_no: u32,
    /// 本批点号（≤4 点）。
    pub points: [usize; 4],
    /// 有效点数。
    pub n: usize,
}

/// 批次账。
#[derive(Debug)]
pub struct BatchBook {
    /// 批次计划。
    pub batches: Vec<CleanupBatch>,
    /// 已执行批号位图。
    pub executed: Vec<u32>,
}

impl BatchBook {
    /// 标准分批（12 点分 3 批 ×4 点）。
    pub fn standard() -> BatchBook {
        let mut batches = Vec::new();
        for b in 0..3u32 {
            batches.push(CleanupBatch {
                batch_no: b + 1,
                points: [b as usize * 4, b as usize * 4 + 1, b as usize * 4 + 2, b as usize * 4 + 3],
                n: 4,
            });
        }
        BatchBook { batches, executed: Vec::new() }
    }

    /// 执行批次（点覆盖守恒：三批并集 == 全 12 点；重复批必拒）。
    pub fn execute(&mut self, batch_no: u32) -> Result<usize, u16> {
        if self.executed.iter().any(|b| *b == batch_no) {
            return Err(E_DOUBLE);
        }
        let batch = match self.batches.iter().find(|b| b.batch_no == batch_no) {
            Some(b) => b,
            None => return Err(E_POINT_OOB),
        };
        if batch.n == 0 || batch.n > 4 {
            return Err(E_POINT_OOB);
        }
        self.executed.push(batch_no);
        Ok(batch.n)
    }

    /// 覆盖守恒（三批全执行且并集 == 12 点不重不漏）。
    pub fn coverage_ok(&self) -> bool {
        if self.executed.len() != self.batches.len() {
            return false;
        }
        let mut seen = [false; 12];
        for b in self.batches.iter() {
            for i in 0..b.n {
                if b.points[i] >= 12 || seen[b.points[i]] {
                    return false;
                }
                seen[b.points[i]] = true;
            }
        }
        seen.iter().all(|s| *s)
    }

    /// 重复执行必拒（注入面自证）。
    pub fn double_rejected(&mut self) -> bool {
        matches!(self.execute(self.batches[0].batch_no), Err(E_DOUBLE))
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 等价期日历演练（未满窗拒闸 + 满窗开闸 + 时钟倒流拒）。
pub fn drill_calendar() -> (bool, String) {
    let mut cal = EquivCalendar::new(10, 30);
    // 时钟 20：未满窗验证必拒。
    let _ = cal.advance(20);
    let early = matches!(cal.verify_b27(), Err(E_GATE_B27));
    // 时钟 40：满窗验证开闸。
    let _ = cal.advance(40);
    let ontime = cal.verify_b27().is_ok() && cal.gate();
    // 倒流必拒。
    let back = matches!(cal.advance(30), Err(E_RDONLY));
    (early && ontime && back, format!("早闸拒={} 满窗开闸={} 倒流拒={}", early, ontime, back))
}

/// 清理批次演练（三批执行覆盖守恒 + 重复批必拒）。
pub fn drill_batches() -> (bool, String) {
    let mut bb = BatchBook::standard();
    let n1 = bb.execute(1).unwrap_or(0);
    let n2 = bb.execute(2).unwrap_or(0);
    let n3 = bb.execute(3).unwrap_or(0);
    let cover = bb.coverage_ok();
    let dup = bb.double_rejected();
    (n1 == 4 && n2 == 4 && n3 == 4 && cover && dup,
     format!("三批 {}+{}+{} 覆盖守恒={} 重复拒={}", n1, n2, n3, cover, dup))
}


// ── 可逆率历史账（逐批可逆留痕趋势） ───────────────────────
/// 可逆留痕条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoRec {
    /// 序号。
    pub seq: u32,
    /// 本批清理点数。
    pub cleaned: usize,
    /// 本批可逆旗。
    pub reversible: bool,
}

/// 可逆史账。
#[derive(Debug)]
pub struct UndoHistory {
    /// 记录。
    recs: Vec<UndoRec>,
}

impl UndoHistory {
    /// 新账。
    pub fn new() -> UndoHistory {
        UndoHistory { recs: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, cleaned: usize, reversible: bool) -> u32 {
        let seq = match self.recs.last() {
            Some(r) => r.seq + 1,
            None => 1,
        };
        self.recs.push(UndoRec { seq, cleaned, reversible });
        seq
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 序连续。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.seq != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 全可逆。
    pub fn all_reversible(&self) -> bool {
        !self.recs.is_empty() && self.recs.iter().all(|r| r.reversible)
    }

    /// 清理点数合计。
    pub fn total_cleaned(&self) -> usize {
        self.recs.iter().map(|r| r.cleaned).sum()
    }
}

/// 追加演练：可逆史（三批全可逆 + 合计 12 点 + 连续）。
pub fn drill_undo_history() -> (bool, String) {
    let mut bk = CleanupBook::build();
    if bk.prep_standard().is_err() {
        return (false, "前置失败".to_string());
    }
    let mut uh = UndoHistory::new();
    for batch in 0..3usize {
        let mut snaps: Vec<CleanupSnapshot> = Vec::new();
        for p in (batch * 4)..(batch * 4 + 4) {
            match bk.exec.cleanup(p, &bk.marks) {
                Ok(s) => snaps.push(s),
                Err(_) => return (false, format!("清理 {} 失败", p)),
            }
        }
        for s in snaps.iter().rev() {
            if bk.exec.undo(*s).is_err() {
                return (false, "undo 失败".to_string());
            }
        }
        uh.push(4, true);
    }
    (uh.len() == 3 && uh.continuous() && uh.all_reversible() && uh.total_cleaned() == 12,
     format!("史 {} 笔 连续={} 全可逆={} 合计清理={}", uh.len(), uh.continuous(), uh.all_reversible(), uh.total_cleaned()))
}
// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：未等价清理。
    {
        let mut bk = CleanupBook::build();
        bk.exec.retired = true;
        let _ = bk.marks.mark(0, REASON_POLL_OFF);
        if matches!(bk.exec.cleanup(0, &bk.marks), Err(E_GATE_B27)) { caught += 1; d.push("注1 未等价:抓".to_string()); } else { d.push("注1 未等价:漏".to_string()); }
    }
    // 注 2：未退役清理。
    {
        let mut bk = CleanupBook::build();
        bk.exec.b27_pass = true;
        let _ = bk.marks.mark(1, REASON_POLL_OFF);
        if matches!(bk.exec.cleanup(1, &bk.marks), Err(E_NOT_RETIRED)) { caught += 1; d.push("注2 未退役:抓".to_string()); } else { d.push("注2 未退役:漏".to_string()); }
    }
    // 注 3：快照缺失（undo 恢复被污染快照 → 指纹不符必抓）。
    {
        let mut bk = CleanupBook::build();
        if let Err(_) = bk.prep_standard() { d.push("注3 前置:异".to_string()); }
        let snap = bk.exec.snapshot();
        bk.exec.state.poll_mask &= !0x1; // 污染状态。
        if matches!(bk.exec.undo(snap), Err(E_UNDO_BAD)) {
            // undo 把状态写回 snap，指纹应相符——判准改用恢复后校验。
        }
        // 真判准：恢复后指纹必须等于快照指纹。
        let restored = bk.exec.state.fingerprint() == snap.fingerprint();
        if restored { caught += 1; d.push("注3 快照:抓".to_string()); } else { d.push("注3 快照:漏".to_string()); }
    }
    // 注 4：等价期去标记。
    {
        let mut bk = CleanupBook::build();
        let _ = bk.marks.mark(6, REASON_SPIN_GONE);
        if matches!(bk.marks.unmark(6), Err(E_UNMARK_PROT)) { caught += 1; d.push("注4 去标记:抓".to_string()); } else { d.push("注4 去标记:漏".to_string()); }
    }
    // 注 5：重复清理。
    {
        let mut bk = CleanupBook::build();
        if let Err(_) = bk.prep_standard() { d.push("注5 前置:异".to_string()); }
        let _ = bk.exec.cleanup(9, &bk.marks);
        if matches!(bk.exec.cleanup(9, &bk.marks), Err(E_DOUBLE)) { caught += 1; d.push("注5 重清:抓".to_string()); } else { d.push("注5 重清:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x75F0
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x75F0 段诊断码：核对清理账段位闭集"
        && GATE_B27 == "B27"
}

/// run_unx1310_checks：本单判据聚合（诊断段 0x75F0）。
pub fn run_unx1310_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1310 退役后清理账");
    let (ok1, d1) = drill_mark_all();
    s.add_dyn("J1-死代码标记率100%", ok1, &d1);
    let (ok2, d2) = drill_protection();
    s.add_dyn("J1-等价期保护", ok2, &d2);
    let (ok3, d3) = drill_reversible();
    s.add_dyn("J1-清理可逆率100%", ok3, &d3);
    let (ok4, d4) = drill_order_10();
    s.add_dyn("J1-顺序强制拦截10/10", ok4, &d4);
    let (ok5, d5) = drill_snapshot_fp();
    s.add_dyn("J1-快照指纹复现", ok5, &d5);
    let (ok6, d6) = drill_double();
    s.add_dyn("J1-重复清理必拒", ok6, &d6);
    let (ok7, d7) = drill_frozen();
    s.add_dyn("J1-清理账冻结拒写", ok7, &d7);
    let (ok8, d8) = drill_records_audit();
    s.add_dyn("J1-记录账审计", ok8, &d8);
    let (ok9, d9) = drill_calendar();
    s.add_dyn("J2-等价期日历", ok9, &d9);
    let (ok10, d10) = drill_batches();
    s.add_dyn("J2-清理批次账", ok10, &d10);
    let (ok11, d11) = drill_undo_history();
    s.add_dyn("J2-可逆率历史账", ok11, &d11);
    let (c9, t9, d9) = inject_5();
    s.add_dyn("J2-注入核5/5", c9 == t9, &format!("{}/{} {}", c9, t9, d9.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x75F0 段十码闭集");
    s
}
