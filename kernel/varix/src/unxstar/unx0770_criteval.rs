#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0770 · 判据三成分全域抽核（crit 数值成分）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 1 单）
// 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥580 行；② 判据原文：UNX-F0770-J1 抽核过（800 条判据三成分：
//   数值成分全域 100% + 真机/对照成分抽样 ≥40 条核）；③ 完成时 API/软件写回：
//   状态=已完成、结果摘要非空。
// 设计纪律：① 数值成分全域 100% 逐条核（800/800），禁抽样替代；真机/对照成分
//   定步长抽样核（stride=20 恰 40 条，≥40 判据下限），抽样序列写死可复算。
// ② 计数走双路求和（正序/逆序）交叉一致，checked_add 溢出即时红案，禁饱和吞数。
// ③ 状态机合法边闭集，跨段边/回头边一律 StateInvalid 拒记。
// ④ 常量全部写死并注来源；判据原文取数（800/40/stride 20）不得改动。
// ⑤ 错误为 XcCode 闭集（SEG 0x5F00 段独占），每码可定位成分与条目号。
// 诊断段声明：0x5F00–0x5F1F 本单独占（清单 <SEG 0x5f00>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 1. 诊断码段（本单独占 0x5F00–0x5F1F）
// ===========================================================================

pub const SEG_BASE: u16 = 0x5F00;  // 诊断码段基数。来源：W7 清单 UNX-F0770 行 `<SEG 0x5f00>`。
pub const SEG_TOP: u16 = 0x5F1F;  // 诊断码段顶界（含）。32 码位闭集。

pub const XC_STATE_INVALID: u16 = SEG_BASE + 1;  // 状态非法（跨段边/回头边/跳段边）。
pub const XC_TOTAL_SHORT: u16 = SEG_BASE + 2;  // 判据总条数不足 800（全域口径被破坏）。
pub const XC_DUP_ID: u16 = SEG_BASE + 3;  // 判据号重复（全域登记表要求 fid 严格唯一）。
pub const XC_NUMERIC_FLOOR: u16 = SEG_BASE + 4;  // 数值成分低于下阈（全域 100% 核逐条检出）。
pub const XC_NUMERIC_CEIL: u16 = SEG_BASE + 5;  // 数值成分高于上阈（全域 100% 核逐条检出）。
pub const XC_SUM_MISMATCH: u16 = SEG_BASE + 6;  // 双路求和不一致（正序 Σ ≠ 逆序 Σ，账面被扰动）。
pub const XC_SUM_OVERFLOW: u16 = SEG_BASE + 7;  // 计数加法溢出（checked_add 即时红案）。
pub const XC_NUMERIC_NOT_DONE: u16 = SEG_BASE + 8;  // 未完成全域数值扫核即请求抽样（顺序非法）。
pub const XC_SAMPLE_SHORT: u16 = SEG_BASE + 9;  // 抽样条数不足 40（判据原文下限）。
pub const XC_SAMPLE_DUP: u16 = SEG_BASE + 10;  // 抽样序列出现重复下标（stride 配置被破坏）。
pub const XC_MACHINE_FAIL: u16 = SEG_BASE + 11;  // 真机成分不达标（抽样核逐条检出：ok < rounds）。
pub const XC_CONTROL_DRIFT: u16 = SEG_BASE + 12;  // 对照成分漂移超容差（抽样核逐条检出）。
pub const XC_ALREADY_VERDICT: u16 = SEG_BASE + 13;  // 终判重复请求（已出终判后禁再判）。
pub const XC_EMPTY_BATCH: u16 = SEG_BASE + 14;  // 空批次喂入（禁零长度批次污染账面）。
pub const XC_VERDICT_NOT_READY: u16 = SEG_BASE + 15;  // 终判条件未齐（数值或抽样任一面未走完）。

// ===========================================================================
// 2. 判据常量（全部写死，来源=判据原文）
// ===========================================================================

pub const TOTAL_CRITS: usize = 800;  // 判据总条数：800。来源：判据原文「800 条判据三成分」。
pub const SAMPLE_MIN: usize = 40;  // 真机/对照成分抽样条数下限：40。来源：判据原文「抽样 ≥40 条核」。
pub const SAMPLE_STRIDE: usize = TOTAL_CRITS / SAMPLE_MIN;  // 抽样步长：800 / 40 = 20。来源：全域等距抽样派生（写死防再算漂移）。
pub const NUMERIC_FLOOR: u32 = 1;  // 数值成分下阈：1。来源：本单抽核口径（数值成分有效值下界，头注假设）。
pub const NUMERIC_CEIL: u32 = 65_535;  // 数值成分上阈：65_535。来源：本单抽核口径（u16 量纲上界，头注假设）。
pub const CONTROL_TOLERANCE: u32 = 5;  // 对照成分漂移容差：5（整数绝对差）。来源：本单抽核口径（头注假设）。
pub const MACHINE_MIN_ROUNDS: u16 = 1;  // 真机成分轮数下限：1（至少跑 1 轮才有资格被抽核）。

// ===========================================================================
// 3. 状态机（合法边闭集）
// ===========================================================================

/// 抽核状态机。
/// 合法边：Fresh→Feeding（首批喂入）；Feeding→Feeding（续喂）；
/// Feeding→NumericDone（全域数值扫核完成）；NumericDone→SampledDone（抽样核完成）；
/// SampledDone→Verdict（终判）。其余边一律 XC_STATE_INVALID。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SweepPhase {
    /// 未喂入。
    Fresh = 0,
    /// 喂入中（允许续喂）。
    Feeding = 1,
    /// 全域数值扫核完成。
    NumericDone = 2,
    /// 抽样核完成。
    SampledDone = 3,
    /// 终判已出（只读）。
    Verdict = 4,
}

impl SweepPhase {
    pub fn can_step(self, next: SweepPhase) -> bool {  // 合法边闭集判定：self → next 是否允许。
        match (self, next) {
            (SweepPhase::Fresh, SweepPhase::Feeding) => true,
            (SweepPhase::Feeding, SweepPhase::Feeding) => true,
            (SweepPhase::Feeding, SweepPhase::NumericDone) => true,
            (SweepPhase::NumericDone, SweepPhase::SampledDone) => true,
            (SweepPhase::SampledDone, SweepPhase::Verdict) => true,
            _ => false,
        }
    }
}

// ===========================================================================
// 4. 判据三成分数据结构
// ===========================================================================

/// 数值成分：每条判据的量化指标（全域 100% 核对象）。
#[derive(Clone, Copy, Debug)]
pub struct NumericComp {
    pub value: u32,  // 指标取值（整数，禁浮点）。
    pub unit: u8,  // 量纲代号（1=行数 2=计数 3=毫秒 4=百分比×100，闭集外拒收）。
}

/// 真机成分：该判据在真机上的执行结论（抽样核对象之一）。
#[derive(Clone, Copy, Debug)]
pub struct MachineComp {
    pub machine: u8,  // 真机代号（1..=3，三机矩阵口径）。
    pub rounds: u16,  // 计划轮数。
    pub ok: u16,  // 通过轮数（要求 ok == rounds 才算达标）。
}

/// 对照成分：与对照基线的偏差核（抽样核对象之二）。
#[derive(Clone, Copy, Debug)]
pub struct ControlComp {
    pub baseline: u32,  // 对照基线值。
    pub observed: u32,  // 本轮观测值。
}

/// 一条判据的完整三成分。
#[derive(Clone, Copy, Debug)]
pub struct CritEntry {
    pub fid: u16,  // 判据号（1..=800，全域严格唯一）。
    pub numeric: NumericComp,  // 数值成分。
    pub machine: MachineComp,  // 真机成分。
    pub control: ControlComp,  // 对照成分。
}

impl CritEntry {
    pub fn numeric_ok(&self) -> bool {  // 数值成分单条判定：下阈 ≤ value ≤ 上阈 且量纲在闭集。
        let unit_ok = self.numeric.unit >= 1 && self.numeric.unit <= 4;
        unit_ok
            && self.numeric.value >= NUMERIC_FLOOR
            && self.numeric.value <= NUMERIC_CEIL
    }
    pub fn machine_ok(&self) -> bool {  // 真机成分单条判定：轮数达下限且通过数 == 计划数。
        self.machine.machine >= 1
            && self.machine.machine <= 3
            && self.machine.rounds >= MACHINE_MIN_ROUNDS
            && self.machine.ok == self.machine.rounds
    }
    pub fn control_ok(&self) -> bool {  // 对照成分单条判定：|observed − baseline| ≤ 容差（整数绝对差，禁浮点）。
        let drift = if self.control.observed >= self.control.baseline {
            self.control.observed - self.control.baseline
        } else {
            self.control.baseline - self.control.observed
        };
        drift <= CONTROL_TOLERANCE
    }
}

/// 一条抽样核记录。
#[derive(Clone, Debug)]
pub struct SampleRecord {
    pub index: usize,  // 被抽中的全域下标（0..800）。
    pub fid: u16,  // 判据号。
    pub machine_pass: bool,  // 真机成分结论。
    pub control_pass: bool,  // 对照成分结论。
    pub note: String,  // 结论注记（红案时写明成分与偏差）。
}

// ===========================================================================
// 5. 报告结构
// ===========================================================================

/// 全域数值扫核报告。
#[derive(Clone, Debug, PartialEq)]
pub struct NumericReport {
    pub checked: u32,  // 已核条数（必须 == 800，全域口径）。
    pub passed: u32,  // 通过条数。
    pub failed: u32,  // 红案条数。
    pub first_bad_fid: u16,  // 首个红案判据号（无红案为 0）。
    pub first_bad_code: u16,  // 首个红案码。
    pub sum_forward: u64,  // 正序累加和（数值成分 value 之和，u64 防溢出域）。
    pub sum_reverse: u64,  // 逆序累加和（与正序必须一致）。
}

/// 抽样核报告。
#[derive(Clone, Debug)]
pub struct SampleReport {
    pub sampled: u32,  // 抽样条数（必须 ≥ 40）。
    pub machine_red: u32,  // 真机成分红案数。
    pub control_red: u32,  // 对照成分红案数。
    pub records: Vec<SampleRecord>,  // 抽样记录（按抽样顺序，确定性）。
}

/// 终判报告。
#[derive(Clone, Debug, PartialEq)]
pub struct VerdictReport {
    pub numeric_all_pass: bool,  // 数值面结论：true=全域 100% 过。
    pub sample_all_pass: bool,  // 抽样面结论：true=抽样全过且 ≥40 条。
    pub sampled: u32,  // 抽样条数回执。
    pub summary: String,  // 结论摘要（非空，写回 API/软件用）。
    pub fingerprint: u64,  // 账面指纹（FNV-1a-64，可对拍）。
}

// ===========================================================================
// 6. 抽核机本体
// ===========================================================================

pub struct CritSampler {  // 判据三成分全域抽核机。
    /// 状态机当前相位。
    phase: SweepPhase,
    /// 全域判据账（按喂入序；fid 唯一性在喂入时守卫）。
    book: Vec<CritEntry>,
    /// fid → 账内下标 的定位账（容量守恒：len == book.len()）。
    index_by_fid: Vec<u16>,
    /// 数值扫核报告（None=未扫）。
    numeric: Option<NumericReport>,
    /// 抽样核报告（None=未抽）。
    sample: Option<SampleReport>,
    /// 事件账（审计留痕，只增不删）。
    events: Vec<String>,
}

impl CritSampler {
    pub fn new() -> Self {  // 建机：Fresh 相位，空账。
        CritSampler {
            phase: SweepPhase::Fresh,
            book: Vec::new(),
            index_by_fid: Vec::new(),
            numeric: None,
            sample: None,
            events: Vec::new(),
        }
    }

    pub fn feed_batch(&mut self, batch: &[CritEntry]) -> Result<u32, u16> {  // 喂入一批判据。守卫：非空批、fid 全域唯一、总量不超 800。
        if batch.is_empty() {
            return Err(XC_EMPTY_BATCH);
        }
        if !self.phase.can_step(SweepPhase::Feeding) {
            return Err(XC_STATE_INVALID);
        }
        let mut added: u32 = 0;
        for e in batch.iter() {
            if self.book.len() >= TOTAL_CRITS {
                return Err(XC_TOTAL_SHORT);
            }
            // fid 唯一性：定位账线性查（800 规模确定性可接受）。
            let mut dup = false;
            let mut i = 0usize;
            while i < self.index_by_fid.len() {
                if self.index_by_fid[i] == e.fid {
                    dup = true;
                    break;
                }
                i += 1;
            }
            if dup {
                return Err(XC_DUP_ID);
            }
            self.index_by_fid.push(e.fid);
            self.book.push(*e);
            added = match added.checked_add(1) {
                Some(v) => v,
                None => return Err(XC_SUM_OVERFLOW),
            };
        }
        if self.book.len() > TOTAL_CRITS {
            return Err(XC_TOTAL_SHORT);
        }
        if matches!(self.phase, SweepPhase::Fresh) {
            self.phase = SweepPhase::Feeding;
        }
        self.events
            .push(alloc::format!("feed +{} total={}", added, self.book.len()));
        Ok(added)
    }

    pub fn is_full(&self) -> bool {  // 喂入是否已满额（满额才允许进入扫核）。
        self.book.len() == TOTAL_CRITS
    }

    pub fn numeric_sweep(&mut self) -> Result<NumericReport, u16> {  // 全域数值扫核：800/800 逐条 100%，双路求和交叉一致。
        if !self.phase.can_step(SweepPhase::NumericDone) {
            return Err(if matches!(self.phase, SweepPhase::Fresh | SweepPhase::Feeding) {
                XC_VERDICT_NOT_READY
            } else {
                XC_STATE_INVALID
            });
        }
        if self.book.len() != TOTAL_CRITS {
            return Err(XC_TOTAL_SHORT);
        }
        // 正序扫核
        let mut passed: u32 = 0;
        let mut failed: u32 = 0;
        let mut first_bad_fid: u16 = 0;
        let mut first_bad_code: u16 = 0;
        let mut sum_forward: u64 = 0;
        let mut i = 0usize;
        while i < self.book.len() {
            let e = &self.book[i];
            sum_forward = match sum_forward.checked_add(e.numeric.value as u64) {
                Some(v) => v,
                None => return Err(XC_SUM_OVERFLOW),
            };
            if e.numeric_ok() {
                passed = match passed.checked_add(1) {
                    Some(v) => v,
                    None => return Err(XC_SUM_OVERFLOW),
                };
            } else {
                let code = if e.numeric.value < NUMERIC_FLOOR {
                    XC_NUMERIC_FLOOR
                } else if e.numeric.value > NUMERIC_CEIL {
                    XC_NUMERIC_CEIL
                } else {
                    XC_NUMERIC_FLOOR
                };
                if first_bad_fid == 0 {
                    first_bad_fid = e.fid;
                    first_bad_code = code;
                }
                failed = match failed.checked_add(1) {
                    Some(v) => v,
                    None => return Err(XC_SUM_OVERFLOW),
                };
            }
            i += 1;
        }
        // 逆序复扫（双路求和交叉一致）
        let mut sum_reverse: u64 = 0;
        let mut j = self.book.len();
        while j > 0 {
            j -= 1;
            sum_reverse = match sum_reverse.checked_add(self.book[j].numeric.value as u64) {
                Some(v) => v,
                None => return Err(XC_SUM_OVERFLOW),
            };
        }
        if sum_forward != sum_reverse {
            return Err(XC_SUM_MISMATCH);
        }
        // 计数守恒恒等式：checked == passed + failed == 800（禁虚记）
        let total = match passed.checked_add(failed) {
            Some(v) => v,
            None => return Err(XC_SUM_OVERFLOW),
        };
        if total != TOTAL_CRITS as u32 {
            return Err(XC_SUM_MISMATCH);
        }
        let rep = NumericReport {
            checked: total,
            passed,
            failed,
            first_bad_fid,
            first_bad_code,
            sum_forward,
            sum_reverse,
        };
        self.phase = SweepPhase::NumericDone;
        self.numeric = Some(rep.clone());
        self.events.push(alloc::format!(
            "numeric sweep pass={} fail={}",
            passed,
            failed
        ));
        Ok(rep)
    }

    pub fn stride_sample(&mut self) -> Result<SampleReport, u16> {  // 定步长抽样核：下标 0,20,40,…,780 恰 40 条，真机+对照双成分走核。
        if !self.phase.can_step(SweepPhase::SampledDone) {
            return Err(if matches!(self.phase, SweepPhase::NumericDone) {
                XC_SAMPLE_SHORT
            } else if matches!(self.phase, SweepPhase::Fresh | SweepPhase::Feeding) {
                XC_NUMERIC_NOT_DONE
            } else {
                XC_STATE_INVALID
            });
        }
        let mut records: Vec<SampleRecord> = Vec::new();
        let mut machine_red: u32 = 0;
        let mut control_red: u32 = 0;
        let mut k = 0usize;
        while k < SAMPLE_MIN {
            let idx = k * SAMPLE_STRIDE;
            if idx >= self.book.len() {
                return Err(XC_SAMPLE_SHORT);
            }
            // 抽样序列唯一性守卫（stride 配置被破坏即红）。
            let mut t = 0usize;
            while t < records.len() {
                if records[t].index == idx {
                    return Err(XC_SAMPLE_DUP);
                }
                t += 1;
            }
            let e = &self.book[idx];
            let mp = e.machine_ok();
            let cp = e.control_ok();
            let mut note = String::from("ok");
            if !mp {
                note = String::from("machine red: ok<rounds");
                machine_red += 1;
            }
            if !cp {
                note = String::from("control red: drift>tolerance");
                control_red += 1;
            }
            records.push(SampleRecord {
                index: idx,
                fid: e.fid,
                machine_pass: mp,
                control_pass: cp,
                note,
            });
            k += 1;
        }
        if records.len() < SAMPLE_MIN {
            return Err(XC_SAMPLE_SHORT);
        }
        let rep = SampleReport {
            sampled: records.len() as u32,
            machine_red,
            control_red,
            records,
        };
        self.phase = SweepPhase::SampledDone;
        self.sample = Some(rep.clone());
        self.events
            .push(alloc::format!("sample n={} mred={} cred={}", rep.sampled, machine_red, control_red));
        Ok(rep)
    }

    pub fn verdict(&mut self) -> Result<VerdictReport, u16> {  // 终判：数值面 100% 过 + 抽样面全过且 ≥40 → 结论定格。
        if matches!(self.phase, SweepPhase::Verdict) {
            return Err(XC_ALREADY_VERDICT);
        }
        if !self.phase.can_step(SweepPhase::Verdict) {
            return Err(XC_VERDICT_NOT_READY);
        }
        let num = match &self.numeric {
            Some(n) => n.clone(),
            None => return Err(XC_VERDICT_NOT_READY),
        };
        let smp = match &self.sample {
            Some(s) => s.clone(),
            None => return Err(XC_VERDICT_NOT_READY),
        };
        let numeric_all_pass = num.failed == 0 && num.checked == TOTAL_CRITS as u32;
        let sample_all_pass =
            smp.machine_red == 0 && smp.control_red == 0 && smp.sampled >= SAMPLE_MIN as u32;
        let fp = self.fingerprint();
        let summary = if numeric_all_pass && sample_all_pass {
            String::from("UNX-F0770-J1 抽核过：数值成分全域 100% 过，真机/对照抽样 40/40 过")
        } else {
            String::from("UNX-F0770-J1 抽核未过：存在红案，见数值/抽样报告定位")
        };
        let rep = VerdictReport {
            numeric_all_pass,
            sample_all_pass,
            sampled: smp.sampled,
            summary,
            fingerprint: fp,
        };
        self.phase = SweepPhase::Verdict;
        Ok(rep)
    }

    pub fn fingerprint(&self) -> u64 {  // 账面指纹：FNV-1a-64 对（fid, value, rounds, baseline）规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.book.len() {
            let e = &self.book[i];
            for v in [e.fid as u64, e.numeric.value as u64, e.machine.rounds as u64, e.control.baseline as u64] {
                let mut b = 0usize;
                while b < 8 {
                    h ^= (v >> (b * 8)) & 0xFF;
                    h = h.wrapping_mul(0x0000_0100_0000_01B3);
                    b += 1;
                }
            }
            i += 1;
        }
        h
    }

    pub fn event_count(&self) -> usize {  // 事件账长度（审计口径）。
        self.events.len()
    }
    pub fn phase(&self) -> SweepPhase {  // 当前相位。
        self.phase
    }
    pub fn registered(&self) -> usize {  // 已登记条数（守恒口径：feed 累计 − 拒收回滚 == book.len()）。
        self.book.len()
    }
}

// ===========================================================================
// 7. 样本数据发生器（确定性 LCG，drill 与对拍共用）
// ===========================================================================

/// 确定性发生 800 条判据（种子固定，逐字节可复算）。
pub fn gen_crits(seed: u64) -> Vec<CritEntry> {  // 假设头注：drill 样本不代表真实批册，仅验证抽核机主路径/错误路径逻辑。
    let mut s = seed;
    let mut out: Vec<CritEntry> = Vec::new();
    let mut fid: u16 = 1;
    while fid <= TOTAL_CRITS as u16 {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let r1 = (s >> 33) as u32;
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let r2 = (s >> 33) as u32;
        let baseline = 100 + (r1 % 200);
        out.push(CritEntry {
            fid,
            numeric: NumericComp {
                value: NUMERIC_FLOOR + (r1 % (NUMERIC_CEIL - NUMERIC_FLOOR + 1)),
                unit: (r2 % 4) as u8 + 1,
            },
            machine: MachineComp {
                machine: (r1 % 3) as u8 + 1,
                rounds: (r2 % 5) as u16 + 1,
                ok: (r2 % 5) as u16 + 1,
            },
            // 观测值压在容差内：baseline ± 3（确定性合规样本）。
            control: ControlComp {
                baseline,
                observed: if r2 & 1 == 0 { baseline + 3 } else { baseline - 3 },
            },
        });
        fid += 1;
    }
    out
}

// ===========================================================================
// 8. 自例程 drill：主路径 + 错误路径，0=全过
// ===========================================================================

/// 自例程：样本走主路径（800 喂满→全域扫核→抽样→终判全过）+
pub fn drill() -> Result<u32, u16> {  // 错误路径（欠额、fid 重复、数值越阈、真机红案逐项检出）。0=全过。
    // —— 主路径：分 5 批喂满 800 → 全域扫核 → 抽样 → 终判全过 ——
    let mut m = CritSampler::new();
    let crits = gen_crits(0x5F00_5F00_5F00_0001);
    let mut off = 0usize;
    while off < TOTAL_CRITS {
        let end = if off + 160 < TOTAL_CRITS { off + 160 } else { TOTAL_CRITS };
        m.feed_batch(&crits[off..end])?;
        off = end;
    }
    if !m.is_full() || m.registered() != TOTAL_CRITS {
        return Err(XC_TOTAL_SHORT);
    }
    let num = m.numeric_sweep()?;
    if num.checked != TOTAL_CRITS as u32 || num.failed != 0 || num.sum_forward != num.sum_reverse {
        return Err(XC_SUM_MISMATCH);
    }
    let smp = m.stride_sample()?;
    if smp.sampled != SAMPLE_MIN as u32 || smp.machine_red != 0 || smp.control_red != 0 {
        return Err(XC_SAMPLE_SHORT);
    }
    let v = m.verdict()?;
    if !v.numeric_all_pass || !v.sample_all_pass || v.summary.is_empty() {
        return Err(XC_VERDICT_NOT_READY);
    }
    // 终判后重复判 → 必红 XC_ALREADY_VERDICT。
    if m.verdict() != Err(XC_ALREADY_VERDICT) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：欠额即请求扫核 ——
    let mut m2 = CritSampler::new();
    m2.feed_batch(&crits[0..10])?;
    if m2.numeric_sweep() != Err(XC_VERDICT_NOT_READY) {
        return Err(XC_NUMERIC_NOT_DONE);
    }
    // —— 错误路径 2：fid 重复 ——
    let mut m3 = CritSampler::new();
    let mut dupbatch = crits[0..2].to_vec();
    dupbatch.push(crits[0]);
    if m3.feed_batch(&dupbatch) != Err(XC_DUP_ID) {
        return Err(XC_DUP_ID);
    }
    // —— 错误路径 3：空批 ——
    let mut m4 = CritSampler::new();
    if m4.feed_batch(&[]) != Err(XC_EMPTY_BATCH) {
        return Err(XC_EMPTY_BATCH);
    }
    // —— 错误路径 4：数值越下阈（全域扫核必抓）——
    let mut m5 = CritSampler::new();
    let mut bad = crits.clone();
    bad[0].numeric.value = 0;
    m5.feed_batch(&bad)?;
    let n5 = m5.numeric_sweep()?;
    if n5.failed != 1 || n5.first_bad_code != XC_NUMERIC_FLOOR || n5.first_bad_fid != 1 {
        return Err(XC_NUMERIC_FLOOR);
    }
    // —— 错误路径 5：真机成分红案（抽样必检出）——
    let mut m6 = CritSampler::new();
    let mut bad2 = crits.clone();
    bad2[SAMPLE_STRIDE].machine.ok = bad2[SAMPLE_STRIDE].machine.rounds - 1;
    m6.feed_batch(&bad2)?;
    let _n6 = m6.numeric_sweep()?;
    let s6 = m6.stride_sample()?;
    if s6.machine_red != 1 || m6.verdict().is_ok() {
        return Err(XC_MACHINE_FAIL);
    }
    Ok(0)
}
