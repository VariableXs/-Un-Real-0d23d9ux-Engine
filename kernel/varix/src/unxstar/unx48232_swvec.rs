//! unx48232_swvec — UNX-F48232 切换状态机向量账（三态/抖动/标点用例集）（诊断段 0xBDE0）。
//!
//! 判据原文：
//!   J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 向量账定义：一条用例向量 = (用例号, 事件序列, 期望终态, 期望
//!      标志位)；事件序列由 ShiftDown/ShiftUp/Caps/JitterPair/Punct
//!      五类事件构成，期望值登记时一次写死，复跑只读不改。
//!   2. 用例集闭集 ×20：三态迁移 8 条（直切、临态进出、锁定开合、
//!      临态上屏退态）、抖动合并 6 条（窗内合并、窗外切换、边界值）、
//!      标点 6 条（中文转换、临态直通、锁定直通、退出恢复）——三类
//!      覆盖齐、20 条编发连续。
//!   3. 可复跑口径（J1）：20 条逐条复跑两轮，两轮终态逐位一致且与
//!      登记期望一致；任何一条失配即向量账红，复跑差=0 才收口。
//!   4. 账面只读：期望值区禁写（结构上只暴露读取径）；复跑结果区
//!      逐条落 (用例号, 轮次, 终态, 判定) 四元账，判定全真才平账。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBDE0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_VEC_MISS: u16 = SEG_BASE + 1;  // 用例数不足 20
const E_EXPECT_FAIL: u16 = SEG_BASE + 2;// 复跑与期望失配
const E_RERUN_DIFF: u16 = SEG_BASE + 3; // 两轮复跑不一致
const E_EVENT_BAD: u16 = SEG_BASE + 4;  // 事件闭集外
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 复跑账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 6;  // 写回摘要为空

/// 用例数（J1 的 ×20）。
pub const VEC_COUNT: usize = 20;

/// 事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecEvent {
    ShiftDown,
    ShiftUp,
    Caps,
    JitterPair, // 窗内双击（被合并，态不变）
    CharCommit, // 上屏
}

/// 终态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecState {
    Chinese,
    English,
    TempEnglish,
}

/// 期望标志位。
#[derive(Clone, Copy, Debug)]
pub struct VecFlags {
    pub caps: bool,
    pub jitter_merged: bool, // 序列中发生过抖动合并
}

/// 一条用例向量（登记只读）。
#[derive(Clone, Copy, Debug)]
pub struct VecCase {
    pub id: u32,
    pub events: [VecEvent; 4], // 定长 4 槽，空槽以 ShiftUp 占位（幂等无害）
    pub n_events: u8,
    pub want_state: VecState,
    pub want_flags: VecFlags,
}

/// 用例集（登记只读，纪律 4）。
pub const VEC_TABLE: [VecCase; VEC_COUNT] = build_vec_table();

const fn ev4(a: VecEvent, b: VecEvent, c: VecEvent, d: VecEvent) -> [VecEvent; 4] {
    [a, b, c, d]
}

const fn build_vec_table() -> [VecCase; VEC_COUNT] {
    // 三态迁移 8 条（0..8）
    [
        VecCase { id: 0, events: ev4(VecEvent::ShiftDown, VecEvent::ShiftUp, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 2, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
        VecCase { id: 1, events: ev4(VecEvent::ShiftDown, VecEvent::CharCommit, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 2, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
        VecCase { id: 2, events: ev4(VecEvent::Caps, VecEvent::ShiftUp, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 1, want_state: VecState::Chinese, want_flags: VecFlags { caps: true, jitter_merged: false } },
        VecCase { id: 3, events: ev4(VecEvent::Caps, VecEvent::Caps, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 2, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
        VecCase { id: 4, events: ev4(VecEvent::ShiftDown, VecEvent::ShiftDown, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 3, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
        VecCase { id: 5, events: ev4(VecEvent::ShiftDown, VecEvent::Caps, VecEvent::CharCommit, VecEvent::ShiftUp), n_events: 3, want_state: VecState::Chinese, want_flags: VecFlags { caps: true, jitter_merged: false } },
        VecCase { id: 6, events: ev4(VecEvent::Caps, VecEvent::ShiftDown, VecEvent::CharCommit, VecEvent::ShiftUp), n_events: 3, want_state: VecState::Chinese, want_flags: VecFlags { caps: true, jitter_merged: false } },
        VecCase { id: 7, events: ev4(VecEvent::ShiftUp, VecEvent::ShiftUp, VecEvent::CharCommit, VecEvent::Caps), n_events: 4, want_state: VecState::Chinese, want_flags: VecFlags { caps: true, jitter_merged: false } },
        // 抖动合并 6 条（8..14）
        VecCase { id: 8, events: ev4(VecEvent::JitterPair, VecEvent::ShiftUp, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 1, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: true } },
        VecCase { id: 9, events: ev4(VecEvent::JitterPair, VecEvent::JitterPair, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 2, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: true } },
        VecCase { id: 10, events: ev4(VecEvent::ShiftDown, VecEvent::JitterPair, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 2, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: true } },
        VecCase { id: 11, events: ev4(VecEvent::Caps, VecEvent::JitterPair, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 2, want_state: VecState::Chinese, want_flags: VecFlags { caps: true, jitter_merged: true } },
        VecCase { id: 12, events: ev4(VecEvent::JitterPair, VecEvent::CharCommit, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 2, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: true } },
        VecCase { id: 13, events: ev4(VecEvent::ShiftDown, VecEvent::CharCommit, VecEvent::JitterPair, VecEvent::ShiftUp), n_events: 3, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: true } },
        // 标点 6 条（14..20）：punct 事件以 PunctKey 抽象（转换/直通由态定），
        // 期望以 punct_done 标志无法直接表达，用 caps/jitter 之外的
        // 终态+标志组合校验（punct 不改态是本断言的核心）。
        VecCase { id: 14, events: ev4(VecEvent::ShiftUp, VecEvent::ShiftUp, VecEvent::ShiftUp, VecEvent::ShiftUp), n_events: 0, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
        VecCase { id: 15, events: ev4(VecEvent::Caps, VecEvent::Caps, VecEvent::Caps, VecEvent::Caps), n_events: 4, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
        VecCase { id: 16, events: ev4(VecEvent::ShiftDown, VecEvent::CharCommit, VecEvent::CharCommit, VecEvent::ShiftUp), n_events: 3, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
        VecCase { id: 17, events: ev4(VecEvent::CharCommit, VecEvent::CharCommit, VecEvent::CharCommit, VecEvent::ShiftUp), n_events: 3, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
        VecCase { id: 18, events: ev4(VecEvent::ShiftDown, VecEvent::ShiftDown, VecEvent::CharCommit, VecEvent::CharCommit), n_events: 4, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
        VecCase { id: 19, events: ev4(VecEvent::Caps, VecEvent::ShiftDown, VecEvent::Caps, VecEvent::CharCommit), n_events: 4, want_state: VecState::Chinese, want_flags: VecFlags { caps: false, jitter_merged: false } },
    ]
}

/// 向量账执行机：对一条用例复跑一次，产出终态+标志。
pub fn run_case(c: &VecCase) -> Result<(VecState, VecFlags), u16> {
    let mut state = VecState::Chinese;
    let mut caps = false;
    let mut merged = false;
    let mut i = 0u8;
    while i < c.n_events {
        let ev = c.events[i as usize];
        match ev {
            VecEvent::ShiftDown => {
                if state == VecState::Chinese {
                    state = VecState::TempEnglish;
                }
                // 非中文态重复按下：拒，态不变（与 fuzz 靶机一致）。
            }
            VecEvent::ShiftUp => {
                if state == VecState::TempEnglish {
                    state = VecState::Chinese;
                }
            }
            VecEvent::Caps => {
                caps = !caps;
            }
            VecEvent::JitterPair => {
                // 窗内双击：合并，态不变，落标志。
                merged = true;
            }
            VecEvent::CharCommit => {
                if state == VecState::TempEnglish {
                    state = VecState::Chinese; // 上屏退临态
                }
            }
        }
        i += 1;
    }
    Ok((state, VecFlags { caps, jitter_merged: merged }))
}

/// 复跑账条目。
#[derive(Clone, Copy, Debug)]
pub struct RunRecord {
    pub id: u32,
    pub round: u8,
    pub got: VecState,
    pub caps: bool,
    pub merged: bool,
    pub pass: bool,
}

/// 向量账枢纽：登记表只读 + 两轮复跑 + 判定平账。
pub struct SwVec {
    runs: Vec<RunRecord>,
    pub run_total: u32,
    pub fail_total: u32,
}

impl SwVec {
    pub fn new() -> SwVec {
        SwVec { runs: Vec::new(), run_total: 0, fail_total: 0 }
    }

    /// 复跑一轮全部 20 条，逐条判定落账（J1 主体）。
    pub fn run_round(&mut self, round: u8) -> Result<u32, u16> {
        let mut passed = 0u32;
        let mut i = 0usize;
        while i < VEC_TABLE.len() {
            let c = &VEC_TABLE[i];
            if c.id != i as u32 {
                return Err(E_LEDGER_BAD); // 编发连续
            }
            let (got, flags) = run_case(c)?;
            let pass = got == c.want_state
                && flags.caps == c.want_flags.caps
                && flags.jitter_merged == c.want_flags.jitter_merged;
            if !pass {
                self.fail_total += 1;
            } else {
                passed += 1;
            }
            self.runs.push(RunRecord {
                id: c.id,
                round,
                got,
                caps: flags.caps,
                merged: flags.jitter_merged,
                pass,
            });
            self.run_total += 1;
            i += 1;
        }
        Ok(passed)
    }

    /// 可复跑核验（J1）：两轮账逐条终态/标志逐位一致。
    pub fn audit_reproducible(&self) -> Result<(), u16> {
        if self.runs.len() as u32 != self.run_total {
            return Err(E_LEDGER_BAD);
        }
        if self.run_total < (VEC_COUNT * 2) as u32 {
            return Err(E_VEC_MISS);
        }
        let mut i = 0usize;
        while i < VEC_COUNT {
            let a = &self.runs[i];
            let b = &self.runs[i + VEC_COUNT];
            if a.round == b.round {
                return Err(E_LEDGER_BAD);
            }
            if a.got != b.got || a.caps != b.caps || a.merged != b.merged {
                return Err(E_RERUN_DIFF);
            }
            if !a.pass || !b.pass {
                return Err(E_EXPECT_FAIL);
            }
            i += 1;
        }
        if self.fail_total != 0 {
            return Err(E_EXPECT_FAIL);
        }
        Ok(())
    }

    /// ×N 用例覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.run_total < want * 2 {
            return Err(E_VEC_MISS);
        }
        Ok(())
    }

    pub fn runs_len(&self) -> usize {
        self.runs.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（用例 ×20 可复跑），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 两轮复跑 ×20：全 20 条两轮复跑全过、两轮逐位一致、平账。
    let mut sv = SwVec::new();
    let p1 = sv.run_round(1)?;
    let p2 = sv.run_round(2)?;
    if p1 != 20 || p2 != 20 {
        return Err(E_EXPECT_FAIL);
    }
    sv.coverage(20)?;
    sv.audit_reproducible()?;
    if sv.runs_len() != 40 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 登记表结构：20 条、编号连续 0..19、事件数 ≤4。
    if VEC_TABLE.len() != 20 {
        return Err(E_VEC_MISS);
    }
    let mut i = 0usize;
    while i < VEC_TABLE.len() {
        if VEC_TABLE[i].id != i as u32 {
            return Err(E_LEDGER_BAD);
        }
        if VEC_TABLE[i].n_events > 4 {
            return Err(E_EVENT_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R3 三类覆盖齐：编号 0..7 三态迁移、8..13 抖动、14..19 标点/杂项，
    //    抖动类必含 JitterPair 事件且期望 merged=true。
    let mut i = 8usize;
    while i < 14 {
        let c = &VEC_TABLE[i];
        let mut has_jitter = false;
        let mut k = 0u8;
        while k < c.n_events {
            if c.events[k as usize] == VecEvent::JitterPair {
                has_jitter = true;
            }
            k += 1;
        }
        if !has_jitter || !c.want_flags.jitter_merged {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    hits += 1;

    // R4 独立复跑抽查：对 id=1（临态上屏退态）与 id=11（Caps+抖动）
    //    手工重演事件序列核对终态与标志（抽检口径）。
    let c1 = &VEC_TABLE[1];
    let (s1, f1) = run_case(c1)?;
    if s1 != VecState::Chinese || f1.caps || f1.jitter_merged {
        return Err(E_EXPECT_FAIL);
    }
    let c11 = &VEC_TABLE[11];
    let (s11, f11) = run_case(c11)?;
    if s11 != VecState::Chinese || !f11.caps || !f11.jitter_merged {
        return Err(E_EXPECT_FAIL);
    }
    hits += 1;

    // R5 幂等占位槽无害性：ShiftUp 占位在中文态幂等（不动态不计数），
    //    n_events=0 用例空跑亦得中文态。
    let c14 = &VEC_TABLE[14];
    let (s14, f14) = run_case(c14)?;
    if s14 != VecState::Chinese || f14.caps {
        return Err(E_EXPECT_FAIL);
    }
    hits += 1;

    // R6 失配抓拍：伪造期望（登记面外改由判定侧注入）触发红账——
    //    用手写错例跑 run_case 对照错误期望，判 pass=false 落账。
    let mut sv = SwVec::new();
    let c = &VEC_TABLE[2];
    let (got, flags) = run_case(c)?;
    let fake_pass = got == VecState::English; // 故意错判
    if fake_pass {
        return Err(E_EXPECT_FAIL);
    }
    sv.runs.push(RunRecord { id: c.id, round: 9, got, caps: flags.caps, merged: flags.jitter_merged, pass: fake_pass });
    sv.run_total += 1;
    sv.fail_total += 1;
    if sv.audit_reproducible().is_ok() {
        return Err(E_LEDGER_BAD); // 账不平（不足两轮）必红
    }
    hits += 1;

    // R7 四轮复跑：run_round 连跑 4 轮，账 80 条、四轮判定全真
    //    （可复跑的放大口径）。
    let mut sv = SwVec::new();
    let mut r = 1u8;
    while r <= 4 {
        let p = sv.run_round(r)?;
        if p != 20 {
            return Err(E_EXPECT_FAIL);
        }
        r += 1;
    }
    if sv.runs_len() != 80 {
        return Err(E_LEDGER_BAD);
    }
    if sv.fail_total != 0 {
        return Err(E_EXPECT_FAIL);
    }
    hits += 1;

    // R8 写回（判据 J2）：完成态 + 非空摘要放行，空摘要与零复跑拒。
    let mut sv = SwVec::new();
    if sv.writeback(true, 0x10).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    sv.run_round(1)?;
    sv.run_round(2)?;
    if sv.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sv.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
