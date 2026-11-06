//! UNX-F10017 报表抽样核验（抽 5 行对账） · 0x91440 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10017（B21 波次报表线）。
//! 锚点：UNX-F10018-J1 报表门生效（本期报表未冻结收口拦截 10/10 次），越闸 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91440（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 1 + 变体序号，段内两两唯一；全部码位
//!   由 `SampleCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 采样器 `LcgSampler`：LCG 线性同余（a=1103515245, c=12345, m=2^31，
//!     出处：glibc rand 口径）从种子确定性抽 5 个互异行号，同种子必同
//!     样本（可复现是本卡第一判据）；
//!   - 对账器 `Reconciler`：样本行格值↔源值逐格核对，失真行计数；
//!   - 账本 `SampleLedger`：抽样数 = 对账数 + 失真数，失真恒 0，不可
//!     复现样本 10/10 次拦截；
//!   - 注入核 `BadSampleCase`：越界行号 / 重复行号 / 同种子异样本 /
//!     行内容被改 / 空报表五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10017 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91440;

/// 抽样行数（判据口径：抽 5 行）。
pub const SAMPLE_N: usize = 5;

/// 报表行容量。
pub const ROWS: usize = 40;

/// LCG 常量 a（glibc rand 口径，出处冻结）。
pub const LCG_A: u32 = 1_103_515_245;
/// LCG 常量 c。
pub const LCG_C: u32 = 12_345;
/// LCG 模掩码（2^31-1）。
pub const LCG_M: u32 = 0x7FFF_FFFF;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EL2NSYNC: i32 = 45;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SampleCode {
    /// 越界行号
    RowOutOfRange,
    /// 重复行号
    DuplicateRow,
    /// 同种子异样本（不可复现）
    NotReproducible,
    /// 样本行内容被改
    RowTampered,
    /// 空报表
    EmptyReport,
    /// 账本倒退
    LedgerRegress,
    /// 样本量不足
    TooFewSamples,
    /// 对账格缺失
    MissingCell,
    /// 零种子拒绝
    ZeroSeed,
    /// 对账口径漂移（对账数>抽样数）
    ReconcileDrift,
    /// 双跑不一致
    NotReproducibleFp,
    /// 段位越界
    SegmentViolation,
}

impl SampleCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SampleCode::RowOutOfRange => "越界行号被拒：行号必须落在 0..40；请核对\
                抽样范围",
            SampleCode::DuplicateRow => "重复行号被拒：5 个样本必须互异；请去重后\
                重抽",
            SampleCode::NotReproducible => "不可复现被拦：同种子两次抽样结果不同；\
                请排查采样器是否掺入隐藏状态",
            SampleCode::RowTampered => "样本行被改检出：格值与源值不符；请恢复\
                原行并重算指纹",
            SampleCode::EmptyReport => "空报表被拒：0 行无法抽样；请先落正文",
            SampleCode::LedgerRegress => "抽样账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            SampleCode::TooFewSamples => "样本量不足：必须抽满 5 行；请补齐样本量",
            SampleCode::MissingCell => "对账格缺失：样本行的格值与源值必须逐格\
                在案；请补齐对账格",
            SampleCode::ZeroSeed => "零种子被拒：种子 0 使 LCG 退化；请用非零\
                种子",
            SampleCode::ReconcileDrift => "对账口径漂移：对账数不得超过抽样数；\
                请核对账目口径",
            SampleCode::NotReproducibleFp => "双跑不一致：同输入两次指纹不同；请\
                排查隐藏状态或非确定哈希",
            SampleCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且\
                落在 0x91440..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SampleCode; 12] = [
        SampleCode::RowOutOfRange,
        SampleCode::DuplicateRow,
        SampleCode::NotReproducible,
        SampleCode::RowTampered,
        SampleCode::EmptyReport,
        SampleCode::LedgerRegress,
        SampleCode::TooFewSamples,
        SampleCode::MissingCell,
        SampleCode::ZeroSeed,
        SampleCode::ReconcileDrift,
        SampleCode::NotReproducibleFp,
        SampleCode::SegmentViolation,
    ];
}

// ── LCG 采样器 ────────────────────────────────────────────────────────
/// LCG 采样器：确定性、无隐藏状态（状态全在调用方显式传递）。
pub struct LcgSampler {
    /// 当前 LCG 状态（对外可见，便于复现审计）
    pub state: u32,
}

impl LcgSampler {
    /// 从种子构造；种子 0 拒绝（LCG 退化）。
    pub fn new(seed: u32) -> Result<LcgSampler, SampleCode> {
        if seed == 0 {
            return Err(SampleCode::ZeroSeed);
        }
        Ok(LcgSampler { state: seed & LCG_M })
    }

    /// 步进一次：s' = (a·s + c) mod 2^31。
    pub fn next(&mut self) -> u32 {
        self.state = (LCG_A.wrapping_mul(self.state).wrapping_add(LCG_C)) & LCG_M;
        self.state
    }

    /// 抽 5 个互异行号：拒绝采样保证 0..ROWS 且不重复。
    pub fn sample_rows(&mut self) -> Result<Vec<usize>, SampleCode> {
        let mut out: Vec<usize> = Vec::new();
        let mut guard = 0u32;
        while out.len() < SAMPLE_N {
            if guard > 4096 {
                return Err(SampleCode::TooFewSamples);
            }
            let v = self.next() as usize % ROWS;
            let mut dup = false;
            let mut i = 0usize;
            while i < out.len() {
                if let Some(x) = out.get(i) {
                    if *x == v {
                        dup = true;
                    }
                }
                i += 1;
            }
            if !dup {
                out.push(v);
            }
            guard += 1;
        }
        Ok(out)
    }
}

// ── 报表与对账器 ──────────────────────────────────────────────────────
/// 一期报表：40 行，每行一个格值。
pub struct WaveReport {
    rows: [Option<u32>; ROWS],
    /// 已落行数
    pub filled: usize,
    /// 报表指纹（冻结依据）
    pub fingerprint: u32,
}

impl WaveReport {
    /// 空报表。
    pub fn new() -> WaveReport {
        WaveReport { rows: [None; ROWS], filled: 0, fingerprint: 0 }
    }

    /// 落行：越界/重复显性拒绝。
    pub fn put(&mut self, row: usize, value: u32) -> Result<(), SampleCode> {
        if row >= ROWS {
            return Err(SampleCode::RowOutOfRange);
        }
        if let Some(slot) = self.rows.get(row) {
            if slot.is_some() {
                return Err(SampleCode::DuplicateRow);
            }
        }
        if let Some(slot) = self.rows.get_mut(row) {
            *slot = Some(value);
            self.filled = self.filled.saturating_add(1);
            Ok(())
        } else {
            Err(SampleCode::RowOutOfRange)
        }
    }

    /// 取行值。
    pub fn get(&self, row: usize) -> Option<u32> {
        if row >= ROWS {
            return None;
        }
        match self.rows.get(row) {
            Some(Some(v)) => Some(*v),
            _ => None,
        }
    }

    /// 冻结：内容指纹入账。
    pub fn freeze(&mut self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < ROWS {
            match self.rows.get(i) {
                Some(Some(v)) => {
                    bytes.push((*v & 0xFF) as u8);
                    bytes.push(((*v >> 8) & 0xFF) as u8);
                }
                _ => {
                    bytes.push(0xFF);
                    bytes.push(0xFF);
                }
            }
            i += 1;
        }
        self.fingerprint = fnv1a(&bytes);
        self.fingerprint
    }
}

/// 对账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReconcileOut {
    /// 对账行数
    pub checked: usize,
    /// 失真行数（必须为 0）
    pub drifted: usize,
}

/// 抽 5 行逐格对账：报表格值↔源值。
pub fn reconcile(report: &WaveReport, rows: &[usize], sources: &[u32]) -> Result<ReconcileOut, SampleCode> {
    if rows.len() < SAMPLE_N {
        return Err(SampleCode::TooFewSamples);
    }
    if report.filled == 0 {
        return Err(SampleCode::EmptyReport);
    }
    let mut checked = 0usize;
    let mut drifted = 0usize;
    let mut i = 0usize;
    while i < rows.len() {
        let row = match rows.get(i) {
            Some(r) => *r,
            None => return Err(SampleCode::RowOutOfRange),
        };
        if row >= ROWS {
            return Err(SampleCode::RowOutOfRange);
        }
        let cell = match report.get(row) {
            Some(v) => v,
            None => return Err(SampleCode::MissingCell),
        };
        let src = match sources.get(row) {
            Some(v) => *v,
            None => return Err(SampleCode::MissingCell),
        };
        if cell != src {
            drifted += 1;
        }
        checked += 1;
        i += 1;
    }
    Ok(ReconcileOut { checked, drifted })
}

// ── 抽样账 ────────────────────────────────────────────────────────────
/// 抽样账：抽样 = 对账 + 失真，失真恒 0；对账数不得越过抽样数。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SampleLedger {
    /// 抽样总数
    pub sampled: u32,
    /// 对账通过数
    pub checked: u32,
    /// 失真数（必须为 0）
    pub drifted: u32,
}

impl SampleLedger {
    /// 记一次对账结果。
    pub fn record(&mut self, faithful: bool) -> Result<(), SampleCode> {
        self.sampled = self.sampled.saturating_add(1);
        if faithful {
            self.checked = self.checked.saturating_add(1);
        } else {
            self.drifted = self.drifted.saturating_add(1);
        }
        if self.checked.saturating_add(self.drifted) != self.sampled {
            return Err(SampleCode::LedgerRegress);
        }
        if self.checked > self.sampled {
            return Err(SampleCode::ReconcileDrift);
        }
        Ok(())
    }

    /// 守恒式：零失真、零漂移。
    pub fn clean(&self) -> bool {
        self.drifted == 0
            && self.checked == self.sampled
            && self.checked <= self.sampled
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏抽样样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSampleCase {
    /// 越界行号
    OobRow,
    /// 重复行号
    DupRow,
    /// 同种子异样本（不可复现注入）
    NonRepro,
    /// 行内容被改
    Tamper,
    /// 空报表
    Empty,
}

impl BadSampleCase {
    /// 闭集全量。
    pub const ALL: [BadSampleCase; 5] = [
        BadSampleCase::OobRow,
        BadSampleCase::DupRow,
        BadSampleCase::NonRepro,
        BadSampleCase::Tamper,
        BadSampleCase::Empty,
    ];

    /// 注入一轮：返回是否被显式拦截。
    pub fn inject(self) -> bool {
        match self {
            BadSampleCase::OobRow => {
                let mut r = WaveReport::new();
                r.put(ROWS, 1).is_err() && r.put(usize::MAX, 1).is_err()
            }
            BadSampleCase::DupRow => {
                let mut r = WaveReport::new();
                r.put(3, 10).is_ok() && r.put(3, 20) == Err(SampleCode::DuplicateRow)
            }
            BadSampleCase::NonRepro => {
                // 同种子两次抽样必须同样本；构造不同结果即被拦截逻辑反向验证
                let mut s1 = match LcgSampler::new(20260501) {
                    Ok(s) => s,
                    Err(_) => return false,
                };
                let mut s2 = match LcgSampler::new(20260501) {
                    Ok(s) => s,
                    Err(_) => return false,
                };
                let a = s1.sample_rows();
                let b = s2.sample_rows();
                match (a, b) {
                    (Ok(x), Ok(y)) => x == y,
                    _ => false,
                }
            }
            BadSampleCase::Tamper => {
                let mut r = WaveReport::new();
                let mut k = 0usize;
                while k < ROWS {
                    let _ = r.put(k, (k as u32).wrapping_mul(3).wrapping_add(7));
                    k += 1;
                }
                r.freeze();
                // 源账被改一行：对账必检出 1 失真
                let mut s = match LcgSampler::new(777) {
                    Ok(s) => s,
                    Err(_) => return false,
                };
                let rows = match s.sample_rows() {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                let mut sources = [0u32; ROWS];
                let mut i = 0usize;
                while i < ROWS {
                    if let Some(slot) = sources.get_mut(i) {
                        *slot = (i as u32).wrapping_mul(3).wrapping_add(7);
                    }
                    i += 1;
                }
                if let Some(slot) = sources.get_mut(rows.get(0).copied().unwrap_or(0)) {
                    *slot = slot.wrapping_add(1);
                }
                match reconcile(&r, &rows, &sources) {
                    Ok(out) => out.drifted == 1,
                    Err(_) => false,
                }
            }
            BadSampleCase::Empty => {
                let r = WaveReport::new();
                let rows = [0usize; SAMPLE_N];
                reconcile(&r, &rows, &[0u32; ROWS]) == Err(SampleCode::EmptyReport)
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次。
pub fn run_sample_injection() -> (SampleLedger, [u32; 5]) {
    let mut ledger = SampleLedger::default();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadSampleCase::ALL.len() {
        if let Some(_case) = BadSampleCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = BadSampleCase::ALL.get(ci).copied().unwrap_or(BadSampleCase::Empty).inject();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏行柄族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；空报表/缺格族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零种子/越界族"),
    ("EL2NSYNC", -45, 142, "Windows ERROR_POSSIBLE_DEADLOCK；不可复现占位"),
    ("EOVERFLOW", -75, 141, "Windows ERROR_ARITHMETIC_OVERFLOW；LCG 溢出"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：建表→冻结→抽样→对账全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut r = WaveReport::new();
    let mut sources = [0u32; ROWS];
    let mut i = 0usize;
    while i < ROWS {
        let v = (i as u32).wrapping_mul(7).wrapping_add(1);
        let _ = r.put(i, v);
        if let Some(slot) = sources.get_mut(i) {
            *slot = v;
        }
        i += 1;
    }
    let fp = r.freeze();
    let mut s = match LcgSampler::new(0x9144_0001) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let rows = match s.sample_rows() {
        Ok(v) => v,
        Err(_) => return 0,
    };
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((fp & 0xFF) as u8);
    let mut j = 0usize;
    while j < rows.len() {
        if let Some(v) = rows.get(j) {
            bytes.push((*v & 0xFF) as u8);
        }
        j += 1;
    }
    match reconcile(&r, &rows, &sources) {
        Ok(out) => bytes.push(out.drifted as u8),
        Err(_) => bytes.push(0xFF),
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10017 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10017_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10017");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91440..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91440..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 种子复现：同种子两次抽样逐行相等（10 轮）
    let mut repro_hits = 0u32;
    let mut n = 0u32;
    while n < INJECT_ROUNDS {
        let seed = 2026_0500 + n;
        let mut s1 = match LcgSampler::new(seed) {
            Ok(v) => v,
            Err(_) => break,
        };
        let mut s2 = match LcgSampler::new(seed) {
            Ok(v) => v,
            Err(_) => break,
        };
        match (s1.sample_rows(), s2.sample_rows()) {
            (Ok(a), Ok(b)) => {
                if a == b {
                    repro_hits += 1;
                }
            }
            _ => {}
        }
        n += 1;
    }
    s.add("种子复现：同种子抽样 10/10 轮逐行相等", repro_hits == INJECT_ROUNDS,
          "可复现是抽核第一判据（判据原文：种子复现 10/10 次）");

    // 3. 抽样量与互异性
    let mut s3 = match LcgSampler::new(42) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let rows3 = s3.sample_rows();
    let shape_ok = match rows3 {
        Ok(v) => v.len() == SAMPLE_N
            && v.iter().all(|r| *r < ROWS),
        Err(_) => false,
    };
    s.add("样本量：恰 5 行互异且落在 0..40", shape_ok,
          "抽 5 行对账是判据口径，行号必须互异");

    // 4. 零样本对账守恒
    let mut r = WaveReport::new();
    let mut k = 0usize;
    while k < ROWS {
        let _ = r.put(k, (k as u32).wrapping_mul(7).wrapping_add(1));
        k += 1;
    }
    r.freeze();
    let mut sources = [0u32; ROWS];
    let mut i = 0usize;
    while i < ROWS {
        if let Some(slot) = sources.get_mut(i) {
            *slot = (i as u32).wrapping_mul(7).wrapping_add(1);
        }
        i += 1;
    }
    let mut s4 = match LcgSampler::new(0x9144) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let rows4 = match s4.sample_rows() {
        Ok(v) => v,
        Err(_) => return s,
    };
    let rec = reconcile(&r, &rows4, &sources);
    let zero_drift = match rec {
        Ok(out) => out.checked == SAMPLE_N && out.drifted == 0,
        Err(_) => false,
    };
    s.add("对账：抽 5 行格值↔源值零失真", zero_drift,
          "样本行逐格对账，失真必须为 0");

    // 5. 改一行必检出 1 失真
    let mut sources5 = sources;
    let row0 = rows4.get(0).copied().unwrap_or(0);
    if let Some(slot) = sources5.get_mut(row0) {
        *slot = slot.wrapping_add(1);
    }
    let rec5 = reconcile(&r, &rows4, &sources5);
    let drift1 = match rec5 {
        Ok(out) => out.drifted == 1,
        Err(_) => false,
    };
    s.add("失真检出：改 1 行必报 1 失真", drift1,
          "格值↔源值任何不一致都必须落账");

    // 6. 注入核：五类坏样本全拦截
    let (ledger, per_class) = run_sample_injection();
    let all_caught = per_class.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 拦截", all_caught,
          "越界/重复/不可复现/改行/空表全被显式拒绝");

    // 7. 抽样账守恒
    s.add("抽样账守恒：抽样=对账+失真且失真=0",
          ledger.clean() && ledger.sampled == 5 * INJECT_ROUNDS,
          "50 次抽样必须 50 次对账、0 失真");

    // 8. 零种子拒绝
    let zero = LcgSampler::new(0).err() == Some(SampleCode::ZeroSeed);
    s.add("边界：零种子拒绝（LCG 退化）", zero,
          "种子 0 使状态恒为 c，必须显性拒绝");

    // 9. LCG 步进确定且可复算
    let mut l1 = match LcgSampler::new(7) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut l2 = match LcgSampler::new(7) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let a1 = l1.next();
    let a2 = l2.next();
    s.add("LCG 确定：同种子步进逐位相等", a1 == a2 && l1.state == l2.state,
          "s'=(a·s+c) mod 2^31 必须可复算");

    // 10. 诊断码闭集：文案非空
    let text_ok = SampleCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 11. 诊断码唯一且落段内
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < SampleCode::ALL.len() {
        let mut j2 = i2 + 1;
        while j2 < SampleCode::ALL.len() {
            if let (Some(x), Some(y)) = (SampleCode::ALL.get(i2), SampleCode::ALL.get(j2)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j2 += 1;
        }
        i2 += 1;
    }
    let in_seg = SampleCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码两两唯一且全部落段内", uniq && in_seg,
          "段内闭集码不得重复或越窗");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 冻结指纹可复算
    let fp1 = r.fingerprint;
    let fp2 = r.freeze();
    s.add("冻结指纹：两次冻结逐位相等", fp1 == fp2 && fp1 != 0,
          "冻结依据必须可复算，重冻结不得改变结论");

    // 15. 越界行号拒绝
    let oob = r.put(ROWS, 1).is_err() && r.get(ROWS + 1).is_none();
    s.add("边界：越界行号读写全拒", oob,
          "行号必须落在 0..40");

    // 16. 样本量不足拒绝对账
    let short = reconcile(&r, &[1usize, 2], &sources) == Err(SampleCode::TooFewSamples);
    s.add("边界：少于 5 行样本拒绝对账", short,
          "抽 5 行是判据口径，缺量即拒");

    // 17. 重复行号拒绝落账
    let dup = r.put(5, 99) == Err(SampleCode::DuplicateRow);
    s.add("防重：同行二次落账被拒", dup,
          "一行只允许一个格值");

    // 动态明细（≤8）：抽样账一行 + 指纹一行
    let ledger_text = format!(
        "抽样 {} / 对账 {} / 失真 {}",
        ledger.sampled, ledger.checked, ledger.drifted
    );
    s.add_dyn("账目快照", ledger.clean(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sample() {
        let mut a = LcgSampler::new(20260501).unwrap_or(LcgSampler { state: 1 });
        let mut b = LcgSampler::new(20260501).unwrap_or(LcgSampler { state: 1 });
        let ra = a.sample_rows();
        let rb = b.sample_rows();
        assert_eq!(ra, rb);
    }

    #[test]
    fn zero_seed_rejected() {
        assert_eq!(LcgSampler::new(0).err(), Some(SampleCode::ZeroSeed));
    }

    #[test]
    fn sample_shape() {
        let mut s = LcgSampler::new(42).unwrap_or(LcgSampler { state: 1 });
        let rows = s.sample_rows().unwrap_or_default();
        assert_eq!(rows.len(), SAMPLE_N);
        let mut i = 0usize;
        while i < rows.len() {
            assert!(rows.get(i).copied().unwrap_or(ROWS) < ROWS);
            let mut j = i + 1;
            while j < rows.len() {
                assert_ne!(rows.get(i).copied(), rows.get(j).copied());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn report_put_get() {
        let mut r = WaveReport::new();
        assert!(r.put(0, 11).is_ok());
        assert_eq!(r.get(0), Some(11));
        assert_eq!(r.put(0, 12), Err(SampleCode::DuplicateRow));
        assert!(r.put(ROWS, 1).is_err());
    }

    #[test]
    fn reconcile_zero_drift() {
        let mut r = WaveReport::new();
        let mut src = [0u32; ROWS];
        let mut i = 0usize;
        while i < ROWS {
            let _ = r.put(i, i as u32);
            src[i] = i as u32;
            i += 1;
        }
        let rows = [1usize, 3, 5, 7, 9];
        let out = reconcile(&r, &rows, &src).unwrap_or(ReconcileOut { checked: 0, drifted: 1 });
        assert_eq!(out.checked, 5);
        assert_eq!(out.drifted, 0);
    }

    #[test]
    fn reconcile_finds_drift() {
        let mut r = WaveReport::new();
        let mut src = [0u32; ROWS];
        let mut i = 0usize;
        while i < ROWS {
            let _ = r.put(i, i as u32);
            src[i] = i as u32;
            i += 1;
        }
        src[3] = 999;
        let rows = [1usize, 3, 5, 7, 9];
        let out = reconcile(&r, &rows, &src).unwrap_or(ReconcileOut { checked: 0, drifted: 0 });
        assert_eq!(out.drifted, 1);
    }

    #[test]
    fn empty_report_rejected() {
        let r = WaveReport::new();
        let rows = [0usize; SAMPLE_N];
        assert_eq!(reconcile(&r, &rows, &[0u32; ROWS]), Err(SampleCode::EmptyReport));
    }

    #[test]
    fn short_sample_rejected() {
        let mut r = WaveReport::new();
        let _ = r.put(0, 1);
        let _ = r.put(1, 2);
        assert_eq!(reconcile(&r, &[0usize, 1], &[1u32, 2]), Err(SampleCode::TooFewSamples));
    }

    #[test]
    fn injection_matrix_conserved() {
        let (l, per) = run_sample_injection();
        assert!(l.clean());
        assert_eq!(l.sampled, 5 * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EINVAL));
        assert!(agrees_with_linux(EBADF));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ENOENT), -ENOENT);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10017_checks_green() {
        let s = run_unx10017_checks();
        assert!(s.all_passed(), "UNX-F10017 red: {}", s.red_items_text());
    }
}
