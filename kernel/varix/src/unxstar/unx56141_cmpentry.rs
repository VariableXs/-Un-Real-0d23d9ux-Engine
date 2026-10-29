//! UNX-F56141 cmp_entry 账本骨架总装件（F 型收官旗舰）（0x611A0 段）
//!
//! 锚点：UNX-F56141-J1 账本骨架总装账（对照账本骨架五件套——行空间/
//! 只追加执行器接口位/判定总闸接口位/签章接口位/守闸接口位——逐件落
//! 位）；J2 骨架不变量（行号严格递增 + 只追加 + 封账只读三性质全绿）；
//! J3 总装指纹（五件接口位指纹折叠留痕）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·F 型总装旗舰面）。
//!
//! 结构：账本骨架（行空间预分配 + 行号发号器）+ 五件接口位登记（各
//! 接口位以能力标记落位，实现件各自独立）+ 骨架不变量核（递增/只追
//! 加/只读三性质）+ 总装指纹（FNV 链折叠）+ 注入核（跳号/倒插/封账
//! 后写/接口位重占/空骨架印必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x611A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x611A0;
pub const SKL_GAP: u32 = SEG_BASE + 0x01; // 行号断档（跳号发号）
pub const SKL_BACK_INSERT: u32 = SEG_BASE + 0x02; // 倒插（行号回退）
pub const SKL_WRITE_SEALED: u32 = SEG_BASE + 0x03; // 封账后写
pub const SKL_SLOT_DUP: u32 = SEG_BASE + 0x04; // 接口位重占
pub const SKL_EMPTY_SEAL: u32 = SEG_BASE + 0x05; // 空骨架收官印
pub const SKL_ROW_CAP: u32 = SEG_BASE + 0x06; // 行空间溢出

/// 五件接口位闭集。
pub const SLOT_ROWSPACE: usize = 0;
pub const SLOT_APPEND: usize = 1;
pub const SLOT_VERDICT: usize = 2;
pub const SLOT_SIGN: usize = 3;
pub const SLOT_GUARD: usize = 4;
pub const SLOT_COUNT: usize = 5;

/// 接口位名渲染。
pub const SLOT_NAMES: [&str; SLOT_COUNT] = ["rowspace", "append", "verdict", "sign", "guard"];

/// 行空间容量（骨架预分配上界）。
pub const ROW_SPACE_CAP: usize = 4096;

/// 骨架行：行号 × 场景占位 × 状态。
#[derive(Clone)]
pub struct SkeletonRow {
    pub row_no: u32,
    pub scene: String,
    pub filled: bool,
}

/// 账本骨架总装。
pub struct EntrySkeleton {
    rows: Vec<SkeletonRow>,
    next_row: u32,
    sealed: bool,
    slots: [bool; SLOT_COUNT],
}

impl EntrySkeleton {
    pub fn new() -> Self {
        EntrySkeleton {
            rows: Vec::with_capacity(ROW_SPACE_CAP.min(64)),
            next_row: 1,
            sealed: false,
            slots: [false; SLOT_COUNT],
        }
    }

    /// 追加一行：行号自动发号（严格递增），封账拒写，容量守卫。
    pub fn append_row(&mut self, scene: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(SKL_WRITE_SEALED);
        }
        if self.rows.len() >= ROW_SPACE_CAP {
            return Err(SKL_ROW_CAP);
        }
        let no = self.next_row;
        self.rows.push(SkeletonRow {
            row_no: no,
            scene: scene.to_string(),
            filled: false,
        });
        self.next_row += 1;
        Ok(no)
    }

    /// 回填一行内容（filled 置位）：行号必须存在且未回填。
    pub fn fill_row(&mut self, row_no: u32, scene: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(SKL_WRITE_SEALED);
        }
        if let Some(pos) = self.rows.iter().position(|r| r.row_no == row_no) {
            if self.rows[pos].filled {
                return Err(SKL_BACK_INSERT);
            }
            self.rows[pos].scene = scene.to_string();
            self.rows[pos].filled = true;
            Ok(())
        } else {
            Err(SKL_GAP)
        }
    }

    /// 占用接口位：一次性语义，重占必拒。
    pub fn occupy_slot(&mut self, slot: usize) -> Result<(), u32> {
        if slot >= SLOT_COUNT {
            return Err(SKL_SLOT_DUP);
        }
        if self.slots[slot] {
            return Err(SKL_SLOT_DUP);
        }
        self.slots[slot] = true;
        Ok(())
    }

    pub fn slot_taken(&self, slot: usize) -> bool {
        slot < SLOT_COUNT && self.slots[slot]
    }

    /// 收官印：五件接口位全占 + 行空间非空才可印。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.rows.is_empty() {
            return false;
        }
        if self.slots.iter().any(|s| !s) {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 不变量核：行号严格递增且从 1 连续。
    pub fn invariant_ok(&self) -> bool {
        self.rows
            .iter()
            .enumerate()
            .all(|(i, r)| r.row_no == i as u32 + 1)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn next_row_no(&self) -> u32 {
        self.next_row
    }

    /// 总装指纹：接口位占用 × 行数 × 收官态折叠。
    pub fn skeleton_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for (i, s) in self.slots.iter().enumerate() {
            h ^= ((*s as u32) << i) | (i as u32);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.rows.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 渲染导出。
    pub fn render_skeleton(&self) -> String {
        let mut out = String::new();
        for r in &self.rows {
            out.push_str(&format!(
                "r{}|{}|{}",
                r.row_no,
                r.scene,
                if r.filled { "F" } else { "E" }
            ));
            out.push('\n');
        }
        for (i, name) in SLOT_NAMES.iter().enumerate() {
            if self.slots[i] {
                out.push_str(&format!("slot:{}:OK\n", name));
            }
        }
        if self.sealed {
            out.push_str("SKELETON-SEAL\n");
        }
        out
    }
}

impl Default for EntrySkeleton {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三行全回填 + 五件接口位全占。
fn standard_skeleton() -> EntrySkeleton {
    let mut sk = EntrySkeleton::new();
    let r1 = sk.append_row("full_load").unwrap_or(0);
    let _ = sk.append_row("mid_load");
    let _ = sk.append_row("idle");
    let _ = sk.fill_row(r1, "full_load|60|57|9500");
    for s in 0..SLOT_COUNT {
        let _ = sk.occupy_slot(s);
    }
    sk
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：封账后写
    let mut sk = standard_skeleton();
    let _ = sk.seal();
    if sk.append_row("x") == Err(SKL_WRITE_SEALED) {
        caught += 1;
    }
    // 注 2：接口位重占
    let mut sk = EntrySkeleton::new();
    let _ = sk.occupy_slot(SLOT_SIGN);
    if sk.occupy_slot(SLOT_SIGN) == Err(SKL_SLOT_DUP) {
        caught += 1;
    }
    // 注 3：接口位越界
    let mut sk = EntrySkeleton::new();
    if sk.occupy_slot(9) == Err(SKL_SLOT_DUP) {
        caught += 1;
    }
    // 注 4：缺位拒印
    let mut sk = EntrySkeleton::new();
    let _ = sk.append_row("x");
    let _ = sk.occupy_slot(SLOT_ROWSPACE);
    if !sk.seal() && !sk.sealed() {
        caught += 1;
    }
    // 注 5：空骨架拒印
    let mut sk = EntrySkeleton::new();
    for s in 0..SLOT_COUNT {
        let _ = sk.occupy_slot(s);
    }
    if !sk.seal() && !sk.sealed() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 总装账；J2 骨架不变量；J3 总装指纹。
pub fn run_unx56141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56141");
    // J1-1 五件接口位全占
    let sk = standard_skeleton();
    let ok = (0..SLOT_COUNT).all(|i| sk.slot_taken(i));
    s.add("56141-J1-五件全占", ok, if ok { "" } else { "接口位缺失" });
    // J1-2 行空间三行落位
    let ok = sk.row_count() == 3 && sk.invariant_ok();
    s.add("56141-J1-行空间落位", ok, if ok { "" } else { "行空间漂移" });
    // J1-3 渲染导出齐
    let text = standard_skeleton().render_skeleton();
    let ok = text.contains("r1|full_load|60|57|9500|F") || text.contains("r1|")
        && text.contains("slot:rowspace:OK") && text.contains("slot:guard:OK");
    s.add("56141-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 行号发号器严格递增
    let mut sk2 = EntrySkeleton::new();
    let a = sk2.append_row("a").unwrap_or(0);
    let b = sk2.append_row("b").unwrap_or(0);
    let c = sk2.append_row("c").unwrap_or(0);
    let ok = a == 1 && b == 2 && c == 3 && sk2.invariant_ok();
    s.add("56141-J2-发号递增", ok, if ok { "" } else { "发号器漂移" });
    // J2-2 只追加：回填一次后二次回填拒
    let mut sk2 = EntrySkeleton::new();
    let r = sk2.append_row("x").unwrap_or(0);
    let first = sk2.fill_row(r, "x|1");
    let second = sk2.fill_row(r, "x|2");
    let ok = first.is_ok() && second == Err(SKL_BACK_INSERT);
    s.add("56141-J2-二次回填拒", ok, if ok { "" } else { "只追加闸失效" });
    // J2-3 回填未知行号拒
    let mut sk2 = EntrySkeleton::new();
    let ok = sk2.fill_row(99, "x") == Err(SKL_GAP);
    s.add("56141-J2-未知行拒", ok, if ok { "" } else { "行号核失效" });
    // J2-4 收官印只读
    let mut sk = standard_skeleton();
    let _ = sk.seal();
    let ok = sk.sealed() && sk.fill_row(1, "y") == Err(SKL_WRITE_SEALED);
    s.add("56141-J2-印后只读", ok, if ok { "" } else { "只读闸失效" });
    // J3-1 总装指纹确定性
    let ok = standard_skeleton().skeleton_fingerprint() == standard_skeleton().skeleton_fingerprint();
    s.add("56141-J3-指纹确定性", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 指纹随位变（占位变化指纹必变）
    let mut sk2 = EntrySkeleton::new();
    let _ = sk2.occupy_slot(SLOT_ROWSPACE);
    let fp_a = sk2.skeleton_fingerprint();
    let _ = sk2.occupy_slot(SLOT_SIGN);
    let ok = sk2.skeleton_fingerprint() != fp_a;
    s.add("56141-J3-指纹随位变", ok, if ok { "" } else { "折叠失效" });
    // J3-3 指纹随行数变
    let fp_empty = EntrySkeleton::new().skeleton_fingerprint();
    let mut sk2 = EntrySkeleton::new();
    let _ = sk2.append_row("x");
    let ok = sk2.skeleton_fingerprint() != fp_empty;
    s.add("56141-J3-指纹随行变", ok, if ok { "" } else { "折叠退化" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56141-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56141_all_checks_green() {
        let set = run_unx56141_checks();
        assert!(set.all_passed(), "UNX-F56141 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56141_invariant_stable() {
        assert!(standard_skeleton().invariant_ok());
    }
}
