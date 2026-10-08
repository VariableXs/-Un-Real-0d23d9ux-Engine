//! F178 签名角标 · 批次八（v8）——签名到期预警、批量信任导入账、
//! 角标布局碰撞消解、审计周报帧。零堆、no_std。

use crate::checks::CheckSet;

/// 预警提前量（天——到期前 14 天转黄点）。
pub const EXPIRY_WARN_DAYS: u32 = 14;
/// 批量导入上限。
pub const IMPORT_CAP: usize = 20;
/// 布局列数（角标网格排布）。
pub const GRID_COLS: usize = 4;
/// 周报帧长（12B）。
pub const AUDIT_FRAME_LEN: usize = 12;

/// 签名到期预警：按剩余天数分级（0=无、1=临近、2=已到期）。
/// 语义：warn_days 内 → 1；≤0 → 2；其余 → 0。
pub fn expiry_level(days_left: i32) -> u8 {
    if days_left <= 0 {
        2
    } else if days_left <= EXPIRY_WARN_DAYS as i32 {
        1
    } else {
        0
    }
}

/// 批量信任导入账：逐条校验（id 非零 / 无重复），返回 (接受数, 拒绝明细数)。
#[derive(Clone, Copy)]
pub struct ImportBatch {
    pub accepted: u32,
    pub rejected_dup: u32,
    pub rejected_zero: u32,
    pub rejected_full: u32,
    ids: [u32; IMPORT_CAP],
    pub n: usize,
}

impl ImportBatch {
    pub const fn new() -> ImportBatch {
        ImportBatch { accepted: 0, rejected_dup: 0, rejected_zero: 0, rejected_full: 0, ids: [0; IMPORT_CAP], n: 0 }
    }

    pub fn import(&mut self, id: u32) -> bool {
        if id == 0 {
            self.rejected_zero += 1;
            return false;
        }
        for i in 0..self.n {
            if self.ids[i] == id {
                self.rejected_dup += 1;
                return false;
            }
        }
        if self.n >= IMPORT_CAP {
            self.rejected_full += 1;
            return false;
        }
        self.ids[self.n] = id;
        self.n += 1;
        self.accepted += 1;
        true
    }

    /// 总账守恒：接受 + 三类拒绝 = 提交总数（由调用方比对输入计数）。
    pub fn total_accounted(&self, submitted: u32) -> bool {
        self.accepted + self.rejected_dup + self.rejected_zero + self.rejected_full == submitted
    }
}

/// 角标网格布局：N 个角标按 4 列排布 → 行数；碰撞消解 = 严格网格化
/// （不重叠是布局不变量——坐标 (col, row) 唯一）。
pub fn grid_layout(count: usize) -> (usize, usize) {
    let rows = count.div_ceil(GRID_COLS);
    (rows, GRID_COLS)
}

/// 网格坐标唯一性：同格不双放（count ≤ rows×cols 恒成立）。
pub fn grid_no_collision(count: usize, rows: usize) -> bool {
    count <= rows * GRID_COLS
}

/// 角标层级裁决：严重度高的画在上层（遮挡方向明确——重的盖轻的）。
pub fn z_order(top_sev: u8, bottom_sev: u8) -> bool {
    top_sev >= bottom_sev
}

/// 审计周报帧（12B）：
/// [0..2) "SA" · [2..4) 周号 LE · [4..6) 信任总数 LE · [6..8) 到期预警数 LE ·
/// [8..10) 撤销数 LE · [10..12) 校验和（前 10B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_audit(week: u16, trusted: u16, expiring: u16, revoked: u16, out: &mut [u8; AUDIT_FRAME_LEN]) -> bool {
    if revoked > trusted {
        return false; // 撤销数 > 信任总数 = 账不平
    }
    out[0] = b'S';
    out[1] = b'A';
    out[2..4].copy_from_slice(&week.to_le_bytes());
    out[4..6].copy_from_slice(&trusted.to_le_bytes());
    out[6..8].copy_from_slice(&expiring.to_le_bytes());
    out[8..10].copy_from_slice(&revoked.to_le_bytes());
    let c = fnv16(&out[..10]);
    out[10] = (c & 0xFF) as u8;
    out[11] = (c >> 8) as u8;
    true
}

pub fn decode_audit(frame: &[u8; AUDIT_FRAME_LEN]) -> Option<(u16, u16, u16, u16)> {
    if frame[0] != b'S' || frame[1] != b'A' {
        return None;
    }
    let want = (frame[11] as u16) << 8 | frame[10] as u16;
    if fnv16(&frame[..10]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u16::from_le_bytes(frame[6..8].try_into().ok()?),
        u16::from_le_bytes(frame[8..10].try_into().ok()?),
    ))
}

#[inline(never)]
pub fn run_signbadge_b8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F178-b8");

    // 1) 到期分级三段：30 天无警、10 天临近、0/负已到期（线值逐点）。
    cs.add(
        "expiry_levels_exact",
        expiry_level(30) == 0
            && expiry_level(EXPIRY_WARN_DAYS as i32) == 1
            && expiry_level(EXPIRY_WARN_DAYS as i32 + 1) == 0
            && expiry_level(1) == 1
            && expiry_level(0) == 2
            && expiry_level(-5) == 2,
        "",
    );

    // 2) 批量导入：8 提交 → 6 收、1 重复拒、1 零 id 拒（明细各归其账）。
    let mut b = ImportBatch::new();
    let input: [u32; 8] = [11, 22, 33, 0, 22, 44, 55, 0];
    for id in input {
        b.import(id);
    }
    cs.add(
        "import_itemized",
        b.accepted == 5 && b.rejected_zero == 2 && b.rejected_dup == 1 && b.total_accounted(8),
        "",
    );

    // 3) 满容拒：20 满后第 21 条 → rejected_full 计数（账仍守恒）。
    let mut b2 = ImportBatch::new();
    for i in 1..=IMPORT_CAP as u32 {
        b2.import(i);
    }
    let full = !b2.import(999);
    cs.add("import_full_accounted", full && b2.rejected_full == 1 && b2.total_accounted(21), "");

    // 4) 网格布局：5 个角标 → 2 行 4 列（ceil 语义）、10 个 → 3 行。
    cs.add(
        "grid_rows_ceil",
        grid_layout(5) == (2, 4) && grid_layout(10) == (3, 4) && grid_layout(0) == (0, 4),
        "",
    );

    // 5) 网格无碰撞：count ≤ rows×cols 恒成立（布局不变量）。
    cs.add(
        "grid_no_collision",
        grid_no_collision(5, 2) && grid_no_collision(8, 2) && !grid_no_collision(9, 2),
        "",
    );

    // 6) 层级裁决：红盖灰、同严重度后画在上（>= 语义）。
    cs.add(
        "z_order_severity",
        z_order(2, 0) && z_order(1, 1) && !z_order(0, 2),
        "",
    );

    // 7) 审计帧 round-trip + 账平守门 + 撕裂拒。
    let mut f = [0u8; AUDIT_FRAME_LEN];
    let ok = encode_audit(20, 50, 6, 3, &mut f);
    let bad = !encode_audit(20, 2, 6, 3, &mut f);
    let mut tear_ok = true;
    for i in 0..AUDIT_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0x37;
        if decode_audit(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add(
        "audit_frame_guards",
        ok && decode_audit(&f) == Some((20, 50, 6, 3)) && bad && tear_ok,
        "",
    );

    // 8) 常量自洽：预警 14 天、导入 20、列 4。
    cs.add(
        "b8_constants",
        EXPIRY_WARN_DAYS == 14 && IMPORT_CAP == 20 && GRID_COLS == 4,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_all_zero_batch() {
        // 全零提交：全拒、零接受、账守恒（空批不是错误——是事实）。
        let mut b = ImportBatch::new();
        for _ in 0..3 {
            b.import(0);
        }
        assert_eq!(b.accepted, 0);
        assert_eq!(b.rejected_zero, 3);
        assert!(b.total_accounted(3));
    }

    #[test]
    fn grid_exact_multiple() {
        // 恰好整行：4 个 → 1 行（ceil 不虚增行数）。
        assert_eq!(grid_layout(4), (1, 4));
        assert_eq!(grid_layout(8), (2, 4));
    }

    #[test]
    fn expiry_boundary_sweep() {
        // -2..30 逐日扫：负与零=2、1..14=1、15..30=0（全界无跳档）。
        for d in -2..=0 {
            assert_eq!(expiry_level(d), 2, "day={d}");
        }
        for d in 1..=EXPIRY_WARN_DAYS as i32 {
            assert_eq!(expiry_level(d), 1, "day={d}");
        }
        for d in EXPIRY_WARN_DAYS as i32 + 1..=30 {
            assert_eq!(expiry_level(d), 0, "day={d}");
        }
    }
}
