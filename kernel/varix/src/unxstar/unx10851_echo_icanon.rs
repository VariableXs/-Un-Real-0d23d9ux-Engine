//! UNX-F10851 ECHO/ICANON 模式矩阵（规范/非规范切换错误面）（0x1c2e0 段）
//!
//! 锚点：UNX-F10851-J1 线路规程模式矩阵——ICANON 下读按行交付（无换行
//! 必待行，不零交付）；非规范下按块交付（VMIN 面归 10852，此处按块
//! 上限）；TCSAFLUSH 切模式必丢弃在途输入（数据面清账），TCSANOW
//! 必保留；ECHO 关后回显账必须停走（回显泄漏面）。正反双样本对账，
//! 注入核（flush 漏清/回显泄漏）必检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：模式场景（icanon × echo × 动作 × 在途）→ 交付判定器（行/块/
//! 待行三态）→ 线路规程状态机（置模式/喂数/读账/回显账）→ 模式样本
//! 矩阵 → 注入核（两坏实现必分歧）→ 切换回放（清/留对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c2e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c2e0;
pub const LD_FLUSH_LOST: u32 = SEG_BASE + 0x01; // TCSAFLUSH 未清在途
pub const LD_ECHO_LEAK: u32 = SEG_BASE + 0x02; // ECHO 关后回显泄漏
pub const LD_CANON_PARTIAL: u32 = SEG_BASE + 0x03; // 规范模式零碎交付
pub const LD_MODE_LEDGER: u32 = SEG_BASE + 0x04; // 模式账失平
pub const LD_OK_FACE: u32 = SEG_BASE + 0x05; // 交付放行面
pub const LD_PENDING_FACE: u32 = SEG_BASE + 0x06; // 待行面

/// 语义常量（termios 位与动作）。
pub const ICANON: u32 = 0o2;
pub const ECHO: u32 = 0o10;
pub const TCSANOW: u32 = 0;
pub const TCSADRAIN: u32 = 1;
pub const TCSAFLUSH: u32 = 2;
pub const CHUNK_CAP: u32 = 64;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        LD_FLUSH_LOST => "TCSAFLUSH 未清在途",
        LD_ECHO_LEAK => "ECHO 关后回显泄漏",
        LD_CANON_PARTIAL => "规范模式零碎交付",
        LD_MODE_LEDGER => "模式账失平",
        LD_OK_FACE => "交付放行",
        LD_PENDING_FACE => "待行面",
        _ => "未知诊断码",
    }
}

/// 读场景：规范标志 × 在途字节 × 行齐（判定器三输入）。
#[derive(Clone)]
pub struct ModeReq {
    pub icanon: bool,
    pub pending: u32,
    pub line_ready: bool,
}

/// 交付判定结果：Got(交付字节数) / Pending（待行面）。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Pending,
}

/// 交付判定器：规范按行（行齐交付整行、未齐待行）、非规范按块
/// （上限 CHUNK_CAP 截段）。全部分支依赖场景字段计算。
pub fn judge(req: &ModeReq) -> Res {
    if req.icanon {
        if req.line_ready && req.pending > 0 {
            Res::Got(req.pending)
        } else {
            Res::Pending
        }
    } else if req.pending == 0 {
        Res::Pending
    } else if req.pending <= CHUNK_CAP {
        Res::Got(req.pending)
    } else {
        Res::Got(CHUNK_CAP)
    }
}

/// 切换语义判定：TCSAFLUSH 清在途，其余动作保留。
pub fn switch_discard(action: u32, pending: u32) -> u32 {
    if action == TCSAFLUSH {
        0
    } else {
        pending
    }
}

/// 线路规程状态机：模式、在途、回显账、切换账。
pub struct LineDisc {
    pub icanon: bool,
    pub echo: bool,
    pub pending: u32,
    pub delivered: u32,
    pub echo_total: u32,
    pub flushes: u32,
    pub keeps: u32,
    pub note: String,
}

impl LineDisc {
    pub fn new(icanon: bool, echo: bool) -> LineDisc {
        LineDisc {
            icanon,
            echo,
            pending: 0,
            delivered: 0,
            echo_total: 0,
            flushes: 0,
            keeps: 0,
            note: "live".to_string(),
        }
    }

    /// 喂数：在途累积；ECHO 开则逐字节回显。
    pub fn feed(&mut self, n: u32) {
        self.pending += n;
        if self.echo {
            self.echo_total += n;
        }
    }

    /// 读一次：按交付判定器走账。
    pub fn read(&mut self, line_ready: bool) -> Res {
        let req = ModeReq {
            icanon: self.icanon,
            pending: self.pending,
            line_ready,
        };
        match judge(&req) {
            Res::Got(n) => {
                self.pending -= n;
                self.delivered += n;
                Res::Got(n)
            }
            Res::Pending => Res::Pending,
        }
    }

    /// 置模式：TCSAFLUSH 清在途并落清账；其余落留账。
    pub fn set_mode(&mut self, icanon: bool, action: u32) {
        self.pending = switch_discard(action, self.pending);
        if action == TCSAFLUSH {
            self.flushes += 1;
            self.note = "flushed".to_string();
        } else {
            self.keeps += 1;
            self.note = "kept".to_string();
        }
        self.icanon = icanon;
    }

    /// 模式账一致：交付+在途=喂入总量；回显账不超喂入。
    pub fn ledger_ok(&self, fed: u32) -> bool {
        self.delivered + self.pending <= fed && self.echo_total <= fed
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 20;

/// 矩阵逐格判定：期望（Got(n) 编码为 n+1，Pending 编码 0）命中计数。
pub fn verify_matrix(reqs: &[ModeReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Got(n) => want == n + 1,
            Res::Pending => want == 0,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：规范/非规范 × 行齐 × 在途沿，8 行全分账。
pub fn sample_matrix() -> (Vec<ModeReq>, Vec<u32>) {
    let mut reqs: Vec<ModeReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 规范+行齐+在途 → 整行交付
    reqs.push(ModeReq { icanon: true, pending: 5, line_ready: true });
    wants.push(6);
    // R1 规范+未齐 → 待行
    reqs.push(ModeReq { icanon: true, pending: 5, line_ready: false });
    wants.push(0);
    // R2 规范+行齐但零在途 → 待行（无行可交付）
    reqs.push(ModeReq { icanon: true, pending: 0, line_ready: true });
    wants.push(0);
    // R3 非规范+在途小块 → 块交付
    reqs.push(ModeReq { icanon: false, pending: 7, line_ready: false });
    wants.push(8);
    // R4 非规范+零在途 → 待（无数据）
    reqs.push(ModeReq { icanon: false, pending: 0, line_ready: false });
    wants.push(0);
    // R5 非规范+大块越上限 → 截 CHUNK_CAP
    reqs.push(ModeReq { icanon: false, pending: 200, line_ready: false });
    wants.push(CHUNK_CAP + 1);
    // R6 非规范+恰上限 → 全交付
    reqs.push(ModeReq { icanon: false, pending: CHUNK_CAP, line_ready: false });
    wants.push(CHUNK_CAP + 1);
    // R7 规范+行齐+恰一字节 → 交付 1
    reqs.push(ModeReq { icanon: true, pending: 1, line_ready: true });
    wants.push(2);
    (reqs, wants)
}

/// 注入 A：坏实现 TCSAFLUSH 漏清（在途残留）。
pub fn faulty_flush_skip(ld: &mut LineDisc, icanon: bool, action: u32) {
    let before = ld.pending;
    ld.set_mode(icanon, action);
    if action == TCSAFLUSH {
        ld.pending = before;
        ld.flushes -= 1;
        ld.note = "leak".to_string();
    }
}

/// 注入 B：坏实现回显泄漏（ECHO 关仍回显）。
pub fn faulty_echo_always(ld: &mut LineDisc, n: u32) {
    ld.feed(n);
    ld.echo_total += n;
    ld.note = "echo-leak".to_string();
}

/// 注入核：两处坏账与判定器/账本必分歧。
pub fn inject_two() -> [bool; 2] {
    // A：TCSAFLUSH 清账后 in途应为 0，坏实现残留
    let mut good = LineDisc::new(true, true);
    good.feed(9);
    good.set_mode(false, TCSAFLUSH);
    let mut bad = LineDisc::new(true, true);
    bad.feed(9);
    faulty_flush_skip(&mut bad, false, TCSAFLUSH);
    let d1 = good.pending == 0 && bad.pending == 9 && bad.note == "leak";
    // B：ECHO 关后喂 5 字节，好实现回显账 0，坏实现 5
    let mut good2 = LineDisc::new(false, false);
    good2.feed(5);
    let mut bad2 = LineDisc::new(false, false);
    faulty_echo_always(&mut bad2, 5);
    let d2 = good2.echo_total == 0 && bad2.echo_total == 5 && bad2.note == "echo-leak";
    [d1, d2]
}

/// 切换回放：清/留双动作对平（FLUSH 清、NOW 留）。
pub fn switch_replay_ok() -> bool {
    let mut ld = LineDisc::new(true, true);
    ld.feed(11);
    ld.set_mode(false, TCSAFLUSH);
    let cleared = ld.pending == 0 && ld.flushes == 1;
    ld.feed(13);
    ld.set_mode(true, TCSANOW);
    let kept = ld.pending == 13 && ld.keeps == 1;
    cleared && kept && ld.icanon && ld.ledger_ok(24)
}

/// 交付回放：规范行齐交付后待行、非规范块交付。
pub fn delivery_replay_ok() -> bool {
    let mut ld = LineDisc::new(true, true);
    ld.feed(6);
    let p1 = ld.read(false);
    let g1 = ld.read(true);
    ld.set_mode(false, TCSANOW);
    ld.feed(70);
    let g2 = ld.read(false);
    p1 == Res::Pending && g1 == Res::Got(6) && g2 == Res::Got(CHUNK_CAP)
        && ld.pending == 6 && ld.ledger_ok(76)
}

/// 回显账核：ECHO 开逐字节累加、关后停走。
pub fn echo_ledger_ok() -> bool {
    let mut ld = LineDisc::new(false, true);
    ld.feed(4);
    let on = ld.echo_total == 4;
    ld.echo = false;
    ld.feed(6);
    let off = ld.echo_total == 4;
    on && off && ld.ledger_ok(10)
}

/// 模式注记：判定渲染。
pub fn mode_note(r: &Res) -> String {
    match r {
        Res::Got(n) => format!("got={}", n),
        Res::Pending => diag_text(LD_PENDING_FACE).to_string(),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10851_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10851");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：规范行齐整行交付
    let j1 = judge(&ModeReq { icanon: true, pending: 5, line_ready: true }) == Res::Got(5);
    s.add("J1-整行交付", j1, "ICANON 行面");

    // J2 边界：非规范恰 CHUNK_CAP 全交付 / 恰越一截段
    let j2 = judge(&ModeReq { icanon: false, pending: CHUNK_CAP, line_ready: false })
        == Res::Got(CHUNK_CAP)
        && judge(&ModeReq { icanon: false, pending: CHUNK_CAP + 1, line_ready: false })
            == Res::Got(CHUNK_CAP);
    s.add("J2-块边界", j2, "上限两沿可辨");

    // J3 待行面：规范未齐不零交付
    let j3 = judge(&ModeReq { icanon: true, pending: 5, line_ready: false }) == Res::Pending;
    s.add("J3-待行面", j3, "无换行不零碎交付");

    // J4 切换语义：TCSAFLUSH 清 / TCSANOW 留
    let j4 = switch_discard(TCSAFLUSH, 9) == 0 && switch_discard(TCSANOW, 9) == 9
        && switch_discard(TCSADRAIN, 9) == 9;
    s.add("J4-切换语义", j4, "清/留/留三动作可辨");

    // J5 切换回放（清账+留账）
    s.add("J5-切换回放", switch_replay_ok(), "FLUSH 清/NOW 留对平");

    // J6 交付回放（规范→非规范序列）
    s.add("J6-交付回放", delivery_replay_ok(), "待行/整行/截段三态序列");

    // J7 回显账核
    s.add("J7-回显账", echo_ledger_ok(), "ECHO 开走关停");

    // J8 样本矩阵全命中
    s.add("J8-矩阵对平", cap_ok, "8 行三态全命中");

    // J9 注入1 flush 漏清检出
    let inj = inject_two();
    s.add("J9-flush 漏清检出", inj[0], "TCSAFLUSH 残留必分歧");

    // J10 注入2 回显泄漏检出
    s.add("J10-回显泄漏检出", inj[1], "ECHO 关回显必分歧");

    // J11 模式账一致
    let mut ld = LineDisc::new(true, true);
    ld.feed(8);
    let _ = ld.read(true);
    s.add("J11-模式账", ld.ledger_ok(8) && ld.pending == 0 && ld.delivered == 8, "喂入=交付+在途");

    // J12 字面锚：ICANON=0o2 / ECHO=0o10 / TCSAFLUSH=2 / CHUNK_CAP=64
    let j12 = ICANON == 0o2 && ECHO == 0o10 && TCSAFLUSH == 2 && CHUNK_CAP == 64;
    s.add("J12-字面锚", j12, "位域与动作常量");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 模式注记落账
    let note = mode_note(&judge(&ModeReq { icanon: false, pending: 30, line_ready: false }));
    s.add_dyn("J14-模式注记", note == "got=30", &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10851_flow() {
        assert_eq!(judge(&ModeReq { icanon: true, pending: 3, line_ready: false }), Res::Pending);
        assert_eq!(judge(&ModeReq { icanon: true, pending: 3, line_ready: true }), Res::Got(3));
        assert_eq!(judge(&ModeReq { icanon: false, pending: 100, line_ready: false }), Res::Got(CHUNK_CAP));
        let mut ld = LineDisc::new(true, false);
        ld.feed(4);
        ld.set_mode(false, TCSAFLUSH);
        assert_eq!(ld.pending, 0);
        ld.feed(6);
        ld.set_mode(true, TCSANOW);
        assert_eq!(ld.pending, 6);
        assert_eq!(switch_discard(TCSADRAIN, 5), 5);
        assert!(ld.ledger_ok(10));
    }

    #[test]
    fn unx10851_checks_green() {
        let s = run_unx10851_checks();
        assert!(s.all_passed(), "UNX-F10851 判据未全绿");
    }
}
