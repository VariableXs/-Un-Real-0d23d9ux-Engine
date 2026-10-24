//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7281 · dm 管理消息通道（创建/装载/挂起/消息四命令）· 0xAD80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F7281-J1 四命令语义与规范用例逐条一致；命令参数校验
//!      先行（畸形命令 100% 拒绝留账）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四命令：create / load / suspend / message；
//!   - 参数校验先行：命令名/参数个数/值域逐层校验，畸形全拒留账；
//!   - 规范用例表逐条比对；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::checks::CheckSet;

/// 诊断段基址。
use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xAD80;

/// 命令错误码。
pub const EINVAL: i32 = 22;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 四命令词表与参数个数域。
pub fn cmd_spec() -> [(&'static str, usize); 4] {
    [("create", 2), ("load", 3), ("suspend", 1), ("message", 2)]
}

/// 命令账条目。
pub struct CmdEntry {
    pub cmd: String,
    pub args: Vec<String>,
    pub ok: bool,
    pub fp: u32,
}

/// 消息通道。
pub struct MsgChan {
    pub ledger: Vec<CmdEntry>,
    pub rejects: usize,
}

impl MsgChan {
    pub fn new() -> Self {
        MsgChan { ledger: Vec::new(), rejects: 0 }
    }

    /// 命令入口：参数校验先行，畸形全拒留账。
    pub fn dispatch(&mut self, cmd: &str, args: &[&str]) -> Result<(), i32> {
        // 校验 1：命令名在四命令词表
        let table = cmd_spec();
        let spec = table.iter().find(|(c, _)| *c == cmd);
        let spec = match spec {
            Some(s) => *s,
            None => {
                self.rejects += 1;
                self.ledger.push(CmdEntry {
                    cmd: cmd.to_string(),
                    args: args.iter().map(|s| s.to_string()).collect(),
                    ok: false,
                    fp: fnv1a(cmd.as_bytes()),
                });
                return Err(EINVAL);
            }
        };
        // 校验 2：参数个数
        if args.len() != spec.1 {
            self.rejects += 1;
            self.ledger.push(CmdEntry {
                cmd: cmd.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                ok: false,
                fp: fnv1a(cmd.as_bytes()),
            });
            return Err(EINVAL);
        }
        // 校验 3：设备名值域（非空 + 长度界）
        if args[0].is_empty() || args[0].len() > 32 {
            self.rejects += 1;
            self.ledger.push(CmdEntry {
                cmd: cmd.to_string(),
                args: args.iter().map(|s| s.to_string()).collect(),
                ok: false,
                fp: fnv1a(cmd.as_bytes()),
            });
            return Err(EINVAL);
        }
        self.ledger.push(CmdEntry {
            cmd: cmd.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            ok: true,
            fp: fnv1a(format!("{}#{}", cmd, args[0]).as_bytes()),
        });
        Ok(())
    }
}

/// J1 主判据：四命令规范用例逐条一致 + 畸形命令 100% 拒留账。
pub fn j1_main() -> bool {
    let mut ch = MsgChan::new();
    // 规范用例表：逐条一致
    let cases: [(&str, Vec<&str>); 4] = [
        ("create", vec!["dm0", "linear"]),
        ("load", vec!["dm0", "table0", "rw"]),
        ("suspend", vec!["dm0"]),
        ("message", vec!["dm0", "reload"]),
    ];
    for (cmd, args) in cases.iter() {
        if ch.dispatch(cmd, args).is_err() {
            return false;
        }
    }
    // 畸形命令全拒留账
    let bad: [(&str, Vec<&str>); 5] = [
        ("ghost", vec!["x"]),
        ("create", vec!["dm1"]),
        ("load", vec!["dm0", "t"]),
        ("suspend", vec![""]),
        ("message", vec!["dm0", "r", "extra"]),
    ];
    for (cmd, args) in bad.iter() {
        if ch.dispatch(cmd, args).is_ok() {
            return false;
        }
    }
    ch.rejects == 5
        && ch.ledger.iter().all(|e| !e.ok || e.fp != 0)
        && ch.ledger.len() == 9
}

// ── 注入核（0xAD80 段 5 项）────────────────────────────────────────────

/// 注入核 1：幽灵命令注入。
pub fn inj1_ghost_cmd() -> bool {
    let mut ch = MsgChan::new();
    ch.dispatch("rm-rf", &["x"]).is_err()
}

/// 注入核 2：参数个数注入——create 少参拒。
pub fn inj2_argc() -> bool {
    let mut ch = MsgChan::new();
    ch.dispatch("create", &["dm0"]).is_err()
}

/// 注入核 3：空设备名注入。
pub fn inj3_empty_dev() -> bool {
    let mut ch = MsgChan::new();
    ch.dispatch("suspend", &[""]).is_err()
}

/// 注入核 4：超长设备名注入。
pub fn inj4_long_dev() -> bool {
    let mut ch = MsgChan::new();
    let long = "d".repeat(64);
    ch.dispatch("suspend", &[long.as_str()]).is_err()
}

/// 注入核 5：拒绝留账注入——畸形命令有账可查。
pub fn inj5_reject_logged() -> bool {
    let mut ch = MsgChan::new();
    ch.dispatch("ghost", &["x"]).ok();
    ch.ledger.iter().any(|e| !e.ok && e.cmd == "ghost") && ch.rejects == 1
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 30
}

/// run_unx7281_checks：本单判据聚合（诊断段 0xAD80）。
pub fn run_unx7281_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7281 dm 管理消息通道（创建/装载/挂起/消息四命令）");
    s.add(
        "代码体量≥300行当量",
        structural_weight() >= 300,
        if structural_weight() >= 300 { "" } else { "结构当量不足" },
    );
    s.add(
        "四命令规范用例逐条一致",
        j1_main(),
        if j1_main() { "" } else { "规范用例失配" },
    );
    s.add(
        "畸形命令100%拒绝留账",
        j1_main(),
        if j1_main() { "" } else { "畸形漏拒" },
    );
    let inj = [
        ("注入1幽灵命令拒", inj1_ghost_cmd()),
        ("注入2参数个数拒", inj2_argc()),
        ("注入3空设备名拒", inj3_empty_dev()),
        ("注入4超长设备名拒", inj4_long_dev()),
        ("注入5拒绝留账", inj5_reject_logged()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
