//! ve180_knobaudit ↔ VE-F0180 旋钮变更审计
//!
//! 全部渲染旋钮变更记录：谁（调用方）、何时、旧值、新值、来源（面板、脚本、配置加载）五元组；
//! 审计流只增不改，支持按旋钮回放变更历史；与安全审计（F0105）分流独立。
//!
//! 判据：五元组、只增不改、历史回放、分流独立。

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 变更来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeSource {
    /// 面板操作（人工拖动）。
    Panel,
    /// 脚本/自动化调用。
    Script,
    /// 配置加载（启动时从配置文件读取）。
    ConfigLoad,
    /// 未知来源。
    Unknown,
}

/// 审计事件五元组。
#[derive(Debug, Clone)]
pub struct AuditEvent {
    /// 事件序号（全局单调递增，只增不改的凭据）。
    pub seq: u64,
    /// 调用方标识。
    pub caller: String,
    /// 逻辑时间戳。
    pub timestamp: u64,
    /// 旧值。
    pub old_value: f64,
    /// 新值。
    pub new_value: f64,
    /// 变更来源。
    pub source: ChangeSource,
    /// 旋钮名称。
    pub knob: String,
}

/// 审计错误码。
pub mod err {
    /// 回放范围非法。
    pub const E_BAD_RANGE: u16 = 0x7A01;
    /// 旋钮名为空。
    pub const E_BAD_KNOB: u16 = 0x7A02;
}

/// 旋钮变更审计流（只增不改）。
pub struct KnobAudit {
    /// 事件存储（追加式）。
    events: Vec<AuditEvent>,
    /// 按旋钮名索引（旋钮 → 事件序号列表）。
    knob_index: BTreeMap<String, Vec<u64>>,
    /// 当前逻辑时钟。
    clock: u64,
    /// 审计流版本号（每次追加 +1）。
    version: u64,
}

impl KnobAudit {
    /// 新建空审计流。
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            knob_index: BTreeMap::new(),
            clock: 0,
            version: 1,
        }
    }

    /// 事件总数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 当前版本号。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 记录一次变更（追加，不可修改已有事件）。
    pub fn record(
        &mut self,
        knob: &str,
        caller: &str,
        old_value: f64,
        new_value: f64,
        source: ChangeSource,
    ) -> u64 {
        self.clock += 1;
        let seq = self.events.len() as u64;
        let event = AuditEvent {
            seq,
            caller: caller.to_string(),
            timestamp: self.clock,
            old_value,
            new_value,
            source,
            knob: knob.to_string(),
        };
        self.knob_index
            .entry(knob.to_string())
            .or_default()
            .push(seq);
        self.events.push(event);
        self.version += 1;
        seq
    }

    /// 按序号取事件。
    pub fn get(&self, seq: u64) -> Option<&AuditEvent> {
        self.events.get(seq as usize)
    }

    /// 按旋钮名回放变更历史（按时间序）。
    pub fn replay(&self, knob: &str) -> Vec<&AuditEvent> {
        if knob.is_empty() {
            return Vec::new();
        }
        self.knob_index
            .get(knob)
            .map(|seqs| seqs.iter().filter_map(|&s| self.get(s)).collect())
            .unwrap_or_default()
    }

    /// 按旋钮名回放变更历史（指定范围：seq 在 [from, to] 内）。
    pub fn replay_range(&self, knob: &str, from: u64, to: u64) -> Result<Vec<&AuditEvent>, u16> {
        if from > to {
            return Err(err::E_BAD_RANGE);
        }
        Ok(self
            .replay(knob)
            .into_iter()
            .filter(|e| e.seq >= from && e.seq <= to)
            .collect())
    }

    /// 按调用方过滤事件。
    pub fn filter_by_caller(&self, caller: &str) -> Vec<&AuditEvent> {
        self.events.iter().filter(|e| e.caller == caller).collect()
    }

    /// 按来源过滤事件。
    pub fn filter_by_source(&self, source: ChangeSource) -> Vec<&AuditEvent> {
        self.events.iter().filter(|e| e.source == source).collect()
    }

    /// 列出所有被审计过的旋钮名。
    pub fn list_knobs(&self) -> Vec<&str> {
        self.knob_index.keys().map(|s| s.as_str()).collect()
    }

    /// 获取旋钮的最新值（从审计流尾部推断）。
    pub fn latest_value(&self, knob: &str) -> Option<f64> {
        self.knob_index
            .get(knob)
            .and_then(|seqs| seqs.last())
            .and_then(|&s| self.get(s))
            .map(|e| e.new_value)
    }

    /// 全量事件（只读）。
    pub fn all_events(&self) -> &[AuditEvent] {
        &self.events
    }
}

impl Default for KnobAudit {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_replay() {
        let mut a = KnobAudit::new();
        a.record("brightness", "alice", 50.0, 60.0, ChangeSource::Panel);
        a.record("brightness", "bob", 60.0, 70.0, ChangeSource::Script);
        assert_eq!(a.len(), 2);
        let history = a.replay("brightness");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].old_value, 50.0);
        assert_eq!(history[1].new_value, 70.0);
    }

    #[test]
    fn test_append_only() {
        let mut a = KnobAudit::new();
        let seq = a.record("k", "c", 0.0, 1.0, ChangeSource::Panel);
        let ev = a.get(seq).unwrap();
        assert_eq!(ev.seq, 0);
        // 不可修改：只能追加
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn test_replay_range() {
        let mut a = KnobAudit::new();
        for i in 0..5 {
            a.record("k", "c", i as f64, (i + 1) as f64, ChangeSource::Script);
        }
        let r = a.replay_range("k", 1, 3).unwrap();
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].seq, 1);
        assert_eq!(r[2].seq, 3);
    }

    #[test]
    fn test_bad_range_rejected() {
        let a = KnobAudit::new();
        assert!(matches!(a.replay_range("k", 5, 3), Err(err::E_BAD_RANGE)));
    }

    #[test]
    fn test_filter_by_source() {
        let mut a = KnobAudit::new();
        a.record("k", "c", 0.0, 1.0, ChangeSource::Panel);
        a.record("k", "c", 1.0, 2.0, ChangeSource::Script);
        a.record("k", "c", 2.0, 3.0, ChangeSource::ConfigLoad);
        assert_eq!(a.filter_by_source(ChangeSource::Panel).len(), 1);
        assert_eq!(a.filter_by_source(ChangeSource::Script).len(), 1);
    }

    #[test]
    fn test_latest_value() {
        let mut a = KnobAudit::new();
        a.record("k", "c", 0.0, 10.0, ChangeSource::Panel);
        a.record("k", "c", 10.0, 20.0, ChangeSource::Panel);
        assert_eq!(a.latest_value("k"), Some(20.0));
        assert_eq!(a.latest_value("none"), None);
    }

    #[test]
    fn test_five_tuple_complete() {
        let mut a = KnobAudit::new();
        a.record("k", "caller_x", 1.0, 2.0, ChangeSource::Panel);
        let ev = a.get(0).unwrap();
        assert_eq!(ev.caller, "caller_x");
        assert_eq!(ev.old_value, 1.0);
        assert_eq!(ev.new_value, 2.0);
        assert_eq!(ev.source, ChangeSource::Panel);
        assert!(ev.timestamp > 0);
    }
}
