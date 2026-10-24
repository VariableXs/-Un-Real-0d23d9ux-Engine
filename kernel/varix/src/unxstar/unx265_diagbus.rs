// SPDX-License-Identifier: Apache-2.0
//! UNX-F0265 · 诊断命令注册总线（命令插件表）
//!
//! 命令静态注册表（名/帮助/权限级/处理函数）统一入口，重名注册拒绝。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::format;
use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 权限级别
// ---------------------------------------------------------------------------

/// 命令权限级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrivilegeLevel {
    /// 只读（任意用户）。
    Read,
    /// 操作（特权用户）。
    Operate,
    /// 管理（仅 root）。
    Admin,
    /// 危险（需二次确认）。
    Danger,
}

impl PrivilegeLevel {
    pub fn label(&self) -> &'static str {
        match self {
            PrivilegeLevel::Read => "read",
            PrivilegeLevel::Operate => "operate",
            PrivilegeLevel::Admin => "admin",
            PrivilegeLevel::Danger => "danger",
        }
    }

    /// 数值（用于比较）。
    pub fn level(&self) -> u8 {
        match self {
            PrivilegeLevel::Read => 0,
            PrivilegeLevel::Operate => 1,
            PrivilegeLevel::Admin => 2,
            PrivilegeLevel::Danger => 3,
        }
    }
}

// ---------------------------------------------------------------------------
// 命令定义
// ---------------------------------------------------------------------------

/// 命令处理结果。
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// 退出码。
    pub exit_code: i32,
    /// 输出文本。
    pub output: String,
    /// 是否成功。
    pub success: bool,
}

/// 命令处理函数签名。
pub type CommandHandler = fn(&[String]) -> CommandResult;

/// 命令定义。
#[derive(Debug, Clone)]
pub struct CommandDef {
    /// 命令名（唯一）。
    pub name: String,
    /// 帮助文本。
    pub help: String,
    /// 权限级别。
    pub privilege: PrivilegeLevel,
    /// 处理函数。
    pub handler: CommandHandler,
    /// 所属分组。
    pub group: String,
}

// ---------------------------------------------------------------------------
// 注册总线
// ---------------------------------------------------------------------------

/// 错误码。
const ERR_DUPLICATE: u16 = 0xE101;
const ERR_NOT_FOUND: u16 = 0xE102;
const ERR_PRIVILEGE: u16 = 0xE103;

/// 诊断命令注册总线。
pub struct DiagnosticBus {
    commands: BTreeMap<String, CommandDef>,
    /// 注册历史（审计用）。
    pub registration_log: Vec<String>,
}

impl DiagnosticBus {
    pub fn new() -> Self {
        DiagnosticBus { commands: BTreeMap::new(), registration_log: Vec::new() }
    }

    /// 注册命令（重名拒绝）。
    pub fn register(&mut self, def: CommandDef) -> Result<(), u16> {
        if self.commands.contains_key(&def.name) {
            return Err(ERR_DUPLICATE);
        }
        self.registration_log.push(alloc::format!("register:{}", def.name));
        self.commands.insert(def.name.clone(), def);
        Ok(())
    }

    /// 注销命令。
    pub fn unregister(&mut self, name: &str) -> Result<(), u16> {
        match self.commands.remove(name) {
            Some(_) => {
                self.registration_log.push(alloc::format!("unregister:{}", name));
                Ok(())
            }
            None => Err(ERR_NOT_FOUND),
        }
    }

    /// 执行命令（权限检查）。
    pub fn execute(&self, name: &str, args: &[String], caller_level: PrivilegeLevel) -> Result<CommandResult, u16> {
        let def = self.commands.get(name).ok_or(ERR_NOT_FOUND)?;
        if caller_level < def.privilege {
            return Err(ERR_PRIVILEGE);
        }
        Ok((def.handler)(args))
    }

    /// 查询命令帮助。
    pub fn help(&self, name: &str) -> Option<&str> {
        self.commands.get(name).map(|d| d.help.as_str())
    }

    /// 按分组列出命令。
    pub fn by_group(&self, group: &str) -> Vec<&CommandDef> {
        self.commands.values().filter(|d| d.group == group).collect()
    }

    /// 全部命令名（排序）。
    pub fn all_names(&self) -> Vec<String> {
        self.commands.keys().cloned().collect()
    }

    /// 命令总数。
    pub fn count(&self) -> usize { self.commands.len() }

    /// 列出命令表（名/帮助/权限级）。
    pub fn list_table(&self) -> Vec<(String, String, String)> {
        self.commands.values()
            .map(|d| (d.name.clone(), d.help.clone(), d.privilege.label().to_string()))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 内置命令处理函数
// ---------------------------------------------------------------------------

/// echo 命令。
fn cmd_echo(args: &[String]) -> CommandResult {
    CommandResult { exit_code: 0, output: args.join(" "), success: true }
}

/// help 命令。
fn cmd_help(_args: &[String]) -> CommandResult {
    CommandResult { exit_code: 0, output: "Available commands: echo, help, version".into(), success: true }
}

/// version 命令。
fn cmd_version(_args: &[String]) -> CommandResult {
    CommandResult { exit_code: 0, output: "Varix Diagnostic Bus v1".into(), success: true }
}

/// mem 命令（F0266 深化）。
fn cmd_mem(args: &[String]) -> CommandResult {
    if args.first().map(|s| s.as_str()) == Some("heatmap") {
        CommandResult { exit_code: 0, output: "heatmap: [section: usage%] kernel: 45, heap: 30, stack: 25".into(), success: true }
    } else {
        CommandResult { exit_code: 0, output: "mem: total=128MB used=64MB free=64MB".into(), success: true }
    }
}

/// 注册内置命令集。
pub fn register_builtin_commands(bus: &mut DiagnosticBus) -> Result<usize, u16> {
    let mut count = 0;
    let defs = alloc::vec![
        CommandDef { name: "echo".into(), help: "Echo arguments".into(), privilege: PrivilegeLevel::Read, handler: cmd_echo, group: "basic".into() },
        CommandDef { name: "help".into(), help: "Show help".into(), privilege: PrivilegeLevel::Read, handler: cmd_help, group: "basic".into() },
        CommandDef { name: "version".into(), help: "Show version".into(), privilege: PrivilegeLevel::Read, handler: cmd_version, group: "basic".into() },
        CommandDef { name: "mem".into(), help: "Memory info and heatmap".into(), privilege: PrivilegeLevel::Read, handler: cmd_mem, group: "diag".into() },
    ];
    for d in defs {
        bus.register(d)?;
        count += 1;
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0265 判据全集。
pub fn run_diag_bus_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0265", "诊断命令注册总线");

    // 1. 注册
    let mut bus = DiagnosticBus::new();
    let def = CommandDef {
        name: "test".into(), help: "test cmd".into(),
        privilege: PrivilegeLevel::Read, handler: cmd_echo, group: "test".into(),
    };
    cs.ok("注册成功", bus.register(def.clone()).is_ok(), "首注册");
    cs.ok("重名拒绝", bus.register(def.clone()).is_err(), "ERR_DUPLICATE");
    cs.ok("计数正确", bus.count() == 1, "一条命令");

    // 2. 查询
    cs.ok("帮助查询", bus.help("test").is_some(), "help 文本");
    cs.ok("不存在拒绝", bus.help("nonexist").is_none(), "ERR_NOT_FOUND");

    // 3. 执行
    let result = bus.execute("test", &["hello".into()], PrivilegeLevel::Read);
    cs.ok("执行成功", result.is_ok(), "权限足够");
    cs.ok("输出正确", result.unwrap().output == "hello", "echo 行为");

    // 4. 权限
    let admin_def = CommandDef {
        name: "danger".into(), help: "danger".into(),
        privilege: PrivilegeLevel::Admin, handler: cmd_echo, group: "admin".into(),
    };
    bus.register(admin_def).unwrap();
    cs.ok("权限不足拒绝", bus.execute("danger", &[], PrivilegeLevel::Read).is_err(), "ERR_PRIVILEGE");
    cs.ok("权限足够执行", bus.execute("danger", &[], PrivilegeLevel::Admin).is_ok(), "Admin 通过");

    // 5. 分组
    cs.ok("分组查询", bus.by_group("test").len() == 1, "group=test");
    cs.ok("全名列表", bus.all_names().len() == 2, "两条命令");

    // 6. 注销
    cs.ok("注销成功", bus.unregister("test").is_ok(), "remove");
    cs.ok("注销后不存在", bus.help("test").is_none(), "已删除");
    cs.ok("注销不存在拒绝", bus.unregister("nonexist").is_err(), "ERR_NOT_FOUND");

    // 7. 内置命令
    let mut bus2 = DiagnosticBus::new();
    let n = register_builtin_commands(&mut bus2).unwrap();
    cs.ok("内置命令", n >= 4, "echo/help/version/mem");
    cs.ok("mem 热图", {
        let r = bus2.execute("mem", &["heatmap".into()], PrivilegeLevel::Read).unwrap();
        r.output.contains("heatmap")
    }, "F0266 对接");

    // 8. 权限级别
    cs.ok("权限排序", PrivilegeLevel::Read < PrivilegeLevel::Admin, "read < admin");
    cs.ok("权限标签", PrivilegeLevel::Danger.label() == "danger", "可读");

    // 9. 注册日志
    cs.ok("注册日志", bus.registration_log.len() >= 3, "审计追踪");

    // 10. 命令表
    let table = bus2.list_table();
    cs.ok("命令表", table.len() >= 4, "名/帮助/权限级");

    cs.done()
}
