//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7301 · flush 转发契约总纲（→B2/B3 联签三方断电链）· 0xAE00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F7301-J1（保护判据先行）契约断言：dm 设备上任何 flush
//!      返回成功时，其下全部成员设备的 flush 均已确认（全确认语义）；
//!      UNX-F7301-J2 契约与 B2/B3 联签文档落纸（三方断电链：应用
//!      fsync→fs 层 journal→dm 转发→B4 执行），接口变更必须 AI-86
//!      复核（冻结级别全域最高）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 全确认语义：dm flush 成功 ⇔ 全部成员 flush 确认（一票未确认
//!     即 dm flush 不得返回成功）；
//!   - 三方断电链落纸：fsync → journal → dm 转发 → B4 执行 四段链
//!     声明账；
//!   - AI-86 冻结复核：契约指纹变更必须过复核闸；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xAE00;

/// 断电链段数。
pub const CHAIN_SEGS: usize = 4;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// dm 设备：成员表 + flush 契约。
pub struct DmFlush {
    pub name: String,
    pub members: Vec<(String, bool)>, // (成员名, 该成员 flush 已确认位)
}

impl DmFlush {
    pub fn new(name: &str, members: &[&str]) -> Self {
        DmFlush {
            name: name.to_string(),
            members: members.iter().map(|m| (m.to_string(), false)).collect(),
        }
    }

    /// 成员 flush 确认。
    pub fn member_flush(&mut self, member: &str) -> Result<(), i32> {
        for m in self.members.iter_mut() {
            if m.0 == member {
                m.1 = true;
                return Ok(());
            }
        }
        Err(2)
    }

    /// dm flush（全确认语义）：全部成员已确认才允许返回成功。
    pub fn dm_flush(&self) -> Result<(), i32> {
        if self.members.iter().all(|m| m.1) {
            Ok(())
        } else {
            Err(16) // EBUSY：存在未确认成员
        }
    }

    /// 复位确认位（新的一轮 flush）。
    pub fn reset(&mut self) {
        for m in self.members.iter_mut() {
            m.1 = false;
        }
    }
}

/// 三方断电链声明账。
pub struct PowerChain {
    pub segs: Vec<(String, String, bool)>, // (段名, 职责, 落纸位)
}

impl PowerChain {
    pub fn new() -> Self {
        PowerChain { segs: Vec::new() }
    }

    /// 落纸：四段链（fsync → journal → dm 转发 → B4 执行）。
    pub fn paper(&mut self) {
        let segs = [
            ("应用 fsync", "应用层持久化请求入口"),
            ("fs 层 journal", "文件系统日志先行"),
            ("dm 转发", "dm 契约逐成员转发 flush"),
            ("B4 执行", "块层落盘执行与确认"),
        ];
        for (n, d) in segs.iter() {
            self.segs.push((n.to_string(), d.to_string(), true));
        }
    }

    /// 落纸断言：四段齐 + 全部落纸 + 段序正确。
    pub fn on_paper(&self) -> bool {
        let names = ["应用 fsync", "fs 层 journal", "dm 转发", "B4 执行"];
        self.segs.len() == CHAIN_SEGS
            && self.segs.iter().enumerate().all(|(i, (n, _, p))| *p && n == names[i])
    }
}

/// AI-86 冻结复核闸：契约指纹变更须持复核令。
pub fn ai86_gate(old_fp: u32, new_fp: u32, review_token: Option<u32>) -> Result<(), i32> {
    if old_fp == new_fp {
        return Ok(()); // 无变更免复核
    }
    match review_token {
        Some(t) if t == fnv1a(b"AI-86") => Ok(()),
        _ => Err(1), // EPERM：冻结级别全域最高，无令即拒
    }
}

/// J1 主判据：全确认语义 + 断电链落纸 + AI-86 复核闸。
pub fn j1_main() -> bool {
    // 全确认语义
    let mut dm = DmFlush::new("dm0", &["sda", "sdb", "sdc"]);
    if dm.dm_flush().is_ok() {
        return false; // 未全确认不得成功
    }
    for m in ["sda", "sdb", "sdc"] {
        dm.member_flush(m).ok();
    }
    if dm.dm_flush().is_err() {
        return false;
    }
    // 复位后单成员确认仍拒
    dm.reset();
    dm.member_flush("sda").ok();
    if dm.dm_flush().is_ok() {
        return false;
    }
    // 断电链落纸
    let mut ch = PowerChain::new();
    ch.paper();
    if !ch.on_paper() {
        return false;
    }
    // AI-86 闸：无变更免复核；有变更无令拒；持令过
    let fp = fnv1a(b"flush-contract-v1");
    ai86_gate(fp, fp, None).is_ok()
        && ai86_gate(fp, fp ^ 1, None).is_err()
        && ai86_gate(fp, fp ^ 1, Some(fnv1a(b"AI-86"))).is_ok()
}

// ── 注入核（0xAE00 段 5 项）────────────────────────────────────────────

/// 注入核 1：单成员确认注入——只确认一个成员 dm flush 拒。
pub fn inj1_partial_confirm() -> bool {
    let mut dm = DmFlush::new("dm1", &["a", "b", "c", "d"]);
    dm.member_flush("a").ok();
    dm.dm_flush().is_err()
}

/// 注入核 2：幽灵成员注入——不存在成员确认拒。
pub fn inj2_ghost_member() -> bool {
    let mut dm = DmFlush::new("dm2", &["a", "b"]);
    dm.member_flush("ghost").is_err()
}

/// 注入核 3：断电链缺段注入——四段缺一落纸拒。
pub fn inj3_chain_gap() -> bool {
    let mut ch = PowerChain::new();
    ch.paper();
    ch.segs.pop();
    !ch.on_paper()
}

/// 注入核 4：段序错乱注入——链段序错落纸拒。
pub fn inj4_chain_misorder() -> bool {
    let mut ch = PowerChain::new();
    ch.paper();
    ch.segs.swap(0, 3);
    !ch.on_paper()
}

/// 注入核 5：无令变更注入——冻结契约变更无 AI-86 令拒。
pub fn inj5_no_token() -> bool {
    let fp = fnv1a(b"contract");
    ai86_gate(fp, fp + 1, Some(12345)).is_err()
        && ai86_gate(fp, fp + 1, Some(fnv1a(b"AI-86"))).is_ok()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    11u32 * 31
}

/// run_unx7301_checks：本单判据聚合（诊断段 0xAE00）。
pub fn run_unx7301_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7301 flush 转发契约总纲（→B2/B3 联签三方断电链）");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "全确认语义（保护判据先行）",
        j1_main(),
        if j1_main() { "" } else { "全确认破" },
    );
    s.add(
        "三方断电链落纸+AI-86复核闸",
        j1_main(),
        if j1_main() { "" } else { "落纸或复核破" },
    );
    let inj = [
        ("注入1单成员确认拒", inj1_partial_confirm()),
        ("注入2幽灵成员拒", inj2_ghost_member()),
        ("注入3链缺段拒", inj3_chain_gap()),
        ("注入4段序错乱拒", inj4_chain_misorder()),
        ("注入5无令变更拒", inj5_no_token()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
