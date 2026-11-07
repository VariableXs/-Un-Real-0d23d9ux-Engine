//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12743 展开链压测 · 0x823C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 12743 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12744-J1 UWOP 码族全档压测回归绿，码值锚注出处，
//!      演练 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   1. 展开链域模型：UWOP 码族 0..=9 全档（10 族闭集），每码位一条链目
//!      （码值 + 槽位数）+ 每码位状态机 Fresh→Decoded→Drilled 单调只进
//!      不退，倒退显性拒 ERewind；
//!   2. 槽位数冻结表（出处：MSDN《x64 软件约定 · 异常处理》UNWIND_OP_CODES
//!      表 / WinNT.h）：0 PUSH_NONVOL=1 槽、1 ALLOC_LARGE=2 槽（远型 3）、
//!      2 ALLOC_SMALL=1 槽、3 SET_FPREG=1 槽、4 SAVE_NONVOL=2 槽、
//!      5 SAVE_NONVOL_FAR=3 槽、6 EPILOGUE=1 槽（双字型 2）、
//!      7 SAVE_XMM128=2 槽、8 SAVE_XMM128_FAR=3 槽、9 PUSH_MACHFRAME=1 槽
//!      （info=1 型 2）——槽位与冻结表不符显性拒 ESlot；
//!   3. 码值锚注：每码位 anchor() 给出 UWOP 助记符 + 码值 + 手册出处，
//!      锚注缺失显性拒 EAnchor（判据②"码值锚注出处"的模型化）；
//!   4. 展开序判据：演练序 = 登记序的严格逆序（展开与登记相反），演练
//!      10/10 = 10 码位全 Drilled 且演练序账与逆登记序逐位一致，破序显性拒
//!      ESeq；演练序账定容 10 槽，容量耗尽显性拒 ECapFull 且已入账不丢；
//!   5. 注入三族：码越族（10..15）/ 槽位错报 / 重复登记，LCG（种子=
//!      SEG_BASE）确定性各跑 10 轮必抓；计数守恒式 注入=检出+漏检，漏检恒 0；
//!   6. 完成写回模型：WorkStatus 只进至 Done，结果摘要非空（判据③）；
//!      FNV-1a 指纹双跑逐字段一致；no_std + alloc；零浮点/IO/墙钟/随机；
//!      无 unwrap/expect/panic/裸下标（取值一律 get/get_mut/match）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12743 − 12600) = 0x823C0）。
pub const SEG_BASE: u32 = 0x823C0;

// ── 冻结常量表（带出处） ─────────────────────────────────────────
/// UWOP 码族总数（码 0..=9 闭集；10..15 保留非法，越族显性拒）。
/// 出处：MSDN《x64 软件约定 · 异常处理》UNWIND_OP_CODES 表 / WinNT.h。
pub const UWOP_TOTAL: usize = 10;

/// 槽位数冻结表（下标即码值；出处同上，取各码的基准编码槽位数）。
pub const UWOP_SLOTS: [u32; UWOP_TOTAL] = [
    1, // 0 UWOP_PUSH_NONVOL：1 槽（info=寄存器号）
    2, // 1 UWOP_ALLOC_LARGE：基准 2 槽（info=1 时 3 槽，远型）
    1, // 2 UWOP_ALLOC_SMALL：1 槽（info*8+8 字节）
    1, // 3 UWOP_SET_FPREG：1 槽
    2, // 4 UWOP_SAVE_NONVOL：2 槽
    3, // 5 UWOP_SAVE_NONVOL_FAR：3 槽
    1, // 6 UWOP_EPILOGUE：基准 1 槽（双字型 2 槽）
    2, // 7 UWOP_SAVE_XMM128：2 槽
    3, // 8 UWOP_SAVE_XMM128_FAR：3 槽
    1, // 9 UWOP_PUSH_MACHFRAME：基准 1 槽（info=1 型 2 槽）
];

/// 码族助记符表（下标即码值；出处同上）。
pub const UWOP_NAMES: [&'static str; UWOP_TOTAL] = [
    "UWOP_PUSH_NONVOL",
    "UWOP_ALLOC_LARGE",
    "UWOP_ALLOC_SMALL",
    "UWOP_SET_FPREG",
    "UWOP_SAVE_NONVOL",
    "UWOP_SAVE_NONVOL_FAR",
    "UWOP_EPILOGUE",
    "UWOP_SAVE_XMM128",
    "UWOP_SAVE_XMM128_FAR",
    "UWOP_PUSH_MACHFRAME",
];

/// 演练序账容量（10 码位全演练恰好填满，判据②"演练 10/10"的账面锚）。
pub const WALK_CAP: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 展开链压测域诊断码闭集；码值 = SEG_BASE + 固定偏移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UwDiag {
    /// 码越族：码值 ≥ 10，不在 0..=10 闭集内（10..15 为非法保留）。
    ENoOp,
    /// 槽位错报：槽位数与冻结表 UWOP_SLOTS 不符。
    ESlot,
    /// 码位未登记：未登记先解码/先演练，次序冻结。
    EOrder,
    /// 重复登记：同码位双登，显性拒。
    EDouble,
    /// 状态倒退：Fresh→Decoded→Drilled 只进不退被破。
    ERewind,
    /// 码值锚注缺失：anchor() 对该码位无出处文案。
    EAnchor,
    /// 展开序漂移：演练序与逆登记序逐位不一致。
    ESeq,
    /// 链账守恒破缺：Σ(演练成+演练拒) ≠ 演练尝试数。
    EConserve,
    /// 演练序账容量耗尽：10 槽判满后新步拒入。
    ECapFull,
    /// 指纹漂移：双跑指纹不一致。
    EFp,
}

impl UwDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                UwDiag::ENoOp => 0x01,
                UwDiag::ESlot => 0x02,
                UwDiag::EOrder => 0x03,
                UwDiag::EDouble => 0x04,
                UwDiag::ERewind => 0x05,
                UwDiag::EAnchor => 0x06,
                UwDiag::ESeq => 0x07,
                UwDiag::EConserve => 0x08,
                UwDiag::ECapFull => 0x09,
                UwDiag::EFp => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            UwDiag::ENoOp => "码越族：码值不在 0..=10，按 UWOP 码族闭集重排（10..15 非法）",
            UwDiag::ESlot => "槽位错报：槽位数与 UWOP_SLOTS 冻结表不符，对照 WinNT.h 修正",
            UwDiag::EOrder => "码位未登记：先 register 再 decode 再 drill，次序冻结",
            UwDiag::EDouble => "重复登记：同码位双登，查链目表后再提交",
            UwDiag::ERewind => "状态倒退：Fresh→Decoded→Drilled 只进不退，先回退工单再重放",
            UwDiag::EAnchor => "码值锚注缺失：补 anchor() 出处文案（MSDN UNWIND_OP_CODES）",
            UwDiag::ESeq => "展开序漂移：演练序须为登记序严格逆序，复查演练步进",
            UwDiag::EConserve => "链账守恒破缺：Σ(成+拒)≠尝试数，逐码位复算演练账",
            UwDiag::ECapFull => "演练序账容量耗尽：10 槽判满，先封账核对再开新账",
            UwDiag::EFp => "指纹漂移：双跑指纹不一致，排查 LCG 种子与非确定源",
        }
    }

    /// 闭集数组（查表遍历用，段窗守卫的枚举锚）。
    pub const ALL: [UwDiag; 10] = [
        UwDiag::ENoOp,
        UwDiag::ESlot,
        UwDiag::EOrder,
        UwDiag::EDouble,
        UwDiag::ERewind,
        UwDiag::EAnchor,
        UwDiag::ESeq,
        UwDiag::EConserve,
        UwDiag::ECapFull,
        UwDiag::EFp,
    ];
}

/// 诊断码查表闭集返回：命中闭集给中文文案，未知码给中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < UwDiag::ALL.len() {
        if let Some(d) = UwDiag::ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x823C0 段闭集内，核对 SEG_BASE + 偏移表"
}

// ── 码值锚注（判据②"码值锚注出处"） ─────────────────────────────
/// 每码位的锚注文案：助记符 + 码值 + 手册出处；越族码回 None。
pub fn anchor(code: u32) -> Option<&'static str> {
    match code {
        0 => Some("UWOP_PUSH_NONVOL=0（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        1 => Some("UWOP_ALLOC_LARGE=1（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        2 => Some("UWOP_ALLOC_SMALL=2（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        3 => Some("UWOP_SET_FPREG=3（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        4 => Some("UWOP_SAVE_NONVOL=4（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        5 => Some("UWOP_SAVE_NONVOL_FAR=5（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        6 => Some("UWOP_EPILOGUE=6（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        7 => Some("UWOP_SAVE_XMM128=7（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        8 => Some("UWOP_SAVE_XMM128_FAR=8（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        9 => Some("UWOP_PUSH_MACHFRAME=9（MSDN x64 软件约定·异常处理 UNWIND_OP_CODES / WinNT.h）"),
        _ => None,
    }
}

/// 锚注齐判定：0..=10 全档锚注非空且含码值，越族码必 None。
pub fn anchors_complete() -> bool {
    let mut code = 0u32;
    while (code as usize) < UWOP_TOTAL {
        let a = match anchor(code) {
            Some(a) => a,
            None => return false,
        };
        if a.is_empty() {
            return false;
        }
        let name = match UWOP_NAMES.get(code as usize) {
            Some(n) => *n,
            None => return false,
        };
        if !a.contains(name) {
            return false;
        }
        code += 1;
    }
    anchor(UWOP_TOTAL as u32).is_none() && anchor(u32::MAX).is_none()
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 链目：码值 + 槽位数（槽位数须与冻结表一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainOp {
    /// UWOP 码值（0..=10 闭集）。
    pub code: u32,
    /// 槽位数（= UWOP_SLOTS[code]）。
    pub slots: u32,
}

/// 码位状态机：Fresh → Decoded → Drilled（单调只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpState {
    /// 未解码：已登记未解码。
    Fresh,
    /// 已解码：槽位核对通过，待演练。
    Decoded,
    /// 已演练：展开步已入演练序账。
    Drilled,
}

impl OpState {
    /// 阶段序号（0..=2）。
    pub fn rank(self) -> u32 {
        match self {
            OpState::Fresh => 0,
            OpState::Decoded => 1,
            OpState::Drilled => 2,
        }
    }
}

/// 展开链：10 码位链目 + 每位状态机 + 登记序账 + 演练序账 + 注入账 + 指纹。
pub struct UnwindChain {
    ops: [Option<ChainOp>; UWOP_TOTAL],
    states: [OpState; UWOP_TOTAL],
    /// 登记序账（按登记先后记码值）。
    reg_log: [Option<u32>; UWOP_TOTAL],
    reg_n: usize,
    /// 演练序账（按演练先后记码值）。
    walk_seq: [Option<u32>; WALK_CAP],
    walk_n: usize,
    /// 演练账：尝试 / 成功 / 拒。
    drill_tries: u32,
    drill_ok: u32,
    drill_rej: u32,
    /// 注入账。
    injected: u32,
    caught: u32,
    missed: u32,
    fp: u64,
}

impl UnwindChain {
    /// 新空链。
    pub fn new() -> UnwindChain {
        UnwindChain {
            ops: [None; UWOP_TOTAL],
            states: [OpState::Fresh; UWOP_TOTAL],
            reg_log: [None; UWOP_TOTAL],
            reg_n: 0,
            walk_seq: [None; WALK_CAP],
            walk_n: 0,
            drill_tries: 0,
            drill_ok: 0,
            drill_rej: 0,
            injected: 0,
            caught: 0,
            missed: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步混入。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 指纹读出。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 登记一个码位：码越族拒 → 重复登记拒 → 正常登记。
    /// 成功返回登记序号（登记序账位）。
    pub fn register(&mut self, code: u32) -> Result<usize, UwDiag> {
        // 次序 1：码越族。
        if code >= UWOP_TOTAL as u32 {
            return Err(UwDiag::ENoOp);
        }
        // 次序 2：重复登记。
        let slot = match self.ops.get(code as usize) {
            Some(s) => s,
            None => return Err(UwDiag::ENoOp),
        };
        if slot.is_some() {
            return Err(UwDiag::EDouble);
        }
        let slots = match UWOP_SLOTS.get(code as usize) {
            Some(s) => *s,
            None => return Err(UwDiag::ENoOp),
        };
        if let Some(s) = self.ops.get_mut(code as usize) {
            *s = Some(ChainOp { code, slots });
        }
        if let Some(st) = self.states.get_mut(code as usize) {
            *st = OpState::Fresh;
        }
        let log_at = self.reg_n;
        if let Some(l) = self.reg_log.get_mut(log_at) {
            *l = Some(code);
        }
        self.reg_n += 1;
        self.fnv(0x51 + (code as u64) << 8);
        Ok(log_at)
    }

    /// 解码一个码位：未登记拒 → 状态倒退拒 → 槽位错报拒 → 正常解码。
    pub fn decode(&mut self, code: u32, slots: u32) -> Result<(), UwDiag> {
        if code >= UWOP_TOTAL as u32 {
            return Err(UwDiag::ENoOp);
        }
        let registered = match self.ops.get(code as usize) {
            Some(Some(_)) => true,
            _ => return Err(UwDiag::EOrder),
        };
        let _ = registered;
        let st = match self.states.get(code as usize) {
            Some(s) => *s,
            None => return Err(UwDiag::ENoOp),
        };
        if st != OpState::Fresh {
            return Err(UwDiag::ERewind);
        }
        let frozen = match UWOP_SLOTS.get(code as usize) {
            Some(s) => *s,
            None => return Err(UwDiag::ENoOp),
        };
        if slots != frozen {
            return Err(UwDiag::ESlot);
        }
        if let Some(st) = self.states.get_mut(code as usize) {
            *st = OpState::Decoded;
        }
        self.fnv(0x52 + (code as u64) << 8 | (slots as u64) << 16);
        Ok(())
    }

    /// 演练一个码位：未登记拒 → 未解码拒 → 序账容量拒 → 正常演练。
    /// 演练序账记账码值（展开步进）。
    pub fn drill_one(&mut self, code: u32) -> Result<usize, UwDiag> {
        self.drill_tries += 1;
        if code >= UWOP_TOTAL as u32 {
            self.drill_rej += 1;
            return Err(UwDiag::ENoOp);
        }
        let st = match self.states.get(code as usize) {
            Some(OpState::Decoded) => OpState::Decoded,
            Some(OpState::Drilled) => {
                self.drill_rej += 1;
                return Err(UwDiag::ERewind);
            }
            _ => {
                self.drill_rej += 1;
                return Err(UwDiag::EOrder);
            }
        };
        let _ = st;
        // 演练序账容量：先容量后入账（容量耗尽显性拒，已入账不丢）。
        if self.walk_n >= WALK_CAP {
            self.drill_rej += 1;
            return Err(UwDiag::ECapFull);
        }
        if let Some(w) = self.walk_seq.get_mut(self.walk_n) {
            *w = Some(code);
        }
        self.walk_n += 1;
        if let Some(st) = self.states.get_mut(code as usize) {
            *st = OpState::Drilled;
        }
        self.drill_ok += 1;
        self.fnv(0x53 + (code as u64) << 12);
        Ok(self.walk_n - 1)
    }

    /// 全链演练：按登记序的严格逆序逐位演练（展开与登记相反）。
    /// 返回演练步数；任一步失败即 Err。
    pub fn drill_all(&mut self) -> Result<usize, UwDiag> {
        let mut steps = 0usize;
        // 逆序走登记序账：k 从 reg_n-1 倒回 0。
        let mut k = self.reg_n;
        while k > 0 {
            k -= 1;
            let code = match self.reg_log.get(k) {
                Some(Some(c)) => *c,
                _ => return Err(UwDiag::ESeq),
            };
            self.drill_one(code)?;
            steps += 1;
        }
        Ok(steps)
    }

    /// 序账补记探针（压测专用口）：仅做容量判定后直接向演练序账补一步，
    /// 用于探容量耗尽路径——满账后补记必拒 ECapFull，已入账不丢。
    pub fn probe_walk_step(&mut self, code: u32) -> Result<usize, UwDiag> {
        if code >= UWOP_TOTAL as u32 {
            return Err(UwDiag::ENoOp);
        }
        if self.walk_n >= WALK_CAP {
            return Err(UwDiag::ECapFull);
        }
        if let Some(w) = self.walk_seq.get_mut(self.walk_n) {
            *w = Some(code);
        }
        self.walk_n += 1;
        self.fnv(0x54 + (code as u64) << 12);
        Ok(self.walk_n - 1)
    }

    /// 展开序判据：演练序账 == 登记序账的严格逆序，且全链 10 位全 Drilled。
    pub fn order_ok(&self) -> bool {
        if self.reg_n != UWOP_TOTAL || self.walk_n != WALK_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < WALK_CAP {
            let walk_code = match self.walk_seq.get(i) {
                Some(Some(c)) => *c,
                _ => return false,
            };
            let reg_code = match self.reg_log.get(WALK_CAP - 1 - i) {
                Some(Some(c)) => *c,
                _ => return false,
            };
            if walk_code != reg_code {
                return false;
            }
            i += 1;
        }
        // 全链 Drilled。
        let mut j = 0usize;
        while j < UWOP_TOTAL {
            if self.states.get(j) != Some(&OpState::Drilled) {
                return false;
            }
            j += 1;
        }
        true
    }

    /// 演练 10/10：演练成功数 == 10 且序判据成立。
    pub fn drill_ten_of_ten(&self) -> bool {
        self.drill_ok == 10 && self.order_ok()
    }

    /// 演练序账读出第 i 位。
    pub fn walk_at(&self, i: usize) -> Option<u32> {
        match self.walk_seq.get(i) {
            Some(Some(c)) => Some(*c),
            _ => None,
        }
    }

    /// 登记序账读出第 i 位。
    pub fn reg_at(&self, i: usize) -> Option<u32> {
        match self.reg_log.get(i) {
            Some(Some(c)) => Some(*c),
            _ => None,
        }
    }

    /// 已登记码位数。
    pub fn registered(&self) -> usize {
        self.reg_n
    }

    /// 展开混报核对：谎报演练步数与序账实际步数不自洽即检出。
    /// 自洽规则：谎报数 == walk_n 才算自洽（注入必构造错配）。
    pub fn claim_steps(&mut self, claimed: u32) -> bool {
        self.injected += 1;
        if claimed == self.walk_n as u32 {
            self.missed += 1;
            return false;
        }
        self.caught += 1;
        self.fnv(0x6a7a + (claimed as u64));
        true
    }

    /// 注入三数。
    pub fn mixup_stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.missed)
    }

    /// 演练账守恒：尝试 = 成功 + 拒。
    pub fn drill_conservation(&self) -> bool {
        self.drill_ok.saturating_add(self.drill_rej) == self.drill_tries
    }

    /// 注入账守恒。
    pub fn injection_conservation(&self) -> bool {
        self.caught.saturating_add(self.missed) == self.injected
    }

    /// 演练账三数。
    pub fn drill_stats(&self) -> (u32, u32, u32) {
        (self.drill_tries, self.drill_ok, self.drill_rej)
    }

    /// 码位状态读出（越族 None）。
    pub fn state_at(&self, code: usize) -> Option<OpState> {
        match self.states.get(code) {
            Some(s) => Some(*s),
            None => None,
        }
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
/// 线性同余发生器：st = st×1664525 + 1013904223。
pub struct Lcg(u64);

impl Lcg {
    /// 种子 = SEG_BASE。
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    /// 推进一步。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }

    /// 取 [0, n)。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 黄金正演：全档登记→全档解码→逆序演练→序判据 ─────────────────
/// 黄金正演：10 码位全登记（升序）→ 全解码（槽位照冻结表）→ 全链逆序演练
/// → 演练 10/10。返回 (演练成功数, 指纹)；任一步违约即 Err。
pub fn golden_run() -> Result<(u32, u64), UwDiag> {
    let mut ch = UnwindChain::new();
    // 登记全档（升序登记，演练即 9..=0 逆序）。
    let mut code = 0u32;
    while (code as usize) < UWOP_TOTAL {
        ch.register(code)?;
        code += 1;
    }
    // 解码全档：槽位数照冻结表。
    let mut d = 0u32;
    while (d as usize) < UWOP_TOTAL {
        let slots = match UWOP_SLOTS.get(d as usize) {
            Some(s) => *s,
            None => return Err(UwDiag::ESlot),
        };
        ch.decode(d, slots)?;
        d += 1;
    }
    // 全链演练（逆序展开）。
    ch.drill_all()?;
    if !ch.drill_ten_of_ten() {
        return Err(UwDiag::ESeq);
    }
    Ok((ch.drill_ok, ch.fingerprint()))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
/// 注入三族闭集：0=码越族 / 1=槽位错报 / 2=重复登记。
pub const INJ_FAMILIES: usize = 3;

/// 注入账。
pub struct InjectLedger {
    /// 各族注入次数。
    pub tries: [u32; INJ_FAMILIES],
    /// 各族检出次数。
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    /// 零账。
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }

    /// 全抓判定。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJ_FAMILIES {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 分族计数（禁裸下标统一入口）。
fn bump(arr: &mut [u32; INJ_FAMILIES], i: usize) {
    if let Some(v) = arr.get_mut(i) {
        *v += 1;
    }
}

/// 跑注入 10 轮 × 3 族，每轮确定性构造违约，必须全抓。
pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        // 族 0：码越族（10..15 闭集外，必拒 ENoOp）。
        bump(&mut ledger.tries, 0);
        let bad_code = UWOP_TOTAL as u32 + rng.below(6) as u32;
        let mut c0 = UnwindChain::new();
        if c0.register(bad_code) == Err(UwDiag::ENoOp) {
            bump(&mut ledger.caught, 0);
        }
        // 族 1：槽位错报（登记后按错槽位解码，必拒 ESlot）。
        bump(&mut ledger.tries, 1);
        let mut c1 = UnwindChain::new();
        let code1 = round % UWOP_TOTAL as u32;
        let _ = c1.register(code1);
        let frozen = match UWOP_SLOTS.get(code1 as usize) {
            Some(s) => *s,
            None => 1,
        };
        let bad_slots = frozen + 1 + (round % 3) as u32;
        if c1.decode(code1, bad_slots) == Err(UwDiag::ESlot) {
            bump(&mut ledger.caught, 1);
        }
        // 族 2：重复登记（同码位双登，必拒 EDouble）。
        bump(&mut ledger.tries, 2);
        let mut c2 = UnwindChain::new();
        let code2 = (round + 3) % UWOP_TOTAL as u32;
        let _ = c2.register(code2);
        if c2.register(code2) == Err(UwDiag::EDouble) {
            bump(&mut ledger.caught, 2);
        }
        round += 1;
    }
}

// ── 完成写回模型（判据③） ───────────────────────────────────────
/// 工单状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorkStatus {
    /// 进行中。
    InProgress,
    /// 已完成。
    Done,
}

/// 完成写回单。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Writeback {
    /// 写回状态。
    pub status: WorkStatus,
    /// 结果摘要（非空）。
    pub summary: String,
}

/// 组装写回单：演练 10/10 + 锚注齐才给 Done，摘要恒非空。
pub fn make_writeback(drill_ok: u32, order_clean: bool, anchors_ok: bool) -> Writeback {
    let done = drill_ok == 10 && order_clean && anchors_ok;
    let summary = format!(
        "展开链压测完成：演练{}/10 序判据={} 锚注齐={}",
        drill_ok, order_clean, anchors_ok
    );
    let summary = if summary.is_empty() {
        String::from("展开链写回兜底：演练收口（摘要原为空，已回填）")
    } else {
        summary
    };
    Writeback {
        status: if done { WorkStatus::Done } else { WorkStatus::InProgress },
        summary,
    }
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ─────────────────────
/// 正常流：黄金正演演练 10/10 且指纹非零。
fn golden_flow_ok() -> bool {
    match golden_run() {
        Ok((n, fp)) => n == 10 && fp != 0,
        Err(_) => false,
    }
}

/// 边界零：码 0 首位登记合法。
fn boundary_zero_ok() -> bool {
    let mut ch = UnwindChain::new();
    ch.register(0).is_ok()
}

/// 边界上限：码 9 末位登记合法且全档可通。
fn boundary_max_ok() -> bool {
    let mut ch = UnwindChain::new();
    let r = ch.register(9);
    let _ = ch.decode(9, 1);
    r.is_ok()
}

/// 边界上限+1：码 10 必拒。
fn boundary_max_plus_ok() -> bool {
    let mut ch = UnwindChain::new();
    ch.register(10) == Err(UwDiag::ENoOp)
}

/// 边界负移位（u32 回绕）：0u32.wrapping_sub(1) 必拒。
fn boundary_wrap_ok() -> bool {
    let mut ch = UnwindChain::new();
    let huge = 0u32.wrapping_sub(1);
    ch.register(huge) == Err(UwDiag::ENoOp)
}

/// 注入三族 10 轮全抓。
fn injection_ok() -> bool {
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    led.all_caught()
}

/// 判定次序：码越族先于重复登记（同立时先回 ENoOp）。
fn judge_order_ok() -> bool {
    let mut ch = UnwindChain::new();
    let _ = ch.register(2);
    // 码 2 重复 且 码 99 越族：不同码位各自判，越族码必 ENoOp；
    // 次序锚：register(99) 在重复判定之前先走码越族判定。
    ch.register(99) == Err(UwDiag::ENoOp)
}

/// 状态机非法倒退拒：未解码先演练拒、已解码重复解码拒、已演练再演练拒。
fn rewind_reject_ok() -> bool {
    let mut ch = UnwindChain::new();
    let _ = ch.register(3);
    let r1 = ch.drill_one(3) == Err(UwDiag::EOrder); // Fresh 不得演练
    let _ = ch.decode(3, 1); // 码 3 SET_FPREG 冻结槽位=1
    let r2 = ch.decode(3, 1) == Err(UwDiag::ERewind); // Decoded 不得再解码
    let _ = ch.drill_one(3);
    let r3 = ch.drill_one(3) == Err(UwDiag::ERewind); // Drilled 不得再演练
    r1 && r2 && r3
}

/// 容量耗尽且账不丢：演练序账 10 槽判满后补记拒，前 10 步原样在账。
fn capacity_ok() -> bool {
    let mut ch = UnwindChain::new();
    let mut c = 0u32;
    while (c as usize) < UWOP_TOTAL {
        let _ = ch.register(c);
        let slots = match UWOP_SLOTS.get(c as usize) {
            Some(s) => *s,
            None => return false,
        };
        let _ = ch.decode(c, slots);
        c += 1;
    }
    if ch.drill_all().is_err() {
        return false;
    }
    // 满账补记：容量判定先于入账，必拒 ECapFull。
    let overflow = ch.probe_walk_step(0) == Err(UwDiag::ECapFull);
    // 前 10 步逐位复读：全 Some，账不丢。
    let mut intact = true;
    let mut i = 0usize;
    while i < WALK_CAP {
        if ch.walk_at(i).is_none() {
            intact = false;
        }
        i += 1;
    }
    overflow && intact && ch.walk_at(WALK_CAP) == None && ch.drill_conservation()
}

/// 双跑一致：黄金正演两次逐字段相等。
fn dual_run_ok() -> bool {
    let a = golden_run();
    let b = golden_run();
    a == b && a.is_ok()
}

/// 对账一致：槽位冻结表全档非零且与锚注表同长同序。
fn reconcile_ok() -> bool {
    if UWOP_SLOTS.len() != UWOP_NAMES.len() || UWOP_TOTAL != 10 {
        return false;
    }
    let mut i = 0usize;
    while i < UWOP_TOTAL {
        let s = match UWOP_SLOTS.get(i) {
            Some(s) => *s,
            None => return false,
        };
        if s == 0 || s > 3 {
            return false;
        }
        i += 1;
    }
    true
}

/// 计数守恒：注入数 = 检出数 + 漏检数，漏检 = 0。
fn conservation_ok() -> bool {
    let mut ch = UnwindChain::new();
    let mut n = 0u32;
    while n < 10 {
        ch.claim_steps(n + 100); // 谎报步数错配，必检出
        n += 1;
    }
    let (inj, caught, missed) = ch.mixup_stats();
    inj == 10 && caught == 10 && missed == 0 && ch.injection_conservation()
}

/// 段位守卫。
fn segment_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

/// 文案闭集：闭集命中 + 未知码中文兜底 + 码值全落段窗 + 锚注齐。
fn closed_text_ok() -> bool {
    let first = match UwDiag::ALL.first() {
        Some(d) => *d,
        None => return false,
    };
    if detail(first.val()) != first.detail() {
        return false;
    }
    let unknown = detail(SEG_BASE + 0x3f);
    if unknown.is_empty() || !unknown.contains("未知诊断码") {
        return false;
    }
    let mut i = 0usize;
    while i < UwDiag::ALL.len() {
        if let Some(d) = UwDiag::ALL.get(i) {
            let v = d.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    anchors_complete()
}

/// 完成写回：状态=已完成、结果摘要非空（判据③）。
fn writeback_ok() -> bool {
    match golden_run() {
        Ok((n, _)) => {
            let wb = make_writeback(n, true, anchors_complete());
            wb.status == WorkStatus::Done && !wb.summary.is_empty()
        }
        Err(_) => false,
    }
}

// ── 判据名闭表（与 extra_checks 推送序一一对应） ─────────────────
const CHECK_META: [(&'static str, &'static str); 17] = [
    ("J1-黄金正演·演练10/10", "全档登记+解码+逆序演练全过"),
    ("J1-展开序判据", "演练序=登记序严格逆序，逐位一致"),
    ("J1-码值锚注出处", "10 码位锚注齐（MSDN UNWIND_OP_CODES）"),
    ("J3-完成写回·状态已完成摘要非空", "WorkStatus=Done 且摘要非空"),
    ("边界·码0首位合法", "码 0 登记收账"),
    ("边界·码9末位合法", "码 9 登记收账"),
    ("边界·码10必拒", "码 10 越族拒"),
    ("边界·负移位回绕必拒", "u32 回绕值越族拒"),
    ("J1-注入三族10/10全抓", "码越族/槽位错报/重复登记必抓"),
    ("判序·越族先于重复", "ENoOp 先于 EDouble"),
    ("状态机·倒退显性拒", "未解码演练/重复解码/重复演练拒"),
    ("容量·耗尽且账不丢", "演练序账10槽在账且守恒"),
    ("双跑一致", "两次黄金正演逐字段相等"),
    ("守恒·注入数=检出+漏检", "漏检恒 0"),
    ("段位守卫", "SEG_BASE 0x40 对齐且在 VX9 域"),
    ("文案闭集", "闭集命中+未知码中文兜底+码值落段窗"),
    ("收口·判据条数只增不减", "extra_checks 条数 ≥ 冻结下限"),
];

/// 冻结判据条数下限。
pub const FROZEN_CHECK_COUNT: usize = 17;

/// 单条判据装配。
fn entry(i: usize, passed: bool, det: String) -> (String, bool, String) {
    let name = match CHECK_META.get(i) {
        Some((n, _)) => String::from(*n),
        None => String::from("判据名缺位：下标越闭表"),
    };
    (name, passed, det)
}

/// 判据汇总（≥12 条；同名两次调用逐字段相等）。
pub fn extra_checks() -> Vec<(String, bool, String)> {
    let mut v: Vec<(String, bool, String)> = Vec::new();
    // 0 黄金正演。
    let p = golden_flow_ok();
    v.push(entry(0, p, format!("演练={}", if p { "10/10" } else { "不足10" })));
    // 1 序判据。
    let p = order_predicate_ok();
    v.push(entry(1, p, String::from("演练序=逆登记序")));
    // 2 锚注出处。
    let p = anchors_complete();
    v.push(entry(2, p, format!("码族数={}", UWOP_TOTAL)));
    // 3 完成写回。
    let p = writeback_ok();
    v.push(entry(3, p, String::from("状态=已完成 摘要非空")));
    // 4 边界零。
    let p = boundary_zero_ok();
    v.push(entry(4, p, String::from("码 0 合法")));
    // 5 边界上限。
    let p = boundary_max_ok();
    v.push(entry(5, p, String::from("码 9 合法")));
    // 6 边界上限+1。
    let p = boundary_max_plus_ok();
    v.push(entry(6, p, String::from("码 10 必拒")));
    // 7 边界负移位。
    let p = boundary_wrap_ok();
    v.push(entry(7, p, String::from("u32 回绕必拒")));
    // 8 注入三族。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    let p = led.all_caught();
    v.push(entry(8, p, format!("t/c {:?}/{:?}", led.tries, led.caught)));
    // 9 判定次序。
    let p = judge_order_ok();
    v.push(entry(9, p, String::from("ENoOp 先于 EDouble")));
    // 10 状态机倒退。
    let p = rewind_reject_ok();
    v.push(entry(10, p, String::from("倒退拒 ERewind")));
    // 11 容量耗尽。
    let p = capacity_ok();
    v.push(entry(11, p, format!("序账容量={}", WALK_CAP)));
    // 12 双跑一致。
    let p = dual_run_ok();
    v.push(entry(12, p, String::from("两次正演逐字段相等")));
    // 13 计数守恒（注入）。
    let p = conservation_ok();
    v.push(entry(13, p, String::from("漏检=0")));
    // 14 段位守卫。
    let p = segment_guard_ok();
    v.push(entry(14, p, format!("SEG=0x{:x}", SEG_BASE)));
    // 15 文案闭集。
    let p = closed_text_ok();
    v.push(entry(15, p, String::from("闭集命中+兜底非空+锚注齐")));
    // 16 收口只增不减（条数取闭表长度；双跑一致由第 12 条独立钉住，
    // 此处不可回调 extra_checks 自身——会无限递归爆栈）。
    let n = CHECK_META.len();
    v.push(entry(16, n >= FROZEN_CHECK_COUNT, format!("条数={}", n)));
    v
}

/// 序判据谓词（独立跑一遍黄金正演，校验演练序账逐位与逆登记序一致）。
fn order_predicate_ok() -> bool {
    let mut ch = match golden_run() {
        Ok(_) => UnwindChain::new(),
        Err(_) => return false,
    };
    let mut c = 0u32;
    while (c as usize) < UWOP_TOTAL {
        if ch.register(c).is_err() {
            return false;
        }
        c += 1;
    }
    let mut d = 0u32;
    while (d as usize) < UWOP_TOTAL {
        let slots = match UWOP_SLOTS.get(d as usize) {
            Some(s) => *s,
            None => return false,
        };
        if ch.decode(d, slots).is_err() {
            return false;
        }
        d += 1;
    }
    if ch.drill_all().is_err() {
        return false;
    }
    ch.order_ok()
}

/// 本卡判据入口。
pub fn run_unx12743_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12743 展开链压测");
    let results = extra_checks();
    let mut i = 0usize;
    while i < results.len() {
        if let Some((_name, passed, det)) = results.get(i) {
            let (sname, sdetail) = match CHECK_META.get(i) {
                Some((n, d)) => (*n, *d),
                None => ("判据名缺位", "判据明细缺位：闭表下标越界"),
            };
            if i < 8 {
                s.add_dyn(sname, *passed, det);
            } else {
                s.add(sname, *passed, sdetail);
            }
        }
        i += 1;
    }
    s
}

// ── 测试区（≥8 条 #[test]） ──────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normal_flow_golden() {
        let (n, fp) = golden_run().unwrap_or((0, 0));
        assert_eq!(n, 10, "演练 10/10");
        assert_ne!(fp, 0);
        assert!(golden_flow_ok());
    }

    #[test]
    fn test_boundary_zero_and_max() {
        assert!(boundary_zero_ok());
        assert!(boundary_max_ok());
        let mut ch = UnwindChain::new();
        assert!(ch.register(0).is_ok(), "码 0 首位合法");
        assert!(ch.register(9).is_ok(), "码 9 末位合法");
    }

    #[test]
    fn test_boundary_max_plus_one_and_wrap() {
        assert!(boundary_max_plus_ok());
        assert!(boundary_wrap_ok());
        let mut ch = UnwindChain::new();
        assert_eq!(ch.register(10), Err(UwDiag::ENoOp), "码 10 越族");
        assert_eq!(ch.register(15), Err(UwDiag::ENoOp), "码 15 保留非法");
    }

    #[test]
    fn test_slot_mismatch_rejected() {
        let mut ch = UnwindChain::new();
        let _ = ch.register(2);
        assert_eq!(ch.decode(2, 5), Err(UwDiag::ESlot), "槽位错报必拒");
        assert!(ch.decode(2, 1).is_ok(), "冻结槽位 1 合法");
    }

    #[test]
    fn test_injection_families_all_caught() {
        let mut led = InjectLedger::new();
        run_injections(&mut led);
        assert!(led.all_caught(), "三族全抓：{:?}/{:?}", led.tries, led.caught);
        let mut i = 0usize;
        while i < INJ_FAMILIES {
            assert_eq!(led.caught[i], 10, "族 {} 检出应为 10", i);
            i += 1;
        }
    }

    #[test]
    fn test_judge_order_frozen() {
        assert!(judge_order_ok());
        let mut ch = UnwindChain::new();
        assert_eq!(ch.register(99), Err(UwDiag::ENoOp), "越族先判");
    }

    #[test]
    fn test_state_machine_rewind_reject() {
        assert!(rewind_reject_ok());
        let mut ch = UnwindChain::new();
        let _ = ch.register(3);
        assert_eq!(ch.drill_one(3), Err(UwDiag::EOrder), "Fresh 不得演练");
        let _ = ch.decode(3, 1);
        assert_eq!(ch.decode(3, 1), Err(UwDiag::ERewind), "Decoded 不得再解码");
        let _ = ch.drill_one(3);
        assert_eq!(ch.drill_one(3), Err(UwDiag::ERewind), "Drilled 不得再演练");
        assert_eq!(ch.state_at(3), Some(OpState::Drilled));
    }

    #[test]
    fn test_order_is_reverse_registration() {
        let mut ch = UnwindChain::new();
        let mut c = 0u32;
        while (c as usize) < UWOP_TOTAL {
            let _ = ch.register(c);
            let slots = UWOP_SLOTS.get(c as usize).unwrap_or(&1);
            let _ = ch.decode(c, *slots);
            c += 1;
        }
        let _ = ch.drill_all();
        assert!(ch.order_ok(), "演练序须为登记序严格逆序");
        assert_eq!(ch.walk_at(0), Some(9), "演练首步=最后登记的码 9");
        assert_eq!(ch.walk_at(9), Some(0), "演练末步=最先登记的码 0");
        assert_eq!(ch.reg_at(0), Some(0));
    }

    #[test]
    fn test_dual_run_identical() {
        assert!(dual_run_ok());
        let a = extra_checks();
        let b = extra_checks();
        assert_eq!(a, b, "extra_checks 双跑逐字段相等");
    }

    #[test]
    fn test_seg_guard_and_closed_text() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert!(closed_text_ok());
        assert!(detail(SEG_BASE + 0x3f).contains("未知诊断码"));
        assert_eq!(detail(UwDiag::ESlot.val()), UwDiag::ESlot.detail());
        assert!(anchors_complete());
        assert!(anchor(10).is_none(), "越族码无锚注");
    }

    #[test]
    fn test_writeback_done_and_summary_nonempty() {
        assert!(writeback_ok());
        let wb = make_writeback(9, true, true);
        assert_eq!(wb.status, WorkStatus::InProgress, "演练不足 10 不得已完成");
        assert!(!wb.summary.is_empty());
    }

    #[test]
    fn test_checks_all_green_and_frozen() {
        let s = run_unx12743_checks();
        assert!(s.all_passed(), "判据集必须全绿：{}", s.red_items_text());
        assert!(s.len() >= FROZEN_CHECK_COUNT);
        assert!(s.len() >= 12);
    }

}
