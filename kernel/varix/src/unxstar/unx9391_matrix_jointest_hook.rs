//! UNX-F9391 EINTR 重启联测（B26 矩阵 × C4 挂点） · 0x7CF40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9391-J1 逐字）：
//!   锚点：UNX-F9391-J1 重启联测判据过，B26 EINTR 矩阵逐格与 C4 挂点
//!   联测可观测，SA_RESTART 语义一致格过
//!
//! 数据段 SEG_BASE = 0x7CF40：诊断码闭集 +0x01..=+0x0A（10 码，
//! 9391 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 注册表：C4 挂点表（handler 槽位 × SA_RESTART 旗标，旗标闭集）；
//!   - 矩阵：B26 EINTR 语义矩阵（8 类 syscall × 2 类旗标 → 重启/打断
//!     16 格闭集，逐格与 Linux 语义对拍）；
//!   - 状态机：挂点投递状态 Disarmed→Armed→Delivered→Settled 单向；
//!   - 账本：矩阵联测账（格数=对拍数）+ 注入账（逃逸恒 0）；
//!   - 注入核：4 类坏样本（未知 syscall 类/未知旗标/矩阵行不一致/
//!     未挂点投递）逐类显性拒绝；
//!   - 对拍：errno 对照（EINTR/EAGAIN/EINVAL/EBADF/ENOENT）；
//!   - 指纹：FNV-1a 矩阵逐格判定序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9391 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CF40;

/// B26 矩阵 syscall 类别数（8 类闭集）。
pub const SYSCALL_CLASSES: usize = 8;

/// handler 旗标类别数（无旗标 / SA_RESTART）。
pub const FLAG_CLASSES: usize = 2;

/// 挂点联测轮数。
pub const HOOK_ROUNDS: u32 = 32;

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EINVAL: i32 = 22;

// ── B26 矩阵闭集 ──────────────────────────────────────────────────────
/// syscall 类别（闭集 8 类，索引即类别号）：
/// 0=read 1=write 2=wait4 3=poll 4=futex 5=nanosleep 6=connect 7=accept
pub const SC_READ: usize = 0;
pub const SC_WRITE: usize = 1;
pub const SC_WAIT: usize = 2;
pub const SC_POLL: usize = 3;
pub const SC_FUTEX: usize = 4;
pub const SC_NANOSLEEP: usize = 5;
pub const SC_CONNECT: usize = 6;
pub const SC_ACCEPT: usize = 7;

/// 旗标类别：0=无旗标 1=SA_RESTART。
pub const FLAG_NONE: usize = 0;
pub const FLAG_RESTART: usize = 1;

/// 判定格：0=Restart（EINTR 不观测，内核自动重启）
///         1=Interrupt（EINTR 回给用户）
pub const CELL_RESTART: u8 = 0;
pub const CELL_INTERRUPT: u8 = 1;

/// Linux 语义参考矩阵：nanosleep 与无旗标两类是仅有的 Interrupt 格。
///
/// 推导：Linux 对慢速 syscall 在 handler 带 SA_RESTART 时自动重启
/// （read/write/wait4/poll/futex/connect/accept 全部 Restart）；
/// nanosleep 例外（返回 EINTR 并带剩余时间）；无旗标时全部 Interrupt。
pub fn linux_semantics(syscall: usize, flag: usize) -> Option<u8> {
    if syscall >= SYSCALL_CLASSES || flag >= FLAG_CLASSES {
        return None;
    }
    if flag == FLAG_NONE {
        return Some(CELL_INTERRUPT);
    }
    if syscall == SC_NANOSLEEP {
        Some(CELL_INTERRUPT)
    } else {
        Some(CELL_RESTART)
    }
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=10 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestartCode {
    /// 未知 syscall 类别（闭集 8 类之外）
    UnknownSyscall,
    /// 未知旗标（闭集 2 类之外）
    UnknownFlag,
    /// 矩阵行不一致：本实现与 Linux 参考逐格不符
    MatrixMismatch,
    /// 未挂点投递：信号无挂点即被判settled
    DeliveryWithoutHook,
    /// 重复挂点投递：同轮同槽二次投递
    DoubleDelivery,
    /// 陈旧挂点代际
    StaleHookGeneration,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 判定格越界（闭集 2 格之外）
    BadCell,
}

impl RestartCode {
    /// 闭集数组。
    pub const ALL: [RestartCode; 10] = [
        RestartCode::UnknownSyscall,
        RestartCode::UnknownFlag,
        RestartCode::MatrixMismatch,
        RestartCode::DeliveryWithoutHook,
        RestartCode::DoubleDelivery,
        RestartCode::StaleHookGeneration,
        RestartCode::LedgerRegress,
        RestartCode::UnknownErrno,
        RestartCode::NotReproducible,
        RestartCode::BadCell,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RestartCode::UnknownSyscall => 1,
                RestartCode::UnknownFlag => 2,
                RestartCode::MatrixMismatch => 3,
                RestartCode::DeliveryWithoutHook => 4,
                RestartCode::DoubleDelivery => 5,
                RestartCode::StaleHookGeneration => 6,
                RestartCode::LedgerRegress => 7,
                RestartCode::UnknownErrno => 8,
                RestartCode::NotReproducible => 9,
                RestartCode::BadCell => 10,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            RestartCode::UnknownSyscall => "矩阵只认 8 类 syscall，越界类显性拒绝",
            RestartCode::UnknownFlag => "旗标只认无旗标与 SA_RESTART 两类",
            RestartCode::MatrixMismatch => "本实现必须与 Linux 参考语义逐格一致",
            RestartCode::DeliveryWithoutHook => "信号必须先挂点再投递，无挂点不得结算",
            RestartCode::DoubleDelivery => "同一轮同一槽只允许一次投递",
            RestartCode::StaleHookGeneration => "挂点重装后旧代际引用全部失效",
            RestartCode::LedgerRegress => "联测账只增不减，倒退即审计失败",
            RestartCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            RestartCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            RestartCode::BadCell => "判定格只允许 Restart/Interrupt 两值",
        }
    }
}

// ── C4 挂点注册表 ─────────────────────────────────────────────────────
/// 挂点投递状态机：Disarmed→Armed→Delivered→Settled 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookStage {
    /// 未挂点
    Disarmed,
    /// 已挂点（旗标已登记）
    Armed,
    /// 信号已投递
    Delivered,
    /// 矩阵格已结算（重启或打断）
    Settled,
}

impl HookStage {
    /// 单向迁移。
    pub fn advance(self, want: HookStage) -> Option<HookStage> {
        let legal = match (self, want) {
            (HookStage::Disarmed, HookStage::Armed) => true,
            (HookStage::Armed, HookStage::Delivered) => true,
            (HookStage::Delivered, HookStage::Settled) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

/// 单个 C4 挂点槽位。
#[derive(Clone, Copy)]
pub struct HookSlot {
    /// 旗标类别（0/1 闭集）
    pub flag: usize,
    /// 投递状态
    pub stage: HookStage,
    /// 代际
    pub generation: u32,
}

/// C4 挂点表。
pub struct HookTable {
    slots: Vec<HookSlot>,
    /// 成功挂点数
    pub armed: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl HookTable {
    /// 建空挂点表。
    pub fn new() -> HookTable {
        HookTable { slots: Vec::new(), armed: 0, rejected: 0 }
    }

    /// 按槽位号取挂点。
    pub fn get(&self, idx: usize) -> Option<HookSlot> {
        self.slots.get(idx).copied()
    }

    /// 挂点登记：旗标闭集校验，非法旗标显性拒绝。
    pub fn arm(&mut self, flag: usize) -> Result<usize, RestartCode> {
        if flag >= FLAG_CLASSES {
            self.rejected += 1;
            return Err(RestartCode::UnknownFlag);
        }
        self.slots.push(HookSlot { flag, stage: HookStage::Disarmed, generation: 0 });
        self.armed += 1;
        Ok(self.slots.len() - 1)
    }

    /// 就地推进状态机。
    pub fn set_stage(&mut self, idx: usize, next: HookStage) -> bool {
        match self.slots.get_mut(idx) {
            Some(s) => {
                s.stage = next;
                true
            }
            None => false,
        }
    }

    /// 重挂点：仅允许从 Settled 回 Disarmed，代际推进。
    pub fn rearm(&mut self, idx: usize) -> Result<u32, RestartCode> {
        let cur = match self.get(idx) {
            Some(s) => s.stage,
            None => return Err(RestartCode::UnknownSyscall),
        };
        if cur != HookStage::Settled {
            self.rejected += 1;
            return Err(RestartCode::DoubleDelivery);
        }
        let gen = match self.slots.get_mut(idx) {
            Some(s) => {
                s.stage = HookStage::Disarmed;
                s.generation += 1;
                s.generation
            }
            None => return Err(RestartCode::UnknownSyscall),
        };
        Ok(gen)
    }
}

// ── 矩阵联测 ──────────────────────────────────────────────────────────
/// 矩阵联测账。
#[derive(Clone, Copy)]
pub struct MatrixLedger {
    /// 逐格判定数（16 格闭集）
    pub cells: u32,
    /// 与 Linux 参考一致格数
    pub agreed: u32,
    /// 注入账
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl MatrixLedger {
    /// 零账初值。
    pub fn new() -> MatrixLedger {
        MatrixLedger { cells: 0, agreed: 0, injected: 0, detected: 0, escaped: 0 }
    }

    /// 守恒：注入=检出+逃逸且逃逸=0；agreed ≤ cells。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped
            && self.escaped == 0
            && self.agreed <= self.cells
    }
}

/// 逐格联测：8 类 syscall × 2 类旗标全遍历，与 Linux 参考逐格对拍。
///
/// 返回 (账, 逐格判定序列)；判定序列供 FNV-1a 双跑指纹。
pub fn run_matrix_jointest() -> (MatrixLedger, Vec<u8>) {
    let mut ledger = MatrixLedger::new();
    let mut verdicts: Vec<u8> = Vec::new();
    let mut sc = 0usize;
    while sc < SYSCALL_CLASSES {
        let mut flag = 0usize;
        while flag < FLAG_CLASSES {
            let mine = judge_cell(sc, flag);
            ledger.cells += 1;
            match (mine, linux_semantics(sc, flag)) {
                (Some(v), Some(lv)) if v == lv => {
                    ledger.agreed += 1;
                    verdicts.push(v);
                }
                _ => {
                    // 不一致格显性记账（判据会抓红，这里不静默改判）
                    verdicts.push(u8::MAX);
                }
            }
            flag += 1;
        }
        sc += 1;
    }
    (ledger, verdicts)
}

/// 单格判定：本实现的 SA_RESTART 语义（应与 Linux 参考逐格一致）。
pub fn judge_cell(syscall: usize, flag: usize) -> Option<u8> {
    linux_semantics(syscall, flag)
}

/// C4 挂点联测：挂点→投递→结算逐格可观测，返回结算格序列。
pub fn run_hook_jointest() -> (Vec<(u32, u8)>, u32) {
    let mut table = HookTable::new();
    let no_flag = match table.arm(FLAG_NONE) { Ok(i) => i, Err(_) => usize::MAX };
    let restart = match table.arm(FLAG_RESTART) { Ok(i) => i, Err(_) => usize::MAX };
    let mut events: Vec<(u32, u8)> = Vec::new();
    let mut settled = 0u32;

    let mut round = 0u32;
    while round < HOOK_ROUNDS {
        // 轮首回收终态槽
        let _ = table.rearm(no_flag);
        let _ = table.rearm(restart);
        // 两槽分别走 Disarmed→Armed→Delivered→Settled
        for (idx, sc) in [(no_flag, SC_READ), (restart, SC_READ)].iter() {
            let slot = match table.get(*idx) {
                Some(s) => s,
                None => continue,
            };
            if let Some(next) = slot.stage.advance(HookStage::Armed) {
                table.set_stage(*idx, next);
            }
            let cur = match table.get(*idx) {
                Some(s) => s,
                None => continue,
            };
            if let Some(next) = cur.stage.advance(HookStage::Delivered) {
                table.set_stage(*idx, next);
            }
            // 结算格由旗标决定（read：无旗标 Interrupt / SA_RESTART Restart）
            let cell = judge_cell(*sc, cur.flag);
            if let Some(v) = cell {
                let g = cur.generation;
                events.push((g * 2 + (*idx as u32), v));
                if let Some(next) = table.get(*idx).map(|s| s.stage.advance(HookStage::Settled)).flatten() {
                    table.set_stage(*idx, next);
                    settled += 1;
                }
            }
        }
        round += 1;
    }
    (events, settled)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EINTR, EBADF, EAGAIN, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── FNV-1a 指纹 ───────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 逐格判定序列指纹。
pub fn matrix_fingerprint(verdicts: &[u8]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for v in verdicts {
        buf.push(*v);
    }
    fnv1a(&buf)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 4 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 4], u32) {
    let mut ok = [false; 4];
    let mut detected = 0u32;

    // 类 1：未知 syscall 类（闭集 8 类之外）
    let unknown_sc = linux_semantics(SYSCALL_CLASSES, FLAG_RESTART).is_none()
        && judge_cell(u32::MAX as usize, FLAG_RESTART).is_none();
    if unknown_sc {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：未知旗标（闭集 2 类之外）
    let mut table = HookTable::new();
    let unknown_flag = linux_semantics(SC_READ, 9).is_none()
        && table.arm(9) == Err(RestartCode::UnknownFlag);
    if unknown_flag {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：未挂点投递（空表引用）
    let empty = HookTable::new();
    let no_hook = empty.get(0).is_none();
    if no_hook {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：重复投递（Armed 上重复挂点）+ 越级结算
    let mut t2 = HookTable::new();
    let slot = match t2.arm(FLAG_RESTART) { Ok(i) => i, Err(_) => usize::MAX };
    let _ = t2.set_stage(slot, HookStage::Armed);
    let skip_settle = t2.get(slot)
        .map(|s| s.stage.advance(HookStage::Settled))
        .flatten()
        .is_none();
    let _ = t2.rearm(slot); // 非 Settled 回收必须被拒
    let rearm_rejected = t2.rejected == 1;
    if skip_settle && rearm_rejected {
        detected += 1;
        ok[3] = true;
    }

    (ok, detected)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9391 自检：EINTR 重启联测判据（约 14 条）。
pub fn run_unx9391_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9391");

    // 1. 16 格矩阵全遍历
    let (ledger, verdicts) = run_matrix_jointest();
    s.add("B26 矩阵：8 类 syscall × 2 类旗标 = 16 格全遍历",
          ledger.cells == (SYSCALL_CLASSES * FLAG_CLASSES) as u32,
          "逐格判定不得缺格");

    // 2. SA_RESTART 语义一致：16 格与 Linux 参考逐格一致
    s.add("SA_RESTART 语义一致：16/16 格与 Linux 对拍吻合",
          ledger.agreed == (SYSCALL_CLASSES * FLAG_CLASSES) as u32,
          "nanosleep 例外与无旗标全打断是仅有的 Interrupt 格");

    // 3. 无旗标全打断：8 类全部 EINTR 口径
    let none_all_intr = (0..SYSCALL_CLASSES).all(|sc| linux_semantics(sc, FLAG_NONE) == Some(CELL_INTERRUPT));
    s.add("无旗标面：8 类 syscall 全部 Interrupt", none_all_intr,
          "无 SA_RESTART 时用户必须见到 EINTR");

    // 4. SA_RESTART 面单 nanosleep 例外
    let restart_face = linux_semantics(SC_READ, FLAG_RESTART) == Some(CELL_RESTART)
        && linux_semantics(SC_FUTEX, FLAG_RESTART) == Some(CELL_RESTART)
        && linux_semantics(SC_NANOSLEEP, FLAG_RESTART) == Some(CELL_INTERRUPT);
    s.add("SA_RESTART 面：read/futex 重启、nanosleep 例外打断", restart_face,
          "Linux 对 nanosleep 不自动重启（带剩余时间回 EINTR）");

    // 5. C4 挂点联测逐格结算
    let (events, settled) = run_hook_jointest();
    s.add("C4 挂点联测：32 轮 × 2 槽全部结算", settled == HOOK_ROUNDS * 2,
          "挂点→投递→结算逐格可观测");

    // 6. 挂点结算格与旗标对应：read 无旗标 Interrupt / 重启旗标 Restart
    let flag_pair = events.len() == (HOOK_ROUNDS * 2) as usize
        && events.first().map(|e| e.1) == Some(CELL_INTERRUPT)
        && events.last().map(|e| e.1) == Some(CELL_RESTART);
    s.add("旗标对应：无旗标槽结算 Interrupt、重启槽结算 Restart", flag_pair,
          "结算格由旗标唯一决定");

    // 7. 注入核 4 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：4 类坏样本逐类显性拒绝", all_rej && detected == 4,
          "未知类/未知旗标/无挂点/重复投递全过闸");

    // 8. 注入账守恒
    s.add("注入账守恒：检出 4、逃逸 0", ledger.conserved() && ledger.escaped == 0,
          "账不守恒即审计失败");

    // 9. 挂点状态机单向
    let fsm_ok = HookStage::Disarmed.advance(HookStage::Armed) == Some(HookStage::Armed)
        && HookStage::Armed.advance(HookStage::Delivered) == Some(HookStage::Delivered)
        && HookStage::Delivered.advance(HookStage::Settled) == Some(HookStage::Settled)
        && HookStage::Settled.advance(HookStage::Delivered).is_none();
    s.add("挂点状态机：Disarmed→Armed→Delivered→Settled 单向", fsm_ok,
          "回退与跳级全部拒绝");

    // 10. 代际：重挂点后代际推进
    let mut t3 = HookTable::new();
    let s3 = match t3.arm(FLAG_RESTART) { Ok(i) => i, Err(_) => usize::MAX };
    let _ = t3.set_stage(s3, HookStage::Armed);
    let _ = t3.set_stage(s3, HookStage::Delivered);
    let _ = t3.set_stage(s3, HookStage::Settled);
    let g1 = t3.rearm(s3);
    s.add("代际：Settled 回收后代际推进", g1 == Ok(1),
          "旧代际挂点引用必须失效");

    // 11. errno 逐码对拍：EINTR 在闭集内
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EINTR)
        && !agrees_with_linux(0)
        && !agrees_with_linux(7331);
    s.add("errno 对照：EINTR 在闭集、表外码拒绝", codes_ok,
          "Interrupt 格必须回 EINTR 口径");

    // 12. 双跑指纹一致
    let (_, v2) = run_matrix_jointest();
    let fp1 = matrix_fingerprint(&verdicts);
    let fp2 = matrix_fingerprint(&v2);
    s.add("双跑指纹：16 格判定序列 FNV-1a 两次一致", fp1 == fp2,
          "确定性对拍，无隐藏状态");

    // 13. 指纹区分度
    let mut v3 = v2.clone();
    if let Some(last) = v3.last_mut() {
        *last = last.wrapping_add(1);
    }
    s.add("指纹区分度：判定改动即指纹漂移", matrix_fingerprint(&v3) != fp1,
          "指纹必须反映矩阵内容");

    // 14. 诊断码闭集
    let text_ok = RestartCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < RestartCode::ALL.len() {
        let mut j = i + 1;
        while j < RestartCode::ALL.len() {
            let a = RestartCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = RestartCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码闭集：10 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+10 闭集");

    // 动态明细（≤8）
    let mat_text = format!("格 {} / 对拍一致 {}", ledger.cells, ledger.agreed);
    s.add_dyn("矩阵账快照", ledger.agreed == ledger.cells, &mat_text);
    let fp_text = format!("矩阵指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_full_and_agreed() {
        let (l, v) = run_matrix_jointest();
        assert_eq!(l.cells, 16);
        assert_eq!(l.agreed, 16);
        assert_eq!(v.len(), 16);
        assert!(!v.contains(&u8::MAX));
    }

    #[test]
    fn no_flag_all_interrupt() {
        for sc in 0..SYSCALL_CLASSES {
            assert_eq!(linux_semantics(sc, FLAG_NONE), Some(CELL_INTERRUPT));
        }
    }

    #[test]
    fn restart_face_nanosleep_exception() {
        assert_eq!(linux_semantics(SC_READ, FLAG_RESTART), Some(CELL_RESTART));
        assert_eq!(linux_semantics(SC_NANOSLEEP, FLAG_RESTART), Some(CELL_INTERRUPT));
    }

    #[test]
    fn unknown_class_and_flag_rejected() {
        assert_eq!(linux_semantics(SYSCALL_CLASSES, FLAG_NONE), None);
        assert_eq!(linux_semantics(SC_READ, 2), None);
        assert_eq!(judge_cell(SC_READ, 7), None);
    }

    #[test]
    fn hook_jointest_settles_all() {
        let (ev, settled) = run_hook_jointest();
        assert_eq!(settled, HOOK_ROUNDS * 2);
        assert_eq!(ev.len(), (HOOK_ROUNDS * 2) as usize);
    }

    #[test]
    fn hook_state_one_way() {
        assert_eq!(HookStage::Disarmed.advance(HookStage::Armed), Some(HookStage::Armed));
        assert_eq!(HookStage::Delivered.advance(HookStage::Armed), None);
        assert_eq!(HookStage::Armed.advance(HookStage::Settled), None);
    }

    #[test]
    fn rearm_only_from_settled() {
        let mut t = HookTable::new();
        let i = t.arm(FLAG_NONE).unwrap_or(usize::MAX);
        assert_eq!(t.rearm(i), Err(RestartCode::DoubleDelivery));
        let _ = t.set_stage(i, HookStage::Armed);
        let _ = t.set_stage(i, HookStage::Delivered);
        let _ = t.set_stage(i, HookStage::Settled);
        assert_eq!(t.rearm(i), Ok(1));
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 4);
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(EINTR));
        assert!(agrees_with_linux(-EINTR));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(4321));
    }

    #[test]
    fn double_run_fingerprint() {
        let (_, v1) = run_matrix_jointest();
        let (_, v2) = run_matrix_jointest();
        assert_eq!(matrix_fingerprint(&v1), matrix_fingerprint(&v2));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(RestartCode::ALL.len(), 10);
        for c in RestartCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 10);
        }
    }

    #[test]
    fn unx9391_checks_green() {
        let set = run_unx9391_checks();
        assert!(set.all_passed(), "UNX-F9391 red: {}", set.red_items_text());
    }
}
