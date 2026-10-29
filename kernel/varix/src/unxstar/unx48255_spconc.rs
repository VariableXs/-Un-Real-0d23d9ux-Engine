//! unx48255_spconc — UNX-F48255 B13 批并发方案纪律（多焦点各自方案态隔离）（诊断段 0xC0C0）。
//!
//! 判据原文：
//!   J1 并发 ×10^3 零互染；判据锚定 M5 母版。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 焦点槽模型：每个焦点槽独立持有自己的方案态（方案 id、简繁
//!      开关、组串缓冲深度），槽间无共享可变字段；操作只带槽号寻址。
//!   2. 互染定义：任一槽的可见态因另一槽的操作而改变，或某槽的操作
//!      账出现他槽槽号——两种形态任一出现即互染。
//!   3. 并发口径（J1 ×10^3）：4 槽轮流注入 1000 条操作（每槽各得一条
//!      确定性子序列），逐槽独立重放各自子序列得期望终态，与并发跑
//!      后的实存终态逐槽逐位比对——全等才零互染。
//!   4. 期望独立推演：期望态由「槽号→子序列→纯函数推演」得出，与
//!      并发执行器零代码共享推演路径（双路径对账防同错）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC0C0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CROSS_TALK: u16 = SEG_BASE + 1;  // 槽间互染
const E_SLOT_BAD: u16 = SEG_BASE + 2;    // 槽号越界
const E_CONC_MISS: u16 = SEG_BASE + 3;   // ×10^3 操作不足
const E_LEDGER_BAD: u16 = SEG_BASE + 4;  // 操作账不平
const E_STATE_BAD: u16 = SEG_BASE + 5;   // 方案态闭集外
const E_EMPTY_SUM: u16 = SEG_BASE + 6;   // 写回摘要为空

/// 焦点槽数。
pub const SLOT_COUNT: usize = 4;
/// 并发操作数（J1 的 ×10^3）。
pub const OP_TOTAL: u32 = 1000;

/// 方案 id 闭集（三双拼）。
pub const SP_XIAOHE: u8 = 0;
pub const SP_ZIRANMA: u8 = 1;
pub const SP_MS: u8 = 2;

/// 操作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConcOp {
    SetScheme(u8),
    S2tToggle,
    CompPush(u8), // 组串入一字节（缓冲深度 +1）
    CompPop,      // 组串退一字节
    FocusOff,     // 焦点离开（缓冲清空）
}

/// 一个焦点槽的方案态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotState {
    pub scheme: u8,
    pub s2t: bool,
    pub comp_depth: u32,
    pub focused: bool,
}

impl SlotState {
    fn fresh() -> SlotState {
        SlotState { scheme: SP_XIAOHE, s2t: false, comp_depth: 0, focused: true }
    }
}

/// 并发执行器：槽阵 + 全局操作账。
pub struct SpConc {
    slots: [SlotState; SLOT_COUNT],
    touches: Vec<u8>, // 每条操作的槽号账（互染检测用）
    pub op_total: u32,
}

impl SpConc {
    pub fn new() -> SpConc {
        SpConc {
            slots: [SlotState::fresh(), SlotState::fresh(), SlotState::fresh(), SlotState::fresh()],
            touches: Vec::new(),
            op_total: 0,
        }
    }

    /// 单操作注入（唯一写入口，按槽寻址）。
    pub fn apply(&mut self, slot: usize, op: ConcOp) -> Result<(), u16> {
        if slot >= SLOT_COUNT {
            return Err(E_SLOT_BAD);
        }
        self.touches.push(slot as u8);
        self.op_total += 1;
        let s = &mut self.slots[slot];
        match op {
            ConcOp::SetScheme(id) => {
                if id > SP_MS {
                    return Err(E_STATE_BAD);
                }
                s.scheme = id;
            }
            ConcOp::S2tToggle => {
                s.s2t = !s.s2t;
            }
            ConcOp::CompPush(_) => {
                s.comp_depth += 1;
            }
            ConcOp::CompPop => {
                if s.comp_depth > 0 {
                    s.comp_depth -= 1;
                }
            }
            ConcOp::FocusOff => {
                s.comp_depth = 0;
                s.focused = false;
            }
        }
        Ok(())
    }

    /// 实存终态快照。
    pub fn snapshot(&self, slot: usize) -> Result<SlotState, u16> {
        if slot >= SLOT_COUNT {
            return Err(E_SLOT_BAD);
        }
        Ok(self.slots[slot])
    }

    /// 互染检测一：操作账内每条槽号必须 ∈ 闭集（写入寻址可回溯）。
    pub fn audit_touch_domain(&self) -> Result<(), u16> {
        if self.touches.len() as u32 != self.op_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.touches.len() {
            if self.touches[i] as usize >= SLOT_COUNT {
                return Err(E_CROSS_TALK);
            }
            i += 1;
        }
        Ok(())
    }

    /// ×N 覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.op_total < want {
            return Err(E_CONC_MISS);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.op_total == 0 {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性子序列生成：第 slot 槽的第 k 条操作（与执行器无关的独立生成器）。
pub fn gen_op(slot: usize, k: u32) -> ConcOp {
    let m = (slot as u32 + k) % 5;
    match m {
        0 => ConcOp::SetScheme(SP_XIAOHE + (slot as u8 + (k % 3) as u8) % 3),
        1 => ConcOp::S2tToggle,
        2 => ConcOp::CompPush((k % 26) as u8),
        3 => ConcOp::CompPop,
        _ => ConcOp::FocusOff,
    }
}

/// 独立推演：对单槽重放子序列得期望终态（纪律 4：与执行器零共享路径）。
pub fn replay_expected(slot: usize, k_count: u32) -> Result<SlotState, u16> {
    let mut st = SlotState::fresh();
    let mut k = 0u32;
    while k < k_count {
        match gen_op(slot, k) {
            ConcOp::SetScheme(id) => {
                if id > SP_MS {
                    return Err(E_STATE_BAD);
                }
                st.scheme = id;
            }
            ConcOp::S2tToggle => st.s2t = !st.s2t,
            ConcOp::CompPush(_) => st.comp_depth += 1,
            ConcOp::CompPop => {
                if st.comp_depth > 0 {
                    st.comp_depth -= 1;
                }
            }
            ConcOp::FocusOff => {
                st.comp_depth = 0;
                st.focused = false;
            }
        }
        k += 1;
    }
    Ok(st)
}

/// 自例程判据：逐条落实 J1（并发 ×10^3 零互染），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主并发 ×10^3：4 槽轮流 1000 条操作；逐槽独立推演期望终态，
    //    与实存终态逐槽逐位比对（零互染主判据）。
    let mut sc = SpConc::new();
    let mut i = 0u32;
    while i < OP_TOTAL {
        let slot = (i % SLOT_COUNT as u32) as usize;
        let k = i / SLOT_COUNT as u32; // 该槽内子序列序号
        sc.apply(slot, gen_op(slot, k))?;
        i += 1;
    }
    sc.coverage(OP_TOTAL)?;
    sc.audit_touch_domain()?;
    // 每槽子序列长度：前 OP_TOTAL/SLOT_COUNT 条完整，余数槽多 1 条。
    let per = OP_TOTAL / SLOT_COUNT as u32;
    let rem = OP_TOTAL % SLOT_COUNT as u32;
    let mut s = 0usize;
    while s < SLOT_COUNT {
        let k_count = if (s as u32) < rem { per + 1 } else { per };
        let want = replay_expected(s, k_count)?;
        let got = sc.snapshot(s)?;
        if want != got {
            return Err(E_CROSS_TALK);
        }
        s += 1;
    }
    hits += 1;

    // R2 期望独立推演可复算：replay_expected 同参两次调用逐位相等
    //    （纯函数红线）。
    let mut s = 0usize;
    while s < SLOT_COUNT {
        let a = replay_expected(s, 137)?;
        let b = replay_expected(s, 137)?;
        if a != b {
            return Err(E_STATE_BAD);
        }
        s += 1;
    }
    hits += 1;

    // R3 槽号越界拒收：apply(4, ...) 拒且账零增长；snapshot(4) 拒。
    let mut sc = SpConc::new();
    if sc.apply(SLOT_COUNT, ConcOp::S2tToggle).is_ok() {
        return Err(E_SLOT_BAD);
    }
    if sc.snapshot(SLOT_COUNT).is_ok() {
        return Err(E_SLOT_BAD);
    }
    if sc.op_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 互染抓拍：伪造一条越域槽号账（模拟互染被审计捕获），
    //    audit_touch_domain 必红（诚实记账口径）。
    let mut sc = SpConc::new();
    sc.apply(0, ConcOp::S2tToggle)?;
    sc.touches.push(9); // 越域槽号（互染形态二）
    if sc.audit_touch_domain().is_ok() {
        return Err(E_CROSS_TALK);
    }
    hits += 1;

    // R5 方案 id 闭集：SetScheme(3) 越闭集拒收，槽态保持旧值。
    let mut sc = SpConc::new();
    sc.apply(1, ConcOp::SetScheme(SP_ZIRANMA))?;
    if sc.apply(1, ConcOp::SetScheme(3)).is_ok() {
        return Err(E_STATE_BAD);
    }
    let st = sc.snapshot(1)?;
    if st.scheme != SP_ZIRANMA {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R6 焦点离开清缓冲：CompPush×5 后 FocusOff 深度归零且 focused=false；
    //    其余槽不受影响（零互染的点状验证）。
    let mut sc = SpConc::new();
    let mut k = 0u32;
    while k < 5 {
        sc.apply(2, ConcOp::CompPush(b'a'))?;
        k += 1;
    }
    sc.apply(2, ConcOp::FocusOff)?;
    let st2 = sc.snapshot(2)?;
    if st2.comp_depth != 0 || st2.focused {
        return Err(E_STATE_BAD);
    }
    let st0 = sc.snapshot(0)?;
    if st0.comp_depth != 0 || !st0.focused {
        return Err(E_CROSS_TALK); // 槽 0 未被波及
    }
    hits += 1;

    // R7 组串深度守恒：单槽 Push×10 + Pop×3 深度 7；Pop 超深处不减
    //    （深度下界 0 钳制）。
    let mut sc = SpConc::new();
    let mut k = 0u32;
    while k < 10 {
        sc.apply(3, ConcOp::CompPush(b'x'))?;
        k += 1;
    }
    let mut k = 0u32;
    while k < 3 {
        sc.apply(3, ConcOp::CompPop)?;
        k += 1;
    }
    let st3 = sc.snapshot(3)?;
    if st3.comp_depth != 7 {
        return Err(E_LEDGER_BAD);
    }
    let mut k = 0u32;
    while k < 5 {
        sc.apply(3, ConcOp::CompPop)?;
        k += 1;
    }
    let st3 = sc.snapshot(3)?;
    if st3.comp_depth != 0 {
        return Err(E_LEDGER_BAD); // 深度钳 0
    }
    hits += 1;

    // R8 短程并发 ×100 与写回（判据 J2）：4 槽 100 条操作零互染，
    //    末尾写回断言（空摘要与零操作拒）。
    let mut sc = SpConc::new();
    let mut i = 0u32;
    while i < 100 {
        let slot = (i % SLOT_COUNT as u32) as usize;
        let k = i / SLOT_COUNT as u32;
        sc.apply(slot, gen_op(slot, k))?;
        i += 1;
    }
    sc.coverage(100)?;
    sc.audit_touch_domain()?;
    let per = 100 / SLOT_COUNT as u32;
    let rem = 100 % SLOT_COUNT as u32;
    let mut s = 0usize;
    while s < SLOT_COUNT {
        let k_count = if (s as u32) < rem { per + 1 } else { per };
        if replay_expected(s, k_count)? != sc.snapshot(s)? {
            return Err(E_CROSS_TALK);
        }
        s += 1;
    }
    if sc.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sc.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
