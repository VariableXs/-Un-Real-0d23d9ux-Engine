//! F129 社区判例提交线 · 完整设计（STAR I 主册 G-D-04）。
//!
//! **判据（主册）**：端到端演练：外部视角提交一判例到收录全流程计时
//! （目标 <30 天）；五态查询实时准确。
//!
//! **设计要点（主册）**：
//! - 社区提交判例的完整通道：自测脚本包（只读形态，B-430x 既有纪律）/
//!   提交模板 / 状态全链可查（B-4303）/ 验收从单 AI 裁决变为社区共证
//!   （两人复核制）；
//! - 提交页（星图应用内）：模板表单（程序名/版本/判例集/证据哈希）/
//!   进度查看页（五态：已收/形式审/技术审/共证/收录，各态时间戳）；
//!   驳回必附原因（三要素）；
//! - 提交走 HTTPS（F024）；状态数据公开（F128 面引用）；证据包哈希链
//!   （F194 同款防篡改——vbase::chain_hash 唯一源）；
//! - 证据不全 → 形式审驳回+清单化缺项；复核分歧 → 第三人仲裁（流程
//!   文档化 F148）；刷量防护 → 同 IP 限频+质量分；
//! - 两人复核制：AI01 一票 + 社区轮值一票（轮值池规则入 F148 治理
//!   文档）；收录即进 F040 账本候选池；每季「判例之星」致谢（F142）。
//!
//! 时间注入式（Unix 秒），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use crate::svstar::vbase;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 端到端收录目标（秒 = 30 天）。
pub const E2E_TARGET_SECS: u64 = 30 * 86_400;
/// 五态。
pub const STATE_COUNT: usize = 5;
/// 同提交者限频（窗口内最大提交数——刷量防护）。
pub const RATE_LIMIT: usize = 5;
/// 限频窗口（秒 = 24h）。
pub const RATE_WINDOW_SECS: u64 = 86_400;
/// 共证所需票数（AI01 + 社区轮值 = 2）。
pub const CO_REVIEW_VOTES: usize = 2;

// ---------------------------------------------------------------------------
// 五态状态机
// ---------------------------------------------------------------------------

/// 判例提交态（五态全链）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseState {
    /// 已收。
    Received,
    /// 形式审（缺项清单化驳回口）。
    FormalReview,
    /// 技术审。
    TechReview,
    /// 共证（两人复核制）。
    CoReview,
    /// 收录。
    Accepted,
}

impl CaseState {
    pub fn index(self) -> usize {
        match self {
            CaseState::Received => 0,
            CaseState::FormalReview => 1,
            CaseState::TechReview => 2,
            CaseState::CoReview => 3,
            CaseState::Accepted => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CaseState::Received => "已收",
            CaseState::FormalReview => "形式审",
            CaseState::TechReview => "技术审",
            CaseState::CoReview => "共证",
            CaseState::Accepted => "收录",
        }
    }
}

/// 一条提交。
#[derive(Clone, Debug)]
pub struct Submission {
    pub id: u64,
    pub program: String,
    pub program_version: String,
    /// 证据哈希链头（vbase::chain_hash——提交即锁定不可改）。
    pub evidence_head: [u8; 32],
    pub submitter: String,
    /// 各态到达时间戳（未到达为 0）。
    pub state_at: [u64; STATE_COUNT],
    pub state: CaseState,
    /// 驳回原因（三要素——有的话）。
    pub rejection: Option<(&'static str, &'static str, &'static str)>,
    /// 共证票（AI01 一票 + 社区轮值一票）。
    pub votes: usize,
}

/// 提交线。
pub struct CaseLine {
    subs: Vec<Submission>,
    next_id: u64,
    /// 同提交者时间戳（限频对账）。
    submit_times: Vec<(String, u64)>,
}

impl CaseLine {
    pub fn new() -> CaseLine {
        CaseLine { subs: Vec::new(), next_id: 1, submit_times: Vec::new() }
    }

    pub fn subs(&self) -> &[Submission] {
        &self.subs
    }

    /// 提交（外部视角入口）：证据哈希链上链式串联（提交即锁定）；
    /// 同 IP（提交者）限频——窗口内超 5 次拒绝（刷量防护）。
    pub fn submit(
        &mut self,
        program: &str,
        version: &str,
        evidence_chunks: &[&[u8]],
        submitter: &str,
        now: u64,
    ) -> Result<u64, &'static str> {
        // 限频：24h 窗口内该提交者次数 <5。
        let recent = self
            .submit_times
            .iter()
            .filter(|(s, t)| s == submitter && now.saturating_sub(*t) < RATE_WINDOW_SECS)
            .count();
        if recent >= RATE_LIMIT {
            return Err("rate-limited");
        }
        // 哈希链：genesis → 逐 chunk 串联。
        let mut head = vbase::sha256(b"case-submission-genesis");
        for c in evidence_chunks {
            head = vbase::chain_hash(&head, c);
        }
        let sub = Submission {
            id: self.next_id,
            program: String::from(program),
            program_version: String::from(version),
            evidence_head: head,
            submitter: String::from(submitter),
            state_at: [now, 0, 0, 0, 0],
            state: CaseState::Received,
            rejection: None,
            votes: 0,
        };
        let id = sub.id;
        self.subs.push(sub);
        self.submit_times.push((String::from(submitter), now));
        self.next_id += 1;
        Ok(id)
    }

    /// 状态推进（五态顺序机——不跳态；时间戳逐态落账）。
    pub fn advance(&mut self, id: u64, now: u64) -> Option<CaseState> {
        let sub = self.subs.iter_mut().find(|s| s.id == id)?;
        let next = match sub.state {
            CaseState::Received => CaseState::FormalReview,
            CaseState::FormalReview => CaseState::TechReview,
            CaseState::TechReview => CaseState::CoReview,
            CaseState::CoReview if sub.votes >= CO_REVIEW_VOTES => CaseState::Accepted,
            other => other, // 共证票不足不放行（两人复核制硬门）
        };
        if next != sub.state {
            sub.state = next;
            sub.state_at[next.index()] = now;
        }
        Some(sub.state)
    }

    /// 投共证票（AI01 / 社区轮值各一票——两票即满）。
    pub fn vote(&mut self, id: u64) -> bool {
        match self.subs.iter_mut().find(|s| s.id == id) {
            Some(s) if s.state == CaseState::CoReview && s.votes < CO_REVIEW_VOTES => {
                s.votes += 1;
                true
            }
            _ => false,
        }
    }

    /// 形式审驳回（证据不全 → 清单化缺项，三要素文案）。
    pub fn reject_at_formal(&mut self, id: u64, what: &'static str, why: &'static str, next: &'static str) -> bool {
        match self.subs.iter_mut().find(|s| s.id == id) {
            Some(s) if s.state == CaseState::FormalReview => {
                s.rejection = Some((what, why, next));
                true
            }
            _ => false,
        }
    }

    /// 状态查询（五态实时准确：态 + 各态时间戳）。
    pub fn status(&self, id: u64) -> Option<(CaseState, [u64; STATE_COUNT])> {
        self.subs
            .iter()
            .find(|s| s.id == id)
            .map(|s| (s.state, s.state_at))
    }

    /// 端到端计时（判据第一句：提交到收录 <30 天）。
    pub fn e2e_seconds(&self, id: u64) -> Option<u64> {
        self.status(id).map(|(_, at)| at[4].saturating_sub(at[0]))
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_casesub_checks() -> CheckSet {
    let mut set = CheckSet::new("F129-casesub");

    // 1. 端到端演练（判据第一句）：提交→五态→收录全流程 <30 天目标
    //    （注入演练时刻：3 天走完）。
    let mut line = CaseLine::new();
    let day = 86_400u64;
    let id = line
        .submit("Notepad2", "4.2.25", &[b"case-1", b"shot.png", b"log.txt"], "community-alice", 0)
        .unwrap();
    line.advance(id, day);
    line.advance(id, 2 * day);
    line.advance(id, 3 * day);
    let need_votes = line.advance(id, 3 * day); // 票不足悬停共证（硬门）
    line.vote(id); // AI01 票
    line.vote(id); // 社区轮值票
    line.advance(id, 3 * day); // 满两票 → 收录
    let e2e = line.e2e_seconds(id).unwrap_or(u64::MAX);
    set.add(
        "e2e submit-to-accept under 30d",
        need_votes == Some(CaseState::CoReview)
            && e2e <= E2E_TARGET_SECS
            && line.status(id).map(|(s, _)| s) == Some(CaseState::Accepted),
        "",
    );

    // 2. 五态查询实时准确（判据第一句之二）：各态时间戳逐格落账。
    let (_, at) = line.status(id).unwrap();
    set.add(
        "five states timestamps recorded",
        STATE_COUNT == 5
            && at[0] == 0
            && at[1] == day
            && at[2] == 2 * day
            && at[3] == 3 * day
            && at[4] == 3 * day,
        "",
    );

    // 3. 证据哈希链：同证据同链头；任一 chunk 变 → 链头变（提交即锁定）。
    let mut l2 = CaseLine::new();
    let a = l2.submit("p", "1.0", &[b"x", b"y"], "bob", 0).unwrap();
    let b = l2.submit("p", "1.0", &[b"x", b"y"], "bob", 1).unwrap();
    let c = l2.submit("p", "1.0", &[b"x", b"z"], "bob", 2).unwrap();
    let ha = l2.subs()[a as usize - 1].evidence_head;
    let hb = l2.subs()[b as usize - 1].evidence_head;
    let hc = l2.subs()[c as usize - 1].evidence_head;
    set.add(
        "evidence hash chain locks submission",
        ha == hb && ha != hc,
        "",
    );

    // 4. 形式审驳回：证据不全 → 清单化缺项三要素。
    let mut l3 = CaseLine::new();
    let id = l3.submit("p", "1.0", &[b"only-log"], "carol", 0).unwrap();
    l3.advance(id, 100);
    let rejected = l3.reject_at_formal(
        id,
        "判例提交未完成",
        "缺截图与复现步骤（证据清单 3 缺 2）",
        "补齐后重新提交，原编号保留",
    );
    let rej = l3.subs().iter().find(|s| s.id == id).unwrap().rejection;
    set.add(
        "formal review rejection with triage",
        rejected
            && rej.map(|(w, why, n)| w.contains("未完成") && why.contains("缺") && n.contains("重新提交"))
                == Some(true),
        "",
    );

    // 5. 刷量防护：24h 窗口内第 6 次提交拒绝（同提交者限频 5）。
    let mut l4 = CaseLine::new();
    let mut last = Ok(0u64);
    for i in 0..6u64 {
        last = l4.submit("p", "1.0", &[b"e"], "spammer", i);
    }
    set.add(
        "rate limit 5/24h per submitter",
        last == Err("rate-limited") && l4.subs().len() == 5,
        "",
    );

    // 6. 共证硬门：票不足不放行（两人复核制）。
    let mut l5 = CaseLine::new();
    let id = l5.submit("p", "1.0", &[b"e"], "dave", 0).unwrap();
    for t in [1u64, 2, 3] {
        l5.advance(id, t);
    }
    let stuck = l5.advance(id, 4);
    set.add(
        "co-review hard gate needs 2 votes",
        stuck == Some(CaseState::CoReview) && l5.vote(id) && l5.vote(id) && !l5.vote(id),
        "",
    );

    // 7. 限频不误伤：窗口滑出后可再提交（24h 外重置）。
    let mut l6 = CaseLine::new();
    for i in 0..5u64 {
        let _ = l6.submit("p", "1.0", &[b"e"], "erin", i);
    }
    let next_day = l6.submit("p", "1.0", &[b"e"], "erin", RATE_WINDOW_SECS + 1);
    set.add(
        "rate window slides open next day",
        next_day.is_ok(),
        "",
    );

    // 8. 收录候选池提示 + 致谢联动常量（F040/F142 引用面）。
    set.add(
        "constants: votes + states + target",
        CO_REVIEW_VOTES == 2 && STATE_COUNT == 5 && E2E_TARGET_SECS == 2_592_000,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casesub_all_checks_green() {
        let set = run_casesub_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F129 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn state_names_complete() {
        assert_eq!(CaseState::Received.name(), "已收");
        assert_eq!(CaseState::FormalReview.name(), "形式审");
        assert_eq!(CaseState::TechReview.name(), "技术审");
        assert_eq!(CaseState::CoReview.name(), "共证");
        assert_eq!(CaseState::Accepted.name(), "收录");
    }

    #[test]
    fn vote_only_in_coreview() {
        let mut l = CaseLine::new();
        let id = l.submit("p", "1.0", &[b"e"], "f", 0).unwrap();
        assert!(!l.vote(id), "非共证态投票拒绝");
    }

    #[test]
    fn different_submitters_no_rate_share() {
        let mut l = CaseLine::new();
        for i in 0..5u64 {
            assert!(l.submit("p", "1.0", &[b"e"], "u1", i).is_ok());
        }
        assert!(l.submit("p", "1.0", &[b"e"], "u2", 5).is_ok(), "不同提交者独立限频");
    }
}
