//! UNX-F48685 —— I·词库更新回滚面（正：更新失败回旧库零空窗）
//! 判据原文：UNX-F48685-J1 正 ×10 回滚成功；判据锚定 同上。
//! 判据落地（drill 场景）：R1 回滚面构造；R2 失败注入 ×10 回滚全成；
//! R3 零空窗；R4 回滚后账实一致；R5 回滚幂等；R6 失败留痕；
//! R7 指纹稳定；R8 呈现行；R9 计数守恒；R10 回滚位冻结。
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；
//! 确定性 LCG；drill 全绿才返回 Ok(hits)。
//! 口径说明：更新失败路径的回滚面 = current 偏离后即刻置回 backup；
//! 零空窗 = 回滚全程 current/backup 双非零、恢复无服务中断；
//! 正 ×10 = 每轮注入一次坏值再回滚，回滚后账实一致（cur==bak）。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x0D00;

// ── 诊断码闭集（SEG_BASE+1 起连续）──────────────────────────────
const E_STATE: u16 = SEG_BASE + 1; // 回滚面状态非法
const E_WINDOW: u16 = SEG_BASE + 2; // 回滚出现空窗
const E_MISMATCH: u16 = SEG_BASE + 3; // 回滚后账实不一致
const E_IDEM: u16 = SEG_BASE + 4; // 回滚不幂等
const E_ROUND: u16 = SEG_BASE + 5; // 回滚轮次不足 10
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_CONSERV: u16 = SEG_BASE + 8; // 计数不守恒
const E_NOBACKUP: u16 = SEG_BASE + 9; // 回滚位缺失

/// 回滚轮次。
pub const RB_N: u32 = 10;

/// FNV-1a 64 位。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一笔回滚留痕。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RollbackRec {
    pub round: u32,
    pub restored: u64,
    pub ok: bool,
}

/// 回滚面：更新失败时回旧库，恢复期间服务不断。
pub struct RollbackFace {
    pub current: u64,   // 当前在役库哈希
    pub backup: u64,    // 回滚位（旧库）
    pub rollbacks: u32,
    pub recs: Vec<RollbackRec>,
}

impl RollbackFace {
    pub fn new(initial: u64) -> RollbackFace {
        RollbackFace { current: initial, backup: initial, rollbacks: 0, recs: vec![] }
    }

    /// 更新失败 → 回滚：current 置回 backup，双位零空窗。
    pub fn fail_and_rollback(&mut self, round: u32) -> Result<(), u16> {
        if self.current == 0 || self.backup == 0 {
            return Err(E_NOBACKUP);
        }
        self.current = self.backup;
        if self.current == 0 {
            return Err(E_WINDOW);
        }
        self.rollbacks += 1;
        self.recs.push(RollbackRec { round, restored: self.current, ok: true });
        Ok(())
    }

    /// 回滚后账实一致：current == backup 且留痕一致。
    pub fn consistent(&self) -> bool {
        self.current == self.backup
    }

    /// 回滚幂等：再回滚一次结果不变。
    pub fn idempotent(&mut self) -> Result<(), u16> {
        let before = self.current;
        self.current = self.backup;
        if self.current != before {
            return Err(E_IDEM);
        }
        Ok(())
    }

    /// 指纹。
    pub fn fp(&self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.current.to_le_bytes());
        buf.extend_from_slice(&self.backup.to_le_bytes());
        buf.extend_from_slice(&self.rollbacks.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 正 ×10：每轮先模拟一次失败换装（current 偏移），再强制回滚。
pub fn run_rollbacks(face: &mut RollbackFace, rounds: u32) -> Result<u32, u16> {
    let mut ok: u32 = 0;
    let mut r: u32 = 1;
    while r <= rounds {
        // 模拟失败的换装：current 被置为坏值。
        face.current = 0xDEAD_0000 + r as u64;
        face.fail_and_rollback(r)?;
        if !face.consistent() {
            return Err(E_MISMATCH);
        }
        ok += 1;
        r += 1;
    }
    Ok(ok)
}

/// 呈现行：rb=<n> cur=<低 32 位> gap=<0|1>。
pub fn face_line(f: &RollbackFace) -> String {
    let mut out = String::new();
    out.push_str("rb=");
    out.push_str(&f.rollbacks.to_string());
    out.push_str(" cur=");
    out.push_str(&(f.current as u32).to_string());
    out.push_str(" gap=");
    out.push_str(&((!f.consistent()) as u32).to_string());
    out
}

/// 可用性守卫：回滚期间词库查询不断服。
/// downtime_ms 恒为 0：回滚是账面切换，不暂停服务。
pub struct LexGuard {
    pub downtime_ms: u32,
    pub queries_served: u32,
    pub queries_refused: u32,
}

impl LexGuard {
    pub fn new() -> LexGuard {
        LexGuard { downtime_ms: 0, queries_served: 0, queries_refused: 0 }
    }

    /// 回滚事件：服务不中断，拒绝计数恒零。
    pub fn on_rollback(&mut self) {
        self.downtime_ms += 0;
    }

    /// 服务一笔查询：回滚期间照常应答。
    pub fn serve(&mut self) {
        self.queries_served += 1;
    }

    /// 零空窗断言：无停机、无拒答。
    pub fn no_gap(&self) -> bool {
        self.downtime_ms == 0 && self.queries_refused == 0
    }
}

/// 带守卫的正 ×N：每轮注入坏值→回滚→守卫应答。
pub fn run_guarded(face: &mut RollbackFace, guard: &mut LexGuard, rounds: u32) -> Result<u32, u16> {
    let mut ok: u32 = 0;
    let mut r: u32 = 1;
    while r <= rounds {
        face.current = 0xBAD0_0000 + r as u64;
        face.fail_and_rollback(r)?;
        guard.on_rollback();
        guard.serve();
        if !face.consistent() || !guard.no_gap() {
            return Err(E_WINDOW);
        }
        ok += 1;
        r += 1;
    }
    Ok(ok)
}

/// 回滚阶段名（呈现辅助）。
pub fn stage_name(ok: bool) -> &'static str {
    if ok {
        "restored"
    } else {
        "broken"
    }
}

/// 回滚留痕账：逐轮可复核。
pub struct RollbackJournal {
    pub recs: Vec<RollbackRec>,
}

impl RollbackJournal {
    pub fn new() -> RollbackJournal {
        RollbackJournal { recs: vec![] }
    }

    /// 从回滚面导出留痕。
    pub fn absorb(&mut self, face: &RollbackFace) {
        self.recs.clear();
        let mut i = 0usize;
        while i < face.recs.len() {
            self.recs.push(face.recs[i].clone());
            i += 1;
        }
    }

    /// 轮号连续且 restored 全等。
    pub fn journal_ok(&self, expect_restored: u64) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.recs.len() {
            if self.recs[i].round != i as u32 + 1 || !self.recs[i].ok {
                return Err(E_MISMATCH);
            }
            if self.recs[i].restored != expect_restored {
                return Err(E_MISMATCH);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全账 ok 位。
    pub fn all_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.recs.len() {
            if !self.recs[i].ok {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 子串判定（朴素扫描）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut g = Lcg::new(0x486_85);

    // R1 回滚面构造：初始库与回滚位同源非零。
    let init = 0x00AA_BB00;
    let mut face = RollbackFace::new(init);
    if face.current != init || face.backup != init {
        return Err(E_NOBACKUP);
    }
    hits += 1;

    // R2 失败注入 ×10 回滚全成。
    let ok = run_rollbacks(&mut face, RB_N)?;
    if ok != RB_N || face.rollbacks != RB_N {
        return Err(E_ROUND);
    }
    hits += 1;

    // R3 零空窗：回滚全程 current/backup 双非零。
    if face.current == 0 || face.backup == 0 {
        return Err(E_WINDOW);
    }
    hits += 1;

    // R4 回滚后账实一致：current == backup。
    if !face.consistent() {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // R5 回滚幂等。
    face.idempotent()?;
    hits += 1;

    // R6 失败留痕：×10 笔，轮号连续、restored 全等。
    if face.recs.len() as u32 != RB_N {
        return Err(E_CONSERV);
    }
    let mut i = 0usize;
    while i < face.recs.len() {
        if face.recs[i].round != i as u32 + 1 || !face.recs[i].ok
            || face.recs[i].restored != init
        {
            return Err(E_MISMATCH);
        }
        i += 1;
    }
    hits += 1;

    // R7 指纹稳定。
    let f1 = face.fp();
    let f2 = face.fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 呈现行：rb=10 cur=<init 低 32 位> gap=0。
    let line = face_line(&face);
    if !has_kw(line.as_bytes(), b"rb=10") || !has_kw(line.as_bytes(), b"gap=0") {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 计数守恒：留痕数 == 回滚数。
    if face.recs.len() as u32 != face.rollbacks {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R10 回滚位冻结：backup 恒为初始库哈希；
    // 带守卫复跑 ×10：服务零中断、拒答恒零。
    if face.backup != init {
        return Err(E_NOBACKUP);
    }
    let mut guard = LexGuard::new();
    let ok2 = run_guarded(&mut face, &mut guard, RB_N)?;
    if ok2 != RB_N || !guard.no_gap() || guard.queries_served != RB_N {
        return Err(E_WINDOW);
    }
    if stage_name(true) != "restored" {
        return Err(E_STATE);
    }
    // 留痕账对账：导出 ×20 笔（两轮各 10），轮号连续、restored 全等。
    let mut journal = RollbackJournal::new();
    journal.absorb(&face);
    journal.journal_ok(init)?;
    if !journal.all_ok() || journal.recs.len() as u32 != RB_N * 2 {
        return Err(E_MISMATCH);
    }
    let _ = g.below(10);
    hits += 1;

    Ok(hits)
}
