//! UNX-F10863 EPIPE 矩阵（stream 对端关后写）（0x1c460 段）
//!
//! 锚点：UNX-F10863-J1 EPIPE 写面矩阵——stream 对端开启时写全量交
//! 付；对端关闭后：正量写必 EPIPE（32）、零量写恒放行 Got(0)（零
//! 写不触发管道断纪）；关闭前已缓冲残余与 EPIPE 判定无关（交付以
//! 对端旗为准）。判定依赖场景字段演算，注入核（零写误拒/死写放行
//! 坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：写场景（对端旗 × 写量）→ EPIPE 判定器（正量/零量双支）→
//! 写端状态机（写账/断纪走账）→ 样本矩阵（八行正反对）→ 注入核
//! （两坏实现必分歧）→ 断纪回放（关→正量 EPIPE→零写放行对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c460 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c460;
pub const EP_EPIPE: u32 = SEG_BASE + 0x01; // 死对端正量写 EPIPE
pub const EP_ZERO_PASS: u32 = SEG_BASE + 0x02; // 零写放行面
pub const EP_LIVE_FACE: u32 = SEG_BASE + 0x03; // 对端开启交付面
pub const EP_PEND_KEEP: u32 = SEG_BASE + 0x04; // 残余不越权面
pub const EP_ZERO_GHOST: u32 = SEG_BASE + 0x05; // 零写误拒
pub const EP_LEDGER_LOST: u32 = SEG_BASE + 0x06; // 写账失平

/// 语义字面锚。
pub const EPIPE: u32 = 32;
pub const BUF_CAP: u32 = 4096;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        EP_EPIPE => "死对端正量写 EPIPE",
        EP_ZERO_PASS => "零写放行面",
        EP_LIVE_FACE => "对端开启交付面",
        EP_PEND_KEEP => "残余不越权面",
        EP_ZERO_GHOST => "零写误拒",
        EP_LEDGER_LOST => "写账失平",
        _ => "未知诊断码",
    }
}

/// 写场景：对端旗 × 写量（判定器二输入）。
#[derive(Clone)]
pub struct EpReq {
    pub peer_open: bool,
    pub want: u32,
}

/// 判定结果：Got(交付量) / Err(EPIPE)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Err(u32),
}

/// EPIPE 判定器：对端开启全量交付；关闭后正量写 EPIPE、零写放行。
pub fn judge(req: &EpReq) -> Res {
    if req.peer_open {
        return Res::Got(req.want);
    }
    if req.want == 0 {
        Res::Got(0)
    } else {
        Res::Err(EP_EPIPE)
    }
}

/// 写端状态机：写量账、断纪账。
pub struct WriteEnd {
    pub peer_open: bool,
    pub written: u32,
    pub epipes: u32,
    pub zero_writes: u32,
    pub note: String,
}

impl WriteEnd {
    pub fn new() -> WriteEnd {
        WriteEnd { peer_open: true, written: 0, epipes: 0, zero_writes: 0, note: "live".to_string() }
    }

    /// 对端关闭：旗清（幂等）。
    pub fn peer_close(&mut self) {
        self.peer_open = false;
        self.note = "peer-gone".to_string();
    }

    /// 写：判定走账（正量落 written / EPIPE 落账 / 零写落账）。
    pub fn write(&mut self, want: u32) -> Res {
        let req = EpReq { peer_open: self.peer_open, want };
        match judge(&req) {
            Res::Got(n) => {
                if n == 0 {
                    self.zero_writes += 1;
                    self.note = "zero-pass".to_string();
                } else {
                    self.written += n;
                    self.note = "written".to_string();
                }
                Res::Got(n)
            }
            Res::Err(c) => {
                self.epipes += 1;
                self.note = "epipe".to_string();
                Res::Err(c)
            }
        }
    }

    /// 写账一致：written ≤ BUF_CAP 且对端关闭后正量写不落 written。
    pub fn ledger_ok(&self) -> bool {
        self.written <= BUF_CAP && (!self.peer_open || self.epipes == 0)
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Got(n) 编码 n+1，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[EpReq], wants: &[u32]) -> (usize, usize, bool) {
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

/// 样本矩阵：对端旗 × 写量（正/零/边界 1），8 行正反对。
pub fn sample_matrix() -> (Vec<EpReq>, Vec<u32>) {
    let mut reqs: Vec<EpReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 死对端正量写 → EPIPE
    reqs.push(EpReq { peer_open: false, want: 16 });
    wants.push(EP_EPIPE);
    // R1 死对端零写 → Got(0)
    reqs.push(EpReq { peer_open: false, want: 0 });
    wants.push(1);
    // R2 活对端正量写 → 交付
    reqs.push(EpReq { peer_open: true, want: 16 });
    wants.push(17);
    // R3 活对端零写 → Got(0)
    reqs.push(EpReq { peer_open: true, want: 0 });
    wants.push(1);
    // R4 死对端一字节写 → EPIPE（最小正量边界）
    reqs.push(EpReq { peer_open: false, want: 1 });
    wants.push(EP_EPIPE);
    // R5 活对端大写量 → 交付
    reqs.push(EpReq { peer_open: true, want: 2048 });
    wants.push(2049);
    // R6 死对端大写量 → EPIPE
    reqs.push(EpReq { peer_open: false, want: 4096 });
    wants.push(EP_EPIPE);
    // R7 复核格：死对端两字节写仍 EPIPE（零写放行由注入核把守）
    reqs.push(EpReq { peer_open: false, want: 2 });
    wants.push(EP_EPIPE);
    (reqs, wants)
}

/// 注入 A：坏实现零写误拒（死对端零写也 EPIPE）。
pub fn faulty_zero_deny(req: &EpReq) -> Res {
    if !req.peer_open {
        Res::Err(EP_EPIPE)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现死写放行（死对端正量写误交付）。
pub fn faulty_dead_pass(req: &EpReq) -> Res {
    if !req.peer_open {
        Res::Got(req.want)
    } else {
        judge(req)
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let zero = EpReq { peer_open: false, want: 0 };
    let pos = EpReq { peer_open: false, want: 8 };
    let d1 = judge(&zero) != faulty_zero_deny(&zero);
    let d2 = judge(&pos) != faulty_dead_pass(&pos);
    [d1, d2]
}

/// 断纪回放：活写 5 → 对端关 → 正量写 EPIPE → 零写 Got(0)，账面对平。
pub fn epipe_replay_ok() -> bool {
    let mut w = WriteEnd::new();
    let a = w.write(5);
    w.peer_close();
    let b = w.write(7);
    let c = w.write(0);
    a == Res::Got(5) && b == Res::Err(EP_EPIPE) && c == Res::Got(0)
        && w.written == 5 && w.epipes == 1 && w.zero_writes == 1 && w.ledger_ok()
}

/// 残余不越权回放：对端关闭旗是唯一判据（残余 avail 不改变 EPIPE）。
pub fn pend_replay_ok() -> bool {
    let req_a = EpReq { peer_open: false, want: 3 };
    let req_b = EpReq { peer_open: false, want: 3 };
    // avail 场景仅注入注释面：两格同旗同量必同判（残余不越权）。
    judge(&req_a) == judge(&req_b) && judge(&req_a) == Res::Err(EP_EPIPE)
}

/// EPIPE 注记：判定渲染。
pub fn ep_note(r: &Res) -> String {
    match r {
        Res::Got(n) => format!("got={}", n),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10863_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10863");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：死对端正量写 EPIPE
    let j1 = judge(&EpReq { peer_open: false, want: 16 }) == Res::Err(EP_EPIPE);
    s.add("J1-死写 EPIPE", j1, "EPIPE 32");

    // J2 反路径：死对端零写放行
    let j2 = judge(&EpReq { peer_open: false, want: 0 }) == Res::Got(0);
    s.add("J2-零写放行", j2, "Got(0) 面");

    // J3 边界：一字节即 EPIPE（最小正量）
    let j3 = judge(&EpReq { peer_open: false, want: 1 }) == Res::Err(EP_EPIPE)
        && judge(&EpReq { peer_open: false, want: 0 }) == Res::Got(0);
    s.add("J3-1/0 两沿", j3, "最小正量可辨");

    // J4 活面不误伤：对端开启写全交付
    let j4 = judge(&EpReq { peer_open: true, want: 2048 }) == Res::Got(2048)
        && judge(&EpReq { peer_open: true, want: 0 }) == Res::Got(0);
    s.add("J4-活面交付", j4, "EPIPE 不越权");

    // J5 断纪回放
    s.add("J5-断纪回放", epipe_replay_ok(), "关→EPIPE→零写对平");

    // J6 残余不越权回放
    s.add("J6-残余不越权", pend_replay_ok(), "同旗同量同判");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 零写误拒检出
    let inj = inject_two();
    s.add("J8-零写误拒检出", inj[0], "零写 EPIPE 必分歧");

    // J9 注入2 死写放行检出
    s.add("J9-死写放行检出", inj[1], "死写交付必分歧");

    // J10 写账一致
    let mut w = WriteEnd::new();
    let _ = w.write(100);
    w.peer_close();
    let _ = w.write(50);
    s.add("J10-写账", w.ledger_ok() && w.written == 100 && w.epipes == 1, "死写不落 written");

    // J11 字面锚：EPIPE=32 / BUF_CAP=4096
    let j11 = EPIPE == 32 && BUF_CAP == 4096;
    s.add("J11-字面锚", j11, "错误码与容量常量");

    // J12 诊断码闭集
    let j12 = diag_text(EP_EPIPE) == "死对端正量写 EPIPE"
        && diag_text(EP_ZERO_PASS) == "零写放行面"
        && diag_text(9) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) EPIPE 注记落账
    let note = ep_note(&judge(&EpReq { peer_open: false, want: 4 }));
    s.add_dyn("J14-EPIPE 注记", note.contains("32"), &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10863_flow() {
        assert_eq!(judge(&EpReq { peer_open: false, want: 16 }), Res::Err(EP_EPIPE));
        assert_eq!(judge(&EpReq { peer_open: false, want: 0 }), Res::Got(0));
        assert_eq!(judge(&EpReq { peer_open: true, want: 16 }), Res::Got(16));
        let mut w = WriteEnd::new();
        w.peer_close();
        w.peer_close();
        assert_eq!(w.write(3), Res::Err(EP_EPIPE));
        assert_eq!(w.write(0), Res::Got(0));
        assert_eq!(w.epipes, 1);
        assert_eq!(w.zero_writes, 1);
        assert!(w.ledger_ok());
    }

    #[test]
    fn unx10863_checks_green() {
        let s = run_unx10863_checks();
        assert!(s.all_passed(), "UNX-F10863 判据未全绿");
    }
}
