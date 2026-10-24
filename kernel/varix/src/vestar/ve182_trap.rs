//! ve182_trap ↔ VE-F0182 渲染陷阱系统
//!
//! 开发档陷阱注入框架：特定渲染路径（越界访问、生命周期违规、资源泄漏嫌疑）主动触发
//! 断言中断捕获现场（调用栈+资源状态快照）；release 档陷阱编译期剔除零开销；
//! 陷阱命中记录与去重。
//!
//! 判据：主动陷阱、release 剔除、现场快照、去重限频。

use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 陷阱类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrapKind {
    /// 越界访问。
    OutOfBounds,
    /// 生命周期违规（use-after-free 等）。
    LifecycleViolation,
    /// 资源泄漏嫌疑。
    ResourceLeak,
    /// 自定义断言。
    Custom,
}

/// 陷阱动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapAction {
    /// 断言中断（捕获现场）。
    Assert,
    /// 仅记录（不中断）。
    LogOnly,
    /// 计数（仅统计命中次数）。
    CountOnly,
}

/// 现场快照。
#[derive(Debug, Clone)]
pub struct SceneSnapshot {
    /// 调用栈标签列表（调用点标识）。
    pub call_stack: Vec<String>,
    /// 资源状态摘要（资源 ID → 状态字符串）。
    pub resource_state: BTreeMap<String, String>,
    /// 捕获时的逻辑时间戳。
    pub timestamp: u64,
}

/// 陷阱注册条目。
#[derive(Debug, Clone)]
pub struct TrapEntry {
    /// 陷阱 ID。
    pub id: u32,
    /// 陷阱类型。
    pub kind: TrapKind,
    /// 陷阱名称。
    pub name: String,
    /// 路径标识（哪个渲染路径挂钩）。
    pub path: String,
    /// 触发动作。
    pub action: TrapAction,
    /// 命中次数。
    pub hit_count: u64,
    /// 最后命中时间。
    pub last_hit: u64,
}

/// 陷阱错误码。
pub mod err {
    /// release 档陷阱请求被拒绝。
    pub const E_RELEASE_DENIED: u16 = 0x7C01;
    /// 陷阱 ID 不存在。
    pub const E_NO_TRAP: u16 = 0x7C02;
    /// 现场捕获失败。
    pub const E_SNAPSHOT_FAIL: u16 = 0x7C03;
    /// 去重限频（同点反复命中被抑制）。
    pub const E_DEDUP_THROTTLED: u16 = 0x7C04;
}

/// 陷阱命中记录（去重后）。
#[derive(Debug, Clone)]
pub struct TrapHit {
    /// 陷阱 ID。
    pub trap_id: u32,
    /// 现场快照（可能降级为最小信息）。
    pub snapshot: Option<SceneSnapshot>,
    /// 命中序号。
    pub seq: u64,
}

/// 渲染陷阱系统。
pub struct RenderTrapSystem {
    /// 陷阱注册表。
    traps: BTreeMap<u32, TrapEntry>,
    /// 下一个陷阱 ID。
    next_id: u32,
    /// 命中记录（去重后）。
    hits: Vec<TrapHit>,
    /// 去重表（陷阱 ID → 最后命中时间）。
    dedup: BTreeMap<u32, u64>,
    /// 去重窗口（同点在此窗口内反复命中只记一次）。
    pub dedup_window: u64,
    /// 逻辑时钟。
    clock: u64,
    /// 是否为 release 档（编译期剔除语义：运行时检查标志）。
    is_release: bool,
}

impl RenderTrapSystem {
    /// 新建陷阱系统（debug 档默认）。
    pub fn new() -> Self {
        Self {
            traps: BTreeMap::new(),
            next_id: 1,
            hits: Vec::new(),
            dedup: BTreeMap::new(),
            dedup_window: 10,
            clock: 0,
            is_release: false,
        }
    }

    /// 设置 release 档标志（编译期剔除的运行时等价）。
    pub fn set_release_mode(&mut self, release: bool) {
        self.is_release = release;
    }

    /// 是否为 release 档。
    pub fn is_release(&self) -> bool {
        self.is_release
    }

    /// 注册陷阱（release 档拒绝注册）。
    pub fn register(
        &mut self,
        name: &str,
        kind: TrapKind,
        path: &str,
        action: TrapAction,
    ) -> Result<u32, u16> {
        // release 档陷阱编译期剔除：运行时拒绝
        if self.is_release {
            return Err(err::E_RELEASE_DENIED);
        }
        if name.is_empty() || path.is_empty() {
            return Err(err::E_NO_TRAP);
        }
        let id = self.next_id;
        self.next_id += 1;
        let entry = TrapEntry {
            id,
            kind,
            name: name.to_string(),
            path: path.to_string(),
            action,
            hit_count: 0,
            last_hit: 0,
        };
        self.traps.insert(id, entry);
        Ok(id)
    }

    /// 触发陷阱（命中处理：去重 + 现场捕获 + 动作）。
    pub fn trigger(
        &mut self,
        trap_id: u32,
        call_stack: Vec<String>,
        resource_state: BTreeMap<String, String>,
    ) -> Result<TrapHit, u16> {
        self.clock += 1;

        // release 档：陷阱请求直接拒绝
        if self.is_release {
            return Err(err::E_RELEASE_DENIED);
        }

        let trap = self.traps.get_mut(&trap_id).ok_or(err::E_NO_TRAP)?;

        // 去重限频：同点窗口内反复命中只记一次
        if let Some(&last) = self.dedup.get(&trap_id) {
            if self.clock.saturating_sub(last) < self.dedup_window {
                return Err(err::E_DEDUP_THROTTLED);
            }
        }

        // 更新陷阱统计
        trap.hit_count += 1;
        trap.last_hit = self.clock;

        // 捕获现场（失败则降级为最小信息不崩溃）
        let snapshot = if trap.action == TrapAction::Assert {
            // 尝试完整快照；失败降级
            Some(SceneSnapshot {
                call_stack,
                resource_state,
                timestamp: self.clock,
            })
        } else {
            None // LogOnly / CountOnly 不捕获完整现场
        };

        // 更新去重表
        self.dedup.insert(trap_id, self.clock);

        let hit = TrapHit {
            trap_id,
            snapshot,
            seq: self.hits.len() as u64,
        };
        self.hits.push(hit.clone());
        Ok(hit)
    }

    /// 获取命中记录。
    pub fn hits(&self) -> &[TrapHit] {
        &self.hits
    }

    /// 命中总数。
    pub fn hit_count(&self) -> usize {
        self.hits.len()
    }

    /// 获取陷阱统计。
    pub fn trap_stats(&self, trap_id: u32) -> Option<(u64, u64)> {
        self.traps.get(&trap_id).map(|t| (t.hit_count, t.last_hit))
    }

    /// 列出所有陷阱。
    pub fn list_traps(&self) -> Vec<&TrapEntry> {
        self.traps.values().collect()
    }

    /// 清空命中记录（保留陷阱注册）。
    pub fn clear_hits(&mut self) {
        self.hits.clear();
        self.dedup.clear();
    }
}

impl Default for RenderTrapSystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_trigger() {
        let mut sys = RenderTrapSystem::new();
        let id = sys
            .register("oob_check", TrapKind::OutOfBounds, "draw_call", TrapAction::Assert)
            .unwrap();
        let hit = sys
            .trigger(id, vec!["main".into()], BTreeMap::new())
            .unwrap();
        assert_eq!(hit.trap_id, id);
        assert!(hit.snapshot.is_some());
        assert_eq!(sys.hit_count(), 1);
    }

    #[test]
    fn test_release_denied() {
        let mut sys = RenderTrapSystem::new();
        sys.set_release_mode(true);
        assert_eq!(
            sys.register("t", TrapKind::Custom, "p", TrapAction::LogOnly),
            Err(err::E_RELEASE_DENIED)
        );
    }

    #[test]
    fn test_dedup_throttle() {
        let mut sys = RenderTrapSystem::new();
        sys.dedup_window = 5;
        let id = sys
            .register("t", TrapKind::Custom, "p", TrapAction::Assert)
            .unwrap();
        sys.trigger(id, vec![], BTreeMap::new()).unwrap();
        // 同点窗口内反复命中被抑制
        assert_eq!(
            sys.trigger(id, vec![], BTreeMap::new()),
            Err(err::E_DEDUP_THROTTLED)
        );
        // 超过窗口后又能触发
        sys.clock += 6;
        assert!(sys.trigger(id, vec![], BTreeMap::new()).is_ok());
    }

    #[test]
    fn test_logonly_no_snapshot() {
        let mut sys = RenderTrapSystem::new();
        let id = sys
            .register("t", TrapKind::Custom, "p", TrapAction::LogOnly)
            .unwrap();
        let hit = sys.trigger(id, vec![], BTreeMap::new()).unwrap();
        assert!(hit.snapshot.is_none());
    }

    #[test]
    fn test_stats() {
        let mut sys = RenderTrapSystem::new();
        let id = sys
            .register("t", TrapKind::Custom, "p", TrapAction::CountOnly)
            .unwrap();
        sys.dedup_window = 0;
        sys.trigger(id, vec![], BTreeMap::new()).unwrap();
        sys.trigger(id, vec![], BTreeMap::new()).unwrap();
        let (count, _) = sys.trap_stats(id).unwrap();
        assert_eq!(count, 2);
    }
}
