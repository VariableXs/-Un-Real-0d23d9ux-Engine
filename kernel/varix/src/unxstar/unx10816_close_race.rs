//! UNX-F10816 对端关闭竞态（write 与 close 事件序）（0x1be80 段）
//!
//! 锚点：UNX-F10816-J1 事件序对平——close 前的 write 放行、close 后的
//! write 必 EPIPE（SIGPIPE 注记）、序内逐事件重放与权威同判三线全对平；
//! 注入坏实现（无视 close 序/超前 EPIPE）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：事件账（seq 严格递增 × read 端存续 × write 事件）+ 竞态判定器
//! （按 seq 对撞读端生死算放行/EPIPE）+ 重放核（逐事件先验场景同判）+
//! 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EPIPE=32、SIGPIPE=13。
pub const POSIX_EPIPE: u32 = 32;
pub const SIGPIPE: u32 = 13;

/// 诊断段（0x1be80 独占 0x20）。
pub const SEG_BASE: u32 = 0x1be80;
pub const CR_ORDER_MISS: u32 = SEG_BASE + 0x01; // close 序漏查
pub const CR_EARLY_EPIPE: u32 = SEG_BASE + 0x02; // close 前误报 EPIPE
pub const CR_SEQ_SKEW: u32 = SEG_BASE + 0x03; // 事件序错账
pub const CR_SIG_LOSS: u32 = SEG_BASE + 0x04; // SIGPIPE 注记丢失
pub const CR_DUP_SEQ: u32 = SEG_BASE + 0x05; // 序号重复
pub const CR_LIFE_SKEW: u32 = SEG_BASE + 0x06; // 读端生死账错

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        CR_ORDER_MISS => "close 序漏查",
        CR_EARLY_EPIPE => "close 前误报 EPIPE",
        CR_SEQ_SKEW => "事件序错账",
        CR_SIG_LOSS => "SIGPIPE 注记丢失",
        CR_DUP_SEQ => "事件序号重复",
        CR_LIFE_SKEW => "读端生死账错",
        _ => "未知诊断码",
    }
}

/// 事件类型。
#[derive(Clone, PartialEq, Eq)]
pub enum Ev {
    /// 写事件（携带序号与写长）。
    Write(u64, usize),
    /// 读端全关事件（携带序号）。
    ReaderClose(u64),
}

/// 事件判定输出。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 放行：写进 n 字节。
    Pass(usize),
    /// EPIPE + SIGPIPE 注记。
    Broken,
}

/// 场景输入：事件序 × 读端生死快照。
#[derive(Clone)]
pub struct Scenario {
    pub seq: u64,
    pub readers_alive: bool,
    pub want: usize,
}

/// 竞态判定核：读端存续放行；读端已亡（close 序在前）必 EPIPE+SIGPIPE。
pub fn judge(sc: &Scenario) -> Verdict {
    if sc.want == 0 {
        return Verdict::Pass(0);
    }
    if sc.readers_alive {
        Verdict::Pass(sc.want)
    } else {
        Verdict::Broken
    }
}

/// 事件账状态机：seq 递增落账，ReaderClose 翻转读端生死，Write 按快照裁决。
pub struct EvLedger {
    pub readers_alive: bool,
    pub last_seq: u64,
    pub rows: Vec<(u64, String, Verdict)>,
    pub note: String,
}

impl EvLedger {
    pub fn new() -> EvLedger {
        EvLedger {
            readers_alive: true,
            last_seq: 0,
            rows: Vec::new(),
            note: "open".to_string(),
        }
    }

    /// 事件序合法性：seq 必须严格递增（重复/回退拒绝）。
    pub fn seq_valid(&mut self, seq: u64) -> bool {
        seq > self.last_seq
    }

    /// 落一笔事件：Write 判决入账，ReaderClose 翻生死账。
    pub fn feed(&mut self, ev: &Ev) -> Option<Verdict> {
        let (seq, tag, want) = match ev {
            Ev::Write(s, w) => (*s, "write", *w),
            Ev::ReaderClose(s) => (*s, "rclose", 0),
        };
        if !self.seq_valid(seq) {
            return None;
        }
        self.last_seq = seq;
        if tag == "rclose" {
            self.readers_alive = false;
            self.note = "readers-gone".to_string();
            self.rows.push((seq, tag.to_string(), Verdict::Pass(0)));
            return None;
        }
        let sc = Scenario { seq, readers_alive: self.readers_alive, want };
        let v = judge(&sc);
        self.rows.push((seq, tag.to_string(), v.clone()));
        Some(v)
    }

    /// 重放核：逐行按落账时刻快照与权威同判。
    pub fn replay_ok(&self) -> bool {
        let mut ok = true;
        let mut alive = true;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (seq, tag, v) = &self.rows[i];
            let sc = Scenario { seq: *seq, readers_alive: alive, want: 8 };
            let want_v = if *tag == "rclose" { Verdict::Pass(0) } else { judge(&sc) };
            if *v != want_v {
                ok = false;
            }
            if *tag == "rclose" {
                alive = false;
            }
            i += 1;
        }
        ok
    }

    /// 序号严格递增核：账内 seq 无重复无回退。
    pub fn seq_strict(&self) -> bool {
        let mut ok = true;
        let mut prev = 0u64;
        let mut i = 0usize;
        while i < self.rows.len() {
            let (seq, _tag, _v) = &self.rows[i];
            if i > 0 && *seq <= prev {
                ok = false;
            }
            prev = *seq;
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望裁决。
pub fn sample_matrix() -> Vec<(Scenario, Verdict)> {
    let mut m: Vec<(Scenario, Verdict)> = Vec::new();
    // 正：close 前 write 放行
    m.push((
        Scenario { seq: 1, readers_alive: true, want: 64 },
        Verdict::Pass(64),
    ));
    // 正：零长写免检（读端已亡也 Pass）
    m.push((
        Scenario { seq: 2, readers_alive: false, want: 0 },
        Verdict::Pass(0),
    ));
    // 反：close 后 write 必 EPIPE
    m.push((
        Scenario { seq: 3, readers_alive: false, want: 8 },
        Verdict::Broken,
    ));
    // 边界：close 前 1 字节放行
    m.push((
        Scenario { seq: 4, readers_alive: true, want: 1 },
        Verdict::Pass(1),
    ));
    // 边界：close 后大块仍 Broken（不分片）
    m.push((
        Scenario { seq: 5, readers_alive: false, want: 8192 },
        Verdict::Broken,
    ));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want) = &m[i];
        if &judge(sc) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：无视 close 序（读端已亡仍放行）。
pub fn faulty_ignore_close(sc: &Scenario) -> Verdict {
    Verdict::Pass(sc.want)
}

/// 坏实现 B：超前 EPIPE（读端存续也报 EPIPE）。
pub fn faulty_early_epipe(sc: &Scenario) -> Verdict {
    if sc.want > 0 {
        return Verdict::Broken;
    }
    judge(sc)
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) close 后 write：权威 Broken、坏实现放行 → 检出
    let sc1 = Scenario { seq: 9, readers_alive: false, want: 8 };
    got[0] = judge(&sc1) != faulty_ignore_close(&sc1) && judge(&sc1) == Verdict::Broken;
    // 2) close 前 write：权威 Pass、坏实现 Broken → 检出
    let sc2 = Scenario { seq: 10, readers_alive: true, want: 8 };
    got[1] = judge(&sc2) != faulty_early_epipe(&sc2) && judge(&sc2) == Verdict::Pass(8);
    got
}

/// 全序竞态核：write(1) → write(2) → rclose → write(3) 逐笔落判对平。
pub fn event_order_ok() -> bool {
    let mut lg = EvLedger::new();
    let v1 = lg.feed(&Ev::Write(1, 16));
    let v2 = lg.feed(&Ev::Write(2, 16));
    let _ = lg.feed(&Ev::ReaderClose(3));
    let v3 = lg.feed(&Ev::Write(4, 16));
    let c1 = v1 == Some(Verdict::Pass(16)) && v2 == Some(Verdict::Pass(16));
    let c2 = v3 == Some(Verdict::Broken);
    let c3 = lg.rows.len() == 4 && !lg.readers_alive && lg.note == "readers-gone";
    let c4 = lg.replay_ok() && lg.seq_strict();
    c1 && c2 && c3 && c4
}

/// 序号防御核：重复/回退序号拒绝落账。
pub fn seq_guard_ok() -> bool {
    let mut lg = EvLedger::new();
    let v1 = lg.feed(&Ev::Write(5, 8));
    let v2 = lg.feed(&Ev::Write(5, 8));
    let v3 = lg.feed(&Ev::Write(4, 8));
    v1.is_some() && v2.is_none() && v3.is_none() && lg.rows.len() == 1
}

/// 事件面注记（对账/落账用）。
pub fn verdict_note(v: &Verdict) -> String {
    match v {
        Verdict::Pass(n) => format!("pass({})", n),
        Verdict::Broken => format!("broken epipe={} sig={}", POSIX_EPIPE, SIGPIPE),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10816_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10816");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "五线竞态样本全命中");

    // J2 正路径：close 前 write 放行
    let j2 = judge(&Scenario { seq: 1, readers_alive: true, want: 32 }) == Verdict::Pass(32);
    s.add("J2-close前放行", j2, "读端存续写通行");

    // J3 反路径：close 后 write 必 EPIPE
    let j3 = judge(&Scenario { seq: 2, readers_alive: false, want: 8 }) == Verdict::Broken;
    s.add("J3-close后EPIPE", j3, "读端亡写必破");

    // J4 边界：零长写免检（读端已亡也放行）
    let j4 = judge(&Scenario { seq: 3, readers_alive: false, want: 0 }) == Verdict::Pass(0);
    s.add("J4-零长免检", j4, "零长不触发破管");

    // J5 事件全序核：write→write→rclose→write
    s.add("J5-事件全序核", event_order_ok(), "四步逐笔落判重放对平");

    // J6 注入1 无视 close 检出
    s.add("J6-漏序检出", inj[0], "读端亡仍放行坏实现必分歧");

    // J7 注入2 超前 EPIPE 检出
    s.add("J7-超前检出", inj[1], "读端存续误破坏实现必分歧");

    // J8 序号防御核：重复/回退拒绝
    s.add("J8-序号防御", seq_guard_ok(), "seq 非严格递增拒落账");

    // J9 SIGPIPE 注记：Broken 携 13 号语义
    let n = verdict_note(&Verdict::Broken);
    let j9 = n.contains("epipe=32") && n.contains("sig=13");
    s.add("J9-SIGPIPE注记", j9, "破管面 32/13 双锚入注");

    // J10 字面锚：EPIPE=32 / SIGPIPE=13
    let j10 = POSIX_EPIPE == 32 && SIGPIPE == 13 && diag_text(CR_ORDER_MISS).len() > 0;
    s.add("J10-字面锚对平", j10, "32/13 双锚且语义完备");

    // J11 重放对拍核
    let mut lg2 = EvLedger::new();
    let _ = lg2.feed(&Ev::Write(1, 4));
    let _ = lg2.feed(&Ev::ReaderClose(2));
    let _ = lg2.feed(&Ev::Write(3, 4));
    let j11 = lg2.replay_ok() && lg2.rows.len() == 3;
    s.add("J11-重放对拍", j11, "三行账逐条同判");

    // J12 裁决闭集：Pass/Broken 两值互斥
    let a = judge(&Scenario { seq: 1, readers_alive: true, want: 4 });
    let b = judge(&Scenario { seq: 2, readers_alive: false, want: 4 });
    let j12 = a != b && verdict_note(&a) != verdict_note(&b);
    s.add("J12-裁决互斥", j12, "两裁决注记互异");

    // J13 (dyn) 事件账指纹落账
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.last_seq as u32);
    h = h.wrapping_mul(31).wrapping_add(lg2.rows.len() as u32);
    h = h.wrapping_mul(31).wrapping_add(lg2.readers_alive as u32);
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-事件账指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细
    s.add_dyn(
        "J14-命中明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10816_race_flow() {
        let mut lg = EvLedger::new();
        assert_eq!(lg.feed(&Ev::Write(1, 8)), Some(Verdict::Pass(8)));
        let _ = lg.feed(&Ev::ReaderClose(2));
        assert_eq!(lg.feed(&Ev::Write(3, 8)), Some(Verdict::Broken));
        assert!(lg.feed(&Ev::Write(3, 8)).is_none());
        assert!(lg.seq_strict() && lg.replay_ok());
        let sc = Scenario { seq: 9, readers_alive: false, want: 0 };
        assert_eq!(judge(&sc), Verdict::Pass(0));
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10816_checks_green() {
        let s = run_unx10816_checks();
        assert!(s.all_passed(), "UNX-F10816 判据未全绿");
    }
}
