//! UNX-F10858 termios 并发修改竞态矩阵（0x1c3c0 段）
//!
//! 锚点：UNX-F10858-J1 termios CAS 竞态矩阵——并发写者以期望版本
//! 号提交：版本匹配必成功并推进版本（Ok(v+1)）；版本失配必丢更新
//! （LostUpdate，不落账）；读者快照（version,cflag）二元组原子可辨，
//! 丢更新后旧值不被静默混合（无混合面）。判定依赖场景字段演算，
//! 注入核（失配放行/混合面坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：写场景（期望版本 × 现版本 × 新旗）→ CAS 判定器（匹配/失配
//! 双支）→ termios 状态机（提交/回读走账）→ 样本矩阵（八行正反对）
//! → 注入核（两坏实现必分歧）→ 竞态回放（丢更新→重试对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c3c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c3c0;
pub const TR_CAS_OK: u32 = SEG_BASE + 0x01; // CAS 提交成功
pub const TR_LOST_UPDATE: u32 = SEG_BASE + 0x02; // 丢更新
pub const TR_STALE_READ: u32 = SEG_BASE + 0x03; // 过期读
pub const TR_FLAG_MIX: u32 = SEG_BASE + 0x04; // 旗面混合
pub const TR_RETRIED: u32 = SEG_BASE + 0x05; // 重试成功面
pub const TR_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 版本账失平

/// 语义字面锚。
pub const EAGAIN: u32 = 11;
pub const CS8_RAW: u32 = 0x0030;
pub const CS7_ECHO: u32 = 0x0050;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        TR_CAS_OK => "CAS 提交成功",
        TR_LOST_UPDATE => "丢更新",
        TR_STALE_READ => "过期读",
        TR_FLAG_MIX => "旗面混合",
        TR_RETRIED => "重试成功面",
        TR_LEDGER_LOST => "版本账失平",
        _ => "未知诊断码",
    }
}

/// 写场景：期望版本 × 现版本 × 新旗值（判定器三输入）。
#[derive(Clone)]
pub struct TrReq {
    pub expect: u32,
    pub current: u32,
    pub new_flag: u32,
}

/// 判定结果：Ok(新版本) / Lost / Err(码)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Ok(u32),
    Lost,
    Err(u32),
}

/// CAS 判定器：期望版本匹配现版本才提交；失配必丢更新。
pub fn judge(req: &TrReq) -> Res {
    if req.expect == req.current {
        Res::Ok(req.current + 1)
    } else {
        Res::Lost
    }
}

/// termios 状态机：版本号与旗值原子对、提交/回读账。
pub struct TermiosCell {
    pub cflag: u32,
    pub version: u32,
    pub commits: u32,
    pub losses: u32,
    pub stale_reads: u32,
    pub note: String,
}

impl TermiosCell {
    pub fn new(cflag: u32) -> TermiosCell {
        TermiosCell {
            cflag,
            version: 0,
            commits: 0,
            losses: 0,
            stale_reads: 0,
            note: "init".to_string(),
        }
    }

    /// CAS 提交：版本对平才落旗并推进版本。
    pub fn cas_write(&mut self, expect: u32, new_flag: u32) -> Res {
        let req = TrReq { expect, current: self.version, new_flag };
        match judge(&req) {
            Res::Ok(v) => {
                self.cflag = new_flag;
                self.version = v;
                self.commits += 1;
                self.note = "committed".to_string();
                Res::Ok(v)
            }
            Res::Lost => {
                self.losses += 1;
                self.note = "lost".to_string();
                Res::Lost
            }
            other => other,
        }
    }

    /// 原子快照读：（version,cflag）恒成对；调用方持过期版本比对时
    /// stale_reads 入账。
    pub fn snapshot(&mut self, caller_version: u32) -> (u32, u32) {
        if caller_version != self.version {
            self.stale_reads += 1;
        }
        (self.version, self.cflag)
    }

    /// 版本账一致：commits == version（从 0 起步单调）。
    pub fn ledger_ok(&self) -> bool {
        self.commits == self.version && self.note != ""
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Ok(n) 编码 n+1，Lost 0）命中计数。
pub fn verify_matrix(reqs: &[TrReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Ok(n) => want == n + 1,
            Res::Lost => want == 0,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：匹配/失配 × 版本前后沿，8 行正反对。
pub fn sample_matrix() -> (Vec<TrReq>, Vec<u32>) {
    let mut reqs: Vec<TrReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 匹配 v0 → Ok(v1)
    reqs.push(TrReq { expect: 0, current: 0, new_flag: CS8_RAW });
    wants.push(2);
    // R1 失配（期望 0 现 1）→ Lost
    reqs.push(TrReq { expect: 0, current: 1, new_flag: CS8_RAW });
    wants.push(0);
    // R2 匹配 v3 → Ok(v4)
    reqs.push(TrReq { expect: 3, current: 3, new_flag: CS7_ECHO });
    wants.push(5);
    // R3 失配（期望超前）→ Lost
    reqs.push(TrReq { expect: 5, current: 3, new_flag: CS7_ECHO });
    wants.push(0);
    // R4 匹配 v7 → Ok(v8)
    reqs.push(TrReq { expect: 7, current: 7, new_flag: CS8_RAW });
    wants.push(9);
    // R5 失配（落后一版）→ Lost
    reqs.push(TrReq { expect: 6, current: 7, new_flag: CS8_RAW });
    wants.push(0);
    // R6 匹配 v9 → Ok(v10)
    reqs.push(TrReq { expect: 9, current: 9, new_flag: CS7_ECHO });
    wants.push(11);
    // R7 失配（远落后）→ Lost
    reqs.push(TrReq { expect: 1, current: 9, new_flag: CS7_ECHO });
    wants.push(0);
    (reqs, wants)
}

/// 注入 A：坏实现失配放行（版本失配仍提交）。
pub fn faulty_mismatch_pass(req: &TrReq) -> Res {
    match judge(req) {
        Res::Lost => Res::Ok(req.expect + 1),
        other => other,
    }
}

/// 注入 B：坏实现混合面（失配时静默写新旗，读出混合态）。
pub fn faulty_flag_mix(req: &TrReq) -> Res {
    match judge(req) {
        Res::Lost => Res::Ok(req.current),
        other => other,
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let stale = TrReq { expect: 0, current: 1, new_flag: CS8_RAW };
    let d1 = judge(&stale) != faulty_mismatch_pass(&stale);
    let d2 = judge(&stale) != faulty_flag_mix(&stale);
    [d1, d2]
}

/// 竞态回放：A/B 双写者同基版本竞争——一成一丢，输者重读后重试成。
pub fn race_replay_ok() -> bool {
    let mut cell = TermiosCell::new(CS7_ECHO);
    let a = cell.cas_write(0, CS8_RAW);
    let b = cell.cas_write(0, CS7_ECHO);
    let first = a == Res::Ok(1) && b == Res::Lost && cell.losses == 1;
    let retry = cell.cas_write(cell.version, CS7_ECHO);
    first && retry == Res::Ok(2) && cell.cflag == CS7_ECHO && cell.ledger_ok()
}

/// 快照回放：过期读者比对必失配、快照二元组恒成对。
pub fn snapshot_replay_ok() -> bool {
    let mut cell = TermiosCell::new(CS8_RAW);
    let _ = cell.cas_write(0, CS7_ECHO);
    let (v1, f1) = cell.snapshot(0);
    let (v2, f2) = cell.snapshot(1);
    v1 == 1 && f1 == CS7_ECHO && v2 == 1 && f2 == CS7_ECHO && cell.stale_reads == 1
}

/// 竞态注记：判定渲染。
pub fn tr_note(r: &Res) -> String {
    match r {
        Res::Ok(n) => format!("ok v={}", n),
        Res::Lost => diag_text(TR_LOST_UPDATE).to_string(),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10858_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10858");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：版本匹配提交推进
    let j1 = judge(&TrReq { expect: 0, current: 0, new_flag: CS8_RAW }) == Res::Ok(1);
    s.add("J1-CAS 提交", j1, "匹配必 Ok(v+1)");

    // J2 反路径：版本失配丢更新
    let j2 = judge(&TrReq { expect: 0, current: 1, new_flag: CS8_RAW }) == Res::Lost
        && judge(&TrReq { expect: 2, current: 1, new_flag: CS8_RAW }) == Res::Lost;
    s.add("J2-丢更新", j2, "失配双沿（前/后偏差）全拒");

    // J3 边界：恰好同版提交 / 差一版必丢
    let j3 = judge(&TrReq { expect: 3, current: 3, new_flag: CS7_ECHO }) == Res::Ok(4)
        && judge(&TrReq { expect: 4, current: 3, new_flag: CS7_ECHO }) == Res::Lost;
    s.add("J3-版本两沿", j3, "v3/v4 差一可辨");

    // J4 丢更新不落旗：Lost 后旗值不变（无混合面）
    let mut cell = TermiosCell::new(CS8_RAW);
    let _ = cell.cas_write(0, CS7_ECHO);
    let _ = cell.cas_write(0, 0xffff);
    let flag_kept = cell.cflag == CS7_ECHO && cell.losses == 1;
    s.add("J4-丢更不落旗", flag_kept, "Lost 不改 cflag");

    // J5 竞态回放（一成一丢+重试）
    s.add("J5-竞态回放", race_replay_ok(), "双写竞争重试对平");

    // J6 快照回放（过期读入账）
    s.add("J6-快照回放", snapshot_replay_ok(), "快照成对+过期入账");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 失配放行检出
    let inj = inject_two();
    s.add("J8-失配放行检出", inj[0], "Lost 误 Ok 必分歧");

    // J9 注入2 混合面检出
    s.add("J9-混合面检出", inj[1], "静默写旗必分歧");

    // J10 版本账一致
    let mut cell2 = TermiosCell::new(CS8_RAW);
    let _ = cell2.cas_write(0, CS7_ECHO);
    let _ = cell2.cas_write(1, CS8_RAW);
    s.add("J10-版本账", cell2.ledger_ok() && cell2.version == 2 && cell2.commits == 2, "commits==version");

    // J11 字面锚：EAGAIN=11 / CS8_RAW=0x0030 / CS7_ECHO=0x0050
    let j11 = EAGAIN == 11 && CS8_RAW == 0x0030 && CS7_ECHO == 0x0050;
    s.add("J11-字面锚", j11, "旗值常量");

    // J12 诊断码闭集
    let j12 = diag_text(TR_CAS_OK) == "CAS 提交成功"
        && diag_text(TR_LOST_UPDATE) == "丢更新"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 竞态注记落账
    let note = tr_note(&judge(&TrReq { expect: 2, current: 1, new_flag: CS8_RAW }));
    s.add_dyn("J14-竞态注记", note.contains("丢更新"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10858_flow() {
        assert_eq!(judge(&TrReq { expect: 0, current: 0, new_flag: CS8_RAW }), Res::Ok(1));
        assert_eq!(judge(&TrReq { expect: 0, current: 1, new_flag: CS8_RAW }), Res::Lost);
        let mut cell = TermiosCell::new(CS7_ECHO);
        assert_eq!(cell.cas_write(0, CS8_RAW), Res::Ok(1));
        assert_eq!(cell.cas_write(0, CS7_ECHO), Res::Lost);
        assert_eq!(cell.cas_write(1, CS7_ECHO), Res::Ok(2));
        assert!(cell.ledger_ok());
        assert_eq!(cell.cflag, CS7_ECHO);
    }

    #[test]
    fn unx10858_checks_green() {
        let s = run_unx10858_checks();
        assert!(s.all_passed(), "UNX-F10858 判据未全绿");
    }
}
