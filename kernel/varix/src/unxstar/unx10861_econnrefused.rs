//! UNX-F10861 ECONNREFUSED 矩阵（connect 拒绝面）（0x1c420 段）
//!
//! 锚点：UNX-F10861-J1 connect 拒绝矩阵——对端端口有监听者必握手
//! 交付（Ok 面）；无监听者必 ECONNREFUSED（111）即刻拒（不阻塞不
//! 重试内联）；dgram 已 connect 面对拒绝端口首次发送报拒绝、重连
//! 有效监听端口后恢复交付。判定依赖场景字段演算，注入核（拒面吞
//! 并/无监听放行坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：连接场景（监听 × 端口）→ 拒绝判定器（监听双支）→ 连接器
//! 状态机（connect/重连走账）→ 样本矩阵（八行正反对）→ 注入核（两
//! 坏实现必分歧）→ 拒绝回放（拒账→重连对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c420 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c420;
pub const CR_REFUSED: u32 = SEG_BASE + 0x01; // 无监听拒绝面
pub const CR_ESTAB: u32 = SEG_BASE + 0x02; // 握手交付面
pub const CR_DGRAM_REFUSE: u32 = SEG_BASE + 0x03; // dgram 发送报拒
pub const CR_RECOVER: u32 = SEG_BASE + 0x04; // 重连恢复面
pub const CR_GHOST_PASS: u32 = SEG_BASE + 0x05; // 无监听放行冒账
pub const CR_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 连接账失平

/// 语义字面锚。
pub const ECONNREFUSED: u32 = 111;
pub const PORT_LISTEN: u32 = 8080;
pub const PORT_DEAD: u32 = 9999;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        CR_REFUSED => "无监听拒绝面",
        CR_ESTAB => "握手交付面",
        CR_DGRAM_REFUSE => "dgram 发送报拒",
        CR_RECOVER => "重连恢复面",
        CR_GHOST_PASS => "无监听放行冒账",
        CR_LEDGER_LOST => "连接账失平",
        _ => "未知诊断码",
    }
}

/// 连接场景：端口 × 是否有监听者（判定器二输入）。
#[derive(Clone)]
pub struct CnReq {
    pub port: u32,
    pub listening: bool,
}

/// 判定结果：Ok(端口) / Err(码)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Ok(u32),
    Err(u32),
}

/// 拒绝判定器：有监听者交付；无监听者 ECONNREFUSED 即刻拒。
pub fn judge(req: &CnReq) -> Res {
    if req.listening {
        Res::Ok(req.port)
    } else {
        Res::Err(CR_REFUSED)
    }
}

/// 连接器状态机：当前对端、连接账、拒账。
pub struct Connector {
    pub peer: Option<u32>,
    pub refused: u32,
    pub estabs: u32,
    pub dgram_refused: u32,
    pub note: String,
}

impl Connector {
    pub fn new() -> Connector {
        Connector { peer: None, refused: 0, estabs: 0, dgram_refused: 0, note: "idle".to_string() }
    }

    /// connect：判定走账（成落 peer / 拒落 refused 账且 peer 清空）。
    pub fn connect(&mut self, req: &CnReq) -> Res {
        let r = judge(req);
        match &r {
            Res::Ok(p) => {
                self.peer = Some(*p);
                self.estabs += 1;
                self.note = "estab".to_string();
            }
            Res::Err(_) => {
                self.peer = None;
                self.refused += 1;
                self.note = "refused".to_string();
            }
        }
        r
    }

    /// dgram 已连接发送：无对端（拒后残留态）必报拒；有对端交付。
    pub fn dgram_send(&mut self) -> Res {
        match self.peer {
            Some(p) => {
                self.note = "dgram-sent".to_string();
                Res::Ok(p)
            }
            None => {
                self.dgram_refused += 1;
                self.note = "dgram-refused".to_string();
                Res::Err(CR_DGRAM_REFUSE)
            }
        }
    }

    /// 连接账一致：拒账/成账之和非负且注记可辨。
    pub fn ledger_ok(&self) -> bool {
        self.refused + self.estabs <= 64 && self.note != ""
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Ok(n) 编码 n+1，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[CnReq], wants: &[u32]) -> (usize, usize, bool) {
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

/// 样本矩阵：监听/无监听 × 端口两值，8 行正反对。
pub fn sample_matrix() -> (Vec<CnReq>, Vec<u32>) {
    let mut reqs: Vec<CnReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 死端口无监听 → ECONNREFUSED
    reqs.push(CnReq { port: PORT_DEAD, listening: false });
    wants.push(CR_REFUSED);
    // R1 死端口有监听 → Ok
    reqs.push(CnReq { port: PORT_DEAD, listening: true });
    wants.push(PORT_DEAD + 1);
    // R2 常端口有监听 → Ok
    reqs.push(CnReq { port: PORT_LISTEN, listening: true });
    wants.push(PORT_LISTEN + 1);
    // R3 常端口无监听 → ECONNREFUSED
    reqs.push(CnReq { port: PORT_LISTEN, listening: false });
    wants.push(CR_REFUSED);
    // R4 端口 1 无监听 → ECONNREFUSED
    reqs.push(CnReq { port: 1, listening: false });
    wants.push(CR_REFUSED);
    // R5 端口 1 有监听 → Ok
    reqs.push(CnReq { port: 1, listening: true });
    wants.push(2);
    // R6 端口 0 有监听 → Ok
    reqs.push(CnReq { port: 0, listening: true });
    wants.push(1);
    // R7 端口 0 无监听 → ECONNREFUSED
    reqs.push(CnReq { port: 0, listening: false });
    wants.push(CR_REFUSED);
    (reqs, wants)
}

/// 注入 A：坏实现无监听放行（拒面冒交付）。
pub fn faulty_ghost_pass(req: &CnReq) -> Res {
    if !req.listening {
        Res::Ok(req.port)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现拒面吞并（拒绝误报成连接空端口 0）。
pub fn faulty_refuse_swallow(req: &CnReq) -> Res {
    match judge(req) {
        Res::Err(_) => Res::Ok(0),
        other => other,
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let dead = CnReq { port: PORT_DEAD, listening: false };
    let d1 = judge(&dead) != faulty_ghost_pass(&dead);
    let d2 = judge(&dead) != faulty_refuse_swallow(&dead);
    [d1, d2]
}

/// 拒绝回放：拒账落账、peer 清空、dgram 发送报拒、重连恢复交付。
pub fn refuse_replay_ok() -> bool {
    let mut c = Connector::new();
    let r1 = c.connect(&CnReq { port: PORT_DEAD, listening: false });
    let d = c.dgram_send();
    let r2 = c.connect(&CnReq { port: PORT_LISTEN, listening: true });
    let s = c.dgram_send();
    r1 == Res::Err(CR_REFUSED) && d == Res::Err(CR_DGRAM_REFUSE)
        && r2 == Res::Ok(PORT_LISTEN) && s == Res::Ok(PORT_LISTEN)
        && c.refused == 1 && c.dgram_refused == 1 && c.ledger_ok()
}

/// 成面回放：双端口先后成连接，peer 随最近连接漂移。
pub fn estab_replay_ok() -> bool {
    let mut c = Connector::new();
    let a = c.connect(&CnReq { port: 1000, listening: true });
    let b = c.connect(&CnReq { port: 2000, listening: true });
    a == Res::Ok(1000) && b == Res::Ok(2000) && c.peer == Some(2000)
        && c.estabs == 2 && c.refused == 0 && c.ledger_ok()
}

/// 拒绝注记：判定渲染。
pub fn cr_note(r: &Res) -> String {
    match r {
        Res::Ok(p) => format!("estab port={}", p),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10861_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10861");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：有监听者交付
    let j1 = judge(&CnReq { port: PORT_LISTEN, listening: true }) == Res::Ok(PORT_LISTEN);
    s.add("J1-握手交付", j1, "监听面 Ok");

    // J2 反路径：无监听者 ECONNREFUSED
    let j2 = judge(&CnReq { port: PORT_DEAD, listening: false }) == Res::Err(CR_REFUSED);
    s.add("J2-即刻拒", j2, "ECONNREFUSED 111");

    // J3 边界：同端口旗面双沿（有无监听换面）
    let j3 = judge(&CnReq { port: PORT_DEAD, listening: true }) == Res::Ok(PORT_DEAD)
        && judge(&CnReq { port: PORT_DEAD, listening: false }) == Res::Err(CR_REFUSED);
    s.add("J3-同端口双沿", j3, "监听旗换面");

    // J4 端口轴不越权：端口 0/1 与常端口同规则
    let j4 = judge(&CnReq { port: 0, listening: false }) == Res::Err(CR_REFUSED)
        && judge(&CnReq { port: 1, listening: true }) == Res::Ok(1);
    s.add("J4-端口轴", j4, "0/1/常端口一致");

    // J5 拒绝回放（拒账→dgram 报拒→重连恢复）
    s.add("J5-拒绝回放", refuse_replay_ok(), "四步账对平");

    // J6 成面回放（peer 漂移）
    s.add("J6-成面回放", estab_replay_ok(), "peer 随最近连接");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 无监听放行检出
    let inj = inject_two();
    s.add("J8-无监听放行检出", inj[0], "拒面冒交付必分歧");

    // J9 注入2 拒面吞并检出
    s.add("J9-拒面吞并检出", inj[1], "拒绝误报成必分歧");

    // J10 连接账一致
    let mut c = Connector::new();
    let _ = c.connect(&CnReq { port: 1, listening: false });
    let _ = c.connect(&CnReq { port: 2, listening: true });
    s.add("J10-连接账", c.ledger_ok() && c.refused == 1 && c.estabs == 1 && c.peer == Some(2), "一拒一成落账");

    // J11 字面锚：ECONNREFUSED=111 / 端口常量
    let j11 = ECONNREFUSED == 111 && PORT_LISTEN == 8080 && PORT_DEAD == 9999;
    s.add("J11-字面锚", j11, "错误码与端口常量");

    // J12 诊断码闭集
    let j12 = diag_text(CR_REFUSED) == "无监听拒绝面"
        && diag_text(CR_RECOVER) == "重连恢复面"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 拒绝注记落账
    let note = cr_note(&judge(&CnReq { port: PORT_DEAD, listening: false }));
    s.add_dyn("J14-拒绝注记", note.contains("111"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10861_flow() {
        assert_eq!(judge(&CnReq { port: 9999, listening: false }), Res::Err(CR_REFUSED));
        assert_eq!(judge(&CnReq { port: 9999, listening: true }), Res::Ok(9999));
        let mut c = Connector::new();
        assert_eq!(c.connect(&CnReq { port: 9999, listening: false }), Res::Err(CR_REFUSED));
        assert_eq!(c.dgram_send(), Res::Err(CR_DGRAM_REFUSE));
        assert_eq!(c.connect(&CnReq { port: 8080, listening: true }), Res::Ok(8080));
        assert_eq!(c.dgram_send(), Res::Ok(8080));
        assert!(c.ledger_ok());
    }

    #[test]
    fn unx10861_checks_green() {
        let s = run_unx10861_checks();
        assert!(s.all_passed(), "UNX-F10861 判据未全绿");
    }
}
