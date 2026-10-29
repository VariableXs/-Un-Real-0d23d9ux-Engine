//! UNX-F9068 glibc dup/fcntl 消费核（B08 号面）（0x62840 段）
//!
//! 锚点：UNX-F9068-J1 复制账（DUP/FCNTL/CLOEXEC 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 复制语义（DUP 加合 / FCNTL 并合 / CLOEXEC 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（GDF 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B14。
//!
//! 结构：复制账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! GDF 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 glibc dup/fcntl 消费核（B08 号面） 的语义单点：以三式账本承载 DUP/FCNTL/CLOEXEC
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62840 独占 0x20）。
pub const SEG_BASE: u32 = 0x62840;
pub const GDF_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const GDF_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const GDF_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const GDF_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const GDF_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// DUP 式：加合（new = old.wrapping_add(val)）。
pub const GDF_OP_DUP: u32 = 0;
/// FCNTL 式：并合（new = old | val）。
pub const GDF_OP_FCNTL: u32 = 1;
/// CLOEXEC 式：直设（new = val）。
pub const GDF_OP_CLOEXEC: u32 = 2;
/// 键域上界（1..=16）。
pub const GDF_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn gdf_op_name(code: u32) -> &'static str {
    match code {
        GDF_OP_DUP => "DUP",
        GDF_OP_FCNTL => "FCNTL",
        GDF_OP_CLOEXEC => "CLOEXEC",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn gdf_key_ok(key: u8) -> bool {
    key != 0 && key <= GDF_KEY_MAX
}

/// 领域辅助：值钳制（>64 截到 64，短值原样通过）。
pub fn gdf_clamp(val: u64) -> u64 {
    if val > 64 {
        64
    } else {
        val
    }
}

/// 单笔复制账。
#[derive(Clone)]
pub struct GlibcDupOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl GlibcDupOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            gdf_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn gdf_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        GDF_OP_DUP => old.wrapping_add(val),
        GDF_OP_FCNTL => old | val,
        GDF_OP_CLOEXEC => val,
        _ => old,
    }
}

/// 复制账本。
pub struct GlibcDupLedger {
    ops: Vec<GlibcDupOp>,
    state: u64,
    sealed: bool,
}

impl GlibcDupLedger {
    pub fn new() -> Self {
        GlibcDupLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔复制：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(GDF_SEALED);
        }
        if code > GDF_OP_CLOEXEC {
            return Err(GDF_BAD_OP);
        }
        if !gdf_key_ok(key) {
            return Err(GDF_KEY_OOB);
        }
        let old = self.state;
        let new = gdf_apply(old, code, val);
        self.ops.push(GlibcDupOp {
            code,
            key,
            val,
            old_state: old,
            new_state: new,
        });
        self.state = new;
        Ok(old)
    }

    /// 状态链守恒核：首笔 old = 0，逐笔 old = 前笔 new，
    /// 每笔 new 可由三式核重算复现，末笔 new = 当前状态。
    pub fn chain_ok(&self) -> bool {
        let mut expect: u64 = 0;
        for op in &self.ops {
            if op.old_state != expect {
                return false;
            }
            if op.new_state != gdf_apply(op.old_state, op.code, op.val) {
                return false;
            }
            expect = op.new_state;
        }
        self.state == expect
    }

    /// 键 key 是否已有落账笔。
    pub fn has_key(&self, key: u8) -> bool {
        self.ops.iter().any(|op| op.key == key)
    }

    /// 逐笔旧态回读序列。
    pub fn states(&self) -> Vec<u64> {
        self.ops.iter().map(|op| op.old_state).collect()
    }

    /// 各笔跨度序列（审计面）。
    pub fn deltas(&self) -> Vec<u64> {
        self.ops.iter().map(|op| op.delta()).collect()
    }

    /// GDF 指纹：状态 + 各笔操作折叠。
    pub fn gdf_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9068u32;
        h = h.wrapping_mul(0x0100_0193) ^ (self.state as u32);
        h = h.wrapping_mul(0x0100_0193) ^ ((self.state >> 32) as u32);
        for op in &self.ops {
            h = h.wrapping_mul(0x0100_0193) ^ op.code;
            h = h.wrapping_mul(0x0100_0193) ^ (op.key as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (op.val as u32);
            h ^= ((op.val >> 32) as u32) ^ ((op.old_state >> 32) as u32);
            h = h.wrapping_mul(0x0100_0193) ^ (op.new_state as u32);
        }
        h ^= self.sealed as u32;
        h = h.wrapping_mul(0x0100_0193);
        h
    }

    /// 封账印：空账拒 + 重封拒。
    pub fn seal(&mut self) -> bool {
        if self.sealed || self.ops.is_empty() {
            return false;
        }
        self.sealed = true;
        true
    }

    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 渲染导出。
    pub fn render(&self) -> String {
        let mut out = String::new();
        for op in &self.ops {
            out.push_str(&op.render_line());
            out.push('\n');
        }
        out.push_str(&format!("GDF-STATE|{}\n", self.state));
        out.push_str(&format!("GDF-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("GDF-SEAL\n");
        }
        out
    }
}

impl Default for GlibcDupLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（DUP[2]=14→FCNTL[9]=2048→DUP[11]=4→CLOEXEC[9]=165）。
/// 手验：0→14→2062→2066→165。
fn standard_gdf() -> GlibcDupLedger {
    let mut l = GlibcDupLedger::new();
    let _ = l.apply(GDF_OP_DUP, 2, 14); // old 0   → new 14
    let _ = l.apply(GDF_OP_FCNTL, 9, 2048); // old 14 → new 2062
    let _ = l.apply(GDF_OP_DUP, 11, 4); // old 2062 → new 2066
    let _ = l.apply(GDF_OP_CLOEXEC, 9, 165); // old 2066 → new 165
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = GlibcDupLedger::new();
    if l.apply(7, 1, 1).err() == Some(GDF_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = GlibcDupLedger::new();
    if l.apply(GDF_OP_DUP, 0, 1).err() == Some(GDF_KEY_OOB)
        && l.apply(GDF_OP_DUP, 17, 1).err() == Some(GDF_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_gdf();
    let _ = l.seal();
    if l.apply(GDF_OP_DUP, 1, 1).err() == Some(GDF_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = GlibcDupLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_gdf();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 复制账；J2 复制语义；J3 封账印。
pub fn run_unx9068_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9068");
    let p = standard_gdf();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9068-J1-操作全落账", ok, if ok { "" } else { "复制账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 14, 2062, 2066];
    s.add("9068-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("DUP|2|old0|new14")
        && text.contains("FCNTL|9|old14|new2062")
        && text.contains("DUP|11|old2062|new2066")
        && text.contains("CLOEXEC|9|old2066|new165")
        && text.contains("GDF-STATE|165")
        && text.contains("GDF-POPC|4");
    s.add("9068-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 165
    let ok = p.state == 165;
    s.add("9068-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 DUP 加合（0 + 14 = 14）
    let ok = p.ops[0].code == GDF_OP_DUP
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 14
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(14);
    s.add("9068-J2-DUP加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 FCNTL 并合（14 | 2048 = 2062）
    let ok = p.ops[1].code == GDF_OP_FCNTL
        && p.ops[1].old_state == 14
        && p.ops[1].new_state == 2062
        && p.ops[1].new_state == p.ops[1].old_state | 2048;
    s.add("9068-J2-FCNTL并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 CLOEXEC 直设（2066 → 165）
    let ok = p.ops[3].code == GDF_OP_CLOEXEC
        && p.ops[3].old_state == 2066
        && p.ops[3].new_state == 165
        && p.ops[3].new_state == 165;
    s.add("9068-J2-CLOEXEC直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = gdf_key_ok(2) && gdf_key_ok(16) && !gdf_key_ok(0) && !gdf_key_ok(17);
    s.add("9068-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 钳制辅助（值钳制（>64 截到 64））
    let ok = gdf_clamp(14) == 14 && gdf_clamp(999) == 64;
    s.add("9068-J2-钳制", ok, if ok { "" } else { "钳制漂移" });
    // J3-1 指纹确定
    let ok = standard_gdf().gdf_fp() == standard_gdf().gdf_fp();
    s.add("9068-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_gdf();
    let fp0 = p2.gdf_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.gdf_fp() != fp0;
    s.add("9068-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_gdf();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("GDF-SEAL");
    s.add("9068-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(GDF_OP_CLOEXEC, 0, 0).err() == Some(GDF_SEALED);
    s.add("9068-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9068-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9068_all_checks_green() {
        let set = run_unx9068_checks();
        assert!(set.all_passed(), "UNX-F9068 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9068_state_stable() {
        let l = standard_gdf();
        assert!(l.chain_ok());
        assert_eq!(l.state, 165);
        assert_eq!(l.state.count_ones(), 4);
    }
}
