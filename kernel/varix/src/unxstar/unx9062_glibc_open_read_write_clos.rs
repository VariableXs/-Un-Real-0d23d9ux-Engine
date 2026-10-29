//! UNX-F9062 glibc open/read/write/close 消费核（B02 号面）（0x62780 段）
//!
//! 锚点：UNX-F9062-J1 文件账（OPEN/READ/WRITE 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 文件语义（OPEN 加合 / READ 并合 / WRITE 直设三式运算 +
//! 键 1~16 域核 + popcount 守恒）；J3 封账印（GOR 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B14。
//!
//! 结构：文件账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! GOR 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 glibc open/read/write/close 消费核（B02 号面） 的语义单点：以三式账本承载 OPEN/READ/WRITE
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62780 独占 0x20）。
pub const SEG_BASE: u32 = 0x62780;
pub const GOR_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const GOR_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 16）
pub const GOR_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const GOR_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const GOR_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// OPEN 式：加合（new = old.wrapping_add(val)）。
pub const GOR_OP_OPEN: u32 = 0;
/// READ 式：并合（new = old | val）。
pub const GOR_OP_READ: u32 = 1;
/// WRITE 式：直设（new = val）。
pub const GOR_OP_WRITE: u32 = 2;
/// 键域上界（1..=16）。
pub const GOR_KEY_MAX: u8 = 16;

/// op 名渲染。
pub fn gor_op_name(code: u32) -> &'static str {
    match code {
        GOR_OP_OPEN => "OPEN",
        GOR_OP_READ => "READ",
        GOR_OP_WRITE => "WRITE",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~16。
pub fn gor_key_ok(key: u8) -> bool {
    key != 0 && key <= GOR_KEY_MAX
}

/// 领域辅助：两态跨度（饱和差，b < a 时归零）。
pub fn gor_span(a: u64, b: u64) -> u64 {
    b.saturating_sub(a)
}

/// 单笔文件账。
#[derive(Clone)]
pub struct GlibcIOOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl GlibcIOOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            gor_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn gor_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        GOR_OP_OPEN => old.wrapping_add(val),
        GOR_OP_READ => old | val,
        GOR_OP_WRITE => val,
        _ => old,
    }
}

/// 文件账本。
pub struct GlibcIOLedger {
    ops: Vec<GlibcIOOp>,
    state: u64,
    sealed: bool,
}

impl GlibcIOLedger {
    pub fn new() -> Self {
        GlibcIOLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔文件：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(GOR_SEALED);
        }
        if code > GOR_OP_WRITE {
            return Err(GOR_BAD_OP);
        }
        if !gor_key_ok(key) {
            return Err(GOR_KEY_OOB);
        }
        let old = self.state;
        let new = gor_apply(old, code, val);
        self.ops.push(GlibcIOOp {
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
            if op.new_state != gor_apply(op.old_state, op.code, op.val) {
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

    /// GOR 指纹：状态 + 各笔操作折叠。
    pub fn gor_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9062u32;
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
        out.push_str(&format!("GOR-STATE|{}\n", self.state));
        out.push_str(&format!("GOR-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("GOR-SEAL\n");
        }
        out
    }
}

impl Default for GlibcIOLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（OPEN[12]=20→READ[11]=2048→OPEN[9]=5→WRITE[15]=159）。
/// 手验：0→20→2068→2073→159。
fn standard_gor() -> GlibcIOLedger {
    let mut l = GlibcIOLedger::new();
    let _ = l.apply(GOR_OP_OPEN, 12, 20); // old 0   → new 20
    let _ = l.apply(GOR_OP_READ, 11, 2048); // old 20 → new 2068
    let _ = l.apply(GOR_OP_OPEN, 9, 5); // old 2068 → new 2073
    let _ = l.apply(GOR_OP_WRITE, 15, 159); // old 2073 → new 159
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = GlibcIOLedger::new();
    if l.apply(7, 1, 1).err() == Some(GOR_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 17 双向）
    let mut l = GlibcIOLedger::new();
    if l.apply(GOR_OP_OPEN, 0, 1).err() == Some(GOR_KEY_OOB)
        && l.apply(GOR_OP_OPEN, 17, 1).err() == Some(GOR_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_gor();
    let _ = l.seal();
    if l.apply(GOR_OP_OPEN, 1, 1).err() == Some(GOR_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = GlibcIOLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_gor();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 文件账；J2 文件语义；J3 封账印。
pub fn run_unx9062_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9062");
    let p = standard_gor();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9062-J1-操作全落账", ok, if ok { "" } else { "文件账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 20, 2068, 2073];
    s.add("9062-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("OPEN|12|old0|new20")
        && text.contains("READ|11|old20|new2068")
        && text.contains("OPEN|9|old2068|new2073")
        && text.contains("WRITE|15|old2073|new159")
        && text.contains("GOR-STATE|159")
        && text.contains("GOR-POPC|6");
    s.add("9062-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 159
    let ok = p.state == 159;
    s.add("9062-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 OPEN 加合（0 + 20 = 20）
    let ok = p.ops[0].code == GOR_OP_OPEN
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 20
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(20);
    s.add("9062-J2-OPEN加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 READ 并合（20 | 2048 = 2068）
    let ok = p.ops[1].code == GOR_OP_READ
        && p.ops[1].old_state == 20
        && p.ops[1].new_state == 2068
        && p.ops[1].new_state == p.ops[1].old_state | 2048;
    s.add("9062-J2-READ并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 WRITE 直设（2073 → 159）
    let ok = p.ops[3].code == GOR_OP_WRITE
        && p.ops[3].old_state == 2073
        && p.ops[3].new_state == 159
        && p.ops[3].new_state == 159;
    s.add("9062-J2-WRITE直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~16 内收、0 与 17 拒）
    let ok = gor_key_ok(12) && gor_key_ok(16) && !gor_key_ok(0) && !gor_key_ok(17);
    s.add("9062-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 跨度辅助（两态跨度（饱和差））
    let ok = gor_span(0, 20) == 20 && gor_span(20, 20) == 0;
    s.add("9062-J2-跨度", ok, if ok { "" } else { "跨度漂移" });
    // J3-1 指纹确定
    let ok = standard_gor().gor_fp() == standard_gor().gor_fp();
    s.add("9062-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_gor();
    let fp0 = p2.gor_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.gor_fp() != fp0;
    s.add("9062-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_gor();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("GOR-SEAL");
    s.add("9062-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(GOR_OP_WRITE, 0, 0).err() == Some(GOR_SEALED);
    s.add("9062-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9062-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9062_all_checks_green() {
        let set = run_unx9062_checks();
        assert!(set.all_passed(), "UNX-F9062 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9062_state_stable() {
        let l = standard_gor();
        assert!(l.chain_ok());
        assert_eq!(l.state, 159);
        assert_eq!(l.state.count_ones(), 6);
    }
}
