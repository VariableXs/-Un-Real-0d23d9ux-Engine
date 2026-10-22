//! CGPU-F0168 · 帧图与 GPU 队列映射（CGPU-B 域 · B02 组）。
//! 锚点：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0168`；判据（锚点原文）：队列映射正确、多 GPU 路由、约束校验、映射开销、三队列用例。
//! 子模块：`mapping`（诊断码/类型规则/设备面/节点模型/映射引擎）、`repair`（校验/开销账/搬运/路由/负载）、`ledger`（用例/账本/管线/回放/遥测）；pub 路径经 re-export 原样保留。

mod ledger;
mod mapping;
mod repair;

pub use ledger::*;
pub use mapping::*;
pub use repair::*;

/// 复用对拍锚点（判据侧逐条 grep 的锚字面量——单一来源，只引用不重定义）。
pub const REUSE_ANCHORS: [&str; 4] = [
    "cgb07_execfail::TaskNode 拓扑序执行面——映射在拓扑序之后",
    "F0106 放置策略——资源所在卡为路由依据",
    "VE-F0006 队列调度器——三队列语义不另立",
    "F2408 预算行口径——步=0.1% 帧预算",
];
