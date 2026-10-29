//! UNX-F9008 信号队列与排队语义（rt 信号排队）（0x620c0 段）
//!
//! 锚点：UNX-F9008-J1 排队账（ENQ/DEQ/FLUSH 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 排队语义（ENQ 加合 / DEQ 并合 / FLUSH 直设三式运算 +
//! 键 1~64 域核 + popcount 守恒）；J3 封账印（QUE 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B11。
//!
//! 结构：排队账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! QUE 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 信号队列与排队语义（rt 信号排队） 的语义单点：以三式账本承载 ENQ/DEQ/FLUSH
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x620c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x620c0;
pub const QUE_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const QUE_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 64）
pub const QUE_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const QUE_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const QUE_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// ENQ 式：加合（new = old.wrapping_add(val)）。
pub const QUE_OP_ENQ: u32 = 0;
/// DEQ 式：并合（new = old | val）。
pub const QUE_OP_DEQ: u32 = 1;
/// FLUSH 式：直设（new = val）。
pub const QUE_OP_FLUSH: u32 = 2;
/// 键域上界（1..=64）。
pub const QUE_KEY_MAX: u8 = 64;

/// op 名渲染。
pub fn que_op_name(code: u32) -> &'static str {
    match code {
        QUE_OP_ENQ => "ENQ",
        QUE_OP_DEQ => "DEQ",
        QUE_OP_FLUSH => "FLUSH",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~64。
pub fn que_key_ok(key: u8) -> bool {
    key != 0 && key <= QUE_KEY_MAX
}

/// 领域辅助：状态留数（模 13 折叠，账页轮转位）。
pub fn que_mod13(state: u64) -> u32 {
    (state % 13) as u32
}

/// 单笔排队账。
#[derive(Clone)]
pub struct SigQueueOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl SigQueueOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            que_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn que_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        QUE_OP_ENQ => old.wrapping_add(val),
        QUE_OP_DEQ => old | val,
        QUE_OP_FLUSH => val,
        _ => old,
    }
}

/// 排队账本。
pub struct SigQueueLedger {
    ops: Vec<SigQueueOp>,
    state: u64,
    sealed: bool,
}

impl SigQueueLedger {
    pub fn new() -> Self {
        SigQueueLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔排队：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(QUE_SEALED);
        }
        if code > QUE_OP_FLUSH {
            return Err(QUE_BAD_OP);
        }
        if !que_key_ok(key) {
            return Err(QUE_KEY_OOB);
        }
        let old = self.state;
        let new = que_apply(old, code, val);
        self.ops.push(SigQueueOp {
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
            if op.new_state != que_apply(op.old_state, op.code, op.val) {
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

    /// QUE 指纹：状态 + 各笔操作折叠。
    pub fn que_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9008u32;
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
        out.push_str(&format!("QUE-STATE|{}\n", self.state));
        out.push_str(&format!("QUE-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("QUE-SEAL\n");
        }
        out
    }
}

impl Default for SigQueueLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（ENQ[6]=14→DEQ[29]=2048→ENQ[23]=7→FLUSH[37]=105）。
/// 手验：0→14→2062→2069→105。
fn standard_que() -> SigQueueLedger {
    let mut l = SigQueueLedger::new();
    let _ = l.apply(QUE_OP_ENQ, 6, 14); // old 0   → new 14
    let _ = l.apply(QUE_OP_DEQ, 29, 2048); // old 14 → new 2062
    let _ = l.apply(QUE_OP_ENQ, 23, 7); // old 2062 → new 2069
    let _ = l.apply(QUE_OP_FLUSH, 37, 105); // old 2069 → new 105
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = SigQueueLedger::new();
    if l.apply(7, 1, 1).err() == Some(QUE_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 65 双向）
    let mut l = SigQueueLedger::new();
    if l.apply(QUE_OP_ENQ, 0, 1).err() == Some(QUE_KEY_OOB)
        && l.apply(QUE_OP_ENQ, 65, 1).err() == Some(QUE_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_que();
    let _ = l.seal();
    if l.apply(QUE_OP_ENQ, 1, 1).err() == Some(QUE_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = SigQueueLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_que();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 排队账；J2 排队语义；J3 封账印。
pub fn run_unx9008_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9008");
    let p = standard_que();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9008-J1-操作全落账", ok, if ok { "" } else { "排队账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 14, 2062, 2069];
    s.add("9008-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("ENQ|6|old0|new14")
        && text.contains("DEQ|29|old14|new2062")
        && text.contains("ENQ|23|old2062|new2069")
        && text.contains("FLUSH|37|old2069|new105")
        && text.contains("QUE-STATE|105")
        && text.contains("QUE-POPC|4");
    s.add("9008-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 105
    let ok = p.state == 105;
    s.add("9008-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 ENQ 加合（0 + 14 = 14）
    let ok = p.ops[0].code == QUE_OP_ENQ
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 14
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(14);
    s.add("9008-J2-ENQ加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 DEQ 并合（14 | 2048 = 2062）
    let ok = p.ops[1].code == QUE_OP_DEQ
        && p.ops[1].old_state == 14
        && p.ops[1].new_state == 2062
        && p.ops[1].new_state == p.ops[1].old_state | 2048;
    s.add("9008-J2-DEQ并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 FLUSH 直设（2069 → 105）
    let ok = p.ops[3].code == QUE_OP_FLUSH
        && p.ops[3].old_state == 2069
        && p.ops[3].new_state == 105
        && p.ops[3].new_state == 105;
    s.add("9008-J2-FLUSH直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~64 内收、0 与 65 拒）
    let ok = que_key_ok(6) && que_key_ok(64) && !que_key_ok(0) && !que_key_ok(65);
    s.add("9008-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 留数辅助（状态留数（模 13 折叠））
    let ok = que_mod13(2069) == 2 && que_mod13(0) == 0;
    s.add("9008-J2-留数", ok, if ok { "" } else { "留数漂移" });
    // J3-1 指纹确定
    let ok = standard_que().que_fp() == standard_que().que_fp();
    s.add("9008-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_que();
    let fp0 = p2.que_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.que_fp() != fp0;
    s.add("9008-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_que();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("QUE-SEAL");
    s.add("9008-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(QUE_OP_FLUSH, 0, 0).err() == Some(QUE_SEALED);
    s.add("9008-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9008-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9008_all_checks_green() {
        let set = run_unx9008_checks();
        assert!(set.all_passed(), "UNX-F9008 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9008_state_stable() {
        let l = standard_que();
        assert!(l.chain_ok());
        assert_eq!(l.state, 105);
        assert_eq!(l.state.count_ones(), 4);
    }
}
