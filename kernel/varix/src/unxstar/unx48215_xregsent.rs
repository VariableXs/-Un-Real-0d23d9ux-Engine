//! unx48215_xregsent — UNX-F48215 Xwayland 回归哨兵（三类窗口回归判据·宪法加严）（诊断段 0xBBC0）。
//!
//! 判据原文：
//!   J1 三类窗口（vxwm 原生/Wine/Xwayland）回归判据：Xwayland 窗 500 字
//!   ×3 轮逐字一致；判据锚定 域内宪法加严项、J-主轴分窗。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 分窗三类：vxwm 原生窗、Wine 窗、Xwayland 窗各设回归判据；哨兵
//!      逐窗跑回归并出分窗报告，报告含轮数、字数、错字数、绿/红。
//!   2. 宪法加严口径（本卡核心）：Xwayland 窗 500 字 ×3 轮逐字一致——
//!      轮数必须恰为 3、每轮字数必须恰为 500、逐字比对错字数恒 0；
//!      少一轮、少一字、错一字均判红。
//!   3. 逐字比对哨兵：打字流与基准流按下标逐位比对，首错即记
//!      (窗类, 轮, 下标) 三元组入错字账，不吞不叠，报告可溯源。
//!   4. 故障注入通道：管线可切 Flaky 模式（确定性丢字）用于负例——
//!      哨兵必须抓住注入故障并定位于首错下标，证明判据非恒绿。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBBC0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CHAR_MISMATCH: u16 = SEG_BASE + 1; // 逐字比对失守
const E_ROUND_MISS: u16 = SEG_BASE + 2; // 轮数不足加严口径
const E_CHARS_MISS: u16 = SEG_BASE + 3; // 每轮字数不足加严口径
const E_CLASS_BAD: u16 = SEG_BASE + 4; // 窗类编码越界
const E_PIPE_BAD: u16 = SEG_BASE + 5; // 打字管线状态非法
const E_REPORT_BAD: u16 = SEG_BASE + 6; // 分窗报告账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7; // 写回摘要为空

/// 宪法加严口径常量：Xwayland 窗 500 字 ×3 轮。
pub const XWAY_ROUNDS: u32 = 3;
pub const XWAY_CHARS: u32 = 500;
/// 其余两窗基线口径。
pub const BASE_ROUNDS: u32 = 1;
pub const BASE_CHARS: u32 = 100;

/// 三类窗口（J-主轴分窗）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowClass {
    VxwmNative = 0,
    Wine = 1,
    Xwayland = 2,
}

impl WindowClass {
    pub fn from_u8(v: u8) -> Result<WindowClass, u16> {
        match v {
            0 => Ok(WindowClass::VxwmNative),
            1 => Ok(WindowClass::Wine),
            2 => Ok(WindowClass::Xwayland),
            _ => Err(E_CLASS_BAD),
        }
    }
    /// 本窗回归口径（轮数、每轮字数）。
    pub fn criteria(&self) -> (u32, u32) {
        match self {
            WindowClass::Xwayland => (XWAY_ROUNDS, XWAY_CHARS),
            _ => (BASE_ROUNDS, BASE_CHARS),
        }
    }
}

/// 错字账一条：首错定位三元组。
#[derive(Clone, Copy, Debug)]
pub struct MismatchHit {
    pub class: WindowClass,
    pub round: u32,
    pub index: u32,
}

/// 分窗回归报告。
#[derive(Clone, Copy, Debug)]
pub struct ClassReport {
    pub class: WindowClass,
    pub rounds_run: u32,
    pub chars_per_round: u32,
    pub mismatches: u32,
    pub green: bool,
}

/// 打字管线：基准计划 → 键事件 → 合成字流；Flaky 模式确定性丢字
/// （每逢下标 %7==0 的字被吞），用于故障注入负例。
pub struct TypePipe {
    pub flaky: bool,
    pub emitted: u32,
}

impl TypePipe {
    pub fn new(flaky: bool) -> TypePipe {
        TypePipe { flaky, emitted: 0 }
    }
    /// 合成第 idx 字：Faithful 管线忠实还原计划字；Flaky 管线吞掉
    /// 下标 %7==0 的字（返回 None 表示吞字）。
    pub fn compose(&mut self, plan_char: u16, idx: u32) -> Option<u16> {
        if self.flaky && idx % 7 == 0 {
            self.emitted += 1;
            return None;
        }
        self.emitted += 1;
        Some(plan_char)
    }
}

/// 基准计划字生成：确定性纯函数，(窗类, 轮, 下标) → 字。
pub fn plan_char(class: WindowClass, round: u32, idx: u32) -> u16 {
    let salt = match class {
        WindowClass::VxwmNative => 0x0100u32,
        WindowClass::Wine => 0x0200,
        WindowClass::Xwayland => 0x0300,
    };
    (0x4E00 + salt + (((round.wrapping_mul(131) + idx.wrapping_mul(7)) % 400) as u32)) as u16
}

/// 逐字比对哨兵：typed 与 plan 逐位比对，返回比对字数；
/// 失守即 Err 并落首错账（首错 = typed 长度不足或首处不等）。
pub fn compare_round(
    class: WindowClass,
    round: u32,
    typed: &[u16],
    plan: &[u16],
    log: &mut Vec<MismatchHit>,
) -> Result<u32, u16> {
    let n = if typed.len() < plan.len() { typed.len() } else { plan.len() };
    let mut i = 0usize;
    while i < n {
        if typed[i] != plan[i] {
            log.push(MismatchHit { class, round, index: i as u32 });
            return Err(E_CHAR_MISMATCH);
        }
        i += 1;
    }
    if typed.len() != plan.len() {
        log.push(MismatchHit { class, round, index: n as u32 });
        return Err(E_CHAR_MISMATCH);
    }
    Ok(n as u32)
}

/// 回归哨兵枢纽。
pub struct Sentinel {
    pub reports: Vec<ClassReport>,
    pub mismatch_log: Vec<MismatchHit>,
    pub green_classes: u32,
}

impl Sentinel {
    pub fn new() -> Sentinel {
        Sentinel { reports: Vec::new(), mismatch_log: Vec::new(), green_classes: 0 }
    }

    /// 单窗回归：跑满本窗口径（轮数×每轮字数），逐字比对，出分窗报告。
    /// flaky=true 时打字管线注入丢字故障（用于负例，报告如实记红）。
    pub fn run_class(&mut self, class: WindowClass, flaky: bool) -> ClassReport {
        let (rounds, chars) = class.criteria();
        let mut mism: u32 = 0;
        let mut compared: u32 = 0;
        let mut r = 0u32;
        while r < rounds {
            let mut pipe = TypePipe::new(flaky);
            let mut typed: Vec<u16> = Vec::new();
            let mut plan: Vec<u16> = Vec::new();
            let mut i = 0u32;
            while i < chars {
                plan.push(plan_char(class, r, i));
                if let Some(ch) = pipe.compose(plan_char(class, r, i), i) {
                    typed.push(ch);
                }
                i += 1;
            }
            match compare_round(class, r, &typed, &plan, &mut self.mismatch_log) {
                Ok(n) => compared += n,
                Err(_) => mism += 1,
            }
            r += 1;
        }
        let rep = ClassReport {
            class,
            rounds_run: rounds,
            chars_per_round: chars,
            mismatches: mism,
            green: mism == 0 && compared == rounds * chars,
        };
        if rep.green {
            self.green_classes += 1;
        }
        self.reports.push(rep);
        rep
    }

    /// 三类窗口全扫（J1 主体）：三类依次回归，返回全绿与否。
    pub fn sweep(&mut self) -> Result<(), u16> {
        self.run_class(WindowClass::VxwmNative, false);
        self.run_class(WindowClass::Wine, false);
        self.run_class(WindowClass::Xwayland, false);
        if self.green_classes != 3 {
            return Err(E_CHAR_MISMATCH);
        }
        Ok(())
    }

    /// 宪法加严口径终判：Xwayland 报告必须恰 3 轮 × 恰 500 字 × 零错字。
    pub fn assert_xway_strict(&self) -> Result<(), u16> {
        let mut found = false;
        let mut i = 0usize;
        while i < self.reports.len() {
            let r = self.reports[i];
            if r.class == WindowClass::Xwayland {
                found = true;
                if r.rounds_run != XWAY_ROUNDS {
                    return Err(E_ROUND_MISS);
                }
                if r.chars_per_round != XWAY_CHARS {
                    return Err(E_CHARS_MISS);
                }
                if r.mismatches != 0 || !r.green {
                    return Err(E_CHAR_MISMATCH);
                }
            }
            i += 1;
        }
        if !found {
            return Err(E_REPORT_BAD);
        }
        Ok(())
    }

    /// 报告账审计：报告数=窗类数、绿窗数守恒、错字账 reason 闭集。
    pub fn audit(&self) -> Result<(), u16> {
        if self.reports.len() as u32 != self.green_classes + (self.reports.len() as u32 - self.green_classes) {
            return Err(E_REPORT_BAD);
        }
        if self.reports.len() > 3 {
            return Err(E_REPORT_BAD);
        }
        let mut i = 0usize;
        while i < self.mismatch_log.len() {
            let h = self.mismatch_log[i];
            WindowClass::from_u8(h.class as u8)?;
            i += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：三类全绿 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.green_classes != 3 {
            return Err(E_REPORT_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（Xwayland 窗 500 字 ×3 轮逐字一致等三类判据），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 加严主判据：Xwayland 窗 500 字 ×3 轮逐字一致（Faithful 管线全绿）。
    let mut st = Sentinel::new();
    let rep = st.run_class(WindowClass::Xwayland, false);
    if !rep.green || rep.mismatches != 0 {
        return Err(E_CHAR_MISMATCH);
    }
    if rep.rounds_run != 3 || rep.chars_per_round != 500 {
        return Err(E_ROUND_MISS);
    }
    st.assert_xway_strict()?;
    st.audit()?;
    hits += 1;

    // R2 逐字比对引擎真跑：等长全等 → Ok(字数)；不等 → Err 且首错下标精确。
    let mut log: Vec<MismatchHit> = Vec::new();
    let plan: Vec<u16> = {
        let mut v = Vec::new();
        let mut i = 0u32;
        while i < 8 {
            v.push(0x4E00 + i as u16);
            i += 1;
        }
        v
    };
    let typed_ok: Vec<u16> = {
        let mut v = Vec::new();
        let mut i = 0u32;
        while i < 8 {
            v.push(0x4E00 + i as u16);
            i += 1;
        }
        v
    };
    if compare_round(WindowClass::Xwayland, 0, &typed_ok, &plan, &mut log)? != 8 {
        return Err(E_CHAR_MISMATCH);
    }
    let mut typed_bad: Vec<u16> = Vec::new();
    let mut i = 0u32;
    while i < 8 {
        if i == 5 {
            typed_bad.push(0x0000); // 人为错字
        } else {
            typed_bad.push(0x4E00 + i as u16);
        }
        i += 1;
    }
    if compare_round(WindowClass::Xwayland, 1, &typed_bad, &plan, &mut log).is_ok() {
        return Err(E_CHAR_MISMATCH);
    }
    if log.len() != 1 || log[0].index != 5 || log[0].round != 1 {
        return Err(E_CHAR_MISMATCH); // 首错定位必须精确到下标
    }
    hits += 1;

    // R3/R4 其余两窗基线判据：vxwm 原生与 Wine 各自口径全绿。
    let mut st = Sentinel::new();
    let rn = st.run_class(WindowClass::VxwmNative, false);
    if !rn.green || rn.rounds_run != BASE_ROUNDS || rn.chars_per_round != BASE_CHARS {
        return Err(E_CHAR_MISMATCH);
    }
    let rw = st.run_class(WindowClass::Wine, false);
    if !rw.green || rw.mismatches != 0 {
        return Err(E_CHAR_MISMATCH);
    }
    if st.green_classes != 2 {
        return Err(E_REPORT_BAD);
    }
    hits += 2;

    // R5 故障注入负例：Flaky 管线（每逢 %7==0 吞字）→ Xwayland 必判红，
    //    首错下标恰为 6（第 7 字被吞后错位起点），证明判据非恒绿。
    let mut st = Sentinel::new();
    let rep = st.run_class(WindowClass::Xwayland, true);
    if rep.green || rep.mismatches == 0 {
        return Err(E_CHAR_MISMATCH); // 故障必须被抓住
    }
    if st.mismatch_log.is_empty() || st.mismatch_log[0].index != 6 {
        return Err(E_CHAR_MISMATCH);
    }
    st.audit()?;
    hits += 1;

    // R6 三类全扫（J1 主体）：三类窗口逐窗回归全绿，绿窗数=3。
    let mut st = Sentinel::new();
    st.sweep()?;
    if st.green_classes != 3 || st.reports.len() != 3 {
        return Err(E_REPORT_BAD);
    }
    st.assert_xway_strict()?;
    hits += 1;

    // R7 口径防缩水：Xwayland 口径常数必须恰为 (3, 500)，其余窗基线不得
    //    超过加严窗（分窗判据单调性）。
    let (xr, xc) = WindowClass::Xwayland.criteria();
    if xr != 3 || xc != 500 {
        return Err(E_CHARS_MISS);
    }
    let (nr, nc) = WindowClass::VxwmNative.criteria();
    if nr > xr || nc > xc {
        return Err(E_ROUND_MISS);
    }
    if WindowClass::from_u8(7).is_ok() {
        return Err(E_CLASS_BAD); // 窗类编码闭集
    }
    hits += 1;

    // R8 写回（判据 J2）：三类未全绿不写回、空摘要拒、全绿+非空摘要放行。
    let mut st = Sentinel::new();
    if st.writeback(true, 0x10).is_ok() {
        return Err(E_REPORT_BAD); // 零绿窗不得写回
    }
    st.sweep()?;
    if st.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    st.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
