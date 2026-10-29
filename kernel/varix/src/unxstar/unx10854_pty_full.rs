//! UNX-F10854 pty 缓冲满写矩阵（阻塞正反）（0x1c340 段）
//!
//! 锚点：UNX-F10854-J1 pty 写满面矩阵——缓冲有零头空间时写必须部分
//! 交付（partial face，交付 min(free,want)），不得误报 EAGAIN；缓冲
//! 全满时非阻塞写必 EAGAIN、阻塞写必阻塞面；腾空后余量续写（阻塞
//! 循环直至写尽）。判定依赖场景字段演算，注入核（整吞坏实现/满态
//! 放行坏实现）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：写场景（nonblock × want × free）→ 写满判定器（零头/全满
//! 双层）→ 阻塞写状态机（while 循环续写 + 腾空推进）→ 写满样本矩阵
//! → 注入核（两坏实现必分歧）→ 续写回放（部分交付后余量对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c340 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c340;
pub const FW_ZERO_EAGAIN: u32 = SEG_BASE + 0x01; // 满态非阻塞写 EAGAIN
pub const FW_PARTIAL_FACE: u32 = SEG_BASE + 0x02; // 零头部分交付面
pub const FW_BLOCK_FACE: u32 = SEG_BASE + 0x03; // 满态阻塞面
pub const FW_FULL_FACE: u32 = SEG_BASE + 0x04; // 写尽面
pub const FW_LEDGER_LOST: u32 = SEG_BASE + 0x05; // 写账失平
pub const FW_SPACE_INV: u32 = SEG_BASE + 0x06; // 空间账反转

/// 语义字面锚。
pub const EAGAIN: u32 = 11;
pub const BUF_CAP: u32 = 4096;
pub const WRITE_CAP: u32 = 512;

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        FW_ZERO_EAGAIN => "满态非阻塞写 EAGAIN",
        FW_PARTIAL_FACE => "零头部分交付面",
        FW_BLOCK_FACE => "满态阻塞面",
        FW_FULL_FACE => "写尽面",
        FW_LEDGER_LOST => "写账失平",
        FW_SPACE_INV => "空间账反转",
        _ => "未知诊断码",
    }
}

/// 写场景：旗标 × 请求量 × 空闲空间（判定器三输入）。
#[derive(Clone)]
pub struct FwReq {
    pub nonblock: bool,
    pub want: u32,
    pub free: u32,
}

/// 判定结果：Got(本笔交付字节数) / Block（满态阻塞面）/ Err(EAGAIN)。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Block,
    Err(u32),
}

/// 写满判定器：零请求恒放行（写 0 字节）；有零头必部分交付；
/// 全满看旗（位 EAGAIN / 清 Block）。
pub fn judge(req: &FwReq) -> Res {
    if req.want == 0 {
        return Res::Got(0);
    }
    if req.free == 0 {
        if req.nonblock {
            Res::Err(FW_ZERO_EAGAIN)
        } else {
            Res::Block
        }
    } else if req.free >= req.want {
        Res::Got(req.want)
    } else {
        Res::Got(req.free)
    }
}

/// 阻塞写状态机：续写循环 + 腾空推进账。
pub struct FullWriter {
    pub free: u32,
    pub nonblock: bool,
    pub written_total: u32,
    pub partials: u32,
    pub eagains: u32,
    pub blocks: u32,
    pub note: String,
}

impl FullWriter {
    pub fn new(free: u32, nonblock: bool) -> FullWriter {
        FullWriter {
            free,
            nonblock,
            written_total: 0,
            partials: 0,
            eagains: 0,
            blocks: 0,
            note: "idle".to_string(),
        }
    }

    /// 单笔写：按判定器走账（部分交付落 partials 账）。
    pub fn write_once(&mut self, want: u32) -> Res {
        let req = FwReq { nonblock: self.nonblock, want, free: self.free };
        match judge(&req) {
            Res::Got(n) => {
                if n < want {
                    self.partials += 1;
                    self.note = "partial".to_string();
                } else {
                    self.note = "full-shot".to_string();
                }
                self.free -= n;
                self.written_total += n;
                Res::Got(n)
            }
            Res::Block => {
                self.blocks += 1;
                self.note = "blocked".to_string();
                Res::Block
            }
            Res::Err(c) => {
                self.eagains += 1;
                self.note = "eagain".to_string();
                Res::Err(c)
            }
        }
    }

    /// 腾空推进：阻塞写每轮腾 1 字节（while 三要素推进至空间充足）。
    pub fn make_room(&mut self, rounds: u32) {
        let mut i = 0u32;
        while i < rounds {
            if self.free < BUF_CAP {
                self.free += 1;
            }
            i += 1;
        }
    }

    /// 阻塞续写：循环写直至 want 尽（腾空由外部注入）。
    /// init(余量 want0)/cond(余量>0 且轮数封顶)/incr(单笔写)。
    pub fn write_all(&mut self, want0: u32, room_rounds: u32) -> u32 {
        let mut rest = want0;
        let mut rounds = 0u32;
        while rest > 0 && rounds <= room_rounds {
            if self.free == 0 && !self.nonblock {
                self.make_room(1);
            }
            let r = self.write_once(rest);
            match r {
                Res::Got(n) => rest -= n,
                _ => {}
            }
            rounds += 1;
        }
        want0 - rest
    }

    /// 写账一致：写总量 ≤ BUF_CAP 且账面注记可辨。
    pub fn ledger_ok(&self) -> bool {
        self.written_total <= BUF_CAP && self.note != ""
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Got(n) 编码 n+1，Block 0，Err(c) 直码）命中计数。
pub fn verify_matrix(reqs: &[FwReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Got(n) => want == n + 1,
            Res::Block => want == 0,
            Res::Err(c) => c == want,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：满态/零头/充足 × 旗位/旗清，8 行正反对。
pub fn sample_matrix() -> (Vec<FwReq>, Vec<u32>) {
    let mut reqs: Vec<FwReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 满态旗位 → EAGAIN
    reqs.push(FwReq { nonblock: true, want: 32, free: 0 });
    wants.push(FW_ZERO_EAGAIN);
    // R1 满态旗清 → Block
    reqs.push(FwReq { nonblock: false, want: 32, free: 0 });
    wants.push(0);
    // R2 零头旗位 → 部分交付 free
    reqs.push(FwReq { nonblock: true, want: 100, free: 40 });
    wants.push(41);
    // R3 零头旗清 → 同部分交付
    reqs.push(FwReq { nonblock: false, want: 100, free: 40 });
    wants.push(41);
    // R4 充足旗位 → 全额
    reqs.push(FwReq { nonblock: true, want: 100, free: 200 });
    wants.push(101);
    // R5 边界：want 恰等于 free → 全额
    reqs.push(FwReq { nonblock: true, want: 64, free: 64 });
    wants.push(65);
    // R6 零头一字节 → 交付 1
    reqs.push(FwReq { nonblock: true, want: 32, free: 1 });
    wants.push(2);
    // R7 零请求 → 交付 0
    reqs.push(FwReq { nonblock: true, want: 0, free: 0 });
    wants.push(1);
    (reqs, wants)
}

/// 注入 A：坏实现零头整吞（零头空间误报 EAGAIN 不部分交付）。
pub fn faulty_all_or_nothing(req: &FwReq) -> Res {
    if req.free > 0 && req.free < req.want {
        if req.nonblock {
            Res::Err(FW_ZERO_EAGAIN)
        } else {
            Res::Block
        }
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现满态放行（全满非阻塞写返 Got(0) 幻影交付）。
pub fn faulty_zero_space_pass(req: &FwReq) -> Res {
    if req.free == 0 && req.nonblock {
        Res::Got(0)
    } else {
        judge(req)
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let partial = FwReq { nonblock: true, want: 100, free: 40 };
    let full = FwReq { nonblock: true, want: 8, free: 0 };
    let d1 = judge(&partial) != faulty_all_or_nothing(&partial);
    let d2 = judge(&full) != faulty_zero_space_pass(&full);
    [d1, d2]
}

/// 续写回放：零头部分交付 + 腾空推进 + 余量写尽。
pub fn drain_replay_ok() -> bool {
    let mut fw = FullWriter::new(10, false);
    let got = fw.write_all(14, 8);
    got == 14 && fw.free == BUF_CAP - 4 && fw.written_total == 14
        && fw.partials >= 1 && fw.ledger_ok()
}

/// 满态回放：非阻塞满态 EAGAIN 计账、腾空后部分/全额续写。
pub fn full_replay_ok() -> bool {
    let mut fw = FullWriter::new(0, true);
    let r1 = fw.write_once(16);
    fw.free = 6;
    let r2 = fw.write_once(16);
    r1 == Res::Err(FW_ZERO_EAGAIN) && r2 == Res::Got(6)
        && fw.eagains == 1 && fw.partials == 1 && fw.written_total == 6 && fw.ledger_ok()
}

/// 写满注记：判定渲染。
pub fn fw_note(r: &Res) -> String {
    match r {
        Res::Got(n) => format!("got={}", n),
        Res::Block => diag_text(FW_BLOCK_FACE).to_string(),
        Res::Err(c) => format!("{} ({})", diag_text(*c), c),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10854_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10854");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 正路径：零头部分交付
    let j1 = judge(&FwReq { nonblock: true, want: 100, free: 40 }) == Res::Got(40);
    s.add("J1-零头部分交付", j1, "min(free,want) 语义");

    // J2 边界：want 恰等 free 全额 / 恰缺一零头
    let j2 = judge(&FwReq { nonblock: true, want: 64, free: 64 }) == Res::Got(64)
        && judge(&FwReq { nonblock: true, want: 65, free: 64 }) == Res::Got(64);
    s.add("J2-零头边界", j2, "全额/零头两沿可辨");

    // J3 满态双层：旗位 EAGAIN / 旗清 Block
    let j3 = judge(&FwReq { nonblock: true, want: 8, free: 0 }) == Res::Err(FW_ZERO_EAGAIN)
        && judge(&FwReq { nonblock: false, want: 8, free: 0 }) == Res::Block;
    s.add("J3-满态双层", j3, "EAGAIN/Block 旗面可辨");

    // J4 旗不压零头：部分交付与旗无关
    let j4 = judge(&FwReq { nonblock: true, want: 100, free: 40 }) == Res::Got(40)
        && judge(&FwReq { nonblock: false, want: 100, free: 40 }) == Res::Got(40);
    s.add("J4-旗不压零头", j4, "部分交付恒定");

    // J5 续写回放（腾空推进）
    s.add("J5-续写回放", drain_replay_ok(), "10 零头起步写尽 14");

    // J6 满态回放（EAGAIN→腾空→部分）
    s.add("J6-满态回放", full_replay_ok(), "EAGAIN 账与部分账对平");

    // J7 样本矩阵全命中
    s.add("J7-矩阵对平", cap_ok, "8 行正反对全命中");

    // J8 注入1 零头整吞检出
    let inj = inject_two();
    s.add("J8-零头整吞检出", inj[0], "部分交付误 EAGAIN 必分歧");

    // J9 注入2 满态放行检出
    s.add("J9-满态放行检出", inj[1], "幻影交付必分歧");

    // J10 写账一致
    let mut fw = FullWriter::new(BUF_CAP, true);
    let _ = fw.write_once(WRITE_CAP);
    s.add(
        "J10-写账",
        fw.ledger_ok() && fw.written_total == WRITE_CAP && fw.free == BUF_CAP - WRITE_CAP,
        "写尽面账平",
    );

    // J11 字面锚：EAGAIN=11 / BUF_CAP=4096 / WRITE_CAP=512
    let j11 = EAGAIN == 11 && BUF_CAP == 4096 && WRITE_CAP == 512;
    s.add("J11-字面锚", j11, "容量与码常量");

    // J12 诊断码闭集
    let j12 = diag_text(FW_ZERO_EAGAIN) == "满态非阻塞写 EAGAIN"
        && diag_text(FW_PARTIAL_FACE) == "零头部分交付面"
        && diag_text(3) == "未知诊断码";
    s.add("J12-码文本闭集", j12, "六码文本可辨");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 写满注记落账
    let note = fw_note(&judge(&FwReq { nonblock: false, want: 200, free: 50 }));
    s.add_dyn("J14-写满注记", note == "got=50", &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10854_flow() {
        assert_eq!(judge(&FwReq { nonblock: true, want: 8, free: 0 }), Res::Err(FW_ZERO_EAGAIN));
        assert_eq!(judge(&FwReq { nonblock: false, want: 8, free: 0 }), Res::Block);
        assert_eq!(judge(&FwReq { nonblock: true, want: 100, free: 40 }), Res::Got(40));
        assert_eq!(judge(&FwReq { nonblock: true, want: 8, free: 16 }), Res::Got(8));
        let mut fw = FullWriter::new(0, false);
        assert_eq!(fw.write_all(5, 4), 5);
        assert!(fw.ledger_ok());
        let mut nb = FullWriter::new(0, true);
        assert_eq!(nb.write_once(3), Res::Err(FW_ZERO_EAGAIN));
        assert_eq!(nb.eagains, 1);
    }

    #[test]
    fn unx10854_checks_green() {
        let s = run_unx10854_checks();
        assert!(s.all_passed(), "UNX-F10854 判据未全绿");
    }
}
