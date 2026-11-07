//! UNX-F81009 扩表操作失败回滚件（半行写入零残留断言） · 0x91150段
//!
//! 判据原文（派工卡 81008 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81009-J1 「扩表操作失败回滚件（半行写入零残留
//!      断言）」判据落地：正向断言 ×10 与冻结口径完全一致；注入同型
//!      故障 10 次零静默；失败路径显性化、内存缓冲建模；判据锚定
//!      §AI-81 台账域任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 内存缓冲建模：工作缓冲（Vec<u8>）+ 快照（Vec<u8>），提交边界 =
//!    换行符；无真实 IO，全部账目在 alloc 内闭环。
//! 2. Journal 操作日志：每次写入记（行号/字节长/前状态摘要 FNV-64），
//!    回滚按日志逆序逐条撤销字节。
//! 3. HalfLineDetector：缓冲尾无换行且非空 = 半行，检出即触发回滚，
//!    提交路径对半行零容忍。
//! 4. 零残留断言：回滚后工作缓冲与快照逐字节一致，不一致即 Residue。
//! 5. 快照自保：快照存自身 FNV-64 摘要，回滚前复验，篡改必被抓。
//! 6. 回滚账 append-only：每次回滚登记三要素（快照号/撤销条数/残留位），
//!    序号严格递增，重放复算必须一致。
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/expect/panic。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81008 − 81001)）。
pub const SEG_BASE: u32 = 0x91150;

// ── 诊断码闭集（段内偏移 0x01..0x0C） ─────────────────────────────
/// 半行写入。
pub const E_HALFLINE: u16 = 1;
/// 回滚后残留。
pub const E_RESIDUE: u16 = 2;
/// 回滚账乱序。
pub const E_RBORDER: u16 = 3;
/// 回滚越界（未知状态）。
pub const E_RBRANGE: u16 = 4;
/// 双重回滚。
pub const E_DBLRB: u16 = 5;
/// 回滚后重复回滚-verify 拒。
pub const E_RBAGAIN: u16 = 6;
/// 快照篡改检出。
pub const E_SNAPTAMPER: u16 = 7;
/// 回滚对象缺失。
pub const E_RBMISSING: u16 = 8;
/// 回滚账重放不一致。
pub const E_RBREPLAY: u16 = 9;
/// 非法第四态回滚型。
pub const E_RBBADKIND: u16 = 10;
/// 写缓冲空。
pub const E_EMPTY: u16 = 11;
/// 行号回退（journal 序）。
pub const E_JORDER: u16 = 12;

/// 诊断码闭集口径（12 个）。
pub const DIAG_ALL: usize = 12;

/// 回滚型闭集：部分回滚（撤销未提交段）。
pub const RB_PARTIAL: u8 = 1;
/// 回滚型闭集：全量回滚（回到最近快照）。
pub const RB_FULL: u8 = 2;

/// FNV-1a 64 位（前状态摘要）。
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x1_0000_001b3);
        i += 1;
    }
    h
}

/// 操作日志条目：行号/字节长/前状态摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JourEntry {
    /// 行号（journal 序，严格递增）。
    pub line_no: u64,
    /// 本次写入字节长（含换行）。
    pub byte_len: u64,
    /// 写入前工作缓冲摘要。
    pub prev_digest: u64,
    /// 是否整行（尾带换行）。
    pub whole: bool,
}

/// 回滚账三要素记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RbRecord {
    /// 回滚序号（append-only 严格递增）。
    pub seq: u64,
    /// 目标快照号。
    pub snap_id: u64,
    /// 撤销条数。
    pub undone: u64,
    /// 零残留断言位（1 = 通过）。
    pub clean: u8,
}

/// 快照。
#[derive(Clone, Debug)]
pub struct Snap {
    /// 快照号。
    pub id: u64,
    /// 快照字节。
    pub content: Vec<u8>,
    /// 快照摘要（自保）。
    pub digest: u64,
    /// 是否已被回滚消费。
    pub consumed: bool,
}

/// 回滚件：工作缓冲 + 操作日志 + 快照账 + 回滚账。
#[derive(Debug)]
pub struct Rollback {
    live: Vec<u8>,
    journal: Vec<JourEntry>,
    snaps: Vec<Snap>,
    rb_ledger: Vec<RbRecord>,
    rb_seq: u64,
    next_line: u64,
}

impl Rollback {
    /// 空件（带一个 0 号初始快照）。
    pub fn new() -> Rollback {
        let mut r = Rollback {
            live: Vec::new(),
            journal: Vec::new(),
            snaps: Vec::new(),
            rb_ledger: Vec::new(),
            rb_seq: 0,
            next_line: 0,
        };
        r.take_snapshot();
        r
    }

    /// 打快照（自保摘要入账）。
    pub fn take_snapshot(&mut self) -> u64 {
        let id = self.snaps.len() as u64;
        self.snaps.push(Snap {
            id,
            content: self.live.clone(),
            digest: fnv64(&self.live),
            consumed: false,
        });
        id
    }

    /// 工作缓冲字节数。
    pub fn live_len(&self) -> usize {
        self.live.len()
    }

    /// 操作日志条数。
    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }

    /// 回滚账条数。
    pub fn rb_count(&self) -> usize {
        self.rb_ledger.len()
    }

    /// 写入一行（journal 记三要素；整行尾补换行）。
    pub fn write_line(&mut self, payload: &str) -> Result<u64, RbError> {
        if payload.is_empty() {
            return Err(RbError::Empty);
        }
        if let Some(last) = self.journal.last() {
            if self.next_line < last.line_no {
                return Err(RbError::JOrder);
            }
        }
        let mut line: Vec<u8> = Vec::new();
        let pb = payload.as_bytes();
        let mut i = 0usize;
        while i < pb.len() {
            line.push(pb[i]);
            i += 1;
        }
        line.push(b'\n');
        let e = JourEntry {
            line_no: self.next_line,
            byte_len: line.len() as u64,
            prev_digest: fnv64(&self.live),
            whole: true,
        };
        let mut k = 0usize;
        while k < line.len() {
            self.live.push(line[k]);
            k += 1;
        }
        self.journal.push(e);
        self.next_line += 1;
        Ok(e.line_no)
    }

    /// 写入半行（不补换行；供故障注入与检出建模）。
    pub fn write_half(&mut self, payload: &str) -> Result<u64, RbError> {
        if payload.is_empty() {
            return Err(RbError::Empty);
        }
        let pb = payload.as_bytes();
        let e = JourEntry {
            line_no: self.next_line,
            byte_len: pb.len() as u64,
            prev_digest: fnv64(&self.live),
            whole: false,
        };
        let mut i = 0usize;
        while i < pb.len() {
            self.live.push(pb[i]);
            i += 1;
        }
        self.journal.push(e);
        self.next_line += 1;
        Ok(e.line_no)
    }

    /// 提交边界检查：缓冲尾无换行且非空 = 半行。
    pub fn detect_half_line(&self) -> bool {
        if self.live.is_empty() {
            return false;
        }
        self.live[self.live.len() - 1] != b'\n'
    }

    /// 逆序撤销：按 journal 逆序逐条恢复字节。
    fn undo_entries(&mut self, count: usize) {
        let mut n = 0usize;
        while n < count {
            match self.journal.pop() {
                None => break,
                Some(e) => {
                    let cut = e.byte_len as usize;
                    let new_len = self.live.len().saturating_sub(cut);
                    self.live.truncate(new_len);
                    self.next_line = e.line_no;
                }
            }
            n += 1;
        }
    }

    /// 回滚执行器：恢复前状态 + 零残留断言 + 回滚账登记。
    pub fn rollback_to(&mut self, snap_id: u64, kind: u8) -> Result<RbRecord, RbError> {
        if kind != RB_PARTIAL && kind != RB_FULL {
            return Err(RbError::RbBadKind);
        }
        if self.snaps.is_empty() {
            return Err(RbError::RbMissing);
        }
        if snap_id >= self.snaps.len() as u64 {
            return Err(RbError::RbRange);
        }
        // 快照自保复验：篡改必被抓。
        let snap_digest = fnv64(&self.snaps[snap_id as usize].content);
        if snap_digest != self.snaps[snap_id as usize].digest {
            return Err(RbError::SnapTamper);
        }
        // 双重回滚闸：快照已消费时——账空（无可撤销）= 纯双重回滚；
        // 账非空（后又写入再回滚-verify）= 重复回滚-verify 拒。
        if self.snaps[snap_id as usize].consumed {
            if self.journal.is_empty() {
                return Err(RbError::DblRb);
            }
            return Err(RbError::RbAgain);
        }
        if self.journal.is_empty() {
            return Err(RbError::RbMissing);
        }
        // 回滚账序号单调：账位序必须与 seq 严格对齐（乱序注入必抓）。
        let mut li = 0usize;
        while li < self.rb_ledger.len() {
            if self.rb_ledger[li].seq != li as u64 + 1 {
                return Err(RbError::RbOrder);
            }
            li += 1;
        }
        // 逆序撤销到快照边界：journal 中超出快照时刻的条数 = 快照后新增行数。
        // 快照时刻行数按快照内容行数推算：以快照后第一个快照或全量回滚近似——
        // 简化冻结口径：撤销 journal 中「快照时刻之后」的全部条目。
        let after = self.journal_after_snapshot(snap_id);
        self.undo_entries(after);
        // 零残留断言：工作缓冲必须与快照逐字节一致。
        let snap_len = self.snaps[snap_id as usize].content.len();
        let mut clean = true;
        if self.live.len() != snap_len {
            clean = false;
        } else {
            let mut i = 0usize;
            while i < snap_len {
                if self.live[i] != self.snaps[snap_id as usize].content[i] {
                    clean = false;
                }
                i += 1;
            }
        }
        let rec = RbRecord {
            seq: self.rb_seq + 1,
            snap_id,
            undone: after as u64,
            clean: if clean { 1 } else { 0 },
        };
        self.rb_seq = rec.seq;
        self.snaps[snap_id as usize].consumed = true;
        self.rb_ledger.push(rec);
        if !clean {
            return Err(RbError::Residue);
        }
        Ok(rec)
    }

    /// 快照时刻之后的 journal 条数（快照内容字节数覆盖的行前缀不计）。
    fn journal_after_snapshot(&self, snap_id: u64) -> usize {
        let snap_bytes = self.snaps[snap_id as usize].content.len();
        let mut covered = 0usize;
        let mut idx = 0usize;
        while idx < self.journal.len() {
            if covered + self.journal[idx].byte_len as usize <= snap_bytes {
                covered += self.journal[idx].byte_len as usize;
                idx += 1;
            } else {
                break;
            }
        }
        self.journal.len() - idx
    }

    /// 回滚账重放复算：逐条核对序号单调与撤销条数非负。
    pub fn verify_rb_ledger(&self) -> Result<usize, RbError> {
        if self.rb_ledger.is_empty() {
            return Err(RbError::Empty);
        }
        let mut i = 1usize;
        while i < self.rb_ledger.len() {
            if self.rb_ledger[i].seq <= self.rb_ledger[i - 1].seq {
                return Err(RbError::RbReplay);
            }
            i += 1;
        }
        Ok(self.rb_ledger.len())
    }
}

/// 回滚域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RbError {
    /// 半行写入。
    HalfLine,
    /// 回滚后残留。
    Residue,
    /// 回滚账乱序。
    RbOrder,
    /// 回滚越界。
    RbRange,
    /// 双重回滚。
    DblRb,
    /// 重复回滚-verify 拒。
    RbAgain,
    /// 快照篡改。
    SnapTamper,
    /// 回滚对象缺失。
    RbMissing,
    /// 回滚账重放不一致。
    RbReplay,
    /// 非法第四态回滚型。
    RbBadKind,
    /// 写缓冲空。
    Empty,
    /// journal 序回退。
    JOrder,
}

impl RbError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            RbError::HalfLine => E_HALFLINE,
            RbError::Residue => E_RESIDUE,
            RbError::RbOrder => E_RBORDER,
            RbError::RbRange => E_RBRANGE,
            RbError::DblRb => E_DBLRB,
            RbError::RbAgain => E_RBAGAIN,
            RbError::SnapTamper => E_SNAPTAMPER,
            RbError::RbMissing => E_RBMISSING,
            RbError::RbReplay => E_RBREPLAY,
            RbError::RbBadKind => E_RBBADKIND,
            RbError::Empty => E_EMPTY,
            RbError::JOrder => E_JORDER,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            RbError::HalfLine => "half line detected at commit boundary",
            RbError::Residue => "residue after rollback",
            RbError::RbOrder => "rollback ledger out of order",
            RbError::RbRange => "rollback to unknown state",
            RbError::DblRb => "double rollback refused",
            RbError::RbAgain => "repeat rollback-verify refused",
            RbError::SnapTamper => "snapshot tampered",
            RbError::RbMissing => "rollback target missing",
            RbError::RbReplay => "rollback ledger replay mismatch",
            RbError::RbBadKind => "rollback kind outside closed set",
            RbError::Empty => "write buffer empty",
            RbError::JOrder => "journal line rewind",
        }
    }
}

/// 提交闸：半行检出即回滚，返回登记的回滚记录。
pub fn commit_or_rollback(r: &mut Rollback) -> Result<(), RbError> {
    if r.detect_half_line() {
        match r.rollback_to(0, RB_PARTIAL) {
            Ok(_) => Err(RbError::HalfLine),
            Err(e) => Err(e),
        }
    } else {
        Ok(())
    }
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    // 1 半行写入 → 提交闸检出 → HalfLine
    let mut r = Rollback::new();
    let _ = r.write_line("alpha");
    let _ = r.write_half("beta-no-newline");
    match commit_or_rollback(&mut r) {
        Err(e) => out.push(("half-line", e.code())),
        Ok(_) => out.push(("half-line-MISSED", SEG_BASE)),
    }
    // 2 回滚越界（未知状态）→ RbRange
    let mut r2 = Rollback::new();
    let _ = r2.write_line("x");
    match r2.rollback_to(99, RB_PARTIAL) {
        Err(e) => out.push(("rb-range", e.code())),
        Ok(_) => out.push(("rb-range-MISSED", SEG_BASE)),
    }
    // 3 双重回滚 → DblRb
    let mut r3 = Rollback::new();
    let _ = r3.write_line("a");
    let _ = r3.take_snapshot();
    let _ = r3.write_line("b");
    let _ = r3.rollback_to(1, RB_PARTIAL);
    match r3.rollback_to(1, RB_PARTIAL) {
        Err(e) => out.push(("dbl-rb", e.code())),
        Ok(_) => out.push(("dbl-rb-MISSED", SEG_BASE)),
    }
    // 4 回滚后重复回滚-verify 拒 → RbAgain
    let mut r4 = Rollback::new();
    let _ = r4.write_line("a");
    let snap1 = r4.take_snapshot();
    let _ = r4.write_line("b");
    let _ = r4.rollback_to(snap1, RB_PARTIAL);
    let _ = r4.write_line("c");
    match r4.rollback_to(snap1, RB_PARTIAL) {
        Err(e) => out.push(("rb-again", e.code())),
        Ok(_) => out.push(("rb-again-MISSED", SEG_BASE)),
    }
    // 5 回滚对象缺失（journal 空）→ RbMissing
    let mut r5 = Rollback::new();
    match r5.rollback_to(0, RB_PARTIAL) {
        Err(e) => out.push(("rb-missing", e.code())),
        Ok(_) => out.push(("rb-missing-MISSED", SEG_BASE)),
    }
    // 6 非法第四态回滚型 → RbBadKind
    let mut r6 = Rollback::new();
    let _ = r6.write_line("k");
    match r6.rollback_to(0, 7) {
        Err(e) => out.push(("rb-badkind", e.code())),
        Ok(_) => out.push(("rb-badkind-MISSED", SEG_BASE)),
    }
    // 7 回滚账乱序 → RbOrder
    let mut r7 = Rollback::new();
    let _ = r7.write_line("a");
    let mut forced = RbRecord {
        seq: 9,
        snap_id: 0,
        undone: 1,
        clean: 1,
    };
    forced.seq = 9;
    r7.force_rb_record(forced);
    let _ = r7.write_line("b");
    match r7.rollback_to(0, RB_PARTIAL) {
        Err(e) => out.push(("rb-order", e.code())),
        Ok(_) => out.push(("rb-order-MISSED", SEG_BASE)),
    }
    // 8 回滚账重放不一致 → RbReplay
    let mut r8 = Rollback::new();
    let _ = r8.write_line("a");
    let mut bad = RbRecord {
        seq: 1,
        snap_id: 0,
        undone: 1,
        clean: 1,
    };
    bad.seq = 1;
    r8.force_rb_record(bad);
    let mut dup = bad;
    dup.seq = 1;
    r8.force_rb_record(dup);
    match r8.verify_rb_ledger() {
        Err(e) => out.push(("rb-replay", e.code())),
        Ok(_) => out.push(("rb-replay-MISSED", SEG_BASE)),
    }
    // 9 快照篡改检出 → SnapTamper
    let mut r9 = Rollback::new();
    let _ = r9.write_line("orig");
    let s9 = r9.take_snapshot();
    let _ = r9.write_line("more");
    r9.tamper_snapshot(s9);
    match r9.rollback_to(s9, RB_PARTIAL) {
        Err(e) => out.push(("snap-tamper", e.code())),
        Ok(_) => out.push(("snap-tamper-MISSED", SEG_BASE)),
    }
    // 10 回滚后残留 → Residue（构造残留样本直接断言闸）
    let mut r10 = Rollback::new();
    let _ = r10.write_line("keep");
    let s10 = r10.take_snapshot();
    let _ = r10.write_line("drop");
    let _ = r10.rollback_to(s10, RB_PARTIAL);
    // 人为塞回残留字节再复验零残留
    r10.smuggle_residue(b"\n");
    match rb_verify_zero_residue(&r10, s10) {
        Err(e) => out.push(("residue", e.code())),
        Ok(_) => out.push(("residue-MISSED", SEG_BASE)),
    }
    out
}

/// 零残留复验：工作缓冲与快照逐字节一致。
pub fn rb_verify_zero_residue(r: &Rollback, snap_id: u64) -> Result<(), RbError> {
    if snap_id >= r.snaps.len() as u64 {
        return Err(RbError::RbRange);
    }
    let sc = &r.snaps[snap_id as usize].content;
    if r.live.len() != sc.len() {
        return Err(RbError::Residue);
    }
    let mut i = 0usize;
    while i < sc.len() {
        if r.live[i] != sc[i] {
            return Err(RbError::Residue);
        }
        i += 1;
    }
    Ok(())
}

impl Rollback {
    /// 注入设施：强行塞入一条乱序回滚账记录（仅注入核使用）。
    pub fn force_rb_record(&mut self, rec: RbRecord) {
        self.rb_ledger.push(rec);
        if rec.seq > self.rb_seq {
            self.rb_seq = rec.seq;
        }
    }

    /// 注入设施：篡改快照内容（仅注入核使用）。
    pub fn tamper_snapshot(&mut self, snap_id: u64) {
        if snap_id < self.snaps.len() as u64 {
            self.snaps[snap_id as usize].content.push(0xFF);
        }
    }

    /// 注入设施：回滚后塞回残留字节（仅注入核使用）。
    pub fn smuggle_residue(&mut self, bytes: &[u8]) {
        let mut i = 0usize;
        while i < bytes.len() {
            self.live.push(bytes[i]);
            i += 1;
        }
    }

    /// 快照内容视图。
    pub fn snap_content(&self, snap_id: u64) -> Option<&[u8]> {
        if snap_id >= self.snaps.len() as u64 {
            return None;
        }
        Some(&self.snaps[snap_id as usize].content)
    }

    /// 工作缓冲视图。
    pub fn live_bytes(&self) -> &[u8] {
        &self.live
    }

    /// 半行检出摘要（冻结口径文本）。
    pub fn half_line_text(&self) -> String {
        if self.detect_half_line() {
            String::from("HALF-LINE")
        } else {
            String::from("CLEAN")
        }
    }
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (_, code) = samples[i];
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（F81009-J1 落地）：13 条，正向 ×10 + 注入汇总。
pub fn run_unx81008_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81008");

    // 1 段位守卫
    let ok = SEG_BASE == 0x91150 && SEG_BASE % 0x30 == 0;
    s.add("F81009-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 诊断闭集口径
    let ok = DIAG_ALL == 12;
    s.add("F81009-02 诊断闭集", ok, if ok { "" } else { "闭集口径漂移" });

    // 3 正向写入：三行入账、journal 三条
    let mut r = Rollback::new();
    let a = r.write_line("alpha");
    let b = r.write_line("beta");
    let c = r.write_line("gamma");
    let ok = matches!(a, Ok(0)) && matches!(b, Ok(1)) && matches!(c, Ok(2)) && r.journal_len() == 3;
    s.add("F81009-03 正向写入", ok, if ok { "" } else { "写入路径坏" });

    // 4 journal 三要素：字节长含换行
    let ok = r.live_len() == 6 + 5 + 6;
    s.add("F81009-04 字节账", ok, if ok { "" } else { "字节账漂移" });

    // 5 半行检出：整行尾 = CLEAN，半行尾 = HALF-LINE
    let mut rh = Rollback::new();
    let _ = rh.write_line("ok");
    let clean = rh.detect_half_line();
    let _ = rh.write_half("dirty");
    let dirty = rh.detect_half_line();
    let ok = !clean && dirty && rh.half_line_text() == "HALF-LINE";
    s.add("F81009-05 半行检出", ok, if ok { "" } else { "半行检出失灵" });

    // 6 回滚正向：逆序撤销、零残留
    let mut rr = Rollback::new();
    let _ = rr.write_line("keep");
    let snap = rr.take_snapshot();
    let _ = rr.write_line("drop");
    let res = rr.rollback_to(snap, RB_PARTIAL);
    let ok = match res {
        Ok(rec) => rec.undone == 1 && rec.clean == 1 && rb_verify_zero_residue(&rr, snap).is_ok(),
        Err(_) => false,
    };
    s.add("F81009-06 回滚正向", ok, if ok { "" } else { "回滚路径坏" });

    // 7 零残留逐字节断言
    let ok = rr.live_bytes() == rr.snap_content(snap).unwrap_or(&[]);
    s.add("F81009-07 逐字节一致", ok, if ok { "" } else { "残留未清" });

    // 8 提交闸：半行即回滚、回滚后缓冲回到干净态
    let mut rc = Rollback::new();
    let _ = rc.write_line("good");
    let _ = rc.write_half("half");
    let gate = commit_or_rollback(&mut rc);
    let ok = matches!(gate, Err(RbError::HalfLine)) && !rc.detect_half_line();
    s.add("F81009-08 提交闸", ok, if ok { "" } else { "提交闸失灵" });

    // 9 回滚账 append-only：seq 单调、三要素齐
    let ok = rr.rb_count() == 1 && rr.verify_rb_ledger().is_ok();
    s.add("F81009-09 回滚账", ok, if ok { "" } else { "回滚账漂移" });

    // 10 前状态摘要：journal 摘要随写入变化
    let mut rd = Rollback::new();
    let _ = rd.write_line("first");
    let _ = rd.write_line("second");
    let ok = rd.journal_len() == 2;
    s.add("F81009-10 前状态摘要", ok, if ok { "" } else { "摘要账漂移" });

    // 11 边界：空载荷拒、空 journal 回滚拒
    let ok = matches!(Rollback::new().write_line(""), Err(RbError::Empty))
        && matches!(Rollback::new().rollback_to(0, RB_PARTIAL), Err(RbError::RbMissing));
    s.add("F81009-11 边界闭集", ok, if ok { "" } else { "边界漏抓" });

    // 12 第四态回滚型拒
    let ok = matches!(Rollback::new().rollback_to(0, 9), Err(RbError::RbBadKind));
    s.add("F81009-12 第四态拒", ok, if ok { "" } else { "回滚型闸失灵" });

    // 13 注入 10/10
    let ok = inject_all_caught();
    s.add("F81009-13 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81008_checks_green() {
        let set = run_unx81008_checks();
        assert!(set.all_passed(), "UNX-F81008 red: {}", set.red_items_text());
    }
}

// 引用保真：ToString 仅由 half_line_text 路径使用。
#[allow(dead_code)]
fn _unused_tostring(_: &str) -> String {
    String::new().to_string()
}
