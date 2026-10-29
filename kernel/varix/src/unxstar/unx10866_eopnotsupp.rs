//! UNX-F10866 EOPNOTSUPP 矩阵（listen 面）（0x1c4c0 段）
//!
//! 锚点：UNX-F10866-J1 listen 面矩阵——listen 只许落在 stream 套接
//! 字：dgram 上 listen 必 EOPNOTSUPP（95）；stream 上 listen 必成
//! （backlog 生效，重复 listen 升级 backlog 放行）；accept 只许落
//! 在已 listen 的 stream（未 listen 必 EINVAL=22）。判定依赖场景字
//! 段演算，注入核（dgram 放行/accept 失明坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：监听场景（族 × 监听旗 × backlog）→ EOPNOTSUPP 判定器（族
//! × 旗三支）→ 监听套接字状态机（listen/accept 走账）→ 样本矩阵
//! （八行正反对）→ 注入核（两坏实现必分歧）→ 监听回放（升级对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c4c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c4c0;
pub const OS_NOTSUPP: u32 = SEG_BASE + 0x01; // dgram listen 拒
pub const OS_LISTEN_OK: u32 = SEG_BASE + 0x02; // stream listen 放行面
pub const OS_UPGRADE: u32 = SEG_BASE + 0x03; // 重复 listen 升级面
pub const OS_ACCEPT_MISS: u32 = SEG_BASE + 0x04; // 未 listen accept 拒
pub const OS_GHOST_PASS: u32 = SEG_BASE + 0x05; // dgram 放行冒账
pub const OS_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 监听账失平

/// 语义字面锚。
pub const EOPNOTSUPP: u32 = 95;
pub const EINVAL: u32 = 22;
pub const BACKLOG_DEF: u32 = 16;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        OS_NOTSUPP => "dgram listen 拒",
        OS_LISTEN_OK => "stream listen 放行面",
        OS_UPGRADE => "重复 listen 升级面",
        OS_ACCEPT_MISS => "未 listen accept 拒",
        OS_GHOST_PASS => "dgram 放行冒账",
        OS_LEDGER_LOST => "监听账失平",
        _ => "未知诊断码",
    }
}

/// 套接字族：Stream / Dgram。
#[derive(Clone, PartialEq)]
pub enum Fam {
    Stream,
    Dgram,
}

/// 监听场景：族 × 已监听 backlog × 新 backlog（判定器三输入）。
#[derive(Clone)]
pub struct OsReq {
    pub fam: Fam,
    pub listening: Option<u32>,
    pub backlog: u32,
}

/// 判定结果：Ok(生效 backlog) / Err(码)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Ok(u32),
    Err(u32),
}

/// EOPNOTSUPP 判定器：dgram listen 必拒；stream 未 listen 放行、
/// 已 listen 升级放行（生效 backlog 取大者）。
pub fn judge(req: &OsReq) -> Res {
    if req.fam == Fam::Dgram {
        return Res::Err(OS_NOTSUPP);
    }
    match req.listening {
        None => Res::Ok(req.backlog),
        Some(old) => {
            if req.backlog > old {
                Res::Ok(req.backlog)
            } else {
                Res::Ok(old)
            }
        }
    }
}

/// accept 判定：已 listen 的 stream 才可 accept，否则 EINVAL。
pub fn judge_accept(fam: &Fam, listening: Option<u32>) -> Res {
    match listening {
        Some(b) if *fam == Fam::Stream => Res::Ok(b),
        Some(_) => Res::Err(OS_NOTSUPP),
        None => Res::Err(EINVAL),
    }
}

/// 监听套接字状态机：族、监听 backlog、accept 账。
pub struct ListenSock {
    pub fam: Fam,
    pub listening: Option<u32>,
    pub listens: u32,
    pub accepts: u32,
    pub rejects: u32,
    pub note: String,
}

impl ListenSock {
    pub fn new(fam: Fam) -> ListenSock {
        ListenSock {
            fam,
            listening: None,
            listens: 0,
            accepts: 0,
            rejects: 0,
            note: "fresh".to_string(),
        }
    }

    /// listen：判定走账（放行落 backlog / 拒账落笔）。
    pub fn listen(&mut self, backlog: u32) -> Res {
        let req = OsReq { fam: self.fam.clone(), listening: self.listening, backlog };
        match judge(&req) {
            Res::Ok(b) => {
                self.listening = Some(b);
                self.listens += 1;
                self.note = "listening".to_string();
                Res::Ok(b)
            }
            Res::Err(c) => {
                self.rejects += 1;
                self.note = "eopnotsupp".to_string();
                Res::Err(c)
            }
        }
    }

    /// accept：判定走账（已 listen 落账 / 拒账落笔）。
    pub fn accept(&mut self) -> Res {
        let r = judge_accept(&self.fam, self.listening);
        match &r {
            Res::Ok(_) => {
                self.accepts += 1;
                self.note = "accepted".to_string();
            }
            Res::Err(_) => {
                self.rejects += 1;
                self.note = "accept-deny".to_string();
            }
        }
        r
    }

    /// 监听账一致：dgram 必无监听、stream 监听账可辨。
    pub fn ledger_ok(&self) -> bool {
        if self.fam == Fam::Dgram {
            self.listening.is_none()
        } else {
            self.note != "" && self.listens <= 64
        }
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Ok(n) 编码 n+1，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[OsReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Ok(n) => want == n + 1,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：族 × 已监听/未监听 × backlog 升/降，8 行正反对。
pub fn sample_matrix() -> (Vec<OsReq>, Vec<u32>) {
    let mut reqs: Vec<OsReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 dgram listen → EOPNOTSUPP
    reqs.push(OsReq { fam: Fam::Dgram, listening: None, backlog: 8 });
    wants.push(OS_NOTSUPP);
    // R1 stream 未 listen → 放行 backlog
    reqs.push(OsReq { fam: Fam::Stream, listening: None, backlog: 8 });
    wants.push(9);
    // R2 stream 已 listen 升级 → 取大
    reqs.push(OsReq { fam: Fam::Stream, listening: Some(8), backlog: 32 });
    wants.push(33);
    // R3 stream 已 listen 降级 → 保旧
    reqs.push(OsReq { fam: Fam::Stream, listening: Some(32), backlog: 8 });
    wants.push(33);
    // R4 dgram 已 listen 旗残留仍拒（旗不救族）
    reqs.push(OsReq { fam: Fam::Dgram, listening: Some(8), backlog: 8 });
    wants.push(OS_NOTSUPP);
    // R5 stream 未 listen backlog 0 → 放行 0
    reqs.push(OsReq { fam: Fam::Stream, listening: None, backlog: 0 });
    wants.push(1);
    // R6 stream 已 listen 等值 → 保旧
    reqs.push(OsReq { fam: Fam::Stream, listening: Some(16), backlog: 16 });
    wants.push(17);
    // R7 复核格：dgram listen（默认 backlog）仍拒（误期放行由注入核把守）
    reqs.push(OsReq { fam: Fam::Dgram, listening: None, backlog: BACKLOG_DEF });
    wants.push(OS_NOTSUPP);
    (reqs, wants)
}

/// 注入 A：坏实现 dgram 放行（listen 面误交付 backlog）。
pub fn faulty_ghost_pass(req: &OsReq) -> Res {
    if req.fam == Fam::Dgram {
        Res::Ok(req.backlog)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现 accept 失明（未 listen 误放行）。
pub fn faulty_accept_blind(fam: &Fam, listening: Option<u32>) -> Res {
    if listening.is_none() {
        Res::Ok(BACKLOG_DEF)
    } else {
        judge_accept(fam, listening)
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let dg = OsReq { fam: Fam::Dgram, listening: None, backlog: 8 };
    let d1 = judge(&dg) != faulty_ghost_pass(&dg);
    let d2 = judge_accept(&Fam::Stream, None) != faulty_accept_blind(&Fam::Stream, None);
    [d1, d2]
}

/// 监听回放：stream listen 8 → accept 成 → 升级 32 → accept 仍成。
pub fn listen_replay_ok() -> bool {
    let mut s = ListenSock::new(Fam::Stream);
    let a = s.listen(8);
    let c1 = s.accept();
    let b = s.listen(32);
    let c2 = s.accept();
    a == Res::Ok(8) && c1 == Res::Ok(8) && b == Res::Ok(32) && c2 == Res::Ok(32)
        && s.listening == Some(32) && s.listens == 2 && s.accepts == 2 && s.ledger_ok()
}

/// 拒面回放：dgram listen 拒 + accept 拒（未 listen EINVAL），账面对平。
pub fn deny_replay_ok() -> bool {
    let mut s = ListenSock::new(Fam::Dgram);
    let l = s.listen(8);
    let a = s.accept();
    l == Res::Err(OS_NOTSUPP) && a == Res::Err(EINVAL)
        && s.listening.is_none() && s.rejects == 2 && s.ledger_ok()
}

/// EOPNOTSUPP 注记：判定渲染。
pub fn os_note(r: &Res) -> String {
    match r {
        Res::Ok(b) => format!("listen backlog={}", b),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10866_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10866");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：stream listen 放行 backlog
    let j1 = judge(&OsReq { fam: Fam::Stream, listening: None, backlog: 8 }) == Res::Ok(8);
    s.add("J1-listen 放行", j1, "stream 面 Ok");

    // J2 反路径：dgram listen EOPNOTSUPP
    let j2 = judge(&OsReq { fam: Fam::Dgram, listening: None, backlog: 8 })
        == Res::Err(OS_NOTSUPP);
    s.add("J2-dgram 拒", j2, "EOPNOTSUPP 95");

    // J3 边界：升级取大 / 降级保旧两沿
    let j3 = judge(&OsReq { fam: Fam::Stream, listening: Some(8), backlog: 32 }) == Res::Ok(32)
        && judge(&OsReq { fam: Fam::Stream, listening: Some(32), backlog: 8 }) == Res::Ok(32);
    s.add("J3-升级两沿", j3, "backlog max 语义");

    // J4 旗不救族：dgram 已 listen 旗残留仍拒
    let j4 = judge(&OsReq { fam: Fam::Dgram, listening: Some(8), backlog: 8 })
        == Res::Err(OS_NOTSUPP);
    s.add("J4-旗不救族", j4, "族轴先决");

    // J5 监听回放（升级对平）
    s.add("J5-监听回放", listen_replay_ok(), "listen/accept 账对平");

    // J6 拒面回放（dgram 拒+accept EINVAL）
    s.add("J6-拒面回放", deny_replay_ok(), "双拒落账");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 dgram 放行检出
    let inj = inject_two();
    s.add("J8-dgram 放行检出", inj[0], "listen 冒交付必分歧");

    // J9 注入2 accept 失明检出
    s.add("J9-accept 失明检出", inj[1], "未 listen 误放行必分歧");

    // J10 监听账一致
    let mut sk = ListenSock::new(Fam::Dgram);
    let _ = sk.listen(4);
    s.add("J10-监听账", sk.ledger_ok() && sk.rejects == 1 && sk.listening.is_none(), "dgram 不落监听");

    // J11 字面锚：EOPNOTSUPP=95 / EINVAL=22 / BACKLOG_DEF=16
    let j11 = EOPNOTSUPP == 95 && EINVAL == 22 && BACKLOG_DEF == 16;
    s.add("J11-字面锚", j11, "两码与 backlog 常量");

    // J12 诊断码闭集
    let j12 = diag_text(OS_NOTSUPP) == "dgram listen 拒"
        && diag_text(OS_UPGRADE) == "重复 listen 升级面"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) EOPNOTSUPP 注记落账
    let note = os_note(&judge(&OsReq { fam: Fam::Dgram, listening: None, backlog: 8 }));
    s.add_dyn("J14-EOPNOTSUPP 注记", note.contains("95"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10866_flow() {
        assert_eq!(judge(&OsReq { fam: Fam::Dgram, listening: None, backlog: 8 }), Res::Err(OS_NOTSUPP));
        assert_eq!(judge(&OsReq { fam: Fam::Stream, listening: None, backlog: 8 }), Res::Ok(8));
        assert_eq!(judge(&OsReq { fam: Fam::Stream, listening: Some(8), backlog: 32 }), Res::Ok(32));
        assert_eq!(judge_accept(&Fam::Stream, None), Res::Err(EINVAL));
        assert_eq!(judge_accept(&Fam::Dgram, Some(8)), Res::Err(OS_NOTSUPP));
        assert_eq!(judge_accept(&Fam::Stream, Some(8)), Res::Ok(8));
        let mut s = ListenSock::new(Fam::Stream);
        assert_eq!(s.listen(8), Res::Ok(8));
        assert_eq!(s.listen(32), Res::Ok(32));
        assert_eq!(s.listening, Some(32));
        assert!(s.ledger_ok());
    }

    #[test]
    fn unx10866_checks_green() {
        let s = run_unx10866_checks();
        assert!(s.all_passed(), "UNX-F10866 判据未全绿");
    }
}
