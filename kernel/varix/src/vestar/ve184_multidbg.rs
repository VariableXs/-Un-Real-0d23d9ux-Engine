//! ve184_multidbg ↔ VE-F0184 多实例并发调试
//!
//! 多实例并行运行的调试通道隔离：实例 ID 全路由（日志、截图、调试命令按实例隔离不串台），
//! 跨实例对照视图（同指标并列对比）；实例生命周期事件（启停崩溃）入统一时间轴。
//!
//! 判据：ID 路由、通道隔离、跨例对照、生命周期。

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 通道类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Channel {
    /// 日志通道。
    Log,
    /// 截图通道。
    Screenshot,
    /// 调试命令通道。
    DebugCommand,
}

/// 生命周期事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    /// 实例启动。
    Started,
    /// 实例停止。
    Stopped,
    /// 实例崩溃。
    Crashed,
}

/// 实例路由表条目。
#[derive(Debug, Clone)]
pub struct InstanceRoute {
    /// 实例 ID。
    pub id: String,
    /// 是否活跃。
    pub active: bool,
    /// 通道消息计数（按通道）。
    pub channel_counts: BTreeMap<Channel, u64>,
}

/// 生命周期事件记录。
#[derive(Debug, Clone)]
pub struct LifecycleRecord {
    /// 实例 ID。
    pub instance_id: String,
    /// 事件类型。
    pub event: LifecycleEvent,
    /// 逻辑时间戳。
    pub timestamp: u64,
    /// 附加信息。
    pub detail: String,
}

/// 多实例调试错误码。
pub mod err {
    /// 实例 ID 冲突。
    pub const E_ID_CONFLICT: u16 = 0x7D01;
    /// 实例不存在。
    pub const E_NO_INSTANCE: u16 = 0x7D02;
    /// 实例已停止。
    pub const E_STOPPED: u16 = 0x7D03;
    /// 对照实例缺格。
    pub const E_MISSING_CELL: u16 = 0x7D04;
}

/// 跨实例对照视图数据。
#[derive(Debug, Clone)]
pub struct CompareView {
    /// 指标名。
    pub metric: String,
    /// 各实例的值（实例 ID → 值）。
    pub values: BTreeMap<String, Option<f64>>,
}

/// 多实例并发调试器。
pub struct MultiInstanceDebugger {
    /// 实例路由表。
    routes: BTreeMap<String, InstanceRoute>,
    /// 生命周期事件流。
    events: Vec<LifecycleRecord>,
    /// 逻辑时钟。
    clock: u64,
}

impl MultiInstanceDebugger {
    /// 新建调试器。
    pub fn new() -> Self {
        Self {
            routes: BTreeMap::new(),
            events: Vec::new(),
            clock: 0,
        }
    }

    /// 启动实例（ID 冲突拒绝）。
    pub fn start_instance(&mut self, id: &str) -> Result<(), u16> {
        if id.is_empty() {
            return Err(err::E_NO_INSTANCE);
        }
        if self.routes.contains_key(id) {
            return Err(err::E_ID_CONFLICT);
        }
        self.routes.insert(
            id.to_string(),
            InstanceRoute {
                id: id.to_string(),
                active: true,
                channel_counts: BTreeMap::new(),
            },
        );
        self.record_event(id, LifecycleEvent::Started, "instance started");
        Ok(())
    }

    /// 停止实例。
    pub fn stop_instance(&mut self, id: &str) -> Result<(), u16> {
        let route = self.routes.get_mut(id).ok_or(err::E_NO_INSTANCE)?;
        route.active = false;
        self.record_event(id, LifecycleEvent::Stopped, "instance stopped");
        Ok(())
    }

    /// 崩溃实例（隔离记录，不影响他实例）。
    pub fn crash_instance(&mut self, id: &str, reason: &str) -> Result<(), u16> {
        let route = self.routes.get_mut(id).ok_or(err::E_NO_INSTANCE)?;
        route.active = false;
        self.record_event(id, LifecycleEvent::Crashed, reason);
        Ok(())
    }

    /// 路由消息到指定实例的指定通道。
    pub fn route_message(&mut self, id: &str, channel: Channel) -> Result<u64, u16> {
        let route = self.routes.get_mut(id).ok_or(err::E_NO_INSTANCE)?;
        if !route.active {
            return Err(err::E_STOPPED);
        }
        let count = route.channel_counts.entry(channel).or_insert(0);
        *count += 1;
        Ok(*count)
    }

    /// 获取实例通道计数。
    pub fn channel_count(&self, id: &str, channel: Channel) -> Option<u64> {
        self.routes
            .get(id)
            .and_then(|r| r.channel_counts.get(&channel))
            .copied()
    }

    /// 列出所有活跃实例。
    pub fn active_instances(&self) -> Vec<&str> {
        self.routes
            .values()
            .filter(|r| r.active)
            .map(|r| r.id.as_str())
            .collect()
    }

    /// 列出所有实例。
    pub fn all_instances(&self) -> Vec<&InstanceRoute> {
        self.routes.values().collect()
    }

    /// 记录生命周期事件。
    fn record_event(&mut self, id: &str, event: LifecycleEvent, detail: &str) {
        self.clock += 1;
        self.events.push(LifecycleRecord {
            instance_id: id.to_string(),
            event,
            timestamp: self.clock,
            detail: detail.to_string(),
        });
    }

    /// 获取生命周期事件流（按时间序）。
    pub fn timeline(&self) -> &[LifecycleRecord] {
        &self.events
    }

    /// 获取指定实例的生命周期事件。
    pub fn instance_timeline(&self, id: &str) -> Vec<&LifecycleRecord> {
        self.events.iter().filter(|e| e.instance_id == id).collect()
    }

    /// 跨实例对照：同指标并列对比（缺格显示 None）。
    pub fn compare(&self, metric: &str, values: BTreeMap<String, f64>) -> CompareView {
        let mut cells = BTreeMap::new();
        for id in self.routes.keys() {
            cells.insert(id.clone(), values.get(id).copied());
        }
        CompareView {
            metric: metric.to_string(),
            values: cells,
        }
    }

    /// 对照视图中是否有缺格。
    pub fn has_missing_cells(view: &CompareView) -> bool {
        view.values.values().any(|v| v.is_none())
    }
}

impl Default for MultiInstanceDebugger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_start_and_isolate() {
        let mut d = MultiInstanceDebugger::new();
        d.start_instance("inst_a").unwrap();
        d.start_instance("inst_b").unwrap();
        assert_eq!(d.active_instances().len(), 2);
        // 崩溃实例 a 不影响 b
        d.crash_instance("inst_a", "segfault").unwrap();
        assert_eq!(d.active_instances().len(), 1);
        assert_eq!(d.active_instances()[0], "inst_b");
    }

    #[test]
    fn test_id_conflict() {
        let mut d = MultiInstanceDebugger::new();
        d.start_instance("x").unwrap();
        assert_eq!(d.start_instance("x"), Err(err::E_ID_CONFLICT));
    }

    #[test]
    fn test_channel_routing() {
        let mut d = MultiInstanceDebugger::new();
        d.start_instance("a").unwrap();
        d.route_message("a", Channel::Log).unwrap();
        d.route_message("a", Channel::Log).unwrap();
        d.route_message("a", Channel::Screenshot).unwrap();
        assert_eq!(d.channel_count("a", Channel::Log), Some(2));
        assert_eq!(d.channel_count("a", Channel::Screenshot), Some(1));
    }

    #[test]
    fn test_stopped_instance_rejects() {
        let mut d = MultiInstanceDebugger::new();
        d.start_instance("a").unwrap();
        d.stop_instance("a").unwrap();
        assert_eq!(d.route_message("a", Channel::Log), Err(err::E_STOPPED));
    }

    #[test]
    fn test_compare_with_missing() {
        let mut d = MultiInstanceDebugger::new();
        d.start_instance("a").unwrap();
        d.start_instance("b").unwrap();
        let mut vals = BTreeMap::new();
        vals.insert("a".to_string(), 60.0);
        // b 缺格
        let view = d.compare("fps", vals);
        assert!(MultiInstanceDebugger::has_missing_cells(&view));
        assert_eq!(view.values.get("a"), Some(&Some(60.0)));
        assert_eq!(view.values.get("b"), Some(&None));
    }

    #[test]
    fn test_lifecycle_timeline() {
        let mut d = MultiInstanceDebugger::new();
        d.start_instance("a").unwrap();
        d.stop_instance("a").unwrap();
        let tl = d.timeline();
        assert_eq!(tl.len(), 2);
        assert_eq!(tl[0].event, LifecycleEvent::Started);
        assert_eq!(tl[1].event, LifecycleEvent::Stopped);
    }
}
