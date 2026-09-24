//! 交接协议（MD2 篇 2 · WP-102）——双域接力的全部数据面与控制面。
//!
//! # 模块地图
//!
//! - [`state`]：五状态机七迁移（表驱动，任何中间态都有明确出口）。
//! - [`snap`]：handoff.json 快照 schema（字段冻结 = 全链第一闸，WP-22x
//!   助手与 WP-203 存储快照都在等这张表）。
//! - [`flush`]：冲刷管线（四步硬顺序 + 15s 预算）与 WD-040 五步时序账。
//! - [`arming`]：武装序列编排（闸门三条件 → 写 OneShot → 十次读回 →
//!   降级 BootNext → aborted 人话）。
//! - [`screen`]：四帧交接画面（全程有画面、全程有进度、全程说实话）。
//! - [`legacy`]：需求 2 时代的 BootNext+ResetSystem 直通路径（A 卡菜单
//!   选 Variable），原样保留——它是菜单决策面，不是本协议的对端。
//!
//! # 与图纸的对表纪律
//!
//! 篇 2.2 是 handoff.json 字段的唯一权威定义：本模块的结构体、Windows
//! 侧参考实现（`portable/engine/handoff/vx_handoff_proto.py`）、测试夹具
//! 三处与它对表；字段一经定案**只增不改不删**（向后兼容红线），新增字段
//! 走小修订（schema_version 递增），语义变更走 ADR。
//!
//! # 状态计数口径（篇 2.1 的"五个状态、七个迁移"）
//!
//! 正向主干五步 = idle → preserving → flushing → arming → rebooting；
//! aborted 是 arming 失败的错误分支状态（计入枚举、不计入主干）。
//! 七迁移 = 主干四边 + preserving 取消边 + arming 失败边 + aborted 确认边。
//! 零个无出口状态：Rebooting 的出口是物理重启（协议终点事件），Aborted
//! 的出口是用户确认（AbortAck），其余状态均有事件出口。

pub mod arming;
pub mod flush;
pub mod legacy;
pub mod screen;
pub mod snap;
pub mod state;

pub use legacy::{draw_prompt, plan, run, HandoffPlan, PROMPT_MS, PROMPT_SLICES};
pub use state::{HandoffMachine, HEvent, HState, TRANSITIONS};
