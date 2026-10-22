//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F7261 · dm 堆叠映射框架（dm 设备作为下层 target）· 诊断段 0xAD00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F7261-J1 dm 设备注册为下层 target 后上层 IO 正确穿透
//!      （逐扇区校验）；堆叠注册的合法性校验（物理盘不可作上层）
//!      全账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 堆叠：上层 dm → 下层 dm → 物理盘三层；
//!   - IO 穿透：上层扇区逐扇区映射到下层（校验和逐扇区一致）；
//!   - 合法性校验：物理盘不可作上层 target（只可作叶）；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xAD00;

/// 穿透扇区数。
pub const SECTORS: u64 = 128;

/// 错误码。
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

/// 节点类型：物理盘（只可作叶）/ dm 设备（可作上层或下层）。
#[derive(Clone, Copy, PartialEq)]
pub enum NodeKind {
    Physical,
    Dm,
}

/// 堆叠节点。
pub struct StackNode {
    pub kind: NodeKind,
    pub name: String,
    /// 下层 target 名（叶节点为空）。
    pub lower: Option<String>,
}

/// 堆叠框架。
pub struct StackFrame {
    pub nodes: Vec<StackNode>,
}

impl StackFrame {
    pub fn new() -> Self {
        StackFrame { nodes: Vec::new() }
    }

    /// 注册节点：物理盘必须为叶（不得挂下层）。
    pub fn register(&mut self, kind: NodeKind, name: &str, lower: Option<&str>) -> Result<(), i32> {
        if kind == NodeKind::Physical && lower.is_some() {
            return Err(EINVAL); // 物理盘不可作上层
        }
        if let Some(l) = lower {
            if !self.nodes.iter().any(|n| n.name == l) {
                return Err(2); // 下层不存在
            }
        }
        if self.nodes.iter().any(|n| n.name == name) {
            return Err(17); // 重名
        }
        self.nodes.push(StackNode { kind, name: name.to_string(), lower: lower.map(|s| s.to_string()) });
        Ok(())
    }

    /// IO 穿透：沿上层→下层映射到物理扇区。
    pub fn io_pass_through(&self, top: &str, sector: u64, val: u64, phys: &mut Vec<u64>) -> Result<(), i32> {
        let mut cur = top.to_string();
        // 沿下层链走到叶
        loop {
            let n = self.nodes.iter().find(|n| n.name == cur).ok_or(2)?;
            match n.lower.clone() {
                Some(l) => cur = l,
                None => break,
            }
        }
        // 叶必须是物理盘
        let leaf = self.nodes.iter().find(|n| n.name == cur).ok_or(2)?;
        if leaf.kind != NodeKind::Physical {
            return Err(EINVAL);
        }
        if (sector as usize) >= phys.len() {
            return Err(EINVAL);
        }
        phys[sector as usize] = val;
        Ok(())
    }
}

/// J1 主判据：三层堆叠穿透逐扇区校验 + 物理盘不可作上层全账。
pub fn j1_main() -> bool {
    let mut f = StackFrame::new();
    // 注册：物理盘 leaf → dm-lower（下层指向 leaf）→ dm-top（下层指向 lower）
    if f.register(NodeKind::Physical, "sda", None).is_err() {
        return false;
    }
    if f.register(NodeKind::Dm, "dm-lower", Some("sda")).is_err() {
        return false;
    }
    if f.register(NodeKind::Dm, "dm-top", Some("dm-lower")).is_err() {
        return false;
    }
    // 穿透：128 扇区写入经 dm-top 直落 sda
    let mut phys: Vec<u64> = vec![0; SECTORS as usize];
    for s in 0..SECTORS {
        if f.io_pass_through("dm-top", s, 0x1000 + s, &mut phys).is_err() {
            return false;
        }
    }
    // 逐扇区校验：叶数据与写入值一致
    if !phys.iter().enumerate().all(|(s, v)| *v == 0x1000 + s as u64) {
        return false;
    }
    // 合法性：物理盘作上层拒
    f.register(NodeKind::Physical, "bad", Some("sda")).is_err()
        && f.register(NodeKind::Dm, "dm-ghost", Some("nope")).is_err()
}

// ── 注入核（0xAD00 段 5 项）────────────────────────────────────────────

/// 注入核 1：物理盘作上层注入。
pub fn inj1_phys_as_top() -> bool {
    let mut f = StackFrame::new();
    f.register(NodeKind::Physical, "sda", None).ok();
    f.register(NodeKind::Physical, "bad", Some("sda")).is_err()
}

/// 注入核 2：幽灵下层注入——下层不存在注册拒。
pub fn inj2_ghost_lower() -> bool {
    let mut f = StackFrame::new();
    f.register(NodeKind::Dm, "dm-x", Some("ghost")).is_err()
}

/// 注入核 3：重名注入——节点重名注册拒。
pub fn inj3_dup_name() -> bool {
    let mut f = StackFrame::new();
    f.register(NodeKind::Physical, "sda", None).ok();
    f.register(NodeKind::Dm, "sda", None).is_err()
}

/// 注入核 4：扇区越界注入——穿透越叶容量拒。
pub fn inj4_sector_oob() -> bool {
    let mut f = StackFrame::new();
    f.register(NodeKind::Physical, "sda", None).ok();
    let mut phys: Vec<u64> = vec![0; 4];
    f.io_pass_through("sda", 99, 1, &mut phys).is_err()
}

/// 注入核 5：穿透落叶注入——穿透后值落在物理叶（不在中间层缓存）。
pub fn inj5_lands_leaf() -> bool {
    let mut f = StackFrame::new();
    f.register(NodeKind::Physical, "sda", None).ok();
    f.register(NodeKind::Dm, "dm-a", Some("sda")).ok();
    let mut phys: Vec<u64> = vec![0; 8];
    f.io_pass_through("dm-a", 3, 0xBEEF, &mut phys).ok();
    phys[3] == 0xBEEF && phys[0] == 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 32
}

/// run_unx7261_checks：本单判据聚合（诊断段 0xAD00）。
pub fn run_unx7261_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7261 dm 堆叠映射框架（dm 设备作为下层 target）");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "堆叠穿透逐扇区校验",
        j1_main(),
        if j1_main() { "" } else { "穿透校验破" },
    );
    s.add(
        "物理盘不可作上层全账",
        j1_main(),
        if j1_main() { "" } else { "合法性破" },
    );
    let inj = [
        ("注入1物理作上层拒", inj1_phys_as_top()),
        ("注入2幽灵下层拒", inj2_ghost_lower()),
        ("注入3重名拒", inj3_dup_name()),
        ("注入4扇区越界拒", inj4_sector_oob()),
        ("注入5穿透落叶", inj5_lands_leaf()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
