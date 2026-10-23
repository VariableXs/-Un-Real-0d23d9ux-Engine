//! VE-F0105 · 安全审计日志对接（VE-A 域 · A04b 诊断·降级·安全组收口 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0105`
//!
//! **职责定位（锚点原文）**：安全相关事件（白名单切换、脱敏导出、异常订阅变更、
//! 降级通知）统一对接安全审计日志——事件带主体、动作、对象、结果、时间五元组，
//! 审计流只增不改；审计流独立于普通日志文件，写入失败显性化并暂存补写。
//!
//! **铁律**：
//! 1. **五元组**——主体/动作/对象/结果/时间缺一不记。
//! 2. **只增不改**——审计流 append-only，无修改无删除入口。
//! 3. **独立流**——独立于普通日志，磁盘满优先保审计。
//! 4. **补写**——写失败内存暂存，恢复后按序补写。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B0（审计对接）。
pub const SEG_BASE: u16 = 0xE8B0;

/// 五元组缺项。
pub const XC_TUPLE_INCOMPLETE: u16 = SEG_BASE | 0x01;
/// 审计流写失败。
pub const XC_WRITE_FAIL: u16 = SEG_BASE | 0x02;
/// 暂存队列满。
pub const XC_SPILL_FULL: u16 = SEG_BASE | 0x03;
/// 序号乱（时钟回拨防护）。
pub const XC_SEQ_DISORDER: u16 = SEG_BASE | 0x04;

/// 审计事件（五元组）。
#[derive(Clone, Copy, Debug)]
pub struct AuditEvent {
    /// 主体（谁）。
    pub subject: u32,
    /// 动作（做了什么）。
    pub action: u16,
    /// 对象（对谁/对什么）。
    pub object: u32,
    /// 结果（0 成功 1 失败 2 拒绝）。
    pub result: u8,
    /// 时间（tick）。
    pub tick: u64,
    /// 单调序号（时钟回拨时保序）。
    pub seq: u64,
}

impl AuditEvent {
    /// 五元组完整性。
    pub fn complete(&self) -> bool {
        // subject=0 表示未填，action=0 表示未填；tick 可为 0；seq 必非零。
        self.subject != 0 && self.action != 0 && self.seq != 0
    }
}

/// 审计流（append-only）。
pub struct AuditStream {
    /// 已落盘事件。
    committed: Vec<AuditEvent>,
    /// 补写暂存（有上限）。
    spill: Vec<AuditEvent>,
    spill_cap: usize,
    /// 流健康位。
    healthy: bool,
    next_seq: u64,
    last_code: u16,
    /// 磁盘满标记（优先保审计拒普通日志）。
    disk_full: bool,
    /// 累计补写成功。
    replayed: u64,
}

impl AuditStream {
    pub fn new(spill_cap: usize) -> Self {
        AuditStream {
            committed: Vec::new(),
            spill: Vec::new(),
            spill_cap: if spill_cap == 0 { 32 } else { spill_cap },
            healthy: true,
            next_seq: 1,
            last_code: 0,
            disk_full: false,
            replayed: 0,
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 写入审计事件：五元组校验 + 单调序号 + 只增。
    pub fn write(&mut self, mut ev: AuditEvent) -> Result<u64, u16> {
        // 单调序号分配（时钟回拨仍保序）；seq 由流统一填写，
        // 故完整性校验必须在分配之后——否则新建事件的 seq=0 必然被误判缺项。
        ev.seq = self.next_seq;
        self.next_seq += 1;
        if !ev.complete() {
            self.last_code = XC_TUPLE_INCOMPLETE;
            return Err(XC_TUPLE_INCOMPLETE);
        }

        if self.disk_full {
            // 磁盘满：审计优先——暂存不丢，普通日志由调用方拒绝。
            self.last_code = 0;
            return self.spill_push(ev);
        }
        if !self.healthy {
            return self.spill_push(ev);
        }
        self.committed.push(ev);
        self.last_code = 0;
        Ok(ev.seq)
    }

    fn spill_push(&mut self, ev: AuditEvent) -> Result<u64, u16> {
        if self.spill.len() >= self.spill_cap {
            self.last_code = XC_SPILL_FULL;
            return Err(XC_SPILL_FULL);
        }
        let seq = ev.seq;
        self.spill.push(ev);
        self.last_code = XC_WRITE_FAIL;
        Err(XC_WRITE_FAIL)
    }

    /// 流恢复：暂存按序补写。
    pub fn recover(&mut self) -> Result<usize, u16> {
        self.healthy = true;
        self.disk_full = false;
        let n = self.spill.len();
        for ev in self.spill.drain(..) {
            self.committed.push(ev);
        }
        self.replayed += n as u64;
        self.last_code = 0;
        Ok(n)
    }

    /// 注入写故障（测试支撑）。
    pub fn inject_fault(&mut self) {
        self.healthy = false;
    }

    /// 模拟磁盘满（优先保审计）。
    pub fn set_disk_full(&mut self, full: bool) {
        self.disk_full = full;
    }

    /// 按序号查询 O(log n)（二分）。
    pub fn query_seq(&self, seq: u64) -> Option<&AuditEvent> {
        if self.committed.is_empty() {
            return None;
        }
        let mut lo = 0usize;
        let mut hi = self.committed.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.committed[mid].seq.cmp(&seq) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return Some(&self.committed[mid]),
            }
        }
        None
    }

    /// 只增性自检：序号严格递增。
    pub fn verify_append_only(&self) -> bool {
        for w in self.committed.windows(2) {
            if w[0].seq >= w[1].seq {
                return false;
            }
        }
        true
    }

    pub fn committed_len(&self) -> usize {
        self.committed.len()
    }

    pub fn spill_len(&self) -> usize {
        self.spill.len()
    }

    pub fn replayed(&self) -> u64 {
        self.replayed
    }

    /// 事件枚举（只读视图）。
    pub fn events(&self) -> &[AuditEvent] {
        &self.committed
    }
}

/// 审计动作码（对接四个上游）。
pub mod action {
    /// 白名单切换。
    pub const WHITELIST_SWITCH: u16 = 0x0101;
    /// 白名单回滚。
    pub const WHITELIST_ROLLBACK: u16 = 0x0102;
    /// 脱敏导出。
    pub const EXPORT: u16 = 0x0201;
    /// 订阅注册。
    pub const SUB_REGISTER: u16 = 0x0301;
    /// 订阅停用。
    pub const SUB_DISABLE: u16 = 0x0302;
    /// 降级通知。
    pub const NOTIFY: u16 = 0x0401;
}

/// 自检：五元组、只增不改、独立流、补写。
pub fn run_vea105_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn ev(subject: u32, action: u16, object: u32, result: u8, tick: u64) -> AuditEvent {
        AuditEvent {
            subject,
            action,
            object,
            result,
            tick,
            seq: 0,
        }
    }

    let mut set = CheckSet::new("vea105_audit");

    // C105-01 五元组写入
    {
        let mut s = AuditStream::new(8);
        let r = s.write(ev(1, action::EXPORT, 2, 0, 10));
        chk(&mut set, "C105-01 五元组", r.is_ok() && s.committed_len() == 1, "写入失败");
    }

    // C105-02 缺项拒绝
    {
        let mut s = AuditStream::new(8);
        let r = s.write(ev(0, action::EXPORT, 2, 0, 10));
        chk(
            &mut set,
            "C105-02 缺项拒绝",
            r.is_err() && s.last_code() == XC_TUPLE_INCOMPLETE,
            "缺项被放行",
        );
    }

    // C105-03 只增不改（序号严格递增）
    {
        let mut s = AuditStream::new(8);
        let _ = s.write(ev(1, action::SUB_REGISTER, 1, 0, 1));
        let _ = s.write(ev(1, action::SUB_DISABLE, 1, 0, 2));
        let _ = s.write(ev(1, action::NOTIFY, 1, 0, 3));
        chk(&mut set, "C105-03 只增有序", s.verify_append_only(), "序号乱序");
    }

    // C105-04 写故障暂存
    {
        let mut s = AuditStream::new(8);
        s.inject_fault();
        let r = s.write(ev(1, action::EXPORT, 2, 0, 1));
        let ok = r.is_err() && s.spill_len() == 1;
        chk(&mut set, "C105-04 故障暂存", ok, "未暂存");
    }

    // C105-05 恢复补写
    {
        let mut s = AuditStream::new(8);
        s.inject_fault();
        let _ = s.write(ev(1, action::EXPORT, 2, 0, 1));
        let _ = s.write(ev(1, action::EXPORT, 3, 0, 2));
        let r = s.recover();
        let ok = r == Ok(2) && s.committed_len() == 2 && s.spill_len() == 0 && s.replayed() == 2;
        chk(&mut set, "C105-05 恢复补写", ok, "补写失败");
    }

    // C105-06 暂存上限防失控
    {
        let mut s = AuditStream::new(2);
        s.inject_fault();
        let _ = s.write(ev(1, 1, 1, 0, 1));
        let _ = s.write(ev(1, 1, 1, 0, 2));
        let r = s.write(ev(1, 1, 1, 0, 3));
        chk(
            &mut set,
            "C105-06 暂存上限",
            r.is_err() && s.last_code() == XC_SPILL_FULL,
            "上限失控",
        );
    }

    // C105-07 磁盘满优先保审计
    {
        let mut s = AuditStream::new(8);
        s.set_disk_full(true);
        let r = s.write(ev(1, action::EXPORT, 2, 0, 1));
        // 暂存不丢（审计优先），返回失败码但事件保住。
        let ok = r.is_err() && s.spill_len() == 1;
        let _ = s.recover();
        chk(&mut set, "C105-07 优先保审计", ok && s.committed_len() == 1, "审计丢失");
    }

    // C105-08 按序号查询
    {
        let mut s = AuditStream::new(8);
        let _ = s.write(ev(1, 1, 1, 0, 1));
        let _ = s.write(ev(1, 2, 2, 0, 2));
        let r = s.query_seq(2);
        chk(&mut set, "C105-08 序号查询", r.is_some() && r.unwrap().action == 2, "查询失败");
    }

    // C105-09 时钟回拨保序
    {
        let mut s = AuditStream::new(8);
        let _ = s.write(ev(1, 1, 1, 0, 100));
        // tick 回拨。
        let r = s.write(ev(1, 2, 2, 0, 50));
        let ok = r.is_ok() && s.verify_append_only();
        chk(&mut set, "C105-09 回拨保序", ok, "回拨乱序");
    }

    // C105-10 动作码对接面
    {
        let ok = action::WHITELIST_SWITCH != action::EXPORT
            && action::SUB_REGISTER != action::NOTIFY
            && action::EXPORT != 0;
        chk(&mut set, "C105-10 对接面", ok, "动作码冲突");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea105_all_green() {
        let s = run_vea105_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
