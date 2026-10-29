//! UNX-F10808 FIFO ENXIO 正反双判据（O_WRONLY 无读端）（0x1bd80 段）
//!
//! 锚点：UNX-F10808-J1 ENXIO 双判据对平——O_WRONLY|O_NONBLOCK 在读端计数
//! 归零时必 ENXIO 且不占写端位、计数存续必即开、O_RDONLY 任何非阻塞态皆
//! 不出 ENXIO 三线全对平；注入坏实现（读端误出 ENXIO/写端漏出 ENXIO）必
//! 与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：读端计数状态机（open/close 事件驱动，ENXIO 尝试不占位）+ open
//! 尝试判定器（访问型×非阻塞位×读端计数算 Opened/Wait/ENXIO）+ 尝试账
//! （逐次落判重放对拍）+ 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：ENXIO=6。
pub const POSIX_ENXIO: u32 = 6;

/// 诊断段（0x1bd80 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bd80;
pub const FX_RDONLY_ENXIO: u32 = SEG_BASE + 0x01; // 只读口误出 ENXIO
pub const FX_WRONLY_MISS: u32 = SEG_BASE + 0x02; // 写端无读端漏出 ENXIO
pub const FX_GHOST_OCCUPY: u32 = SEG_BASE + 0x03; // ENXIO 尝试误占写端位
pub const FX_BLOCK_ENXIO: u32 = SEG_BASE + 0x04; // 阻塞写端误出 ENXIO
pub const FX_CNT_LOSS: u32 = SEG_BASE + 0x05; // 读端计数丢失
pub const FX_LOG_SKEW: u32 = SEG_BASE + 0x06; // 尝试账错账

/// 访问型。
#[derive(Clone, PartialEq, Eq)]
pub enum Access {
    RdOnly,
    WrOnly,
}

/// open 尝试裁决。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OpenOut {
    Opened,
    Wait,
}

/// 场景输入：读端计数 × 访问型 × 非阻塞位。
#[derive(Clone)]
pub struct Scenario {
    pub readers: u32,
    pub access: Access,
    pub nonblock: bool,
}

/// open 尝试判定核。
pub fn judge(sc: &Scenario) -> Result<OpenOut, u32> {
    match sc.access {
        Access::RdOnly => {
            // 只读口任何组合都不出 ENXIO：非阻塞即开，阻塞视写端注册等待
            if sc.nonblock {
                Ok(OpenOut::Opened)
            } else {
                Ok(OpenOut::Wait)
            }
        }
        Access::WrOnly => {
            if sc.readers > 0 {
                return Ok(OpenOut::Opened);
            }
            if sc.nonblock {
                Err(FX_WRONLY_MISS)
            } else {
                Ok(OpenOut::Wait)
            }
        }
    }
}

/// 读端计数状态机 + 尝试账：逐次 open 尝试落判，ENXIO 不占位。
pub struct FifoTry {
    pub readers: u32,
    pub writers: u32,
    /// 尝试账：(尝试时刻读端计数, 场景, 裁决)。
    pub log: Vec<(u32, Scenario, Result<OpenOut, u32>)>,
    pub note: String,
}

impl FifoTry {
    pub fn new() -> FifoTry {
        FifoTry { readers: 0, writers: 0, log: Vec::new(), note: "idle".to_string() }
    }

    pub fn reader_open(&mut self) {
        self.readers += 1;
        self.note = "reader-in".to_string();
    }

    pub fn reader_close(&mut self) {
        if self.readers > 0 {
            self.readers -= 1;
        }
        self.note = "reader-out".to_string();
    }

    /// 一次 open 尝试：落判成功才占位，ENXIO/Wait 不占位。
    pub fn try_open(&mut self, sc: Scenario) -> Result<OpenOut, u32> {
        let before = self.readers;
        let out = judge(&sc);
        if let Ok(OpenOut::Opened) = out {
            match sc.access {
                Access::RdOnly => self.readers += 1,
                Access::WrOnly => self.writers += 1,
            }
        }
        self.log.push((before, sc, out.clone()));
        out
    }

    /// 尝试账重放对拍：每次尝试按落判时刻读端计数与权威同判。
    pub fn replay_matches(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.log.len() {
            let (before, sc, out) = &self.log[i];
            let probe = Scenario { readers: *before, access: sc.access.clone(), nonblock: sc.nonblock };
            if judge(&probe) != *out {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// ENXIO 命中数。
    pub fn enxio_hits(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.log.len() {
            if self.log[i].2 == Err(FX_WRONLY_MISS) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 () 或期望诊断码。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：读端存续写端即开（计数 1/2/4 档）
    m.push((Scenario { readers: 1, access: Access::WrOnly, nonblock: true }, None));
    m.push((Scenario { readers: 2, access: Access::WrOnly, nonblock: true }, None));
    m.push((Scenario { readers: 4, access: Access::WrOnly, nonblock: false }, None));
    // 正：只读口不出 ENXIO（非阻塞即开 / 阻塞 Wait）
    m.push((Scenario { readers: 0, access: Access::RdOnly, nonblock: true }, None));
    m.push((Scenario { readers: 0, access: Access::RdOnly, nonblock: false }, None));
    // 正：阻塞写端无读端 Wait（不是 ENXIO）
    m.push((Scenario { readers: 0, access: Access::WrOnly, nonblock: false }, None));
    // 反：读端归零 + 非阻塞写端必 ENXIO
    m.push((Scenario { readers: 0, access: Access::WrOnly, nonblock: true }, Some(FX_WRONLY_MISS)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
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

/// 坏实现 A：只读口误出 ENXIO（读端空时拒绝只读非阻塞）。
pub fn faulty_rd_enxio(sc: &Scenario) -> Result<OpenOut, u32> {
    match sc.access {
        Access::RdOnly => {
            if sc.readers == 0 && sc.nonblock {
                Err(FX_RDONLY_ENXIO)
            } else {
                judge(sc)
            }
        }
        _ => judge(sc),
    }
}

/// 坏实现 B：写端漏出 ENXIO（读端空也放行写端）。
pub fn faulty_wr_open(sc: &Scenario) -> Result<OpenOut, u32> {
    match sc.access {
        Access::WrOnly => Ok(OpenOut::Opened),
        _ => judge(sc),
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 只读误出 ENXIO：权威 Opened、坏实现拒 → 检出
    let sc1 = Scenario { readers: 0, access: Access::RdOnly, nonblock: true };
    got[0] = judge(&sc1) != faulty_rd_enxio(&sc1) && judge(&sc1) == Ok(OpenOut::Opened);
    // 2) 写端漏 ENXIO：权威拒、坏实现放行 → 检出
    let sc2 = Scenario { readers: 0, access: Access::WrOnly, nonblock: true };
    got[1] = judge(&sc2) != faulty_wr_open(&sc2) && judge(&sc2).err() == Some(FX_WRONLY_MISS);
    got
}

/// 尝试账全序核：reader-in → writer-ok → reader-out ×2 → writer-enxio → writer-wait。
pub fn try_seq_ok() -> bool {
    let mut f = FifoTry::new();
    let _ = f.try_open(Scenario { readers: 0, access: Access::RdOnly, nonblock: true });
    let w1 = f.try_open(Scenario { readers: 1, access: Access::WrOnly, nonblock: true });
    f.reader_close();
    f.reader_close();
    let w2 = f.try_open(Scenario { readers: 0, access: Access::WrOnly, nonblock: true });
    let w3 = f.try_open(Scenario { readers: 0, access: Access::WrOnly, nonblock: false });
    w1 == Ok(OpenOut::Opened)
        && w2.err() == Some(FX_WRONLY_MISS)
        && w3 == Ok(OpenOut::Wait)
        && f.writers == 1
        && f.readers == 0
        && f.replay_matches()
}

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        FX_RDONLY_ENXIO => "只读口误出 ENXIO",
        FX_WRONLY_MISS => "写端无读端（O_NONBLOCK）",
        FX_GHOST_OCCUPY => "ENXIO 尝试误占写端位",
        FX_BLOCK_ENXIO => "阻塞写端误出 ENXIO",
        FX_CNT_LOSS => "读端计数丢失",
        FX_LOG_SKEW => "尝试账错账",
        _ => "未知诊断码",
    }
}

/// 等待面收账核：阻塞写端 Wait 后读端进场即升格 Opened 且占位。
pub fn wait_settle_ok() -> bool {
    let mut f = FifoTry::new();
    // 写端阻塞等待（读端空）
    let w = f.try_open(Scenario { readers: 0, access: Access::WrOnly, nonblock: false });
    let waiting = w == Ok(OpenOut::Wait) && f.writers == 0;
    // 读端进场 → 计数 1，写端重试即开
    f.reader_open();
    let w2 = f.try_open(Scenario { readers: 1, access: Access::WrOnly, nonblock: false });
    let settled = w2 == Ok(OpenOut::Opened) && f.writers == 1 && f.readers == 1;
    // 收账后读端退场不影响已占写端位
    f.reader_close();
    let tail = f.writers == 1 && f.readers == 0 && f.replay_matches();
    waiting && settled && tail
}

/// FIFO ENXIO 判据（对应卡验收 J 判据）。
pub fn run_unx10808_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10808");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线正反样本全命中");

    // J2 正路径：读端存续非阻塞写端即开
    let j2 = judge(&Scenario { readers: 2, access: Access::WrOnly, nonblock: true })
        == Ok(OpenOut::Opened);
    s.add("J2-读端存续即开", j2, "读端计数>0 写端 Opened");

    // J3 反路径：读端归零非阻塞写端必 ENXIO
    let j3 = judge(&Scenario { readers: 0, access: Access::WrOnly, nonblock: true })
        .err() == Some(FX_WRONLY_MISS);
    s.add("J3-归零必ENXIO", j3, "O_WRONLY|O_NONBLOCK 无读端 ENXIO");

    // J4 只读口不出 ENXIO
    let j4 = judge(&Scenario { readers: 0, access: Access::RdOnly, nonblock: true })
        == Ok(OpenOut::Opened);
    s.add("J4-只读无ENXIO", j4, "O_RDONLY|O_NONBLOCK 即开");

    // J5 阻塞写端无读端 Wait 而非 ENXIO，且读端进场即收账升格
    let j5 = judge(&Scenario { readers: 0, access: Access::WrOnly, nonblock: false })
        == Ok(OpenOut::Wait)
        && wait_settle_ok();
    s.add("J5-阻塞面Wait", j5, "阻塞写端互等不误报 ENXIO");

    // J6 ENXIO 尝试不占位：拒绝后写端计数不变
    let mut f = FifoTry::new();
    f.reader_open();
    f.reader_close();
    let _ = f.try_open(Scenario { readers: 0, access: Access::WrOnly, nonblock: true });
    let j6 = f.writers == 0 && f.readers == 0;
    s.add("J6-拒绝不占位", j6, "ENXIO 尝试零占位");

    // J7 尝试账全序核
    s.add("J7-尝试账全序", try_seq_ok(), "五步全序落判重放对拍");

    // J8 注入1 只读误出 ENXIO 检出
    s.add("J8-只读误拒检出", inj[0], "只读口 ENXIO 坏实现必分歧");

    // J9 注入2 写端漏 ENXIO 检出
    s.add("J9-写端漏拒检出", inj[1], "幽灵放行坏实现必分歧");

    // J10 读端计数下限钳零
    let mut f2 = FifoTry::new();
    f2.reader_open();
    f2.reader_close();
    f2.reader_close();
    let j10 = f2.readers == 0;
    s.add("J10-计数钳零", j10, "close 超发不回绕");

    // J11 字面锚：ENXIO=6 且诊断语义完备
    let j11 = POSIX_ENXIO == 6
        && FX_WRONLY_MISS == SEG_BASE + 0x02
        && diag_text(FX_WRONLY_MISS).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "ENXIO 字面 6 且码位对平");

    // J12 重放对拍：全序账与权威逐条同判
    let mut f3 = FifoTry::new();
    let _ = f3.try_open(Scenario { readers: 0, access: Access::RdOnly, nonblock: false });
    let _ = f3.try_open(Scenario { readers: 1, access: Access::WrOnly, nonblock: false });
    let _ = f3.try_open(Scenario { readers: 1, access: Access::WrOnly, nonblock: true });
    let j12 = f3.replay_matches() && f3.log.len() == 3 && f3.enxio_hits() == 0;
    s.add("J12-重放对拍", j12, "三步账逐条同判零 ENXIO");

    // J13 (dyn) 尝试账指纹落账
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(f3.log.len() as u32);
    let mut i = 0usize;
    while i < f3.log.len() {
        h = h.wrapping_mul(31).wrapping_add(f3.log[i].0);
        i += 1;
    }
    s.add_dyn("J13-账面指纹", h != 0, &format!("fold={:#x} rows={}", h, f3.log.len()));

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
    fn unx10808_enxio_flow() {
        let mut f = FifoTry::new();
        assert!(f.try_open(Scenario { readers: 0, access: Access::WrOnly, nonblock: true })
            .err() == Some(FX_WRONLY_MISS));
        f.reader_open();
        assert_eq!(
            f.try_open(Scenario { readers: 1, access: Access::WrOnly, nonblock: true }),
            Ok(OpenOut::Opened)
        );
        assert_eq!(f.writers, 1);
        assert!(f.replay_matches());
        let sc = Scenario { readers: 0, access: Access::RdOnly, nonblock: false };
        assert_eq!(judge(&sc), Ok(OpenOut::Wait));
        assert!(inject_two().iter().all(|b| *b));
    }

    #[test]
    fn unx10808_checks_green() {
        let s = run_unx10808_checks();
        assert!(s.all_passed(), "UNX-F10808 判据未全绿");
    }
}
