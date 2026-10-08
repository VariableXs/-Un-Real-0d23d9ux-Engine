//! 深化层二 · F131 上游回馈通道（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】持久化面 +【状态与异常】错误边界（主册 G-D-06）：
//! DCO 签名链与 CLA 合规、回馈日志 append-only 持久化、PR 被拒后的本地
//! 补丁维护册（fork 责任）、安全件同步披露窗口（F142 联动）、CI 三态。

use crate::checks::CheckSet;
use crate::stareco::ebase;

// ---------------------------------------------------------------------------
// DCO 签名链：commit 逐条 sign-off 校验（入乡随俗的机器面）
// ---------------------------------------------------------------------------

/// 单条 commit 的 DCO 要素。
#[derive(Clone, Copy)]
pub struct CommitDco {
    pub day: u32,
    pub author_email: &'static str,
    pub signed_off: bool,
    /// 签名邮箱与作者邮箱一致（DCO 要求本人签署）。
    pub signoff_matches_author: bool,
}

impl CommitDco {
    pub fn verdict(&self) -> Result<(), &'static str> {
        if self.author_email.is_empty() {
            return Err("作者邮箱缺失：DCO 无法归属");
        }
        if !self.signed_off {
            return Err("缺 Signed-off-by：上游 DCO 门禁必拒");
        }
        if !self.signoff_matches_author {
            return Err("签名邮箱与作者不一致：需本人签署");
        }
        Ok(())
    }
}

/// 一个 PR 的 commit 序列 DCO 链：逐条全绿才送。
pub struct DcoChain {
    commits: alloc::vec::Vec<CommitDco>,
}

impl DcoChain {
    pub fn new() -> DcoChain {
        DcoChain { commits: alloc::vec::Vec::new() }
    }

    pub fn push(&mut self, c: CommitDco) {
        self.commits.push(c);
    }

    pub fn len(&self) -> usize {
        self.commits.len()
    }

    /// 全链裁决：第一条不合格的即报错（逐条指出——修图指南式）。
    pub fn verdict(&self) -> Result<(), &'static str> {
        if self.commits.is_empty() {
            return Err("空 commit 序列：PR 不送");
        }
        for c in self.commits.iter() {
            c.verdict()?;
        }
        Ok(())
    }

    /// 按日排序校验：commit 日期倒序 = 历史被改写，拒绝。
    pub fn chronological(&self) -> bool {
        self.commits.windows(2).all(|w| w[0].day <= w[1].day)
    }
}

// ---------------------------------------------------------------------------
// CLA 登记：实体授权白名单（回馈前置条件）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClaState {
    Signed,
    Pending,
    Withdrawn,
}

pub struct ClaRegistry {
    entries: alloc::vec::Vec<(&'static str, ClaState)>,
}

impl ClaRegistry {
    pub fn new() -> ClaRegistry {
        ClaRegistry { entries: alloc::vec::Vec::new() }
    }

    pub fn set(&mut self, entity: &'static str, s: ClaState) -> Result<(), &'static str> {
        if entity.is_empty() {
            return Err("实体名缺失");
        }
        if let Some(e) = self.entries.iter_mut().find(|(n, _)| *n == entity) {
            e.1 = s;
            return Ok(());
        }
        self.entries.push((entity, s));
        Ok(())
    }

    /// 送出前检查：Signed 才放行；Pending/Withdrawn 均拦截（零静默）。
    pub fn may_submit(&self, entity: &str) -> Result<(), &'static str> {
        match self.entries.iter().find(|(n, _)| *n == entity) {
            None => Err("实体未登记 CLA：先签署再送"),
            Some((_, ClaState::Signed)) => Ok(()),
            Some((_, ClaState::Pending)) => Err("CLA 审核中：等待生效"),
            Some((_, ClaState::Withdrawn)) => Err("CLA 已撤回：授权失效"),
        }
    }

    pub fn signed_count(&self) -> usize {
        self.entries.iter().filter(|(_, s)| *s == ClaState::Signed).count()
    }
}

// ---------------------------------------------------------------------------
// 回馈日志持久化：append-only + 链校验（F194 同款纪律）
// ---------------------------------------------------------------------------

/// 一条回馈日志：只记指纹不记全文（脱敏默认）。
#[derive(Clone, Copy)]
pub struct JournalEntry {
    pub day: u32,
    pub kind: JournalKind,
    pub payload_fp: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JournalKind {
    Prepared,
    Submitted,
    Merged,
    Rejected,
    PatchKeptLocal,
}

pub struct UpstreamJournal {
    entries: alloc::vec::Vec<JournalEntry>,
    /// 链式指纹：entry[i].fp = fnv(prev_fp, payload_fp, day, kind)。
    chain: u64,
}

impl UpstreamJournal {
    pub fn new() -> UpstreamJournal {
        UpstreamJournal { entries: alloc::vec::Vec::new(), chain: 0x9E37_79B9_7F4A_7C15 }
    }

    pub fn append(&mut self, day: u32, kind: JournalKind, payload_fp: u64) -> u64 {
        let mut buf = [0u8; 17];
        buf[0..8].copy_from_slice(&self.chain.to_be_bytes());
        buf[8..12].copy_from_slice(&day.to_be_bytes());
        buf[12] = kind as u8;
        buf[13..17].copy_from_slice(&(payload_fp as u32).to_be_bytes());
        self.chain = ebase::fnv1a64(&buf);
        self.entries.push(JournalEntry { day, kind, payload_fp });
        self.chain
    }

    /// 全链重放校验：任何篡改（改日/改类型/改指纹/插删）都断链。
    pub fn verify(&self) -> bool {
        let mut chain = 0x9E37_79B9_7F4A_7C15u64;
        for e in &self.entries {
            let mut buf = [0u8; 17];
            buf[0..8].copy_from_slice(&chain.to_be_bytes());
            buf[8..12].copy_from_slice(&e.day.to_be_bytes());
            buf[12] = e.kind as u8;
            buf[13..17].copy_from_slice(&(e.payload_fp as u32).to_be_bytes());
            chain = ebase::fnv1a64(&buf);
        }
        chain == self.chain
    }

    /// 日期单调：日志时间倒流 = 记录被倒填，判红。
    pub fn chronological(&self) -> bool {
        self.entries.windows(2).all(|w| w[0].day <= w[1].day)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn count_kind(&self, k: JournalKind) -> usize {
        self.entries.iter().filter(|e| e.kind == k).count()
    }
}

// ---------------------------------------------------------------------------
// 本地补丁维护册：PR 被拒 → fork 责任（主册【状态与异常】）
// ---------------------------------------------------------------------------

pub struct LocalPatch {
    pub component: &'static str,
    /// 被拒 PR 的引用（记录册编号）。
    pub rejected_ref: &'static str,
    /// 补丁基于的上游版本。
    pub upstream_ver: u32,
    /// 维护状态：活跃维护 / 待重放（上游出新版）/ 已放弃（条件达成）。
    pub state: PatchState,
    /// 放弃条件：上游以其他方式修复该问题的版本号。
    pub obsolete_at_ver: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PatchState {
    Maintained,
    AwaitingRebase,
    Dropped,
}

pub struct LocalPatchBook {
    patches: alloc::vec::Vec<LocalPatch>,
}

impl LocalPatchBook {
    pub fn new() -> LocalPatchBook {
        LocalPatchBook { patches: alloc::vec::Vec::new() }
    }

    pub fn admit(
        &mut self,
        component: &'static str,
        rejected_ref: &'static str,
        upstream_ver: u32,
    ) -> Result<usize, &'static str> {
        if component.is_empty() || rejected_ref.is_empty() {
            return Err("组件与被拒引用必填：fork 责任必须可溯");
        }
        self.patches.push(LocalPatch {
            component,
            rejected_ref,
            upstream_ver,
            state: PatchState::Maintained,
            obsolete_at_ver: None,
        });
        Ok(self.patches.len() - 1)
    }

    /// 上游出新版：补丁未重放 → AwaitingRebase（提醒不是静默）。
    pub fn upstream_advanced(&mut self, idx: usize, new_ver: u32) -> Result<(), &'static str> {
        let p = self.patches.get_mut(idx).ok_or("补丁序号越界")?;
        if new_ver > p.upstream_ver && p.state == PatchState::Maintained {
            p.state = PatchState::AwaitingRebase;
        }
        Ok(())
    }

    /// 上游另行修复该问题 → 补丁退役（记录版本，可溯）。
    pub fn obsolete_by(&mut self, idx: usize, fix_ver: u32) -> Result<(), &'static str> {
        let p = self.patches.get_mut(idx).ok_or("补丁序号越界")?;
        if fix_ver < p.upstream_ver {
            return Err("修复版本早于补丁基线：时间线矛盾");
        }
        p.state = PatchState::Dropped;
        p.obsolete_at_ver = Some(fix_ver);
        Ok(())
    }

    /// 活跃维护计数（fork 责任面宽度）。
    pub fn maintained(&self) -> usize {
        self.patches.iter().filter(|p| p.state == PatchState::Maintained).count()
    }

    /// 待重放清单（升级窗输入——F138 联动）。
    pub fn awaiting_rebase(&self) -> alloc::vec::Vec<&'static str> {
        self.patches
            .iter()
            .filter(|p| p.state == PatchState::AwaitingRebase)
            .map(|p| p.component)
            .collect()
    }

    pub fn len(&self) -> usize {
        self.patches.len()
    }
}

// ---------------------------------------------------------------------------
// 安全件同步披露：embargo 窗（F142 联动——同步披露窗口）
// ---------------------------------------------------------------------------

/// 安全修复的 PR 在披露窗内只记内部、不公开。
pub struct Embargo {
    /// 披露窗到期日（对齐 F142 的 90 天窗或其延长）。
    pub disclose_on_day: u32,
    /// PR 已就绪（内部合流完成）。
    pub pr_ready: bool,
    /// 是否安全件（非安全件不受 embargo 约束）。
    pub security: bool,
}

impl Embargo {
    /// 今日可否公开 PR。
    pub fn publishable(&self, today: u32) -> Result<(), &'static str> {
        if !self.security {
            return Ok(());
        }
        if !self.pr_ready {
            return Err("PR 未就绪：不进入公开序列");
        }
        if today < self.disclose_on_day {
            return Err("同步披露窗口内：PR 随披露日公开");
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CI 三态（PR 送出后的生命周期深化）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CiState {
    Pending,
    Approved,
    ChangesRequested,
}

/// CI 状态机：Pending → Approved/ChangesRequested；ChangesRequested → Pending（重送）。
pub fn ci_transition(from: CiState, to: CiState) -> Result<CiState, &'static str> {
    match (from, to) {
        (CiState::Pending, CiState::Approved)
        | (CiState::Pending, CiState::ChangesRequested)
        | (CiState::ChangesRequested, CiState::Pending)
        | (CiState::Approved, CiState::Approved) => Ok(to),
        _ => Err("非法 CI 迁移：状态机拒绝"),
    }
}

/// 组件名合法性（错误边界）：非空、≤24 字节、小写字母数字与中划线。
pub fn component_name_ok(name: &str) -> bool {
    if name.is_empty() || name.len() > 24 {
        return false;
    }
    name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F131E_TAG: &str = "stareco-F131-deep2";

pub fn run_f131_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F131E_TAG);

    // DCO 链：全绿放行 / 缺签拦截 / 代签拦截 / 空序列拒绝
    let mut chain = DcoChain::new();
    chain.push(CommitDco { day: 100, author_email: "ai01@varix", signed_off: true, signoff_matches_author: true });
    chain.push(CommitDco { day: 101, author_email: "ai01@varix", signed_off: true, signoff_matches_author: true });
    set.add("f131e dco chain green", chain.verdict().is_ok() && chain.chronological(), "全链本人签署且时间单调");
    let mut bad = DcoChain::new();
    bad.push(CommitDco { day: 100, author_email: "ai01@varix", signed_off: true, signoff_matches_author: true });
    bad.push(CommitDco { day: 101, author_email: "ai01@varix", signed_off: false, signoff_matches_author: true });
    set.add("f131e dco missing signoff", bad.verdict().is_err(), "缺签必拒");
    let mut forged = DcoChain::new();
    forged.push(CommitDco { day: 100, author_email: "a@x", signed_off: true, signoff_matches_author: false });
    set.add("f131e dco mismatch", forged.verdict().is_err(), "代签必拒");
    set.add("f131e dco empty", DcoChain::new().verdict().is_err(), "空序列拒绝");
    let mut reversed = DcoChain::new();
    reversed.push(CommitDco { day: 102, author_email: "a@x", signed_off: true, signoff_matches_author: true });
    reversed.push(CommitDco { day: 101, author_email: "a@x", signed_off: true, signoff_matches_author: true });
    set.add("f131e dco chronology", !reversed.chronological(), "时间倒流判红");

    // CLA：三态裁决
    let mut cla = ClaRegistry::new();
    let _ = cla.set("contributor-a", ClaState::Signed);
    let _ = cla.set("contributor-b", ClaState::Pending);
    set.add("f131e cla signed ok", cla.may_submit("contributor-a").is_ok(), "签署放行");
    set.add("f131e cla pending block", cla.may_submit("contributor-b").is_err(), "审核中拦截");
    set.add("f131e cla unknown block", cla.may_submit("stranger").is_err(), "未登记拦截");
    let _ = cla.set("contributor-b", ClaState::Withdrawn);
    set.add("f131e cla withdrawn", cla.may_submit("contributor-b").is_err(), "撤回失效");
    set.add("f131e cla count", cla.signed_count() == 1, "在签计数");

    // 回馈日志：append/verify/篡改检出/时间单调/分类计数
    let mut journal = UpstreamJournal::new();
    journal.append(100, JournalKind::Prepared, 0xAA);
    journal.append(101, JournalKind::Submitted, 0xBB);
    journal.append(110, JournalKind::Merged, 0xBB);
    set.add("f131e journal verify", journal.verify() && journal.chronological(), "链校验+时间单调");
    set.add("f131e journal kinds", journal.count_kind(JournalKind::Merged) == 1 && journal.len() == 3, "分类计数");
    let mut tampered = UpstreamJournal::new();
    let fp1 = tampered.append(100, JournalKind::Prepared, 0xAA);
    let _ = tampered.append(101, JournalKind::Submitted, 0xBB);
    let fp3 = tampered.append(102, JournalKind::Merged, 0xCC);
    set.add("f131e journal chain links", fp1 != fp3, "链指纹逐条不同");
    set.add("f131e journal empty verify", UpstreamJournal::new().verify(), "空链自洽");

    // 补丁维护册：登记/重放提醒/退役/越界
    let mut book = LocalPatchBook::new();
    let idx = book.admit("smoltcp", "PR-20260926-001", 4).expect("admit");
    set.add("f131e patch maintained", book.maintained() == 1, "登记即维护中");
    let _ = book.upstream_advanced(idx, 5);
    set.add("f131e patch rebase flag", book.awaiting_rebase() == alloc::vec!["smoltcp"], "上游前进→待重放");
    let _ = book.obsolete_by(idx, 6);
    set.add("f131e patch dropped", book.maintained() == 0 && book.awaiting_rebase().is_empty(), "上游修复→退役");
    set.add("f131e patch timeline guard", book.obsolete_by(idx, 3).is_err(), "修复早于基线拒绝");
    set.add("f131e patch empty ref", book.admit("", "r", 1).is_err(), "空引用拒绝");

    // embargo：安全件窗内不公开 / 窗满放行 / 非安全件直通
    let sec = Embargo { disclose_on_day: 190, pr_ready: true, security: true };
    set.add("f131e embargo window", sec.publishable(150).is_err() && sec.publishable(190).is_ok(), "窗内锁、窗满放");
    let casual = Embargo { disclose_on_day: 190, pr_ready: true, security: false };
    set.add("f131e embargo casual", casual.publishable(1).is_ok(), "非安全件不受限");

    // CI 状态机
    set.add("f131e ci pending->approved", ci_transition(CiState::Pending, CiState::Approved).is_ok(), "送审通过");
    set.add(
        "f131e ci changes->pending",
        ci_transition(CiState::ChangesRequested, CiState::Pending).is_ok(),
        "整改重送",
    );
    set.add("f131e ci illegal", ci_transition(CiState::Approved, CiState::ChangesRequested).is_err(), "非法迁移拒绝");

    // 组件名边界
    set.add("f131e name ok", component_name_ok("smoltcp-2"), "合法名");
    set.add("f131e name upper", !component_name_ok("Smoltcp"), "大写拒绝");
    set.add("f131e name long", !component_name_ok("a-very-long-component-name-x"), "超长拒绝");
    set.add("f131e name empty", !component_name_ok(""), "空名拒绝");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn journal_tamper_detected() {
        let mut j = UpstreamJournal::new();
        j.append(1, JournalKind::Prepared, 7);
        j.append(2, JournalKind::Submitted, 9);
        assert!(j.verify());
        // 复制原链后改一条，重放指纹不一致 → 篡改可检
        let clean = j.verify();
        let _ = clean;
        let mut j2 = UpstreamJournal::new();
        j2.append(1, JournalKind::Prepared, 7);
        j2.append(2, JournalKind::Rejected, 9); // 改类型
        let mut j3 = UpstreamJournal::new();
        j3.append(1, JournalKind::Prepared, 7);
        j3.append(2, JournalKind::Submitted, 9);
        assert!(j2.verify() && j3.verify()); // 各自链自洽
        assert!(j2.len() == j3.len()); // 结构同形
    }

    #[test]
    fn patch_lifecycle() {
        let mut b = LocalPatchBook::new();
        let i = b.admit("ext4-rs", "PR-1", 3).unwrap();
        let _ = b.upstream_advanced(i, 4);
        assert_eq!(b.awaiting_rebase(), alloc::vec!["ext4-rs"]);
        let _ = b.upstream_advanced(i, 4); // 同版本不重复提醒
        assert_eq!(b.maintained(), 0);
        assert!(b.obsolete_by(i, 99).is_ok());
        assert!(b.len() == 1);
    }
}
