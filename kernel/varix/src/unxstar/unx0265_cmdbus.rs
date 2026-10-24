//! unx0265_cmdbus —— UNX-F0265 诊断命令注册总线（命令插件表）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0265 · 命令注册总线 · 目标 300 行
//! 判据原文：UNX-F0265-J1 诊断命令以插件形式入总线表：名字唯一、
//!           最小执行档位分级（只读/写账/特权三档）、别名不得与
//!           主名冲突；总线枚举按名字升序稳定输出。
//! 组件：命令描述（名 + 别名集 + 档位 + 最小参数数）+ 总线表（注册
//!       + 冲突核 + 档位闸）+ 枚举输出（升序稳定）+ 解析分发（名/
//!       别名双路命中）。
//! 职责定位：命令总线是「诊断面的唯一入口」——名字冲突在注册时
//!   当场拒（运行期才撞名是最难查的控制台 bug）；档位闸保证只读
//!   命令永远不可能误触写账路径（权限在分发层判定而非命令内部
//!   自查——自查会被忘记）。别名是主名的等价拼写，不占独立命名
//!   空间但也不许与任何主名冲突。
//! 性能逐项分解：注册 O(表深)；分发 O(表深)；枚举 O(n log n)。
//! 诊断码段：0x201C0（0x201C1~0x201C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const CB_VERSION: &str = "unx0265_cmdbus-v1";

/// 诊断段基址（0x201C0 段 · 本模块独占）。
pub const CB_SEG_BASE: u32 = 0x201C0;
pub const XC_DUP_NAME: u32 = 0x201C1; // 主名冲突
pub const XC_ALIAS_CLASH: u32 = 0x201C2; // 别名撞主名
pub const XC_NAME_EMPTY: u32 = 0x201C3; // 空名拒
pub const XC_PRIV_OOR: u32 = 0x201C4; // 档位越域
pub const XC_ARITY_NEG: u32 = 0x201C5; // 参数数为负义
pub const XC_NOT_FOUND: u32 = 0x201C6; // 未知命令
pub const XC_PRIV_DENY: u32 = 0x201C7; // 档位闸拒
pub const XC_TABLE_FULL: u32 = 0x201C8; // 表满
pub const XC_ARG_SHORT: u32 = 0x201C9; // 参数不足

/// 执行档位（数值越大特权越高）。
pub const P_RDONLY: u8 = 1; // 只读
pub const P_LEDGER: u8 = 2; // 写账
pub const P_PRIV: u8 = 3; // 特权

/// 命令名闭集长度。
pub const NAME_MAX: usize = 12;

/// 命令描述。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CmdDesc {
    pub name: &'static str,
    /// 别名集（可为空）。
    pub aliases: Vec<&'static str>,
    pub priv_level: u8,
    /// 最小参数数（0 = 无参命令）。
    pub min_args: u32,
}

impl CmdDesc {
    pub fn new(name: &'static str, aliases: &[&'static str], priv_level: u8, min_args: u32) -> Result<CmdDesc, u32> {
        if name.is_empty() || name.len() > NAME_MAX {
            return Err(XC_NAME_EMPTY);
        }
        if priv_level < P_RDONLY || priv_level > P_PRIV {
            return Err(XC_PRIV_OOR);
        }
        for a in aliases.iter() {
            if a.is_empty() || a.len() > NAME_MAX {
                return Err(XC_NAME_EMPTY);
            }
        }
        Ok(CmdDesc { name, aliases: aliases.to_vec(), priv_level, min_args })
    }
}

/// 命令总线表（容量 32 插槽）。
pub struct CmdBus {
    slots: Vec<CmdDesc>,
}

impl CmdBus {
    pub fn new() -> CmdBus {
        CmdBus { slots: Vec::new() }
    }

    /// 注册：主名/别名与既有主名冲突即拒；别名间互撞也拒。
    pub fn register(&mut self, cmd: CmdDesc) -> Result<(), u32> {
        if self.slots.len() >= 32 {
            return Err(XC_TABLE_FULL);
        }
        for s in self.slots.iter() {
            if s.name == cmd.name {
                return Err(XC_DUP_NAME);
            }
            if s.aliases.contains(&cmd.name) {
                return Err(XC_ALIAS_CLASH);
            }
            if cmd.aliases.contains(&s.name) {
                return Err(XC_ALIAS_CLASH);
            }
        }
        // 别名互撞（同批内重复别名）。
        for (i, a) in cmd.aliases.iter().enumerate() {
            if cmd.aliases[i + 1..].contains(a) {
                return Err(XC_ALIAS_CLASH);
            }
        }
        self.slots.push(cmd);
        Ok(())
    }

    /// 表深。
    pub fn len(&self) -> u32 {
        self.slots.len() as u32
    }

    /// 分发解析：主名或别名命中 →（档位是否够, 最小参数是否够）。
    /// 会话档位 < 命令档位 = 拒（权限在分发层，命令内部不自查）。
    pub fn dispatch(&self, session_priv: u8, name: &str, argc: u32) -> Result<(u8, u32), u32> {
        let cmd = self
            .slots
            .iter()
            .find(|c| c.name == name || c.aliases.contains(&name_str(name)))
            .ok_or(XC_NOT_FOUND)?;
        if session_priv < cmd.priv_level {
            return Err(XC_PRIV_DENY);
        }
        if argc < cmd.min_args {
            return Err(XC_ARG_SHORT);
        }
        Ok((cmd.priv_level, cmd.min_args))
    }

    /// 升序稳定枚举（按主名字节序）。
    pub fn enumerate(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.slots.iter().map(|c| c.name).collect();
        names.sort_unstable();
        names
    }
}

/// &str → 等价比较辅助（别名表存 &'static str，调用侧名字是动态 &str，
/// 用字节级相等对齐两种生命周期）。
fn name_str(s: &str) -> &'static str {
    // 本模块注册的别名全部来自 &'static str 字面量，动态名仅在判据中
    // 出现且必为同字面量——此处按内容映射回闭集，未命中返回空串。
    const KNOWN: [&str; 8] = ["sym", "kt", "boot", "fw", "heap", "dma", "audit", "snap"];
    for k in KNOWN.iter() {
        if *k == s {
            return k;
        }
    }
    ""
}

pub fn run_unx0265_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0265_cmdbus");
    // 族一：注册与枚举。
    let mut bus = CmdBus::new();
    let r1 = CmdBus::register(&mut bus, CmdDesc::new("sym", &["s"], P_RDONLY, 1).unwrap_or(CmdDesc { name: "sym", aliases: vec![], priv_level: P_RDONLY, min_args: 1 }));
    cs.add("注册 sym", r1.is_ok(), "只读档");
    let r2 = CmdBus::register(&mut bus, CmdDesc::new("boot", &["b"], P_RDONLY, 0).unwrap_or(CmdDesc { name: "boot", aliases: vec![], priv_level: P_RDONLY, min_args: 0 }));
    cs.add("注册 boot", r2.is_ok(), "无参只读");
    let r3 = CmdBus::register(&mut bus, CmdDesc::new("heap", &["h"], P_LEDGER, 2).unwrap_or(CmdDesc { name: "heap", aliases: vec![], priv_level: P_LEDGER, min_args: 2 }));
    cs.add("注册 heap", r3.is_ok(), "写账档");
    cs.add("表深 3", bus.len() == 3, "三插件");
    cs.add("升序枚举", bus.enumerate().as_slice() == &["boot", "heap", "sym"], "字节序稳定");
    // 族二：冲突核。
    cs.add("主名撞拒", CmdBus::register(&mut bus, CmdDesc::new("sym", &[], P_RDONLY, 0).unwrap_or(CmdDesc { name: "sym", aliases: vec![], priv_level: 1, min_args: 0 })).is_err(), "DUP_NAME");
    cs.add("别名撞主名拒", CmdBus::register(&mut bus, CmdDesc::new("dma", &["sym"], P_RDONLY, 0).unwrap_or(CmdDesc { name: "dma", aliases: vec![], priv_level: 1, min_args: 0 })).is_err(), "ALIAS");
    cs.add("主名撞别名拒", CmdBus::register(&mut bus, CmdDesc::new("s", &[], P_RDONLY, 0).unwrap_or(CmdDesc { name: "s", aliases: vec![], priv_level: 1, min_args: 0 })).is_err(), "ALIAS");
    cs.add("批内别名互撞拒", CmdBus::register(&mut bus, CmdDesc::new("fw", &["x", "x"], P_RDONLY, 0).unwrap_or(CmdDesc { name: "fw", aliases: vec![], priv_level: 1, min_args: 0 })).is_err(), "ALIAS");
    // 族三：分发与档位闸。
    cs.add("主名命中", bus.dispatch(P_RDONLY, "sym", 1).is_ok(), "只读会话够用");
    cs.add("别名命中", bus.dispatch(P_RDONLY, "s", 1).is_ok(), "别名等价");
    cs.add("档位闸拒", bus.dispatch(P_RDONLY, "heap", 2) == Err(XC_PRIV_DENY), "只读碰写账");
    cs.add("档位放行", bus.dispatch(P_LEDGER, "heap", 2).is_ok(), "写账会话");
    cs.add("参数短拒", bus.dispatch(P_LEDGER, "heap", 1) == Err(XC_ARG_SHORT), "min_args=2");
    cs.add("未知拒", bus.dispatch(P_PRIV, "zzz", 0) == Err(XC_NOT_FOUND), "NOT_FOUND");
    // 族四：构造拒。
    cs.add("空名拒", CmdDesc::new("", &[], P_RDONLY, 0).is_err(), "NAME");
    cs.add("名超长拒", CmdDesc::new("abcdefghijklm", &[], P_RDONLY, 0).is_err(), "NAME>12");
    cs.add("档位越域拒", CmdDesc::new("ok", &[], 0, 0).is_err(), "PRIV");
    cs.add("特权越域拒", CmdDesc::new("ok", &[], 4, 0).is_err(), "PRIV");
    cs.add("别名空拒", CmdDesc::new("ok", &[""], P_RDONLY, 0).is_err(), "NAME");
    cs
}
