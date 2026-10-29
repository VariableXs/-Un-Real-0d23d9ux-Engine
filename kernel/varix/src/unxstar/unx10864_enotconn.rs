//! UNX-F10864 ENOTCONN 矩阵（未连接即收发）（0x1c480 段）
//!
//! 锚点：UNX-F10864-J1 ENOTCONN 收发矩阵——stream 套接字未连接时
//! 收发必 ENOTCONN（107）；dgram 未 connect 且无目的地址时 send 必
//! ENOTCONN、sendto 带地址放行（地址轴不越权）；连接建立后收发均
//! 交付。判定依赖场景字段演算，注入核（未连放行/地址轴失明坏实现）
//! 必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：收发场景（连接旗 × 族 × 地址）→ ENOTCONN 判定器（族×地址
//! 二轴）→ 套接字状态机（connect/收发走账）→ 样本矩阵（八行正反对）
//! → 注入核（两坏实现必分歧）→ 连接回放（未连拒→连接后交付对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c480 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c480;
pub const NC_NOTCONN: u32 = SEG_BASE + 0x01; // 未连接收发拒
pub const NC_CONN_FACE: u32 = SEG_BASE + 0x02; // 连接后交付面
pub const NC_DADDR_PASS: u32 = SEG_BASE + 0x03; // dgram 带地址放行面
pub const NC_ADDR_MISS: u32 = SEG_BASE + 0x04; // dgram 无地址拒
pub const NC_GHOST_PASS: u32 = SEG_BASE + 0x05; // 未连放行冒账
pub const NC_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 连接账失平

/// 语义字面锚。
pub const ENOTCONN: u32 = 107;
pub const PEER_ADDR: u32 = 0x0a00_0001;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        NC_NOTCONN => "未连接收发拒",
        NC_CONN_FACE => "连接后交付面",
        NC_DADDR_PASS => "dgram 带地址放行面",
        NC_ADDR_MISS => "dgram 无地址拒",
        NC_GHOST_PASS => "未连放行冒账",
        NC_LEDGER_LOST => "连接账失平",
        _ => "未知诊断码",
    }
}

/// 套接字族：Stream / Dgram。
#[derive(Clone, PartialEq)]
pub enum Fam {
    Stream,
    Dgram,
}

/// 收发场景：族 × 连接旗 × 目的地址（判定器三输入）。
#[derive(Clone)]
pub struct NcReq {
    pub fam: Fam,
    pub connected: bool,
    pub dest: Option<u32>,
}

/// 判定结果：Got(载荷对端) / Err(码)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Err(u32),
}

/// ENOTCONN 判定器：stream 未连必拒；dgram 无连接看地址（带地址放
/// 行、无地址拒）；已连接走交付。
pub fn judge(req: &NcReq) -> Res {
    if req.connected {
        return Res::Got(NC_CONN_FACE);
    }
    match req.fam {
        Fam::Stream => Res::Err(NC_NOTCONN),
        Fam::Dgram => match req.dest {
            Some(a) => Res::Got(a),
            None => Res::Err(NC_NOTCONN),
        },
    }
}

/// 套接字状态机：连接旗、收发账。
pub struct NcSock {
    pub fam: Fam,
    pub connected: bool,
    pub sends: u32,
    pub recvs: u32,
    pub denies: u32,
    pub note: String,
}

impl NcSock {
    pub fn new(fam: Fam) -> NcSock {
        NcSock {
            fam,
            connected: false,
            sends: 0,
            recvs: 0,
            denies: 0,
            note: "unconnected".to_string(),
        }
    }

    /// connect：置连接旗（幂等）。
    pub fn connect(&mut self) {
        if !self.connected {
            self.connected = true;
            self.note = "connected".to_string();
        }
    }

    /// send：判定走账（stream 无地址；dgram 可带地址，载荷直传）。
    pub fn send(&mut self, dest: Option<u32>) -> Res {
        let req = NcReq { fam: self.fam.clone(), connected: self.connected, dest };
        self.sends += 1;
        match judge(&req) {
            Res::Got(n) => {
                self.note = "sent".to_string();
                Res::Got(n)
            }
            Res::Err(c) => {
                self.denies += 1;
                self.note = "enotconn".to_string();
                Res::Err(c)
            }
        }
    }

    /// recv：判定走账（stream 未连必拒）。
    pub fn recv(&mut self) -> Res {
        let req = NcReq { fam: self.fam.clone(), connected: self.connected, dest: None };
        self.recvs += 1;
        match judge(&req) {
            Res::Got(n) => {
                self.note = "recvd".to_string();
                Res::Got(n)
            }
            Res::Err(c) => {
                self.denies += 1;
                self.note = "enotconn".to_string();
                Res::Err(c)
            }
        }
    }

    /// 连接账一致：未连态必有拒账或零调用。
    pub fn ledger_ok(&self) -> bool {
        self.connected || self.note == "unconnected" || self.denies > 0
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Got(n) 编码 n+1，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[NcReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Got(n) => want == n + 1,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：族 × 连接旗 × 地址有无，8 行正反对。
pub fn sample_matrix() -> (Vec<NcReq>, Vec<u32>) {
    let mut reqs: Vec<NcReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 stream 未连 → ENOTCONN
    reqs.push(NcReq { fam: Fam::Stream, connected: false, dest: None });
    wants.push(NC_NOTCONN);
    // R1 stream 已连 → 交付
    reqs.push(NcReq { fam: Fam::Stream, connected: true, dest: None });
    wants.push(NC_CONN_FACE + 1);
    // R2 dgram 未连带地址 → 放行载荷
    reqs.push(NcReq { fam: Fam::Dgram, connected: false, dest: Some(PEER_ADDR) });
    wants.push(PEER_ADDR + 1);
    // R3 dgram 未连无地址 → ENOTCONN
    reqs.push(NcReq { fam: Fam::Dgram, connected: false, dest: None });
    wants.push(NC_NOTCONN);
    // R4 dgram 已连 → 交付
    reqs.push(NcReq { fam: Fam::Dgram, connected: true, dest: None });
    wants.push(NC_CONN_FACE + 1);
    // R5 stream 未连带地址（stream 地址轴不越权）→ ENOTCONN
    reqs.push(NcReq { fam: Fam::Stream, connected: false, dest: Some(PEER_ADDR) });
    wants.push(NC_NOTCONN);
    // R6 stream 已连带地址 → 交付
    reqs.push(NcReq { fam: Fam::Stream, connected: true, dest: Some(PEER_ADDR) });
    wants.push(NC_CONN_FACE + 1);
    // R7 dgram 未连带零地址哨兵 → 放行载荷 0
    reqs.push(NcReq { fam: Fam::Dgram, connected: false, dest: Some(0) });
    wants.push(1);
    (reqs, wants)
}

/// 注入 A：坏实现未连放行（stream 未连误交付）。
pub fn faulty_ghost_pass(req: &NcReq) -> Res {
    if req.fam == Fam::Stream && !req.connected {
        Res::Got(NC_CONN_FACE)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现地址轴失明（dgram 无地址误放行）。
pub fn faulty_addr_blind(req: &NcReq) -> Res {
    if req.fam == Fam::Dgram && !req.connected && req.dest.is_none() {
        Res::Got(NC_CONN_FACE)
    } else {
        judge(req)
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let st = NcReq { fam: Fam::Stream, connected: false, dest: None };
    let dg = NcReq { fam: Fam::Dgram, connected: false, dest: None };
    let d1 = judge(&st) != faulty_ghost_pass(&st);
    let d2 = judge(&dg) != faulty_addr_blind(&dg);
    [d1, d2]
}

/// 连接回放：stream 未连收发双拒 → connect → 收发双交付，账面对平。
pub fn conn_replay_ok() -> bool {
    let mut s = NcSock::new(Fam::Stream);
    let a = s.send(None);
    let b = s.recv();
    s.connect();
    let c = s.send(None);
    let d = s.recv();
    a == Res::Err(NC_NOTCONN) && b == Res::Err(NC_NOTCONN)
        && c == Res::Got(NC_CONN_FACE) && d == Res::Got(NC_CONN_FACE)
        && s.denies == 2 && s.ledger_ok()
}

/// 地址轴回放：dgram 无地址拒、带地址放行载荷、connect 后交付。
pub fn addr_replay_ok() -> bool {
    let mut s = NcSock::new(Fam::Dgram);
    let a = s.send(None);
    let b = s.send(Some(PEER_ADDR));
    s.connect();
    let c = s.send(None);
    a == Res::Err(NC_NOTCONN) && b == Res::Got(PEER_ADDR) && c == Res::Got(NC_CONN_FACE)
        && s.ledger_ok()
}

/// ENOTCONN 注记：判定渲染。
pub fn nc_note(r: &Res) -> String {
    match r {
        Res::Got(n) => format!("got={}", n),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10864_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10864");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：stream 未连收发必拒
    let j1 = judge(&NcReq { fam: Fam::Stream, connected: false, dest: None }) == Res::Err(NC_NOTCONN);
    s.add("J1-未连拒", j1, "ENOTCONN 107");

    // J2 反路径：连接后交付
    let j2 = judge(&NcReq { fam: Fam::Stream, connected: true, dest: None }) == Res::Got(NC_CONN_FACE);
    s.add("J2-连接交付", j2, "连接面 Ok");

    // J3 地址轴双沿：dgram 带地址放行 / 无地址拒
    let j3 = judge(&NcReq { fam: Fam::Dgram, connected: false, dest: Some(PEER_ADDR) })
        == Res::Got(PEER_ADDR)
        && judge(&NcReq { fam: Fam::Dgram, connected: false, dest: None }) == Res::Err(NC_NOTCONN);
    s.add("J3-地址轴双沿", j3, "带/无地址可辨");

    // J4 族轴不越权：stream 带地址仍拒（地址救不了未连 stream）
    let j4 = judge(&NcReq { fam: Fam::Stream, connected: false, dest: Some(PEER_ADDR) })
        == Res::Err(NC_NOTCONN);
    s.add("J4-族轴不越权", j4, "stream 地址无效");

    // J5 连接回放
    s.add("J5-连接回放", conn_replay_ok(), "双拒→双交付对平");

    // J6 地址轴回放
    s.add("J6-地址轴回放", addr_replay_ok(), "dgram 三步账对平");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 未连放行检出
    let inj = inject_two();
    s.add("J8-未连放行检出", inj[0], "stream 冒交付必分歧");

    // J9 注入2 地址轴失明检出
    s.add("J9-地址轴失明检出", inj[1], "无地址误放行必分歧");

    // J10 连接账一致
    let mut sk = NcSock::new(Fam::Stream);
    let _ = sk.send(None);
    s.add("J10-连接账", sk.ledger_ok() && sk.denies == 1 && sk.sends == 1, "拒账落笔");

    // J11 字面锚：ENOTCONN=107 / PEER_ADDR=0x0a000001
    let j11 = ENOTCONN == 107 && PEER_ADDR == 0x0a00_0001;
    s.add("J11-字面锚", j11, "错误码与地址常量");

    // J12 诊断码闭集
    let j12 = diag_text(NC_NOTCONN) == "未连接收发拒"
        && diag_text(NC_DADDR_PASS) == "dgram 带地址放行面"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) ENOTCONN 注记落账
    let note = nc_note(&judge(&NcReq { fam: Fam::Stream, connected: false, dest: None }));
    s.add_dyn("J14-ENOTCONN 注记", note.contains("107"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10864_flow() {
        assert_eq!(judge(&NcReq { fam: Fam::Stream, connected: false, dest: None }), Res::Err(NC_NOTCONN));
        assert_eq!(judge(&NcReq { fam: Fam::Dgram, connected: false, dest: Some(5) }), Res::Got(5));
        assert_eq!(judge(&NcReq { fam: Fam::Dgram, connected: false, dest: None }), Res::Err(NC_NOTCONN));
        let mut s = NcSock::new(Fam::Stream);
        assert_eq!(s.send(None), Res::Err(NC_NOTCONN));
        s.connect();
        assert_eq!(s.send(None), Res::Got(NC_CONN_FACE));
        assert!(s.ledger_ok());
    }

    #[test]
    fn unx10864_checks_green() {
        let s = run_unx10864_checks();
        assert!(s.all_passed(), "UNX-F10864 判据未全绿");
    }
}
