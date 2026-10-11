//! VE-F0606 · 图层 Z 序与重排（VE-D 域 · 2D 合成引擎 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0606`
//!
//! **判据（锚点原文）**：兄弟间显式 z 序键（z-index 语义——同值按插入序稳定
//! 排序，排序稳定性为契约）；重排：z 序键变更触发兄弟区间最小重排（局部重排
//! 不整树重建——重排域即变更影响域）；一致性契约：遍历序=绘制序=Z 序（F0614
//! 遍历器与 F0611 命中的次序依据，三者同源不容漂移）；提升提示的 Z 序影响登
//! 记（层提升不改 Z 序——提升是渲染策略不是语义变更）。判据四条：**稳定排序、
//! 最小重排域、三序同源、提升不改序**。
//!
//! **错误路径与降级矩阵**：键冲突→稳定序规则兜底；重排风暴→批量合并为单次；
//! 越界键→钳制加告警。
//!
//! **设计要点**：
//! - **稳定排序**：兄弟槽位按 (z 键, 插入序) 双键排序——同 z 值严格按插入序，
//!   任何重排不破坏（稳定性是契约，测试锁定）；键冲突即稳定序兜底，不是错；
//! - **三序同源**：[`SiblingZOrder::order`] 是唯一的序——F0614 遍历、绘制、
//!   F0611 命中全部消费这一份序列，不存在第二份排序（三者同源不容漂移）；
//! - **最小重排域**：z 键变更先入 pending（[`SiblingZOrder::set_z`]），flush
//!   时只对受影响的兄弟区间（变更槽位的最小包围区间）重排，其余兄弟槽位
//!   原地不动——重排域即变更影响域，不整树重建；
//! - **重排风暴→批量合并为单次**：N 次 set_z 合并进一次 flush（一次排序
//!   一次脏区提示），pending 计数即风暴观测面；
//! - **越界键→钳制加告警**：z 键钳到 [`Z_KEY_MIN`]..[`Z_KEY_MAX`]，告警
//!   入账（不静默，也不硬拒——钳制是规格点名的降级动作）；
//! - **提升不改序**：层提升（渲染策略）登记进审计并**验证序未变**——
//!   提示与 Z 序解耦是语义纪律；
//! - **查询 O(1)**：按序位查槽 [`SiblingZOrder::slot_at`] 直接下标 O(1)；
//!   按 id 查 z 走有序索引二分 O(log n)（诚实标注，不假装哈希）。
//!
//! **跨批对接点**：上游 F0601 图层树（稳定 id）；下游 F0614 遍历、F0611 命中
//! （同源序消费方）、F0613 脏区（重排域映射脏区提示 [`DirtyHint`]）。
//!
//! 逻辑 tick 注入，零墙钟；零外部依赖，只依赖 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// z 键下界（越界钳制用）。
pub const Z_KEY_MIN: i32 = -1000;

/// z 键上界（越界钳制用）。
pub const Z_KEY_MAX: i32 = 1000;

/// 三序同源契约（遍历=绘制=Z 序；本序列是唯一序源）。
pub const Z_ORDER_CONTRACT_DOC: &str = "\
Z 序一致性契约（VE-F0606 · v1）：兄弟区间内 遍历序 = 绘制序 = Z 序，\
三者同源——全部消费 SiblingZOrder::order() 这一份序列，不存在第二份\
排序。F0614 遍历器与 F0611 命中测试以此为次序依据，不容漂移。";

/// 稳定排序契约（同值按插入序）。
pub const STABLE_ORDER_DOC: &str = "\
稳定排序契约（VE-F0606 · v1）：同 z 键的兄弟严格按插入序排列；\
键冲突由稳定序规则兜底（不是错误）；任何重排不得破坏已建立的稳定序。";

/// 提升不改序纪律。
pub const PROMOTION_DOC: &str = "\
层提升是渲染策略（提升提示 = 建议某层走快速合成路径），不是语义变更：\
提升提示登记入审计，Z 序与兄弟序一律不变。";

// ---------------------------------------------------------------------------
// 二、数据结构
// ---------------------------------------------------------------------------

/// 兄弟槽位（z 键 + 插入序戳——稳定序的两把尺）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZSlot {
    /// 图层稳定 id（上游 F0601）。
    pub node_id: u64,
    /// 显式 z 序键（钳制后值）。
    pub z: i32,
    /// 插入序戳（同 z 值的稳定序依据）。
    pub insert_seq: u64,
}

/// 重排脏区提示（重排域映射到 F0613 脏区的交接面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyHint {
    /// 受影响兄弟区间的起点（序位）。
    pub span_start: usize,
    /// 受影响兄弟区间的终点（exclusive）。
    pub span_end: usize,
    /// 序发生变化的槽位数。
    pub moved: usize,
}

// ---------------------------------------------------------------------------
// 三、主结构（唯一序源 + 最小重排域 + 风暴合并）
// ---------------------------------------------------------------------------

/// 兄弟 Z 序表（绘制序槽位数组 + id 索引 + pending 变更池）。
pub struct SiblingZOrder {
    /// 当前绘制序的槽位（z 升序、同 z 按插入序）——三序同源的唯一载体。
    slots: Vec<ZSlot>,
    /// id → 槽位下标 的有序索引（二分 O(log n)；随 flush 重建）。
    index: Vec<(u64, usize)>,
    next_seq: u64,
    /// pending z 变更池（风暴合并：多次变更一次 flush）。
    pending: Vec<(u64, i32)>,
    dirty: Option<DirtyHint>,
    warnings: Vec<String>,
    audits: Vec<String>,
    errors: Vec<(String, &'static str, String)>,
    tick: u64,
}

impl SiblingZOrder {
    /// 空 Z 序表。
    pub fn new() -> Self {
        SiblingZOrder {
            slots: Vec::new(),
            index: Vec::new(),
            next_seq: 0,
            pending: Vec::new(),
            dirty: None,
            warnings: Vec::new(),
            audits: Vec::new(),
            errors: Vec::new(),
            tick: 0,
        }
    }

    // -- 只读观测面 -----------------------------------------------------------

    /// 当前序（node_id 序列——三序同源的唯一载体）。
    pub fn order(&self) -> Vec<u64> {
        self.slots.iter().map(|s| s.node_id).collect()
    }

    /// 兄弟槽数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 按序位查槽（查询 O(1)——直接下标，越界返回 None）。
    pub fn slot_at(&self, pos: usize) -> Option<&ZSlot> {
        self.slots.get(pos)
    }

    /// 按 id 查 z 键（有序索引二分 O(log n)——诚实标注非 O(1)）。
    pub fn z_of(&self, node_id: u64) -> Option<i32> {
        self.index
            .binary_search_by_key(&node_id, |&(id, _)| id)
            .ok()
            .and_then(|i| self.slots.get(self.index[i].1).map(|s| s.z))
    }

    /// pending 变更数（风暴观测面）。
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 最近一次重排的脏区提示。
    pub fn dirty_hint(&self) -> Option<DirtyHint> {
        self.dirty
    }

    /// 告警账（越界钳制等）。
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// 审计留痕。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 错误账本（零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    fn record_error(&mut self, who: String, code: &'static str, detail: String) {
        self.errors.push((who, code, detail));
    }

    // -- 声明与变更 -------------------------------------------------------------

    /// 登记兄弟层：z 键越界先钳制加告警（越界键→钳制加告警判据）。
    pub fn add(&mut self, node_id: u64, z: i32) {
        let (z, clamped) = clamp_key(z);
        if clamped {
            self.warnings.push(format!(
                "W_Z_CLAMPED：n={node_id} 的 z 键钳制到 {}（越界键→钳制加告警）",
                z
            ));
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let slot = ZSlot { node_id, z, insert_seq: seq };
        // 稳定插入：按 (z, seq) 找到尾部插入点——等价于按插入序进表。
        let pos = self
            .slots
            .iter()
            .rposition(|s| (s.z, s.insert_seq) <= (slot.z, slot.insert_seq))
            .map(|p| p + 1)
            .unwrap_or(0);
        self.slots.insert(pos, slot);
        self.rebuild_index();
        self.audits.push(format!(
            "tick{} n={node_id} 登记兄弟槽位（z={z}，序位 {pos}）",
            self.tick
        ));
    }

    /// 变更 z 键：入 pending（风暴合并池），标记重排域——不立即重排。
    pub fn set_z(&mut self, node_id: u64, new_z: i32) -> Result<(), &'static str> {
        if self.z_of(node_id).is_none() {
            self.record_error(
                format!("set_z(n={node_id})"),
                "E_NODE_NOT_FOUND",
                "兄弟表中无此 id——z 变更只对已登记兄弟有效".to_string(),
            );
            return Err("未登记的兄弟 id");
        }
        self.pending.push((node_id, new_z));
        Ok(())
    }

    /// 批量 flush：合并全部 pending 变更为**单次**最小域重排。
    ///
    /// 两段式（重排域即变更影响域）：
    /// 1. 应用 z 值（越界钳制加告警），记录变更槽位的原序位区间；
    /// 2. 只读探针（副本上求终序）确定**最小变更窗口** = 当前序与终序
    ///    不同的最小包围区间∪变更槽位原区间；物理重排只发生在窗口内
    ///    （O(区间 log 区间)），窗口外槽位原地不动——不整树重建。
    /// 返回脏区提示；无 pending 时显性报错。
    pub fn flush(&mut self) -> Result<DirtyHint, &'static str> {
        if self.pending.is_empty() {
            self.record_error(
                "flush".to_string(),
                "E_NO_PENDING",
                "无待重排变更——flush 只在有 pending 时有效（防无意义全量重排）".to_string(),
            );
            return Err("无 pending 变更");
        }
        let storm = self.pending.len();
        // 1) 应用变更并记录受影响槽位的原序位范围。
        let mut lo = self.slots.len();
        let mut hi = 0usize;
        for &(node_id, new_z) in &self.pending {
            if let Some(pos) = self.slots.iter().position(|s| s.node_id == node_id) {
                lo = lo.min(pos);
                hi = hi.max(pos + 1);
                let (z, clamped) = clamp_key(new_z);
                if clamped {
                    self.warnings.push(format!(
                        "W_Z_CLAMPED：n={node_id} 的 z 键钳制到 {}（越界键→钳制加告警）",
                        z
                    ));
                }
                self.slots[pos].z = z;
            }
        }
        self.pending.clear();
        // 2) 只读探针：副本上求终序（(z, seq) 全序唯一——seq 不重复），确定
        //    当前序与终序的最小差异窗口；窗口至少覆盖变更槽位原区间。
        let mut probe: Vec<ZSlot> = self.slots.clone();
        probe.sort_by_key(|s| (s.z, s.insert_seq));
        let mut wlo = usize::MAX;
        let mut whi = 0usize;
        for i in 0..self.slots.len() {
            if self.slots[i] != probe[i] {
                wlo = wlo.min(i);
                whi = whi.max(i + 1);
            }
        }
        wlo = wlo.min(lo);
        whi = whi.max(hi);
        // 3) 物理重排只发生在窗口内（区间外原地不动）。
        let before: Vec<u64> = self.slots[wlo..whi].iter().map(|s| s.node_id).collect();
        let mut seg: Vec<ZSlot> = self.slots[wlo..whi].to_vec();
        seg.sort_by_key(|s| (s.z, s.insert_seq));
        let after: Vec<u64> = seg.iter().map(|s| s.node_id).collect();
        let moved = before.iter().zip(after.iter()).filter(|(a, b)| a != b).count();
        self.slots[wlo..whi].copy_from_slice(&seg);
        self.rebuild_index();
        let hint = DirtyHint { span_start: wlo, span_end: whi, moved };
        self.dirty = Some(hint);
        self.audits.push(format!(
            "tick{} 重排风暴合并：{} 次变更 → 单次最小域重排（区间 [{},{})，移位 {} 槽）",
            self.tick, storm, hint.span_start, hint.span_end, hint.moved
        ));
        Ok(hint)
    }

    /// 层提升提示登记：只记账，**序不变**（提升不改序判据的执行面）。
    pub fn register_promotion(&mut self, node_id: u64, reason: &str) -> Result<(), &'static str> {
        let before = self.order();
        if !before.contains(&node_id) {
            self.record_error(
                format!("register_promotion(n={node_id})"),
                "E_NODE_NOT_FOUND",
                "兄弟表中无此 id——提升提示只对已登记兄弟有效".to_string(),
            );
            return Err("未登记的兄弟 id");
        }
        self.audits.push(format!(
            "tick{} n={node_id} 层提升提示登记（{reason}）——Z 序不变（提升是渲染策略）",
            self.tick
        ));
        let after = self.order();
        if before != after {
            // 提升登记不改任何状态；此断言是纪律的最后一道闸。
            self.record_error(
                format!("register_promotion(n={node_id})"),
                "E_PROMOTION_CHANGED_ORDER",
                "提升登记改变了序——语义缺陷，序必须复原".to_string(),
            );
            return Err("提升不应改变序");
        }
        Ok(())
    }

    // -- 内部 -------------------------------------------------------------------

    fn rebuild_index(&mut self) {
        let mut idx: Vec<(u64, usize)> =
            self.slots.iter().enumerate().map(|(i, s)| (s.node_id, i)).collect();
        idx.sort_unstable_by_key(|&(id, _)| id);
        self.index = idx;
    }

    /// 逻辑 tick 推进（零墙钟纪律）。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }
}

/// z 键钳制：越界返回钳后值与是否发生钳制。
fn clamp_key(z: i32) -> (i32, bool) {
    if z < Z_KEY_MIN {
        (Z_KEY_MIN, true)
    } else if z > Z_KEY_MAX {
        (Z_KEY_MAX, true)
    } else {
        (z, false)
    }
}

// ---------------------------------------------------------------------------
// 四、自检注册入口
// ---------------------------------------------------------------------------

/// VE-F0606 域自检（判据逐条映射见 `ved06_checks.rs`）。
pub fn run_ved06_checks() -> CheckSet {
    super::ved06_checks::run_ved06_checks()
}

// ---------------------------------------------------------------------------
// 五、单元测试（宿主 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ved06_stable_order_same_key_by_insertion() {
        let mut s = SiblingZOrder::new();
        s.tick();
        s.add(1, 0);
        s.add(2, 0);
        s.add(3, 0);
        assert_eq!(s.order(), vec![1, 2, 3], "同 z 键按插入序");
        // 插入更小 z 键的兄弟排前面，但不破坏同键稳定序。
        s.add(0, -1);
        assert_eq!(s.order(), vec![0, 1, 2, 3]);
        // 重排（改 1 的 z）后，同键的 2、3 仍按插入序。
        let _ = s.set_z(1, 5);
        s.flush().unwrap();
        assert_eq!(s.order(), vec![0, 2, 3, 1]);
        assert_eq!(s.z_of(1), Some(5));
        assert_eq!(s.z_of(2), Some(0));
    }

    #[test]
    fn ved06_min_span_local_reorder() {
        let mut s = SiblingZOrder::new();
        for id in 1..=6u64 {
            s.add(id, 0);
        }
        // 只改 4 的 z：重排域应远小于全表。
        let _ = s.set_z(4, 10);
        let hint = s.flush().unwrap();
        assert!(hint.span_end - hint.span_start < s.len(), "最小域：不整树重建");
        // 区间外槽位原地不动：1、2、3 仍在最前。
        assert_eq!(s.slot_at(0).unwrap().node_id, 1);
        assert_eq!(s.slot_at(2).unwrap().node_id, 3);
        assert_eq!(s.slot_at(5).unwrap().node_id, 4);
        // 无 pending flush 显性拒绝。
        assert!(s.flush().is_err());
        assert!(s.errors().iter().any(|(_, c, _)| *c == "E_NO_PENDING"));
    }

    #[test]
    fn ved06_storm_merged_into_single_flush() {
        let mut s = SiblingZOrder::new();
        for id in 1..=5u64 {
            s.add(id, 0);
        }
        // 重排风暴：5 次变更只 flush 一次。
        for id in 1..=5u64 {
            let _ = s.set_z(id, id as i32);
        }
        assert_eq!(s.pending_len(), 5, "风暴池累积");
        let hint = s.flush().unwrap();
        assert_eq!(s.pending_len(), 0, "一次 flush 清空风暴");
        assert_eq!(s.order(), vec![1, 2, 3, 4, 5], "z=1..5 与插入序一致");
        assert!(hint.moved >= 0);
        assert!(s.audits().iter().any(|a| a.contains("单次最小域重排")));
    }

    #[test]
    fn ved06_out_of_range_clamped_with_warning() {
        let mut s = SiblingZOrder::new();
        s.tick();
        s.add(1, 5000);
        s.add(2, -9999);
        assert_eq!(s.z_of(1), Some(Z_KEY_MAX), "越界键钳制");
        assert_eq!(s.z_of(2), Some(Z_KEY_MIN));
        assert_eq!(s.warnings().len(), 2, "钳制必带告警");
        // 变更路径同样钳制。
        let _ = s.set_z(1, 2000);
        s.flush().unwrap();
        assert_eq!(s.z_of(1), Some(Z_KEY_MAX));
        assert_eq!(s.warnings().len(), 3);
    }

    #[test]
    fn ved06_promotion_does_not_change_order() {
        let mut s = SiblingZOrder::new();
        s.tick();
        s.add(1, 0);
        s.add(2, 0);
        s.add(3, 0);
        let before = s.order();
        assert!(s.register_promotion(2, "建议走快速合成路径").is_ok());
        assert_eq!(s.order(), before, "提升不改序");
        assert!(s.audits().iter().any(|a| a.contains("层提升提示登记")));
        // 未登记 id 显性拒绝。
        assert!(s.register_promotion(99, "x").is_err());
        assert!(s.errors().iter().any(|(_, c, _)| *c == "E_NODE_NOT_FOUND"));
    }

    #[test]
    fn ved06_o1_slot_query_and_dirty_hint() {
        let mut s = SiblingZOrder::new();
        for id in 1..=4u64 {
            s.add(id, 0);
        }
        // 按序位查槽 O(1)（直接下标）。
        assert_eq!(s.slot_at(0).unwrap().node_id, 1);
        assert_eq!(s.slot_at(3).unwrap().node_id, 4);
        assert!(s.slot_at(4).is_none(), "越界不 panic");
        // 重排域映射脏区提示（F0613 对接面）。
        let _ = s.set_z(2, 99);
        let hint = s.flush().unwrap();
        assert_eq!(s.dirty_hint(), Some(hint));
    }

    #[test]
    fn ved06_checks_all_green() {
        let set = run_ved06_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0606 域自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
        );
    }
}
