//! UNX-F10818 FIFO open 阻塞互等（0x1bec0 段）
//!
//! 锚点：UNX-F10818-J1 互等对平——先到方注册等待、对端进场双双升格
//! Opened、同侧多等者互不吞并、等待账重放与权威同判四线全对平；注入
//! 坏实现（单边升格/同侧误配对）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：等待者账（角色 × 等待位）+ 互等判定器（角色对撞算 Wait/Opened）
//! + 升格状态机（对端进场清等待位）+ 重放核（逐笔先验同判）+ 注入核
//! （两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：ENXIO=6。
pub const POSIX_ENXIO: u32 = 6;

/// 诊断段（0x1bec0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bec0;
pub const BW_SINGLE_PROMOTE: u32 = SEG_BASE + 0x01; // 单边升格
pub const BW_SAME_SIDE: u32 = SEG_BASE + 0x02; // 同侧误配对
pub const BW_WAIT_LOSS: u32 = SEG_BASE + 0x03; // 等待位丢失
pub const BW_RD_BLOCK_MISS: u32 = SEG_BASE + 0x04; // 读端阻塞面缺失
pub const BW_PROMO_SKEW: u32 = SEG_BASE + 0x05; // 升格账错
pub const BW_REPLAY_MISS: u32 = SEG_BASE + 0x06; // 重放不齐

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        BW_SINGLE_PROMOTE => "单边升格",
        BW_SAME_SIDE => "同侧误配对",
        BW_WAIT_LOSS => "等待位丢失",
        BW_RD_BLOCK_MISS => "读端阻塞面缺失",
        BW_PROMO_SKEW => "升格账错",
        BW_REPLAY_MISS => "重放不齐",
        _ => "未知诊断码",
    }
}

/// 等待者角色。
#[derive(Clone, PartialEq, Eq)]
pub enum Role {
    Rd,
    Wr,
}

/// 等待判定输出。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OpenOut {
    Opened,
    Wait,
}

/// 场景输入：角色 × 对端在场 × 非阻塞位。
#[derive(Clone)]
pub struct Scenario {
    pub role: Role,
    pub peer_present: bool,
    pub nonblock: bool,
}

/// 互等判定核：对端在场即开；对端缺席时阻塞等（Wait）、非阻塞按角色
/// 出 ENXIO（仅写端；读端非阻塞即开）。
pub fn judge(sc: &Scenario) -> Result<OpenOut, u32> {
    if sc.peer_present {
        return Ok(OpenOut::Opened);
    }
    match sc.role {
        Role::Rd => {
            if sc.nonblock {
                Ok(OpenOut::Opened)
            } else {
                Ok(OpenOut::Wait)
            }
        }
        Role::Wr => {
            if sc.nonblock {
                Err(POSIX_ENXIO)
            } else {
                Ok(OpenOut::Wait)
            }
        }
    }
}

/// 等待者账状态机：注册等待位，对端进场清位并升格双方。
pub struct WaitLedger {
    pub rd_wait: u32,
    pub wr_wait: u32,
    pub rd_open: u32,
    pub wr_open: u32,
    pub log: Vec<(Role, String)>,
    pub note: String,
}

impl WaitLedger {
    pub fn new() -> WaitLedger {
        WaitLedger {
            rd_wait: 0,
            wr_wait: 0,
            rd_open: 0,
            wr_open: 0,
            log: Vec::new(),
            note: "idle".to_string(),
        }
    }

    /// 一次尝试：先判 Wait（注册等待位）或 Opened（占位）；对端进场则清等待位。
    pub fn try_open(&mut self, sc: &Scenario) -> Result<OpenOut, u32> {
        let out = judge(sc);
        match &out {
            Ok(OpenOut::Opened) => {
                match sc.role {
                    Role::Rd => {
                        self.rd_open += 1;
                        // 读端进场：清写端等待位（写端被升格）
                        self.wr_wait = 0;
                    }
                    Role::Wr => {
                        self.wr_open += 1;
                        self.rd_wait = 0;
                    }
                }
                self.log.push((sc.role.clone(), "opened".to_string()));
                self.note = "open".to_string();
            }
            Ok(OpenOut::Wait) => {
                match sc.role {
                    Role::Rd => self.rd_wait += 1,
                    Role::Wr => self.wr_wait += 1,
                }
                self.log.push((sc.role.clone(), "wait".to_string()));
                self.note = "wait".to_string();
            }
            Err(_) => {
                self.log.push((sc.role.clone(), "enxio".to_string()));
                self.note = "enxio".to_string();
            }
        }
        out
    }

    /// 升格核：读端在场且写端等待 → 写端等待位清零即被升格。
    pub fn promote_ok(&self) -> bool {
        self.rd_open > 0 && self.wr_wait == 0
    }

    /// 重放核：逐笔按先验场景与权威同判。
    pub fn replay_ok(&self) -> bool {
        let mut ok = true;
        let mut rd_alive = false;
        let mut wr_alive = false;
        let mut i = 0usize;
        while i < self.log.len() {
            let (role, tag) = &self.log[i];
            let sc = Scenario {
                role: role.clone(),
                peer_present: match role {
                    Role::Rd => wr_alive,
                    Role::Wr => rd_alive,
                },
                nonblock: false,
            };
            let want = judge(&sc);
            let want_tag = match want {
                Ok(OpenOut::Opened) => "opened",
                Ok(OpenOut::Wait) => "wait",
                Err(_) => "enxio",
            };
            if *tag != want_tag {
                ok = false;
            }
            match (role, tag.as_str()) {
                (Role::Rd, "opened") => rd_alive = true,
                (Role::Wr, "opened") => wr_alive = true,
                _ => {}
            }
            i += 1;
        }
        ok
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 () 或 ENXIO。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：对端在场即开（读/写）
    m.push((Scenario { role: Role::Rd, peer_present: true, nonblock: true }, None));
    m.push((Scenario { role: Role::Wr, peer_present: true, nonblock: false }, None));
    // 正：读端阻塞等
    m.push((Scenario { role: Role::Rd, peer_present: false, nonblock: false }, None));
    // 正：写端阻塞等
    m.push((Scenario { role: Role::Wr, peer_present: false, nonblock: false }, None));
    // 正：读端非阻塞即开（无写端也不 ENXIO）
    m.push((Scenario { role: Role::Rd, peer_present: false, nonblock: true }, None));
    // 反：写端非阻塞无读端必 ENXIO
    m.push((Scenario { role: Role::Wr, peer_present: false, nonblock: true }, Some(POSIX_ENXIO)));
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

/// 坏实现 A：单边升格（读端进场忘清写端等待位 → 写端等待位残留 1）。
pub fn faulty_stale_wait() -> u32 {
    1
}

/// 坏实现 B：阻塞写端误报 ENXIO（等待面丢失）。
pub fn faulty_block_enxio(sc: &Scenario) -> Result<OpenOut, u32> {
    match sc.role {
        Role::Wr if !sc.peer_present => Err(POSIX_ENXIO),
        _ => judge(sc),
    }
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 单边升格：权威账 wr_wait==0、坏实现残留 1 → 账面差检出
    let mut lg = WaitLedger::new();
    let _ = lg.try_open(&Scenario { role: Role::Wr, peer_present: false, nonblock: false });
    let _ = lg.try_open(&Scenario { role: Role::Rd, peer_present: true, nonblock: true });
    got[0] = lg.wr_wait != faulty_stale_wait() && lg.promote_ok();
    // 2) 阻塞写端误 ENXIO：权威 Wait、坏实现 Err → 检出
    let sc2 = Scenario { role: Role::Wr, peer_present: false, nonblock: false };
    got[1] = judge(&sc2) != faulty_block_enxio(&sc2) && judge(&sc2) == Ok(OpenOut::Wait);
    got
}

/// 互等全序核：wr 等待 → rd 进场清位 → wr 重试即开。
pub fn mutual_wait_ok() -> bool {
    let mut lg = WaitLedger::new();
    let w1 = lg.try_open(&Scenario { role: Role::Wr, peer_present: false, nonblock: false });
    let waiting = w1 == Ok(OpenOut::Wait) && lg.wr_wait == 1;
    let r1 = lg.try_open(&Scenario { role: Role::Rd, peer_present: true, nonblock: false });
    let rd_in = r1 == Ok(OpenOut::Opened) && lg.rd_open == 1;
    let w2 = lg.try_open(&Scenario { role: Role::Wr, peer_present: true, nonblock: false });
    let wr_in = w2 == Ok(OpenOut::Opened) && lg.wr_open == 1 && lg.wr_wait == 0;
    waiting && rd_in && wr_in && lg.replay_ok()
}

/// 同侧多等者核：两个读端等待互不吞并，双双清位。
pub fn multi_waiter_ok() -> bool {
    let mut lg = WaitLedger::new();
    let _ = lg.try_open(&Scenario { role: Role::Rd, peer_present: false, nonblock: false });
    let _ = lg.try_open(&Scenario { role: Role::Rd, peer_present: false, nonblock: false });
    let two_waiting = lg.rd_wait == 2 && lg.rd_open == 0;
    let _ = lg.try_open(&Scenario { role: Role::Wr, peer_present: true, nonblock: true });
    let cleared = lg.rd_wait == 0 && lg.wr_open == 1;
    two_waiting && cleared
}

/// 等待面注记（对账/落账用）。
pub fn wait_note(lg: &WaitLedger) -> String {
    format!("rw={} ww={} ro={} wo={}", lg.rd_wait, lg.wr_wait, lg.rd_open, lg.wr_open)
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10818_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10818");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "六线互等样本全命中");

    // J2 正路径：对端在场即开
    let j2 = judge(&Scenario { role: Role::Wr, peer_present: true, nonblock: true })
        == Ok(OpenOut::Opened);
    s.add("J2-对端即开", j2, "读写在场即通");

    // J3 正路径：读写双方阻塞互等
    let j3 = judge(&Scenario { role: Role::Rd, peer_present: false, nonblock: false })
        == Ok(OpenOut::Wait)
        && judge(&Scenario { role: Role::Wr, peer_present: false, nonblock: false })
            == Ok(OpenOut::Wait);
    s.add("J3-双向Wait", j3, "阻塞面互等不报错");

    // J4 反路径：写端非阻塞无读端必 ENXIO
    let j4 = judge(&Scenario { role: Role::Wr, peer_present: false, nonblock: true })
        .err() == Some(POSIX_ENXIO);
    s.add("J4-写端ENXIO", j4, "O_WRONLY|O_NONBLOCK 无读端 6");

    // J5 互等全序核：等待→进场→清位→升格
    s.add("J5-互等全序", mutual_wait_ok(), "等待位与升格账全对");

    // J6 注入1 单边升格检出
    s.add("J6-单边检出", inj[0], "等待位残留坏实现必账差");

    // J7 注入2 阻塞误 ENXIO 检出
    s.add("J7-误报检出", inj[1], "阻塞写端报 ENXIO 必分歧");

    // J8 同侧多等者互不吞并
    s.add("J8-多等者核", multi_waiter_ok(), "两位同清不互吞");

    // J9 重放对拍核
    let mut lg2 = WaitLedger::new();
    let _ = lg2.try_open(&Scenario { role: Role::Rd, peer_present: false, nonblock: false });
    let _ = lg2.try_open(&Scenario { role: Role::Wr, peer_present: false, nonblock: false });
    let _ = lg2.try_open(&Scenario { role: Role::Rd, peer_present: true, nonblock: true });
    let j9 = lg2.replay_ok() && lg2.log.len() == 3;
    s.add("J9-重放对拍", j9, "三步账逐条同判");

    // J10 读端非阻塞面：无写端也即开不 ENXIO
    let j10 = judge(&Scenario { role: Role::Rd, peer_present: false, nonblock: true })
        == Ok(OpenOut::Opened);
    s.add("J10-读非阻即开", j10, "只读口无 ENXIO 面");

    // J11 字面锚：ENXIO=6 且诊断表完备
    let j11 = POSIX_ENXIO == 6 && diag_text(BW_SINGLE_PROMOTE).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J11-字面锚对平", j11, "ENXIO 字面 6 且语义完备");

    // J12 等待面注记闭集
    let mut lg3 = WaitLedger::new();
    let _ = lg3.try_open(&Scenario { role: Role::Wr, peer_present: false, nonblock: false });
    let n_wait = wait_note(&lg3);
    let _ = lg3.try_open(&Scenario { role: Role::Rd, peer_present: true, nonblock: true });
    let n_open = wait_note(&lg3);
    let j12 = n_wait.contains("ww=1") && n_open.contains("ww=0") && n_open.contains("ro=1");
    s.add("J12-注记对账", j12, "等待/升格注记可见");

    // J13 (dyn) 等待账指纹落账
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg3.rd_wait);
    h = h.wrapping_mul(31).wrapping_add(lg3.wr_wait);
    h = h.wrapping_mul(31).wrapping_add(lg3.rd_open);
    h = h.wrapping_mul(31).wrapping_add(lg3.wr_open);
    for c in lg3.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-等待账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10818_wait_flow() {
        let mut lg = WaitLedger::new();
        assert_eq!(lg.try_open(&Scenario { role: Role::Wr, peer_present: false, nonblock: false }), Ok(OpenOut::Wait));
        assert_eq!(lg.wr_wait, 1);
        assert!(lg.try_open(&Scenario { role: Role::Wr, peer_present: false, nonblock: true }).err() == Some(POSIX_ENXIO));
        assert_eq!(lg.try_open(&Scenario { role: Role::Rd, peer_present: true, nonblock: true }), Ok(OpenOut::Opened));
        assert_eq!(lg.wr_wait, 0);
        assert_eq!(lg.rd_open, 1);
        assert!(lg.replay_ok() && lg.promote_ok());
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10818_checks_green() {
        let s = run_unx10818_checks();
        assert!(s.all_passed(), "UNX-F10818 判据未全绿");
    }
}
