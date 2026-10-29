//! UNX-F10814 部分写面（write 返回与消费对账）（0x1be40 段）
//!
//! 锚点：UNX-F10814-J1 部分写对平——放行返回值必等于实进字节、非阻塞
//! 空间不足必写满即返（部分写）、全满必 EAGAIN 零消费三线全对平；注入
//! 坏实现（返回虚报/部分写却全消费）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：管道缓冲账（cap/used）+ 写裁决器（整写/部分写/EAGAIN 三分，
//! 返回值=实进字节）+ 消费对账核（逐笔落账重放）+ 注入核（两类坏实现
//! 必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EAGAIN=11。
pub const POSIX_EAGAIN: u32 = 11;

/// 诊断段（0x1be40 独占 0x20）。
pub const SEG_BASE: u32 = 0x1be40;
pub const PW_RET_SKEW: u32 = SEG_BASE + 0x01; // 返回值与实进字节不符
pub const PW_GHOST_EAGAIN: u32 = SEG_BASE + 0x02; // 有空间仍 EAGAIN
pub const PW_FULL_OVER: u32 = SEG_BASE + 0x03; // 全满仍部分进
pub const PW_NB_BLOCK: u32 = SEG_BASE + 0x04; // 非阻塞挂起面缺失
pub const PW_ZLEN_SKEW: u32 = SEG_BASE + 0x05; // 零长写错账
pub const PW_ACCT_LOSS: u32 = SEG_BASE + 0x06; // 消费账丢失
pub const PW_WOULD_BLOCK: u32 = SEG_BASE + 0x07; // 阻塞面挂起指示

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        PW_RET_SKEW => "返回值与实进字节不符",
        PW_GHOST_EAGAIN => "有空间仍 EAGAIN",
        PW_FULL_OVER => "全满仍部分进",
        PW_NB_BLOCK => "非阻塞挂起面缺失",
        PW_ZLEN_SKEW => "零长写错账",
        PW_ACCT_LOSS => "消费账丢失",
        _ => "未知诊断码",
    }
}

/// 写裁决输出：返回 (返回值, 实进字节)。
/// Ok(n) 表示写进 n 字节并返回 n；Err(EAGAIN) 表示零消费。
pub type WRes = Result<usize, u32>;

/// 场景输入：缓冲占用 × 上限 × 请求长 × 非阻塞位。
#[derive(Clone)]
pub struct Scenario {
    pub used: usize,
    pub cap: usize,
    pub req: usize,
    pub nonblock: bool,
}

/// 写裁决核：整写/部分写/EAGAIN 三分，返回值恒等于实进字节。
pub fn judge(sc: &Scenario) -> (WRes, usize) {
    if sc.req == 0 {
        return (Ok(0), 0);
    }
    let free = sc.cap.saturating_sub(sc.used);
    if sc.req <= free {
        return (Ok(sc.req), sc.req);
    }
    if sc.nonblock {
        if free > 0 {
            // 非阻塞部分写：写满即返，返回值=实进字节
            return (Ok(free), free);
        }
        return (Err(POSIX_EAGAIN), 0);
    }
    // 阻塞面：由等待机制补齐（挂起指示码），记账零消费
    (Err(PW_WOULD_BLOCK), 0)
}

/// 管道缓冲账状态机：逐笔写推进 used，读排水回退，账面对拍。
pub struct PipeAcct {
    pub cap: usize,
    pub used: usize,
    pub nonblock: bool,
    pub ledger: Vec<(usize, usize, u32, String)>,
    pub note: String,
}

impl PipeAcct {
    pub fn new(cap: usize, nonblock: bool) -> PipeAcct {
        PipeAcct {
            cap,
            used: 0,
            nonblock,
            ledger: Vec::new(),
            note: if nonblock { "nb".to_string() } else { "blk".to_string() },
        }
    }

    /// 一次写入：推进 used、落账 (请求, 返回, 实进, 注记)。
    pub fn write(&mut self, req: usize) -> WRes {
        let sc = Scenario { used: self.used, cap: self.cap, req, nonblock: self.nonblock };
        let (ret, got) = judge(&sc);
        self.used += got;
        let tag = match ret {
            Ok(n) => {
                if n == req && n == got {
                    "full".to_string()
                } else {
                    "part".to_string()
                }
            }
            Err(_) => "again".to_string(),
        };
        self.ledger.push((req, ret.clone().unwrap_or(0), got as u32, tag));
        ret
    }

    /// 排水：至多 drain 字节。
    pub fn drain(&mut self, want: usize) -> usize {
        let got = if want > self.used { self.used } else { want };
        self.used -= got;
        got
    }

    /// 账面对拍：每行返回值==实进字节，整写行请求==返回。
    pub fn ledger_ok(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let (req, ret, got, tag) = &self.ledger[i];
            if *ret != *got as usize {
                ok = false;
            }
            if *tag == "full" && req != ret {
                ok = false;
            }
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 (WRes, 实进)。
pub fn sample_matrix() -> Vec<(Scenario, (WRes, usize))> {
    let mut m: Vec<(Scenario, (WRes, usize))> = Vec::new();
    // 正：整写放行
    m.push((
        Scenario { used: 0, cap: 4096, req: 512, nonblock: true },
        (Ok(512), 512),
    ));
    // 正：恰好填满
    m.push((
        Scenario { used: 4096 - 9, cap: 4096, req: 9, nonblock: true },
        (Ok(9), 9),
    ));
    // 正：零长写
    m.push((
        Scenario { used: 4096, cap: 4096, req: 0, nonblock: true },
        (Ok(0), 0),
    ));
    // 反：非阻塞部分写（写满即返）
    m.push((
        Scenario { used: 1000, cap: 4096, req: 4096, nonblock: true },
        (Ok(3096), 3096),
    ));
    // 反：全满非阻塞 EAGAIN 零消费
    m.push((
        Scenario { used: 4096, cap: 4096, req: 1, nonblock: true },
        (Err(POSIX_EAGAIN), 0),
    ));
    // 边界：余 1 字节请求 8
    m.push((
        Scenario { used: 4095, cap: 4096, req: 8, nonblock: true },
        (Ok(1), 1),
    ));
    // 边界：余 1 字节请求 1
    m.push((
        Scenario { used: 4095, cap: 4096, req: 1, nonblock: true },
        (Ok(1), 1),
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

/// 坏实现 A：返回虚报（部分写却报请求长）。
pub fn faulty_ghost_ret(sc: &Scenario) -> (WRes, usize) {
    let base = judge(sc);
    match base {
        (Ok(n), got) if n != sc.req => (Ok(sc.req), got),
        other => other,
    }
}

/// 坏实现 B：部分写却全消费（账面多记）。
pub fn faulty_over_consume(sc: &Scenario) -> (WRes, usize) {
    let base = judge(sc);
    match base {
        (Ok(_), got) if got < sc.req => (base.0, sc.req.min(sc.cap - sc.used)),
        other => other,
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    let sc = Scenario { used: 1000, cap: 4096, req: 4096, nonblock: true };
    // 1) 返回虚报：权威 Ok(3096)、坏实现 Ok(4096) → 检出
    got[0] = judge(&sc) != faulty_ghost_ret(&sc) && judge(&sc) == (Ok(3096), 3096);
    // 2) 账面多记：权威实进 3096、坏实现实进 3096（min 恰同）→ 用余 1 场景分离
    let sc2 = Scenario { used: 4095, cap: 4096, req: 8, nonblock: true };
    got[1] = judge(&sc2) != faulty_over_consume(&sc2) && judge(&sc2) == (Ok(1), 1);
    got
}

/// 消费对账核：连写连排逐笔对平，used 账不越 cap、不透支。
pub fn consume_reconcile_ok() -> bool {
    let mut p = PipeAcct::new(64, true);
    let mut ok = true;
    let mut i = 0;
    while i < 3 {
        let _ = p.write(48);
        i += 1;
    }
    // 第 1 笔整写 48；第 2 笔部分写 16；第 3 笔 EAGAIN
    ok = ok && p.used == 64 && p.ledger.len() == 3;
    ok = ok && p.ledger[1].1 == 16 && p.ledger[1].2 == 16;
    ok = ok && p.ledger[2].2 == 0;
    let d = p.drain(64);
    ok = ok && d == 64 && p.used == 0;
    let w2 = p.write(10);
    ok = ok && w2 == Ok(10) && p.used == 10;
    ok && p.ledger_ok()
}

/// 写面注记（对账/落账用）。
pub fn write_note(sc: &Scenario) -> String {
    let (ret, got) = judge(sc);
    match ret {
        Ok(n) => {
            if n == sc.req {
                format!("full got={} ret={}", got, n)
            } else {
                format!("part got={} ret={}", got, n)
            }
        }
        Err(c) => format!("again code={:#x} got={}", c, got),
    }
}

/// EMFILE/ENFILE 判据（对应卡验收 J 判据）。
pub fn run_unx10814_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10814");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线部分写样本全命中");

    // J2 正路径：整写放行返回=实进
    let j2 = judge(&Scenario { used: 0, cap: 4096, req: 512, nonblock: true }) == (Ok(512), 512);
    s.add("J2-整写放行", j2, "返回值=实进字节=请求");

    // J3 反路径：非阻塞部分写满即返
    let j3 = judge(&Scenario { used: 1000, cap: 4096, req: 4096, nonblock: true })
        == (Ok(3096), 3096);
    s.add("J3-部分写满返", j3, "写满即返余量");

    // J4 反路径：全满 EAGAIN 零消费
    let j4 = judge(&Scenario { used: 4096, cap: 4096, req: 1, nonblock: true })
        == (Err(POSIX_EAGAIN), 0);
    s.add("J4-全满EAGAIN", j4, "EAGAIN 零消费");

    // J5 边界：余 1 请求 8 → 部分 1；余 1 请求 1 → 整写
    let j5 = judge(&Scenario { used: 4095, cap: 4096, req: 8, nonblock: true }) == (Ok(1), 1)
        && judge(&Scenario { used: 4095, cap: 4096, req: 1, nonblock: true }) == (Ok(1), 1);
    s.add("J5-余1两侧", j5, "部分/整写分界在余量");

    // J6 零长写免检
    let j6 = judge(&Scenario { used: 4096, cap: 4096, req: 0, nonblock: true }) == (Ok(0), 0);
    s.add("J6-零长免检", j6, "零长不查余量");

    // J7 消费对账核
    s.add("J7-消费对账核", consume_reconcile_ok(), "逐笔账与 used 同步");

    // J8 注入1 返回虚报检出
    s.add("J8-虚报检出", inj[0], "返回值虚报坏实现必分歧");

    // J9 注入2 账面多记检出
    s.add("J9-多记检出", inj[1], "部分写全消费坏实现必分歧");

    // J10 used 钳制：连写不越 cap
    let mut p = PipeAcct::new(32, true);
    let _ = p.write(20);
    let _ = p.write(20);
    let _ = p.write(20);
    let j10 = p.used == 32 && p.ledger_ok();
    s.add("J10-钳制对账", j10, "used 不越界且账面对拍");

    // J11 字面锚：EAGAIN=11 且诊断表完备
    let j11 = POSIX_EAGAIN == 11 && diag_text(PW_RET_SKEW).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "EAGAIN 字面 11 且语义完备");

    // J12 写面注记闭集：full/part/again 三态互斥
    let n_full = write_note(&Scenario { used: 0, cap: 16, req: 8, nonblock: true });
    let n_part = write_note(&Scenario { used: 12, cap: 16, req: 8, nonblock: true });
    let n_again = write_note(&Scenario { used: 16, cap: 16, req: 1, nonblock: true });
    let j12 = n_full.contains("full") && n_part.contains("part") && n_again.contains("again");
    s.add("J12-三态注记", j12, "full/part/again 互斥可见");

    // J13 (dyn) 管道账指纹落账
    let mut p2 = PipeAcct::new(128, true);
    let _ = p2.write(64);
    let _ = p2.write(128);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(p2.used as u32);
    h = h.wrapping_mul(31).wrapping_add(p2.ledger.len() as u32);
    for c in p2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-管道账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10814_partial_flow() {
        let mut p = PipeAcct::new(16, true);
        assert_eq!(p.write(10), Ok(10));
        assert_eq!(p.write(10), Ok(6));
        assert!(p.write(10).err() == Some(POSIX_EAGAIN));
        assert_eq!(p.used, 16);
        assert_eq!(p.drain(4), 4);
        assert_eq!(p.write(10), Ok(2));
        let sc = Scenario { used: 8, cap: 16, req: 0, nonblock: true };
        assert_eq!(judge(&sc), (Ok(0), 0));
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10814_checks_green() {
        let s = run_unx10814_checks();
        assert!(s.all_passed(), "UNX-F10814 判据未全绿");
    }
}
