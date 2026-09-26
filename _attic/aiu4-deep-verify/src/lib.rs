//! AI-U4 深化批次验证舱（_attic · 非功能件）：主树处于多 AI 并发施工态
//! （他域深化批使全 crate 暂不可编译——V1 批 svstar/vxapp.rs 缺 alloc
//! 导入在途），本舱以 `#[path]` 直挂 checks.rs + istar/ 全域（含 deep/
//! 深化层），隔离验证深化批次的 CheckSet 与单测全绿。直挂优于拷贝：
//! 舱内代码与仓库源零漂移，验证结论即仓库源结论。

extern crate alloc;

#[path = "../../../kernel/varix/src/checks.rs"]
pub mod checks;

#[path = "../../../kernel/varix/src/istar/mod.rs"]
pub mod istar;
