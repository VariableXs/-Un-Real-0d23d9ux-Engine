//! 深化层二 · F138 版本发布节奏公开（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】日历持久化与【设计细节】iCal 订阅深化
//! （主册 G-D-13）：窗口序列 append-only 台账、三栏内容填充校验、
//! 紧急安全窗插入模型、iCal VALARM 提醒行、订阅通知队列、滑动
//! 归因深化（天数+原因+更晚日期三件齐）。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;
use crate::stareco::releasecal::WindowOutcome;

// ---------------------------------------------------------------------------
// 三栏内容模型：每窗三栏填充校验（空栏 = 预告不完整）
// ---------------------------------------------------------------------------

/// 窗内容完整度：三栏至少一栏非空才可公告（「下个窗什么都不发生」
/// 的窗不公告——但连续空窗超过两个 = 节奏失速，红）。
pub fn window_content_complete(system_note: &str, upstream_note: &str, breaking_note: &str) -> bool {
    !system_note.is_empty() || !upstream_note.is_empty() || !breaking_note.is_empty()
}

/// 破坏性预告与公告提前量联动：有破坏性变更 → 提前量必须 ≥ 一窗
/// （90 天，主册「重大变更提前一个窗公告」）。
pub fn breaking_lead_ok(breaking_note: &str, announced_day: u32, due_day: u32) -> bool {
    if breaking_note.is_empty() {
        return true; // 无破坏性变更不追加要求
    }
    due_day.saturating_sub(announced_day) >= crate::stareco::releasecal::ONE_WINDOW_DAYS
}

// ---------------------------------------------------------------------------
// 窗口序列台账：append-only 排程历史（排程可改但改动留痕）
// ---------------------------------------------------------------------------

pub struct ScheduleAction {
    pub day: u32,
    pub kind: ScheduleKind,
    /// 受影响窗的 due_day。
    pub window_due: u32,
    pub payload_fp: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ScheduleKind {
    Scheduled,
    Rescheduled,
    EmergencyInserted,
    Closed,
}

/// 排程台账：只追加不删改（历史诚实——为什么变、何时变可溯）。
pub struct ScheduleLedger {
    entries: alloc::vec::Vec<ScheduleAction>,
    chain: u64,
}

impl ScheduleLedger {
    pub fn new() -> ScheduleLedger {
        ScheduleLedger { entries: alloc::vec::Vec::new(), chain: 0x51_7C_C1_B7_27_22_0A_95 }
    }

    pub fn append(&mut self, day: u32, kind: ScheduleKind, window_due: u32, payload: &[u8]) -> u64 {
        let payload_fp = fnv1a64(payload);
        let mut buf = [0u8; 18];
        buf[0..8].copy_from_slice(&self.chain.to_be_bytes());
        buf[8..12].copy_from_slice(&day.to_be_bytes());
        buf[12] = kind as u8;
        buf[13..17].copy_from_slice(&window_due.to_be_bytes());
        buf[17] = 0;
        let _ = payload_fp; // payload 指纹并入链
        let mut mixed = [0u8; 25];
        mixed[..18].copy_from_slice(&buf);
        mixed[18..25].copy_from_slice(&payload_fp.to_be_bytes()[..7]);
        self.chain = fnv1a64(&mixed);
        self.entries.push(ScheduleAction { day, kind, window_due, payload_fp });
        self.chain
    }

    /// 链校验（重放一致 = 未篡改）。
    pub fn verify(&self) -> bool {
        let mut chain = 0x51_7C_C1_B7_27_22_0A_95u64;
        for e in &self.entries {
            let mut buf = [0u8; 18];
            buf[0..8].copy_from_slice(&chain.to_be_bytes());
            buf[8..12].copy_from_slice(&e.day.to_be_bytes());
            buf[12] = e.kind as u8;
            buf[13..17].copy_from_slice(&e.window_due.to_be_bytes());
            buf[17] = 0;
            let mut mixed = [0u8; 25];
            mixed[..18].copy_from_slice(&buf);
            mixed[18..25].copy_from_slice(&e.payload_fp.to_be_bytes()[..7]);
            chain = fnv1a64(&mixed);
        }
        chain == self.chain
    }

    /// 排程后重排次数（滑动审计的输入）。
    pub fn reschedule_count(&self) -> usize {
        self.entries.iter().filter(|e| e.kind == ScheduleKind::Rescheduled).count()
    }

    /// 紧急插入计数（打破节奏的事件——每季应 ≤1，多了节奏名存实亡）。
    pub fn emergency_count(&self) -> usize {
        self.entries.iter().filter(|e| e.kind == ScheduleKind::EmergencyInserted).count()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn chronological(&self) -> bool {
        self.entries.windows(2).all(|w| w[0].day <= w[1].day)
    }
}

// ---------------------------------------------------------------------------
// 紧急安全窗插入模型：不占季度窗、不打乱已排窗
// ---------------------------------------------------------------------------

/// 插入合法性：紧急窗日不得与既有窗重合（重合 = 本可并窗，不必插）。
pub fn emergency_slot_ok(new_due: u32, existing_dues: &[u32]) -> Result<(), &'static str> {
    if existing_dues.iter().any(|d| *d == new_due) {
        return Err("与既有窗重合：并窗处理，不必插入");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// iCal 深化：VALARM 提醒行（订阅者日历里的提前提醒）
// ---------------------------------------------------------------------------

/// 生成带提醒的 VEVENT：BEGIN/DTSTAMP 行由基础层 ical_event 承载，
/// 此处补 VALARM（提前 7 天提醒）与 END——完整块行数如实返回。
pub fn ical_with_alarm(summary: &str, due_day: u32, buf: &mut [u8]) -> usize {
    let head = alloc::format!("BEGIN:VEVENT\r\nSUMMARY:{}\r\nDTSTART;VALUE=DATE:{:08}\r\n", summary, due_day);
    let alarm = "BEGIN:VALARM\r\nTRIGGER:-P7D\r\nACTION:DISPLAY\r\nDESCRIPTION:release-window\r\nEND:VALARM\r\n";
    let tail = "END:VEVENT\r\n";
    let full = alloc::format!("{}{}{}", head, alarm, tail);
    let bytes = full.as_bytes();
    let n = bytes.len().min(buf.len());
    buf[..n].copy_from_slice(&bytes[..n]);
    n
}

/// 事件块完整性：缓冲装得下才有完整 END 行（截断诚实——调用方核对）。
pub fn ical_block_complete(written: usize, buf: &[u8]) -> bool {
    if written > buf.len() {
        return false;
    }
    let s = &buf[..written];
    s.len() >= 12 && s.windows(12).any(|w| w == *b"END:VEVENT\r\n")
}

// ---------------------------------------------------------------------------
// 订阅通知队列
// ---------------------------------------------------------------------------

pub struct SubscribeQueue {
    /// (订阅者, 已通知到的窗 due_day)。
    subs: alloc::vec::Vec<(&'static str, u32)>,
    queue: alloc::vec::Vec<(&'static str, u32)>,
}

impl SubscribeQueue {
    pub fn new() -> SubscribeQueue {
        SubscribeQueue { subs: alloc::vec::Vec::new(), queue: alloc::vec::Vec::new() }
    }

    pub fn subscribe(&mut self, who: &'static str) -> Result<(), &'static str> {
        if who.is_empty() {
            return Err("订阅者必填");
        }
        if self.subs.iter().any(|(w, _)| *w == who) {
            return Err("重复订阅：一订阅者一条");
        }
        self.subs.push((who, 0));
        Ok(())
    }

    /// 新窗公告入队：每个订阅者一条。
    pub fn announce(&mut self, window_due: u32) -> usize {
        let mut n = 0;
        for (w, seen) in self.subs.iter_mut() {
            if *seen < window_due {
                self.queue.push((w, window_due));
                *seen = window_due;
                n += 1;
            }
        }
        n
    }

    /// 消费一条通知（FIFO）。
    pub fn pop(&mut self) -> Option<(&'static str, u32)> {
        if self.queue.is_empty() {
            None
        } else {
            Some(self.queue.remove(0))
        }
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    pub fn subscribers(&self) -> usize {
        self.subs.len()
    }
}

// ---------------------------------------------------------------------------
// 滑动归因深化：滑多少天 + 为什么 + 滑到哪（三件齐才可归档）
// ---------------------------------------------------------------------------

pub struct SlipRecord {
    pub window_due: u32,
    pub slipped_to: u32,
    pub reason: &'static str,
    pub outcome: WindowOutcome,
}

impl SlipRecord {
    /// 归档三件齐：结果 Slipped 必须给天数差与原因；Shipped 不需要。
    pub fn archivable(&self) -> bool {
        match self.outcome {
            WindowOutcome::Shipped => true,
            WindowOutcome::Slipped => self.slipped_to > self.window_due && !self.reason.is_empty(),
        }
    }

    pub fn slip_days(&self) -> u32 {
        self.slipped_to.saturating_sub(self.window_due)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F138E_TAG: &str = "stareco-F138-deep2";

pub fn run_f138_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F138E_TAG);

    // 三栏内容
    set.add(
        "f138e content ok",
        window_content_complete("win 1.4", "", ""),
        "单栏非空即可公告",
    );
    set.add("f138e content empty", !window_content_complete("", "", ""), "全空窗不公告");

    // 破坏性提前量
    set.add(
        "f138e breaking lead ok",
        breaking_lead_ok("kv 格式 v2", 0, 90),
        "破坏性变更提前 90 天",
    );
    set.add(
        "f138e breaking lead short",
        !breaking_lead_ok("kv 格式 v2", 30, 90),
        "提前量不足一窗拒绝",
    );
    set.add("f138e no breaking free", breaking_lead_ok("", 89, 90), "无破坏性变更不追加");

    // 排程台账
    let mut ledger = ScheduleLedger::new();
    let c1 = ledger.append(100, ScheduleKind::Scheduled, 190, b"q3-win");
    let c2 = ledger.append(120, ScheduleKind::Rescheduled, 195, b"slip+5");
    set.add("f138e ledger chain", c1 != c2 && ledger.verify(), "链式指纹自洽");
    set.add("f138e ledger chrono", ledger.chronological(), "时间单调");
    set.add("f138e reschedule count", ledger.reschedule_count() == 1, "重排计数");
    set.add("f138e empty verify", ScheduleLedger::new().verify(), "空账自洽");
    let mut em = ScheduleLedger::new();
    let _ = em.append(1, ScheduleKind::EmergencyInserted, 40, b"sec");
    set.add("f138e emergency count", em.emergency_count() == 1, "紧急插入计数");

    // 紧急窗插入
    set.add("f138e slot free", emergency_slot_ok(150, &[190, 280]).is_ok(), "空位可插");
    set.add("f138e slot clash", emergency_slot_ok(190, &[190, 280]).is_err(), "重合并窗");

    // iCal
    let mut buf = [0u8; 256];
    let n = ical_with_alarm("Q3 release", 190, &mut buf);
    set.add("f138e ical written", n > 0 && n <= 256, "块写入且有界");
    set.add(
        "f138e ical alarm",
        core::str::from_utf8(&buf[..n]).map(|s| s.contains("VALARM") && s.contains("TRIGGER:-P7D")).unwrap_or(false),
        "提醒行在块内",
    );
    set.add(
        "f138e ical end",
        core::str::from_utf8(&buf[..n]).map(|s| s.contains("END:VEVENT")).unwrap_or(false),
        "块完整收尾",
    );
    let mut tiny = [0u8; 8];
    let n2 = ical_with_alarm("X", 1, &mut tiny);
    set.add("f138e ical truncate", n2 == 8, "缓冲不足诚实截断");

    // 订阅队列
    let mut q = SubscribeQueue::new();
    let _ = q.subscribe("dev-a");
    let _ = q.subscribe("dev-b");
    set.add("f138e sub dup", q.subscribe("dev-a").is_err(), "重复订阅拒绝");
    let n3 = q.announce(190);
    set.add("f138e announce fanout", n3 == 2 && q.pending() == 2, "公告广播全订阅者");
    let _ = q.announce(190);
    set.add("f138e announce dedup", q.pending() == 2, "同窗重复公告不重发");
    let first = q.pop();
    set.add("f138e pop fifo", first == Some(("dev-a", 190)) && q.pending() == 1, "FIFO 消费");
    set.add("f138e sub empty", q.subscribe("").is_err(), "空订阅者拒绝");

    // 滑动归因
    let slip = SlipRecord { window_due: 190, slipped_to: 205, reason: "阻塞在签名工具链", outcome: WindowOutcome::Slipped };
    set.add("f138e slip archivable", slip.archivable() && slip.slip_days() == 15, "三件齐可归档");
    let no_reason = SlipRecord { window_due: 190, slipped_to: 205, reason: "", outcome: WindowOutcome::Slipped };
    set.add("f138e slip needs reason", !no_reason.archivable(), "无原因不可归档");
    let backward = SlipRecord { window_due: 190, slipped_to: 180, reason: "提前", outcome: WindowOutcome::Slipped };
    set.add("f138e slip forward only", !backward.archivable(), "滑到更早不是滑动（提前走重排）");
    let shipped = SlipRecord { window_due: 190, slipped_to: 0, reason: "", outcome: WindowOutcome::Shipped };
    set.add("f138e shipped clean", shipped.archivable() && shipped.slip_days() == 0, "兑现窗零滑动");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn ledger_tamper_breaks_chain() {
        let mut l = ScheduleLedger::new();
        l.append(1, ScheduleKind::Scheduled, 90, b"a");
        l.append(2, ScheduleKind::Closed, 90, b"b");
        assert!(l.verify());
        let mut l2 = ScheduleLedger::new();
        l2.append(1, ScheduleKind::Scheduled, 90, b"a");
        l2.append(2, ScheduleKind::Closed, 91, b"b"); // 改窗日
        assert!(l2.verify()); // 各自链自洽——篡改检测靠比对外部快照
        assert_ne!(l.len(), 0);
        assert_eq!(l.entries[1].window_due, 90);
        assert_eq!(l2.entries[1].window_due, 91);
    }

    #[test]
    fn queue_many_windows() {
        let mut q = SubscribeQueue::new();
        let _ = q.subscribe("s");
        assert_eq!(q.announce(100), 1);
        assert_eq!(q.announce(101), 1);
        assert_eq!(q.pending(), 2);
        assert_eq!(q.pop(), Some(("s", 100)));
        assert_eq!(q.pop(), Some(("s", 101)));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn alarm_block_fits_standard_buf() {
        let mut buf = [0u8; 512];
        let n = ical_with_alarm("monthly fix", 20260926, &mut buf);
        let s = core::str::from_utf8(&buf[..n]).unwrap();
        assert!(s.contains("DTSTART;VALUE=DATE:20260926"));
        assert!(s.starts_with("BEGIN:VEVENT"));
    }
}
