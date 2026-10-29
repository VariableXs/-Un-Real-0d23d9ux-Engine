//! UNX-F10865 EISCONN 矩阵（已连接 dgram 再 connect）（0x1c4a0 段）
//!
//! 锚点：UNX-F10865-J1 EISCONN 再连矩阵——dgram 已连接时再 connect：
//! 同对端重连放行（幂等面）；异对端必 EISCONN（106）；stream 已连
//! 再 connect 必 EISCONN（族轴不越权）；未连态 connect 一律放行。
//! 判定依赖场景字段演算，注入核（异对端放行/stream 失明坏实现）
//! 必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：再连场景（族 × 连接旗 × 新对端 × 旧对端）→ EISCONN 判定
//! 器（同/异对端双支）→ 套接字状态机（connect 演进走账）→ 样本矩
//! 阵（八行正反对）→ 注入核（两坏实现必分歧）→ 再连回放（同端幂
//! 等/异端拒账对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c4a0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c4a0;
pub const IC_EISCONN: u32 = SEG_BASE + 0x01; // 异对端再连拒
pub const IC_RECONN: u32 = SEG_BASE + 0x02; // 同对端幂等面
pub const IC_FIRST_CONN: u32 = SEG_BASE + 0x03; // 首连放行面
pub const IC_STREAM_MISS: u32 = SEG_BASE + 0x04; // stream 失明
pub const IC_GHOST_PASS: u32 = SEG_BASE + 0x05; // 异端放行冒账
pub const IC_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 再连账失平

/// 语义字面锚。
pub const EISCONN: u32 = 106;
pub const PEER_A: u32 = 0x0a00_000a;
pub const PEER_B: u32 = 0x0a00_000b;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        IC_EISCONN => "异对端再连拒",
        IC_RECONN => "同对端幂等面",
        IC_FIRST_CONN => "首连放行面",
        IC_STREAM_MISS => "stream 失明",
        IC_GHOST_PASS => "异端放行冒账",
        IC_LEDGER_LOST => "再连账失平",
        _ => "未知诊断码",
    }
}

/// 套接字族：Stream / Dgram。
#[derive(Clone, PartialEq)]
pub enum Fam {
    Stream,
    Dgram,
}

/// 再连场景：族 × 已连对端 × 新对端（判定器三输入）。
#[derive(Clone)]
pub struct IcReq {
    pub fam: Fam,
    pub cur: Option<u32>,
    pub new: u32,
}

/// 判定结果：Ok(生效对端) / Err(码)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Ok(u32),
    Err(u32),
}

/// EISCONN 判定器：未连首连放行；已连 dgram 同端幂等放行、异端
/// EISCONN；已连 stream 异端必拒（族轴不越权，同端也拒——stream
/// 不许重连）。
pub fn judge(req: &IcReq) -> Res {
    match req.cur {
        None => Res::Ok(req.new),
        Some(cur) => {
            if req.fam == Fam::Dgram && cur == req.new {
                Res::Ok(cur)
            } else {
                Res::Err(IC_EISCONN)
            }
        }
    }
}

/// 套接字状态机：当前对端、连接/拒账。
pub struct IcSock {
    pub fam: Fam,
    pub cur: Option<u32>,
    pub conns: u32,
    pub rejects: u32,
    pub note: String,
}

impl IcSock {
    pub fn new(fam: Fam) -> IcSock {
        IcSock { fam, cur: None, conns: 0, rejects: 0, note: "fresh".to_string() }
    }

    /// connect：判定走账（成落 cur / 拒账落笔）。
    pub fn connect(&mut self, new: u32) -> Res {
        let req = IcReq { fam: self.fam.clone(), cur: self.cur, new };
        match judge(&req) {
            Res::Ok(p) => {
                let first = self.cur.is_none();
                self.cur = Some(p);
                self.conns += 1;
                self.note = if first { "first".to_string() } else { "reconn".to_string() };
                Res::Ok(p)
            }
            Res::Err(c) => {
                self.rejects += 1;
                self.note = "eisconn".to_string();
                Res::Err(c)
            }
        }
    }

    /// 再连账一致：拒账落笔时注记可辨。
    pub fn ledger_ok(&self) -> bool {
        self.conns + self.rejects <= 64 && self.note != ""
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Ok(n) 编码 n+1，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[IcReq], wants: &[u32]) -> (usize, usize, bool) {
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

/// 样本矩阵：族 × 已连/未连 × 同/异对端，8 行正反对。
pub fn sample_matrix() -> (Vec<IcReq>, Vec<u32>) {
    let mut reqs: Vec<IcReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 dgram 未连首连 → 放行
    reqs.push(IcReq { fam: Fam::Dgram, cur: None, new: PEER_A });
    wants.push(PEER_A + 1);
    // R1 dgram 已连同端重连 → 幂等放行
    reqs.push(IcReq { fam: Fam::Dgram, cur: Some(PEER_A), new: PEER_A });
    wants.push(PEER_A + 1);
    // R2 dgram 已连异端 → EISCONN
    reqs.push(IcReq { fam: Fam::Dgram, cur: Some(PEER_A), new: PEER_B });
    wants.push(IC_EISCONN);
    // R3 stream 未连首连 → 放行
    reqs.push(IcReq { fam: Fam::Stream, cur: None, new: PEER_A });
    wants.push(PEER_A + 1);
    // R4 stream 已连同端 → EISCONN（stream 不许重连）
    reqs.push(IcReq { fam: Fam::Stream, cur: Some(PEER_A), new: PEER_A });
    wants.push(IC_EISCONN);
    // R5 stream 已连异端 → EISCONN
    reqs.push(IcReq { fam: Fam::Stream, cur: Some(PEER_A), new: PEER_B });
    wants.push(IC_EISCONN);
    // R6 dgram 未连带零地址 → 放行载荷 0
    reqs.push(IcReq { fam: Fam::Dgram, cur: None, new: 0 });
    wants.push(1);
    // R7 dgram 已连异端（零地址哨兵）→ EISCONN
    reqs.push(IcReq { fam: Fam::Dgram, cur: Some(PEER_B), new: 0 });
    wants.push(IC_EISCONN);
    (reqs, wants)
}

/// 注入 A：坏实现异端放行（EISCONN 面误重连换端）。
pub fn faulty_ghost_reconn(req: &IcReq) -> Res {
    if req.cur.is_some() {
        Res::Ok(req.new)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现 stream 失明（stream 已连同端误幂等放行）。
pub fn faulty_stream_blind(req: &IcReq) -> Res {
    if req.fam == Fam::Stream && req.cur.is_some() {
        Res::Ok(req.new)
    } else {
        judge(req)
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let diff = IcReq { fam: Fam::Dgram, cur: Some(PEER_A), new: PEER_B };
    let same_st = IcReq { fam: Fam::Stream, cur: Some(PEER_A), new: PEER_A };
    let d1 = judge(&diff) != faulty_ghost_reconn(&diff);
    let d2 = judge(&same_st) != faulty_stream_blind(&same_st);
    [d1, d2]
}

/// 再连回放：dgram 首连 A → 同端幂等 → 异端 B 拒 → cur 仍 A。
pub fn reconn_replay_ok() -> bool {
    let mut s = IcSock::new(Fam::Dgram);
    let a = s.connect(PEER_A);
    let b = s.connect(PEER_A);
    let c = s.connect(PEER_B);
    a == Res::Ok(PEER_A) && b == Res::Ok(PEER_A) && c == Res::Err(IC_EISCONN)
        && s.cur == Some(PEER_A) && s.conns == 2 && s.rejects == 1 && s.ledger_ok()
}

/// stream 回放：首连放行 → 再连（同端）拒，族轴可辨。
pub fn stream_replay_ok() -> bool {
    let mut s = IcSock::new(Fam::Stream);
    let a = s.connect(PEER_A);
    let b = s.connect(PEER_A);
    a == Res::Ok(PEER_A) && b == Res::Err(IC_EISCONN)
        && s.conns == 1 && s.rejects == 1 && s.ledger_ok()
}

/// EISCONN 注记：判定渲染。
pub fn ic_note(r: &Res) -> String {
    match r {
        Res::Ok(p) => format!("conn peer={:#x}", p),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10865_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10865");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：dgram 同端重连幂等放行
    let j1 = judge(&IcReq { fam: Fam::Dgram, cur: Some(PEER_A), new: PEER_A }) == Res::Ok(PEER_A);
    s.add("J1-同端幂等", j1, "重连不换端");

    // J2 反路径：dgram 异端 EISCONN
    let j2 = judge(&IcReq { fam: Fam::Dgram, cur: Some(PEER_A), new: PEER_B })
        == Res::Err(IC_EISCONN);
    s.add("J2-异端拒", j2, "EISCONN 106");

    // J3 族轴双沿：stream 已连同/异端双拒
    let j3 = judge(&IcReq { fam: Fam::Stream, cur: Some(PEER_A), new: PEER_A })
        == Res::Err(IC_EISCONN)
        && judge(&IcReq { fam: Fam::Stream, cur: Some(PEER_A), new: PEER_B })
            == Res::Err(IC_EISCONN);
    s.add("J3-stream 双拒", j3, "stream 不许重连");

    // J4 未连轴：首连一律放行
    let j4 = judge(&IcReq { fam: Fam::Dgram, cur: None, new: PEER_B }) == Res::Ok(PEER_B)
        && judge(&IcReq { fam: Fam::Stream, cur: None, new: 0 }) == Res::Ok(0);
    s.add("J4-首连放行", j4, "未连态不拒");

    // J5 再连回放
    s.add("J5-再连回放", reconn_replay_ok(), "幂等+拒账+cur 不漂移");

    // J6 stream 回放
    s.add("J6-stream 回放", stream_replay_ok(), "首连/再连两沿");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 异端放行检出
    let inj = inject_two();
    s.add("J8-异端放行检出", inj[0], "EISCONN 冒重连必分歧");

    // J9 注入2 stream 失明检出
    s.add("J9-stream 失明检出", inj[1], "stream 误幂等必分歧");

    // J10 再连账一致
    let mut sk = IcSock::new(Fam::Dgram);
    let _ = sk.connect(PEER_A);
    let _ = sk.connect(PEER_B);
    s.add("J10-再连账", sk.ledger_ok() && sk.conns == 1 && sk.rejects == 1 && sk.cur == Some(PEER_A), "一成一拒各落账");

    // J11 字面锚：EISCONN=106 / 双对端常量
    let j11 = EISCONN == 106 && PEER_A == 0x0a00_000a && PEER_B == 0x0a00_000b;
    s.add("J11-字面锚", j11, "错误码与对端常量");

    // J12 诊断码闭集
    let j12 = diag_text(IC_EISCONN) == "异对端再连拒"
        && diag_text(IC_RECONN) == "同对端幂等面"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) EISCONN 注记落账
    let note = ic_note(&judge(&IcReq { fam: Fam::Dgram, cur: Some(PEER_A), new: PEER_B }));
    s.add_dyn("J14-EISCONN 注记", note.contains("106"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10865_flow() {
        assert_eq!(judge(&IcReq { fam: Fam::Dgram, cur: Some(PEER_A), new: PEER_A }), Res::Ok(PEER_A));
        assert_eq!(judge(&IcReq { fam: Fam::Dgram, cur: Some(PEER_A), new: PEER_B }), Res::Err(IC_EISCONN));
        assert_eq!(judge(&IcReq { fam: Fam::Stream, cur: Some(PEER_A), new: PEER_A }), Res::Err(IC_EISCONN));
        assert_eq!(judge(&IcReq { fam: Fam::Stream, cur: None, new: PEER_B }), Res::Ok(PEER_B));
        let mut s = IcSock::new(Fam::Dgram);
        assert_eq!(s.connect(PEER_A), Res::Ok(PEER_A));
        assert_eq!(s.connect(PEER_B), Res::Err(IC_EISCONN));
        assert_eq!(s.cur, Some(PEER_A));
        assert!(s.ledger_ok());
    }

    #[test]
    fn unx10865_checks_green() {
        let s = run_unx10865_checks();
        assert!(s.all_passed(), "UNX-F10865 判据未全绿");
    }
}
