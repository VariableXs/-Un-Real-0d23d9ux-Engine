//! TRINITY-500 · W1 三系统切换编排层
//!
//! * [`bootnext`] — AI-01 三系统引导与切换域（F001~F025）：Limine 三入口菜单、
//!   BootNext 切换原语、ESP 变量持久化、引导器降级链、引导完整性/自检/回滚。
//! * [`hibernate`] — AI-02 电源与休眠安全域（F026~F050）：Windows↔Varix 休眠
//!   切换闭环、休眠文件加密、掉电安全、切换状态机与预算。
//!
//! 纯逻辑 + 固定容量数组，无分配器；自检经
//! [`crate::checks::CheckSet`] 登记，供全系统闭环（F500）汇总。

pub mod bootnext;
pub mod hibernate;
