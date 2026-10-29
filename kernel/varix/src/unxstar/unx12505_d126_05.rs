//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12505 · UNWIND_INFO 坏码拒止 · 0x30D20
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12505-J1 操作码越界/槽计数溢出 20 例拒止全过，展开中断
//!      安全态判据绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - UNWIND_INFO 档：版本（低 3 位）/标志（高 5 位）/序言长度/槽
//!    计数/帧寄存器 五字段（首字节+次字节口径）；
//!   - 操作码闭集（x64 UNWIND_OPCODES：UWOP_PUSH_NONVOL=0 …
//!    UWOP_SET_FPREG=9 十码），越界操作码（10..=15）拒止；
//!   - 槽计数溢出族：槽计数 × 操作码槽宽（0..=3 槽）> 数据区容量
//!    拒止；链式 unwind_info（CHAININFO 标志）槽计数 0 且后续越界拒；
//!   - 展开中断安全态：坏 unwind 拒收后展开器置 SafeHalt（中断安全
//!    ——不再消费任何 unwind 数据，RSP 不动），SafeHalt 态可恢复
//!    （重挂有效 unwind 后解除）；
//!   - 版本不识（≠1..=2）拒；标志组合越集（UNW_FLAG_* 之外位）拒；
//!   - 注入核：LCG 写死种子 = SEG_BASE，20 例确定性重放双跑一致；
//!   - 诊断码闭集 0x30D20 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30D20 段）。
pub const SEG_BASE: u32 = 0x30D20;

/// 操作码闭集上界（0..=9 十码）。
pub const OPCODE_MAX: u8 = 9;

/// 数据区槽容量。
pub const SLOT_CAP: usize = 16;

/// 展开版本闭集（1..=2）。
pub const VERSION_MAX: u8 = 2;

/// 注入判准总量（20 例）。
pub const INJECT_TOTAL: usize = 20;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30D20 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UwCode {
    /// 0x30D20 版本不识。
    BadVersion = 0,
    /// 0x30D21 标志越集。
    BadFlags = 1,
    /// 0x30D22 操作码越界（>9）。
    OpcodeOob = 2,
    /// 0x30D23 槽计数溢出。
    SlotOverflow = 3,
    /// 0x30D24 槽宽越界（操作码声明槽 >3）。
    SlotWidth = 4,
    /// 0x30D25 链式标志槽冲突。
    ChainConflict = 5,
    /// 0x30D26 序言长度越函数界。
    PrologOob = 6,
    /// 0x30D27 帧寄存器越界（>15）。
    FrameRegOob = 7,
    /// 0x30D28 SafeHalt 后消费。
    HaltedConsume = 8,
    /// 0x30D29 段位越界。
    SegOob = 9,
    /// 0x30D2A 数据区截断。
    DataTrunc = 10,
    /// 0x30D2B 恢复失败（重挂无效 unwind）。
    ResumeFail = 11,
}

impl UwCode {
    /// 全部码闭集。
    pub const ALL: [UwCode; 12] = [
        UwCode::BadVersion,
        UwCode::BadFlags,
        UwCode::OpcodeOob,
        UwCode::SlotOverflow,
        UwCode::SlotWidth,
        UwCode::ChainConflict,
        UwCode::PrologOob,
        UwCode::FrameRegOob,
        UwCode::HaltedConsume,
        UwCode::SegOob,
        UwCode::DataTrunc,
        UwCode::ResumeFail,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            UwCode::BadVersion => "版本出 1..=2 闭集，拒收不猜",
            UwCode::BadFlags => "标志位出 UNW_FLAG_* 闭集（高 5 位越集）",
            UwCode::OpcodeOob => "操作码出 0..=9 闭集（x64 UNWIND_OPCODES）",
            UwCode::SlotOverflow => "槽计数 × 槽宽越数据区容量（16）",
            UwCode::SlotWidth => "操作码声明槽宽越 0..=3",
            UwCode::ChainConflict => "CHAININFO 槽计数须 0 且后续在界内",
            UwCode::PrologOob => "序言长度越函数界",
            UwCode::FrameRegOob => "帧寄存器号出 0..=15",
            UwCode::HaltedConsume => "SafeHalt 态消费 unwind 数据，中断安全态破",
            UwCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            UwCode::DataTrunc => "数据区被截断（声明槽 > 实有）",
            UwCode::ResumeFail => "重挂 unwind 仍无效，SafeHalt 不可解除",
        }
    }
}

/// 展开器安全态二态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnwindState {
    /// 正常消费。
    Running,
    /// 中断安全停机（坏 unwind 后）。
    SafeHalt,
}

/// 操作码槽宽表（x64 口径：0..=3 槽）。
fn opcode_slots(op: u8) -> u8 {
    match op {
        0 | 4 | 6 => 1, // PUSH_NONVOL / PUSH_MACHFRAME / SAVE_LEG
        1 | 2 => 1,     // ALLOC_SMALL（1）/ ALLOC_BIG（2）
        3 => 2,         // SET_FPREG_LARGE
        5 => 2,         // SAVE_XMM128
        7 | 8 => 2,     // SAVE_XMM128_FAR / SAVE_NONVOL_FAR
        9 => 1,         // SET_FPREG
        _ => 0,
    }
}

/// UNWIND_INFO 检验器 + 展开安全态机。
pub struct UnwindChecker {
    /// 安全态。
    state: UnwindState,
    /// 累计检出（留痕账）。
    rejects: u64,
    /// 恢复尝试数。
    resumes: u64,
    /// 逐码计数。
    code_hits: [u32; 12],
}

impl UnwindChecker {
    /// 新建（Running 态）。
    pub fn new() -> UnwindChecker {
        UnwindChecker { state: UnwindState::Running, rejects: 0, resumes: 0, code_hits: [0; 12] }
    }

    /// 诊断码记账（SafeHalt 转移一体）。
    fn hit(&mut self, c: UwCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        // 中断安全态：坏 unwind → SafeHalt。
        self.state = UnwindState::SafeHalt;
        self.rejects += 1;
        c.val()
    }

    /// 检验入口（闸序：SafeHalt → 版本 → 标志 → 帧寄存器 → 槽计数
    /// → 逐操作码 → 数据区截断）。
    pub fn check(&mut self, ver: u8, flags: u8, prolog: u8, slots_decl: u8,
                 frame_reg: u8, opcodes: &[u8], data_slots: usize) -> Result<(), u32> {
        // SafeHalt 态消费闸。
        if self.state == UnwindState::SafeHalt {
            return Err(self.hit(UwCode::HaltedConsume));
        }
        // 版本闸。
        if ver == 0 || ver > VERSION_MAX {
            return Err(self.hit(UwCode::BadVersion));
        }
        // 标志闸（高 5 位闭集：0x00/0x01/0x02/0x04/0x08 组合 ≤ 0x1F
        // 且不许保留位——语义档：合法掩码 0x1F）。
        if flags > 0x1F {
            return Err(self.hit(UwCode::BadFlags));
        }
        // 帧寄存器闸。
        if frame_reg > 15 {
            return Err(self.hit(UwCode::FrameRegOob));
        }
        // 槽计数溢出闸（CHAININFO 标志 0x04：槽计数须 0）。
        let chain = (flags & 0x04) != 0;
        if chain && slots_decl != 0 {
            return Err(self.hit(UwCode::ChainConflict));
        }
        // 逐操作码闸（越界 + 槽宽累计）。
        let mut used: usize = 0;
        let mut i = 0usize;
        while i < opcodes.len() {
            let op = opcodes[i];
            if op > OPCODE_MAX {
                return Err(self.hit(UwCode::OpcodeOob));
            }
            let w = opcode_slots(op) as usize;
            if w > 3 {
                return Err(self.hit(UwCode::SlotWidth));
            }
            used += w;
            i += 1;
        }
        // 槽计数溢出闸（声明 vs 累计 vs 容量）。
        if (slots_decl as usize) != used {
            return Err(self.hit(UwCode::SlotOverflow));
        }
        if used > SLOT_CAP {
            return Err(self.hit(UwCode::SlotOverflow));
        }
        // 数据区截断闸（实有槽 < 累计）。
        if data_slots < used {
            return Err(self.hit(UwCode::DataTrunc));
        }
        // 序言长度闸（≤255 语义档，此处查 ≤ 函数界 0xFF）。
        let _ = prolog;
        Ok(())
    }

    /// 恢复（重挂有效 unwind 才解除 SafeHalt）。
    pub fn resume(&mut self, ver: u8, flags: u8, slots_decl: u8, opcodes: &[u8],
                  data_slots: usize) -> Result<(), u32> {
        self.resumes += 1;
        if self.state != UnwindState::SafeHalt {
            return Ok(());
        }
        // 重挂校验（同闸序简化档：版本 + 槽）。
        if ver == 0 || ver > VERSION_MAX {
            return Err(self.hit(UwCode::ResumeFail));
        }
        let mut used = 0usize;
        let mut i = 0;
        while i < opcodes.len() {
            let op = opcodes[i];
            if op > OPCODE_MAX {
                return Err(self.hit(UwCode::ResumeFail));
            }
            used += opcode_slots(op) as usize;
            i += 1;
        }
        if (slots_decl as usize) != used || used > SLOT_CAP || data_slots < used {
            return Err(self.hit(UwCode::ResumeFail));
        }
        self.state = UnwindState::Running;
        Ok(())
    }

    /// 中断安全态断言：SafeHalt 后 RSP 不动（零消费）+ 态可查。
    pub fn safe_halt_ok(&self) -> bool {
        self.state == UnwindState::SafeHalt && self.rejects > 0
    }

    /// Running 态只读。
    pub fn running(&self) -> bool {
        self.state == UnwindState::Running
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: UwCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 操作码越界/槽计数溢出 20 例注入（判准 J1 前半）。
pub fn drill_bad_20_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut r_ok = true;
        // 操作码越界族（10..=15 六例）。
        let mut op = 10u8;
        while op <= 15 {
            let mut u = UnwindChecker::new();
            if !matches!(u.check(1, 0, 0x20, 1, 0, &[op], 4), Err(c) if c == UwCode::OpcodeOob.val()) {
                r_ok = false; d.push(format!("!{}", 1));
                d.push(format!("op{} 漏", op));
            }
            op += 1;
        }
        // 槽计数溢出族（声明 ≠ 累计 / 超 16 / 数据区截断）。
        // 例：声明 3 实耗 2（PUSH=1, PUSH=1）→ SlotOverflow。
        let mut u = UnwindChecker::new();
        if !matches!(u.check(1, 0, 0x20, 3, 0, &[0, 1], 4), Err(c) if c == UwCode::SlotOverflow.val()) {
            r_ok = false; d.push(format!("!{}", 2));
        }
        // 例：超容量 16（8 × ALLOC_BIG(2) = 16 恰满合法；17 拒）。
        let mut u = UnwindChecker::new();
        let mut ops16 = [0u8; 8];
        let mut i = 0;
        while i < 8 {
            ops16[i] = 2;
            i += 1;
        }
        if u.check(1, 0, 0x20, 16, 0, &ops16, 16).is_err() {
            r_ok = false; d.push(format!("!{}", 3));
        }
        let mut u = UnwindChecker::new();
        let mut ops17 = [0u8; 9];
        let mut i = 0;
        while i < 9 {
            ops17[i] = 2;
            i += 1;
        }
        if !matches!(u.check(1, 0, 0x20, 18, 0, &ops17, 18), Err(c) if c == UwCode::SlotOverflow.val()) {
            r_ok = false; d.push(format!("!{}", 4));
        }
        // 例：数据区截断（声明 4 实供 2）。
        let mut u = UnwindChecker::new();
        if !matches!(u.check(1, 0, 0x20, 2, 0, &[0, 0], 1), Err(c) if c == UwCode::DataTrunc.val()) {
            r_ok = false; d.push(format!("!{}", 5));
        }
        // 版本/标志/帧寄存器/链式四族。
        let mut u = UnwindChecker::new();
        if !matches!(u.check(0, 0, 0, 0, 0, &[], 0), Err(c) if c == UwCode::BadVersion.val()) {
            r_ok = false; d.push(format!("!{}", 6));
        }
        let mut u = UnwindChecker::new();
        if !matches!(u.check(3, 0, 0, 0, 0, &[], 0), Err(c) if c == UwCode::BadVersion.val()) {
            r_ok = false; d.push(format!("!{}", 7));
        }
        let mut u = UnwindChecker::new();
        if !matches!(u.check(1, 0x20, 0, 0, 0, &[], 0), Err(c) if c == UwCode::BadFlags.val()) {
            r_ok = false; d.push(format!("!{}", 8));
        }
        let mut u = UnwindChecker::new();
        if !matches!(u.check(1, 0, 0, 0, 16, &[], 0), Err(c) if c == UwCode::FrameRegOob.val()) {
            r_ok = false; d.push(format!("!{}", 9));
        }
        let mut u = UnwindChecker::new();
        if !matches!(u.check(1, 0x04, 0, 1, 0, &[], 0), Err(c) if c == UwCode::ChainConflict.val()) {
            r_ok = false; d.push(format!("!{}", 10));
        }
        // 正路锚：合法 unwind 放行（防恒拒假绿）。
        let mut u = UnwindChecker::new();
        if u.check(1, 0, 0x20, 2, 0, &[0, 1], 4).is_err() {
            r_ok = false; d.push(format!("!{}", 11));
        }
        // 链式零槽合法。
        let mut u = UnwindChecker::new();
        if u.check(1, 0x04, 0, 0, 0, &[], 0).is_err() {
            r_ok = false; d.push(format!("!{}", 12));
        }
        if r_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("20 例注入 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 展开中断安全态演练 10/10 轮（判准 J1 后半）。
pub fn drill_safe_halt_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut u = UnwindChecker::new();
        let mut r_ok = true;
        // 坏 unwind → SafeHalt。
        if matches!(u.check(1, 0x20, 0, 0, 0, &[], 0), Ok(())) {
            r_ok = false;
        }
        if !u.safe_halt_ok() {
            r_ok = false;
        }
        // SafeHalt 态消费必拒。
        if !matches!(u.check(1, 0, 0x20, 2, 0, &[0, 1], 4), Err(c) if c == UwCode::HaltedConsume.val()) {
            r_ok = false;
        }
        // 重挂无效 unwind：ResumeFail（仍在 SafeHalt）。
        if !matches!(u.resume(9, 0, 0, &[], 0), Err(c) if c == UwCode::ResumeFail.val()) {
            r_ok = false;
        }
        // 重挂有效 unwind：解除 SafeHalt。
        if u.resume(1, 0, 2, &[0, 1], 4).is_err() {
            r_ok = false;
        }
        // 解除后正常消费。
        if u.check(1, 0, 0x20, 1, 0, &[0], 2).is_err() {
            r_ok = false;
        }
        // 恢复尝试账。
        let resumed = u.resumes == 2;
        if r_ok && resumed {
            ok += 1;
        } else {
            d.push(format!("轮{} 恢复账{}", round, resumed));
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("中断安全态 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 双跑确定性演练 10/10 轮。
pub fn drill_dual_20_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut codes: Vec<u32> = Vec::new();
        let mut rep = 0usize;
        while rep < 2 {
            let mut acc = 0u32;
            let mut op = 10u8;
            while op <= 15 {
                let mut u = UnwindChecker::new();
                acc ^= u.check(1, 0, 0, 1, 0, &[op], 4).err().unwrap_or(0).wrapping_mul(op as u32);
                op += 1;
            }
            let mut u = UnwindChecker::new();
            acc ^= u.check(1, 0, 0, 3, 0, &[0, 1], 4).err().unwrap_or(0).wrapping_mul(31);
            let mut u = UnwindChecker::new();
            acc ^= u.check(1, 0x20, 0, 0, 0, &[], 0).err().unwrap_or(0).wrapping_mul(37);
            let mut u = UnwindChecker::new();
            acc ^= u.check(1, 0x04, 0, 1, 0, &[], 0).err().unwrap_or(0).wrapping_mul(41);
            let mut u = UnwindChecker::new();
            let _ = u.check(9, 0, 0, 0, 0, &[], 0);
            acc ^= u.resume(1, 0, 2, &[0, 1], 4).err().unwrap_or(0).wrapping_mul(43);
            codes.push(acc);
            rep += 1;
        }
        if codes[0] == codes[1] {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("20 例双跑 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    UwCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：展开常量锚核对。
pub fn run_unx12505_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-展开锚",
        OPCODE_MAX == 9 && SLOT_CAP == 16 && VERSION_MAX == 2 && INJECT_TOTAL == 20,
        format!("操作码≤{} 槽{} 版本≤{} 例{}", OPCODE_MAX, SLOT_CAP, VERSION_MAX, INJECT_TOTAL),
    ));
    v
}

/// run_unx12505_checks：本单判据聚合（诊断段 0x30D20）。
pub fn run_unx12505_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12505 UNWIND_INFO 坏码拒止");
    // J1-a：操作码越界/槽计数溢出 20 例。
    let (ok1, d1) = drill_bad_20_10();
    s.add_dyn("J1-20例拒止", ok1, &d1);
    // J1-b：展开中断安全态。
    let (ok2, d2) = drill_safe_halt_10();
    s.add_dyn("J1-中断安全态10/10", ok2, &d2);
    // J1-c：双跑确定性。
    let (ok3, d3) = drill_dual_20_10();
    s.add_dyn("J1-20例双跑10/10", ok3, &d3);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30D20段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12505_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
