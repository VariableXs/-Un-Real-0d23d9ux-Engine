//! AI-K1 深化批次验证舱（_attic · 非功能件）：工作树处于多 AI 并发施工态
//! （他域在途修改使全 crate 暂不可编译），本舱隔离编译 perfstar 17 域，
//! 验证深化批次的 CheckSet 与单测全绿。验证后即归档，不入版本库构建链。

pub mod checks;
pub mod perfstar;
