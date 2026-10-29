//! UNX-F56012 采集日联网更新窗口管理件（0x60160 段）
//!
//! 锚点：UNX-F56012-J1 Windows 盘长期断网、仅采集日联网更新的窗口
//! 策略落地（断网态默认 + 开窗显性 + 关窗复断三态账）；J2 更新事件
//! 强制入账（更新前后 env_hash 双记录 ×1 实演留痕）；J3 窗外联网拒收
//! （非采集日开窗即拒）三判据全绿。判据锚定 §AI-71.6 风险①策略。
//! 域：CoRun·UNX-F2-B40（环境冻结域·更新窗口面）。
//!
//! 结构：窗口状态机（断网态/开窗态/关窗态三态迁移）+ 开窗守闸（仅
//! 采集日历位可开）+ 更新事件账（前后 env_hash 双记录逐条落账）+
//! 状态迁移审计链 + 注入核（非采集日开窗/窗外联网/空哈希/封账改写/
//! 重复开窗必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60160 独占 0x20）。
pub const SEG_BASE: u32 = 0x60160;
pub const UW_NOT_DAY: u32 = SEG_BASE + 0x01; // 非采集日开窗
pub const UW_ALREADY_OPEN: u32 = SEG_BASE + 0x02; // 重复开窗（已开再开）
pub const UW_CLOSE_MISS: u32 = SEG_BASE + 0x03; // 未开窗即关窗
pub const UW_NET_OUTSIDE: u32 = SEG_BASE + 0x04; // 窗外联网请求
pub const UW_HASH_EMPTY: u32 = SEG_BASE + 0x05; // 更新事件哈希空记录
pub const UW_SEALED: u32 = SEG_BASE + 0x06; // 窗口账已封账拒迁移
pub const UW_SEQ_BACK: u32 = SEG_BASE + 0x07; // 事件序号回退

/// 窗口三态闭集。
pub const ST_OFFLINE: usize = 0;
pub const ST_OPEN: usize = 1;
pub const ST_CLOSED: usize = 2;
pub const ST_COUNT: usize = 3;
pub const ST_NAMES: [&str; ST_COUNT] = ["offline", "open", "closed"];

/// 状态名同源核。
pub fn state_name(st: usize) -> Option<&'static str> {
    if st < ST_COUNT {
        Some(ST_NAMES[st])
    } else {
        None
    }
}

/// 更新事件账目：序号 × 旧环境哈希 × 新环境哈希 × 版本锚。
#[derive(Clone)]
pub struct UpdateLedgerEntry {
    pub seq: u32,
    pub env_hash_before: u32,
    pub env_hash_after: u32,
    pub anchor_ver: String,
}

impl UpdateLedgerEntry {
    pub fn new(seq: u32, h_before: u32, h_after: u32, anchor_ver: &str) -> Result<Self, u32> {
        if anchor_ver.is_empty() {
            return Err(UW_HASH_EMPTY);
        }
        if h_before == 0 || h_after == 0 {
            return Err(UW_HASH_EMPTY);
        }
        Ok(UpdateLedgerEntry {
            seq,
            env_hash_before: h_before,
            env_hash_after: h_after,
            anchor_ver: anchor_ver.to_string(),
        })
    }

    /// 双记录机读行（前后哈希逐条可读）。
    pub fn render(&self) -> String {
        format!(
            "UPD|{}|{:08x}|{:08x}|{}",
            self.seq, self.env_hash_before, self.env_hash_after, self.anchor_ver
        )
    }
}

/// 更新窗口管理器：三态状态机 + 更新事件账。
pub struct UpdateWindow {
    pub state: usize,
    pub collect_days: Vec<u32>,
    pub events: Vec<UpdateLedgerEntry>,
    pub audit: Vec<String>,
    pub sealed: bool,
}

impl UpdateWindow {
    pub fn new(collect_days: Vec<u32>) -> Self {
        UpdateWindow {
            state: ST_OFFLINE,
            collect_days,
            events: Vec::new(),
            audit: Vec::new(),
            sealed: false,
        }
    }

    /// 是否采集日（日历位查询）。
    pub fn is_collect_day(&self, day: u32) -> bool {
        for d in self.collect_days.iter() {
            if *d == day {
                return true;
            }
        }
        false
    }

    /// 开窗：仅采集日可开；已开再开拒；封账拒。
    pub fn open_window(&mut self, day: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(UW_SEALED);
        }
        if self.state == ST_OPEN {
            return Err(UW_ALREADY_OPEN);
        }
        if !self.is_collect_day(day) {
            self.audit.push(format!("WIN|DENY|day={}", day));
            return Err(UW_NOT_DAY);
        }
        self.state = ST_OPEN;
        self.audit.push(format!("WIN|OPEN|day={}", day));
        Ok(())
    }

    /// 更新事件入账（仅开窗态可入；序号严格递增；双哈希必填）。
    pub fn record_update(&mut self, e: UpdateLedgerEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(UW_SEALED);
        }
        if self.state != ST_OPEN {
            return Err(UW_NET_OUTSIDE);
        }
        if let Some(prev) = self.events.last() {
            if e.seq <= prev.seq {
                return Err(UW_SEQ_BACK);
            }
        }
        self.events.push(e.clone());
        self.audit.push(format!("WIN|UPD|seq={}", e.seq));
        Ok(())
    }

    /// 关窗：开窗态才可关；关后复断网。
    pub fn close_window(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(UW_SEALED);
        }
        if self.state != ST_OPEN {
            return Err(UW_CLOSE_MISS);
        }
        self.state = ST_CLOSED;
        self.audit.push("WIN|CLOSE".to_string());
        Ok(())
    }

    /// 关窗后复位到断网态（下一轮默认断网）。
    pub fn reset_offline(&mut self) -> Result<(), u32> {
        if self.state != ST_CLOSED {
            return Err(UW_CLOSE_MISS);
        }
        self.state = ST_OFFLINE;
        Ok(())
    }

    /// J2：更新事件双记录齐（每条前后哈希非零且互异或同存可读）。
    pub fn events_well_formed(&self) -> bool {
        if self.events.is_empty() {
            return false;
        }
        for e in self.events.iter() {
            if e.env_hash_before == 0 || e.env_hash_after == 0 {
                return false;
            }
            if e.render().split('|').count() != 5 {
                return false;
            }
        }
        true
    }

    /// J1：策略落地核——默认断网 + 审计链含 OPEN/CLOSE 对。
    pub fn policy_ok(&self) -> bool {
        self.state != ST_OPEN
            && self.audit.iter().any(|a| a.contains("WIN|OPEN|"))
            && self.audit.iter().any(|a| a.contains("WIN|CLOSE"))
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：采集日 {7, 14}，实演一轮开窗→更新→关窗→复位。
pub fn standard_window() -> UpdateWindow {
    let mut w = UpdateWindow::new(vec![7, 14]);
    let _ = w.open_window(7);
    let e = UpdateLedgerEntry::new(1, 0x00ABCDEF, 0x00ABCDF0, "win-1.1.0").expect("upd");
    let _ = w.record_update(e);
    let _ = w.close_window();
    let _ = w.reset_offline();
    w
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 非采集日开窗（day=3 不在 {7,14}）
    let mut w = UpdateWindow::new(vec![7, 14]);
    if w.open_window(3) == Err(UW_NOT_DAY) {
        caught += 1;
    }
    // 2) 窗外联网（未开窗即入更新账）
    let e = UpdateLedgerEntry::new(1, 1, 2, "win-1.0.0").expect("e");
    if w.record_update(e) == Err(UW_NET_OUTSIDE) {
        caught += 1;
    }
    // 3) 空哈希更新事件
    if UpdateLedgerEntry::new(1, 0, 2, "win-1.0.0").err() == Some(UW_HASH_EMPTY) {
        caught += 1;
    }
    // 4) 重复开窗
    let mut w2 = UpdateWindow::new(vec![7]);
    let _ = w2.open_window(7);
    if w2.open_window(7) == Err(UW_ALREADY_OPEN) {
        caught += 1;
    }
    // 5) 未开窗即关窗
    let mut w3 = UpdateWindow::new(vec![7]);
    if w3.close_window() == Err(UW_CLOSE_MISS) {
        caught += 1;
    }
    (caught, 5)
}

/// 审计链渲染。
pub fn render_audit(w: &UpdateWindow) -> String {
    let mut out = String::new();
    for a in w.audit.iter() {
        out.push_str(a.as_str());
        out.push('\n');
    }
    out
}

/// 判据入口（CheckSet）。
pub fn run_unx56012_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56012");
    // J1-1 窗口策略落地（默认断网 + 开关窗对 + 复位断网）
    let w = standard_window();
    let ok = w.policy_ok() && w.state == ST_OFFLINE;
    s.add("56012-J1-窗口策略落地", ok, if ok { "" } else { "策略未落地" });
    // J1-2 采集日历位语义（7/14 真，3/8 假）
    let w2 = UpdateWindow::new(vec![7, 14]);
    let ok = w2.is_collect_day(7) && w2.is_collect_day(14) && !w2.is_collect_day(3) && !w2.is_collect_day(8);
    s.add("56012-J1-采集日历位", ok, if ok { "" } else { "日历位漂移" });
    // J1-3 三态闭集同源
    let ok = state_name(ST_OFFLINE) == Some("offline")
        && state_name(ST_OPEN) == Some("open")
        && state_name(9).is_none();
    s.add("56012-J1-三态闭集", ok, if ok { "" } else { "三态漂移" });
    // J2-1 更新事件 ×1 实演留痕（双哈希记录齐）
    let w3 = standard_window();
    let ok = w3.events.len() == 1 && w3.events_well_formed();
    s.add("56012-J2-更新实演留痕", ok, if ok { "" } else { "实演账缺" });
    // J2-2 双哈希机读行（UPD 行五段带前后哈希）
    let e = standard_window().events[0].clone();
    let line = e.render();
    let ok = line.starts_with("UPD|1|00abcdef|00abcdf0|win-1.1.0");
    s.add("56012-J2-双哈希机读", ok, if ok { "" } else { "机读行漂移" });
    // J2-3 更新前后哈希可辨（同哈希也允许但双值必须落）
    let e2 = UpdateLedgerEntry::new(2, 0x00ABCDEF, 0x00ABCDEF, "win-1.1.0").expect("same");
    let ok = e2.render().contains("00abcdef|00abcdef");
    s.add("56012-J2-同哈希可读", ok, if ok { "" } else { "同哈希不可读" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56012-J3-注入核必抓", ok, &detail);
    // J3-2 非采集日开窗留痕（DENY 行落审计）
    let mut w4 = UpdateWindow::new(vec![7, 14]);
    let _ = w4.open_window(3);
    let ok = w4.audit.iter().any(|a| a.contains("WIN|DENY|day=3"));
    s.add("56012-J3-拒开留痕", ok, if ok { "" } else { "拒开未留痕" });
    // J3-3 事件序号回退拒
    let mut w5 = UpdateWindow::new(vec![7]);
    let _ = w5.open_window(7);
    let e1 = UpdateLedgerEntry::new(1, 1, 2, "v").expect("e1");
    let _ = w5.record_update(e1);
    let e0 = UpdateLedgerEntry::new(1, 1, 2, "v").expect("e0");
    let ok = w5.record_update(e0) == Err(UW_SEQ_BACK);
    s.add("56012-J3-序号回退拒", ok, if ok { "" } else { "序号回退穿透" });
    // J3-4 封账冻结（封后开窗/入账/关窗全拒）
    let mut w6 = UpdateWindow::new(vec![7]);
    w6.seal();
    let ok = w6.open_window(7) == Err(UW_SEALED)
        && w6.close_window() == Err(UW_SEALED)
        && w6.record_update(UpdateLedgerEntry::new(1, 1, 2, "v").expect("s")) == Err(UW_SEALED);
    s.add("56012-J3-封账冻结", ok, if ok { "" } else { "封账后可迁" });
    // J1-4 审计链渲染齐（OPEN/CLOSE/DENY 行可读）
    let w7 = standard_window();
    let mut w8 = UpdateWindow::new(vec![7, 14]);
    let _ = w8.open_window(3);
    let text = format!("{}{}", render_audit(&w7), render_audit(&w8));
    let ok = text.contains("WIN|OPEN|day=7") && text.contains("WIN|CLOSE") && text.contains("WIN|DENY");
    s.add("56012-J1-审计链渲染齐", ok, if ok { "" } else { "审计渲染缺" });
    // J1-5 关窗后复位语义（closed → offline）
    let mut w9 = UpdateWindow::new(vec![7]);
    let _ = w9.open_window(7);
    let _ = w9.close_window();
    let ok = w9.state == ST_CLOSED && w9.reset_offline().is_ok() && w9.state == ST_OFFLINE;
    s.add("56012-J1-复位语义", ok, if ok { "" } else { "复位漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56012_all_checks_green() {
        let set = run_unx56012_checks();
        assert!(set.all_passed(), "UNX-F56012 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56012_state_machine() {
        let mut w = UpdateWindow::new(vec![7]);
        assert!(w.open_window(7).is_ok());
        assert_eq!(w.state, ST_OPEN);
        assert!(w.close_window().is_ok());
        assert_eq!(w.state, ST_CLOSED);
    }
}
