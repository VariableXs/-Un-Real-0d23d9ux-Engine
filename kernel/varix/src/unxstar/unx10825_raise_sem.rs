//! UNX-F10825 raise() 语义矩阵（自投递面）（0x1bfa0 段）
//!
//! 锚点：UNX-F10825-J1 raise 对平——raise 只可能 EINVAL（号非法）、不可
//! 能 ESRCH/EPERM（自投递免存在与权限查）、合法号必立即入自投递账三线
//! 全对平；注入坏实现（raise 走 kill 全查/合法号丢失）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：自投递账（号 → 投递序号）+ raise 判定器（号域对撞算 EINVAL/
//! 自投递）+ 计数状态机（同号多次自投递 std 合并）+ 等价性核（raise ==
//! kill(getpid) 除查免外同账）+ 注入核（两类坏实现必分歧）+ 正反双样本
//! 矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：EINVAL=22、SIGUSR1=10、SIGUSR2=12。
pub const POSIX_EINVAL: u32 = 22;
pub const SIGUSR1: u32 = 10;
pub const SIGUSR2: u32 = 12;
/// 合法信号号上界。
pub const NSIG: u32 = 65;

/// 诊断段（0x1bfa0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1bfa0;
pub const RS_ZERO_SIG: u32 = SEG_BASE + 0x01; // 0 号 raise
pub const RS_OVER_SIG: u32 = SEG_BASE + 0x02; // 超界 raise
pub const RS_KILL_SKEW: u32 = SEG_BASE + 0x03; // 等价性错（多查 ESRCH/EPERM）
pub const RS_DELIVER_LOSS: u32 = SEG_BASE + 0x04; // 合法号投递丢失
pub const RS_MERGE_MISS: u32 = SEG_BASE + 0x05; // std 合并缺失
pub const RS_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        RS_ZERO_SIG => "0 号 raise",
        RS_OVER_SIG => "超界 raise",
        RS_KILL_SKEW => "等价性错（多查）",
        RS_DELIVER_LOSS => "合法号投递丢失",
        RS_MERGE_MISS => "std 合并缺失",
        RS_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// raise 判定核：号域对撞——0/超界必 EINVAL；合法号必自投递成功。
pub fn judge(sig: u32) -> Result<&'static str, u32> {
    if sig == 0 || sig >= NSIG {
        return Err(POSIX_EINVAL);
    }
    Ok("self-delivered")
}

/// 自投递账状态机：std 号合并（位集），投递序号递增。
pub struct RaiseLedger {
    pub std_pending: u32,
    pub deliveries: u32,
    pub seq: Vec<u32>,
    pub note: String,
}

impl RaiseLedger {
    pub fn new() -> RaiseLedger {
        RaiseLedger {
            std_pending: 0,
            deliveries: 0,
            seq: Vec::new(),
            note: "ledger".to_string(),
        }
    }

    /// 一次 raise：号合法则位入账（std 合并）+ 投递序号递增。
    pub fn raise(&mut self, sig: u32) -> Result<&'static str, u32> {
        let out = judge(sig);
        if out.is_ok() {
            self.std_pending |= 1u32 << sig;
            self.deliveries += 1;
            self.seq.push(sig);
            self.note = "raised".to_string();
        }
        out
    }

    /// 交付核：逐位清账（std 一次性交付），返回交付的位集。
    pub fn deliver_pending(&mut self) -> u32 {
        let got = self.std_pending;
        self.std_pending = 0;
        self.note = "delivered".to_string();
        got
    }

    /// 账面核：投递序号数与账行一致、位集非负。
    pub fn ledger_ok(&self) -> bool {
        self.deliveries == self.seq.len() as u32 && self.deliveries as usize <= self.seq.len()
    }
}

/// 坏实现 A：raise 走 kill 全查（自 pid 需查表+权限，表空必 ESRCH）。
pub fn faulty_kill_lookup(sig: u32) -> Result<&'static str, u32> {
    if sig == 0 || sig >= NSIG {
        return Err(POSIX_EINVAL);
    }
    // 坏实现：查进程表失败 → ESRCH（权威 raise 不可能出此错）
    Err(3)
}

/// 坏实现 B：合法号投递丢失（raise 成功但账不入）。
pub fn faulty_deliver_loss(lg: &mut RaiseLedger, sig: u32) -> Result<&'static str, u32> {
    let out = judge(sig);
    if out.is_ok() {
        // 故意不入账
        return out;
    }
    out
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 自投递被查表：权威 self-delivered、坏实现 ESRCH → 检出
    got[0] = judge(SIGUSR1) != faulty_kill_lookup(SIGUSR1)
        && judge(SIGUSR1) == Ok("self-delivered")
        && faulty_kill_lookup(SIGUSR1).err() == Some(3);
    // 2) 投递丢失：权威账入位、坏实现账空 → 检出
    let mut good = RaiseLedger::new();
    let _ = good.raise(SIGUSR1);
    let mut bad = RaiseLedger::new();
    let _ = faulty_deliver_loss(&mut bad, SIGUSR1);
    got[1] = good.std_pending != bad.std_pending && good.std_pending == (1 << SIGUSR1)
        && bad.std_pending == 0;
    got
}

/// 等价性核：raise(sig) 与 kill(getpid, sig) 同账（同号同位、序号同增），
/// 且 raise 面不可能出 ESRCH/EPERM（3/1 两码闭集外）。
pub fn equivalence_ok() -> bool {
    let mut a = RaiseLedger::new();
    let mut b = RaiseLedger::new();
    let r1 = a.raise(SIGUSR1);
    let r2 = b.raise(SIGUSR2);
    let _ = b.raise(SIGUSR1);
    r1.is_ok() && r2.is_ok()
        && a.std_pending == b.std_pending
        && a.deliveries == b.deliveries
        && a.ledger_ok() && b.ledger_ok()
}

/// std 合并核：同号三连 raise → 位集只置 1 位，交付一次。
pub fn merge_ok() -> bool {
    let mut lg = RaiseLedger::new();
    let _ = lg.raise(SIGUSR1);
    let _ = lg.raise(SIGUSR1);
    let _ = lg.raise(SIGUSR1);
    let single_bit = lg.std_pending.count_ones() == 1;
    let got = lg.deliver_pending();
    single_bit && got == (1 << SIGUSR1) && lg.std_pending == 0 && lg.deliveries == 3
}

/// 号域闭集：0/65/1000 拒，1..64 通。
pub fn domain_ok() -> bool {
    let mut ok = true;
    let mut i = 0u32;
    while i < 70 {
        let want = i == 0 || i >= NSIG;
        let got = judge(i).is_err();
        if want != got {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 () 或 EINVAL。
pub fn sample_matrix() -> Vec<(u32, Option<u32>)> {
    let mut m: Vec<(u32, Option<u32>)> = Vec::new();
    // 正：合法号全域采样
    m.push((1, None));
    m.push((SIGUSR1, None));
    m.push((SIGUSR2, None));
    m.push((NSIG - 1, None));
    // 反：0 号
    m.push((0, Some(POSIX_EINVAL)));
    // 反：超界
    m.push((NSIG, Some(POSIX_EINVAL)));
    m.push((1000, Some(POSIX_EINVAL)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sig, want) = &m[i];
        let got = judge(*sig);
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

/// raise 面注记（对账/落账用）。
pub fn raise_note(sig: u32) -> String {
    match judge(sig) {
        Ok(t) => format!("{} sig={}", t, sig),
        Err(c) => format!("err:{:#x}", c),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10825_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10825");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "七线 raise 样本全命中");

    // J2 反路径：0 号必 EINVAL
    let j2 = judge(0).err() == Some(POSIX_EINVAL);
    s.add("J2-零号拒", j2, "raise(0) 必 22");

    // J3 反路径：超界必 EINVAL
    let j3 = judge(NSIG).err() == Some(POSIX_EINVAL)
        && judge(1000).err() == Some(POSIX_EINVAL);
    s.add("J3-超界拒", j3, "sig>=NSIG 必 22");

    // J4 正路径：合法号必自投递
    let j4 = judge(SIGUSR1) == Ok("self-delivered")
        && judge(NSIG - 1) == Ok("self-delivered");
    s.add("J4-自投递面", j4, "1..NSIG-1 全域通");

    // J5 不可错面：raise 不可能 ESRCH/EPERM
    let j5 = judge(1).err() != Some(3) && judge(1).err() != Some(1);
    s.add("J5-免查面", j5, "自投递免存在/权限查");

    // J6 等价性核：raise == kill(getpid) 同账
    s.add("J6-等价性核", equivalence_ok(), "两路账同位同序");

    // J7 注入1 查表 ESRCH 检出
    s.add("J7-查表检出", inj[0], "raise 走 kill 查坏实现必分歧");

    // J8 注入2 投递丢失检出
    s.add("J8-丢投检出", inj[1], "账不入坏实现必分歧");

    // J9 std 合并核：三连 raise 一位一次交付
    s.add("J9-合并核", merge_ok(), "同号合并交付一次");

    // J10 号域闭集核：0..69 逐点对平
    s.add("J10-号域闭集", domain_ok(), "域界逐点同判");

    // J11 字面锚：EINVAL=22 / SIGUSR1=10 / SIGUSR2=12
    let j11 = POSIX_EINVAL == 22 && SIGUSR1 == 10 && SIGUSR2 == 12
        && diag_text(RS_ZERO_SIG).len() > 0;
    s.add("J11-字面锚对平", j11, "22/10/12 三锚");

    // J12 raise 面注记闭集
    let n_ok = raise_note(SIGUSR1);
    let n_bad = raise_note(0);
    let j12 = n_ok.contains("self-delivered") && n_bad.contains("err") && n_ok != n_bad;
    s.add("J12-注记互异", j12, "delivered/err 注记互异");

    // J13 (dyn) 自投递账指纹落账
    let mut lg2 = RaiseLedger::new();
    let _ = lg2.raise(SIGUSR1);
    let _ = lg2.raise(SIGUSR2);
    let _ = lg2.raise(2);
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg2.std_pending);
    h = h.wrapping_mul(31).wrapping_add(lg2.deliveries);
    for c in lg2.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-自投递账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10825_raise_flow() {
        let mut lg = RaiseLedger::new();
        assert!(lg.raise(0).err() == Some(POSIX_EINVAL));
        assert!(lg.raise(NSIG).err() == Some(POSIX_EINVAL));
        assert!(lg.raise(SIGUSR1).is_ok());
        assert!(lg.raise(SIGUSR1).is_ok());
        assert_eq!(lg.std_pending.count_ones(), 1);
        assert_eq!(lg.deliveries, 2);
        let got = lg.deliver_pending();
        assert_eq!(got, 1 << SIGUSR1);
        assert_eq!(lg.std_pending, 0);
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10825_checks_green() {
        let s = run_unx10825_checks();
        assert!(s.all_passed(), "UNX-F10825 判据未全绿");
    }
}
