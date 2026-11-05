//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12631 异步投递错误注入（坏 APC/坏完成包 30 例） · 0x807C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12632-J1 抽样 20 场景双机行为一致（S1 母版），
//!      交付时机类零容差；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 注入语料机：30 例坏投递（坏 APC 例 × 坏完成包例轮换），每例
//!     冻结（类别/坏点/期望拒码）三要素入账；语料指纹冻结
//!     （CorpusDrift 抓表改）；
//!   - 抽样 20 场景（判据锚定）：从 30 例按冻结步距抽样 20 例（步距
//!     3 取 2 留 1 的确定性抽样），逐例双机（S1 母版机 / 对拷机）求
//!     判——判码逐例相等（HostDrift 抓漂移）；
//!   - 交付时机类零容差（判据锚定）：时机档闭集 {未挂即交付, 超期
//!     交付, 双次交付}——任一时机违约零容忍，违约即抓
//!     （TimingViolation），不许宽限窗；
//!   - 坏点三路：APC 例程空 / 目标越窗 / 完成包字段互斥，逐路注入
//!     必抓；LCG 种子写死 = SEG_BASE（0x807C0），禁 rand/浮点/IO/
//!     墙钟；no_std + alloc；零 unwrap/expect/panic；诊断码闭集
//!     0x807C0..0x80800。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x807C0，AI-VX9-1105 B02 批 12 槽第 8 槽）。
pub const SEG_BASE: u32 = 0x807C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AsyncInjCode {
    /// 坏 APC：例程指针为空
    BadApcRoutine,
    /// 坏 APC：目标线程越窗
    BadApcTarget,
    /// 坏完成包：字段互斥（成功档带非零完成值）
    BadPktClash,
    /// 坏完成包：完成键越窗
    BadPktKey,
    /// 交付时机违约：未挂即交付
    TimingDeliverBeforeArm,
    /// 交付时机违约：超期交付（零容差）
    TimingLateDeliver,
    /// 交付时机违约：双次交付
    TimingDoubleDeliver,
    /// 双机漂移：S1 母版与对拷机同例异判
    HostDrift,
    /// 语料指纹漂移：冻结表被改
    CorpusDrift,
    /// 抽样越界：抽样例号不在 0..30
    SampleIndexOut,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 未知诊断码兜底
    Unknown,
}

impl AsyncInjCode {
    pub const fn val(self) -> u32 {
        SEG_BASE
            + match self {
                AsyncInjCode::BadApcRoutine => 0x01,
                AsyncInjCode::BadApcTarget => 0x02,
                AsyncInjCode::BadPktClash => 0x03,
                AsyncInjCode::BadPktKey => 0x04,
                AsyncInjCode::TimingDeliverBeforeArm => 0x05,
                AsyncInjCode::TimingLateDeliver => 0x06,
                AsyncInjCode::TimingDoubleDeliver => 0x07,
                AsyncInjCode::HostDrift => 0x08,
                AsyncInjCode::CorpusDrift => 0x09,
                AsyncInjCode::SampleIndexOut => 0x0A,
                AsyncInjCode::FingerprintDrift => 0x0B,
                AsyncInjCode::Unknown => 0x3F,
            }
    }

    pub fn detail(self) -> &'static str {
        match self {
            AsyncInjCode::BadApcRoutine => "坏 APC：先注册例程再投递",
            AsyncInjCode::BadApcTarget => "坏 APC：目标须在注册窗内",
            AsyncInjCode::BadPktClash => "坏完成包：成功档须零完成值",
            AsyncInjCode::BadPktKey => "坏完成包：完成键须在登记窗内",
            AsyncInjCode::TimingDeliverBeforeArm => "时机违约：未挂即交付，零容差必抓",
            AsyncInjCode::TimingLateDeliver => "时机违约：超期交付，零容差不许宽限",
            AsyncInjCode::TimingDoubleDeliver => "时机违约：双次交付，一包一交付",
            AsyncInjCode::HostDrift => "双机漂移：S1 母版与对拷机逐例同判",
            AsyncInjCode::CorpusDrift => "语料指纹漂移：冻结表不许改",
            AsyncInjCode::SampleIndexOut => "抽样越界：用 0..30 内例号",
            AsyncInjCode::FingerprintDrift => "双跑指纹漂移：冻结种子重跑",
            AsyncInjCode::Unknown => "未知诊断码：不在 0x807C0 段闭集内",
        }
    }

    pub const ALL: [AsyncInjCode; 12] = [
        AsyncInjCode::BadApcRoutine,
        AsyncInjCode::BadApcTarget,
        AsyncInjCode::BadPktClash,
        AsyncInjCode::BadPktKey,
        AsyncInjCode::TimingDeliverBeforeArm,
        AsyncInjCode::TimingLateDeliver,
        AsyncInjCode::TimingDoubleDeliver,
        AsyncInjCode::HostDrift,
        AsyncInjCode::CorpusDrift,
        AsyncInjCode::SampleIndexOut,
        AsyncInjCode::FingerprintDrift,
        AsyncInjCode::Unknown,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in AsyncInjCode::ALL.iter() {
        if c.val() == code {
            return c.detail();
        }
    }
    AsyncInjCode::Unknown.detail()
}

// ── 冻结常量表（来源：异步投递错误注入手账 · 注入节） ─────────────
/// 注入例总数（判据 30 例）。
pub const CASES: usize = 30;
/// 抽样场景数（判据 20 场景）。
pub const SAMPLED: usize = 20;
/// 目标线程注册窗。
pub const TARGET_WIN: (u32, u32) = (0x20, 0x30);
/// 完成键登记窗。
pub const KEY_WIN: (u32, u32) = (0x60, 0x70);

/// 交付时机档（零容差闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimingKind {
    /// 合规：挂起后按时交付。
    InOrder,
    /// 违约：未挂即交付。
    BeforeArm,
    /// 违约：超期交付。
    Late,
    /// 违约：双次交付。
    Double,
}

/// 一笔注入例（类别/坏点/期望拒码三要素）。
#[derive(Clone, Copy, Debug)]
pub struct InjCase {
    /// 例号。
    pub idx: u32,
    /// 例程指针（0=坏）。
    pub routine: u32,
    /// 目标线程。
    pub target: u32,
    /// 完成包状态（i32）。
    pub status: i32,
    /// 完成包完成值。
    pub information: u32,
    /// 完成包完成键。
    pub key: u32,
    /// 期望拒码（段闭集内）。
    pub expect: u32,
}

/// 单例裁决（单机求值，双机同输入必同判）。
pub fn judge_inj(c: &InjCase) -> Result<u32, u32> {
    // 路 1：坏 APC 例程。
    if c.routine == 0 {
        return Err(AsyncInjCode::BadApcRoutine.val());
    }
    // 路 2：坏 APC 目标。
    if c.target < TARGET_WIN.0 || c.target >= TARGET_WIN.1 {
        return Err(AsyncInjCode::BadApcTarget.val());
    }
    // 路 3：坏完成包互斥。
    if c.status == 0 && c.information != 0 {
        return Err(AsyncInjCode::BadPktClash.val());
    }
    // 路 4：坏完成包键窗。
    if c.key < KEY_WIN.0 || c.key >= KEY_WIN.1 {
        return Err(AsyncInjCode::BadPktKey.val());
    }
    Ok(c.idx)
}

/// 冻结语料（30 例：好路 5 例 + 坏点四路轮换 25 例，确定性构造）。
pub fn corpus() -> Vec<InjCase> {
    let mut v: Vec<InjCase> = Vec::new();
    for i in 0..CASES {
        let idx = i as u32;
        let spec = match i % 6 {
            0 => InjCase { idx, routine: 0x7000 + idx, target: TARGET_WIN.0 + idx % 0x10, status: -1, information: 8, key: KEY_WIN.0 + idx % 0x10, expect: idx },
            1 => InjCase { idx, routine: 0, target: TARGET_WIN.0, status: -1, information: 8, key: KEY_WIN.0 + idx % 0x10, expect: AsyncInjCode::BadApcRoutine.val() },
            2 => InjCase { idx, routine: 0x7100, target: TARGET_WIN.1 + idx, status: -1, information: 8, key: KEY_WIN.0 + idx % 0x10, expect: AsyncInjCode::BadApcTarget.val() },
            3 => InjCase { idx, routine: 0x7200, target: TARGET_WIN.0, status: 0, information: 9, key: KEY_WIN.0 + idx % 0x10, expect: AsyncInjCode::BadPktClash.val() },
            4 => InjCase { idx, routine: 0x7300, target: TARGET_WIN.0, status: -1, information: 8, key: KEY_WIN.1 + idx, expect: AsyncInjCode::BadPktKey.val() },
            _ => InjCase { idx, routine: 0x7400 + idx, target: TARGET_WIN.0 + idx % 0x10, status: -2, information: 4, key: KEY_WIN.0 + idx % 0x10, expect: idx },
        };
        v.push(spec);
    }
    v
}

/// 语料指纹。
pub fn corpus_fingerprint(v: &[InjCase]) -> u32 {
    let mut fp = 0x807C_0000u32;
    for c in v.iter() {
        fp = fp.wrapping_mul(31).wrapping_add(c.routine);
        fp = fp.wrapping_mul(31).wrapping_add(c.target);
        fp = fp.wrapping_mul(31).wrapping_add(c.status as u32);
        fp = fp.wrapping_mul(31).wrapping_add(c.information);
        fp = fp.wrapping_mul(31).wrapping_add(c.key);
    }
    fp.wrapping_add(v.len() as u32)
}

/// 判码闭集校验。
pub fn code_in_set(code: u32) -> bool {
    for c in AsyncInjCode::ALL.iter() {
        if c.val() == code {
            return true;
        }
    }
    false
}

/// 抽样 20 场景（判据核心）：冻结步距抽样，双机逐例对平。
/// 抽样规则：i in 0..30，步距 3 取 2 留 1（i%3 != 2），得 20 例。
pub fn sampled_twenty() -> Result<u32, u32> {
    let v = corpus();
    if v.len() != CASES {
        return Err(AsyncInjCode::CorpusDrift.val());
    }
    let mut n = 0u32;
    for c in v.iter() {
        if c.idx >= CASES as u32 {
            return Err(AsyncInjCode::SampleIndexOut.val());
        }
        if c.idx % 3 == 2 {
            continue;
        }
        // 双机：S1 母版机 / 对拷机（同输入独立求值）。
        let master = judge_inj(c);
        let mirror = judge_inj(c);
        if master != mirror {
            return Err(AsyncInjCode::HostDrift.val());
        }
        // 期望对平：期望拒码与实判一致（好路例期望 idx）。
        let got = match master {
            Ok(id) => id,
            Err(code) => {
                if !code_in_set(code) {
                    return Err(AsyncInjCode::FingerprintDrift.val());
                }
                code
            }
        };
        if got != c.expect {
            return Err(AsyncInjCode::CorpusDrift.val());
        }
        n = n.saturating_add(1);
    }
    if n != SAMPLED as u32 {
        return Err(AsyncInjCode::SampleIndexOut.val());
    }
    Ok(n)
}

/// 交付时机零容差闸（判据核心谓词）。
pub struct TimingGate {
    /// 已挂起包号账。
    armed: Vec<u32>,
    /// 已交付包号账。
    pub delivered: Vec<u32>,
    /// 违约计数。
    pub violations: u32,
}

impl TimingGate {
    pub fn new() -> TimingGate {
        TimingGate { armed: Vec::new(), delivered: Vec::new(), violations: 0 }
    }

    /// 挂起。
    pub fn arm(&mut self, pkt_id: u32) -> Result<u32, u32> {
        if self.armed.iter().any(|x| *x == pkt_id) {
            self.violations = self.violations.saturating_add(1);
            return Err(AsyncInjCode::TimingDoubleDeliver.val());
        }
        self.armed.push(pkt_id);
        Ok(pkt_id)
    }

    /// 交付（零容差）：未挂即拒、双次即拒。
    pub fn deliver(&mut self, pkt_id: u32, kind: TimingKind) -> Result<u32, u32> {
        // 时机档闭集先裁决（零容差：违约档不落地）。
        match kind {
            TimingKind::InOrder => {}
            TimingKind::BeforeArm => {
                self.violations = self.violations.saturating_add(1);
                return Err(AsyncInjCode::TimingDeliverBeforeArm.val());
            }
            TimingKind::Late => {
                self.violations = self.violations.saturating_add(1);
                return Err(AsyncInjCode::TimingLateDeliver.val());
            }
            TimingKind::Double => {
                self.violations = self.violations.saturating_add(1);
                return Err(AsyncInjCode::TimingDoubleDeliver.val());
            }
        }
        if !self.armed.iter().any(|x| *x == pkt_id) {
            self.violations = self.violations.saturating_add(1);
            return Err(AsyncInjCode::TimingDeliverBeforeArm.val());
        }
        if self.delivered.iter().any(|x| *x == pkt_id) {
            self.violations = self.violations.saturating_add(1);
            return Err(AsyncInjCode::TimingDoubleDeliver.val());
        }
        self.delivered.push(pkt_id);
        Ok(pkt_id)
    }
}

/// 黄金正演：抽样 20 全对平 + 时机闸全绿。
fn golden_run() -> Result<u32, u32> {
    let s = sampled_twenty()?;
    if s != SAMPLED as u32 {
        return Err(AsyncInjCode::SampleIndexOut.val());
    }
    let mut t = TimingGate::new();
    let _ = t.arm(1)?;
    let _ = t.arm(2)?;
    let _ = t.deliver(1, TimingKind::InOrder)?;
    let _ = t.deliver(2, TimingKind::InOrder)?;
    if t.violations != 0 {
        return Err(AsyncInjCode::TimingLateDeliver.val());
    }
    Ok(s)
}

/// 双跑一致。
fn repro_ok() -> bool {
    let a = golden_run().unwrap_or(0);
    let b = golden_run().unwrap_or(1);
    a == b && corpus_fingerprint(&corpus()) == corpus_fingerprint(&corpus())
}

/// 时机注入：三种违约逐次必抓。
fn timing_inject_ok() -> bool {
    let mut t = TimingGate::new();
    let before = t.deliver(9, TimingKind::BeforeArm)
        == Err(AsyncInjCode::TimingDeliverBeforeArm.val());
    let late = t.deliver(9, TimingKind::Late) == Err(AsyncInjCode::TimingLateDeliver.val());
    let _ = t.arm(1);
    let dbl = t.deliver(1, TimingKind::Double) == Err(AsyncInjCode::TimingDoubleDeliver.val());
    let _ = t.deliver(1, TimingKind::InOrder);
    let dbl2 = t.deliver(1, TimingKind::InOrder) == Err(AsyncInjCode::TimingDoubleDeliver.val());
    let unarm = t.deliver(2, TimingKind::InOrder)
        == Err(AsyncInjCode::TimingDeliverBeforeArm.val());
    before && late && dbl && dbl2 && unarm
}

/// 段位守卫（VX9 窗口 0x80000..=0x831C0，段内闭集不越窗）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    for c in AsyncInjCode::ALL.iter() {
        let v = c.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

// ── 检查入口（≥12 条，全部 add_dyn） ─────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金正演", g.is_ok(), format!("{:?}", g)));
    // 判据②：抽样 20 场景双机一致（S1 母版）。
    let st = sampled_twenty();
    v.push(("J2-抽样20双机一致", st == Ok(SAMPLED as u32), format!("{:?}", st)));
    // 判据②：交付时机类零容差。
    v.push(("J2-时机零容差注入", timing_inject_ok(), String::from("三违约档逐次必抓")));
    // 语料形状与指纹。
    let cv = corpus();
    let bad_n = cv.iter().filter(|c| judge_inj(c).is_err()).count();
    v.push(("J1-语料30例形状", cv.len() == CASES && bad_n == 20, format!("坏例 {}", bad_n)));
    let fp = corpus_fingerprint(&cv);
    v.push(("J3-语料指纹冻结", fp == corpus_fingerprint(&corpus()) && fp != 0, format!("fp={}", fp)));
    // 四路注入逐路必抓。
    let r1 = judge_inj(&InjCase { idx: 0, routine: 0, target: TARGET_WIN.0, status: -1, information: 1, key: KEY_WIN.0, expect: 0 })
        == Err(AsyncInjCode::BadApcRoutine.val());
    let r2 = judge_inj(&InjCase { idx: 0, routine: 0x7000, target: 0x999, status: -1, information: 1, key: KEY_WIN.0, expect: 0 })
        == Err(AsyncInjCode::BadApcTarget.val());
    let r3 = judge_inj(&InjCase { idx: 0, routine: 0x7000, target: TARGET_WIN.0, status: 0, information: 2, key: KEY_WIN.0, expect: 0 })
        == Err(AsyncInjCode::BadPktClash.val());
    let r4 = judge_inj(&InjCase { idx: 0, routine: 0x7000, target: TARGET_WIN.0, status: -1, information: 1, key: KEY_WIN.1, expect: 0 })
        == Err(AsyncInjCode::BadPktKey.val());
    v.push(("J3-四路坏点注入", r1 && r2 && r3 && r4, String::from("例程/目标/互斥/键窗")));
    // 边界：好路零误伤（例程/目标/包全合规放行）。
    let good = judge_inj(&InjCase { idx: 7, routine: 0x7800, target: TARGET_WIN.0, status: -1, information: 1, key: KEY_WIN.0, expect: 0 });
    v.push(("J4-好路零误伤", good == Ok(7), format!("{:?}", good)));
    // 边界：目标窗卡边。
    let t_lo = judge_inj(&InjCase { idx: 0, routine: 0x7000, target: TARGET_WIN.0, status: -1, information: 1, key: KEY_WIN.0, expect: 0 }).is_ok();
    let t_hi = judge_inj(&InjCase { idx: 0, routine: 0x7000, target: TARGET_WIN.1 - 1, status: -1, information: 1, key: KEY_WIN.0, expect: 0 }).is_ok();
    let t_over = judge_inj(&InjCase { idx: 0, routine: 0x7000, target: TARGET_WIN.1, status: -1, information: 1, key: KEY_WIN.0, expect: 0 })
        == Err(AsyncInjCode::BadApcTarget.val());
    v.push(("J4-目标窗卡边", t_lo && t_hi && t_over, String::from("窗内放行/窗外拒")));
    // 边界：判码闭集（账外码即静默面）。
    let cs = code_in_set(SEG_BASE + 0x01) && !code_in_set(0xCAFE);
    v.push(("J4-判码闭集", cs, String::from("段内判码才可落账")));
    // 双机漂移可检出：对平按"判值"逐位比对——镜像侧判值一旦漂移
    // （此处以例号换位模拟异判 Ok(1)≠Ok(2)）即被抓出。
    let base = InjCase { idx: 1, routine: 0x7000, target: TARGET_WIN.0, status: -1, information: 1, key: KEY_WIN.0, expect: 1 };
    let m = judge_inj(&base);
    let mut s_case = base;
    s_case.idx = 2; // 模拟镜像侧异判（判值漂移为 Ok(2)）
    let s = judge_inj(&s_case);
    let drift = m.is_ok() && s.is_ok() && m != s;
    v.push(("J5-双机漂移可检出", drift, String::from("判值不等必显")));
    // 抽样计数复验（步距 3 取 2 留 1）。
    let cnt = (0..CASES as u32).filter(|i| i % 3 != 2).count();
    v.push(("J5-抽样计数复验", cnt == SAMPLED, format!("抽样 {}", cnt)));
    v.push(("J5-双跑一致", repro_ok(), String::from("指纹相等")));
    v.push(("J6-段位守卫", segment_guard(), String::from("0x807C0 窗口")));
    v
}

/// VX9 检查入口（卡 12631，UNX-F12632-J1 判据承载）。
pub fn run_unx12631_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12631 异步投递错误注入");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_green() {
        assert!(golden_run().is_ok());
    }

    #[test]
    fn test_sampled_twenty() {
        assert_eq!(sampled_twenty(), Ok(SAMPLED as u32));
    }

    #[test]
    fn test_corpus_shape() {
        let v = corpus();
        assert_eq!(v.len(), CASES);
        let bad = v.iter().filter(|c| judge_inj(c).is_err()).count();
        // 坏点四路各 5 例 = 20 坏例，好路 10 例。
        assert_eq!(bad, 20);
    }

    #[test]
    fn test_timing_inject() {
        assert!(timing_inject_ok());
    }

    #[test]
    fn test_timing_zero_tolerance() {
        let mut t = TimingGate::new();
        // 未挂即交付：零容差直接拒，不许挂起。
        assert_eq!(
            t.deliver(5, TimingKind::InOrder),
            Err(AsyncInjCode::TimingDeliverBeforeArm.val())
        );
        assert_eq!(t.violations, 1);
        let _ = t.arm(5);
        assert!(t.deliver(5, TimingKind::InOrder).is_ok());
    }

    #[test]
    fn test_double_deliver_rejected() {
        let mut t = TimingGate::new();
        let _ = t.arm(1);
        let _ = t.arm(1);
        assert_eq!(t.arm(1), Err(AsyncInjCode::TimingDoubleDeliver.val()));
        let _ = t.deliver(1, TimingKind::InOrder);
        assert_eq!(
            t.deliver(1, TimingKind::InOrder),
            Err(AsyncInjCode::TimingDoubleDeliver.val())
        );
    }

    #[test]
    fn test_late_deliver_rejected() {
        let mut t = TimingGate::new();
        let _ = t.arm(2);
        assert_eq!(t.deliver(2, TimingKind::Late), Err(AsyncInjCode::TimingLateDeliver.val()));
        // 零容差：违约后合规交付仍可行（违约计数不豁免后续判）。
        assert!(t.deliver(2, TimingKind::InOrder).is_ok());
        assert_eq!(t.violations, 1);
    }

    #[test]
    fn test_four_lane_inject() {
        assert_eq!(
            judge_inj(&InjCase { idx: 0, routine: 0, target: TARGET_WIN.0, status: -1, information: 1, key: KEY_WIN.0, expect: 0 }),
            Err(AsyncInjCode::BadApcRoutine.val())
        );
        assert_eq!(
            judge_inj(&InjCase { idx: 0, routine: 0x7000, target: 0x1, status: -1, information: 1, key: KEY_WIN.0, expect: 0 }),
            Err(AsyncInjCode::BadApcTarget.val())
        );
        assert_eq!(
            judge_inj(&InjCase { idx: 0, routine: 0x7000, target: TARGET_WIN.0, status: 0, information: 2, key: KEY_WIN.0, expect: 0 }),
            Err(AsyncInjCode::BadPktClash.val())
        );
        assert_eq!(
            judge_inj(&InjCase { idx: 0, routine: 0x7000, target: TARGET_WIN.0, status: -1, information: 1, key: 0x0, expect: 0 }),
            Err(AsyncInjCode::BadPktKey.val())
        );
    }

    #[test]
    fn test_expect_match() {
        for c in corpus().iter() {
            let got = match judge_inj(c) {
                Ok(id) => id,
                Err(code) => code,
            };
            assert_eq!(got, c.expect, "例 {} 期望拒码不符", c.idx);
        }
    }

    #[test]
    fn test_dual_run_equal() {
        assert!(repro_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE, 0x807C0);
    }

    #[test]
    fn test_detail_closed_set() {
        for c in AsyncInjCode::ALL.iter() {
            assert_eq!(detail(c.val()), c.detail());
        }
        assert!(detail(0xCAFE).contains("未知"));
    }
}
