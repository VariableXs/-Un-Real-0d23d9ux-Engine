//! UNX-F0764 · 冷启动性能终核（≤3,000ms 三机复算） · 诊断段 0x5e40
//! 判据原文摘录：UNX-F0764-J1 终核过（T0–T3 四锚点三机中位值复算 ≤3,000ms
//! 判定再现）且与 F0661/F0679 一致 100%；完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。纯功能代码 ≥580 行。
//!
//! 设计纪律：
//! 1. 终核双报告互证：终核锚点组必须与 F0661（首测）与 F0679（复测）
//!    两个报告账逐机逐锚点一致，双源一致 100% 才算判定再现。
//! 2. 中位值复算：三机总耗时排序取中位数，中位值 ≤3,000ms 判定
//!    必须独立复算再现（不读旧结论，只算新数）。
//! 3. 锚点单调性终核：T0<T1<T2<T3 每机必查，分段耗时全非零。
//! 4. 判定再现链：首测结论 == 复测结论 == 终核结论（三判等值），
//!    结论漂移即终核拒绝。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5e40 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5e40;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_ANCHOR_ORDER: u16 = SEG_BASE + 1; // 锚点非单调
const E_ANCHOR_ZERO: u16 = SEG_BASE + 2; // 锚点时间为 0
const E_MACH_BAD: u16 = SEG_BASE + 3; // 机器号越界（1..=3）
const E_MACH_LACK: u16 = SEG_BASE + 4; // 三机未齐
const E_THRESHOLD: u16 = SEG_BASE + 5; // 中位值超 3,000ms
const E_R1_MISMATCH: u16 = SEG_BASE + 6; // 与 F0661 不一致
const E_R2_MISMATCH: u16 = SEG_BASE + 7; // 与 F0679 不一致
const E_JUDGE_DRIFT: u16 = SEG_BASE + 8; // 判定再现链断裂（三判不等值）
const E_BAL: u16 = SEG_BASE + 9; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 10; // 终核已封，拒绝再写
const E_EMPTY: u16 = SEG_BASE + 11; // 终核账为空
const E_REPORT_LACK: u16 = SEG_BASE + 12; // 双报告未装载
const E_DELTA_ZERO: u16 = SEG_BASE + 13; // 分段耗时为 0
const E_TWICE: u16 = SEG_BASE + 14; // 同机重复终核

/// 三机数。
pub const MACHINES: usize = 3;

/// 阈值：≤3,000ms。
pub const THRESHOLD_MS: u64 = 3_000;

/// 报告账定容（双报告 × 3 机 = 6 行，留容 8）。
pub const MAX_REPORT: usize = 8;

/// 报告源号。
pub const R_F0661: u8 = 1; // 首测报告
pub const R_F0679: u8 = 2; // 复测报告

// ── FNV-1a 64（终核哈希口径）──────────────────────────────────
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
        E_ANCHOR_ZERO => "锚点时间为 0：先采集再终核",
        E_MACH_BAD => "机器号越界：1..=3",
        E_MACH_LACK => "三机未齐：先补齐锚点组",
        E_THRESHOLD => "中位值超 3,000ms：终核不过",
        E_R1_MISMATCH => "与 F0661 不一致：首测锚点对不上",
        E_R2_MISMATCH => "与 F0679 不一致：复测锚点对不上",
        E_JUDGE_DRIFT => "判定再现链断裂：首测/复测/终核结论不等值",
        E_BAL => "守恒破裂：判定数与机器数不平",
        E_SEALED => "终核已封：封账后只读",
        E_EMPTY => "终核账为空",
        E_REPORT_LACK => "双报告未装载：先落 F0661/F0679 行",
        E_DELTA_ZERO => "分段耗时为 0：锚点时间重复",
        E_TWICE => "同机重复终核",
        _ => "未知诊断码",
    }
}

// ── 锚点组 ───────────────────────────────────────────────────
/// 四锚点时间组（毫秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchors {
    /// T0 引导入口。
    pub t0: u64,
    /// T1 内核启动。
    pub t1: u64,
    /// T2 初始化完成。
    pub t2: u64,
    /// T3 用户态就绪。
    pub t3: u64,
}

impl Anchors {
    /// 单调非零核。
    pub fn sane(&self) -> bool {
        self.t0 > 0 && self.t0 < self.t1 && self.t1 < self.t2 && self.t2 < self.t3
    }

    /// 总耗时。
    pub fn total(&self) -> u64 {
        self.t3 - self.t0
    }

    /// 分段三元组。
    pub fn deltas(&self) -> (u64, u64, u64) {
        (self.t1 - self.t0, self.t2 - self.t1, self.t3 - self.t2)
    }

    /// 阈值判定。
    pub fn pass(&self) -> bool {
        self.total() <= THRESHOLD_MS
    }

    /// 四锚点字节化（报告比对用）。
    pub fn quad(&self) -> (u64, u64, u64, u64) {
        (self.t0, self.t1, self.t2, self.t3)
    }
}

// ── 报告行 ───────────────────────────────────────────────────
/// 报告账一行：源号 + 机器号 + 四锚点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportLine {
    /// 报告源号（1=F0661 2=F0679）。
    pub src: u8,
    /// 机器号（1..=3）。
    pub machine: u8,
    /// 四锚点。
    pub a: Anchors,
}

// ── 终核账（主体）────────────────────────────────────────────
/// 冷启动性能终核账：双报告装载 → 锚点终核 → 中位复算 → 判定再现 → 封核。
pub struct BootFinal {
    /// 终核锚点组（三机）。
    finals: [Option<Anchors>; MACHINES],
    mach_count: usize,
    /// 双报告账。
    reports: [Option<ReportLine>; MAX_REPORT],
    report_count: usize,
    /// 判定再现链（首测/复测/终核三判，全 true 才等值）。
    chain_r1: Option<bool>,
    chain_r2: Option<bool>,
    chain_fin: Option<bool>,
    /// 是否封核。
    sealed: bool,
    /// 终核哈希。
    sealed_hash: u64,
}

impl BootFinal {
    /// 开终核账。
    pub fn new() -> BootFinal {
        BootFinal {
            finals: [None; MACHINES],
            mach_count: 0,
            reports: [None; MAX_REPORT],
            report_count: 0,
            chain_r1: None,
            chain_r2: None,
            chain_fin: None,
            sealed: false,
            sealed_hash: 0,
        }
    }

    /// 装载报告行（同源同机查重）。
    pub fn put_report(&mut self, line: ReportLine) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if line.src != R_F0661 && line.src != R_F0679 {
            return Err(E_REPORT_LACK);
        }
        if line.machine == 0 || line.machine as usize > MACHINES {
            return Err(E_MACH_BAD);
        }
        if !line.a.sane() {
            return Err(E_ANCHOR_ORDER);
        }
        if self.report_count >= MAX_REPORT {
            return Err(E_BAL);
        }
        let mut i = 0usize;
        while i < self.report_count {
            if let Some(r) = &self.reports[i] {
                if r.src == line.src && r.machine == line.machine {
                    return Err(E_TWICE);
                }
            }
            i += 1;
        }
        self.reports[self.report_count] = Some(line);
        self.report_count += 1;
        Ok(())
    }

    /// 报告行数。
    pub fn report_count(&self) -> usize {
        self.report_count
    }

    /// 双报告齐备核：F0661 三行 + F0679 三行 = 6 行。
    pub fn reports_complete(&self) -> bool {
        let mut n1 = 0u32;
        let mut n2 = 0u32;
        let mut i = 0usize;
        while i < self.report_count {
            if let Some(r) = &self.reports[i] {
                if r.src == R_F0661 {
                    n1 += 1;
                } else if r.src == R_F0679 {
                    n2 += 1;
                }
            }
            i += 1;
        }
        n1 == MACHINES as u32 && n2 == MACHINES as u32
    }

    /// 登记终核锚点组。
    pub fn put_final_anchors(&mut self, machine: u8, a: Anchors) -> Result<(), u16> {
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
        if self.finals[idx].is_some() {
            return Err(E_TWICE);
        }
        let (d1, d2, d3) = a.deltas();
        if d1 == 0 || d2 == 0 || d3 == 0 {
            return Err(E_DELTA_ZERO);
        }
        self.finals[idx] = Some(a);
        self.mach_count += 1;
        Ok(())
    }

    /// 机数。
    pub fn mach_count(&self) -> usize {
        self.mach_count
    }

    /// 单机双源一致核：终核锚点 == F0661 行 == F0679 行。
    pub fn dual_source_check(&mut self, machine: u8) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if machine == 0 || machine as usize > MACHINES {
            return Err(E_MACH_BAD);
        }
        if !self.reports_complete() {
            return Err(E_REPORT_LACK);
        }
        let fin = *self.finals[(machine - 1) as usize].as_ref().ok_or(E_MACH_LACK)?;
        let mut hit1 = false;
        let mut hit2 = false;
        let mut i = 0usize;
        while i < self.report_count {
            if let Some(r) = &self.reports[i] {
                if r.machine == machine {
                    if r.a.quad() == fin.quad() {
                        if r.src == R_F0661 {
                            hit1 = true;
                        } else {
                            hit2 = true;
                        }
                    }
                }
            }
            i += 1;
        }
        if !hit1 {
            return Err(E_R1_MISMATCH);
        }
        if !hit2 {
            return Err(E_R2_MISMATCH);
        }
        Ok(true)
    }

    /// 判定再现链推进：逐机双源核 + 链落账（三判全等值）。
    pub fn reproduce_judgments(&mut self) -> Result<(), u16> {
        if self.mach_count != MACHINES {
            return Err(E_MACH_LACK);
        }
        let mut m = 1u8;
        while m as usize <= MACHINES {
            self.dual_source_check(m)?;
            m += 1;
        }
        // 三判等值：首测行全过？复测行全过？终核全过？
        let j1 = self.report_all_pass(R_F0661)?;
        let j2 = self.report_all_pass(R_F0679)?;
        let j3 = self.final_all_pass()?;
        self.chain_r1 = Some(j1);
        self.chain_r2 = Some(j2);
        self.chain_fin = Some(j3);
        if !(j1 && j2 && j3) {
            return Err(E_JUDGE_DRIFT);
        }
        Ok(())
    }

    /// 指定报告源全机判定。
    fn report_all_pass(&self, src: u8) -> Result<bool, u16> {
        let mut all = true;
        let mut m = 1u8;
        while m as usize <= MACHINES {
            let mut found = false;
            let mut i = 0usize;
            while i < self.report_count {
                if let Some(r) = &self.reports[i] {
                    if r.src == src && r.machine == m {
                        found = true;
                        if !r.a.pass() {
                            all = false;
                        }
                    }
                }
                i += 1;
            }
            if !found {
                return Err(E_REPORT_LACK);
            }
            m += 1;
        }
        Ok(all)
    }

    /// 终核侧全机判定。
    fn final_all_pass(&self) -> Result<bool, u16> {
        let mut all = true;
        let mut i = 0usize;
        while i < MACHINES {
            let a = self.finals[i].ok_or(E_MACH_LACK)?;
            if !a.pass() {
                all = false;
            }
            i += 1;
        }
        Ok(all)
    }

    /// 中位值复算：三机总耗时取中位。
    pub fn median_total(&self) -> Result<u64, u16> {
        if self.mach_count != MACHINES {
            return Err(E_MACH_LACK);
        }
        let mut t = [0u64; MACHINES];
        let mut i = 0usize;
        while i < MACHINES {
            t[i] = self.finals[i].ok_or(E_MACH_LACK)?.total();
            i += 1;
        }
        if t[0] > t[1] {
            t.swap(0, 1);
        }
        if t[1] > t[2] {
            t.swap(1, 2);
        }
        if t[0] > t[1] {
            t.swap(0, 1);
        }
        Ok(t[1])
    }

    /// 中位值阈值判定再现。
    pub fn median_reproduce_assert(&self) -> Result<(), u16> {
        let m = self.median_total()?;
        if m > THRESHOLD_MS {
            return Err(E_THRESHOLD);
        }
        Ok(())
    }

    /// 守恒：三链齐 + 三机齐。
    pub fn conserved(&self) -> bool {
        self.chain_r1.is_some()
            && self.chain_r2.is_some()
            && self.chain_fin.is_some()
            && self.mach_count == MACHINES
    }

    /// 终核总断言：双报告齐 + 再现链 + 中位复现 + 守恒。
    pub fn final_assert(&self) -> Result<(), u16> {
        if self.mach_count == 0 {
            return Err(E_EMPTY);
        }
        if !self.reports_complete() {
            return Err(E_REPORT_LACK);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        self.median_reproduce_assert()?;
        Ok(())
    }

    /// 封核。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.final_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.median_total()?.to_le_bytes());
        let mut i = 0usize;
        while i < MACHINES {
            let a = self.finals[i].ok_or(E_MACH_LACK)?;
            buf.extend_from_slice(&a.total().to_le_bytes());
            i += 1;
        }
        self.sealed_hash = fnv1a64(&buf);
        self.sealed = true;
        Ok(self.sealed_hash)
    }

    /// 终核哈希。
    pub fn sealed_hash(&self) -> u64 {
        self.sealed_hash
    }

    /// 终核报告行。
    pub fn final_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 冷启动性能终核 · 三机复算 ===".to_string());
        let mut i = 0usize;
        while i < MACHINES {
            if let Some(a) = &self.finals[i] {
                let (d1, d2, d3) = a.deltas();
                let mut line = String::from("M");
                line.push_str(&itoa((i + 1) as u64));
                line.push_str("|总 ");
                line.push_str(&itoa(a.total()));
                line.push_str("ms|");
                line.push_str(if a.pass() { "过" } else { "超" });
                line.push_str("|分 ");
                line.push_str(&itoa(d1));
                line.push('/');
                line.push_str(&itoa(d2));
                line.push('/');
                line.push_str(&itoa(d3));
                lines.push(line);
            }
            i += 1;
        }
        lines.push("中位值: ".to_string() + &itoa(self.median_total().unwrap_or(0))
            + "ms（阈值 3000ms）");
        lines
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

// ── 定版样本（F0661/F0679/终核三方同值）─────────────────────────
/// 标准三机锚点（首测 = 复测 = 终核，中位 2880ms）。
pub fn standard_anchors() -> [Anchors; MACHINES] {
    [
        Anchors { t0: 40, t1: 1600, t2: 2700, t3: 2850 },
        Anchors { t0: 45, t1: 1650, t2: 2760, t3: 2910 },
        Anchors { t0: 42, t1: 1620, t2: 2720, t3: 2880 },
    ]
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：双报告一致 + 判定再现 + 中位复现 + 封核 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let anchors = standard_anchors();
    let mut bf = BootFinal::new();
    // 双报告同值装载。
    let mut src = R_F0661;
    while src <= R_F0679 {
        let mut m = 0usize;
        while m < MACHINES {
            bf.put_report(ReportLine {
                src,
                machine: (m + 1) as u8,
                a: anchors[m],
            })?;
            m += 1;
        }
        src += 1;
    }
    if !bf.reports_complete() || bf.report_count() != 6 {
        return Err(SEG_BASE);
    }
    // 终核锚点组。
    let mut m = 0usize;
    while m < MACHINES {
        bf.put_final_anchors((m + 1) as u8, anchors[m])?;
        m += 1;
    }
    if bf.mach_count() != 3 {
        return Err(SEG_BASE);
    }
    // 判定再现链。
    bf.reproduce_judgments()?;
    bf.final_assert()?;
    bf.median_reproduce_assert()?;
    let med = bf.median_total()?;
    if med != 2880 {
        return Err(SEG_BASE);
    }
    let h = bf.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 封核后写拒绝。
    match bf.put_report(ReportLine { src: R_F0661, machine: 1, a: anchors[0] }) {
        Err(c) if c == E_SEALED || c == E_TWICE => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：单源缺失（只有 F0661）。
    let mut bf2 = BootFinal::new();
    let mut m = 0usize;
    while m < MACHINES {
        bf2.put_report(ReportLine { src: R_F0661, machine: (m + 1) as u8, a: anchors[m] })?;
        bf2.put_final_anchors((m + 1) as u8, anchors[m])?;
        m += 1;
    }
    match bf2.reproduce_judgments() {
        Err(c) if c == E_R2_MISMATCH => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：锚点漂移（终核与报告不等）。
    let mut bf3 = BootFinal::new();
    let mut src = R_F0661;
    while src <= R_F0679 {
        let mut m = 0usize;
        while m < MACHINES {
            bf3.put_report(ReportLine { src, machine: (m + 1) as u8, a: anchors[m] })?;
            m += 1;
        }
        src += 1;
    }
    let drifted = Anchors { t0: 40, t1: 1610, t2: 2700, t3: 2850 };
    bf3.put_final_anchors(1, drifted)?;
    bf3.put_final_anchors(2, anchors[1])?;
    bf3.put_final_anchors(3, anchors[2])?;
    match bf3.reproduce_judgments() {
        Err(c) if c == E_R1_MISMATCH => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：超阈值。
    let mut bf4 = BootFinal::new();
    let slow = [Anchors { t0: 10, t1: 1500, t2: 2600, t3: 3400 },
        Anchors { t0: 11, t1: 1510, t2: 2610, t3: 3410 },
        Anchors { t0: 12, t1: 1520, t2: 2620, t3: 3420 }];
    let mut src = R_F0661;
    while src <= R_F0679 {
        let mut m = 0usize;
        while m < MACHINES {
            bf4.put_report(ReportLine { src, machine: (m + 1) as u8, a: slow[m] })?;
            m += 1;
        }
        src += 1;
    }
    let mut m = 0usize;
    while m < MACHINES {
        bf4.put_final_anchors((m + 1) as u8, slow[m])?;
        m += 1;
    }
    match bf4.median_reproduce_assert() {
        Err(c) if c == E_THRESHOLD => {}
        _ => return Err(SEG_BASE),
    }
    let fl = bf.final_lines();
    if fl.len() != 5 {
        return Err(SEG_BASE);
    }
    Ok(0)
}
