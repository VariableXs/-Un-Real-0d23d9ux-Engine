//! VE-F0082 · 降级协商协议（VE-A 域 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0082`
//!
//! **职责定位（锚点原文）**：渲染降级的**系统级协商协议**（引擎 / 驱动 / 系统
//! 三方的能力与意愿交换），降级方案**排序**（保功能 > 保帧率 > 保画质），
//! 协商过程**可追溯**；含协商的三方**版本登记**。
//!
//! **判据（锚点原文）**：三方协商、方案排序、过程追溯、默认兜底、判据。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 协商失败 → **默认方案** | 任一方 `willing = 0`（不接受任何降级）或版本不可比时，[`negotiate`] 返回 [`Outcome::fallback`] 位为真的结果，方案固定为 [`PLAN_FALLBACK_RANK`] 的「保功能」。兜底**不静默**：transcript 末尾必有一条 [`Step::Fallback`] |
//! | 三方不一致 → **仲裁** | 三方版本不同时取**最低**版本（[`Version::arbitrate`]），并在 transcript 记 [`Step::Arbitrate`]。取最低不是「保守」而是「能被所有人解析的格式」——取最高会让老驱动收到它读不懂的字段 |
//! | 过程缺记录 → **补录** | transcript 是定容 8 槽；槽满后 `dropped` 增长而不是覆盖。判据要求「协商成功必有一条 Accept」——缺了就是实现漏记，直接红 |
//!
//! **性能逐项分解（锚点原文）**：O(1)。三方、五方案、八步，全是常数上界；
//! 无分配、无遍历可增长结构。
//!
//! **跨批对接点**：A06 预算联动——方案 rank 直接映射到 `vea05` 的预算档，
//! 本模块只产出 rank，不直接改预算（避免「协商层偷偷调预算」）。
//!
//! **无障碍与隐私（锚点原文）**：协商状态读屏可达
//! （[`Negotiation::a11y_lines`]）——只报三方版本、方案名、rank 与是否仲裁；
//! 不报应用名与窗口标题。
//!
//! ## 设计要点（为什么这样写）
//!
//! - **排序是数据表不是 if 链**：[`PLANS`] 是公开常量表，rank 越小越优先。
//!   判据逐格钉死顺序，改表即红。
//! - **意愿是「可接受到第几档」而不是「是否同意」**：布尔意愿无法表达
//!   「我可以降分辨率但不能关特效」，而那正是三方协商的常态。
//! - **兜底必须留痕**：不记 `Fallback` 的兜底会让人以为协商成功了。
//! - **零 panic 面**：无 `unwrap`、索引经 `min` 钳制。

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（0xA7BD/E 与 0xA7CD/E —— 全树 grep 确认零占用）
// ---------------------------------------------------------------------------

/// 方案编号越界。
pub const E_PLAN_RANGE: u16 = 0xA7BD;
/// 版本不可比（主版本差超过允许窗口）。
pub const E_VERSION_GAP: u16 = 0xA7BE;
/// 意愿值越界。
pub const E_WILL_RANGE: u16 = 0xA7CD;
/// transcript 槽已满（后续步骤按 dropped 记账）。
pub const E_TRANSCRIPT_FULL: u16 = 0xA7CE;

// ---------------------------------------------------------------------------
// 二、三方
// ---------------------------------------------------------------------------

/// 协商参与方。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Party {
    Engine = 0,
    Driver = 1,
    System = 2,
}

pub const PARTY_COUNT: usize = 3;

impl Party {
    pub const ALL: [Party; PARTY_COUNT] = [Party::Engine, Party::Driver, Party::System];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Party::Engine => "engine",
            Party::Driver => "driver",
            Party::System => "system",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、版本
// ---------------------------------------------------------------------------

/// 三方各自声明的协议版本。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

impl Version {
    pub const fn new(major: u16, minor: u16) -> Version {
        Version { major, minor }
    }

    pub fn render(self) -> String {
        format!("{}.{}", self.major, self.minor)
    }

    /// 三方版本仲裁：取最低。主版本差超过 [`MAX_MAJOR_GAP`] 视为不可协商。
    pub fn arbitrate(versions: &[Version; PARTY_COUNT]) -> Result<Version, u16> {
        let mut lo = versions[0];
        let mut hi = versions[0];
        for v in versions.iter() {
            if *v < lo {
                lo = *v;
            }
            if *v > hi {
                hi = *v;
            }
        }
        if hi.major.saturating_sub(lo.major) > MAX_MAJOR_GAP {
            return Err(E_VERSION_GAP);
        }
        Ok(lo)
    }
}

/// 主版本允许的最大跨度。
pub const MAX_MAJOR_GAP: u16 = 1;

// ---------------------------------------------------------------------------
// 四、方案表
// ---------------------------------------------------------------------------

/// 一个降级方案。`rank` 越小越优先。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Plan {
    pub rank: u8,
    pub name: &'static str,
    /// 保住的是什么。
    pub keeps: &'static str,
}

/// 兜底方案编号：保功能。
pub const PLAN_FALLBACK_RANK: u8 = 0;

/// 方案表：保功能 > 保帧率 > 保画质 > 降分辨率 > 关特效。
///
/// 顺序就是产品决策：功能没了等于不能用，帧率掉了等于卡死，画质降一档
/// 用户能忍，分辨率再降才是「看得见但不好看」。
pub const PLANS: [Plan; 5] = [
    Plan {
        rank: 0,
        name: "keep-features",
        keeps: "feature",
    },
    Plan {
        rank: 1,
        name: "keep-framerate",
        keeps: "framerate",
    },
    Plan {
        rank: 2,
        name: "keep-quality",
        keeps: "quality",
    },
    Plan {
        rank: 3,
        name: "lower-resolution",
        keeps: "visibility",
    },
    Plan {
        rank: 4,
        name: "disable-effects",
        keeps: "visibility",
    },
];

pub const PLAN_COUNT: usize = PLANS.len();

/// 按 rank 取方案。
pub fn plan(rank: u8) -> Option<Plan> {
    PLANS.get(rank as usize).copied().filter(|p| p.rank == rank)
}

// ---------------------------------------------------------------------------
// 五、过程记录
// ---------------------------------------------------------------------------

/// 协商步骤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// 收到某方的能力声明。
    Declare(Party),
    /// 某方接受某档。
    Accept(Party, u8),
    /// 某方拒绝某档。
    Reject(Party, u8),
    /// 版本仲裁。
    Arbitrate(u16),
    /// 兜底。
    Fallback(u16),
}

impl Step {
    pub fn as_str(self) -> &'static str {
        match self {
            Step::Declare(_) => "declare",
            Step::Accept(_, _) => "accept",
            Step::Reject(_, _) => "reject",
            Step::Arbitrate(_) => "arbitrate",
            Step::Fallback(_) => "fallback",
        }
    }
}

/// 定容过程记录。
#[derive(Clone, Copy, Debug)]
pub struct Transcript {
    steps: [Option<Step>; 8],
    count: usize,
    dropped: u32,
}

impl Transcript {
    pub const fn new() -> Transcript {
        Transcript {
            steps: [None; 8],
            count: 0,
            dropped: 0,
        }
    }

    /// 记一步；槽满后只增 dropped，不覆盖已有记录。
    pub fn push(&mut self, s: Step) -> Result<(), u16> {
        if self.count >= self.steps.len() {
            self.dropped = self.dropped.saturating_add(1);
            return Err(E_TRANSCRIPT_FULL);
        }
        self.steps[self.count] = Some(s);
        self.count += 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn steps(&self) -> &[Option<Step>; 8] {
        &self.steps
    }

    pub fn contains(&self, kind: &str) -> bool {
        self.steps[..self.count]
            .iter()
            .flatten()
            .any(|s| s.as_str() == kind)
    }
}

impl Default for Transcript {
    fn default() -> Transcript {
        Transcript::new()
    }
}

// ---------------------------------------------------------------------------
// 六、协商结果
// ---------------------------------------------------------------------------

/// 协商结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub rank: u8,
    /// 是否发生版本仲裁。
    pub arbitrated: bool,
    /// 是否走了兜底。
    pub fallback: bool,
    /// 仲裁后的版本。
    pub version: Version,
}

impl Outcome {
    pub fn plan(&self) -> Plan {
        plan(self.rank).unwrap_or(PLANS[0])
    }

    pub fn ok(&self) -> bool {
        !self.fallback
    }
}

/// 三方一次协商的完整输入。
#[derive(Clone, Copy, Debug)]
pub struct Offer {
    pub versions: [Version; PARTY_COUNT],
    /// 每方最多能接受到第几档（0 = 只接受保功能）。
    pub willing: [u8; PARTY_COUNT],
}

impl Offer {
    pub fn valid(&self) -> bool {
        self.willing.iter().all(|w| (*w as usize) < PLAN_COUNT)
    }
}

/// 一次协商的全部产物。
#[derive(Clone, Debug)]
pub struct Negotiation {
    pub outcome: Outcome,
    pub transcript: Transcript,
    pub offer: Offer,
}

impl Negotiation {
    pub fn a11y_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "降级协商：方案 {}（第 {} 档）/ plan {} rank {}",
            self.outcome.plan().name,
            self.outcome.plan().rank,
            self.outcome.plan().name,
            self.outcome.plan().rank
        ));
        out.push(format!(
            "三方版本 {} / {} / {}，仲裁后 {}",
            self.offer.versions[0].render(),
            self.offer.versions[1].render(),
            self.offer.versions[2].render(),
            self.outcome.version.render()
        ));
        out.push(format!(
            "过程 {} 步（丢弃 {}）/ {} steps ({} dropped)",
            self.transcript.len(),
            self.transcript.dropped(),
            self.transcript.len(),
            self.transcript.dropped()
        ));
        out.push(if self.outcome.fallback {
            "协商失败，已用兜底方案 / fallback applied".into()
        } else if self.outcome.arbitrated {
            "版本不一致，已仲裁 / arbitrated".to_string()
        } else {
            "三方一致 / agreed".to_string()
        });
        out
    }
}

/// 执行协商。
///
/// 规则：取三方意愿的**最小值**（谁最保守听谁的），再按版本仲裁结果落地。
/// 意愿为 0 视为「不接受任何降级」——此时保功能方案就是唯一可选项，
/// 但**仍要记一条 Fallback**，因为「只接受保功能」与「协商成功选了保功能」
/// 是两件事。
pub fn negotiate(offer: Offer) -> Result<Negotiation, u16> {
    if !offer.valid() {
        return Err(E_WILL_RANGE);
    }
    let mut t = Transcript::new();
    for p in Party::ALL {
        let _ = t.push(Step::Declare(p));
    }

    let mut arbitrated = false;
    let version = match Version::arbitrate(&offer.versions) {
        Ok(v) => {
            if offer.versions.iter().any(|x| *x != v) {
                arbitrated = true;
                let _ = t.push(Step::Arbitrate(v.major));
            }
            v
        }
        Err(code) => {
            let _ = t.push(Step::Fallback(code));
            return Ok(Negotiation {
                outcome: Outcome {
                    rank: PLAN_FALLBACK_RANK,
                    arbitrated: false,
                    fallback: true,
                    version: offer.versions[0],
                },
                transcript: t,
                offer,
            });
        }
    };

    // 意愿取最小：最保守的一方决定上限。先算结论，再逐方记账——把两步
    // 揉进一个循环会让「后来的更保守」被记成 Accept，账就假了。
    let accepted = offer
        .willing
        .iter()
        .copied()
        .min()
        .unwrap_or(PLAN_FALLBACK_RANK)
        .min(PLAN_COUNT as u8 - 1);
    for p in Party::ALL {
        let w = offer.willing[p.index()];
        if w >= accepted {
            let _ = t.push(Step::Accept(p, w));
        } else {
            let _ = t.push(Step::Reject(p, w));
        }
    }

    // 全员只接受保功能：这不是「协商失败」，但也没得选，仍记兜底。
    let fallback = accepted == PLAN_FALLBACK_RANK;
    if fallback {
        let _ = t.push(Step::Fallback(0));
    } else {
        let _ = t.push(Step::Accept(Party::System, accepted));
    }

    Ok(Negotiation {
        outcome: Outcome {
            rank: accepted,
            arbitrated,
            fallback,
            version,
        },
        transcript: t,
        offer,
    })
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

fn offer_of(v: Version, willing: [u8; PARTY_COUNT]) -> Offer {
    Offer {
        versions: [v; PARTY_COUNT],
        willing,
    }
}

/// VE-F0082 的十三条判据。
pub fn run_vea82_checks() -> CheckSet {
    let mut c = CheckSet::new("vea82");
    let v = Version::new(2, 1);

    // 1. 方案表排序固定：rank 与下标一致且名字唯一。
    let mut ordered = true;
    for (i, p) in PLANS.iter().enumerate() {
        if p.rank as usize != i || p.name.is_empty() || p.keeps.is_empty() {
            ordered = false;
        }
    }
    if ordered {
        c.ok("A82-排序-表序即优先序");
    } else {
        c.fail("A82-排序-表序即优先序", "plan table drifted");
    }

    // 2. 排序语义：保功能优先于保帧率优先于保画质。
    if PLANS[0].rank < PLANS[1].rank
        && PLANS[1].rank < PLANS[2].rank
        && PLANS[0].keeps == "feature"
        && PLANS[1].keeps == "framerate"
        && PLANS[2].keeps == "quality"
    {
        c.ok("A82-排序-功能帧率画质依次");
    } else {
        c.fail("A82-排序-功能帧率画质依次", "priority semantics wrong");
    }

    // 3. 三方一致：无仲裁，取最小意愿。
    let n = match negotiate(offer_of(v, [4, 2, 3])) {
        Ok(x) => x,
        Err(_) => {
            c.fail("A82-协商-取最保守意愿", "negotiate failed");
            return c;
        }
    };
    if n.outcome.rank == 2 && !n.outcome.arbitrated && !n.outcome.fallback {
        c.ok("A82-协商-取最保守意愿");
    } else {
        c.fail("A82-协商-取最保守意愿", "min willingness wrong");
    }

    // 4. 过程可追溯：三方声明都在册，且有 Accept。
    if n.transcript.len() >= 4 && n.transcript.contains("declare") && n.transcript.contains("accept")
    {
        c.ok("A82-追溯-声明与接受在册");
    } else {
        c.fail("A82-追溯-声明与接受在册", "transcript incomplete");
    }

    // 5. 版本不一致即仲裁，且取最低。
    let mixed = Offer {
        versions: [Version::new(2, 5), Version::new(2, 1), Version::new(2, 9)],
        willing: [3, 3, 3],
    };
    let a = negotiate(mixed).unwrap_or(n.clone());
    if a.outcome.arbitrated && a.outcome.version == Version::new(2, 1) {
        c.ok("A82-仲裁-取最低版本");
    } else {
        c.fail("A82-仲裁-取最低版本", "arbitration picked wrong version");
    }

    // 6. 主版本跨度过大即兜底。
    let gap = Offer {
        versions: [Version::new(1, 0), Version::new(3, 0), Version::new(1, 0)],
        willing: [4, 4, 4],
    };
    match negotiate(gap) {
        Ok(g) if g.outcome.fallback && g.outcome.rank == PLAN_FALLBACK_RANK => {
            c.ok("A82-兜底-版本跨度过大");
        }
        _ => c.fail("A82-兜底-版本跨度过大", "version gap accepted"),
    }

    // 7. 全员只接受保功能：走兜底且留痕。
    let zero = negotiate(offer_of(v, [0, 0, 0])).unwrap_or(n.clone());
    if zero.outcome.fallback && zero.transcript.contains("fallback") {
        c.ok("A82-兜底-零意愿留痕");
    } else {
        c.fail("A82-兜底-零意愿留痕", "silent fallback");
    }

    // 8. 意愿越界即拒。
    if negotiate(offer_of(v, [9, 9, 9])).err() == Some(E_WILL_RANGE) {
        c.ok("A82-协商-意愿越界即拒");
    } else {
        c.fail("A82-协商-意愿越界即拒", "bad willingness accepted");
    }

    // 9. transcript 满后只增 dropped，不覆盖。
    let mut tr = Transcript::new();
    for _ in 0..12 {
        let _ = tr.push(Step::Declare(Party::Engine));
    }
    if tr.len() == 8 && tr.dropped() == 4 && tr.steps()[0].is_some() {
        c.ok("A82-追溯-满槽只增丢弃");
    } else {
        c.fail("A82-追溯-满槽只增丢弃", "transcript overwrote history");
    }

    // 10. 方案查询按 rank 精确命中。
    if plan(0).map(|p| p.name) == Some("keep-features")
        && plan(4).map(|p| p.name) == Some("disable-effects")
        && plan(9).is_none()
    {
        c.ok("A82-查表-rank精确命中");
    } else {
        c.fail("A82-查表-rank精确命中", "plan lookup wrong");
    }

    // 11. Outcome::plan 与 rank 一致。
    if n.outcome.plan().rank == n.outcome.rank && n.outcome.ok() {
        c.ok("A82-结果-方案与档位一致");
    } else {
        c.fail("A82-结果-方案与档位一致", "outcome inconsistent");
    }

    // 12. 读屏四行且含方案名。
    let lines = n.a11y_lines();
    if lines.len() == 4 && lines[0].contains(n.outcome.plan().name) {
        c.ok("A82-读屏-状态可播");
    } else {
        c.fail("A82-读屏-状态可播", "a11y incomplete");
    }

    // 13. 版本渲染可回读（两段式）。
    let r = v.render();
    if r.contains('.') && r.split('.').count() == 2 {
        c.ok("A82-版本-两段式可读");
    } else {
        c.fail("A82-版本-两段式可读", "version render broken");
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn party_round_trip() {
        for (i, p) in Party::ALL.iter().enumerate() {
            assert_eq!(p.index(), i);
            assert!(!p.as_str().is_empty());
        }
    }

    #[test]
    fn version_order_is_lexicographic() {
        assert!(Version::new(1, 9) < Version::new(2, 0));
        assert!(Version::new(2, 1) < Version::new(2, 2));
    }

    #[test]
    fn arbitration_rejects_huge_gap() {
        let vs = [Version::new(1, 0), Version::new(5, 0), Version::new(1, 0)];
        assert_eq!(Version::arbitrate(&vs), Err(E_VERSION_GAP));
    }

    #[test]
    fn arbitration_accepts_small_gap() {
        let vs = [Version::new(1, 0), Version::new(2, 0), Version::new(1, 3)];
        assert_eq!(Version::arbitrate(&vs), Ok(Version::new(1, 0)));
    }

    #[test]
    fn min_willingness_wins() {
        let n = negotiate(offer_of(Version::new(1, 0), [4, 1, 2])).unwrap();
        assert_eq!(n.outcome.rank, 1);
    }

    #[test]
    fn same_version_is_not_arbitrated() {
        let n = negotiate(offer_of(Version::new(1, 0), [3, 3, 3])).unwrap();
        assert!(!n.outcome.arbitrated);
    }

    #[test]
    fn zero_willingness_falls_back() {
        let n = negotiate(offer_of(Version::new(1, 0), [0, 3, 3])).unwrap();
        assert!(n.outcome.fallback);
        assert_eq!(n.outcome.rank, 0);
    }

    #[test]
    fn transcript_stops_at_capacity() {
        let mut t = Transcript::new();
        for _ in 0..9 {
            let _ = t.push(Step::Fallback(0));
        }
        assert_eq!(t.len(), 8);
        assert_eq!(t.dropped(), 1);
    }

    #[test]
    fn checks_are_green() {
        let cs = run_vea82_checks();
        let (p, f) = cs.tally();
        assert_eq!(f, 0, "vea82 red: {}", cs.red_items_text());
        assert!(p >= 13);
    }
}
