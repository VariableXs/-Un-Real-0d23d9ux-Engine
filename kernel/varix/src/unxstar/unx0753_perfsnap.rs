//! UNX-F0753 · 移交包性能基线快照（冷启动耗时账快照） · 诊断段 0x5d20
//! 判据原文摘录：UNX-F0753-J1（承接 F0751-J3）性能快照账齐（T0–T3 四锚点
//! 三机终版 + 阈值判定 ≤3,000ms 凭证指针）且与 F0679 报告一致 100%；
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 四锚点写死：T0 引导入口 / T1 内核启动 / T2 初始化完成 / T3 用户态就绪，
//!    锚点时间单调递增（T0<T1<T2<T3），倒序即快照损坏。
//! 2. 三机终版：三台机器各一组四锚点，逐机判 ≤3,000ms（阈值写死），
//!    三机全过才算基线成立。
//! 3. 凭证指针：每机判定挂报告凭证指针（F0679 报告行号），指针可解引
//!    才算判定有据，缺凭证即判定无效。
//! 4. 与 F0679 报告一致 100%：快照锚点值与报告账行逐机逐锚点双向核对。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术（毫秒整数），
//!    确定性。诊断段 0x5d20 为本单独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5d20;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_ANCHOR_ORDER: u16 = SEG_BASE + 1; // 锚点非单调递增
const E_ANCHOR_ZERO: u16 = SEG_BASE + 2; // 锚点时间为 0（未采集）
const E_MACH_DUP: u16 = SEG_BASE + 3; // 机器号重复登记
const E_MACH_BAD: u16 = SEG_BASE + 4; // 机器号越界（1..=3）
const E_MACH_LACK: u16 = SEG_BASE + 5; // 三机未齐
const E_THRESHOLD: u16 = SEG_BASE + 6; // 超 3,000ms 阈值（判定不过）
const E_PTR_DANGLE: u16 = SEG_BASE + 7; // 凭证指针不可解引
const E_REPORT_MISMATCH: u16 = SEG_BASE + 8; // 与 F0679 报告不一致
const E_JUDGE_TWICE: u16 = SEG_BASE + 9; // 同机重复判定
const E_BAL: u16 = SEG_BASE + 10; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 11; // 快照已封存，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 12; // 快照账为空
const E_REPORT_EMPTY: u16 = SEG_BASE + 13; // 报告账为空（无从一致核）
const E_DELTA_BAD: u16 = SEG_BASE + 14; // 分段耗时为 0（锚点重复）

/// 三机数。
pub const MACHINES: usize = 3;

/// 四锚点号。
pub const A_T0: u8 = 0; // 引导入口
pub const A_T1: u8 = 1; // 内核启动
pub const A_T2: u8 = 2; // 初始化完成
pub const A_T3: u8 = 3; // 用户态就绪

/// 阈值：≤3,000ms（判据原文口径，写死）。
pub const THRESHOLD_MS: u64 = 3_000;

/// 报告账行数（F0679 对侧凭证账定容）。
pub const MAX_REPORT_ROWS: usize = 12;

// ── FNV-1a 64（封存哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_ANCHOR_ORDER => "锚点非单调：T0<T1<T2<T3 必须成立",
        E_ANCHOR_ZERO => "锚点时间为 0：先采集再登记",
        E_MACH_DUP => "机器号重复：三机各一组",
        E_MACH_BAD => "机器号越界：1..=3",
        E_MACH_LACK => "三机未齐：先补齐再判定",
        E_THRESHOLD => "超 3,000ms：冷启动超阈值，基线不成立",
        E_PTR_DANGLE => "凭证指针不可解引：报告行缺失",
        E_REPORT_MISMATCH => "与 F0679 报告不一致：逐锚点排查",
        E_JUDGE_TWICE => "同机重复判定",
        E_BAL => "守恒破裂：判定数与机器数不平",
        E_SEALED => "快照已封存：封存后只读",
        E_EMPTY => "快照账为空",
        E_REPORT_EMPTY => "报告账为空：先装载 F0679 报告行",
        E_DELTA_BAD => "分段耗时为 0：锚点时间重复",
        _ => "未知诊断码",
    }
}

// ── 锚点组 ───────────────────────────────────────────────────
/// 一台机器的四锚点时间（毫秒，整数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnchorSet {
    /// T0 引导入口。
    pub t0: u64,
    /// T1 内核启动。
    pub t1: u64,
    /// T2 初始化完成。
    pub t2: u64,
    /// T3 用户态就绪。
    pub t3: u64,
}

impl AnchorSet {
    /// 单调性与非零核：0<T0<T1<T2<T3。
    pub fn sane(&self) -> bool {
        self.t0 > 0 && self.t0 < self.t1 && self.t1 < self.t2 && self.t2 < self.t3
    }

    /// 冷启动总耗时（T3-T0）。
    pub fn total_ms(&self) -> u64 {
        self.t3 - self.t0
    }

    /// 分段耗时三元组：(T1-T0 引导, T2-T1 内核, T3-T2 用户态)。
    pub fn deltas(&self) -> (u64, u64, u64) {
        (self.t1 - self.t0, self.t2 - self.t1, self.t3 - self.t2)
    }

    /// 阈值判定：总耗时 ≤3000ms。
    pub fn within_threshold(&self) -> bool {
        self.total_ms() <= THRESHOLD_MS
    }
}

// ── 报告凭证行（F0679 对侧）──────────────────────────────────
/// F0679 报告的一行：机器号 + 四锚点值（报告侧快照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportRow {
    /// 机器号（1..=3）。
    pub machine: u8,
    /// 报告侧 T0。
    pub t0: u64,
    /// 报告侧 T1。
    pub t1: u64,
    /// 报告侧 T2。
    pub t2: u64,
    /// 报告侧 T3。
    pub t3: u64,
    /// 报告行号（凭证指针目标）。
    pub row_no: u32,
}

// ── 机判定行 ─────────────────────────────────────────────────
/// 一台机器的判定行：锚点组 + 判定 + 凭证指针。
#[derive(Clone, Debug)]
pub struct MachineJudge {
    /// 机器号。
    pub machine: u8,
    /// 锚点组（快照侧终版）。
    pub anchors: AnchorSet,
    /// 判定结论（true=过阈值）。
    pub passed: bool,
    /// 凭证指针（F0679 报告行号）。
    pub evidence: u32,
}

// ── 性能基线快照（主体）──────────────────────────────────────
/// 移交包性能基线快照：三机锚点 → 阈值判定 → 报告一致核 → 封存。
pub struct PerfSnapshot {
    judges: [Option<MachineJudge>; MACHINES],
    mach_count: usize,
    /// F0679 报告账。
    report: [Option<ReportRow>; MAX_REPORT_ROWS],
    report_count: usize,
    /// 判定数与过数（守恒分子）。
    judged: u32,
    passed: u32,
    /// 是否封存。
    sealed: bool,
    /// 封存哈希。
    sealed_hash: u64,
}

impl PerfSnapshot {
    /// 开快照。
    pub fn new() -> PerfSnapshot {
        PerfSnapshot {
            judges: core::array::from_fn(|_| None),
            mach_count: 0,
            report: [None; MAX_REPORT_ROWS],
            report_count: 0,
            judged: 0,
            passed: 0,
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 装载 F0679 报告行。
    pub fn put_report_row(&mut self, row: ReportRow) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if row.machine == 0 || row.machine as usize > MACHINES {
            return Err(E_MACH_BAD);
        }
        if self.report_count >= MAX_REPORT_ROWS {
            return Err(E_BAL);
        }
        // 同机器号报告行查重。
        let mut i = 0usize;
        while i < self.report_count {
            if let Some(r) = &self.report[i] {
                if r.machine == row.machine {
                    return Err(E_MACH_DUP);
                }
            }
            i += 1;
        }
        self.report[self.report_count] = Some(row);
        self.report_count += 1;
        Ok(())
    }

    /// 登记锚点组（快照侧终版）。
    pub fn put_anchors(&mut self, machine: u8, a: AnchorSet) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if machine == 0 || machine as usize > MACHINES {
            return Err(E_MACH_BAD);
        }
        if !a.sane() {
            return Err(E_ANCHOR_ORDER);
        }
        let idx = (machine - 1) as usize;
        if self.judges[idx].is_some() {
            return Err(E_MACH_DUP);
        }
        // 分段耗时全非零（锚点不重复）。
        let (d1, d2, d3) = a.deltas();
        if d1 == 0 || d2 == 0 || d3 == 0 {
            return Err(E_DELTA_BAD);
        }
        self.judges[idx] = Some(MachineJudge {
            machine,
            anchors: a,
            passed: false,
            evidence: 0,
        });
        self.mach_count += 1;
        Ok(())
    }

    /// 判定一台机器：三机齐 + 凭证可解引 + 报告一致 → 落判定。
    pub fn judge_machine(&mut self, machine: u8) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if machine == 0 || machine as usize > MACHINES {
            return Err(E_MACH_BAD);
        }
        if self.report_count == 0 {
            return Err(E_REPORT_EMPTY);
        }
        let idx = (machine - 1) as usize;
        let j = self.judges[idx].as_mut().ok_or(E_MACH_LACK)?;
        if j.passed || j.evidence != 0 {
            return Err(E_JUDGE_TWICE);
        }
        // 凭证指针解引：报告账必须存在本机行。
        let mut rep: Option<ReportRow> = None;
        let mut i = 0usize;
        while i < self.report_count {
            if let Some(r) = &self.report[i] {
                if r.machine == machine {
                    rep = Some(*r);
                }
            }
            i += 1;
        }
        let r = match rep {
            Some(r) => r,
            None => return Err(E_PTR_DANGLE),
        };
        // 与 F0679 报告一致 100%：四锚点逐位相等。
        if r.t0 != j.anchors.t0
            || r.t1 != j.anchors.t1
            || r.t2 != j.anchors.t2
            || r.t3 != j.anchors.t3
        {
            return Err(E_REPORT_MISMATCH);
        }
        j.evidence = r.row_no;
        j.passed = j.anchors.within_threshold();
        self.judged += 1;
        if j.passed {
            self.passed += 1;
        }
        Ok(j.passed)
    }

    /// 守恒恒等式：判定数 ≤ 机器数，过数 ≤ 判定数。
    pub fn conserved(&self) -> bool {
        self.judged <= self.mach_count as u32 && self.passed <= self.judged
    }

    /// 基线成立断言：三机齐 + 三判定 + 三全过 + 守恒。
    pub fn baseline_assert(&self) -> Result<(), u16> {
        if self.mach_count != MACHINES {
            return Err(E_MACH_LACK);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        if self.judged != MACHINES as u32 {
            return Err(E_JUDGE_TWICE);
        }
        if self.passed != MACHINES as u32 {
            return Err(E_THRESHOLD);
        }
        let mut i = 0usize;
        while i < MACHINES {
            match &self.judges[i] {
                Some(j) => {
                    if !j.passed || j.evidence == 0 {
                        return Err(E_PTR_DANGLE);
                    }
                }
                None => return Err(E_MACH_LACK),
            }
            i += 1;
        }
        Ok(())
    }

    /// 三机中位值复算：总耗时排序取中位数（整数毫秒）。
    pub fn median_total_ms(&self) -> Result<u64, u16> {
        if self.mach_count != MACHINES {
            return Err(E_MACH_LACK);
        }
        let mut totals = [0u64; MACHINES];
        let mut i = 0usize;
        while i < MACHINES {
            totals[i] = self.judges[i].as_ref().ok_or(E_MACH_LACK)?.anchors.total_ms();
            i += 1;
        }
        // 三元小排序。
        if totals[0] > totals[1] {
            totals.swap(0, 1);
        }
        if totals[1] > totals[2] {
            totals.swap(1, 2);
        }
        if totals[0] > totals[1] {
            totals.swap(0, 1);
        }
        Ok(totals[1])
    }

    /// 中位值阈值判定复现。
    pub fn median_assert(&self) -> Result<(), u16> {
        let m = self.median_total_ms()?;
        if m > THRESHOLD_MS {
            return Err(E_THRESHOLD);
        }
        Ok(())
    }

    /// 封存：基线断言 + 中位复现 → 聚合哈希。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.baseline_assert()?;
        self.median_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < MACHINES {
            if let Some(j) = &self.judges[i] {
                buf.push(j.machine);
                buf.extend_from_slice(j.anchors.t0.to_le_bytes().as_slice());
                buf.extend_from_slice(j.anchors.t1.to_le_bytes().as_slice());
                buf.extend_from_slice(j.anchors.t2.to_le_bytes().as_slice());
                buf.extend_from_slice(j.anchors.t3.to_le_bytes().as_slice());
                buf.push(j.passed as u8);
                buf.extend_from_slice(j.evidence.to_le_bytes().as_slice());
            }
            i += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 封存哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 机判定行渲染。
    pub fn judge_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 冷启动性能基线 · T0T3 四锚点三机 ===".to_string());
        let mut i = 0usize;
        while i < self.mach_count {
            if let Some(j) = &self.judges[i] {
                let (d1, d2, d3) = j.anchors.deltas();
                let mut line = String::from("M");
                line.push_str(&itoa(j.machine as u64));
                line.push_str("|总 ");
                line.push_str(&itoa(j.anchors.total_ms()));
                line.push_str("ms|引导 ");
                line.push_str(&itoa(d1));
                line.push_str("|内核 ");
                line.push_str(&itoa(d2));
                line.push_str("|用户 ");
                line.push_str(&itoa(d3));
                line.push_str("|");
                line.push_str(if j.passed { "过" } else { "超" });
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 锚点中文名（锚点号 → 名）。
    pub fn anchor_name(no: u8) -> Result<&'static str, u16> {
        match no {
            A_T0 => Ok("引导入口"),
            A_T1 => Ok("内核启动"),
            A_T2 => Ok("初始化完成"),
            A_T3 => Ok("用户态就绪"),
            _ => Err(E_ANCHOR_ORDER),
        }
    }

    /// 最慢机器号（总耗时最大者；并列取小号，确定性）。
    pub fn slowest_machine(&self) -> Result<u8, u16> {
        if self.mach_count == 0 {
            return Err(E_EMPTY);
        }
        let mut best: u8 = 0;
        let mut best_ms: u64 = 0;
        let mut i = 0usize;
        while i < self.mach_count {
            if let Some(j) = &self.judges[i] {
                let ms = j.anchors.total_ms();
                if ms > best_ms {
                    best_ms = ms;
                    best = j.machine;
                }
            }
            i += 1;
        }
        if best == 0 {
            return Err(E_EMPTY);
        }
        Ok(best)
    }

    /// 凭证指针读取（machine 1..=3 → 报告行号）。
    pub fn evidence_of(&self, machine: u8) -> Result<u32, u16> {
        if machine == 0 || machine as usize > MACHINES {
            return Err(E_MACH_BAD);
        }
        match &self.judges[(machine - 1) as usize] {
            Some(j) if j.evidence != 0 => Ok(j.evidence),
            _ => Err(E_PTR_DANGLE),
        }
    }

    /// 分段预算核对：引导段 ≤60%、内核段 ≤45%、用户态段 ≤15%
    /// （写死预算，来源 F0679 定版基线的分段占比上限）。
    pub fn phase_budget_ok(&self, machine: u8) -> Result<bool, u16> {
        if machine == 0 || machine as usize > MACHINES {
            return Err(E_MACH_BAD);
        }
        let j = self.judges[(machine - 1) as usize].as_ref().ok_or(E_MACH_LACK)?;
        let total = j.anchors.total_ms();
        let (d1, d2, d3) = j.anchors.deltas();
        // 千分比整数化比较：d*1000 <= budget*total。
        Ok(d1 * 1000 <= 600 * total
            && d2 * 1000 <= 450 * total
            && d3 * 1000 <= 150 * total)
    }

    /// 分段预算行渲染（三机逐行）。
    pub fn phase_lines(&self) -> Result<Vec<String>, u16> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 分段预算核对（60%/45%/15%） ===".to_string());
        let mut m = 1u8;
        while m as usize <= MACHINES {
            let ok = self.phase_budget_ok(m)?;
            let mut line = String::from("M");
            line.push_str(&itoa(m as u64));
            line.push_str("|预算=");
            line.push_str(if ok { "过" } else { "超" });
            lines.push(line);
            m += 1;
        }
        Ok(lines)
    }
}

/// 极简无符号转十进制。
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 标准三机样本（与 F0679 定版一致，总耗时 ≤3000）──────────────
/// 构建标准三机锚点组与报告行（定版值：240/1600/1100/2850 等）。
pub fn standard_anchors() -> [AnchorSet; MACHINES] {
    [
        AnchorSet { t0: 40, t1: 1600, t2: 2700, t3: 2850 },
        AnchorSet { t0: 45, t1: 1650, t2: 2760, t3: 2910 },
        AnchorSet { t0: 42, t1: 1620, t2: 2720, t3: 2880 },
    ]
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：三机判定 + 报告一致 100% + 中位复现 + 封存 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let anchors = standard_anchors();
    let mut ps = PerfSnapshot::new();
    // 报告行与锚点组同源装载（一致样本）。
    let mut m = 0usize;
    while m < MACHINES {
        ps.put_report_row(ReportRow {
            machine: (m + 1) as u8,
            t0: anchors[m].t0,
            t1: anchors[m].t1,
            t2: anchors[m].t2,
            t3: anchors[m].t3,
            row_no: 100 + m as u32,
        })?;
        ps.put_anchors((m + 1) as u8, anchors[m])?;
        m += 1;
    }
    // 逐机判定。
    let mut n = 1u8;
    while n as usize <= MACHINES {
        if !ps.judge_machine(n)? {
            return Err(SEG_BASE);
        }
        n += 1;
    }
    ps.baseline_assert()?;
    ps.median_assert()?;
    let med = ps.median_total_ms()?;
    if med == 0 || med > THRESHOLD_MS {
        return Err(SEG_BASE);
    }
    let h = ps.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 取数口：最慢机、凭证、分段预算。
    if ps.slowest_machine()? != 2 {
        return Err(SEG_BASE);
    }
    if ps.evidence_of(1)? != 100 {
        return Err(SEG_BASE);
    }
    if PerfSnapshot::anchor_name(A_T3)? != "用户态就绪" {
        return Err(SEG_BASE);
    }
    let pl = ps.phase_lines()?;
    if pl.len() != 4 {
        return Err(SEG_BASE);
    }
    // 分段预算：三机必须全过（60%/45%/15% 口径）。
    let mut m = 1u8;
    while m as usize <= MACHINES {
        if !ps.phase_budget_ok(m)? {
            return Err(SEG_BASE);
        }
        m += 1;
    }
    // 封存后写拒绝。
    match ps.put_anchors(1, anchors[0]) {
        Err(c) if c == E_SEALED || c == E_MACH_DUP => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：报告不一致。
    let mut ps2 = PerfSnapshot::new();
    ps2.put_report_row(ReportRow {
        machine: 1,
        t0: 40,
        t1: 1600,
        t2: 2700,
        t3: 2850,
        row_no: 1,
    })?;
    ps2.put_anchors(1, AnchorSet { t0: 40, t1: 1610, t2: 2700, t3: 2850 })?;
    match ps2.judge_machine(1) {
        Err(c) if c == E_REPORT_MISMATCH => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：锚点倒序。
    match ps2.put_anchors(2, AnchorSet { t0: 99, t1: 50, t2: 60, t3: 70 }) {
        Err(c) if c == E_ANCHOR_ORDER => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：超阈值。
    let mut ps3 = PerfSnapshot::new();
    let slow = AnchorSet { t0: 10, t1: 1500, t2: 2600, t3: 3500 };
    ps3.put_report_row(ReportRow { machine: 1, t0: slow.t0, t1: slow.t1, t2: slow.t2, t3: slow.t3, row_no: 2 })?;
    ps3.put_anchors(1, slow)?;
    match ps3.judge_machine(1) {
        Ok(false) => {}
        _ => return Err(SEG_BASE),
    }
    match ps3.baseline_assert() {
        Err(c) if c == E_MACH_LACK || c == E_THRESHOLD => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
