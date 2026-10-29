//! UNX-F9030 SysV 共享内存挂点（shmget/shmat 登记档）（0x62380 段）
//!
//! 锚点：UNX-F9030-J1 共内存账（GET/AT/DT 三式全落账 + 状态链逐笔回读 +
//! 渲染导出面）；J2 共内存语义（GET 加合 / AT 并合 / DT 直设三式运算 +
//! 键 1~8 域核 + popcount 守恒）；J3 封账印（SHM 指纹 + 封印后
//! 只读 + 注入核）三判据全绿。
//! 域：CoRun·UNX-C2-B12。
//!
//! 结构：共内存账本 + 三式分派核（非法 op 拒 + 键越界拒）+ 状态回读链 +
//! SHM 指纹 + 注入核（非法 op / 键越界 / 印后改账 / 空账封印 / 链断裂
//! 必抓）。
//!
//! 领域注记：本档为 SysV 共享内存挂点（shmget/shmat 登记档） 的语义单点：以三式账本承载 GET/AT/DT
//! 领域操作序，键值逐笔登记、状态链逐笔回读；指纹与封账印保障账面可审
//! 计，注入核覆盖五类非法路径。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x62380 独占 0x20）。
pub const SEG_BASE: u32 = 0x62380;
pub const SHM_BAD_OP: u32 = SEG_BASE + 0x01; // 非法 op（非三式之一）
pub const SHM_KEY_OOB: u32 = SEG_BASE + 0x02; // 键越界（0 或 > 8）
pub const SHM_SEALED: u32 = SEG_BASE + 0x03; // 封账印后改账
pub const SHM_EMPTY_SEAL: u32 = SEG_BASE + 0x04; // 空账封印（无操作记录）
pub const SHM_CHAIN: u32 = SEG_BASE + 0x05; // 状态链断裂（回读不守恒）

/// GET 式：加合（new = old.wrapping_add(val)）。
pub const SHM_OP_GET: u32 = 0;
/// AT 式：并合（new = old | val）。
pub const SHM_OP_AT: u32 = 1;
/// DT 式：直设（new = val）。
pub const SHM_OP_DT: u32 = 2;
/// 键域上界（1..=8）。
pub const SHM_KEY_MAX: u8 = 8;

/// op 名渲染。
pub fn shm_op_name(code: u32) -> &'static str {
    match code {
        SHM_OP_GET => "GET",
        SHM_OP_AT => "AT",
        SHM_OP_DT => "DT",
        _ => "BADOP",
    }
}

/// 键域核：键必须落在 1~8。
pub fn shm_key_ok(key: u8) -> bool {
    key != 0 && key <= SHM_KEY_MAX
}

/// 领域辅助：两态跨度（饱和差，b < a 时归零）。
pub fn shm_span(a: u64, b: u64) -> u64 {
    b.saturating_sub(a)
}

/// 单笔共内存账。
#[derive(Clone)]
pub struct ShmOp {
    pub code: u32,
    pub key: u8,
    pub val: u64,
    pub old_state: u64,
    pub new_state: u64,
}

impl ShmOp {
    /// 本笔跨度（饱和差，审计辅助面）。
    fn delta(&self) -> u64 {
        self.new_state.saturating_sub(self.old_state)
    }

    fn render_line(&self) -> String {
        format!(
            "{}|{}|old{}|new{}",
            shm_op_name(self.code),
            self.key,
            self.old_state,
            self.new_state
        )
    }
}

/// 三式运算核。
pub fn shm_apply(old: u64, code: u32, val: u64) -> u64 {
    match code {
        SHM_OP_GET => old.wrapping_add(val),
        SHM_OP_AT => old | val,
        SHM_OP_DT => val,
        _ => old,
    }
}

/// 共内存账本。
pub struct ShmLedger {
    ops: Vec<ShmOp>,
    state: u64,
    sealed: bool,
}

impl ShmLedger {
    pub fn new() -> Self {
        ShmLedger {
            ops: Vec::new(),
            state: 0,
            sealed: false,
        }
    }

    /// 应用一笔共内存：三式分派 + 越界拒 + 印后拒。
    /// Ok(old) 即旧态回读。
    pub fn apply(&mut self, code: u32, key: u8, val: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(SHM_SEALED);
        }
        if code > SHM_OP_DT {
            return Err(SHM_BAD_OP);
        }
        if !shm_key_ok(key) {
            return Err(SHM_KEY_OOB);
        }
        let old = self.state;
        let new = shm_apply(old, code, val);
        self.ops.push(ShmOp {
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
            if op.new_state != shm_apply(op.old_state, op.code, op.val) {
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

    /// SHM 指纹：状态 + 各笔操作折叠。
    pub fn shm_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5 ^ 9030u32;
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
        out.push_str(&format!("SHM-STATE|{}\n", self.state));
        out.push_str(&format!("SHM-POPC|{}\n", self.state.count_ones()));
        if self.sealed {
            out.push_str("SHM-SEAL\n");
        }
        out
    }
}

impl Default for ShmLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三式四连操作（GET[4]=12→AT[3]=512→GET[1]=8→DT[7]=127）。
/// 手验：0→12→524→532→127。
fn standard_shm() -> ShmLedger {
    let mut l = ShmLedger::new();
    let _ = l.apply(SHM_OP_GET, 4, 12); // old 0   → new 12
    let _ = l.apply(SHM_OP_AT, 3, 512); // old 12 → new 524
    let _ = l.apply(SHM_OP_GET, 1, 8); // old 524 → new 532
    let _ = l.apply(SHM_OP_DT, 7, 127); // old 532 → new 127
    l
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：非法 op（7 非三式之一）
    let mut l = ShmLedger::new();
    if l.apply(7, 1, 1).err() == Some(SHM_BAD_OP) {
        caught += 1;
    }
    // 注 2：键越界（0 与 9 双向）
    let mut l = ShmLedger::new();
    if l.apply(SHM_OP_GET, 0, 1).err() == Some(SHM_KEY_OOB)
        && l.apply(SHM_OP_GET, 9, 1).err() == Some(SHM_KEY_OOB)
    {
        caught += 1;
    }
    // 注 3：封账印后改账
    let mut l = standard_shm();
    let _ = l.seal();
    if l.apply(SHM_OP_GET, 1, 1).err() == Some(SHM_SEALED) {
        caught += 1;
    }
    // 注 4：空账封印
    let mut l = ShmLedger::new();
    if !l.seal() && !l.sealed() {
        caught += 1;
    }
    // 注 5：状态链断裂（篡改中段旧态）
    let mut l = standard_shm();
    l.ops[1].old_state += 1;
    if !l.chain_ok() {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 共内存账；J2 共内存语义；J3 封账印。
pub fn run_unx9030_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9030");
    let p = standard_shm();
    // J1-1 三式四连操作全落账
    let ok = p.ops.len() == 4 && p.chain_ok();
    s.add("9030-J1-操作全落账", ok, if ok { "" } else { "共内存账不全" });
    // J1-2 状态链逐笔回读
    let ok = p.states() == vec![0u64, 12, 524, 532];
    s.add("9030-J1-状态回读", ok, if ok { "" } else { "状态链漂移" });
    // J1-3 渲染导出齐
    let text = p.render();
    let ok = text.contains("GET|4|old0|new12")
        && text.contains("AT|3|old12|new524")
        && text.contains("GET|1|old524|new532")
        && text.contains("DT|7|old532|new127")
        && text.contains("SHM-STATE|127")
        && text.contains("SHM-POPC|7");
    s.add("9030-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 终态 = 127
    let ok = p.state == 127;
    s.add("9030-J2-终态", ok, if ok { "" } else { "终态漂移" });
    // J2-2 GET 加合（0 + 12 = 12）
    let ok = p.ops[0].code == SHM_OP_GET
        && p.ops[0].old_state == 0
        && p.ops[0].new_state == 12
        && p.ops[0].new_state == p.ops[0].old_state.wrapping_add(12);
    s.add("9030-J2-GET加合", ok, if ok { "" } else { "加合漂移" });
    // J2-3 AT 并合（12 | 512 = 524）
    let ok = p.ops[1].code == SHM_OP_AT
        && p.ops[1].old_state == 12
        && p.ops[1].new_state == 524
        && p.ops[1].new_state == p.ops[1].old_state | 512;
    s.add("9030-J2-AT并合", ok, if ok { "" } else { "并合漂移" });
    // J2-4 DT 直设（532 → 127）
    let ok = p.ops[3].code == SHM_OP_DT
        && p.ops[3].old_state == 532
        && p.ops[3].new_state == 127
        && p.ops[3].new_state == 127;
    s.add("9030-J2-DT直设", ok, if ok { "" } else { "直设漂移" });
    // J2-5 键域核（1~8 内收、0 与 9 拒）
    let ok = shm_key_ok(4) && shm_key_ok(8) && !shm_key_ok(0) && !shm_key_ok(9);
    s.add("9030-J2-键域核", ok, if ok { "" } else { "键域失效" });
    // J2-6 跨度辅助（两态跨度（饱和差））
    let ok = shm_span(0, 12) == 12 && shm_span(12, 12) == 0;
    s.add("9030-J2-跨度", ok, if ok { "" } else { "跨度漂移" });
    // J3-1 指纹确定
    let ok = standard_shm().shm_fp() == standard_shm().shm_fp();
    s.add("9030-J3-指纹确定", ok, if ok { "" } else { "指纹漂移" });
    // J3-2 账变签变
    let mut p2 = standard_shm();
    let fp0 = p2.shm_fp();
    p2.ops[0].new_state += 1;
    let ok = p2.shm_fp() != fp0;
    s.add("9030-J3-账变签变", ok, if ok { "" } else { "折叠退化" });
    // J3-3 封账印（印面入渲染）
    let mut p3 = standard_shm();
    let sealed_ok = p3.seal();
    let ok = sealed_ok && p3.sealed() && p3.render().contains("SHM-SEAL");
    s.add("9030-J3-封账印", ok, if ok { "" } else { "封印失败" });
    // J3-4 印后拒改
    let ok = p3.apply(SHM_OP_DT, 0, 0).err() == Some(SHM_SEALED);
    s.add("9030-J3-印后拒改", ok, if ok { "" } else { "只读闸失效" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("9030-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx9030_all_checks_green() {
        let set = run_unx9030_checks();
        assert!(set.all_passed(), "UNX-F9030 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx9030_state_stable() {
        let l = standard_shm();
        assert!(l.chain_ok());
        assert_eq!(l.state, 127);
        assert_eq!(l.state.count_ones(), 7);
    }
}
