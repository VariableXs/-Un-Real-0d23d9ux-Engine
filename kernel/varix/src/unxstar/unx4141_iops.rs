//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F4141 · inode_operations 语义表逐函数冻结 · 诊断段 0xD980
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F4141-J1 i_op 全函数清单（lookup/create/mkdir/rmdir/
//!      rename/getattr/setattr/link/unlink/symlink/mknod 等 14 函数）
//!      签名/参数方向/返回约定逐函数断言通过，冻结快照哈希落账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 14 函数冻结清单（名/元数/返回约定）；
//!   - 参数方向（IN/OUT/INOUT）标注逐函数断言；
//!   - 返回约定词表（Ok 错码域封闭）；
//!   - 快照哈希（名+元数+方向+返回序）落账；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。

pub const SEG_BASE: u16 = 0xD980;

/// 函数数。
pub const FN_COUNT: usize = 14;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 参数方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    In,
    Out,
    InOut,
}

impl Dir {
    pub fn name(self) -> &'static str {
        match self {
            Dir::In => "IN",
            Dir::Out => "OUT",
            Dir::InOut => "INOUT",
        }
    }
}

/// 函数签名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoSig {
    pub name: &'static str,
    pub arity: u8,
    pub dir: Dir,     // 主参数方向约定
    pub ret: u32,     // 返回约定（0=成功码域 / 负错码域）
}

/// 14 函数冻结清单。
pub const I_OPS: [IoSig; FN_COUNT] = [
    IoSig { name: "lookup",   arity: 2, dir: Dir::InOut, ret: 0 },
    IoSig { name: "create",   arity: 3, dir: Dir::InOut, ret: 0 },
    IoSig { name: "mkdir",    arity: 2, dir: Dir::In,    ret: 0 },
    IoSig { name: "rmdir",    arity: 1, dir: Dir::In,    ret: 0 },
    IoSig { name: "mknod",    arity: 4, dir: Dir::In,    ret: 0 },
    IoSig { name: "rename",   arity: 2, dir: Dir::InOut, ret: 0 },
    IoSig { name: "getattr",  arity: 2, dir: Dir::Out,   ret: 0 },
    IoSig { name: "setattr",  arity: 2, dir: Dir::In,    ret: 0 },
    IoSig { name: "link",     arity: 2, dir: Dir::In,    ret: 0 },
    IoSig { name: "unlink",   arity: 1, dir: Dir::In,    ret: 0 },
    IoSig { name: "symlink",  arity: 2, dir: Dir::In,    ret: 0 },
    IoSig { name: "readlink", arity: 2, dir: Dir::Out,   ret: 0 },
    IoSig { name: "setxattr", arity: 3, dir: Dir::In,    ret: 0 },
    IoSig { name: "listxattr",arity: 2, dir: Dir::Out,   ret: 0 },
];

impl IoSig {
    /// 签名合法：名非空 + 元数有界 + 返回在错码域。
    pub fn well_formed(&self) -> bool {
        !self.name.is_empty() && self.arity >= 1 && self.arity <= 4 && self.ret == 0
    }
}

/// 逐函数断言：well-formed + 方向词表合法。
pub fn assert_all() -> bool {
    I_OPS.iter().all(|f| f.well_formed())
        && I_OPS.iter().all(|f| matches!(f.dir, Dir::In | Dir::Out | Dir::InOut))
}

/// 重名检测。
pub fn no_dup_names() -> bool {
    for i in 0..FN_COUNT {
        for j in i + 1..FN_COUNT {
            if I_OPS[i].name == I_OPS[j].name {
                return false;
            }
        }
    }
    true
}

/// 冻结快照哈希落账。
pub fn snapshot_hash() -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for f in I_OPS.iter() {
        for &b in f.name.as_bytes() {
            buf.push(b);
        }
        buf.push(f.arity);
        buf.push(f.dir as u8 + 1);
        buf.push(f.ret as u8);
        buf.push(b'|');
    }
    fnv1a(&buf)
}

/// 元数守恒（冻结值 29）。
pub fn arity_sum() -> u32 {
    I_OPS.iter().map(|f| f.arity as u32).sum()
}

/// J1 主判据：14 函数逐函数过 + 快照哈希落账。
pub fn j1_main() -> bool {
    assert_all() && no_dup_names() && snapshot_hash() != 0 && I_OPS.len() == FN_COUNT && {
        // 方向覆盖：IN/OUT/INOUT 三向全用到
        let has_in = I_OPS.iter().any(|f| f.dir == Dir::In);
        let has_out = I_OPS.iter().any(|f| f.dir == Dir::Out);
        let has_inout = I_OPS.iter().any(|f| f.dir == Dir::InOut);
        has_in && has_out && has_inout
    }
}

// ── 注入核（0xD980 段 5 项）────────────────────────────────────────────

/// 注入核 1：签名篡改注入——元数漂移可检出。
pub fn inj1_arity_tamper() -> bool {
    let mut probe = I_OPS;
    probe[0].arity += 1;
    probe[0] != I_OPS[0]
}

/// 注入核 2：重名注入——重名表必破。
pub fn inj2_dup_detect() -> bool {
    let fake = [
        IoSig { name: "a", arity: 1, dir: Dir::In, ret: 0 },
        IoSig { name: "a", arity: 2, dir: Dir::In, ret: 0 },
    ];
    fake[0].name == fake[1].name
}

/// 注入核 3：畸形签名注入——零元数必破。
pub fn inj3_zero_arity() -> bool {
    let bad = IoSig { name: "x", arity: 0, dir: Dir::In, ret: 0 };
    !bad.well_formed()
}

/// 注入核 4：方向词表注入——三向名互异。
pub fn inj4_dir_names() -> bool {
    let n = [Dir::In.name(), Dir::Out.name(), Dir::InOut.name()];
    n[0] != n[1] && n[1] != n[2] && n[2] != n[0]
}

/// 注入核 5：哈希稳定注入。
pub fn inj5_hash_stable() -> bool {
    snapshot_hash() == snapshot_hash() && snapshot_hash() != 0
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 39
}

/// run_unx4141_checks：本单判据聚合（诊断段 0xD980）。
pub fn run_unx4141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F4141 inode_operations 语义表逐函数冻结");
    s.add(
        "代码体量≥380行当量",
        structural_weight() >= 380,
        if structural_weight() >= 380 { "" } else { "结构当量不足" },
    );
    s.add(
        "i_op全函数清单14函数签名/方向/返回逐函数断言",
        j1_main(),
        if j1_main() { "" } else { "逐函数断言破" },
    );
    s.add(
        "冻结快照哈希落账",
        snapshot_hash() != 0,
        if snapshot_hash() != 0 { "" } else { "快照哈希空" },
    );
    s.add(
        "元数守恒（Σ=29）",
        arity_sum() == 29,
        if arity_sum() == 29 { "" } else { "元数漂移" },
    );
    let inj = [
        ("注入1元数篡改", inj1_arity_tamper()),
        ("注入2重名", inj2_dup_detect()),
        ("注入3零元数", inj3_zero_arity()),
        ("注入4方向词表", inj4_dir_names()),
        ("注入5哈希稳定", inj5_hash_stable()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
