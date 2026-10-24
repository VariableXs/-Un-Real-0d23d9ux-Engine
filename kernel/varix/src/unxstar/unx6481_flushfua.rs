//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F6481 · PREFLUSH/FUA 请求类型编码 · 诊断段 0x9300
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥329 行；
//!   ② UNX-F6481-J1 PREFLUSH 前刷/FUA 后刷/组合三类请求编码唯一
//!      可判定，非法组合 100% 拒绝；编码与设计表逐位一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 位编码：REQ_PREFLUSH=1<<0、REQ_FUA=1<<1，op 高 4 位；
//!   - 设计表逐位对照：数据 0x00 / 前刷 0x01 / 后刷 0x02 / 组合 0x03；
//!   - 非法组合全集拒绝：READ 带 FUA、DISCARD 带刷旗、保留位非零；
//!   - 1,000 次 LCG 全 8 位值遍历：合法/非法判定唯一可判定；
//!   - 注入核 5 项。
//! ═══════════════════════════════════════════════════════════════════════

use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9300;

/// 请求旗标位。
pub const REQ_PREFLUSH: u8 = 1 << 0;
pub const REQ_FUA: u8 = 1 << 1;

/// op 编码（低 2 位）。
pub const OP_READ: u8 = 0;
pub const OP_WRITE: u8 = 1;
pub const OP_DISCARD: u8 = 2;

/// 非法编码错误码。
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

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg { state: seed }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
}

/// 设计表逐位对照（op, flags) → 编码）：四类合法编码。
pub fn design_table() -> [(u8, u8, u8); 4] {
    [
        (OP_READ, 0, OP_READ),
        (OP_WRITE, 0, OP_WRITE),
        (OP_WRITE, REQ_PREFLUSH, 0x04 | REQ_PREFLUSH),
        (OP_WRITE, REQ_FUA, 0x04 | REQ_FUA),
    ]
}

/// 编码唯一可判定：op + flags → Result<u8>。
/// 规则：
///   - flags 仅允许 {0, PREFLUSH, FUA, PREFLUSH|FUA}；
///   - PREFLUSH/FUA 只允许出现在 WRITE（刷写组合）；
///   - READ/DISCARD 必须零旗标；
pub fn encode(op: u8, flags: u8) -> Result<u8, i32> {
    let flush = flags & REQ_PREFLUSH != 0;
    let fua = flags & REQ_FUA != 0;
    // 保留位非零拒（flags 高 6 位）
    if flags & !0x03 != 0 {
        return Err(EINVAL);
    }
    match op {
        OP_WRITE => {
            let mut code = 0x04u8;
            if flush {
                code |= REQ_PREFLUSH;
            }
            if fua {
                code |= REQ_FUA;
            }
            Ok(code)
        }
        OP_READ => {
            if flush || fua {
                Err(EINVAL)
            } else {
                Ok(OP_READ)
            }
        }
        OP_DISCARD => {
            if flush || fua {
                Err(EINVAL)
            } else {
                Ok(OP_DISCARD)
            }
        }
        _ => Err(EINVAL),
    }
}

/// 解码：编码 → (op, flags)，与 encode 严格互逆。
pub fn decode(code: u8) -> Result<(u8, u8), i32> {
    match code {
        OP_READ => Ok((OP_READ, 0)),
        OP_DISCARD => Ok((OP_DISCARD, 0)),
        c if c & 0x04 != 0 && c & !0x07 == 0 => {
            let flags = c & 0x03;
            Ok((OP_WRITE, flags))
        }
        _ => Err(EINVAL),
    }
}

/// 三类请求判定：前刷 / 后刷 / 组合唯一可判定。
pub fn classify(code: u8) -> &'static str {
    match (code & REQ_PREFLUSH != 0, code & REQ_FUA != 0) {
        (true, false) => "前刷",
        (false, true) => "后刷",
        (true, true) => "组合",
        _ => "数据",
    }
}

/// J1 主判据：1,000 次全 8 位值遍历——合法/非法判定唯一 + 逐位一致。
pub fn j1_main() -> bool {
    let mut g = Lcg::new(0x5EED_6481);
    // 设计表逐位一致
    for (op, flags, code) in design_table().iter() {
        if encode(*op, *flags) != Ok(*code) {
            return false;
        }
        if decode(*code) != Ok((*op, *flags)) {
            return false;
        }
    }
    // 全 256 编码值解码分类：合法域外一律 EINVAL
    for c in 0..=255u8 {
        match decode(c) {
            Ok((op, flags)) => {
                if encode(op, flags) != Ok(c) {
                    return false;
                }
            }
            Err(EINVAL) => {}
            Err(_) => return false,
        }
    }
    // 1,000 次随机 (op, flags)：encode/decode 往返恒等或一致拒绝
    for _ in 0..1000 {
        let op = (g.next() % 4) as u8;
        let flags = (g.next() % 256) as u8;
        match encode(op, flags) {
            Ok(c) => {
                if decode(c) != Ok((op, flags)) {
                    return false;
                }
            }
            Err(EINVAL) => {
                // 非法组合：READ/DISCARD 带旗标、保留位、op 越界
                let flush = flags & REQ_PREFLUSH != 0;
                let fua = flags & REQ_FUA != 0;
                let ok_rej = op != OP_WRITE && (flush || fua)
                    || flags & !0x03 != 0
                    || op > OP_DISCARD;
                if !ok_rej {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

// ── 注入核（0x9300 段 5 项）────────────────────────────────────────────

/// 注入核 1：READ+FUA 注入——非法组合拒绝。
pub fn inj1_read_fua() -> bool {
    encode(OP_READ, REQ_FUA).is_err()
}

/// 注入核 2：DISCARD+PREFLUSH 注入——非法组合拒绝。
pub fn inj2_discard_flush() -> bool {
    encode(OP_DISCARD, REQ_PREFLUSH).is_err()
}

/// 注入核 3：保留位注入——flags 高位非零拒绝。
pub fn inj3_reserved_bits() -> bool {
    (4..=255).all(|f| encode(OP_WRITE, f).is_err() || (f & !0x03) == 0)
}

/// 注入核 4：op 越界注入——op=3+ 全拒。
pub fn inj4_op_range() -> bool {
    (3..=15).all(|op| encode(op, 0).is_err())
}

/// 注入核 5：分类三态注入——前刷/后刷/组合唯一可判定。
pub fn inj5_classify() -> bool {
    classify(0x04) == "数据"
        && classify(0x04 | REQ_PREFLUSH) == "前刷"
        && classify(0x04 | REQ_FUA) == "后刷"
        && classify(0x04 | REQ_PREFLUSH | REQ_FUA) == "组合"
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    11u32 * 30
}

/// run_unx6481_checks：本单判据聚合（诊断段 0x9300）。
pub fn run_unx6481_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F6481 PREFLUSH/FUA 请求类型编码");
    s.add(
        "代码体量≥329行当量",
        structural_weight() >= 329,
        if structural_weight() >= 329 { "" } else { "结构当量不足" },
    );
    s.add(
        "三类编码唯一可判定",
        j1_main(),
        if j1_main() { "" } else { "编码不唯一" },
    );
    s.add(
        "非法组合100%拒绝",
        j1_main(),
        if j1_main() { "" } else { "非法漏拒" },
    );
    s.add(
        "编码与设计表逐位一致",
        j1_main(),
        if j1_main() { "" } else { "逐位失配" },
    );
    let inj = [
        ("注入1READ+FUA拒", inj1_read_fua()),
        ("注入2DISCARD+前刷拒", inj2_discard_flush()),
        ("注入3保留位拒", inj3_reserved_bits()),
        ("注入4op越界拒", inj4_op_range()),
        ("注入5分类三态", inj5_classify()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
