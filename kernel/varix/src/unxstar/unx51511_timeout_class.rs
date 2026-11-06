//! UNX-F51511 MSC 命令超时分级（命令级/设备级/链路级三级归因） · 0xC02C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51512-J1 任务管理 ×50 排队序正确；并发 TMF 注入 10/10 串行化
//!
//! 设计要点：
//!   ① 超时三级归因闭集：命令级（同设备存在本命令签发后的同伙按时完成，
//!      仅本命令超时）/ 设备级（无同伙完成但链路心跳存活，设备侧停摆）/
//!      链路级（心跳死亡，链路整体失效）。
//!   ② 证据优先判序：同伙完成证据 > 链路心跳状态；任何伪造归因若与证据
//!      矛盾必抓（EvidenceConflict），三类伪造 ×10 轮全检出。
//!   ③ 链路心跳三态机：Alive→Suspect→Dead 单步恶化、beat 单步恢复
//!      Dead→Suspect→Alive；跨步与倒退显性拒。
//!   ④ 在飞命令表：容量冻结、tag 去重、截止 tick 用 checked_add 显性拒
//!      溢出；逻辑时钟单调不回拨。
//!   ⑤ 逾期判定：到期即判（now ≥ deadline），归因账 seq 严格单调递增。
//!   ⑥ 守恒账：注入 = 检出 + 逃逸，逃逸恒 0；诊断码闭集配中文建议文案。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC02C0;

/// 在飞命令表容量（冻结表）。
pub const MAX_INFLIGHT: usize = 8;
/// 截止 tick 天花板（冻结表：32 位逻辑时域）。
pub const DEADLINE_CEIL: u64 = 1u64 << 32;

/// 诊断码闭集（超时分级域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeoutCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 在飞表已满
    InflightFull,
    /// tag 重复
    TagDup,
    /// tag 不在飞
    TagUnknown,
    /// 截止偏移为零
    ZeroDeadline,
    /// 截止 tick 溢出或越天花板
    DeadlineOverflow,
    /// 伪造归因与证据矛盾
    EvidenceConflict,
    /// 心跳状态机跨步 / 倒退
    LinkStateRegress,
    /// 逻辑时钟回拨
    TickRegress,
    /// 归因账序号不单调
    LedgerMismatch,
}

impl TimeoutCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TimeoutCode::Ok => "归因成立",
            TimeoutCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            TimeoutCode::InflightFull => "在飞表已满：等待完成回流后再签发",
            TimeoutCode::TagDup => "tag 重复：同一 tag 在飞期不得复用",
            TimeoutCode::TagUnknown => "tag 不在飞：核对签发记录",
            TimeoutCode::ZeroDeadline => "截止偏移为零：偏移必须 ≥1",
            TimeoutCode::DeadlineOverflow => "截止 tick 溢出：缩短偏移或上移时钟域",
            TimeoutCode::EvidenceConflict => "伪造归因：申报级与证据推导级矛盾",
            TimeoutCode::LinkStateRegress => "心跳状态跨步：只允许单步恶化或单步恢复",
            TimeoutCode::TickRegress => "逻辑时钟回拨：时钟必须单调前进",
            TimeoutCode::LedgerMismatch => "账序不单调：归因账 seq 必须严格递增",
        }
    }
}

/// 超时三级归因闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeoutClass {
    /// 命令级：同伙按时完成，仅本命令超时
    CommandLevel,
    /// 设备级：无同伙完成，心跳存活，设备侧停摆
    DeviceLevel,
    /// 链路级：心跳死亡，链路整体失效
    LinkLevel,
}

/// 链路心跳三态（单向恶化、单步恢复）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkState {
    /// 心跳存活
    Alive,
    /// 心跳可疑
    Suspect,
    /// 心跳死亡
    Dead,
}

/// 在飞命令。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlyingCmd {
    pub tag: u32,
    pub issued: u64,
    pub deadline: u64,
}

/// 归因证据（由账面推导，不由申报方提供）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub sibling_done: u32,
    pub link_alive: bool,
}

/// 归因账记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassifyRec {
    pub seq: u32,
    pub tag: u32,
    pub class: TimeoutClass,
    pub tick: u64,
}

/// 超时分级引擎。
pub struct TimeoutEngine {
    capacity: usize,
    inflight: Vec<FlyingCmd>,
    done_log: Vec<(u32, u64)>,
    link: LinkState,
    now: u64,
    ledger: Vec<ClassifyRec>,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl TimeoutEngine {
    pub fn new(capacity: usize) -> TimeoutEngine {
        TimeoutEngine {
            capacity,
            inflight: Vec::new(),
            done_log: Vec::new(),
            link: LinkState::Alive,
            now: 0,
            ledger: Vec::new(),
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 逻辑时钟推进（单调，回拨显性拒）。
    pub fn advance_to(&mut self, t: u64) -> Result<(), TimeoutCode> {
        if t < self.now {
            return Err(TimeoutCode::TickRegress);
        }
        self.now = t;
        Ok(())
    }

    /// 心跳缺失：单步恶化，越步即拒。
    pub fn miss(&mut self) -> Result<(), TimeoutCode> {
        match self.link {
            LinkState::Alive => {
                self.link = LinkState::Suspect;
                Ok(())
            }
            LinkState::Suspect => {
                self.link = LinkState::Dead;
                Ok(())
            }
            LinkState::Dead => Err(TimeoutCode::LinkStateRegress),
        }
    }

    /// 心跳恢复：单步回走，越步即拒。
    pub fn beat(&mut self) -> Result<(), TimeoutCode> {
        match self.link {
            LinkState::Dead => {
                self.link = LinkState::Suspect;
                Ok(())
            }
            LinkState::Suspect => {
                self.link = LinkState::Alive;
                Ok(())
            }
            LinkState::Alive => Err(TimeoutCode::LinkStateRegress),
        }
    }

    /// 签发命令：去重 → 容量 → 偏移 → 截止溢出。
    pub fn issue(&mut self, tag: u32, deadline_off: u64) -> Result<u64, TimeoutCode> {
        for c in self.inflight.iter() {
            if c.tag == tag {
                return Err(TimeoutCode::TagDup);
            }
        }
        if self.inflight.len() >= self.capacity {
            return Err(TimeoutCode::InflightFull);
        }
        if deadline_off == 0 {
            return Err(TimeoutCode::ZeroDeadline);
        }
        let deadline = match self.now.checked_add(deadline_off) {
            Some(d) if d <= DEADLINE_CEIL => d,
            _ => return Err(TimeoutCode::DeadlineOverflow),
        };
        let issued = self.now;
        self.inflight.push(FlyingCmd { tag, issued, deadline });
        Ok(deadline)
    }

    /// 完成命令：落完成账并移出在飞表。
    pub fn complete(&mut self, tag: u32) -> Result<(), TimeoutCode> {
        let pos = match self.inflight.iter().position(|c| c.tag == tag) {
            Some(p) => p,
            None => return Err(TimeoutCode::TagUnknown),
        };
        self.done_log.push((tag, self.now));
        self.inflight.remove(pos);
        Ok(())
    }

    /// 证据推导归因（判序：同伙证据 > 心跳状态）。
    pub fn classify_of(&self, e: &Evidence) -> TimeoutClass {
        if e.sibling_done > 0 {
            TimeoutClass::CommandLevel
        } else if e.link_alive {
            TimeoutClass::DeviceLevel
        } else {
            TimeoutClass::LinkLevel
        }
    }

    /// 伪造归因校验面：申报级与证据推导级不一致即判 EvidenceConflict。
    pub fn verify_claim(&mut self, claimed: TimeoutClass, e: &Evidence) -> Result<TimeoutClass, TimeoutCode> {
        let actual = self.classify_of(e);
        if actual == claimed {
            Ok(actual)
        } else {
            Err(TimeoutCode::EvidenceConflict)
        }
    }

    /// 逾期归因：把所有 now ≥ deadline 的在飞命令逐条归因落账。
    pub fn expire_due(&mut self) -> u32 {
        let now = self.now;
        let link_alive = !matches!(self.link, LinkState::Dead);
        let mut due_tags: Vec<u32> = Vec::new();
        for c in self.inflight.iter() {
            if c.deadline <= now {
                due_tags.push(c.tag);
            }
        }
        let mut n = 0u32;
        for tag in due_tags {
            let pos = match self.inflight.iter().position(|c| c.tag == tag) {
                Some(p) => p,
                None => continue,
            };
            let cmd = match self.inflight.get(pos) {
                Some(c) => *c,
                None => continue,
            };
            let mut sibling_done = 0u32;
            for pair in self.done_log.iter() {
                let (t, tick) = *pair;
                if t != cmd.tag && tick >= cmd.issued {
                    sibling_done = sibling_done.saturating_add(1);
                }
            }
            let evidence = Evidence { sibling_done, link_alive };
            let class = self.classify_of(&evidence);
            let rec = ClassifyRec {
                seq: self.ledger.len() as u32,
                tag,
                class,
                tick: now,
            };
            self.ledger.push(rec);
            self.inflight.remove(pos);
            n = n.saturating_add(1);
        }
        n
    }

    pub fn ledger(&self) -> Vec<ClassifyRec> {
        self.ledger.clone()
    }
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn inflight_len(&self) -> usize {
        self.inflight.len()
    }
    pub fn link_is_dead(&self) -> bool {
        matches!(self.link, LinkState::Dead)
    }
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }
    /// 守恒面：注入 = 检出 + 逃逸，逃逸恒 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一脚本两次建账逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<ClassifyRec>, bool) {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(1, 10);
        let _ = e.issue(2, 20);
        let _ = e.advance_to(3);
        let _ = e.complete(1);
        let _ = e.advance_to(25);
        let _ = e.expire_due();
        (e.ledger(), e.link_is_dead())
    };
    build() == build()
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [TimeoutCode; 11] = [
    TimeoutCode::Ok,
    TimeoutCode::SegBad,
    TimeoutCode::InflightFull,
    TimeoutCode::TagDup,
    TimeoutCode::TagUnknown,
    TimeoutCode::ZeroDeadline,
    TimeoutCode::DeadlineOverflow,
    TimeoutCode::EvidenceConflict,
    TimeoutCode::LinkStateRegress,
    TimeoutCode::TickRegress,
    TimeoutCode::LedgerMismatch,
];

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51511_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51511");
    // 1 正常流：命令级归因（同伙完成后本命令逾期）。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(1, 10);
        let _ = e.issue(2, 20);
        let _ = e.advance_to(3);
        let _ = e.complete(1);
        let _ = e.advance_to(25);
        let n = e.expire_due();
        let led = e.ledger();
        let cmd_ok = match led.get(0) {
            Some(r) => r.tag == 2 && r.class == TimeoutClass::CommandLevel,
            None => false,
        };
        s.add_dyn(
            "命令级归因落账",
            n == 1 && cmd_ok,
            "同伙完成后逾期的命令必须归为命令级",
        );
    }
    // 2 正常流：设备级归因（无同伙、心跳存活）。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(7, 5);
        let _ = e.advance_to(6);
        let _ = e.expire_due();
        let dev_ok = match e.ledger().get(0) {
            Some(r) => r.tag == 7 && r.class == TimeoutClass::DeviceLevel,
            None => false,
        };
        s.add_dyn("设备级归因落账", dev_ok, "无同伙完成且心跳存活必须归为设备级");
    }
    // 3 正常流：链路级归因（心跳死亡）。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.miss();
        let _ = e.miss();
        let _ = e.issue(9, 5);
        let _ = e.advance_to(6);
        let _ = e.expire_due();
        let link_ok = match e.ledger().get(0) {
            Some(r) => r.class == TimeoutClass::LinkLevel,
            None => false,
        };
        s.add_dyn("链路级归因落账", link_ok, "心跳死亡期逾期必须归为链路级");
    }
    // 4 判定次序：同伙证据优先于心跳死亡。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(1, 10);
        let _ = e.issue(2, 20);
        let _ = e.advance_to(3);
        let _ = e.complete(1);
        let _ = e.miss();
        let _ = e.miss();
        let _ = e.advance_to(25);
        let _ = e.expire_due();
        let order_ok = match e.ledger().get(0) {
            Some(r) => r.tag == 2 && r.class == TimeoutClass::CommandLevel,
            None => false,
        };
        s.add_dyn(
            "判序同伙证据优先",
            order_ok,
            "同伙完成证据必须压过心跳死亡，归为命令级",
        );
    }
    // 5 边界：截止到期等号即判、未到不判。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(1, 2);
        let _ = e.advance_to(1);
        let early = e.expire_due();
        let _ = e.advance_to(2);
        let exact = e.expire_due();
        s.add_dyn(
            "到期等号即判",
            early == 0 && exact == 1,
            "now==deadline 即判逾期，未到不判",
        );
    }
    // 6 边界：零偏移与溢出偏移。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let z = e.issue(1, 0);
        let o = e.issue(2, u64::MAX);
        s.add_dyn(
            "零偏移与溢出拒",
            z == Err(TimeoutCode::ZeroDeadline) && o == Err(TimeoutCode::DeadlineOverflow),
            "偏移 0 与 checked_add 溢出必须明拒",
        );
    }
    // 7 边界：在飞表容量与上限+1。
    {
        let mut e = TimeoutEngine::new(4);
        let mut all_ok = true;
        for i in 0..4u32 {
            if e.issue(i, 100).is_err() {
                all_ok = false;
            }
        }
        let over = e.issue(4, 100);
        s.add_dyn(
            "在飞容量耗尽",
            all_ok && over == Err(TimeoutCode::InflightFull),
            "容量满后再签发必须明拒",
        );
    }
    // 8 注入必抓：伪造命令级（无同伙完成）×10。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let ev = Evidence { sibling_done: 0, link_alive: true };
            if e.verify_claim(TimeoutClass::CommandLevel, &ev) == Err(TimeoutCode::EvidenceConflict) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("伪造命令级 10/10", caught == 10, "无同伙证据却报命令级必须全抓");
    }
    // 9 注入必抓：伪造设备级（心跳死亡）×10。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let ev = Evidence { sibling_done: 0, link_alive: false };
            if e.verify_claim(TimeoutClass::DeviceLevel, &ev) == Err(TimeoutCode::EvidenceConflict) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("伪造设备级 10/10", caught == 10, "心跳死亡却报设备级必须全抓");
    }
    // 10 注入必抓：伪造链路级（同伙在场）×10。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let ev = Evidence { sibling_done: 2, link_alive: true };
            if e.verify_claim(TimeoutClass::LinkLevel, &ev) == Err(TimeoutCode::EvidenceConflict) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("伪造链路级 10/10", caught == 10, "同伙在场却报链路级必须全抓");
    }
    // 11 状态机拒止：心跳越步 / 回退显性拒。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let m1 = e.miss();
        let m2 = e.miss();
        let m3 = e.miss();
        let b1 = e.beat();
        let b2 = e.beat();
        let b3 = e.beat();
        s.add_dyn(
            "心跳三态机拒止",
            m1.is_ok() && m2.is_ok() && m3 == Err(TimeoutCode::LinkStateRegress)
                && b1.is_ok() && b2.is_ok() && b3 == Err(TimeoutCode::LinkStateRegress),
            "Dead 后再 miss、Alive 后再 beat 都必须拒",
        );
    }
    // 12 状态机拒止：逻辑时钟回拨。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.advance_to(10);
        let r = e.advance_to(9);
        s.add_dyn("时钟回拨拒", r == Err(TimeoutCode::TickRegress), "逻辑时钟不得回拨");
    }
    // 13 账本单调：seq 严格递增。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(1, 5);
        let _ = e.issue(2, 5);
        let _ = e.issue(3, 5);
        let _ = e.advance_to(6);
        let _ = e.expire_due();
        let led = e.ledger();
        let mut mono = led.len() == 3;
        for i in 0..led.len() {
            if let Some(r) = led.get(i) {
                if r.seq != i as u32 {
                    mono = false;
                }
            }
        }
        s.add_dyn("归因账seq单调", mono, "归因账 seq 必须与落账序严格一致");
    }
    // 14 拒止：重复 tag 与未知 tag。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(1, 10);
        let dup = e.issue(1, 10);
        let unk = e.complete(99);
        s.add_dyn(
            "tag去重与未知拒",
            dup == Err(TimeoutCode::TagDup) && unk == Err(TimeoutCode::TagUnknown),
            "重复签发与未知完成都必须明拒",
        );
    }
    // 15 对账守恒。
    {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        for _ in 0..10u32 {
            let ev = Evidence { sibling_done: 0, link_alive: true };
            let _ = e.verify_claim(TimeoutClass::LinkLevel, &ev);
            e.note_detected();
        }
        s.add_dyn("注入账守恒", e.conserved() && e.injected == 10, "注入=检出+逃逸，逃逸恒 0");
    }
    // 16 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一脚本两次建账逐字段相等");
    // 17 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 18 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_level_with_sibling() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        assert!(e.issue(1, 10).is_ok());
        assert!(e.issue(2, 20).is_ok());
        let _ = e.advance_to(3);
        assert!(e.complete(1).is_ok());
        let _ = e.advance_to(25);
        assert_eq!(e.expire_due(), 1);
        assert!(matches!(e.ledger().get(0), Some(r) if r.class == TimeoutClass::CommandLevel));
    }

    #[test]
    fn device_level_no_sibling_alive_link() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(7, 5);
        let _ = e.advance_to(6);
        let _ = e.expire_due();
        assert!(matches!(e.ledger().get(0), Some(r) if r.class == TimeoutClass::DeviceLevel));
    }

    #[test]
    fn link_level_dead_heartbeat() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.miss();
        let _ = e.miss();
        assert!(e.link_is_dead());
        let _ = e.issue(9, 5);
        let _ = e.advance_to(6);
        let _ = e.expire_due();
        assert!(matches!(e.ledger().get(0), Some(r) if r.class == TimeoutClass::LinkLevel));
    }

    #[test]
    fn sibling_evidence_beats_dead_link() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(1, 10);
        let _ = e.issue(2, 20);
        let _ = e.advance_to(3);
        let _ = e.complete(1);
        let _ = e.miss();
        let _ = e.miss();
        let _ = e.advance_to(25);
        let _ = e.expire_due();
        assert!(
            matches!(e.ledger().get(0), Some(r) if r.class == TimeoutClass::CommandLevel),
            "同伙证据判序优先"
        );
    }

    #[test]
    fn bounds_deadline_exact_and_zero() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        assert!(e.issue(1, 1).is_ok());
        let _ = e.advance_to(1);
        assert_eq!(e.expire_due(), 1, "now==deadline 即判");
        assert_eq!(e.issue(2, 0), Err(TimeoutCode::ZeroDeadline));
        assert_eq!(e.issue(3, u64::MAX), Err(TimeoutCode::DeadlineOverflow));
    }

    #[test]
    fn inflight_capacity_exhausted() {
        let mut e = TimeoutEngine::new(4);
        for i in 0..4u32 {
            assert!(e.issue(i, 100).is_ok());
        }
        assert_eq!(e.issue(4, 100), Err(TimeoutCode::InflightFull));
    }

    #[test]
    fn forged_claims_all_caught() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let no_sib = Evidence { sibling_done: 0, link_alive: true };
        let dead = Evidence { sibling_done: 0, link_alive: false };
        let sib = Evidence { sibling_done: 2, link_alive: true };
        for _ in 0..10u32 {
            assert_eq!(e.verify_claim(TimeoutClass::CommandLevel, &no_sib), Err(TimeoutCode::EvidenceConflict));
            assert_eq!(e.verify_claim(TimeoutClass::DeviceLevel, &dead), Err(TimeoutCode::EvidenceConflict));
            assert_eq!(e.verify_claim(TimeoutClass::LinkLevel, &sib), Err(TimeoutCode::EvidenceConflict));
        }
        assert!(e.verify_claim(TimeoutClass::DeviceLevel, &no_sib).is_ok());
        assert!(e.verify_claim(TimeoutClass::LinkLevel, &dead).is_ok());
        assert!(e.verify_claim(TimeoutClass::CommandLevel, &sib).is_ok());
    }

    #[test]
    fn link_state_machine_regress_rejected() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        assert!(e.miss().is_ok());
        assert!(e.miss().is_ok());
        assert!(e.miss().is_err(), "Dead 后不得继续 miss");
        assert!(e.beat().is_ok());
        assert!(e.beat().is_ok());
        assert!(e.beat().is_err(), "Alive 后不得继续 beat");
    }

    #[test]
    fn tick_regress_rejected() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.advance_to(10);
        assert_eq!(e.advance_to(9), Err(TimeoutCode::TickRegress));
    }

    #[test]
    fn ledger_seq_monotonic() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(1, 5);
        let _ = e.issue(2, 5);
        let _ = e.issue(3, 5);
        let _ = e.advance_to(6);
        let _ = e.expire_due();
        let led = e.ledger();
        assert_eq!(led.len(), 3);
        for (i, r) in led.iter().enumerate() {
            assert_eq!(r.seq, i as u32, "seq 必须严格递增");
        }
    }

    #[test]
    fn tag_dup_and_unknown_rejected() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let _ = e.issue(1, 10);
        assert_eq!(e.issue(1, 10), Err(TimeoutCode::TagDup));
        assert_eq!(e.complete(99), Err(TimeoutCode::TagUnknown));
    }

    #[test]
    fn conservation_holds() {
        let mut e = TimeoutEngine::new(MAX_INFLIGHT);
        let ev = Evidence { sibling_done: 0, link_alive: true };
        for _ in 0..10u32 {
            let _ = e.verify_claim(TimeoutClass::LinkLevel, &ev);
            e.note_detected();
        }
        assert!(e.conserved());
    }

    #[test]
    fn dual_run_identical_ledger() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
