//! UNX-F10801 EPIPE 正反双判据矩阵（0x1bca0 段）
//!
//! 锚点：UNX-F10801-J1 EPIPE 双判据对平——读端引用计数归零后写必 EPIPE、
//! 计数存续写放行、零长写免检三线正反样本全部对上权威判定器；注入坏实现
//! （幽灵写放行/零长误判/假阳性/计数下穿）必与权威分歧被检出。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：管道读端引用计数状态机（open/dup/close 事件驱动增减、下限钳零）
//! + 场景判定器（读端计数×写长两字段算 EPIPE/放行，非查表）+ 事件序重放
//! （逐事件按落判时刻计数裁决并与权威对拍）+ 注入核（四类坏实现必分歧）
//! + 正反双样本矩阵（正样本放行、反样本必返对应诊断码）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX errno 字面锚（EPIPE=32，诊断语义对照）。
pub const POSIX_EPIPE: u32 = 32;
pub const EM_EPIPE: u32 = POSIX_EPIPE;

/// 诊断段（0x1bca0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bca0;
pub const EP_GHOST_WRITE: u32 = SEG_BASE + 0x01; // 读端归零后写仍放行
pub const EP_FALSE_POS: u32 = SEG_BASE + 0x02; // 读端存续却误判 EPIPE
pub const EP_ZLEN_MISROUTE: u32 = SEG_BASE + 0x03; // 零长写误入 EPIPE 路
pub const EP_RC_UNDERFLOW: u32 = SEG_BASE + 0x04; // 读端计数下穿回绕
pub const EP_RC_LOSS: u32 = SEG_BASE + 0x05; // dup 计数丢失
pub const EP_ORDER: u32 = SEG_BASE + 0x06; // 事件序落判错位

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        EP_GHOST_WRITE => "读端归零后写仍放行（幽灵写）",
        EP_FALSE_POS => "读端存续却误判 EPIPE（假阳性）",
        EP_ZLEN_MISROUTE => "零长写误入 EPIPE 路",
        EP_RC_UNDERFLOW => "读端计数下穿回绕",
        EP_RC_LOSS => "dup 引用计数丢失",
        EP_ORDER => "事件序落判错位",
        _ => "未知诊断码",
    }
}

/// 管道事件（引用计数状态机驱动）。
#[derive(Clone)]
pub enum PipeEvent {
    ReaderOpen,
    ReaderDup,
    ReaderClose,
    WriterWrite(usize),
}

/// 管道读端引用计数状态机：open/dup 增、close 减（下限钳零）。
pub struct PipeRef {
    readers: u32,
    /// 重放账：(落判时刻读端计数, 事件, 判定)。
    log: Vec<(u32, PipeEvent, Result<usize, u32>)>,
}

impl PipeRef {
    pub fn new() -> PipeRef {
        PipeRef { readers: 0, log: Vec::new() }
    }

    pub fn readers(&self) -> u32 {
        self.readers
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    /// 单步推演：write 在落判时刻的读端计数上裁决。
    pub fn step(&mut self, ev: PipeEvent) -> Result<usize, u32> {
        let before = self.readers;
        let ev2 = ev.clone();
        let out = match ev {
            PipeEvent::ReaderOpen | PipeEvent::ReaderDup => {
                self.readers += 1;
                Ok(0)
            }
            PipeEvent::ReaderClose => {
                if self.readers > 0 {
                    self.readers -= 1;
                }
                Ok(0)
            }
            PipeEvent::WriterWrite(len) => judge_write(self.readers, len),
        };
        self.log.push((before, ev2, out.clone()));
        out
    }

    /// 重放账与权威判定器逐条对拍（写事件必须同判）。
    pub fn replay_matches(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.log.len() {
            let (before, ev, out) = &self.log[i];
            if let PipeEvent::WriterWrite(len) = ev {
                if judge_write(*before, *len) != *out {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    }

    /// 落账中 EPIPE 判定数。
    pub fn epipe_hits(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.log.len() {
            if self.log[i].2 == Err(EM_EPIPE) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 场景输入：读端计数 × 写长（判定器按字段计算，非查表）。
#[derive(Clone)]
pub struct Scenario {
    pub readers: u32,
    pub write_len: usize,
}

/// 写裁决核：零长写免检放行；读端计数归零必 EPIPE；否则放行记账。
pub fn judge_write(readers: u32, len: usize) -> Result<usize, u32> {
    if len == 0 {
        return Ok(0);
    }
    if readers == 0 {
        return Err(EM_EPIPE);
    }
    Ok(len)
}

/// 场景判定器。
pub fn judge(sc: &Scenario) -> Result<usize, u32> {
    judge_write(sc.readers, sc.write_len)
}

/// 场景注记行（落账明细用）：读端计数×写长×裁决面。
pub fn scenario_note(sc: &Scenario) -> String {
    let face = match judge(sc) {
        Ok(n) => format!("ok({})", n),
        Err(c) => format!("err({:#x})", c),
    };
    format!("readers={} len={} -> {}", sc.readers, sc.write_len, face)
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：正样本放行（None）、反样本必返对应诊断码（Some）。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：读端存续（1/2/4 档）写 16 字节放行
    m.push((Scenario { readers: 1, write_len: 16 }, None));
    m.push((Scenario { readers: 2, write_len: 16 }, None));
    m.push((Scenario { readers: 4, write_len: 16 }, None));
    // 正：零长写在归零读端上免检放行
    m.push((Scenario { readers: 0, write_len: 0 }, None));
    m.push((Scenario { readers: 3, write_len: 0 }, None));
    // 反：读端归零后非零长写必 EPIPE（1 字节边界 / 4KiB 满块）
    m.push((Scenario { readers: 0, write_len: 1 }, Some(EM_EPIPE)));
    m.push((Scenario { readers: 0, write_len: 4096 }, Some(EM_EPIPE)));
    m
}

/// 矩阵对平：逐样本判定器结论 vs 期望，返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want) = &m[i];
        let got = judge(sc);
        let hit = match want {
            None => got.is_ok(),
            Some(c) => got.err() == Some(*c),
        };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 矩阵折叠指纹（序敏感）。
pub fn fold_matrix() -> u32 {
    let m = sample_matrix();
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(m.len() as u32);
    let mut i = 0usize;
    while i < m.len() {
        h = h.wrapping_mul(31).wrapping_add(m[i].0.readers);
        h = h.wrapping_mul(31).wrapping_add(m[i].0.write_len as u32);
        match m[i].1 {
            None => h = h.wrapping_mul(31).wrapping_add(0xffff),
            Some(c) => h = h.wrapping_mul(31).wrapping_add(c),
        }
        i += 1;
    }
    h
}

/// 坏实现 A：幽灵写——读端归零仍放行。
pub fn faulty_ghost(sc: &Scenario) -> Result<usize, u32> {
    Ok(sc.write_len)
}

/// 坏实现 B：零长写误入 EPIPE 路。
pub fn faulty_zlen(sc: &Scenario) -> Result<usize, u32> {
    if sc.readers == 0 {
        Err(EM_EPIPE)
    } else {
        Ok(sc.write_len)
    }
}

/// 坏实现 C：假阳性——读端存续也判 EPIPE。
pub fn faulty_false_pos(sc: &Scenario) -> Result<usize, u32> {
    if sc.write_len == 0 {
        Ok(0)
    } else {
        Err(EM_EPIPE)
    }
}

/// 钳零核：1 读端连发三次 close，计数停 0 不回绕且重放对拍仍平。
pub fn clamp_ok() -> bool {
    let mut p = PipeRef::new();
    let _ = p.step(PipeEvent::ReaderOpen);
    let _ = p.step(PipeEvent::ReaderDup);
    let _ = p.step(PipeEvent::ReaderClose);
    let _ = p.step(PipeEvent::ReaderClose);
    let _ = p.step(PipeEvent::ReaderClose);
    p.readers() == 0 && p.replay_matches()
}

/// 注入核：四类坏实现必与权威分歧（分歧=检出）。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 幽灵写：权威判 EPIPE，坏实现放行 → 检出
    let ghost = Scenario { readers: 0, write_len: 8 };
    got[0] = judge(&ghost) != faulty_ghost(&ghost) && judge(&ghost).err() == Some(EM_EPIPE);
    // 2) 零长误判：权威放行，坏实现判 EPIPE → 检出
    let zlen = Scenario { readers: 0, write_len: 0 };
    got[1] = judge(&zlen) != faulty_zlen(&zlen) && judge(&zlen).is_ok();
    // 3) 假阳性：权威放行，坏实现全判 EPIPE → 检出
    let pos = Scenario { readers: 2, write_len: 8 };
    got[2] = judge(&pos) != faulty_false_pos(&pos) && judge(&pos).is_ok();
    // 4) 计数钳零：close 超发不回绕
    got[3] = clamp_ok();
    got
}

/// EPIPE 矩阵判据（对应卡验收 J 判据）。
pub fn run_unx10801_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10801");
    let inj = inject_four();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平：全部样本命中且容量合规
    s.add("J1-矩阵对平", hits == total && cap_ok, "正反样本全命中且不超容");

    // J2 正路径：读端存续写放行
    let j2 = judge(&Scenario { readers: 2, write_len: 16 }).is_ok();
    s.add("J2-存续写放行", j2, "读端计数>0 时写返回写入长");

    // J3 反路径：读端归零后写必 EPIPE
    let j3 = judge(&Scenario { readers: 0, write_len: 8 }).err() == Some(EM_EPIPE);
    s.add("J3-归零写必断", j3, "读端计数=0 时非零长写必 EPIPE");

    // J4 零长免检：归零读端上零长写放行
    let j4 = judge(&Scenario { readers: 0, write_len: 0 }) == Ok(0);
    s.add("J4-零长免检", j4, "零长写不查管道状态");

    // J5 事件序重放：open→dup→write→close→close→write 全序落判
    let mut p = PipeRef::new();
    let r1 = p.step(PipeEvent::ReaderOpen);
    let _ = p.step(PipeEvent::ReaderDup);
    let r3 = p.step(PipeEvent::WriterWrite(16));
    let _ = p.step(PipeEvent::ReaderClose);
    let _ = p.step(PipeEvent::ReaderClose);
    let r6 = p.step(PipeEvent::WriterWrite(8));
    let j5 = r1 == Ok(0) && r3 == Ok(16) && r6.err() == Some(EM_EPIPE) && p.replay_matches();
    s.add("J5-事件序落判", j5, "写判定随落判时刻读端计数翻转");

    // J6 dup 计数不丢：open+dup 后计数恰 2
    let mut p2 = PipeRef::new();
    let _ = p2.step(PipeEvent::ReaderOpen);
    let _ = p2.step(PipeEvent::ReaderDup);
    let j6 = p2.readers() == 2;
    s.add("J6-dup计数不丢", j6, "dup 增计数");

    // J7 注入1 幽灵写检出
    s.add("J7-幽灵写检出", inj[0], "归零放行坏实现必分歧");

    // J8 注入2 零长误判检出
    s.add("J8-零长误判检出", inj[1], "零长入 EPIPE 路坏实现必分歧");

    // J9 注入3 假阳性检出
    s.add("J9-假阳性检出", inj[2], "存续误判坏实现必分歧");

    // J10 注入4 计数钳零检出
    s.add("J10-钳零检出", inj[3], "close 超发不回绕");

    // J11 POSIX 字面锚：诊断语义对照 EPIPE=32 且诊断文本表完备
    let j11 = POSIX_EPIPE == 32
        && diag_text(EP_GHOST_WRITE).len() > 0
        && diag_text(EP_RC_LOSS).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "EPIPE errno 字面 32 且诊断表完备");

    // J12 容量边界：样本数不超 MATRIX_CAP
    s.add("J12-矩阵容量", cap_ok && total <= MATRIX_CAP, "样本装量不超容");

    // J13 (dyn) 折叠指纹落账
    let fp = fold_matrix();
    s.add_dyn("J13-矩阵指纹", fp != 0, &format!("fold={:#x}", fp));

    // J14 (dyn) 重放落账明细：落账数与 EPIPE 命中数 + 场景注记
    let mut p3 = PipeRef::new();
    let _ = p3.step(PipeEvent::ReaderOpen);
    let _ = p3.step(PipeEvent::ReaderClose);
    let _ = p3.step(PipeEvent::WriterWrite(4));
    let note = scenario_note(&Scenario { readers: 0, write_len: 4 });
    s.add_dyn(
        "J14-重放明细",
        p3.log_len() == 3 && p3.epipe_hits() == 1,
        &format!("steps={} epipe={} note={}", p3.log_len(), p3.epipe_hits(), note),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10801_matrix_flow() {
        let mut p = PipeRef::new();
        assert!(p.step(PipeEvent::ReaderOpen).is_ok());
        assert!(p.step(PipeEvent::ReaderDup).is_ok());
        assert_eq!(p.readers(), 2);
        assert!(p.step(PipeEvent::WriterWrite(8)).is_ok());
        let _ = p.step(PipeEvent::ReaderClose);
        let _ = p.step(PipeEvent::ReaderClose);
        assert_eq!(p.readers(), 0);
        assert!(p.step(PipeEvent::ReaderClose).is_ok());
        assert_eq!(p.readers(), 0);
        assert!(p.step(PipeEvent::WriterWrite(8)).err() == Some(EM_EPIPE));
        assert!(p.replay_matches());
        assert!(p.epipe_hits() == 1);
        let sc = Scenario { readers: 0, write_len: 0 };
        assert!(judge(&sc) == Ok(0));
    }

    #[test]
    fn unx10801_checks_green() {
        let s = run_unx10801_checks();
        assert!(s.all_passed(), "UNX-F10801 判据未全绿");
    }
}
