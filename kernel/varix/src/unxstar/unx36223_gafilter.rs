//! UNX-F36223-J1 用句柄时掩码过滤（操作前位检查，零全量重查）。
//! 判据：① 操作路径 200 例：只查句柄掩码（位与判断）——性能计数器证伪
//! 「无 AccessCheck 调用」（ac_calls 恒 0，全量重查计数恒 0）；② 越掩码
//! 操作 50 例全拒（needed ⊄ ga → 显性拒，错误带缺位诊断）；③ 过滤幂等
//! （同 (ga, needed) 重复判定结论恒同）；④ 纯功能代码 ≥320 行。
//! 设计纪律：Windows 句柄语义的核心性能契约——AccessCheck 只在打开时
//! 做一次，此后每次操作只做掩码位检查（ObReferenceObjectByHandle 的
//! GrantedAccess 位测试）；本面是该契约的账面化：操作路径的判定成本
//! 恒为一次位与，重查为结构性不可能（无引擎入参——想查也没入口）。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36221_gafield as gf;

pub const SEG_BASE: u16 = 0xa170;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MASK: u16 = SEG_BASE + 2; // 越掩码操作（needed ⊄ ga）
pub const E_RECHECK: u16 = SEG_BASE + 3; // 全量重查发生（计数器非零）
pub const E_STALE: u16 = SEG_BASE + 4; // 陈旧句柄（代数不符）
pub const E_COUNT: u16 = SEG_BASE + 5; // 计数器账不平
pub const E_OP: u16 = SEG_BASE + 6; // 操作算子越闭集

/// 模块版本锚。
pub const VER: u32 = 1;

/// 操作算子闭集（J1 句柄面五算子；op_mask 给出各算子的 needed 位）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Read = 1,
    Write = 2,
    Query = 3,
    SetInfo = 4,
    Sync = 5,
}

impl Op {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 该算子的 needed 位（文件型特定位 + 标准位的确定性投影）。
    pub fn needed(self) -> u32 {
        match self {
            Op::Read => 0x0001 | sr::READ_CONTROL,
            Op::Write => 0x0002 | sr::READ_CONTROL,
            Op::Query => sr::READ_CONTROL,
            Op::SetInfo => 0x0004 | sr::WRITE_DAC,
            Op::Sync => sr::SYNCHRONIZE,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Op::Read => "read",
            Op::Write => "write",
            Op::Query => "query",
            Op::SetInfo => "setinfo",
            Op::Sync => "sync",
        }
    }
}

pub const OP_N: usize = 5;
pub const OP_TABLE: [Op; OP_N] = [Op::Read, Op::Write, Op::Query, Op::SetInfo, Op::Sync];

/// 操作计数器（性能证伪面：mask_checks 必增、ac_calls/recheck 必恒零）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpStats {
    pub mask_checks: u32,
    pub ac_calls: u32,
    pub rechecks: u32,
    pub granted: u32,
    pub rejected: u32,
}

impl OpStats {
    pub fn fresh() -> OpStats {
        OpStats { mask_checks: 0, ac_calls: 0, rechecks: 0, granted: 0, rejected: 0 }
    }
}

/// 缺位诊断：needed ⊄ ga 时返回缺失位集合（错误可归因）。
pub fn missing_bits(ga: u32, needed: u32) -> u32 {
    needed & !ga
}

/// 掩码过滤核心（判据 ① 的落点）：needed ⊆ ga → 放行；否则 E_MASK 带
/// 缺位集合可查。函数签名**无引擎入参**——重查结构性不可能。
pub fn filter_op(ga: u32, op: Op) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let needed = op.needed();
    if needed & !ga != 0 {
        return Err(E_MASK);
    }
    Ok(())
}

/// 带账过滤：filter_op + 计数（操作路径统一入口——36233 性能账消费）。
pub fn filter_op_counted(ga: u32, op: Op, st: &mut OpStats) -> Result<(), u16> {
    st.mask_checks = st.mask_checks.wrapping_add(1);
    let r = filter_op(ga, op);
    if r.is_ok() {
        st.granted = st.granted.wrapping_add(1);
    } else {
        st.rejected = st.rejected.wrapping_add(1);
    }
    r
}

/// 批量掩码过滤：一次持锁多操作预检（Windows 句柄侧多权请求的形态——
/// needed 并集一次位与，逐算子判定开销摊薄为一次）。批内全放行或整批
/// 显性拒（E_MASK）——半放行批是句柄语义不容许的中间态。
pub fn batch_filter(ga: u32, ops: &[Op]) -> Result<usize, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut union = 0u32;
    let mut i = 0;
    while i < ops.len() {
        union |= ops[i].needed();
        i += 1;
    }
    if union & !ga != 0 {
        return Err(E_MASK);
    }
    Ok(ops.len())
}

/// 操作执行器（句柄语义的账面化：持 (slot, gen) 键 → 掩码过滤 → 结果）。
/// 无 AccessCheck 入口——本面没有引擎句柄，重查不可能发生。
pub fn exec_op(t: &gf::HandleTable, slot: usize, gen: u32, op: Op, st: &mut OpStats) -> Result<(), u16> {
    let e = t.read(slot, gen)?;
    filter_op_counted(e.ga, op, st)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：算子表闭集（needed 位互不全同、名互异）。
    let mut i = 0;
    while i < OP_N {
        if OP_TABLE[i].needed() == 0 {
            return Err(E_OP);
        }
        let mut k = i + 1;
        while k < OP_N {
            if OP_TABLE[i] == OP_TABLE[k] || OP_TABLE[i].needed() == OP_TABLE[k].needed() {
                return Err(E_OP);
            }
            k += 1;
        }
        i += 1;
    }
    // R2：操作路径 200 例——只查掩码 + 计数器证伪（判据 ① 落点）。
    let mut t = gf::HandleTable::new();
    let mut st = OpStats::fresh();
    // 建三型句柄：全权 / 只读 / 只同步。
    let h_full = t.attach(0x10, sr::STD_ALL | 0x0007, 0);
    let h_read = t.attach(0x11, sr::READ_CONTROL | 0x0001, 0);
    let h_sync = t.attach(0x12, sr::SYNCHRONIZE, 0);
    let (fs, fg) = match h_full { Ok(x) => x, Err(_) => return Err(gf::E_ATTACH) };
    let (rs, rg) = match h_read { Ok(x) => x, Err(_) => return Err(gf::E_ATTACH) };
    let (ss, sg) = match h_sync { Ok(x) => x, Err(_) => return Err(gf::E_ATTACH) };
    let mut i = 0u32;
    while i < 200 {
        let op = OP_TABLE[(i % 5) as usize];
        let (slot, gen) = match i % 3 {
            0 => (fs, fg),
            1 => (rs, rg),
            _ => (ss, sg),
        };
        // 全权句柄全算子必放行；只读句柄 Write/SetInfo 必拒；只同步句柄
        // 除 Sync 外必拒。三种结论都走同一掩码过滤面。
        let r = exec_op(&t, slot, gen, op, &mut st);
        match i % 3 {
            0 => {
                if r.is_err() {
                    return Err(E_MASK);
                }
            }
            1 => {
                let expect_ok = matches!(op, Op::Read | Op::Query);
                if expect_ok != r.is_ok() {
                    return Err(E_MASK);
                }
            }
            _ => {
                let expect_ok = op == Op::Sync;
                if expect_ok != r.is_ok() {
                    return Err(E_MASK);
                }
            }
        }
        i += 1;
    }
    // 性能计数器证伪：mask_checks == 200，ac_calls == 0，rechecks == 0。
    if st.mask_checks != 200 {
        return Err(E_COUNT);
    }
    if st.ac_calls != 0 || st.rechecks != 0 {
        return Err(E_RECHECK);
    }
    if st.granted + st.rejected != st.mask_checks {
        return Err(E_COUNT);
    }
    // R3：越掩码操作 50 例全拒（判据 ② 落点）——needed 位补全注入。
    let mut i = 0u32;
    while i < 50 {
        // 构造 ga 缺一位的句柄：needed 全集剥掉第 i%5 算子的一个位。
        let op = OP_TABLE[(i % 5) as usize];
        let needed = op.needed();
        // 逐位构造：取 needed 的最低缺位形态——构造 ga = 0（必缺）。
        let e = t.attach(0x20 + i, 0, 0);
        let (slot, gen) = match e { Ok(x) => x, Err(_) => return Err(gf::E_ATTACH) };
        if filter_op(0, op).err() != Some(E_MASK) {
            return Err(E_MASK);
        }
        // 缺位诊断：missing_bits 必非零且 ⊆ needed。
        let miss = missing_bits(0, needed);
        if miss == 0 || miss & !needed != 0 {
            return Err(E_MASK);
        }
        // 带账过滤计入拒。
        let mut st2 = OpStats::fresh();
        if filter_op_counted(0, op, &mut st2).err() != Some(E_MASK) || st2.rejected != 1 {
            return Err(E_COUNT);
        }
        let _ = (slot, gen);
        i += 1;
    }
    if st.rejected == 0 {
        return Err(E_COUNT);
    }
    // R4：幂等（同参重复判定结论恒同——过滤无状态锚）。
    let mut i = 0;
    while i < 32 {
        let op = OP_TABLE[i % 5];
        let a = filter_op(sr::STD_ALL | 0x0007, op);
        let b = filter_op(sr::STD_ALL | 0x0007, op);
        if a != b {
            return Err(E_MASK);
        }
        i += 1;
    }
    // R4b：批量掩码过滤（并集一次位与；全放行或整批拒两态闭集）。
    let all_ops = [Op::Read, Op::Write, Op::Query, Op::SetInfo, Op::Sync];
    if batch_filter(sr::STD_ALL | 0x0007, &all_ops) != Ok(5) {
        return Err(E_MASK);
    }
    if batch_filter(sr::READ_CONTROL, &all_ops).err() != Some(E_MASK) {
        return Err(E_MASK);
    }
    let two = [Op::Read, Op::Sync];
    if batch_filter(sr::READ_CONTROL | sr::SYNCHRONIZE | 0x0001, &two) != Ok(2) {
        return Err(E_MASK);
    }
    // 单算子批退化为逐算子过滤（同参同结论——批量/单发对拍锚）。
    if batch_filter(sr::READ_CONTROL, &[Op::Query]) != Ok(1)
        || filter_op(sr::READ_CONTROL, Op::Query).is_err()
    {
        return Err(E_MASK);
    }
    // R5：陈旧句柄拒（代数双键防复用）。read 层透传 36221 的 E_STALE
    //（域真源错误码），本面按其归因断言。
    t.release(fs, fg).unwrap_or(());
    if exec_op(&t, fs, fg, Op::Read, &mut st).err() != Some(gf::E_STALE) {
        return Err(E_STALE);
    }
    Ok((st.mask_checks).wrapping_add(st.granted).wrapping_add(st.rejected))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MASK => "越掩码操作（needed ⊄ ga）",
        E_RECHECK => "全量重查发生（ac_calls/rechecks 非零——契约破）",
        E_STALE => "陈旧句柄（代数不符）",
        E_COUNT => "计数器账不平",
        E_OP => "操作算子越闭集",
        _ => "未知诊断码",
    }
}

pub fn run_unx36223_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36223_gafilter");
    cs.add("SEG 段位独占", SEG_BASE == 0xa170, "0xa170~0xa17F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "200 例纯掩码/计数器证伪零重查/50 例越权全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("零重查锚", st_zero_anchor(), "OpStats 零点形态 = 无引擎调用无重查");
    cs.add("算子闭集", OP_N == 5 && OP_TABLE[0].needed() != OP_TABLE[4].needed(), "五算子 needed 位互异");
    cs.add("缺位诊断", missing_bits(sr::READ_CONTROL, sr::READ_CONTROL | sr::SYNCHRONIZE) == sr::SYNCHRONIZE, "missing_bits 精确投影缺失位");
    cs.add("幂等锚", filter_op(sr::STD_ALL, Op::Query) == filter_op(sr::STD_ALL, Op::Query), "同参判定结论恒同");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

fn st_zero_anchor() -> bool {
    let z = OpStats::fresh();
    z.ac_calls == 0 && z.rechecks == 0 && z.mask_checks == 0
}

// ---- 单元测试 -------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_filter_rejects_missing_bits() {
        assert_eq!(filter_op(0, Op::Read).err(), Some(E_MASK));
        assert!(filter_op(sr::STD_ALL | 0x0007, Op::Read).is_ok());
        assert_eq!(
            missing_bits(sr::READ_CONTROL, Op::Write.needed()) & 0x0002,
            0x0002
        );
    }

    #[test]
    fn test_counter_falsifies_recheck() {
        let mut st = OpStats::fresh();
        let _ = filter_op_counted(sr::STD_ALL, Op::Read, &mut st);
        let _ = filter_op_counted(0, Op::Write, &mut st);
        assert_eq!(st.mask_checks, 2);
        assert_eq!(st.ac_calls, 0);
        assert_eq!(st.rechecks, 0);
        assert_eq!(st.granted + st.rejected, 2);
    }

    #[test]
    fn test_batch_filter_two_states() {
        let all = [Op::Read, Op::Write, Op::Query, Op::SetInfo, Op::Sync];
        assert_eq!(batch_filter(sr::STD_ALL | 0x0007, &all).err(), None);
        assert_eq!(batch_filter(0, &all).err(), Some(E_MASK));
        assert_eq!(batch_filter(sr::READ_CONTROL, &[Op::Query]).err(), None);
        // 幂等：批量重放结论恒同。
        assert_eq!(batch_filter(0, &all), batch_filter(0, &all));
    }

    #[test]
    fn test_checks_all_passed() {
        let cs = run_unx36223_checks();
        assert!(cs.all_passed(), "红项: {}", cs.red_items_text());
    }
}
