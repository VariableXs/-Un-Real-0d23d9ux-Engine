//! UNX-F0687 · 成功率统计账（150 次 ≥99% 判定）· 诊断段 0x1610
//!
//! ── 验收锚点（原文全抄） ─────────────────────────────────────────────
//! 定位：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md 行 2263，
//! 表头「| 编号 | 功能条目 | 行数 | 状态 | 判据 |」下：
//!
//! | UNX-F0687 | 成功率统计账（150 次 ≥99% 判定） | 580 | 已深化 | UNX-F0687-J1
//!   成功率统计（成功/150）≥99% 判定账落簿且分机成功率三列齐，口径二值零灰区 |
//!
//! J 判据拆解（本模块逐条落地的验收口径）：
//!   J1-a 成功率统计（成功/150）≥99%：ppm 整数制，成功×10000/150 ≥ 9900 即达标，
//!        二态判定（Pass/Reject），边界 9900ppm 恰为达标（≥ 语义，非 >）。
//!   J1-b 判定账落簿：判定结论以 VerdictRecord 形式写入账体（ppm/结论/封板指纹），
//!        并同步追加事件账 VerdictWritten 条目；样本不足 150 时拒出结论，
//!        返回 Inconclusive（不虚报 ppm 达标位）。
//!   J1-c 分机成功率三列齐：三机矩阵（机号 0/1/2）各 50 次，
//!        每机一列（成功数/执行数/成功率 ppm），三列 complete 全真才算齐；
//!        缺机（某机 0 次）与缺轮（机内跳号）显性检出。
//!   J1-d 口径二值零灰区：RunOutcome 闭集 {Success=1, Fail=0}，
//!        不存在 Partial/Retry/Unknown 等第三态；外部注入原始字节只认 0/1，
//!        其余一律按灰区结果注入拦截（E_GRAY，0x1616）。
//!
//! ── 相邻任务对接 ─────────────────────────────────────────────────────
//!   F0682 矩阵轮排：canonical_stream() 程序化生成 3 机 × 50 次 = 150 槽位序，
//!                   verify_stream_matches_plan() 逐槽对拍。
//!   F0683 逐次留痕：落簿行带连续 seq（1..=150），机内轮号严格递增，零跳号零重号。
//!   F0688 失败归档：本账只存「失败轮 → 归档件引用号」的引用（≤64 字节串），
//!                   内容本体归 F0688，本账零内容落簿（只引不载）。
//!   F0696 幂等复跑：dual_run_replay / dual_run_full 双跑对拍，指纹全等才算零漂移。
//!
//! ── 通用纪律 ─────────────────────────────────────────────────────────
//!   no_std + alloc；只 use core::… 与 use alloc::{…}，禁 use crate::…。
//!   内核路径零 unwrap/expect/panic!/运行期 assert!（编译期 const assert! 允许）。
//!   可失败 API 全走 Result<T, SuccRateError>；溢出显性 saturating/wrapping。
//!   >64KiB 结构体禁上栈（本文件最大栈值 [RunSlot;150] ≈ 600B，安全）。
//!   const fn 纪律：const fn 内零 slice::get / Vec 索引 / Ord::min-max /
//!   derive(PartialEq) 类型的 == / for 循环（一律 while + 索引）；
//!   含 Vec 字段的结构体禁 derive Copy。
//!
//! 零浮点、零 unwrap、零 expect、零 panic 路径。

use core::mem::size_of;

use alloc::{
    format,
    string::String,
    vec::Vec,
};

// ── 诊断码段（0x1610 独占 0x10，先验空闲：0x1590 被 cga26_gauss 占，
//    0x15A0 被 ves453 占，0x15C0~0x15FF 被 unx2361 声明独占，故取 0x1610） ──
pub const SEG_BASE: u16 = 0x1610;

const E_SEALED: u16 = SEG_BASE + 0x1; // 0x1611 账已封板仍写
const E_MACHINE_RANGE: u16 = SEG_BASE + 0x2; // 0x1612 机号越界（合法 0..3）
const E_ROUND_RANGE: u16 = SEG_BASE + 0x3; // 0x1613 轮号越界（合法 1..=50）
const E_DUP_ROUND: u16 = SEG_BASE + 0x4; // 0x1614 同轮号重复登记
const E_GAP: u16 = SEG_BASE + 0x5; // 0x1615 机内轮号跳号（缺轮注入）
const E_GRAY: u16 = SEG_BASE + 0x6; // 0x1616 灰区结果注入（非 0/1）
const E_NOTFULL: u16 = SEG_BASE + 0x7; // 0x1617 样本不足 150 拒封板
const E_MACHINE_INCOMPLETE: u16 = SEG_BASE + 0x8; // 0x1618 分机不足 50 次（缺机）
const E_REF_MISSING: u16 = SEG_BASE + 0x9; // 0x1619 失败轮缺归档引用号
const E_REF_DUP: u16 = SEG_BASE + 0xA; // 0x161A 引用号重复（同轮或同串）
const E_REF_INVALID: u16 = SEG_BASE + 0xB; // 0x161B 引用号空串或超长
const E_REF_TARGET: u16 = SEG_BASE + 0xC; // 0x161C 引用挂到非失败轮
const E_CONSERVATION: u16 = SEG_BASE + 0xD; // 0x161D 守恒破（Σ分机 ≠ 总账）
const E_REPLAY: u16 = SEG_BASE + 0xE; // 0x161E 双跑对拍不一致
const E_PLAN: u16 = SEG_BASE + 0xF; // 0x161F 与 F0682 轮排槽位不符

/// 诊断码 → 中文可操作诊断（独占段 0x1610~0x161F 内解释，段外一律未知）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "账已封板：再写属误用，请克隆新账或走只读通道",
        E_MACHINE_RANGE => "机号越界：三机矩阵合法机号为 0/1/2",
        E_ROUND_RANGE => "轮号越界：每机合法轮号为 1..=50",
        E_DUP_ROUND => "同轮号重复登记：该机该轮已落簿，拒收二写",
        E_GAP => "机内轮号跳号：缺轮注入，请先补齐缺号轮次",
        E_GRAY => "灰区结果注入：口径二值零灰区，原始字节只认 0/1",
        E_NOTFULL => "样本不足 150：拒出达标结论，请补齐矩阵再封板",
        E_MACHINE_INCOMPLETE => "分机不足 50 次：分机三列不齐，缺机缺轮需补跑",
        E_REF_MISSING => "失败轮缺归档引用号：请对每个失败轮挂 F0688 引用",
        E_REF_DUP => "归档引用号重复：同轮或同串二次挂账，拒收",
        E_REF_INVALID => "归档引用号非法：空串或超长（上限 64 字节）",
        E_REF_TARGET => "归档引用挂到非失败轮：仅失败轮可挂 F0688 引用",
        E_CONSERVATION => "守恒破坏：Σ分机计数 ≠ 总账行数，账体已被污染",
        E_REPLAY => "双跑对拍不一致：同输入两账指纹漂移，幂等破",
        E_PLAN => "与 F0682 轮排槽位不符：机号/轮号顺序偏离编排表",
        _ => "未知诊断码（0x1610 段外或保留位）",
    }
}

// ── 矩阵常量（来源：判据 UNX-F0687-J1「成功/150」与「分机三列」） ──
/// 三机矩阵机数（判据：分机成功率三列）。
pub const MACHINE_COUNT: usize = 3;
/// 每机轮数（判据：150 次总账 ÷ 3 机 = 50 次/机；对接 F0682 轮排）。
pub const ROUNDS_PER_MACHINE: usize = 50;
/// 总账次数（判据原文：成功/150）。
pub const TOTAL_RUNS: usize = MACHINE_COUNT * ROUNDS_PER_MACHINE;
/// 达标线：≥99% = 9900 ppm（判据原文 ≥99%，ppm 整数制零浮点）。
pub const PASS_PPM: u64 = 9_900;
/// ppm 换算基数（10000 ppm = 100%）。
pub const PPM_SCALE: u64 = 10_000;
/// 单条结果原始字节闭集：0=Fail，1=Success（其余全灰区）。
pub const RAW_FAIL: u8 = 0;
pub const RAW_SUCCESS: u8 = 1;
/// 归档引用号长度上限（字节）；只存引用不存内容，内容归 F0688。
pub const FAIL_REF_MAX_LEN: usize = 64;

// 编译期守恒断言（允许：const 期，非运行期）。
const _: () = assert!(MACHINE_COUNT == 3);
const _: () = assert!(ROUNDS_PER_MACHINE == 50);
const _: () = assert!(MACHINE_COUNT * ROUNDS_PER_MACHINE == TOTAL_RUNS);
const _: () = assert!(TOTAL_RUNS == 150);
const _: () = assert!(PASS_PPM == 9_900 && PPM_SCALE == 10_000);
const _: () = assert!(size_of::<LedgerRow>() <= 16);
const _: () = assert!(size_of::<RunSlot>() <= 8);

// ── 二值口径闭集：Success / Fail，零灰区 ─────────────────────────────
/// 单次运行结果。口径二值是判据（J1-d）：不存在 Partial/Retry/Unknown
/// 等任何第三态；本枚举即闭集全集，两臂之外无值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    /// 成功（原始字节 1）。
    Success,
    /// 失败（原始字节 0；内容归 F0688，本账只挂引用）。
    Fail,
}

impl RunOutcome {
    /// 闭集原始字节：Success=1，Fail=0。
    pub const fn as_raw(self) -> u8 {
        match self {
            RunOutcome::Success => RAW_SUCCESS,
            RunOutcome::Fail => RAW_FAIL,
        }
    }

    /// 从原始字节重建闭集值：只认 0/1，其余按灰区注入拒绝。
    /// 这是灰区拦截的唯一入口——任何外部结果必须先过本闸。
    pub fn from_raw(raw: u8) -> Result<RunOutcome, SuccRateError> {
        match raw {
            RAW_SUCCESS => Ok(RunOutcome::Success),
            RAW_FAIL => Ok(RunOutcome::Fail),
            other => Err(SuccRateError::GrayZoneResult { raw: other }),
        }
    }

    /// 二值位（算术用）：Success=1，Fail=0。
    pub const fn as_bit(self) -> u32 {
        match self {
            RunOutcome::Success => 1,
            RunOutcome::Fail => 0,
        }
    }
}

// ── 落簿行 ───────────────────────────────────────────────────────────
/// 一行总账：机号 + 轮号 + 二值结果 + 全账连续序号（1..=150，对接 F0683 留痕）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LedgerRow {
    /// 机号 0/1/2（三机矩阵列位）。
    pub machine: u8,
    /// 轮号 1..=50（机内严格递增，零跳号）。
    pub round: u16,
    /// 二值结果（闭集，零灰区）。
    pub outcome: RunOutcome,
    /// 全账连续序号 1..=150（落簿次序，零跳号零重号）。
    pub seq: u32,
}

impl LedgerRow {
    /// 规范字节序（指纹用）：机号|轮号 LE|结果|序号 LE。
    pub fn write_fingerprint(&self, h: &mut Fnv64) {
        h.write_u8(self.machine);
        h.write_u16(self.round);
        h.write_u8(self.outcome.as_raw());
        h.write_u32(self.seq);
    }
}

// ── 错误闭集（15 变体，恰占满 0x1611..0x161F 十五个诊断码） ───────────
/// 成功率账全通道错误。每变体携带定位字段与中文可操作诊断（detail）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuccRateError {
    /// 账已封板仍尝试写入。
    Sealed,
    /// 机号越界（合法 0/1/2）。
    MachineOutOfRange { got: u8 },
    /// 轮号越界（合法 1..=50）。
    RoundOutOfRange { got: u16 },
    /// 同轮号重复登记（同机同轮二写）。
    DuplicateRound { machine: u8, round: u16 },
    /// 机内轮号跳号（缺轮注入）：expected 为应收轮号。
    GapDetected { machine: u8, expected: u16, got: u16 },
    /// 灰区结果注入：原始字节不在 {0,1} 闭集内。
    GrayZoneResult { raw: u8 },
    /// 样本不足：have < need（need=150），拒封板/拒流对拍。
    NotFullyPopulated { have: u32, need: u32 },
    /// 分机不齐：该机 have 次 < need(50) 次（缺机/缺轮）。
    MachineIncomplete { machine: u8, have: u32, need: u32 },
    /// 封板时存在失败轮未挂归档引用号。
    FailRefMissing { machine: u8, round: u16 },
    /// 归档引用号重复（同轮已挂或同串已存在）。
    FailRefDuplicate { machine: u8, round: u16 },
    /// 引用号非法：len=0 空串或 len > cap(64)。
    RefInvalid { len: usize, cap: usize },
    /// 引用挂到非失败轮（Success 轮或未落簿轮）。
    RefTargetNotFail { machine: u8, round: u16 },
    /// 守恒破坏：Σ分机计数 ≠ 总账行数。
    ConservationBroken { sum_machine_runs: u32, total_rows: u32 },
    /// 双跑对拍不一致：fp_a/fp_b 为两账指纹。
    ReplayMismatch { fp_a: u64, fp_b: u64 },
    /// 与 F0682 轮排槽位不符：index 为首个偏离槽位。
    PlanMismatch { index: usize, machine: u8, round: u16 },
}

impl SuccRateError {
    /// 错误 → 诊断码（独占段 0x1610 内）。
    pub const fn err_code(&self) -> u16 {
        match self {
            SuccRateError::Sealed => E_SEALED,
            SuccRateError::MachineOutOfRange { .. } => E_MACHINE_RANGE,
            SuccRateError::RoundOutOfRange { .. } => E_ROUND_RANGE,
            SuccRateError::DuplicateRound { .. } => E_DUP_ROUND,
            SuccRateError::GapDetected { .. } => E_GAP,
            SuccRateError::GrayZoneResult { .. } => E_GRAY,
            SuccRateError::NotFullyPopulated { .. } => E_NOTFULL,
            SuccRateError::MachineIncomplete { .. } => E_MACHINE_INCOMPLETE,
            SuccRateError::FailRefMissing { .. } => E_REF_MISSING,
            SuccRateError::FailRefDuplicate { .. } => E_REF_DUP,
            SuccRateError::RefInvalid { .. } => E_REF_INVALID,
            SuccRateError::RefTargetNotFail { .. } => E_REF_TARGET,
            SuccRateError::ConservationBroken { .. } => E_CONSERVATION,
            SuccRateError::ReplayMismatch { .. } => E_REPLAY,
            SuccRateError::PlanMismatch { .. } => E_PLAN,
        }
    }

    /// 中文可操作诊断（判据：每变体 detail 给出下一步动作）。
    pub fn detail(&self) -> &'static str {
        diag_detail(self.err_code())
    }
}

// ── FNV-1a 64 位指纹（参数为 FNV-1a 标准常量：offset=0xcbf29ce484222325，
//    prime=0x100000001b3） ──────────────────────────────────────────────
/// 增量式 FNV-1a 哈希器（字节序显性小端推送）。
pub struct Fnv64 {
    h: u64,
}

impl Fnv64 {
    /// 新哈希器（FNV-1a offset basis）。
    pub const fn new() -> Self {
        Fnv64 {
            h: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// 推送单字节。
    pub fn write_u8(&mut self, b: u8) {
        self.h ^= b as u64;
        self.h = self.h.wrapping_mul(0x0000_0100_0000_01B3);
    }

    /// 推送 u16（小端两字节）。
    pub fn write_u16(&mut self, v: u16) {
        self.write_u8((v & 0xFF) as u8);
        self.write_u8((v >> 8) as u8);
    }

    /// 推送 u32（小端四字节）。
    pub fn write_u32(&mut self, v: u32) {
        self.write_u16((v & 0xFFFF) as u16);
        self.write_u16((v >> 16) as u16);
    }

    /// 推送 u64（小端八字节）。
    pub fn write_u64(&mut self, v: u64) {
        self.write_u32((v & 0xFFFF_FFFF) as u32);
        self.write_u32((v >> 32) as u32);
    }

    /// 推送字节串。
    pub fn write_bytes(&mut self, b: &[u8]) {
        let mut i: usize = 0;
        while i < b.len() {
            self.write_u8(b[i]);
            i += 1;
        }
    }

    /// 取指纹值。
    pub const fn finish(&self) -> u64 {
        self.h
    }
}

/// 一次性 FNV-1a 便捷入口。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = Fnv64::new();
    h.write_bytes(data);
    h.finish()
}

// ── 失败样本引用（只存引用不存内容，内容归 F0688） ───────────────────
/// 一条失败引用：失败轮定位 + F0688 归档件引用号。
/// 纪律：本结构体与账体只持有引用字符串（≤64 字节），
/// 日志段/截图位等归档内容本体一律不入本账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailRef {
    /// 失败轮机号。
    pub machine: u8,
    /// 失败轮轮号。
    pub round: u16,
    /// F0688 归档件引用号（如 "F0688-ARC-0007"；≤64 字节）。
    pub archive_ref: String,
}

impl FailRef {
    /// 规范字节序（指纹用）。
    pub fn write_fingerprint(&self, h: &mut Fnv64) {
        h.write_u8(self.machine);
        h.write_u16(self.round);
        h.write_bytes(self.archive_ref.as_bytes());
        h.write_u8(0xFF); // 串终结哨兵，防拼接歧义
    }
}

// ── 事件账 ───────────────────────────────────────────────────────────
/// 事件种类（事件账七类闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 结果行落簿成功。
    RowRecorded,
    /// 结果行写入被拒（注入拦截等，带诊断码）。
    RowRejected,
    /// 失败引用挂账成功。
    FailRefAttached,
    /// 失败引用挂账被拒。
    FailRefRejected,
    /// 封板（守恒+三列+引用齐验毕）。
    Sealed,
    /// 判定结论落簿（J1-b：判定账落簿）。
    VerdictWritten,
    /// 健康巡检落簿。
    HealthChecked,
}

impl EventKind {
    /// 事件种类规范字节（指纹外审计用）。
    pub const fn as_raw(self) -> u8 {
        match self {
            EventKind::RowRecorded => 1,
            EventKind::RowRejected => 2,
            EventKind::FailRefAttached => 3,
            EventKind::FailRefRejected => 4,
            EventKind::Sealed => 5,
            EventKind::VerdictWritten => 6,
            EventKind::HealthChecked => 7,
        }
    }
}

/// 单条事件：序号 + 种类 + 定位（机/轮，无定位填 0）+ 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookEvent {
    /// 事件账连续序号（从 1 起）。
    pub seq: u32,
    /// 事件种类。
    pub kind: EventKind,
    /// 关联机号（无关联填 0xFF）。
    pub machine: u8,
    /// 关联轮号（无关联填 0）。
    pub round: u16,
    /// 诊断码（成功类填 0，拒绝类填段内错误码）。
    pub code: u16,
}

// ── 分机三列（J1-c：分机成功率三列齐） ───────────────────────────────
/// 一机一列：成功数 / 执行数 / 成功率 ppm 三元组。
/// 三列齐 = 该机 50 次全落簿且轮号 1..=50 连续无缺。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachineColumn {
    /// 机号 0/1/2。
    pub machine: u8,
    /// 成功次数（≤50）。
    pub ok_count: u32,
    /// 执行次数（齐列时 = 50）。
    pub run_count: u32,
    /// 该机成功率（ok×10000/runs，runs=0 时为 0，不虚报）。
    pub ppm: u64,
    /// 该列是否齐（run_count==50 且轮位图全亮）。
    pub complete: bool,
}

impl MachineColumn {
    /// 列内失败数（runs - ok，runs≥ok 由闭集二值保证，仍用 saturating 防御）。
    pub const fn fail_count(&self) -> u32 {
        self.run_count.saturating_sub(self.ok_count)
    }
}

// ── 判定（J1-a / J1-b） ──────────────────────────────────────────────
/// 判定结论二态 + 不结论态。达标/不达标之间零灰区：
/// ppm ≥ 9900 即 Pass，否则 Reject；样本不足 150 时 Inconclusive（拒出结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 达标（ppm ≥ 9900）。
    Pass,
    /// 不达标（ppm < 9900）。
    Reject,
    /// 样本不足 150，拒出结论（不虚报达标位）。
    Inconclusive,
}

impl Verdict {
    /// 结论规范字节：Pass=2 / Reject=1 / Inconclusive=0。
    pub const fn as_raw(self) -> u8 {
        match self {
            Verdict::Pass => 2,
            Verdict::Reject => 1,
            Verdict::Inconclusive => 0,
        }
    }
}

/// 一次判定快照（可落簿、可格式化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Judgement {
    /// 判定结论。
    pub verdict: Verdict,
    /// 成功次数（样本不足时仍如实给出）。
    pub success_count: u32,
    /// 已入账总次数。
    pub total: u32,
    /// 成功率 ppm（total=0 时不虚报，置 0）。
    pub success_ppm: u64,
    /// 达标线（恒 9900，来源：判据 ≥99%）。
    pub threshold_ppm: u64,
    /// 是否已出结论（true=Pass/Reject，false=Inconclusive）。
    pub conclusive: bool,
}

/// 封板落簿的判定账（J1-b：判定账落簿）——ppm/结论/封板指纹三要素。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerdictRecord {
    /// 成功次数（封板时应为 ≤150）。
    pub success_count: u32,
    /// 总次数（封板时应为 150）。
    pub total: u32,
    /// 成功率 ppm。
    pub ppm: u64,
    /// 判定结论（封板时必为 Pass/Reject，不可能 Inconclusive）。
    pub verdict: Verdict,
    /// 封板指纹（判定账与账体内容的绑定锚）。
    pub seal_fp: u64,
}

// ── 守恒 / 连续性 / 健康报告 ─────────────────────────────────────────
/// 守恒审计报告：Σ分机 ≡ 总账（判据：总数守恒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConservationReport {
    /// 总账行数（直接扫行得）。
    pub total_rows: u32,
    /// Σ分机执行数。
    pub sum_machine_runs: u32,
    /// Σ分机成功数。
    pub sum_machine_ok: u32,
    /// 直接扫行成功数（应与 Σ分机成功数相等）。
    pub direct_ok: u32,
    /// 直接扫行失败数。
    pub direct_fail: u32,
    /// 失败引用条数（封板前可少于失败行数）。
    pub fail_refs: u32,
    /// 是否守恒（Σ分机执行 == 行数 且 Σ分机成功 == 直接成功数）。
    pub balanced: bool,
}

/// 轮号连续性审计报告（对接 F0683：150 行零跳号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContinuityReport {
    /// 该机已落簿最高轮号（0 表示空）。
    pub last_round: u16,
    /// 该机已落簿轮数（位图计数）。
    pub seen_rounds: u32,
    /// 位图与计数是否一致（不一致即账体被污染）。
    pub bitmap_consistent: bool,
    /// 该机缺号数（last_round - seen_rounds，封板时须为 0）。
    pub missing_rounds: u32,
}

/// 健康巡检报告（health() 产出）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 总体健康（issue_codes 为空即 true）。
    pub healthy: bool,
    /// 总账行数。
    pub row_count: u32,
    /// 已封板与否。
    pub sealed: bool,
    /// 事件账条数。
    pub event_count: u32,
    /// 失败引用条数。
    pub fail_ref_count: u32,
    /// 巡检发现问题诊断码列表（空即健康）。
    pub issue_codes: Vec<u16>,
    /// 人读摘要（中文，format! 拼装）。
    pub note: String,
}

// ── 成功率账主体 ─────────────────────────────────────────────────────
/// 成功率统计账：三机 × 50 次矩阵 = 150 次总账 + 分机三列子账。
///
/// 结构纪律：
///   * rows/fail_refs/events 为 Vec（故本类型禁 derive Copy，仅 Clone）；
///   * 机内轮号严格递增（1,2,…,50），同轮二写拒收（E_DUP_ROUND），
///     跳号拒收（E_GAP）——重复注入与缺轮注入由此天然检出；
///   * 封板（close_book）前必须：150 行齐 + 三机各 50 齐 +
///     每个失败轮挂有 F0688 归档引用 + 守恒审计过。
#[derive(Clone, Debug)]
pub struct SuccessRateBook {
    /// 总账行（≤150，落簿序）。
    rows: Vec<LedgerRow>,
    /// 分机成功数（三列子账之一）。
    machine_ok: [u32; MACHINE_COUNT],
    /// 分机执行数（三列子账之二）。
    machine_runs: [u32; MACHINE_COUNT],
    /// 机内轮位图（三列子账之三：第 mi 机第 r-1 轮是否落簿）。
    round_seen: [[bool; ROUNDS_PER_MACHINE]; MACHINE_COUNT],
    /// 机内已落簿最高轮号（0=空，用于跳号判定）。
    machine_last_round: [u16; MACHINE_COUNT],
    /// 失败轮 → F0688 引用（只存引用不存内容）。
    fail_refs: Vec<FailRef>,
    /// 事件账（审计流）。
    events: Vec<BookEvent>,
    /// 落簿序号发生器（下一行 seq）。
    seq_next: u32,
    /// 事件序号发生器（下一事件 seq）。
    ev_seq_next: u32,
    /// 封板标志。
    sealed: bool,
    /// 封板指纹（封板时算出的账体指纹）。
    seal_fp: u64,
    /// 封板落簿判定账（J1-b）。
    verdict_rec: Option<VerdictRecord>,
}

impl SuccessRateBook {
    /// 新空账。
    pub fn new() -> Self {
        SuccessRateBook {
            rows: Vec::new(),
            machine_ok: [0u32; MACHINE_COUNT],
            machine_runs: [0u32; MACHINE_COUNT],
            round_seen: [[false; ROUNDS_PER_MACHINE]; MACHINE_COUNT],
            machine_last_round: [0u16; MACHINE_COUNT],
            fail_refs: Vec::new(),
            events: Vec::new(),
            seq_next: 1,
            ev_seq_next: 1,
            sealed: false,
            seal_fp: 0,
            verdict_rec: None,
        }
    }

    // ── 只读视图 ──

    /// 总账行数。
    pub fn row_count(&self) -> u32 {
        self.rows.len() as u32
    }

    /// 是否已封板。
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 封板指纹（未封板为 0）。
    pub const fn seal_fingerprint(&self) -> u64 {
        self.seal_fp
    }

    /// 封板落簿判定账（J1-b 只读视图）。
    pub const fn sealed_verdict(&self) -> Option<&VerdictRecord> {
        self.verdict_rec.as_ref()
    }

    /// 总账行切片（只读，落簿序）。
    pub fn rows(&self) -> &[LedgerRow] {
        &self.rows
    }

    /// 按序号取行。
    pub fn row_at(&self, index: usize) -> Option<LedgerRow> {
        if index < self.rows.len() {
            Some(self.rows[index])
        } else {
            None
        }
    }

    /// 按机号+轮号查行（线性扫，账体 ≤150 行，代价有界）。
    pub fn row_for(&self, machine: u8, round: u16) -> Option<LedgerRow> {
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r = self.rows[i];
            if r.machine == machine && r.round == round {
                return Some(r);
            }
            i += 1;
        }
        None
    }

    /// 事件账切片。
    pub fn events(&self) -> &[BookEvent] {
        &self.events
    }

    /// 事件账条数。
    pub fn event_count(&self) -> u32 {
        self.events.len() as u32
    }

    /// 失败引用切片（只读）。
    pub fn fail_refs(&self) -> &[FailRef] {
        &self.fail_refs
    }

    /// 失败引用条数。
    pub fn fail_ref_count(&self) -> u32 {
        self.fail_refs.len() as u32
    }

    /// 全账成功数（分机子账求和，saturating 防御）。
    pub fn success_count(&self) -> u32 {
        let mut s: u32 = 0;
        let mut m: usize = 0;
        while m < MACHINE_COUNT {
            s = s.saturating_add(self.machine_ok[m]);
            m += 1;
        }
        s
    }

    /// 全账失败数。
    pub fn fail_count(&self) -> u32 {
        self.row_count().saturating_sub(self.success_count())
    }

    /// 全账成功率 ppm（0 行时返回 0，不虚报；判定结论见 judge()）。
    pub fn success_ppm(&self) -> u64 {
        ppm_of(self.success_count(), self.row_count())
    }

    // ── 事件账内部写 ──

    fn push_event(&mut self, kind: EventKind, machine: u8, round: u16, code: u16) {
        let ev = BookEvent {
            seq: self.ev_seq_next,
            kind,
            machine,
            round,
            code,
        };
        self.ev_seq_next = self.ev_seq_next.saturating_add(1);
        self.events.push(ev);
    }

    // ── 落簿主通道 ──

    /// 登记一轮结果（闭集值入口）。
    ///
    /// 纪律：机内轮号必须严格递增——
    ///   * round ≤ 该机已落最高轮 → DuplicateRound（重复注入检出）；
    ///   * round > 最高+1 → GapDetected（缺轮注入检出）；
    ///   * round == 最高+1 → 落簿。
    /// 返回落簿行的全账连续 seq（1..=150）。
    pub fn record(
        &mut self,
        machine: u8,
        round: u16,
        outcome: RunOutcome,
    ) -> Result<u32, SuccRateError> {
        if self.sealed {
            self.push_event(EventKind::RowRejected, machine, round, E_SEALED);
            return Err(SuccRateError::Sealed);
        }
        if machine as usize >= MACHINE_COUNT {
            self.push_event(EventKind::RowRejected, machine, round, E_MACHINE_RANGE);
            return Err(SuccRateError::MachineOutOfRange { got: machine });
        }
        if round == 0 || round as usize > ROUNDS_PER_MACHINE {
            self.push_event(EventKind::RowRejected, machine, round, E_ROUND_RANGE);
            return Err(SuccRateError::RoundOutOfRange { got: round });
        }
        let mi = machine as usize;
        let last = self.machine_last_round[mi];
        if round <= last {
            self.push_event(EventKind::RowRejected, machine, round, E_DUP_ROUND);
            return Err(SuccRateError::DuplicateRound { machine, round });
        }
        if round != last.saturating_add(1) {
            self.push_event(EventKind::RowRejected, machine, round, E_GAP);
            return Err(SuccRateError::GapDetected {
                machine,
                expected: last.saturating_add(1),
                got: round,
            });
        }
        // 落簿：行 + 分机三列子账同步更新（溢出显性 saturating）。
        let seq = self.seq_next;
        let row = LedgerRow {
            machine,
            round,
            outcome,
            seq,
        };
        self.seq_next = self.seq_next.saturating_add(1);
        self.rows.push(row);
        self.machine_runs[mi] = self.machine_runs[mi].saturating_add(1);
        self.machine_ok[mi] = self.machine_ok[mi].saturating_add(outcome.as_bit());
        self.round_seen[mi][(round - 1) as usize] = true;
        self.machine_last_round[mi] = round;
        self.push_event(EventKind::RowRecorded, machine, round, 0);
        Ok(seq)
    }

    /// 登记一轮结果（原始字节入口）：先过灰区闸（from_raw），再走 record。
    /// 外部采集器（字符串/JSON/寄存器读数）一律从本口入账，灰区零透传。
    pub fn record_raw(
        &mut self,
        machine: u8,
        round: u16,
        raw: u8,
    ) -> Result<u32, SuccRateError> {
        let outcome = RunOutcome::from_raw(raw)?;
        self.record(machine, round, outcome)
    }

    /// 该机已落簿最高轮号（0=空）。
    pub fn machine_last_round(&self, machine: u8) -> Result<u16, SuccRateError> {
        if machine as usize >= MACHINE_COUNT {
            return Err(SuccRateError::MachineOutOfRange { got: machine });
        }
        Ok(self.machine_last_round[machine as usize])
    }

    // ── 失败引用通道（只存引用不存内容，内容归 F0688） ──

    /// 给失败轮挂 F0688 归档引用号。
    /// 校验：机/轮界内 + 该轮已落簿且为 Fail + 引用非空且 ≤64 字节 +
    /// 同轮未挂过 + 同串未挂过（引用号唯一性）。
    pub fn attach_fail_ref(
        &mut self,
        machine: u8,
        round: u16,
        archive_ref: &str,
    ) -> Result<(), SuccRateError> {
        if self.sealed {
            self.push_event(EventKind::FailRefRejected, machine, round, E_SEALED);
            return Err(SuccRateError::Sealed);
        }
        if machine as usize >= MACHINE_COUNT {
            self.push_event(EventKind::FailRefRejected, machine, round, E_MACHINE_RANGE);
            return Err(SuccRateError::MachineOutOfRange { got: machine });
        }
        if round == 0 || round as usize > ROUNDS_PER_MACHINE {
            self.push_event(EventKind::FailRefRejected, machine, round, E_ROUND_RANGE);
            return Err(SuccRateError::RoundOutOfRange { got: round });
        }
        let len = archive_ref.len();
        if len == 0 || len > FAIL_REF_MAX_LEN {
            self.push_event(EventKind::FailRefRejected, machine, round, E_REF_INVALID);
            return Err(SuccRateError::RefInvalid {
                len,
                cap: FAIL_REF_MAX_LEN,
            });
        }
        // 目标轮必须是已落簿的失败轮。
        let target = match self.row_for(machine, round) {
            Some(r) => r,
            None => {
                self.push_event(EventKind::FailRefRejected, machine, round, E_REF_TARGET);
                return Err(SuccRateError::RefTargetNotFail { machine, round });
            }
        };
        if target.outcome != RunOutcome::Fail {
            self.push_event(EventKind::FailRefRejected, machine, round, E_REF_TARGET);
            return Err(SuccRateError::RefTargetNotFail { machine, round });
        }
        // 唯一性：同轮未挂过，同串未挂过。
        let mut i: usize = 0;
        while i < self.fail_refs.len() {
            let f = &self.fail_refs[i];
            if f.machine == machine && f.round == round {
                self.push_event(EventKind::FailRefRejected, machine, round, E_REF_DUP);
                return Err(SuccRateError::FailRefDuplicate { machine, round });
            }
            if f.archive_ref.as_str() == archive_ref {
                self.push_event(EventKind::FailRefRejected, machine, round, E_REF_DUP);
                return Err(SuccRateError::FailRefDuplicate { machine, round });
            }
            i += 1;
        }
        let fr = FailRef {
            machine,
            round,
            archive_ref: String::from(archive_ref),
        };
        self.fail_refs.push(fr);
        self.push_event(EventKind::FailRefAttached, machine, round, 0);
        Ok(())
    }

    // ── 守恒 / 三列 / 连续性审计 ──

    /// 守恒审计（判据：总数守恒 Σ分机 ≡ 总数）。
    /// 双向对账：Σ分机执行数 vs 直接扫行数；Σ分机成功数 vs 直接扫行成功数。
    pub fn audit_conservation(&self) -> Result<ConservationReport, SuccRateError> {
        let mut direct_ok: u32 = 0;
        let mut direct_fail: u32 = 0;
        let mut i: usize = 0;
        while i < self.rows.len() {
            match self.rows[i].outcome {
                RunOutcome::Success => direct_ok = direct_ok.saturating_add(1),
                RunOutcome::Fail => direct_fail = direct_fail.saturating_add(1),
            }
            i += 1;
        }
        let mut sum_runs: u32 = 0;
        let mut sum_ok: u32 = 0;
        let mut m: usize = 0;
        while m < MACHINE_COUNT {
            sum_runs = sum_runs.saturating_add(self.machine_runs[m]);
            sum_ok = sum_ok.saturating_add(self.machine_ok[m]);
            m += 1;
        }
        let rep = ConservationReport {
            total_rows: self.rows.len() as u32,
            sum_machine_runs: sum_runs,
            sum_machine_ok: sum_ok,
            direct_ok,
            direct_fail,
            fail_refs: self.fail_refs.len() as u32,
            balanced: sum_runs == self.rows.len() as u32 && sum_ok == direct_ok,
        };
        if !rep.balanced {
            return Err(SuccRateError::ConservationBroken {
                sum_machine_runs: sum_runs,
                total_rows: self.rows.len() as u32,
            });
        }
        Ok(rep)
    }

    /// 单机轮号连续性审计（零跳号 / 位图-计数一致）。
    pub fn audit_machine_continuity(
        &self,
        machine: u8,
    ) -> Result<ContinuityReport, SuccRateError> {
        if machine as usize >= MACHINE_COUNT {
            return Err(SuccRateError::MachineOutOfRange { got: machine });
        }
        let mi = machine as usize;
        let mut seen: u32 = 0;
        let mut r: usize = 0;
        while r < ROUNDS_PER_MACHINE {
            if self.round_seen[mi][r] {
                seen = seen.saturating_add(1);
            }
            r += 1;
        }
        let last = self.machine_last_round[mi];
        let rep = ContinuityReport {
            last_round: last,
            seen_rounds: seen,
            bitmap_consistent: seen == self.machine_runs[mi],
            missing_rounds: (last as u32).saturating_sub(seen),
        };
        if !rep.bitmap_consistent {
            // 位图与计数失配：账体被外力污染，走守恒诊断码。
            return Err(SuccRateError::ConservationBroken {
                sum_machine_runs: seen,
                total_rows: self.machine_runs[mi],
            });
        }
        Ok(rep)
    }

    /// 分机三列快照（J1-c）。
    pub fn machine_columns(&self) -> [MachineColumn; MACHINE_COUNT] {
        let mut out = [MachineColumn {
            machine: 0,
            ok_count: 0,
            run_count: 0,
            ppm: 0,
            complete: false,
        }; MACHINE_COUNT];
        let mut m: usize = 0;
        while m < MACHINE_COUNT {
            let runs = self.machine_runs[m];
            let ok = self.machine_ok[m];
            let mut seen_all = true;
            let mut r: usize = 0;
            while r < ROUNDS_PER_MACHINE {
                if !self.round_seen[m][r] {
                    seen_all = false;
                }
                r += 1;
            }
            out[m] = MachineColumn {
                machine: m as u8,
                ok_count: ok,
                run_count: runs,
                ppm: ppm_of(ok, runs),
                complete: runs == ROUNDS_PER_MACHINE as u32 && seen_all,
            };
            m += 1;
        }
        out
    }

    /// 三列齐判据：三机各 50 次齐且位图全亮。
    pub fn three_columns_complete(&self) -> bool {
        let cols = self.machine_columns();
        let mut m: usize = 0;
        while m < MACHINE_COUNT {
            if !cols[m].complete {
                return false;
            }
            m += 1;
        }
        true
    }

    // ── 封板（J1-b：判定账落簿的前置闸） ──

    /// 封板：150 行齐 + 三机各 50 齐 + 失败轮引用全覆盖 + 守恒审计，
    /// 全过才落封板指纹与判定账（VerdictRecord）。
    /// 样本不足一律 E_NOTFULL 拒封板——Inconclusive 结论只在未封板 judge() 里出。
    pub fn close_book(&mut self) -> Result<VerdictRecord, SuccRateError> {
        if self.sealed {
            return Err(SuccRateError::Sealed);
        }
        if self.rows.len() != TOTAL_RUNS {
            self.push_event(
                EventKind::RowRejected,
                0xFF,
                0,
                E_NOTFULL,
            );
            return Err(SuccRateError::NotFullyPopulated {
                have: self.rows.len() as u32,
                need: TOTAL_RUNS as u32,
            });
        }
        let mut m: usize = 0;
        while m < MACHINE_COUNT {
            if self.machine_runs[m] != ROUNDS_PER_MACHINE as u32 {
                self.push_event(
                    EventKind::RowRejected,
                    m as u8,
                    self.machine_last_round[m],
                    E_MACHINE_INCOMPLETE,
                );
                return Err(SuccRateError::MachineIncomplete {
                    machine: m as u8,
                    have: self.machine_runs[m],
                    need: ROUNDS_PER_MACHINE as u32,
                });
            }
            m += 1;
        }
        // 每个失败轮必须有 F0688 引用（缺一件即拒封板）。
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r = self.rows[i];
            if r.outcome == RunOutcome::Fail && !self.has_fail_ref(r.machine, r.round) {
                self.push_event(EventKind::FailRefRejected, r.machine, r.round, E_REF_MISSING);
                return Err(SuccRateError::FailRefMissing {
                    machine: r.machine,
                    round: r.round,
                });
            }
            i += 1;
        }
        // 守恒终审。
        self.audit_conservation()?;
        // 封板指纹 + 判定账落簿。
        let fp = self.fingerprint();
        let ok = self.success_count();
        let ppm = ppm_of(ok, self.rows.len() as u32);
        let verdict = if ppm >= PASS_PPM {
            Verdict::Pass
        } else {
            Verdict::Reject
        };
        let rec = VerdictRecord {
            success_count: ok,
            total: self.rows.len() as u32,
            ppm,
            verdict,
            seal_fp: fp,
        };
        self.sealed = true;
        self.seal_fp = fp;
        self.verdict_rec = Some(rec);
        self.push_event(EventKind::Sealed, 0xFF, 0, 0);
        self.push_event(EventKind::VerdictWritten, 0xFF, 0, rec.verdict.as_raw() as u16);
        Ok(rec)
    }

    /// 是否已给指定失败轮挂过引用（内部/测试共用）。
    fn has_fail_ref(&self, machine: u8, round: u16) -> bool {
        let mut i: usize = 0;
        while i < self.fail_refs.len() {
            let f = &self.fail_refs[i];
            if f.machine == machine && f.round == round {
                return true;
            }
            i += 1;
        }
        false
    }

    // ── 判定（J1-a：ppm 整数制二态） ──

    /// 判定：成功率 = 成功×10000/150 ppm；≥9900 → Pass，否则 Reject；
    /// 样本不足 150 → Inconclusive（拒出结论，不虚报达标位）。
    /// 判定本身不写账（落簿判定账在 close_book），本口可重复调用零副作用。
    pub fn judge(&self) -> Judgement {
        let total = self.row_count();
        let ok = self.success_count();
        if total < TOTAL_RUNS as u32 {
            return Judgement {
                verdict: Verdict::Inconclusive,
                success_count: ok,
                total,
                success_ppm: ppm_of(ok, total),
                threshold_ppm: PASS_PPM,
                conclusive: false,
            };
        }
        let ppm = ppm_of(ok, total);
        let verdict = if ppm >= PASS_PPM {
            Verdict::Pass
        } else {
            Verdict::Reject
        };
        Judgement {
            verdict,
            success_count: ok,
            total,
            success_ppm: ppm,
            threshold_ppm: PASS_PPM,
            conclusive: true,
        }
    }

    // ── 指纹（双跑确定性对拍 / 封板绑定） ──

    /// 账体指纹：FNV-1a over [域分隔 | 行数 | 全部行规范序 | 引用数 | 全部引用规范序]。
    /// 行序 = 落簿序（Vec 插入序），双跑同输入必同序 → 指纹必等。
    pub fn fingerprint(&self) -> u64 {
        let mut h = Fnv64::new();
        h.write_bytes(b"UNX-F0687:v1");
        h.write_u32(self.rows.len() as u32);
        let mut i: usize = 0;
        while i < self.rows.len() {
            self.rows[i].write_fingerprint(&mut h);
            i += 1;
        }
        h.write_u32(self.fail_refs.len() as u32);
        let mut j: usize = 0;
        while j < self.fail_refs.len() {
            self.fail_refs[j].write_fingerprint(&mut h);
            j += 1;
        }
        h.finish()
    }

    /// 单机哈希链：FNV over 该机全部行（机号域分隔 + 轮号 + 结果 + seq）。
    /// 三链独立，支持分机列级篡改定位。
    pub fn machine_chain(&self, machine: u8) -> Result<u64, SuccRateError> {
        if machine as usize >= MACHINE_COUNT {
            return Err(SuccRateError::MachineOutOfRange { got: machine });
        }
        let mut h = Fnv64::new();
        h.write_bytes(b"UNX-F0687:machine");
        h.write_u8(machine);
        let mut i: usize = 0;
        while i < self.rows.len() {
            let r = self.rows[i];
            if r.machine == machine {
                h.write_u16(r.round);
                h.write_u8(r.outcome.as_raw());
                h.write_u32(r.seq);
            }
            i += 1;
        }
        Ok(h.finish())
    }

    // ── 健康巡检 ──

    /// 健康巡检：守恒、三机位图-计数一致、失败引用与失败行对账、
    /// 封板态一致（封板必有指纹与判定账）、事件账序号连续。
    /// 巡检零副作用除事件账追加一条 HealthChecked（审计留痕）。
    pub fn health(&mut self) -> HealthReport {
        let mut issues: Vec<u16> = Vec::new();
        // 守恒。
        if let Err(e) = self.audit_conservation() {
            issues.push(e.err_code());
        }
        // 三机位图-计数一致。
        let mut m: u8 = 0;
        while (m as usize) < MACHINE_COUNT {
            if let Err(e) = self.audit_machine_continuity(m) {
                issues.push(e.err_code());
            }
            m = m.saturating_add(1);
        }
        // 失败引用数 ≤ 失败行数（多挂即污染；少挂在未封板时合法）。
        let fail_rows = self.fail_count();
        if self.fail_refs.len() as u32 > fail_rows {
            issues.push(E_REF_DUP);
        }
        // 封板态一致。
        if self.sealed && (self.seal_fp == 0 || self.verdict_rec.is_none()) {
            issues.push(E_CONSERVATION);
        }
        if !self.sealed && (self.seal_fp != 0 || self.verdict_rec.is_some()) {
            issues.push(E_CONSERVATION);
        }
        // 事件账序号连续（1..=n）。
        let mut k: usize = 0;
        while k < self.events.len() {
            if self.events[k].seq != (k as u32).saturating_add(1) {
                issues.push(E_CONSERVATION);
            }
            k += 1;
        }
        // 落簿 seq 连续（1..=rows）。
        let mut r: usize = 0;
        while r < self.rows.len() {
            if self.rows[r].seq != (r as u32).saturating_add(1) {
                issues.push(E_CONSERVATION);
            }
            r += 1;
        }
        let healthy = issues.is_empty();
        let note = format!(
            "UNX-F0687 健康巡检：行数 {}，成功 {}，失败引用 {}，封板 {}，问题码 {} 项",
            self.rows.len(),
            self.success_count(),
            self.fail_refs.len(),
            self.sealed as u8,
            issues.len(),
        );
        let rep = HealthReport {
            healthy,
            row_count: self.rows.len() as u32,
            sealed: self.sealed,
            event_count: self.events.len() as u32,
            fail_ref_count: self.fail_refs.len() as u32,
            issue_codes: issues,
            note,
        };
        self.push_event(EventKind::HealthChecked, 0xFF, 0, rep.healthy as u16);
        rep
    }
}

// ── ppm 整数运算（零浮点） ───────────────────────────────────────────
/// 成功率 ppm = ok × 10000 / total（u64 域，saturating 乘、total=0 归 0）。
/// 来源：判据「成功率统计（成功/150）≥99%」的 ppm 整数制口径。
pub fn ppm_of(ok: u32, total: u32) -> u64 {
    if total == 0 {
        return 0;
    }
    (ok as u64).saturating_mul(PPM_SCALE) / (total as u64)
}

/// 达标二态判定：ppm ≥ 9900 即达标（≥ 语义，边界 9900 恰达标）。
pub const fn ppm_passes(ppm: u64) -> bool {
    ppm >= PASS_PPM
}

// ── F0682 轮排对接：程序化 150 槽位序 ────────────────────────────────
/// 单个轮排槽位（机号 + 轮号），无结果——结果由采集流填入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunSlot {
    /// 机号 0/1/2。
    pub machine: u8,
    /// 轮号 1..=50。
    pub round: u16,
}

/// 程序化生成规范轮排：机主序（m0 全 50 轮 → m1 → m2）。
/// const fn 纪律：while + 索引，零 for / 零 Vec / 零 PartialEq 比较。
/// 来源：判据「3 机 × 50 次 = 150 次序」（F0682-J1 口径）。
pub const fn canonical_stream() -> [RunSlot; TOTAL_RUNS] {
    let mut out = [RunSlot {
        machine: 0,
        round: 0,
    }; TOTAL_RUNS];
    let mut idx: usize = 0;
    let mut m: usize = 0;
    while m < MACHINE_COUNT {
        let mut r: u16 = 1;
        while r <= ROUNDS_PER_MACHINE as u16 {
            out[idx] = RunSlot {
                machine: m as u8,
                round: r,
            };
            idx += 1;
            r += 1;
        }
        m += 1;
    }
    out
}

/// 采集输入：轮排槽位 + 原始结果字节（必过灰区闸）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunInput {
    /// 机号 0/1/2。
    pub machine: u8,
    /// 轮号 1..=50。
    pub round: u16,
    /// 原始结果字节：0=Fail，1=Success，其余灰区（入账时拦截）。
    pub raw: u8,
}

/// 规范全成功流（150 槽全 raw=1）——基准流，测试与双跑对拍共用。
pub fn plan_inputs_all_success() -> Vec<RunInput> {
    let slots = canonical_stream();
    let mut out: Vec<RunInput> = Vec::new();
    let mut i: usize = 0;
    while i < TOTAL_RUNS {
        out.push(RunInput {
            machine: slots[i].machine,
            round: slots[i].round,
            raw: RAW_SUCCESS,
        });
        i += 1;
    }
    out
}

/// 规范流 + 指定失败槽位（其余全成功）。
/// 失败槽位须为合法轮排槽（机/轮界内），越界槽位拒绝。
pub fn plan_inputs_with_fails(fails: &[(u8, u16)]) -> Result<Vec<RunInput>, SuccRateError> {
    let mut out = plan_inputs_all_success();
    let mut k: usize = 0;
    while k < fails.len() {
        let (m, r) = fails[k];
        if m as usize >= MACHINE_COUNT {
            return Err(SuccRateError::MachineOutOfRange { got: m });
        }
        if r == 0 || r as usize > ROUNDS_PER_MACHINE {
            return Err(SuccRateError::RoundOutOfRange { got: r });
        }
        // 线性定位该槽位并翻转结果。
        let mut i: usize = 0;
        let mut hit = false;
        while i < out.len() {
            if out[i].machine == m && out[i].round == r {
                out[i].raw = RAW_FAIL;
                hit = true;
            }
            i += 1;
        }
        // 轮排内必含该槽位（canonical_stream 全覆盖），hit 恒真；
        // 若未来轮排生成器改动导致缺席，此处显性拒绝而非静默。
        if !hit {
            return Err(SuccRateError::PlanMismatch {
                index: k,
                machine: m,
                round: r,
            });
        }
        k += 1;
    }
    Ok(out)
}

/// 流对拍：采集流必须逐槽吻合 F0682 规范轮排（机号/轮号/长度三面）。
/// 长度不符走 NotFullyPopulated（have/need），槽位不符走 PlanMismatch。
pub fn verify_stream_matches_plan(stream: &[RunInput]) -> Result<(), SuccRateError> {
    if stream.len() != TOTAL_RUNS {
        return Err(SuccRateError::NotFullyPopulated {
            have: stream.len() as u32,
            need: TOTAL_RUNS as u32,
        });
    }
    let plan = canonical_stream();
    let mut i: usize = 0;
    while i < TOTAL_RUNS {
        if stream[i].machine != plan[i].machine || stream[i].round != plan[i].round {
            return Err(SuccRateError::PlanMismatch {
                index: i,
                machine: stream[i].machine,
                round: stream[i].round,
            });
        }
        i += 1;
    }
    Ok(())
}

// ── 建账（流 → 账） ──────────────────────────────────────────────────
/// 从采集流建账：逐条 record_raw（灰区闸内建），流须吻合 F0682 轮排。
/// 建账成功返回未封板账（封板由 close_book 显性执行）。
pub fn build_book(stream: &[RunInput]) -> Result<SuccessRateBook, SuccRateError> {
    verify_stream_matches_plan(stream)?;
    let mut book = SuccessRateBook::new();
    let mut i: usize = 0;
    while i < stream.len() {
        let s = stream[i];
        book.record_raw(s.machine, s.round, s.raw)?;
        i += 1;
    }
    Ok(book)
}

/// 从采集流前 take 条建账（注入面用：半满账激发缺轮/重复/灰区路径）。
pub fn build_book_prefix(stream: &[RunInput], take: usize) -> Result<SuccessRateBook, SuccRateError> {
    if take > stream.len() {
        return Err(SuccRateError::NotFullyPopulated {
            have: stream.len() as u32,
            need: take as u32,
        });
    }
    let mut book = SuccessRateBook::new();
    let mut i: usize = 0;
    while i < take {
        let s = stream[i];
        book.record_raw(s.machine, s.round, s.raw)?;
        i += 1;
    }
    Ok(book)
}

// ── 失败引用规格（建账侧输入） ───────────────────────────────────────
/// 失败引用挂账规格：建账 API 用（与 FailRef 的区别：此处是输入侧）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailRefSpec {
    /// 失败轮机号。
    pub machine: u8,
    /// 失败轮轮号。
    pub round: u16,
    /// F0688 归档件引用号（≤64 字节）。
    pub ref_no: String,
}

impl FailRefSpec {
    /// 便捷构造（String::from 拷入，零借用悬挂）。
    pub fn new(machine: u8, round: u16, ref_no: &str) -> Self {
        FailRefSpec {
            machine,
            round,
            ref_no: String::from(ref_no),
        }
    }
}

/// 给账批量挂失败引用：逐条走 attach_fail_ref（全部校验过闸）。
pub fn attach_fail_refs(
    book: &mut SuccessRateBook,
    specs: &[FailRefSpec],
) -> Result<(), SuccRateError> {
    let mut i: usize = 0;
    while i < specs.len() {
        let s = &specs[i];
        book.attach_fail_ref(s.machine, s.round, s.ref_no.as_str())?;
        i += 1;
    }
    Ok(())
}

/// 自动生成全部失败轮的默认引用号：格式 "F0688-ARC-<seq 三位>"，
/// 引用号由落簿 seq 派生（确定性，双跑一致）。
pub fn auto_fail_ref_specs(stream: &[RunInput]) -> Result<Vec<FailRefSpec>, SuccRateError> {
    verify_stream_matches_plan(stream)?;
    let mut out: Vec<FailRefSpec> = Vec::new();
    let mut i: usize = 0;
    while i < stream.len() {
        if stream[i].raw == RAW_FAIL {
            let spec = FailRefSpec::new(
                stream[i].machine,
                stream[i].round,
                &format!("F0688-ARC-{:03}", i + 1),
            );
            out.push(spec);
        }
        i += 1;
    }
    Ok(out)
}

// ── 双跑确定性对拍（F0696 幂等口径） ─────────────────────────────────
/// 双跑对拍报告：两账指纹/行数/判定逐面比对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualRunReport {
    /// A 账指纹。
    pub fp_a: u64,
    /// B 账指纹。
    pub fp_b: u64,
    /// A 账行数。
    pub rows_a: u32,
    /// B 账行数。
    pub rows_b: u32,
    /// A 账判定 ppm（未封板时为账面 ppm）。
    pub ppm_a: u64,
    /// B 账判定 ppm。
    pub ppm_b: u64,
    /// 两账全等（指纹+行数+ppm 三面齐）。
    pub identical: bool,
}

impl DualRunReport {
    /// 对拍不齐时显性报错（幂等破须人工介入）。
    pub fn ensure_identical(&self) -> Result<(), SuccRateError> {
        if self.identical {
            Ok(())
        } else {
            Err(SuccRateError::ReplayMismatch {
                fp_a: self.fp_a,
                fp_b: self.fp_b,
            })
        }
    }
}

/// 双跑对拍（未封板面）：同输入流独立建两账，指纹/行数/ppm 三面全等。
pub fn dual_run_replay(stream: &[RunInput]) -> Result<DualRunReport, SuccRateError> {
    let a = build_book(stream)?;
    let b = build_book(stream)?;
    Ok(compare_books(&a, &b))
}

/// 双跑对拍（封板面）：挂引用 → 双账各自封板 → 指纹/判定账全等。
/// 判据 F0696-J1「同档复跑结果全等（成功率/分类分布/复测结论三面 diff 零漂移）」。
pub fn dual_run_full(
    stream: &[RunInput],
    refs: &[FailRefSpec],
) -> Result<DualRunReport, SuccRateError> {
    let mut a = build_book(stream)?;
    let mut b = build_book(stream)?;
    attach_fail_refs(&mut a, refs)?;
    attach_fail_refs(&mut b, refs)?;
    let ra = a.close_book()?;
    let rb = b.close_book()?;
    let mut rep = compare_books(&a, &b);
    // 封板面加第四面：判定账全等（ppm/结论/成功数）。
    let verdict_equal = ra == rb;
    if !verdict_equal {
        rep.identical = false;
    }
    Ok(rep)
}

/// 两账三面对拍（指纹/行数/ppm）。
fn compare_books(a: &SuccessRateBook, b: &SuccessRateBook) -> DualRunReport {
    let fp_a = a.fingerprint();
    let fp_b = b.fingerprint();
    let rows_a = a.row_count();
    let rows_b = b.row_count();
    let ppm_a = a.success_ppm();
    let ppm_b = b.success_ppm();
    DualRunReport {
        fp_a,
        fp_b,
        rows_a,
        rows_b,
        ppm_a,
        ppm_b,
        identical: fp_a == fp_b && rows_a == rows_b && ppm_a == ppm_b,
    }
}

// ── 注入面（判据：重复注入/缺轮注入/灰区注入各检出） ────────────────
/// 注入探针种类闭集（七类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectionKind {
    /// 同轮号重复登记注入。
    DuplicateRound,
    /// 机内跳号（缺轮）注入。
    MissingRound,
    /// 灰区结果注入（raw ∉ {0,1}）。
    GrayZone,
    /// 机号越界注入。
    MachineOutOfRange,
    /// 轮号越界注入。
    RoundOutOfRange,
    /// 引用挂到非失败轮注入。
    RefOnSuccess,
    /// 封板后写入注入。
    SealedWrite,
}

impl InjectionKind {
    /// 种类中文名（报告用）。
    pub const fn name(self) -> &'static str {
        match self {
            InjectionKind::DuplicateRound => "同轮重复注入",
            InjectionKind::MissingRound => "缺轮跳号注入",
            InjectionKind::GrayZone => "灰区结果注入",
            InjectionKind::MachineOutOfRange => "机号越界注入",
            InjectionKind::RoundOutOfRange => "轮号越界注入",
            InjectionKind::RefOnSuccess => "非失败轮挂引用注入",
            InjectionKind::SealedWrite => "封板后写入注入",
        }
    }
}

/// 单次注入探针结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectionProbe {
    /// 注入种类。
    pub kind: InjectionKind,
    /// 是否被检出（错误码与预期码吻合）。
    pub detected: bool,
    /// 实得诊断码（注入被拒时的错误码；意外通过为 0）。
    pub code: u16,
    /// 诊断文案。
    pub detail: &'static str,
}

/// 单次注入探测：对账施放一类注入，验其被正确拦截。
/// 纪律：探测只对可变账操作，账为克隆体，原始账零污染。
pub fn inject_probe(book: &mut SuccessRateBook, kind: InjectionKind) -> InjectionProbe {
    let expected: u16 = match kind {
        InjectionKind::DuplicateRound => E_DUP_ROUND,
        InjectionKind::MissingRound => E_GAP,
        InjectionKind::GrayZone => E_GRAY,
        InjectionKind::MachineOutOfRange => E_MACHINE_RANGE,
        InjectionKind::RoundOutOfRange => E_ROUND_RANGE,
        InjectionKind::RefOnSuccess => E_REF_TARGET,
        InjectionKind::SealedWrite => E_SEALED,
    };
    let got: u16 = match kind {
        InjectionKind::DuplicateRound => {
            // 找一机有落簿者，重放其最高轮。
            let mut m: u8 = 0;
            let mut picked: (u8, u16) = (0, 0);
            while (m as usize) < MACHINE_COUNT {
                if let Ok(last) = book.machine_last_round(m) {
                    if last > 0 {
                        picked = (m, last);
                    }
                }
                m = m.saturating_add(1);
            }
            let (m, r) = picked;
            match book.record(m, r, RunOutcome::Success) {
                Err(e) => e.err_code(),
                Ok(_) => 0,
            }
        }
        InjectionKind::MissingRound => {
            // 找一机有落簿者，跳一号写。
            let mut m: u8 = 0;
            let mut picked: (u8, u16) = (0, 0);
            while (m as usize) < MACHINE_COUNT {
                if let Ok(last) = book.machine_last_round(m) {
                    if last > 0 {
                        picked = (m, last);
                    }
                }
                m = m.saturating_add(1);
            }
            let (m, r) = picked;
            match book.record(m, r.saturating_add(2), RunOutcome::Success) {
                Err(e) => e.err_code(),
                Ok(_) => 0,
            }
        }
        InjectionKind::GrayZone => {
            match book.record_raw(0, 1, 0x2A) {
                Err(e) => e.err_code(),
                Ok(_) => 0,
            }
        }
        InjectionKind::MachineOutOfRange => {
            match book.record(0xB1, 1, RunOutcome::Success) {
                Err(e) => e.err_code(),
                Ok(_) => 0,
            }
        }
        InjectionKind::RoundOutOfRange => {
            match book.record(0, ROUNDS_PER_MACHINE as u16 + 1, RunOutcome::Success) {
                Err(e) => e.err_code(),
                Ok(_) => 0,
            }
        }
        InjectionKind::RefOnSuccess => {
            // 找一已落簿成功轮挂引用。
            let mut i: usize = 0;
            let mut picked: (u8, u16) = (0, 0);
            let rows = book.rows();
            while i < rows.len() {
                if rows[i].outcome == RunOutcome::Success {
                    picked = (rows[i].machine, rows[i].round);
                }
                i += 1;
            }
            let (m, r) = picked;
            match book.attach_fail_ref(m, r, "F0688-ARC-INJ") {
                Err(e) => e.err_code(),
                Ok(_) => 0,
            }
        }
        InjectionKind::SealedWrite => {
            match book.record(0, 1, RunOutcome::Success) {
                Err(e) => e.err_code(),
                Ok(_) => 0,
            }
        }
    };
    InjectionProbe {
        kind,
        detected: got == expected,
        code: got,
        detail: diag_detail(if got == expected { expected } else { 0 }),
    }
}

/// 注入矩阵全绿探针：七类注入各施放一次，全检出才返回 Ok。
/// 判据：注入检出 100%（重复/缺轮/灰区三面为 J1 直接要求，另四面为账体防线）。
pub fn injection_matrix() -> Result<Vec<InjectionProbe>, SuccRateError> {
    let mut probes: Vec<InjectionProbe> = Vec::new();
    // 半满账：前 10 槽（m0 r1..r10）激发前六类。
    let stream = plan_inputs_all_success();
    let mut partial = build_book_prefix(&stream, 10)?;
    let kinds_a = [
        InjectionKind::DuplicateRound,
        InjectionKind::MissingRound,
        InjectionKind::GrayZone,
        InjectionKind::MachineOutOfRange,
        InjectionKind::RoundOutOfRange,
        InjectionKind::RefOnSuccess,
    ];
    let mut k: usize = 0;
    while k < kinds_a.len() {
        probes.push(inject_probe(&mut partial, kinds_a[k]));
        k += 1;
    }
    // 封板账：全成功流 + 自动引用 → 封板 → 第七类。
    let refs = auto_fail_ref_specs(&stream)?;
    let _ = refs; // 全成功流无失败轮，refs 恒空
    let mut full = build_book(&stream)?;
    full.close_book()?;
    probes.push(inject_probe(&mut full, InjectionKind::SealedWrite));
    // 全绿验收。
    let mut i: usize = 0;
    while i < probes.len() {
        if !probes[i].detected {
            return Err(SuccRateError::PlanMismatch {
                index: i,
                machine: 0xFF,
                round: probes[i].code,
            });
        }
        i += 1;
    }
    Ok(probes)
}

// ── 人读格式化（alloc::format 拼装，零浮点） ────────────────────────
/// 判定快照 → 中文行（ppm 定点四位数呈现，如 9933ppm=99.33%）。
pub fn format_judgement(j: &Judgement) -> String {
    let verdict = match j.verdict {
        Verdict::Pass => "达标",
        Verdict::Reject => "不达标",
        Verdict::Inconclusive => "样本不足，拒出结论",
    };
    // ppm→百分数：1% = 100ppm，整数部分 ppm/100，两位小数 ppm%100。
    let whole = j.success_ppm / 100;
    let frac = j.success_ppm % 100;
    format!(
        "UNX-F0687 判定：{}（成功 {}/{}，{}.{:02}%，线 {}.{:02}%）",
        verdict,
        j.success_count,
        j.total,
        whole,
        frac,
        j.threshold_ppm / 100,
        j.threshold_ppm % 100,
    )
}

/// 三机三列 → 中文三行。
pub fn format_columns(cols: &[MachineColumn; MACHINE_COUNT]) -> String {
    let mut out = String::new();
    let mut m: usize = 0;
    while m < MACHINE_COUNT {
        let c = &cols[m];
        let whole = c.ppm / 100;
        let frac = c.ppm % 100;
        out.push_str(&format!(
            "机{}：成功 {}/{} 次，{}.{:02}%，列齐 {}\n",
            c.machine, c.ok_count, c.run_count, whole, frac, c.complete as u8,
        ));
        m += 1;
    }
    out
}

/// 健康报告 → 中文行。
pub fn format_health(h: &HealthReport) -> String {
    let mut codes = String::new();
    let mut i: usize = 0;
    while i < h.issue_codes.len() {
        if i > 0 {
            codes.push_str(",");
        }
        codes.push_str(&format!("0x{:04X}", h.issue_codes[i]));
        i += 1;
    }
    format!("{}\n问题码：[{}]", h.note, codes)
}

/// 封板判定账 → 中文行（判定账落簿凭据）。
pub fn format_verdict_record(rec: &VerdictRecord) -> String {
    let whole = rec.ppm / 100;
    let frac = rec.ppm % 100;
    format!(
        "判定账：成功 {}/{} = {}.{:02}%，结论 {:?}，封板指纹 0x{:016X}",
        rec.success_count,
        rec.total,
        whole,
        frac,
        rec.verdict,
        rec.seal_fp,
    )
}

// ── 冒烟测试（探针工程 vx1024_probe_unx0687 下编译运行） ────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 建一账：流 + 自动引用 + 封板，返回（账，判定账）。
    fn build_sealed(fails: &[(u8, u16)]) -> (SuccessRateBook, VerdictRecord) {
        let stream = plan_inputs_with_fails(fails).unwrap_or_default();
        let refs = auto_fail_ref_specs(&stream).unwrap_or_default();
        let mut book = build_book(&stream).unwrap_or_else(|_| SuccessRateBook::new());
        attach_fail_refs(&mut book, &refs).unwrap_or(());
        let rec = book.close_book().unwrap_or(VerdictRecord {
            success_count: 0,
            total: 0,
            ppm: 0,
            verdict: Verdict::Inconclusive,
            seal_fp: 0,
        });
        (book, rec)
    }

    #[test]
    fn t01_full_150_conservation_and_pass() {
        let (book, rec) = build_sealed(&[]);
        assert!(book.is_sealed());
        assert_eq!(book.row_count(), 150);
        assert_eq!(rec.success_count, 150);
        assert_eq!(rec.total, 150);
        assert_eq!(rec.ppm, 10000);
        assert_eq!(rec.verdict, Verdict::Pass);
        let cons = book.audit_conservation().unwrap_or(ConservationReport {
            total_rows: 0,
            sum_machine_runs: 0,
            sum_machine_ok: 0,
            direct_ok: 0,
            direct_fail: 0,
            fail_refs: 0,
            balanced: false,
        });
        assert!(cons.balanced);
        assert_eq!(cons.total_rows, 150);
        assert_eq!(cons.sum_machine_runs, 150);
        assert!(book.three_columns_complete());
        // 事件账：150 记录 + 封板 + 判定落簿（全成功无引用事件）。
        assert_eq!(book.event_count(), 152);
    }

    #[test]
    fn t02_boundary_9900_and_reject() {
        // 148/150 = 9866ppm < 9900 → Reject。
        let (book, rec) = build_sealed(&[(0, 1), (0, 2)]);
        assert_eq!(rec.success_count, 148);
        assert_eq!(rec.ppm, 9866);
        assert_eq!(rec.verdict, Verdict::Reject);
        assert_eq!(book.fail_ref_count(), 2);
        // 149/150 = 9933ppm ≥ 9900 → Pass（边界之上，恰判 ≥ 语义）。
        let (_b2, rec2) = build_sealed(&[(1, 50)]);
        assert_eq!(rec2.ppm, 9933);
        assert_eq!(rec2.verdict, Verdict::Pass);
        // 边界精确断言：9900ppm 本身达标（148 成功 + …不可达 9900；
        // 直接用 ppm_passes 常量函数验证 ≥ 语义）。
        assert!(ppm_passes(9900));
        assert!(!ppm_passes(9899));
    }

    #[test]
    fn t03_inconclusive_when_short() {
        let stream = plan_inputs_all_success();
        let book = build_book_prefix(&stream, 149).unwrap_or_else(|_| SuccessRateBook::new());
        let j = book.judge();
        assert_eq!(j.verdict, Verdict::Inconclusive);
        assert!(!j.conclusive);
        assert_eq!(j.total, 149);
        assert_eq!(j.success_count, 149);
        // 未封板账拒 close。
        let mut book2 = build_book_prefix(&stream, 100).unwrap_or_else(|_| SuccessRateBook::new());
        assert!(book2.close_book().is_err());
    }

    #[test]
    fn t04_injections_all_detected() {
        let probes = injection_matrix().unwrap_or_default();
        assert_eq!(probes.len(), 7);
        let mut i: usize = 0;
        while i < probes.len() {
            assert!(probes[i].detected, "probe {} 未检出", i);
            i += 1;
        }
        // 三类 J1 直接要求逐条复验。
        let stream = plan_inputs_all_success();
        let mut partial = build_book_prefix(&stream, 5).unwrap_or_else(|_| SuccessRateBook::new());
        assert!(matches!(
            partial.record(0, 5, RunOutcome::Fail),
            Err(SuccRateError::DuplicateRound { .. })
        ));
        assert!(matches!(
            partial.record(0, 9, RunOutcome::Success),
            Err(SuccRateError::GapDetected { .. })
        ));
        assert!(matches!(
            partial.record_raw(0, 6, 0x7F),
            Err(SuccRateError::GrayZoneResult { raw: 0x7F })
        ));
    }

    #[test]
    fn t05_dual_run_identical_and_tamper_sensitive() {
        let stream = plan_inputs_with_fails(&[(2, 17), (1, 3)]).unwrap_or_default();
        let refs = auto_fail_ref_specs(&stream).unwrap_or_default();
        let rep = dual_run_full(&stream, &refs).unwrap_or(DualRunReport {
            fp_a: 1,
            fp_b: 2,
            rows_a: 0,
            rows_b: 0,
            ppm_a: 0,
            ppm_b: 0,
            identical: false,
        });
        assert!(rep.identical);
        rep.ensure_identical().unwrap_or(());
        assert_eq!(rep.fp_a, rep.fp_b);
        assert_eq!(rep.rows_a, 150);
        // 篡改敏感性：单槽翻转 → 指纹必变。
        let mut tampered = stream.clone();
        let mut i: usize = 0;
        let mut flipped = false;
        while i < tampered.len() && !flipped {
            if tampered[i].machine == 0 && tampered[i].round == 10 {
                tampered[i].raw = RAW_FAIL;
                flipped = true;
            }
            i += 1;
        }
        assert!(flipped);
        let a = build_book(&stream).unwrap_or_else(|_| SuccessRateBook::new());
        let b = build_book(&tampered).unwrap_or_else(|_| SuccessRateBook::new());
        assert_ne!(a.fingerprint(), b.fingerprint());
        assert_ne!(
            a.machine_chain(0).unwrap_or(0),
            b.machine_chain(0).unwrap_or(0)
        );
        // 机 1 链不受机 0 篡改影响。
        assert_eq!(
            a.machine_chain(1).unwrap_or(1),
            b.machine_chain(1).unwrap_or(2)
        );
    }

    #[test]
    fn t06_refs_missing_dup_and_targets() {
        let stream = plan_inputs_with_fails(&[(0, 7)]).unwrap_or_default();
        let mut book = build_book(&stream).unwrap_or_else(|_| SuccessRateBook::new());
        // 缺引用拒封板。
        assert!(matches!(
            book.close_book(),
            Err(SuccRateError::FailRefMissing { machine: 0, round: 7 })
        ));
        // 空引用拒。
        assert!(matches!(
            book.attach_fail_ref(0, 7, ""),
            Err(SuccRateError::RefInvalid { len: 0, .. })
        ));
        // 超长引用拒。
        let long = "X".repeat(FAIL_REF_MAX_LEN + 1);
        assert!(matches!(
            book.attach_fail_ref(0, 7, long.as_str()),
            Err(SuccRateError::RefInvalid { cap: 64, .. })
        ));
        // 挂成功轮拒。
        assert!(matches!(
            book.attach_fail_ref(0, 1, "F0688-ARC-X"),
            Err(SuccRateError::RefTargetNotFail { .. })
        ));
        // 正挂 + 重挂拒 + 同串异轮拒。
        assert!(book.attach_fail_ref(0, 7, "F0688-ARC-001").is_ok());
        assert!(matches!(
            book.attach_fail_ref(0, 7, "F0688-ARC-001"),
            Err(SuccRateError::FailRefDuplicate { .. })
        ));
        // 引用只存引用不存内容：账内仅 64 字节引用串。
        assert_eq!(book.fail_ref_count(), 1);
        assert_eq!(book.fail_refs()[0].archive_ref.as_str(), "F0688-ARC-001");
        // 补齐后封板过。
        let rec = book.close_book().unwrap_or(VerdictRecord {
            success_count: 0,
            total: 0,
            ppm: 0,
            verdict: Verdict::Inconclusive,
            seal_fp: 0,
        });
        assert_eq!(rec.verdict, Verdict::Pass);
    }

    #[test]
    fn t07_plan_stream_and_mismatch() {
        // 规范轮排：3×50=150 槽，机主序轮升序。
        let plan = canonical_stream();
        assert_eq!(plan.len(), 150);
        assert_eq!(plan[0], RunSlot { machine: 0, round: 1 });
        assert_eq!(plan[49], RunSlot { machine: 0, round: 50 });
        assert_eq!(plan[50], RunSlot { machine: 1, round: 1 });
        assert_eq!(plan[149], RunSlot { machine: 2, round: 50 });
        // 吻合流零错。
        let stream = plan_inputs_all_success();
        assert!(verify_stream_matches_plan(&stream).is_ok());
        // 长度不符。
        assert!(matches!(
            verify_stream_matches_plan(&stream[..100]),
            Err(SuccRateError::NotFullyPopulated { have: 100, need: 150 })
        ));
        // 槽位不符：交换两槽机号。
        let mut bad = stream.clone();
        bad[3].machine = 1;
        assert!(matches!(
            verify_stream_matches_plan(&bad),
            Err(SuccRateError::PlanMismatch { index: 3, .. })
        ));
        // 建账侧拒绝：轮排对拍先拦槽位偏离（m0 r5 被改写为 r9）。
        let mut gap = plan_inputs_all_success();
        gap[4].round = 9;
        assert!(matches!(
            build_book(&gap),
            Err(SuccRateError::PlanMismatch { index: 4, .. })
        ));
        // 绕开轮排直接入账时，机内跳号由 record 通道拦截。
        let mut manual = SuccessRateBook::new();
        manual.record_raw(0, 1, RAW_SUCCESS).unwrap_or(0);
        manual.record_raw(0, 2, RAW_SUCCESS).unwrap_or(0);
        assert!(matches!(
            manual.record_raw(0, 9, RAW_SUCCESS),
            Err(SuccRateError::GapDetected { machine: 0, expected: 3, got: 9 })
        ));
    }

    #[test]
    fn t08_health_and_diag_codes() {
        let (mut book, rec) = build_sealed(&[(2, 50), (2, 49)]);
        let h = book.health();
        assert!(h.healthy);
        assert_eq!(h.row_count, 150);
        assert!(h.issue_codes.is_empty());
        assert!(h.note.contains("150"));
        // 健康巡检事件已落账。
        let evs = book.events();
        assert_eq!(evs[evs.len() - 1].kind, EventKind::HealthChecked);
        // 诊断码段独占：全部码落 0x1611..0x161F。
        let mut code = SEG_BASE + 1;
        while code <= SEG_BASE + 0xF {
            let d = diag_detail(code);
            assert!(!d.contains("未知"));
            code += 1;
        }
        assert!(diag_detail(0x15C0).contains("未知"));
        assert_eq!(rec.verdict, Verdict::Reject); // 148/150 = 9866ppm
        // 二值口径：from_raw 闭集外全灰区。
        let mut raw: u16 = 2;
        while raw <= 255 {
            assert!(RunOutcome::from_raw(raw as u8).is_err());
            raw += 1;
        }
        assert!(RunOutcome::from_raw(0).unwrap_or(RunOutcome::Success) == RunOutcome::Fail);
        assert!(RunOutcome::from_raw(1).unwrap_or(RunOutcome::Fail) == RunOutcome::Success);
    }

    #[test]
    fn t09_formatting_and_fingerprint_stability() {
        let stream = plan_inputs_with_fails(&[(1, 25)]).unwrap_or_default();
        let refs = auto_fail_ref_specs(&stream).unwrap_or_default();
        let mut book = build_book(&stream).unwrap_or_else(|_| SuccessRateBook::new());
        attach_fail_refs(&mut book, &refs).unwrap_or(());
        let rec = book.close_book().unwrap_or(VerdictRecord {
            success_count: 0,
            total: 0,
            ppm: 0,
            verdict: Verdict::Inconclusive,
            seal_fp: 0,
        });
        let j = book.judge();
        let s1 = format_judgement(&j);
        assert!(s1.contains("达标"));
        assert!(s1.contains("149/150"));
        let cols = book.machine_columns();
        let s2 = format_columns(&cols);
        assert!(s2.contains("机0"));
        assert!(s2.contains("机2"));
        let s3 = format_verdict_record(&rec);
        assert!(s3.contains("9933") || s3.contains("99.33"));
        let h = book.health();
        let s4 = format_health(&h);
        assert!(s4.contains("[]"));
        // 封板指纹 == 判定账指纹 == 重算指纹。
        assert_eq!(book.seal_fingerprint(), rec.seal_fp);
        assert_eq!(book.fingerprint(), rec.seal_fp);
    }
}


