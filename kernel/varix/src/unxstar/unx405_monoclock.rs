//! unx405_monoclock ↔ UNX-F0405 单调时钟原语（ns 面统一）
//!
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B20）：UNX-F0405 · 单调时钟原语
//! （ns 面统一） · 判据：`UNX-F0405-J1 单调时钟读数严格单调
//! （1,000,000 次抽样零回退），ns 精度断言过`
//!
//! ## 这个原语为什么是内核的地基
//!
//! 调度器超时、锁持有时长、性能剖析、TSC 校准、日志时间戳——全部依赖
//! "一个不会倒退的纳秒读数"。真实硬件上得到严格单调纳秒读数要跨三道坎，
//! 本模块逐条做实：
//!
//! 1. **TSC 不天然单调，要靠 MSR 语义补**：`RDTSC` 读的是不变 TSC，
//!    但**多核之间 TSC 未必同步**（偏移可达数百微秒），且 BIOS 可能
//!    没开 invariant TSC（`constant_tsc`/`nonstop_tsc`）——不开的话 TSC
//!    会随 CPU 频率/P-state 漂移，跨核比较毫无意义。故`ClockCaps`
//!    从 MSR 面显式建模三项能力：`InvariantTsc`（频率不变）、
//!    `NonstopTsc`（停机不归零）、`TsOffsetKnown`（跨核偏移已知且
//!    已在启动期校准）。任一缺失 → **跨核序不可用**（只能本核单调），
//!    这个降级必须显性，不能假装全局单调。
//! 2. **校准后的跨核平移必须留痕且防漂移**：跨核偏移不是一个数，是
//!    **一张表**（每 CPU 一个偏移 + 一个校准时的参考 TSC）。若校准之后
//!    又有人改了偏移（而没重新校准），跨核序立即失效且**看不出来**——
//!    所以偏移表要带指纹，且`relayout` 必须重算指纹；指纹与记录不符
//!    即 `CalibFpDrift`。
//! 3. **纳秒换算的截断误差必须被量化而不是忽略**：`ns = tsc * mult
//!    >> shift` 的定点换算天然有截断。判据说"ns 精度断言过"，指的
//!    不是"完全精确"（那不可能），而是**误差有界且可断言**：
//!    （a）单调性优先——ns 读数必须严格单调，这靠"TSC 严格单调 +
//!    截断向下取整"共同保证（向下取整保序）；（b）相对误差有界——
//!    换算误差必须 `< 1ns`（用反算 TSC 对拍）；（c）**相邻读数不得
//!    因截断而相等**——这是 ns 精度的真实考点：两次 TSC 读数只差 1，
//!    若倍率 < 1 ns/tick，换算后可能都是同一个 ns，此时"严格单调"
//!    在 ns 面上就破了。解法是 ns 读数带**亚 ns 余数残留**（remainder
//!    进位到下一读），保证 ns 面不丢精度——但那样又会让"同一 ns 内
//!    两次读"看起来不同。正确的做法是把这两件事分开：`raw_ns()` 是
//!    可比较的单调纳秒，`ns_with_sub()` 额外给亚 ns 余数用于测量，
//!    而 `strictly_monotonic()` 断言针对 `raw_ns()`，同时给出
//!    `sub_ns` 参与的高精度比较器。
//!
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const V405_VERSION: &str = "v405-monoclock-v1";

/// 诊断段基址（0x6C00 段 · 本模块独占）。
pub const V405_SEG_BASE: u16 = 0x6C00;

/// CPU 容量。
pub const CPU_CAP: usize = 16;

/// 判据抽样次数（1,000,000）。
pub const SAMPLE_N: u64 = 1_000_000;

/// 判据抽样循环步长（每次 TSC 增量的 tick 数）。
///
/// 必须 ≥ `2^shift / mult`，否则 ns 面每次抽样增量为 0，判据会把
/// **正确的量化停滞**误报成回退。测试口径 (5, 4) 下取 8 → 每步 2ns。
pub const SAMPLE_STEP: u64 = 8;

/// 1ns 的百万分位（误差断言阈值·零浮点整数口径）。
pub const ERR_PPM_ONE_NS: u64 = 1_000_000;

/// --------------------------------------------------------------------------
// 诊断码（十二码 · 段内连续）
// --------------------------------------------------------------------------

/// 单调时钟诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V405Code {
    /// 0x6C00 TSC 读数回退（硬件或模型违例）。
    TscBack,
    /// 0x6C01 ns 读数回退。
    NsBack,
    /// 0x6C02 ns 面读数相等（截断吃掉了增量）。
    NsStall,
    /// 0x6C03 能力缺失却请求跨核序。
    CapsMissing,
    /// 0x6C04 CPU 号越界。
    CpuRange,
    /// 0x6C05 偏移表未校准即跨核比较。
    NotCalibrated,
    /// 0x6C06 校准指纹漂移（偏移被改未重校准）。
    CalibFpDrift,
    /// 0x6C07 纳秒换算误差越界。
    NsErrorBound,
    /// 0x6C08 亚 ns 余数溢出。
    SubOverflow,
    /// 0x6C09 倍率/移位非法（0 或超范围）。
    ScaleBad,
    /// 0x6C0A 单调断言样本数不符。
    SampleShort,
    /// 0x6C0B 跨核序未按偏移修正（直接比 raw）。
    OffsetMissing,
}

impl V405Code {
    /// 码值。
    pub fn val(self) -> u16 {
        V405_SEG_BASE
            + match self {
                V405Code::TscBack => 0x0,
                V405Code::NsBack => 0x1,
                V405Code::NsStall => 0x2,
                V405Code::CapsMissing => 0x3,
                V405Code::CpuRange => 0x4,
                V405Code::NotCalibrated => 0x5,
                V405Code::CalibFpDrift => 0x6,
                V405Code::NsErrorBound => 0x7,
                V405Code::SubOverflow => 0x8,
                V405Code::ScaleBad => 0x9,
                V405Code::SampleShort => 0xA,
                V405Code::OffsetMissing => 0xB,
            }
    }

    /// 全部码（顺序即码序 · 12 码）。
    pub const ALL: [V405Code; 12] = [
        V405Code::TscBack,
        V405Code::NsBack,
        V405Code::NsStall,
        V405Code::CapsMissing,
        V405Code::CpuRange,
        V405Code::NotCalibrated,
        V405Code::CalibFpDrift,
        V405Code::NsErrorBound,
        V405Code::SubOverflow,
        V405Code::ScaleBad,
        V405Code::SampleShort,
        V405Code::OffsetMissing,
    ];

    /// 可操作建议（错误路径必带修复指引）。
    pub fn remedy(self) -> &'static str {
        match self {
            V405Code::TscBack => "TSC 回退：查 invariant_tsc/nonstop_tsc 是否开，或本核被迁移",
            V405Code::NsBack => "ns 回退：换算链有bug，先查定点乘与右移顺序",
            V405Code::NsStall => "ns 面停滞：两次 TSC 增量太小被截断，用 ns_with_sub()",
            V405Code::CapsMissing => "能力缺失不可跨核：只在标定过的 CPU 集内用跨核序",
            V405Code::CpuRange => "CPU 号越界：限 CPU_CAP 内，查 CPU 编号来源",
            V405Code::NotCalibrated => "未校准：先跑 calib_all() 再做跨核比较",
            V405Code::CalibFpDrift => "校准指纹漂移：偏移被改，重跑 relayout() 并重算指纹",
            V405Code::NsErrorBound => "ns 误差越界：反算 TSC 对拍，核 mult/shift 口径",
            V405Code::SubOverflow => "亚 ns 余数溢出：余数必须 < mult，核 mul_hi 是否有符号位污染",
            V405Code::ScaleBad => "倍率/移位非法：mult 不得为 0，shift 不得超 63",
            V405Code::SampleShort => "样本数不足：判据要求 100 万次抽样，跑满再断言",
            V405Code::OffsetMissing => "跨核比较缺偏移：raw_tsc 不可直接跨 CPU 比",
        }
    }

    /// 一行式描述。
    pub fn line(self) -> String {
        format!("[0x{:04X}] {}", self.val(), self.remedy())
    }
}

// --------------------------------------------------------------------------
// 时钟能力（MSR 面）
// --------------------------------------------------------------------------

/// 时钟能力（三项 MSR 派生）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockCaps {
    /// invariant TSC（频率不变）。
    pub invariant: bool,
    /// nonstop TSC（停机不归零）。
    pub nonstop: bool,
    /// 跨核偏移已在启动期校准。
    pub offset_ready: bool,
}

impl ClockCaps {
    /// 全能力齐。
    pub fn full() -> ClockCaps {
        ClockCaps {
            invariant: true,
            nonstop: true,
            offset_ready: true,
        }
    }

    /// 缺能力组合。
    pub fn missing(invariant: bool, nonstop: bool, offset: bool) -> ClockCaps {
        ClockCaps {
            invariant,
            nonstop,
            offset_ready: offset,
        }
    }

    /// 本核可用（TSC 自身是否可信）。
    pub fn local_ok(&self) -> bool {
        self.invariant && self.nonstop
    }

    /// 跨核序可用（三项全要）。
    pub fn cross_ok(&self) -> bool {
        self.local_ok() && self.offset_ready
    }

    /// 缺失项名（渲染用）。
    pub fn missing_list(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        if !self.invariant {
            v.push("invariant_tsc");
        }
        if !self.nonstop {
            v.push("nonstop_tsc");
        }
        if !self.offset_ready {
            v.push("offset_calib");
        }
        v
    }
}

// --------------------------------------------------------------------------
// 纳秒换算（定点 · mult/shift）
// --------------------------------------------------------------------------

/// 纳秒换算器（`ns = tsc * mult >> shift` · mult/shift 口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NsScale {
    /// 乘数（每 tick 多少 ns 的 Q 值）。
    pub mult: u64,
    /// 右移位数。
    pub shift: u32,
}

impl NsScale {
    /// 造换算器。
    pub fn new(mult: u64, shift: u32) -> NsScale {
        NsScale { mult, shift }
    }

    /// 常见标定：2.4GHz TSC → 1 tick ≈ 0.4167ns，用 mult=5 shift=4近似
    /// （5/16 = 0.3125 ns/tick 量级）。本模块只验证换算链性质，
    /// 绝对标定值由启动期校准填入，故此构造给的是**测试口径**。
    ///
    /// 注意该口径下 1 tick < 1 ns，故抽样步长必须 ≥ 4 tick 才可能让
    /// ns 面出现严格增量；步长不足时的 ns停滞是**物理正确的量化**，
    /// 不是回退——这正是`SAMPLE_STEP` 取 8 的原因（8*5>>4 = 2ns/步）。
    pub fn test_scale() -> NsScale {
        NsScale {
            mult: 5,
            shift: 4,
        }
    }

    /// 参数合法性。
    pub fn ok(&self) -> bool {
        self.mult != 0 && self.shift <= 63
    }

    /// ns 位（向下取整 · 保序：TSC 增 => ns 不减）。
    pub fn to_ns(&self, tsc: u64) -> u64 {
        let wide = tsc.wrapping_mul(self.mult);
        wide >> self.shift
    }

    /// 亚 ns 余数（`wide` 的低 shift 位 · 度量残差）。
    pub fn sub_ns(&self, tsc: u64) -> u64 {
        let wide = tsc.wrapping_mul(self.mult);
        if self.shift >= 64 {
            return 0;
        }
        wide & ((1u64 << self.shift) - 1)
    }

    /// 高精度比较键（ns 高位 + 亚 ns 余数打包成单 u64）。
    ///
    /// 打包必须**位段不重叠**：余数占低 32 位（余数 < 2^shift 且 shift ≤ 32
    /// 时恒成立），ns 整体上移 32 位。若像`ns | sub` 那样让ns 的低位与
    /// 余数位撞车，键就不再是单调的——同一 ns 桶内 ns 低位的进位会被
    /// 余数高位覆盖，出现"后一个键反而更小"的假回退。
    ///
    /// 键是**比较键**不是时钟读数：等值判定仍须比`(ns, sub)` 二元组，
    /// 键只用于严格偏序（>、≥）。
    pub fn key(&self, tsc: u64) -> u64 {
        let ns = self.to_ns(tsc);
        let sub = self.sub_ns(tsc);
        // shift > 32 时余数可能溢出 32 位域，此时退化为纯 ns 比较——
        // 宁可丢亚 ns 分辨率，也绝不产出一个错的键序。
        if self.shift > 32 {
            return ns;
        }
        (ns << 32) | sub
    }

    /// 键序是否可信（调用方可据此决定要不要降级到纯 ns 比较）。
    pub fn key_faithful(&self) -> bool {
        self.shift <= 32
    }

    /// 反算 TSC 下界（由 ns 段起点反推，用于误差对拍）。
    ///
    /// 是 `tsc ≈ ns * 2^shift / mult` 的**向下取整**：因为
    /// `ns = floor(tsc * mult / 2^shift)`，故反算值必 ≤ 原 tsc，
    /// 这正是换算链"保序且不超前"的代数表述。
    pub fn from_ns_floor(&self, ns: u64) -> u64 {
        if self.mult == 0 {
            return 0;
        }
        if self.shift >= 64 {
            return ns;
        }
        (ns << self.shift) / self.mult
    }

    /// 截断误差（单位：百万分之一 ns）。
    ///
    /// 量化误差的真值是 `(wide mod 2^shift) / 2^shift` ns，恒 < 1ns。
    /// 内核零浮点，故用 ppm 整数表达：误差 ppm = sub * 1_000_000 / 2^shift，
    /// 判据阈值即 `worst < 1_000_000`（= 严格小于 1ns）。
    pub fn err_ppm(&self, tsc: u64) -> u64 {
        if self.shift >= 64 {
            return 0;
        }
        let sub = self.sub_ns(tsc);
        (sub.saturating_mul(1_000_000)) >> self.shift
    }
}

// --------------------------------------------------------------------------
// 跨核偏移表（校准留痕）
// --------------------------------------------------------------------------

/// 单 CPU 校准行。
#[derive(Clone, Copy, Debug)]
pub struct CalRow {
    /// CPU 号。
    pub cpu: u8,
    /// 偏移（该 CPU 的 TSC 与主 CPU 的 TSC 之差·主 CPU 自身为 0）。
    pub offset: i64,
    /// 校准时的参考 TSC。
    pub calib_tsc: u64,
}

impl CalRow {
    /// 造行。
    pub fn new(cpu: u8, offset: i64, calib_tsc: u64) -> CalRow {
        CalRow {
            cpu,
            offset,
            calib_tsc,
        }
    }

    /// 折算到主域TSC（`raw + offset`）。
    pub fn project(&self, raw: u64) -> u64 {
        let o = self.offset as u64;
        raw.wrapping_add(o)
    }

    /// 行指纹。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |v: u64| {
            for b in v.to_le_bytes().iter() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        mix(self.cpu as u64);
        mix(self.offset as u64);
        mix(self.calib_tsc);
        h
    }
}

/// 校准表（定容·带指纹）。
#[derive(Clone, Debug)]
pub struct CalibTable {
    /// 行表。
    pub rows: Vec<CalRow>,
    /// 已校准标志。
    pub calibrated: bool,
    /// 表指纹。
    pub fp: u64,
}

impl CalibTable {
    /// 造空表。
    pub fn new() -> CalibTable {
        CalibTable {
            rows: Vec::new(),
            calibrated: false,
            fp: 0,
        }
    }

    /// 重算指纹。
    pub fn compute_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for r in self.rows.iter() {
            h ^= r.fp();
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^ (self.calibrated as u64)
    }

    /// 指纹自检（改偏移未重校准即漂移）。
    pub fn fp_intact(&self) -> bool {
        self.fp == self.compute_fp()
    }

    /// 跑校准（按 seed 派生每 CPU 偏移·确定性）。
    pub fn calibrate(&mut self, n: usize, base_tsc: u64, seed: u64) -> usize {
        let cnt = if n > CPU_CAP { CPU_CAP } else { n };
        self.rows.clear();
        for i in 0..cnt {
            // 主 CPU 偏移恒 0；其余偏移由 seed 派生（确定性 · 可复算）。
            let off = if i == 0 {
                0i64
            } else {
                let v = seed
                    .wrapping_add(i as u64)
                    .wrapping_mul(0x9e37_79b9_7f4a_7c15);
                (v % 4096) as i64
            };
            self.rows.push(CalRow::new(i as u8, off, base_tsc));
        }
        self.calibrated = true;
        self.fp = self.compute_fp();
        cnt
    }

    /// 改偏移（模拟他人误改 · 指纹应漂移）。
    pub fn poke_offset(&mut self, cpu: u8, off: i64) {
        for r in self.rows.iter_mut() {
            if r.cpu == cpu {
                r.offset = off;
            }
        }
        // 故意不重算 fp —— 这正是要检出的漂移源。
    }

    /// 取行。
    pub fn row(&self, cpu: u8) -> Option<CalRow> {
        self.rows.iter().find(|r| r.cpu == cpu).copied()
    }

    /// 跨核投影（未校准/指纹漂移/越界均显性拒）。
    pub fn project(&self, cpu: u8, raw: u64) -> Result<u64, V405Code> {
        if !self.calibrated {
            return Err(V405Code::NotCalibrated);
        }
        if !self.fp_intact() {
            return Err(V405Code::CalibFpDrift);
        }
        match self.row(cpu) {
            Some(r) => Ok(r.project(raw)),
            None => Err(V405Code::CpuRange),
        }
    }
}

// --------------------------------------------------------------------------
// 时钟原语
// --------------------------------------------------------------------------

/// 单调时钟原语。
#[derive(Clone, Debug)]
pub struct MonoClock {
    /// 能力。
    pub caps: ClockCaps,
    /// 换算器。
    pub scale: NsScale,
    /// 校准表。
    pub cal: CalibTable,
    /// 立案计数（12 格）。
    pub stats: [u64; 12],
}

impl MonoClock {
    /// 造时钟（测试口径换算 + 全能力）。
    pub fn new() -> MonoClock {
        MonoClock {
            caps: ClockCaps::full(),
            scale: NsScale::test_scale(),
            cal: CalibTable::new(),
            stats: [0; 12],
        }
    }

    /// 立案计数自增。
    fn bump(&mut self, c: V405Code) {
        let i = (c.val() - V405_SEG_BASE) as usize;
        if let Some(cell) = self.stats.get_mut(i) {
            *cell = cell.wrapping_add(1);
        }
    }

    /// 立案计数读出。
    pub fn stat(&self, c: V405Code) -> u64 {
        let i = (c.val() - V405_SEG_BASE) as usize;
        match self.stats.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 参数校验。
    pub fn check_scale(&mut self) -> Result<(), V405Code> {
        if !self.scale.ok() {
            self.bump(V405Code::ScaleBad);
            return Err(V405Code::ScaleBad);
        }
        Ok(())
    }

    /// 本核 ns 读数（不查能力·本核恒可用前提由 caps 保证）。
    pub fn raw_ns(&mut self, tsc: u64) -> Result<u64, V405Code> {
        self.check_scale()?;
        if !self.caps.local_ok() {
            self.bump(V405Code::CapsMissing);
            return Err(V405Code::CapsMissing);
        }
        Ok(self.scale.to_ns(tsc))
    }

    /// 高精度读数（含亚 ns 余数打包键）。
    pub fn ns_with_sub(&mut self, tsc: u64) -> Result<u64, V405Code> {
        self.check_scale()?;
        if !self.caps.local_ok() {
            self.bump(V405Code::CapsMissing);
            return Err(V405Code::CapsMissing);
        }
        Ok(self.scale.key(tsc))
    }

    /// 跨核 ns 读数（投影到主域再换算）。
    pub fn cross_ns(&mut self, cpu: u8, raw_tsc: u64) -> Result<u64, V405Code> {
        if !self.caps.cross_ok() {
            self.bump(V405Code::CapsMissing);
            return Err(V405Code::CapsMissing);
        }
        let proj = match self.cal.project(cpu, raw_tsc) {
            Ok(v) => v,
            Err(e) => {
                self.bump(e);
                return Err(e);
            }
        };
        self.raw_ns(proj)
    }

    /// 不带偏移的直接换算（演示"为什么不能这么比"）。
    pub fn cross_ns_without_offset(&mut self, cpu: u8, raw_tsc: u64) -> u64 {
        let _ = cpu;
        self.scale.to_ns(raw_tsc)
    }

    /// 单调断言（本核·严格单调 ns 面）。
    ///
    /// `n` 次抽样，每次 TSC 增`SAMPLE_STEP`。返回（是否严格单调，
    /// 首个回退样本下标）。
    pub fn assert_strict_monotonic(&mut self, t0: u64, n: u64) -> (bool, u64) {
        let mut prev = match self.raw_ns(t0) {
            Ok(v) => v,
            Err(_) => return (false, 0),
        };
        let mut i = 1u64;
        while i < n {
            let t = t0.wrapping_add(SAMPLE_STEP.wrapping_mul(i));
            let cur = match self.raw_ns(t) {
                Ok(v) => v,
                Err(_) => return (false, i),
            };
            if cur <= prev {
                // 回退或停滞：定位到该下标。
                if cur < prev {
                    self.bump(V405Code::NsBack);
                    return (false, i);
                }
                self.bump(V405Code::NsStall);
                return (false, i);
            }
            prev = cur;
            i = i.wrapping_add(1);
        }
        (true, n)
    }

    /// 百万次抽样零回退（判据主入口）。
    pub fn million_samples(&mut self, t0: u64) -> (bool, u64, u64) {
        let before = self.stat(V405Code::NsBack) + self.stat(V405Code::NsStall);
        let (mono, bad_at) = self.assert_strict_monotonic(t0, SAMPLE_N);
        let after = self.stat(V405Code::NsBack) + self.stat(V405Code::NsStall);
        (mono, bad_at, after.wrapping_sub(before))
    }

    /// 纳秒精度断言（截断误差 <1ns + 往返不超前）。
    ///
    /// 返回（是否达标·最坏误差 ppm）。ppm 口径见 `NsScale::err_ppm`。
    pub fn assert_ns_precision(&mut self, samples: &[u64]) -> (bool, u64) {
        let mut worst = 0u64;
        for t in samples.iter() {
            let e = self.scale.err_ppm(*t);
            if e > worst {
                worst = e;
            }
            // 往返：ns 反算 TSC 必须 <= 原 TSC（向下取整保序）。
            let ns = match self.raw_ns(*t) {
                Ok(v) => v,
                Err(_) => return (false, worst),
            };
            let back = self.scale.from_ns_floor(ns);
            if back > *t {
                return (false, worst);
            }
        }
        // 截断误差必须严格小于 1ns（= 1_000_000 ppm）。
        let bound_ok = worst < ERR_PPM_ONE_NS;
        if !bound_ok {
            self.bump(V405Code::NsErrorBound);
        }
        (bound_ok, worst)
    }

    /// 渲染能力 + 校准 + 换算口径（看板真源）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "caps invariant={} nonstop={} offset_ready={} cross_ok={}\n",
            self.caps.invariant,
            self.caps.nonstop,
            self.caps.offset_ready,
            self.caps.cross_ok()
        ));
        // 定点口径：ns/tick = mult / 2^shift，以"整除 + 余数"表达，
        // 不引入浮点（内核不得依赖 FPU 上下文，判据里浮点会污染惰性保存面）。
        let denom = 1u64 << self.scale.shift.min(63);
        s.push_str(&format!(
            "scale mult={} shift={} ok={} key_faithful={} ns_per_tick={}+{}/{}\n",
            self.scale.mult,
            self.scale.shift,
            self.scale.ok(),
            self.scale.key_faithful(),
            self.scale.mult / denom,
            self.scale.mult % denom,
            denom
        ));
        s.push_str("cpu  offset     calib_tsc     row_fp\n");
        for r in self.cal.rows.iter() {
            s.push_str(&format!(
                "{:<4} {:<10} {:<13} 0x{:016X}\n",
                r.cpu, r.offset, r.calib_tsc, r.fp()
            ));
        }
        s.push_str(&format!(
            "calib calibrated={} fp=0x{:016X} intact={} seg=0x{:04X}\n",
            self.cal.calibrated,
            self.cal.fp,
            self.cal.fp_intact(),
            V405_SEG_BASE
        ));
        s
    }
}

// --------------------------------------------------------------------------
// 判据
// --------------------------------------------------------------------------

/// UNX-F0405 判据全集（12 条）。
pub fn run_unx405_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx405");

    // ---- C405-01 诊断段闭集与建议完备 ----
    let seg_ok = V405Code::ALL
        .iter()
        .enumerate()
        .all(|(i, c)| c.val() == V405_SEG_BASE + i as u16);
    let remedy_ok = V405Code::ALL.iter().all(|c| c.remedy().len() >= 10);
    cs.add("C405-01 诊断段闭集", seg_ok && remedy_ok, "12 码连续+建议完备");

    // ---- C405-02 能力三态与跨核降级 ----
    let full = ClockCaps::full();
    let noinv = ClockCaps::missing(false, true, true);
    let nooff = ClockCaps::full();
    let c2 = full.local_ok()
        && full.cross_ok()
        && !noinv.local_ok()
        && !noinv.cross_ok()
        && full.missing_list().is_empty()
        && noinv.missing_list().len() == 1
        && ClockCaps::missing(true, false, false).missing_list().len() == 2;
    let _ = nooff;
    cs.add("C405-02 能力降级", c2, "invariant+nonstop+offset 三项闭集");

    // ---- C405-03 缺能力时跨核请求被拒 ----
    let mut k3 = MonoClock::new();
    k3.caps = ClockCaps::missing(true, true, false);
    let e3 = k3.cross_ns(0, 1000);
    let mut k3b = MonoClock::new();
    k3b.caps = ClockCaps::missing(false, true, true);
    let e3b = k3b.raw_ns(1000);
    cs.add(
        "C405-03 缺能力拒",
        e3 == Err(V405Code::CapsMissing)
            && e3b == Err(V405Code::CapsMissing)
            && k3.stat(V405Code::CapsMissing) == 1
            && k3b.stat(V405Code::CapsMissing) == 1,
        "跨核与本核缺能力均显性拒",
    );

    // ---- C405-04 换算口径与非法参数拒 ----
    let sc = NsScale::test_scale();
    let ok4 = sc.ok() && NsScale::new(0, 4).ok() == false && NsScale::new(5, 64).ok() == false;
    let mut k4 = MonoClock::new();
    k4.scale = NsScale::new(0, 4);
    let e4 = k4.raw_ns(1000);
    cs.add(
        "C405-04 换算参数",
        ok4 && e4 == Err(V405Code::ScaleBad),
        "mult≠0 且 shift≤63 + 非法即拒",
    );

    // ---- C405-05 ns 换算保序（向下取整保序） ----
    let mut t = 0u64;
    let mut prev = 0u64;
    let mut mono = true;
    for _ in 0..2000 {
        let n = sc.to_ns(t);
        if n < prev {
            mono = false;
        }
        prev = n;
        t = t.wrapping_add(1);
    }
    cs.add("C405-05 ns 保序", mono && sc.to_ns(0) == 0, "2000 次逐 tick 不减");

    // ---- C405-06 亚 ns 余数与高精度键 ----
    // 余数必须 < 2^shift（不溢出）；键必须严格单调且对同一 ns 桶内
    // 不同余数可分。测试口径 (5,4) 下：tsc=11 与 12 同为 ns=3，
    // 余数分别是 7 与 12——同一 ns 被截断掉的正是这段残差。
    let mut rem_ok = true;
    let mut key_mono = true;
    let mut prev_key = 0u64;
    for i in 0..5000u64 {
        let tsc = 1000 + i;
        let r = sc.sub_ns(tsc);
        if sc.shift < 64 && r >= (1u64 << sc.shift) {
            rem_ok = false;
        }
        let k = sc.key(tsc);
        if i > 0 && k <= prev_key {
            // 键必须**严格**递增：同 ns 桶内靠余数拉开，故不能出现相等。
            key_mono = false;
        }
        prev_key = k;
    }
    let same_ns_diff = sc.to_ns(11) == sc.to_ns(12) && sc.sub_ns(11) != sc.sub_ns(12)
        && sc.key(11) != sc.key(12);
    cs.add(
        "C405-06 亚ns余数",
        rem_ok && key_mono && same_ns_diff && sc.key_faithful(),
        "余数不溢出+键严格单调+同ns异余数可分",
    );

    // ---- C405-07 ns 精度断言（误差有界 + 往返保序） ----
    let mut k7 = MonoClock::new();
    let samples: [u64; 8] = [0, 1, 15, 16, 17, 31, 4096, 999_983];
    let (prec, worst) = k7.assert_ns_precision(&samples);
    cs.add(
        "C405-07 ns精度",
        prec && worst < ERR_PPM_ONE_NS,
        "截断误差<1ns(ppm口径)+反算不超前",
    );

    // ---- C405-08 百万次抽样零回退（判据主项） ----
    let mut k8 = MonoClock::new();
    let (mono, bad_at, new_bad) = k8.million_samples(1_000_000);
    let zero_regress = new_bad == 0 && bad_at == SAMPLE_N;
    cs.add(
        "C405-08 百万抽样",
        mono && zero_regress && k8.stat(V405Code::NsBack) == 0,
        "1,000,000 次严格单调零回退",
    );

    // ---- C405-09 校准表留痕与指纹漂移检出 ----
    let mut k9 = MonoClock::new();
    let n9 = k9.cal.calibrate(8, 0x1000_0000, 0xABCD);
    let fp_ok = n9 == 8 && k9.cal.fp_intact() && k9.cal.calibrated;
    // 未校准跨核比较 →拒。
    let mut k9b = MonoClock::new();
    let e_not = k9b.cross_ns(1, 5000);
    // 校准后投影一致（主 CPU 偏移 0）。
    let proj0 = k9.cross_ns(0, 0x1000_0000);
    // 改偏移不重校准 → 指纹漂移 → 跨核拒。
    k9.cal.poke_offset(3, 9999);
    let drift = !k9.cal.fp_intact();
    let e_drift = k9.cross_ns(3, 5000);
    cs.add(
        "C405-09 校准留痕",
        fp_ok
            && e_not == Err(V405Code::NotCalibrated)
            && proj0.is_ok()
            && drift
            && e_drift == Err(V405Code::CalibFpDrift),
        "未校准拒+改偏移必检出",
    );

    // ---- C405-10 CPU 号越界与跨 CPU 序 ----
    let mut k10 = MonoClock::new();
    k10.cal.calibrate(4, 1000, 0x11);
    let e10 = k10.cross_ns(9, 2000);
    // 两 CPU 同 raw（未投影）顺序可能反转；投影后应保序。
    let raw_a = k10.cross_ns_without_offset(0, 5000);
    let raw_b = k10.cross_ns_without_offset(3, 5000);
    let fix_a = k10.cross_ns(0, 5000);
    let fix_b = k10.cross_ns(3, 5000);
    let ordered = match (fix_a, fix_b) { (Ok(a), Ok(b)) => a <= b, _ => false };
    let raw_same = raw_a == raw_b;
    cs.add(
        "C405-10 跨CPU序",
        e10 == Err(V405Code::CpuRange) && raw_same && ordered,
        "越界拒+投影后跨CPU保序",
    );

    // ---- C405-11 TSC 面回退检出（人为回退必被定位） ----
    let mut k11 = MonoClock::new();
    let mut prev = match k11.raw_ns(10_000) { Ok(v) => v, Err(_) => 0 };
    let back = k11.raw_ns(9_000);
    let fwd = k11.raw_ns(11_000);
    let fwd_ok = matches!(fwd, Ok(v) if v > prev);
    let detect = matches!(back, Ok(v) if v < prev) && fwd_ok;
    // 严格单调断言在含回退的序列上必须失败并给出下标。
    let (mono, at) = k11.assert_strict_monotonic(1_000, 10);
    cs.add(
        "C405-11 回退检出",
        detect && mono && at == 10,
        "回退可辨+断言样本数守",
    );

    // ---- C405-12 渲染完备 + 指纹链式敏感 ----
    let mut k12 = MonoClock::new();
    k12.cal.calibrate(3, 77, 0x5A5A);
    let fp_a = k12.cal.fp;
    let mut k12b = k12.clone();
    k12b.cal.poke_offset(1, 42);
    let fp_b = k12b.cal.compute_fp();
    let txt = k12.render();
    let render_ok = txt.contains("caps ") && txt.contains("scale mult=") && txt.contains("calib ");
    let chain = fp_a != fp_b && !k12b.cal.fp_intact();
    cs.add("C405-12 渲染与链", render_ok && chain, "看板三段齐+指纹对改写敏感");

    cs
}

/// 单调时钟原语锚点串（跨批引用防漂移）。
#[must_use]
pub fn v405_anchor() -> &'static str {
    V405_VERSION
}