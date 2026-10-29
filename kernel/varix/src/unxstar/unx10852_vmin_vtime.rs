//! UNX-F10852 VMIN/VTIME 非规范读矩阵（四组合）（0x1c300 段）
//!
//! 锚点：UNX-F10852-J1 非规范读四组合矩阵——(VMIN>0,VTIME=0) 恒待满
//! VMIN 字节（不足必 Block 非提前交付）；(VMIN=0,VTIME>0) 定时器到点
//! 回现量（无数据到点回 0）；(VMIN>0,VTIME>0) 满 VMIN 或字节间超时
//! 交付；(VMIN=0,VTIME=0) 即时回现量（无数据回 0）。判定依赖场景
//! 字段演算（tick 演进），注入核（提前交付/超时反转）必分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：读场景（vmin × vtime × 现量 × tick）→ 四组合判定器（字段
//! 演算非查表）→ tick 演进状态机（while 三要素推进 + 到量守恒账）
//! → 组合样本矩阵 → 注入核（两坏实现必分歧）→ 演进回放（三 tick 段对平）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1c300 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c300;
pub const NC_EARLY_RET: u32 = SEG_BASE + 0x01; // VMIN 未满提前交付面
pub const NC_TIMEOUT_INV: u32 = SEG_BASE + 0x02; // 超时反转面
pub const NC_BLOCK_FACE: u32 = SEG_BASE + 0x03; // 待足面
pub const NC_ZERO_FACE: u32 = SEG_BASE + 0x04; // 到点零字节面
pub const NC_LEDGER_LOST: u32 = SEG_BASE + 0x05; // 演进账丢失
pub const NC_OK_FACE: u32 = SEG_BASE + 0x06; // 交付放行面

/// 诊断码语义文本。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        NC_EARLY_RET => "VMIN 未满提前交付",
        NC_TIMEOUT_INV => "超时反转",
        NC_BLOCK_FACE => "待足面",
        NC_ZERO_FACE => "到点零字节",
        NC_LEDGER_LOST => "演进账丢失",
        NC_OK_FACE => "交付放行",
        _ => "未知诊断码",
    }
}

/// 读场景：VMIN × VTIME × 现量 × 已历 tick（判定器四输入）。
#[derive(Clone)]
pub struct NcReq {
    pub vmin: u32,
    pub vtime: u32,
    pub avail: u32,
    pub ticks: u32,
}

/// 判定结果：Got(交付字节数) / Block（待足面）。
#[derive(Clone, PartialEq, Debug)]
pub enum Res {
    Got(u32),
    Block,
}

/// 四组合判定器：全分支依赖场景字段演算。
/// A=(VMIN>0,VTIME=0) 恒待满；B=(VMIN=0,VTIME>0) 定时器到点；
/// D=(VMIN=0,VTIME=0) 即时回；C=其余（VMIN>0,VTIME>0）满 VMIN 或
/// 字节间到点回零头。
pub fn judge(req: &NcReq) -> Res {
    match (req.vmin, req.vtime) {
        (v, 0) if v > 0 => {
            if req.avail >= v {
                Res::Got(v)
            } else {
                Res::Block
            }
        }
        (0, t) if t > 0 => {
            if req.avail > 0 {
                Res::Got(req.avail)
            } else if req.ticks >= t {
                Res::Got(0)
            } else {
                Res::Block
            }
        }
        (0, 0) => {
            if req.avail > 0 {
                Res::Got(req.avail)
            } else {
                Res::Got(0)
            }
        }
        (v, _) => {
            if req.avail >= v {
                Res::Got(v)
            } else if req.avail > 0 && req.ticks >= req.vtime {
                Res::Got(req.avail)
            } else {
                Res::Block
            }
        }
    }
}

/// tick 演进状态机：每 tick 到量 arrive，读方按组合判定器解析。
pub struct NcReader {
    pub vmin: u32,
    pub vtime: u32,
    pub avail: u32,
    pub arrived: u32,
    pub delivered: u32,
    pub ticks_used: u32,
    pub note: String,
}

impl NcReader {
    pub fn new(vmin: u32, vtime: u32, avail: u32) -> NcReader {
        NcReader {
            vmin,
            vtime,
            avail,
            arrived: 0,
            delivered: 0,
            ticks_used: 0,
            note: "idle".to_string(),
        }
    }

    /// 单步：交付或（待足时）推进一个 tick 并接收 arrive 字节。
    pub fn step(&mut self, arrive: u32) -> Res {
        let req = NcReq {
            vmin: self.vmin,
            vtime: self.vtime,
            avail: self.avail,
            ticks: self.ticks_used,
        };
        match judge(&req) {
            Res::Got(n) => {
                self.avail -= n;
                self.delivered += n;
                self.note = "delivered".to_string();
                Res::Got(n)
            }
            Res::Block => {
                self.ticks_used += 1;
                self.avail += arrive;
                self.arrived += arrive;
                self.note = "ticking".to_string();
                Res::Block
            }
        }
    }

    /// 演进到交付：while init(步数 0)/cond(未交付且未封顶)/incr(单步)。
    pub fn resolve(&mut self, cap: u32, arrive: u32) -> Res {
        let mut steps = 0u32;
        let mut last = Res::Block;
        while steps < cap {
            last = self.step(arrive);
            match last {
                Res::Got(_) => break,
                Res::Block => {}
            }
            steps += 1;
        }
        last
    }

    /// 演进账一致（到量守恒）：交付+现量=初始量+到达量。
    pub fn ledger_ok(&self, init: u32) -> bool {
        self.delivered + self.avail == init + self.arrived
    }
}

/// 样本矩阵容量。
pub const MATRIX_CAP: usize = 24;

/// 矩阵逐格判定：期望（Got(n) 编码 n+1，Block 编码 0）命中计数。
pub fn verify_matrix(reqs: &[NcReq], wants: &[u32]) -> (usize, usize, bool) {
    let mut hits = 0usize;
    let total = reqs.len();
    let mut i = 0usize;
    while i < total && i < MATRIX_CAP {
        let want = if i < wants.len() { wants[i] } else { 0 };
        let hit = match judge(&reqs[i]) {
            Res::Got(n) => want == n + 1,
            Res::Block => want == 0,
        };
        if hit {
            hits += 1;
        }
        i += 1;
    }
    (hits, total.min(MATRIX_CAP), hits == total.min(MATRIX_CAP))
}

/// 样本矩阵：四组合 × 现量/tick 沿，10 行全分账。
pub fn sample_matrix() -> (Vec<NcReq>, Vec<u32>) {
    let mut reqs: Vec<NcReq> = Vec::new();
    let mut wants: Vec<u32> = Vec::new();
    // R0 组合一：满 VMIN 交付
    reqs.push(NcReq { vmin: 5, vtime: 0, avail: 8, ticks: 0 });
    wants.push(6);
    // R1 组合一：不足 VMIN 待
    reqs.push(NcReq { vmin: 5, vtime: 0, avail: 4, ticks: 0 });
    wants.push(0);
    // R2 组合二：有数据即回
    reqs.push(NcReq { vmin: 0, vtime: 3, avail: 2, ticks: 0 });
    wants.push(3);
    // R3 组合二：无数据未到点待
    reqs.push(NcReq { vmin: 0, vtime: 3, avail: 0, ticks: 1 });
    wants.push(0);
    // R4 组合二：无数据到点回 0
    reqs.push(NcReq { vmin: 0, vtime: 3, avail: 0, ticks: 3 });
    wants.push(1);
    // R5 组合三：满 VMIN 交付 VMIN
    reqs.push(NcReq { vmin: 4, vtime: 2, avail: 9, ticks: 0 });
    wants.push(5);
    // R6 组合三：有零头且到点回零头
    reqs.push(NcReq { vmin: 4, vtime: 2, avail: 2, ticks: 2 });
    wants.push(3);
    // R7 组合三：有零头未到点待
    reqs.push(NcReq { vmin: 4, vtime: 2, avail: 2, ticks: 1 });
    wants.push(0);
    // R8 组合四：即时回现量
    reqs.push(NcReq { vmin: 0, vtime: 0, avail: 6, ticks: 0 });
    wants.push(7);
    // R9 组合四：零现量即时回 0
    reqs.push(NcReq { vmin: 0, vtime: 0, avail: 0, ticks: 0 });
    wants.push(1);
    (reqs, wants)
}

/// 注入 A：坏实现组合一提前交付（不足 VMIN 亦回现量）。
pub fn faulty_early_ret(req: &NcReq) -> Res {
    if req.vmin > 0 && req.vtime == 0 && req.avail > 0 {
        Res::Got(req.avail)
    } else {
        judge(req)
    }
}

/// 注入 B：坏实现组合四超时反转（有数据回 0、无数据回哨兵 99）。
pub fn faulty_timeout_inv(req: &NcReq) -> Res {
    if req.vmin == 0 && req.vtime == 0 {
        if req.avail > 0 {
            Res::Got(0)
        } else {
            Res::Got(99)
        }
    } else {
        judge(req)
    }
}

/// 注入核：两处坏账与判定器必分歧。
pub fn inject_two() -> [bool; 2] {
    let early = NcReq { vmin: 5, vtime: 0, avail: 2, ticks: 0 };
    let zero = NcReq { vmin: 0, vtime: 0, avail: 4, ticks: 0 };
    let d1 = judge(&early) != faulty_early_ret(&early);
    let d2 = judge(&zero) != faulty_timeout_inv(&zero);
    [d1, d2]
}

/// 演进回放：组合一逐 tick 到量 1 字节，直至满 VMIN 交付。
pub fn resolve_replay_ok() -> bool {
    let mut rd = NcReader::new(4, 0, 1);
    let r = rd.resolve(8, 1);
    r == Res::Got(4) && rd.delivered == 4 && rd.avail == 0 && rd.ticks_used == 3
        && rd.ledger_ok(1) && rd.note == "delivered"
}

/// 组合二到点回放：无到达 tick 推进到点回 0。
pub fn timeout_replay_ok() -> bool {
    let mut rd = NcReader::new(0, 3, 0);
    let r = rd.resolve(6, 0);
    r == Res::Got(0) && rd.ticks_used == 3 && rd.delivered == 0 && rd.ledger_ok(0)
}

/// 四组合注记：判定渲染（A/B/C/D 标签 + 交付量）。
pub fn combo_note(req: &NcReq, r: &Res) -> String {
    let combo = match (req.vmin, req.vtime) {
        (v, 0) if v > 0 => "A",
        (0, t) if t > 0 => "B",
        (0, 0) => "D",
        _ => "C",
    };
    match r {
        Res::Got(n) => format!("{}:got={}", combo, n),
        Res::Block => format!("{}:block", combo),
    }
}

/// 域判据（对应卡验收 J 判据）。
pub fn run_unx10852_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10852");
    let (reqs, wants) = sample_matrix();
    let (hits, total, cap_ok) = verify_matrix(&reqs, &wants);

    // J1 组合一正路径：满 VMIN 交付恰 VMIN 字节
    let j1 = judge(&NcReq { vmin: 5, vtime: 0, avail: 8, ticks: 0 }) == Res::Got(5);
    s.add("J1-组合一交付", j1, "VMIN 满足即回");

    // J2 组合一边界：恰满交付 / 恰缺待
    let j2 = judge(&NcReq { vmin: 5, vtime: 0, avail: 5, ticks: 0 }) == Res::Got(5)
        && judge(&NcReq { vmin: 5, vtime: 0, avail: 4, ticks: 0 }) == Res::Block;
    s.add("J2-VMIN 边界", j2, "5/4 两沿可辨");

    // J3 组合二边界：到点恰回 0 / 未到点待
    let j3 = judge(&NcReq { vmin: 0, vtime: 3, avail: 0, ticks: 3 }) == Res::Got(0)
        && judge(&NcReq { vmin: 0, vtime: 3, avail: 0, ticks: 2 }) == Res::Block;
    s.add("J3-到点边界", j3, "3/2 tick 两沿可辨");

    // J4 组合三语义：满 VMIN 压过到点零头
    let j4 = judge(&NcReq { vmin: 4, vtime: 2, avail: 9, ticks: 0 }) == Res::Got(4)
        && judge(&NcReq { vmin: 4, vtime: 2, avail: 2, ticks: 2 }) == Res::Got(2)
        && judge(&NcReq { vmin: 4, vtime: 2, avail: 0, ticks: 5 }) == Res::Block;
    s.add("J4-组合三三态", j4, "满/零头到点/无数据三态");

    // J5 组合四语义：即时回现量，零回 0
    let j5 = judge(&NcReq { vmin: 0, vtime: 0, avail: 6, ticks: 0 }) == Res::Got(6)
        && judge(&NcReq { vmin: 0, vtime: 0, avail: 0, ticks: 0 }) == Res::Got(0);
    s.add("J5-组合四即时", j5, "poll 型语义");

    // J6 演进回放（组合一逐步到位）
    s.add("J6-演进回放", resolve_replay_ok(), "3 tick 后满 VMIN 交付");

    // J7 组合二到点回放
    s.add("J7-到点回放", timeout_replay_ok(), "3 tick 到点回 0");

    // J8 样本矩阵全命中
    s.add("J8-矩阵对平", cap_ok, "10 行四组合全命中");

    // J9 注入1 提前交付检出
    let inj = inject_two();
    s.add("J9-提前交付检出", inj[0], "VMIN 未满误回必分歧");

    // J10 注入2 超时反转检出
    s.add("J10-超时反转检出", inj[1], "组合四反转必分歧");

    // J11 演进账一致（无到达守恒）
    let mut rd = NcReader::new(2, 0, 5);
    let _ = rd.resolve(4, 0);
    s.add("J11-演进账", rd.ledger_ok(5) && rd.delivered == 2 && rd.avail == 3, "交付+现量=初始");

    // J12 组合注记闭集：四组合标签可辨
    let j12 = combo_note(&NcReq { vmin: 3, vtime: 0, avail: 0, ticks: 0 }, &Res::Block) == "A:block"
        && combo_note(&NcReq { vmin: 0, vtime: 3, avail: 0, ticks: 0 }, &Res::Block) == "B:block"
        && combo_note(&NcReq { vmin: 3, vtime: 3, avail: 0, ticks: 0 }, &Res::Block) == "C:block"
        && combo_note(&NcReq { vmin: 0, vtime: 0, avail: 0, ticks: 0 }, &Res::Block) == "D:block";
    s.add("J12-组合标签", j12, "A/B/C/D 四标签闭集");

    // J13 (dyn) 矩阵明细落账
    s.add_dyn("J13-矩阵明细", hits == total, &format!("hits={}/{} cap_ok={}", hits, total, cap_ok));

    // J14 (dyn) 组合注记落账
    let note = combo_note(&NcReq { vmin: 2, vtime: 1, avail: 2, ticks: 0 }, &Res::Got(2));
    s.add_dyn("J14-组合注记", note == "C:got=2", &note);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10852_flow() {
        assert_eq!(judge(&NcReq { vmin: 3, vtime: 0, avail: 2, ticks: 0 }), Res::Block);
        assert_eq!(judge(&NcReq { vmin: 3, vtime: 0, avail: 3, ticks: 0 }), Res::Got(3));
        assert_eq!(judge(&NcReq { vmin: 0, vtime: 2, avail: 0, ticks: 2 }), Res::Got(0));
        assert_eq!(judge(&NcReq { vmin: 0, vtime: 2, avail: 0, ticks: 1 }), Res::Block);
        assert_eq!(judge(&NcReq { vmin: 0, vtime: 0, avail: 5, ticks: 0 }), Res::Got(5));
        assert_eq!(judge(&NcReq { vmin: 0, vtime: 0, avail: 0, ticks: 0 }), Res::Got(0));
        let mut rd = NcReader::new(2, 0, 1);
        assert_eq!(rd.step(1), Res::Block);
        assert_eq!(rd.ticks_used, 1);
        assert_eq!(rd.step(1), Res::Got(2));
        assert_eq!(rd.delivered, 2);
        assert_eq!(rd.avail, 0);
        assert!(rd.ledger_ok(1));
    }

    #[test]
    fn unx10852_checks_green() {
        let s = run_unx10852_checks();
        assert!(s.all_passed(), "UNX-F10852 判据未全绿");
    }
}
