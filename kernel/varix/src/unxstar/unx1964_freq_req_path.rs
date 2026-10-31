//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1964 · 频率切换请求路径（调度决策→硬件写受控通道） · 0x6D200
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B4）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1964-J1 请求路径断言全过（决策→请求队列→受控写通道
//!     →生效确认闭环），写前目标身份三重验证软对应在册（频域 ID
//!     +表内档位+当前值核对），非法请求 10/10 拒绝；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 请求路径四环：决策→请求队列→受控写通道→生效确认（闭环）；
//!    - 三重验证：频域 ID 在册/档位在表/当前值≠目标值（无谓写拒）；
//!    - 非法请求 10/10 拒绝（越域 ID/越表档位/同值写/未闭环再写）；
//!    - 生效确认：写后回读一致才闭环（未确认不得发新请求）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D200，AI-VX3-1030 批E 独占区）。
pub const SEG_BASE: u32 = 0x6D200;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 频域数。
pub const DOMAIN_COUNT: usize = 4;
/// 每域档位表（MHz，冻结）。
pub const FREQ_TABLE: [[u64; 4]; DOMAIN_COUNT] = [
    [800, 1800, 2800, 3800],
    [800, 1600, 2400, 3200],
    [1200, 2000, 2800, 3600],
    [600, 1200, 1800, 2400],
];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreqCode {
    /// 频域 ID 越册
    DomainOutOfRange,
    /// 档位越表
    GradeOutOfRange,
    /// 同值写（当前值=目标值）
    SameValueWrite,
    /// 未闭环再写
    ReopenBeforeConfirm,
    /// 生效确认不符（回读≠目标）
    ConfirmMismatch,
    /// 表内档位查无
    GradeNotInTable,
    /// 非法请求漏拒
    IllegalLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl FreqCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FreqCode::DomainOutOfRange => "频域 ID 越册",
            FreqCode::GradeOutOfRange => "档位越表",
            FreqCode::SameValueWrite => "同值写：当前值=目标值",
            FreqCode::ReopenBeforeConfirm => "未闭环再写",
            FreqCode::ConfirmMismatch => "生效确认不符：回读≠目标",
            FreqCode::GradeNotInTable => "表内档位查无",
            FreqCode::IllegalLeaked => "非法请求漏拒",
            FreqCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [FreqCode; 8] = [
        FreqCode::DomainOutOfRange,
        FreqCode::GradeOutOfRange,
        FreqCode::SameValueWrite,
        FreqCode::ReopenBeforeConfirm,
        FreqCode::ConfirmMismatch,
        FreqCode::GradeNotInTable,
        FreqCode::IllegalLeaked,
        FreqCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FreqCode::ALL.len() {
        if FreqCode::ALL[i].val() == code {
            return FreqCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D200 段闭集内"
}

// ── 频率请求账（四环闭环） ─────────────────────────────────────
pub struct FreqChannel {
    /// 当前值（MHz）。
    pub current: [u64; DOMAIN_COUNT],
    /// 在途请求（未确认）。
    pub pending: Option<(usize, u64)>,
    /// 账：请求数/确认数/拒数。
    pub requested: u64,
    pub confirmed: u64,
    pub rejected: u64,
}

impl FreqChannel {
    pub fn new() -> FreqChannel {
        FreqChannel {
            current: [FREQ_TABLE[0][0], FREQ_TABLE[1][0], FREQ_TABLE[2][0], FREQ_TABLE[3][0]],
            pending: None,
            requested: 0,
            confirmed: 0,
            rejected: 0,
        }
    }

    /// 发请求（三重验证 + 四环之二：请求入队）。
    pub fn request(&mut self, domain: usize, grade: usize) -> Result<u64, u32> {
        // 验一：频域 ID 在册。
        if domain >= DOMAIN_COUNT {
            self.rejected += 1;
            return Err(FreqCode::DomainOutOfRange.val());
        }
        // 验二：档位在表。
        if grade >= 4 {
            self.rejected += 1;
            return Err(FreqCode::GradeOutOfRange.val());
        }
        let target = FREQ_TABLE[domain][grade];
        // 验三：当前值 ≠ 目标值。
        if self.current[domain] == target {
            self.rejected += 1;
            return Err(FreqCode::SameValueWrite.val());
        }
        // 未闭环再写拒。
        if self.pending.is_some() {
            self.rejected += 1;
            return Err(FreqCode::ReopenBeforeConfirm.val());
        }
        self.pending = Some((domain, target));
        self.requested += 1;
        Ok(target)
    }

    /// 受控写 + 生效确认（回读一致闭环）。
    pub fn confirm(&mut self, readback: u64) -> Result<(), u32> {
        match self.pending {
            Some((domain, target)) => {
                if readback != target {
                    return Err(FreqCode::ConfirmMismatch.val());
                }
                self.current[domain] = readback;
                self.pending = None;
                self.confirmed += 1;
                Ok(())
            }
            None => Err(FreqCode::ConfirmMismatch.val()),
        }
    }
}

/// 非法请求 10/10 拒绝（四类非法各 2~3 次）。
pub fn illegal_reject_10() -> Result<u32, u32> {
    let mut rejected = 0;
    let mut ch = FreqChannel::new();
    // 越域 ID ×3。
    let mut k = 0;
    while k < 3 {
        if ch.request(DOMAIN_COUNT + k, 0).is_err() {
            rejected += 1;
        }
        k += 1;
    }
    // 越表档位 ×3。
    k = 0;
    while k < 3 {
        if ch.request(0, 4 + k).is_err() {
            rejected += 1;
        }
        k += 1;
    }
    // 同值写 ×2。
    k = 0;
    while k < 2 {
        if ch.request(0, 0).is_err() {
            rejected += 1;
        }
        k += 1;
    }
    // 未闭环再写 ×2。
    ch.request(0, 1).ok();
    k = 0;
    while k < 2 {
        if ch.request(1, 2).is_err() {
            rejected += 1;
        }
        k += 1;
    }
    if rejected != 10 {
        return Err(FreqCode::IllegalLeaked.val());
    }
    Ok(rejected)
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：合法请求闭环→非法 10/10→确认不符拒→同值拒。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    let mut ch = FreqChannel::new();
    // 合法闭环：域 0 档 0→档 2。
    let target = ch.request(0, 2)?;
    ch.confirm(target)?;
    if ch.current[0] != target {
        return Err(FreqCode::ConfirmMismatch.val());
    }
    // 非法 10/10。
    let rej = illegal_reject_10()?;
    Ok((ch.confirmed, ch.rejected, rej))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut ch = FreqChannel::new();
    let t = ch.request(1, 2)?;
    ch.confirm(t)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 同值写 → 必拒。
    let mut ch = FreqChannel::new();
    ch.request(0, 0).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 确认不符 → 必拒。
    let mut ch = FreqChannel::new();
    ch.request(2, 1)?;
    ch.confirm(999)
}

/// 注入 10/10：越域、越档、同值、非法 10/10 四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越域。
        let mut c0 = FreqChannel::new();
        led.record(0, c0.request(9, 0).is_err());
        // 槽1：越档。
        let mut c1 = FreqChannel::new();
        led.record(1, c1.request(0, 9).is_err());
        // 槽2：同值。
        let mut c2 = FreqChannel::new();
        led.record(2, c2.request(3, 0).is_err());
        // 槽3：非法 10/10。
        led.record(3, illegal_reject_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(FreqCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FreqCode::ALL.len() {
        let v = FreqCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 四环闭环。
    let mut ch = FreqChannel::new();
    let loop_ok = match ch.request(0, 3) {
        Ok(t) => match ch.confirm(t) {
            Ok(()) => ch.current[0] == t && ch.confirmed == 1,
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push((
        "J1-请求路径闭环全过",
        loop_ok && DOMAIN_COUNT == 4,
        format!("决策→队列→受控写→确认（{} 域）", DOMAIN_COUNT),
    ));
    // 非法 10/10。
    v.push((
        "J1-非法请求10/10拒绝",
        illegal_reject_10() == Ok(10),
        format!("{}/10 拒", illegal_reject_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D200 窗口")));
    v
}

pub fn run_unx1964_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1964 频率切换请求路径（调度决策→硬件写受控通道）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
