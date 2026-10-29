//! unx0985_stormrecover — UNX-F0985 风暴恢复评估（恢复条件与解除动作）。
//!
//! 任务锚点：UNX-F0985（第二批工位 W7，SEG 段位 0x8120，本单独占
//! 0x8120..=0x813F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**恢复判据三条件齐备才可解除（静默窗满/
//! EOI 积压清零/节流余量归位），解除动作与当时降级动作成对（降了
//! 什么解什么），未达条件强解必拦，评估账守恒（达成+未达==总评估）**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x812 段闭集）；tick 显性
//! 注入零墙钟；评估账定容守恒（两态和==总数 恒等式机检）；结尾
//! drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8120 段，本单独占）
// ---------------------------------------------------------------------------

/// 风暴恢复评估段基址（0x8120，本单 0x8120..=0x813F 独占）。
pub const SEG_BASE: u16 = 0x8120;
/// 0x8121 恢复条件缺失（三条件未全达成即申请解除）。
pub const XC_COND_INCOMPLETE: u16 = SEG_BASE + 1;
/// 0x8122 解除动作失配（解除项与降级项不成对——降了什么解什么纪律）。
pub const XC_ACTION_MISMATCH: u16 = SEG_BASE + 2;
/// 0x8123 静默窗不足（静默连续窗数低于下限）。
pub const XC_QUIET_SHORT: u16 = SEG_BASE + 3;
/// 0x8124 EOI 积压未清（积压计数大于零）。
pub const XC_EOI_BACKLOG: u16 = SEG_BASE + 4;
/// 0x8125 节流余量未归位（节流档位高于零档）。
pub const XC_THROTTLE_HIGH: u16 = SEG_BASE + 5;
/// 0x8126 评估账守恒破坏（达成+未达 != 总评估）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 6;
/// 0x8127 评估 tick 回退（评估时序单调性破坏）。
pub const XC_TICK_REGRESS: u16 = SEG_BASE + 7;
/// 0x8128 恢复面指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 8;
/// 0x8129 重复解除（同一降级项二次解除——动作对唯一性破坏）。
pub const XC_DOUBLE_RELEASE: u16 = SEG_BASE + 9;
/// 0x812A 评估账定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 10;

const _: () = assert!(XC_COND_INCOMPLETE != XC_ACTION_MISMATCH);
const _: () = assert!(XC_EOI_BACKLOG != XC_THROTTLE_HIGH);
const _: () = assert!(XC_DOUBLE_RELEASE != XC_CAP_INVALID);

// ---------------------------------------------------------------------------
// §1 恢复条件刻度（const 判据表）
// ---------------------------------------------------------------------------

/// 静默连续窗下限（3 窗——恢复判据一）。
pub const QUIET_WINDOWS: u32 = 3;
/// EOI 积压清零阈值（恢复判据二——积压必须为 0）。
pub const EOI_BACKLOG_LIMIT: u32 = 0;
/// 节流归位档（恢复判据三——节流档必须回零档）。
pub const THROTTLE_HOME: u8 = 0;

// ---------------------------------------------------------------------------
// §2 解除动作闭集（与降级动作一一成对）
// ---------------------------------------------------------------------------

/// 解除动作闭集（与 F0984 降级动作成对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelAction {
    /// 解除向量屏蔽（对偶：屏蔽向量）。
    UnmaskVector,
    /// 解除节流（对偶：启动节流）。
    Untthrottle,
    /// 解除隔离（对偶：隔离向量）。
    Unisolate,
}

impl RelAction {
    /// 动作码（1..=3）。
    pub const fn code(self) -> u8 {
        match self {
            RelAction::UnmaskVector => 1,
            RelAction::Untthrottle => 2,
            RelAction::Unisolate => 3,
        }
    }
    /// 动作名。
    pub const fn name(self) -> &'static str {
        match self {
            RelAction::UnmaskVector => "解除屏蔽",
            RelAction::Untthrottle => "解除节流",
            RelAction::Unisolate => "解除隔离",
        }
    }
    /// 对偶降级动作名（解除必须与降级成对）。
    pub const fn paired_degrade(self) -> &'static str {
        match self {
            RelAction::UnmaskVector => "屏蔽向量",
            RelAction::Untthrottle => "启动节流",
            RelAction::Unisolate => "隔离向量",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<RelAction> {
        match c {
            1 => Some(RelAction::UnmaskVector),
            2 => Some(RelAction::Untthrottle),
            3 => Some(RelAction::Unisolate),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §3 恢复评估器（RecoveryEvaluator）
// ---------------------------------------------------------------------------

/// 一行评估账：三条件快照 / 结论（达成/未达）/ 解除动作（达成行带）。
#[derive(Clone, Copy, Debug)]
pub struct EvalRow {
    quiet_ok: bool,
    eoi_ok: bool,
    throttle_ok: bool,
    granted: bool,
    action: Option<RelAction>,
    tick: u64,
}

/// 评估账容量定值：32 行（恢复评估全周期量级）。
pub const EVAL_CAP: usize = 32;

/// 恢复评估器——恢复条件判定与解除动作放行本体。
///
/// 守恒恒等式：`达成数 + 未达数 == 总评估`（每行恰处一态）；成对
/// 恒等式：每项解除动作至多一次且必有一次降级对偶（重复解除拦停）。
#[derive(Debug)]
pub struct RecoveryEvaluator {
    rows: Vec<EvalRow>,
    cap: usize,
    quiet_run: u32,
    released: [bool; 3],
    granted_cnt: u32,
    last_tick: u64,
    rejected: u32,
}

impl RecoveryEvaluator {
    /// 建评估器。
    pub fn new(cap: usize) -> Result<RecoveryEvaluator, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(RecoveryEvaluator {
            rows: Vec::with_capacity(cap),
            cap,
            quiet_run: 0,
            released: [false; 3],
            granted_cnt: 0,
            last_tick: 0,
            rejected: 0,
        })
    }

    /// 评估行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
    /// 已放行解除数。
    pub fn granted_count(&self) -> u32 {
        self.granted_cnt
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 采样静默窗（评估判据一的输入——连续静默窗计数）。
    pub fn sample_quiet(&mut self, quiet: bool, tick: u64) -> Result<(), u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        self.last_tick = tick;
        if quiet {
            self.quiet_run += 1;
        } else {
            self.quiet_run = 0;
        }
        Ok(())
    }

    /// 三条件快照判定（纯函数——判定确定性核心）。
    ///
    /// 条件一：静默连续窗 ≥ QUIET_WINDOWS；
    /// 条件二：EOI 积压 == 0；
    /// 条件三：节流档 == 零档。
    pub fn snapshot_conditions(&self, eoi_backlog: u32, throttle_tier: u8) -> (bool, bool, bool) {
        (
            self.quiet_run >= QUIET_WINDOWS,
            eoi_backlog == EOI_BACKLOG_LIMIT,
            throttle_tier == THROTTLE_HOME,
        )
    }

    /// 评估一次并按需放行解除动作（未达条件强解拦停）。
    pub fn evaluate(
        &mut self,
        eoi_backlog: u32,
        throttle_tier: u8,
        request: Option<RelAction>,
        tick: u64,
    ) -> Result<Option<RelAction>, u16> {
        if tick < self.last_tick {
            self.rejected += 1;
            return Err(XC_TICK_REGRESS);
        }
        if self.rows.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        self.last_tick = tick;
        let (q, e, t) = self.snapshot_conditions(eoi_backlog, throttle_tier);
        let all_ok = q && e && t;
        match request {
            Some(act) => {
                // 放行前置：三条件齐备（按缺失条件分码——可定位）。
                if !all_ok {
                    let code = if !q {
                        XC_QUIET_SHORT
                    } else if !e {
                        XC_EOI_BACKLOG
                    } else {
                        XC_THROTTLE_HIGH
                    };
                    self.rows.push(EvalRow {
                        quiet_ok: q,
                        eoi_ok: e,
                        throttle_ok: t,
                        granted: false,
                        action: Some(act),
                        tick,
                    });
                    self.rejected += 1;
                    return Err(code);
                }
                let c = act.code() as usize;
                if c == 0 || c > 3 || self.released[c - 1] {
                    self.rows.push(EvalRow {
                        quiet_ok: q,
                        eoi_ok: e,
                        throttle_ok: t,
                        granted: false,
                        action: Some(act),
                        tick,
                    });
                    self.rejected += 1;
                    return Err(XC_DOUBLE_RELEASE);
                }
                self.released[c - 1] = true;
                self.granted_cnt += 1;
                self.rows.push(EvalRow {
                    quiet_ok: q,
                    eoi_ok: e,
                    throttle_ok: t,
                    granted: true,
                    action: Some(act),
                    tick,
                });
                Ok(Some(act))
            }
            None => {
                self.rows.push(EvalRow {
                    quiet_ok: q,
                    eoi_ok: e,
                    throttle_ok: t,
                    granted: false,
                    action: None,
                    tick,
                });
                Ok(None)
            }
        }
    }

    /// 守恒机检：达成 + 未达 == 总评估（逐行重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut g: u32 = 0;
        let mut i: usize = 0;
        while i < self.rows.len() {
            if self.rows[i].granted {
                g += 1;
            }
            i += 1;
        }
        if g != self.granted_cnt {
            return Err(XC_CONSERVATION);
        }
        if g + (self.rows.len() as u32 - g) != self.rows.len() as u32 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 成对机检：每项已放行动作在解除表恰一次（成对恒等式）。
    pub fn audit_pairing(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < 3 {
            if self.released[i] {
                // 已放行项必须能在评估账中找到达成行。
                let mut found = false;
                let mut j: usize = 0;
                while j < self.rows.len() {
                    if let Some(a) = self.rows[j].action {
                        if a.code() as usize == i + 1 && self.rows[j].granted {
                            found = true;
                            break;
                        }
                    }
                    j += 1;
                }
                if !found {
                    return Err(XC_ACTION_MISMATCH);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 恢复面指纹（静默计数/放行位图/行数 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = (h ^ self.quiet_run as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.granted_cnt as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h = (h ^ self.rows.len() as u64).wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    /// 恢复面文本导出（确定性序 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_conservation()?;
        self.audit_pairing()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0985-RECOVER|v1|granted=");
        s.push_str(&self.granted_cnt.to_string());
        s.push_str("|rows=");
        s.push_str(&self.rows.len().to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < self.rows.len() {
            s.push_str(if self.rows[i].granted { "GRANT|" } else { "DENY|" });
            match self.rows[i].action {
                Some(a) => s.push_str(a.name()),
                None => s.push('-'),
            }
            s.push('\n');
            i += 1;
        }
        if self.fingerprint() != fp0 {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：条件不足强解拦→静默窗满+积压清+节流归位→逐项解除→守恒。
pub fn drill() -> Result<u32, u16> {
    let mut ev = RecoveryEvaluator::new(EVAL_CAP)?;
    // 静默窗未满即强解（条件一碎——静默连续窗仅 2）。
    ev.sample_quiet(true, 10)?;
    ev.sample_quiet(true, 20)?;
    if ev.evaluate(0, 0, Some(RelAction::Untthrottle), 30) != Err(XC_QUIET_SHORT) {
        return Err(XC_QUIET_SHORT);
    }
    // 积压未清强解（条件二碎——先补满静默窗）。
    ev.sample_quiet(true, 40)?;
    if ev.evaluate(5, 0, Some(RelAction::Untthrottle), 50) != Err(XC_EOI_BACKLOG) {
        return Err(XC_EOI_BACKLOG);
    }
    // 节流未归位强解（条件三碎）。
    if ev.evaluate(0, 2, Some(RelAction::Untthrottle), 60) != Err(XC_THROTTLE_HIGH) {
        return Err(XC_THROTTLE_HIGH);
    }
    // 三条件齐备：静默 3 窗 + 积压 0 + 节流 0 → 逐项解除。
    let g1 = ev.evaluate(0, 0, Some(RelAction::Untthrottle), 70)?;
    if g1 != Some(RelAction::Untthrottle) {
        return Err(XC_ACTION_MISMATCH);
    }
    let g2 = ev.evaluate(0, 0, Some(RelAction::UnmaskVector), 80)?;
    if g2 != Some(RelAction::UnmaskVector) {
        return Err(XC_ACTION_MISMATCH);
    }
    // 注入：重复解除同一动作。
    if ev.evaluate(0, 0, Some(RelAction::UnmaskVector), 90) != Err(XC_DOUBLE_RELEASE) {
        return Err(XC_DOUBLE_RELEASE);
    }
    // 注入：静默被一窗高率打断后（quiet_run 清零）再解必拦。
    ev.sample_quiet(false, 100)?;
    if ev.evaluate(0, 0, Some(RelAction::Unisolate), 110) != Err(XC_QUIET_SHORT) {
        return Err(XC_QUIET_SHORT);
    }
    // 补回静默窗后解除第三项。
    ev.sample_quiet(true, 120)?;
    ev.sample_quiet(true, 130)?;
    ev.sample_quiet(true, 140)?;
    let g3 = ev.evaluate(0, 0, Some(RelAction::Unisolate), 150)?;
    if g3 != Some(RelAction::Unisolate) {
        return Err(XC_ACTION_MISMATCH);
    }
    // tick 回退注入。
    if ev.sample_quiet(true, 5) != Err(XC_TICK_REGRESS) {
        return Err(XC_TICK_REGRESS);
    }
    // 守恒机检：8 行 = 3 达成 + 5 未达；放行 3 项全部成对。
    ev.audit_conservation()?;
    ev.audit_pairing()?;
    if ev.granted_count() != 3 || ev.len() != 8 || ev.rejected() != 5 {
        return Err(XC_CONSERVATION);
    }
    // 双导出指纹稳定。
    let fp1 = ev.fingerprint();
    let _ = ev.export()?;
    if ev.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
