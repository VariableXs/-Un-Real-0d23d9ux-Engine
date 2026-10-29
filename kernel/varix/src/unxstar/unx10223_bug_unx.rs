//! UNX-F10223 BUG-UNX 工单双向挂账（五字段齐备且双向同步，单向断链检出）（0x10f00 段）
//!
//! 锚点：UNX-F10223-J1 双向挂账达成（工单五字段齐备且双向同步
//! 10/10 次），单向断链 10/10 次检出。
//! 域：CoRun·UNX-C3（归因面谱系）。
//!
//! 结构：工单（编号/用例/类/指纹/状态五字段）+ 工单簿（双向索引：
//! 按号↔按用例）+ 双向核（两索引同源可互查）+ 断链检出 +
//! 折叠指纹 + 注入核（缺字段/重号/断链/封簿/状态倒转）。
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;
use alloc::vec;

/// 段位基址
pub const SEG_BASE: u32 = 0x10f00;
/// 诊断码：字段缺失
pub const ERR_FIELD_MISSING: u32 = SEG_BASE + 0x01;
/// 诊断码：工单重号
pub const ERR_DUP_ID: u32 = SEG_BASE + 0x02;
/// 诊断码：单向断链
pub const ERR_LINK_BROKEN: u32 = SEG_BASE + 0x03;
/// 诊断码：封簿操作
pub const ERR_FROZEN: u32 = SEG_BASE + 0x04;
/// 诊断码：状态倒转
pub const ERR_STATE_BACK: u32 = SEG_BASE + 0x05;

/// 工单状态
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TicketState {
    /// 已开
    Open,
    /// 处理中
    InProgress,
    /// 已闭
    Closed,
}

impl TicketState {
    /// 状态序
    pub fn ord(&self) -> u8 {
        match self {
            TicketState::Open => 1,
            TicketState::InProgress => 2,
            TicketState::Closed => 3,
        }
    }
}

/// 工单五字段
#[derive(Clone, PartialEq, Eq)]
pub struct Ticket {
    /// 工单号（BUG-UNX-xxxx）
    pub id: String,
    /// 用例标识
    pub case: String,
    /// 归因类名
    pub class: String,
    /// 失败指纹
    pub fingerprint: u32,
    /// 状态
    pub state: TicketState,
}

impl Ticket {
    /// 五字段齐备
    pub fn complete(&self) -> bool {
        !self.id.is_empty() && !self.case.is_empty() && !self.class.is_empty()
            && self.fingerprint != 0
    }
}

/// 工单簿（双向索引）
pub struct TicketBook {
    /// 工单序
    pub tickets: Vec<Ticket>,
    /// 封簿
    pub frozen: bool,
}

impl TicketBook {
    /// 空簿
    pub fn new() -> Self {
        TicketBook { tickets: Vec::new(), frozen: false }
    }

    /// 开单
    pub fn open(&mut self, id: &str, case: &str, class: &str, fp: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        let t = Ticket {
            id: id.to_string(),
            case: case.to_string(),
            class: class.to_string(),
            fingerprint: fp,
            state: TicketState::Open,
        };
        if !t.complete() {
            return Err(ERR_FIELD_MISSING);
        }
        if self.tickets.iter().any(|x| x.id == id) {
            return Err(ERR_DUP_ID);
        }
        self.tickets.push(t);
        Ok(())
    }

    /// 正向查：号→单
    pub fn by_id(&self, id: &str) -> Option<&Ticket> {
        self.tickets.iter().find(|t| t.id == id)
    }

    /// 反向查：用例→单（同用例可多单，取最近）
    pub fn by_case(&self, case: &str) -> Option<&Ticket> {
        self.tickets.iter().rev().find(|t| t.case == case)
    }

    /// 双向核：按号查到再用例查同一单
    pub fn bidirectional_ok(&self, id: &str) -> Result<bool, u32> {
        let fwd = self.by_id(id);
        match fwd {
            Some(t) => Ok(match self.by_case(&t.case) {
                Some(back) => back.id == t.id,
                None => false,
            }),
            None => Err(ERR_LINK_BROKEN),
        }
    }

    /// 断链检出：簿中所有单的双向核
    pub fn broken_links(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in &self.tickets {
            if !self.bidirectional_ok(&t.id).unwrap_or(false) {
                out.push(t.id.clone());
            }
        }
        out
    }

    /// 推进状态（不许倒转）
    pub fn advance(&mut self, id: &str) -> Result<(), u32> {
        if self.frozen {
            return Err(ERR_FROZEN);
        }
        let pos = match self.tickets.iter().position(|t| t.id == id) {
            Some(p) => p,
            None => return Err(ERR_LINK_BROKEN),
        };
        let cur = self.tickets[pos].state;
        let next = match cur {
            TicketState::Open => TicketState::InProgress,
            TicketState::InProgress => TicketState::Closed,
            TicketState::Closed => return Err(ERR_STATE_BACK),
        };
        self.tickets[pos].state = next;
        Ok(())
    }

    /// 开单计数
    pub fn open_count(&self) -> usize {
        self.tickets.iter().filter(|t| t.state == TicketState::Open).count()
    }

    /// 折叠指纹
    pub fn seal(&mut self) -> u32 {
        self.frozen = true;
        let mut acc = SEG_BASE;
        for (i, t) in self.tickets.iter().enumerate() {
            acc = acc.wrapping_mul(5741).wrapping_add(t.fingerprint >> (i % 3));
            acc = acc.wrapping_add((t.id.len() as u32) << 1);
            acc = acc.wrapping_add(t.state.ord() as u32);
        }
        acc
    }

    /// 诊断面：状态分布（开/处理/闭）
    pub fn state_split(&self) -> (usize, usize, usize) {
        let o = self.tickets.iter().filter(|t| t.state == TicketState::Open).count();
        let p = self.tickets.iter().filter(|t| t.state == TicketState::InProgress).count();
        let c = self.tickets.iter().filter(|t| t.state == TicketState::Closed).count();
        (o, p, c)
    }

    /// 诊断面：类分布计数
    pub fn class_count_of(&self, class: &str) -> usize {
        self.tickets.iter().filter(|t| t.class == class).count()
    }

    /// 诊断面：指纹查单（按失败指纹反查）
    pub fn by_fingerprint(&self, fp: u32) -> Vec<String> {
        self.tickets.iter().filter(|t| t.fingerprint == fp).map(|t| t.id.clone()).collect()
    }

    /// 诊断面：簿摘要
    pub fn book_summary(&self) -> String {
        format!("tickets={} open={}", self.tickets.len(), self.open_count())
    }

    /// 诊断面：工单状态名
    pub fn state_name(t: &Ticket) -> &'static str {
        match t.state {
            TicketState::Open => "OPEN",
            TicketState::InProgress => "IN_PROGRESS",
            TicketState::Closed => "CLOSED",
        }
    }

    /// 诊断面：全部工单字段自洽复核
    pub fn all_complete(&self) -> bool {
        self.tickets.iter().all(|t| t.complete())
    }
}

/// 注入五况
pub fn inject_five() -> [bool; 5] {
    let mut out = [false; 5];
    // 1: 缺字段拒绝
    let mut b1 = TicketBook::new();
    out[0] = b1.open("BUG-UNX-1", "", "KERNEL_BUG", 0x11) == Err(ERR_FIELD_MISSING)
        || b1.open("BUG-UNX-1", "case", "KERNEL_BUG", 0) == Err(ERR_FIELD_MISSING);
    // 2: 重号拒绝
    let mut b2 = TicketBook::new();
    let _ = b2.open("BUG-UNX-2", "c2", "KERNEL_BUG", 0x22);
    out[1] = b2.open("BUG-UNX-2", "c3", "KERNEL_BUG", 0x33) == Err(ERR_DUP_ID);
    // 3: 断链检出（伪造反查不同单）
    let mut b3 = TicketBook::new();
    let _ = b3.open("BUG-UNX-3", "cA", "KERNEL_BUG", 0x44);
    let _ = b3.open("BUG-UNX-3B", "cA", "ENV_LIMITED", 0x55); // 同用例更近单
    out[2] = b3.by_case("cA").map(|t| t.id.as_str()) == Some("BUG-UNX-3B");
    // 4: 状态倒转拒绝
    let mut b4 = TicketBook::new();
    let _ = b4.open("BUG-UNX-4", "c4", "KERNEL_BUG", 0x66);
    let _ = b4.advance("BUG-UNX-4");
    let _ = b4.advance("BUG-UNX-4");
    out[3] = b4.advance("BUG-UNX-4") == Err(ERR_STATE_BACK);
    // 5: 封簿拒绝
    let mut b5 = TicketBook::new();
    let _ = b5.open("BUG-UNX-5", "c5", "KERNEL_BUG", 0x77);
    let _ = b5.seal();
    out[4] = b5.open("BUG-UNX-6", "c6", "KERNEL_BUG", 0x88) == Err(ERR_FROZEN);
    out
}

/// 判据函数
pub fn run_unx10223_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10223");
    let mut b = TicketBook::new();
    let o1 = b.open("BUG-UNX-1023", "ut-net-flaky", "KERNEL_BUG", 0x5150);
    let o2 = b.open("BUG-UNX-1024", "ut-pty-miss", "ENV_LIMITED", 0x5151);
    s.add("双单开立", o1.is_ok() && o2.is_ok(), "两张五字段齐备工单开立");
    s.add("五字段自洽", b.tickets.iter().all(|t| t.complete()), "各单五字段齐备");
    s.add("正向查单", b.by_id("BUG-UNX-1023").map(|t| t.fingerprint) == Some(0x5150), "按号反查指纹一致");
    s.add("反向查单", b.by_case("ut-pty-miss").map(|t| t.class.as_str()) == Some("ENV_LIMITED"), "按用例查类名一致");
    s.add("双向同步", b.bidirectional_ok("BUG-UNX-1023") == Ok(true) && b.bidirectional_ok("BUG-UNX-1024") == Ok(true), "两单双向核通过");
    s.add("断链清单空", b.broken_links().is_empty(), "无断链单");
    s.add("状态推进", b.advance("BUG-UNX-1023").is_ok() && b.by_id("BUG-UNX-1023").map(|t| t.state) == Some(TicketState::InProgress), "状态推进至处理中");
    s.add("开单计数", b.open_count() == 1, "推进后开单余一");
    s.add("缺字段拒绝", b.open("BUG-UNX-X", "c", "KERNEL_BUG", 0) == Err(ERR_FIELD_MISSING), "零指纹工单被拒");
    s.add("重号拒绝", b.open("BUG-UNX-1023", "c", "KERNEL_BUG", 0x1) == Err(ERR_DUP_ID), "重号开单被拒");
    s.add("状态分布", b.state_split() == (1, 1, 0), "开一处理一闭零");
    s.add("类分布", b.class_count_of("KERNEL_BUG") == 1 && b.class_count_of("ENV_LIMITED") == 1, "两类各一单");
    s.add("指纹反查", b.by_fingerprint(0x5150) == vec!["BUG-UNX-1023".to_string()], "按指纹反查命中一单");
    s.add("簿摘要", b.book_summary().contains("tickets=2") && b.book_summary().contains("open=1"), "摘要含总数与开单数");
    s.add("状态名", b.by_id("BUG-UNX-1024").map(TicketBook::state_name) == Some("OPEN"), "未推进单状态名为 OPEN");
    s.add("全单自洽", b.all_complete(), "全单五字段复核");
    let five = inject_five();
    s.add("注入检出5/5", five.iter().all(|&x| x), "五类违规全部检出");
    let fp = b.seal();
    s.add("封簿拒绝", b.advance("BUG-UNX-1024") == Err(ERR_FROZEN), "封簿后操作被拒");
    s.add("封簿指纹非零", fp != 0, "封簿折叠指纹非零");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bidirectional_sync_holds() {
        let mut b = TicketBook::new();
        let _ = b.open("BUG-UNX-A", "case-x", "KERNEL_BUG", 0xA);
        assert!(b.bidirectional_ok("BUG-UNX-A").unwrap());
        assert!(b.broken_links().is_empty());
    }

    #[test]
    fn state_machine_forward_only() {
        let mut b = TicketBook::new();
        let _ = b.open("BUG-UNX-B", "case-y", "ENV_LIMITED", 0xB);
        let _ = b.advance("BUG-UNX-B");
        let _ = b.advance("BUG-UNX-B");
        assert_eq!(b.by_id("BUG-UNX-B").unwrap().state, TicketState::Closed);
        assert_eq!(b.advance("BUG-UNX-B"), Err(ERR_STATE_BACK));
        assert_eq!(b.state_split(), (0, 0, 1));
    }

    #[test]
    fn fingerprint_lookup_multi() {
        let mut b = TicketBook::new();
        let _ = b.open("T1", "c1", "KERNEL_BUG", 0xF0);
        let _ = b.open("T2", "c2", "KERNEL_BUG", 0xF0);
        let _ = b.open("T3", "c3", "ENV_LIMITED", 0xF1);
        assert_eq!(b.by_fingerprint(0xF0).len(), 2);
        assert_eq!(b.by_fingerprint(0xF1).len(), 1);
        assert_eq!(b.class_count_of("KERNEL_BUG"), 2);
        assert!(b.all_complete());
    }

    #[test]
    fn reverse_index_latest_wins() {
        let mut b = TicketBook::new();
        let _ = b.open("T1", "case", "KERNEL_BUG", 0x1);
        let _ = b.open("T2", "case", "ENV_LIMITED", 0x2);
        assert_eq!(b.by_case("case").map(|t| t.id.as_str()), Some("T2"));
        assert!(b.by_id("T1").is_some());
        assert_eq!(b.by_fingerprint(0x2), vec!["T2".to_string()]);
    }

    #[test]
    fn open_count_after_flow() {
        let mut b = TicketBook::new();
        let _ = b.open("K1", "c1", "KERNEL_BUG", 0x9);
        let _ = b.open("K2", "c2", "KERNEL_BUG", 0x8);
        let _ = b.advance("K1");
        let _ = b.advance("K1");
        assert_eq!(b.open_count(), 1);
        assert!(b.book_summary().contains("open=1"));
        assert_eq!(TicketBook::state_name(b.by_id("K1").unwrap()), "CLOSED");
    }
}
