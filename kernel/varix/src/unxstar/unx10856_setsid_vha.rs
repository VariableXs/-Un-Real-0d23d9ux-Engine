//! UNX-F10856 setsid 释放与 vha（0x1c380 段）
//!
//! 锚点：UNX-F10856-J1 setsid 释放矩阵——调用进程已是进程组长
//! （pid == pgid）必 EPERM（1）；非组长必成新会话（sid=pgid=pid）
//! 且原控制终端释放入 vha 账（released 悬挂账，终端无主可被后续
//! 抢占）；已无 ctty 者再释放为空释放（幂等面，vha 账不增）。判定
//! 依赖场景字段演算，注入核（组长放行/释放冒账坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：会话场景（pid × pgid × ctty）→ setsid 判定器（组长/非组
//! 长双支）→ 会话状态机（释放/悬挂账走账）→ 样本矩阵（八行正反对）
//! → 注入核（两坏实现必分歧）→ 释放回放（vha 悬挂账对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c380 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c380;
pub const SD_EPERM: u32 = SEG_BASE + 0x01; // 组长 setsid 拒
pub const SD_RELEASE: u32 = SEG_BASE + 0x02; // 控制终端释放面
pub const SD_NOOP_FACE: u32 = SEG_BASE + 0x03; // 空释放幂等面
pub const SD_NEW_SID: u32 = SEG_BASE + 0x04; // 新会话面
pub const SD_VHA_LOST: u32 = SEG_BASE + 0x05; // vha 悬挂账丢失
pub const SD_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 会话账失平

/// 语义字面锚。
pub const EPERM: u32 = 1;
pub const CTTY_MARK: u32 = 0x7474_7900; // "tty" 释放标记

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        SD_EPERM => "组长 setsid 拒",
        SD_RELEASE => "控制终端释放面",
        SD_NOOP_FACE => "空释放幂等面",
        SD_NEW_SID => "新会话面",
        SD_VHA_LOST => "vha 悬挂账丢失",
        SD_LEDGER_LOST => "会话账失平",
        _ => "未知诊断码",
    }
}

/// 会话场景：pid × pgid × 控制终端（判定器三输入）。
#[derive(Clone)]
pub struct SessReq {
    pub pid: u32,
    pub pgid: u32,
    pub ctty: Option<u32>,
}

/// 判定结果：Rel(新 sid, 释放的 ctty) / Noop(新 sid) / Err(EPERM)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Rel(u32, u32),
    Noop(u32),
    Err(u32),
}

/// setsid 判定器：组长必拒；非组长成新会话，有 ctty 则释放入账。
pub fn judge(req: &SessReq) -> Res {
    if req.pid == req.pgid {
        return Res::Err(SD_EPERM);
    }
    match req.ctty {
        Some(t) => Res::Rel(req.pid, t),
        None => Res::Noop(req.pid),
    }
}

/// 会话状态机：sid/pgid 演进与 vha 悬挂账。
pub struct Session {
    pub pid: u32,
    pub sid: u32,
    pub pgid: u32,
    pub ctty: Option<u32>,
    pub vha: Vec<u32>,
    pub calls: u32,
    pub note: String,
}

impl Session {
    pub fn new(pid: u32, pgid: u32, ctty: Option<u32>) -> Session {
        Session {
            pid,
            sid: 1,
            pgid,
            ctty,
            vha: Vec::new(),
            calls: 0,
            note: "outer".to_string(),
        }
    }

    /// setsid：判定走账（释放落 vha / 拒落 EPERM 账）。
    pub fn setsid(&mut self) -> Res {
        let req = SessReq { pid: self.pid, pgid: self.pgid, ctty: self.ctty };
        self.calls += 1;
        match judge(&req) {
            Res::Rel(newsid, tty) => {
                self.sid = newsid;
                self.pgid = newsid;
                self.ctty = None;
                self.vha.push(tty | CTTY_MARK);
                self.note = "released".to_string();
                Res::Rel(newsid, tty)
            }
            Res::Noop(newsid) => {
                self.sid = newsid;
                self.pgid = newsid;
                self.note = "no-ctty".to_string();
                Res::Noop(newsid)
            }
            Res::Err(c) => {
                self.note = "eperm".to_string();
                Res::Err(c)
            }
        }
    }

    /// 抢占悬挂终端：vha 首项可被无主进程取走（取后账减）。
    pub fn claim_vha(&mut self) -> Option<u32> {
        if self.vha.is_empty() {
            None
        } else {
            Some(self.vha.remove(0))
        }
    }

    /// 会话账一致：组长态（pid==pgid）必有 sid==1 之外的来源可辨，
    /// 非组长态 sid==pgid==pid。
    pub fn ledger_ok(&self) -> bool {
        if self.pid == self.pgid {
            self.calls > 0
        } else {
            self.sid == self.pgid && self.sid == self.pid
        }
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Rel(n) 编码 n+1，Noop 0，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[SessReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Rel(n, _) => want == n + 1,
            Res::Noop(n) => want == n + 1,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：组长/非组长 × 有/无 ctty，8 行正反对。
pub fn sample_matrix() -> (Vec<SessReq>, Vec<u32>) {
    let mut reqs: Vec<SessReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 组长有 ctty → EPERM
    reqs.push(SessReq { pid: 100, pgid: 100, ctty: Some(4) });
    wants.push(SD_EPERM);
    // R1 组长无 ctty → EPERM
    reqs.push(SessReq { pid: 100, pgid: 100, ctty: None });
    wants.push(SD_EPERM);
    // R2 非组长有 ctty → Rel
    reqs.push(SessReq { pid: 101, pgid: 100, ctty: Some(4) });
    wants.push(102);
    // R3 非组长无 ctty → Noop
    reqs.push(SessReq { pid: 101, pgid: 100, ctty: None });
    wants.push(102);
    // R4 子进程 pid=pgid ≠ 0 组长 → EPERM
    reqs.push(SessReq { pid: 7, pgid: 7, ctty: Some(9) });
    wants.push(SD_EPERM);
    // R5 子进程非组长 → Rel
    reqs.push(SessReq { pid: 8, pgid: 7, ctty: Some(9) });
    wants.push(9);
    // R6 pid=0 哨兵：pgid 也 0（组长）→ EPERM
    reqs.push(SessReq { pid: 0, pgid: 0, ctty: None });
    wants.push(SD_EPERM);
    // R7 非组长无 ctty（0 pid 哨兵不算）→ Noop
    reqs.push(SessReq { pid: 9, pgid: 8, ctty: None });
    wants.push(10);
    (reqs, wants)
}

/// 注入 A：坏实现组长放行（组长 setsid 误成新会话）。
pub fn faulty_leader_pass(req: &SessReq) -> Res {
    if req.pid == req.pgid {
        Res::Noop(req.pid)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现释放冒账（无 ctty 误报 Rel 空 tty）。
pub fn faulty_ghost_release(req: &SessReq) -> Res {
    if req.ctty.is_none() {
        Res::Rel(req.pid, 0)
    } else {
        judge(req)
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let leader = SessReq { pid: 100, pgid: 100, ctty: Some(4) };
    let plain = SessReq { pid: 101, pgid: 100, ctty: None };
    let d1 = judge(&leader) != faulty_leader_pass(&leader);
    let d2 = judge(&plain) != faulty_ghost_release(&plain);
    [d1, d2]
}

/// 释放回放：非组长带 ctty setsid → sid/pgid 对平 + vha 悬挂账入账
/// + 抢占取走后账空。
pub fn release_replay_ok() -> bool {
    let mut s = Session::new(101, 100, Some(4));
    let r = s.setsid();
    let claimed = s.claim_vha();
    r == Res::Rel(101, 4) && s.sid == 101 && s.pgid == 101 && s.ctty.is_none()
        && claimed == Some(4 | CTTY_MARK) && s.vha.is_empty() && s.ledger_ok()
}

/// 幂等回放：新会话再 setsid（pid==pgid 已成组长）必 EPERM。
pub fn idem_replay_ok() -> bool {
    let mut s = Session::new(101, 100, None);
    let r1 = s.setsid();
    let r2 = s.setsid();
    r1 == Res::Noop(101) && r2 == Res::Err(SD_EPERM) && s.calls == 2 && s.ledger_ok()
}

/// 释放注记：判定渲染。
pub fn sd_note(r: &Res) -> String {
    match r {
        Res::Rel(n, t) => format!("rel sid={} tty={}", n, t),
        Res::Noop(n) => format!("noop sid={}", n),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10856_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10856");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：非组长有 ctty 释放
    let j1 = judge(&SessReq { pid: 101, pgid: 100, ctty: Some(4) }) == Res::Rel(101, 4);
    s.add("J1-释放面", j1, "Rel 新会话+释放");

    // J2 反路径：组长必 EPERM
    let j2 = judge(&SessReq { pid: 100, pgid: 100, ctty: Some(4) }) == Res::Err(SD_EPERM)
        && judge(&SessReq { pid: 100, pgid: 100, ctty: None }) == Res::Err(SD_EPERM);
    s.add("J2-组长拒", j2, "EPERM 面 1");

    // J3 边界：无 ctty 幂等面 / pid=0 哨兵组长拒
    let j3 = judge(&SessReq { pid: 101, pgid: 100, ctty: None }) == Res::Noop(101)
        && judge(&SessReq { pid: 0, pgid: 0, ctty: None }) == Res::Err(SD_EPERM);
    s.add("J3-幂等/哨兵", j3, "Noop 与哨兵两沿可辨");

    // J4 释放与旗面无关：Rel 载荷只看 ctty 有无
    let j4 = judge(&SessReq { pid: 8, pgid: 7, ctty: Some(9) }) == Res::Rel(8, 9)
        && judge(&SessReq { pid: 8, pgid: 7, ctty: None }) == Res::Noop(8);
    s.add("J4-ctty 轴", j4, "有/无 ctty 双支可辨");

    // J5 释放回放（vha 悬挂账对平）
    s.add("J5-释放回放", release_replay_ok(), "释放入账+抢占取走");

    // J6 幂等回放（新会话再 setsid 必拒）
    s.add("J6-幂等回放", idem_replay_ok(), "成组长后 EPERM");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 组长放行检出
    let inj = inject_two();
    s.add("J8-组长放行检出", inj[0], "组长误成会话必分歧");

    // J9 注入2 释放冒账检出
    s.add("J9-释放冒账检出", inj[1], "空 tty 误 Rel 必分歧");

    // J10 会话账一致
    let mut sess = Session::new(50, 40, Some(4));
    let _ = sess.setsid();
    s.add("J10-会话账", sess.ledger_ok() && sess.sid == 50 && sess.pgid == 50, "sid==pgid==pid");

    // J11 字面锚：EPERM=1 / CTTY_MARK=0x74747900
    let j11 = EPERM == 1 && CTTY_MARK == 0x7474_7900;
    s.add("J11-字面锚", j11, "错误码与释放标记常量");

    // J12 诊断码闭集
    let j12 = diag_text(SD_EPERM) == "组长 setsid 拒"
        && diag_text(SD_NOOP_FACE) == "空释放幂等面"
        && diag_text(3) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 释放注记落账
    let note = sd_note(&judge(&SessReq { pid: 101, pgid: 100, ctty: Some(4) }));
    s.add_dyn("J14-释放注记", note.contains("sid=101"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10856_flow() {
        assert_eq!(judge(&SessReq { pid: 100, pgid: 100, ctty: Some(4) }), Res::Err(SD_EPERM));
        assert_eq!(judge(&SessReq { pid: 101, pgid: 100, ctty: Some(4) }), Res::Rel(101, 4));
        assert_eq!(judge(&SessReq { pid: 101, pgid: 100, ctty: None }), Res::Noop(101));
        let mut s = Session::new(101, 100, Some(4));
        assert_eq!(s.setsid(), Res::Rel(101, 4));
        assert_eq!(s.setsid(), Res::Err(SD_EPERM));
        assert!(s.ledger_ok());
        assert_eq!(s.claim_vha(), Some(4 | CTTY_MARK));
    }

    #[test]
    fn unx10856_checks_green() {
        let s = run_unx10856_checks();
        assert!(s.all_passed(), "UNX-F10856 判据未全绿");
    }
}
