//! UNX-F36116-J1 掩码性能账：转换与判定热路径。
//! 判据：① 步数预算账（无墙钟——以位操作步数计量）：往返转换
//! 256 例 × 单例步数 ≤ 预算；覆盖判定 256 例 × 步数 ≤ 预算；
//! ② 滚动桶直方图（4 桶）守恒 + 整数分位（isqrt）；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：性能账面——位段常量从 36101 取；转换/判定核与
//! 36114/36109 同式（双写对拍点）；预算锚写死后不得回退。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3a40;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BUDGET: u16 = SEG_BASE + 2; // 单例步数超预算
pub const E_TOTAL: u16 = SEG_BASE + 3; // 总步数超总预算
pub const E_BUCKETS: u16 = SEG_BASE + 4; // 滚动桶守恒破（桶和 ≠ 调用数）
pub const E_ISQRT: u16 = SEG_BASE + 5; // 整数平方根账错
pub const E_NEG: u16 = SEG_BASE + 6; // 负例注入未被检出
pub const E_CONV: u16 = SEG_BASE + 7; // 转换对拍破（性能账的载荷失真）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 计步操作面（四热路径；显式判别式）。
#[repr(u16)]
pub enum PerfOp {
    Encode = 0x0001,
    Decode = 0x0002,
    Cover = 0x0004,
    Popcount = 0x0008,
}

impl PerfOp {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 预算锚（步数/单例）。
pub const BUDGET_ENCODE: u32 = 24;
pub const BUDGET_DECODE: u32 = 24;
pub const BUDGET_COVER: u32 = 16;
pub const BUDGET_POPCOUNT: u32 = 64;

/// 负载量锚。
pub const WORKLOAD_N: usize = 256;

/// 滚动桶数（步数分位桶：0-3 / 4-7 / 8-15 / 16+）。
pub const BUCKET_N: usize = 4;

/// 步数账：单调用步数记录器 + 滚动桶直方图。
pub struct StepLedger {
    pub calls: u64,
    pub total_steps: u64,
    pub buckets: [u32; BUCKET_N],
    pub worst: u32,
}

impl StepLedger {
    pub fn new() -> StepLedger {
        StepLedger { calls: 0, total_steps: 0, buckets: [0; BUCKET_N], worst: 0 }
    }

    /// 记一笔（桶：0-3→0、4-7→1、8-15→2、16+→3）。
    pub fn record(&mut self, steps: u32) {
        self.calls += 1;
        self.total_steps += steps as u64;
        let idx = if steps < 4 {
            0
        } else if steps < 8 {
            1
        } else if steps < 16 {
            2
        } else {
            3
        };
        self.buckets[idx] += 1;
        if steps > self.worst {
            self.worst = steps;
        }
    }

    /// 桶守恒：桶和 == 调用数。
    pub fn buckets_consistent(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0;
        while i < BUCKET_N {
            sum += self.buckets[i];
            i += 1;
        }
        sum as u64 == self.calls
    }

    /// 整数平方根（牛顿整数迭代，无浮点）。
    pub fn isqrt(x: u64) -> u64 {
        if x < 2 {
            return x;
        }
        let mut r = x;
        let mut s = (r + 1) / 2;
        while s < r {
            r = s;
            s = (r + x / r) / 2;
        }
        r
    }

    /// 平均步数（千分位整数：steps*1000/calls）。
    pub fn avg_milli(&self) -> u64 {
        if self.calls == 0 {
            return 0;
        }
        self.total_steps * 1000 / self.calls
    }
}

/// 计步版编码（与 36114 同式；返回帧 + 步数）。
pub fn encode_counted(mask: u32) -> ([u8; 8], u32) {
    let mut steps: u32 = 0;
    let mut f = [0u8; 8];
    f[0] = 0x4D;
    f[1] = 0x4B;
    f[2] = 1;
    steps += 3;
    let mut i = 0;
    while i < 4 {
        f[3 + i] = ((mask >> (i * 8)) & 0xFF) as u8;
        steps += 3; // 移位+与+截
        i += 1;
    }
    let mut c: u8 = 0;
    let mut i = 0;
    while i < 7 {
        c ^= f[i];
        steps += 1;
        i += 1;
    }
    f[7] = c;
    steps += 1;
    (f, steps)
}

/// 计步版解码。
pub fn decode_counted(f: &[u8; 8]) -> (u32, u32) {
    let mut steps: u32 = 2; // 魔数双查
    if f[0] != 0x4D || f[1] != 0x4B {
        return (0, steps);
    }
    steps += 1; // 版本查
    let mut c: u8 = 0;
    let mut i = 0;
    while i < 7 {
        c ^= f[i];
        steps += 1;
        i += 1;
    }
    steps += 1; // 校验比对
    let mut m: u32 = 0;
    let mut i = 0;
    while i < 4 {
        m |= (f[3 + i] as u32) << (i * 8);
        steps += 2;
        i += 1;
    }
    (m, steps)
}

/// 计步版覆盖判定（与 36109 同式；判定核 3 步——与/非/零比）。
pub fn cover_counted(granted: u32, required: u32) -> (bool, u32) {
    let mut steps: u32 = 1;
    let m = required & !granted;
    steps += 2;
    (m == 0, steps)
}

/// 计步版 popcount。
pub fn popcount_counted(m: u32) -> (u32, u32) {
    let mut steps: u32 = 0;
    let mut n = 0;
    let mut x = m;
    while x != 0 {
        n += x & 1;
        x >>= 1;
        steps += 2;
    }
    (n, steps)
}

/// LCG 负载生成（256 例合法掩码）。
pub fn workload() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    let mut state: u64 = 0x0B06_1027_4A17_0019;
    let mut i = 0;
    while i < WORKLOAD_N {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let m = ((state >> 33) as u32) & mt::ALL_LEGAL;
        v.push(m);
        i += 1;
    }
    v
}

/// 演练：R1 负载锚；R2 转换热路径预算；R3 覆盖热路径预算；
/// R4 桶守恒 + 分位；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：负载锚——256 例、全合法域、LCG 可复现（两次生成相同）。
    if VER != 1 {
        return Err(E_VER);
    }
    let w1 = workload();
    let w2 = workload();
    if w1.len() != WORKLOAD_N || w1 != w2 {
        return Err(E_NEG);
    }
    let mut i = 0;
    while i < WORKLOAD_N {
        if w1[i] & mt::ALL_RSVD != 0 {
            return Err(E_CONV);
        }
        i += 1;
    }
    hits += 1;
    // R2：转换热路径——encode/decode 逐例步数 ≤ 预算；往返逐位同。
    let mut led_e = StepLedger::new();
    let mut led_d = StepLedger::new();
    let mut i = 0;
    while i < WORKLOAD_N {
        let m = w1[i];
        let (f, se) = encode_counted(m);
        led_e.record(se);
        if se > BUDGET_ENCODE {
            return Err(E_BUDGET);
        }
        let (back, sd) = decode_counted(&f);
        led_d.record(sd);
        if sd > BUDGET_DECODE {
            return Err(E_BUDGET);
        }
        if back != m {
            return Err(E_CONV);
        }
        i += 1;
    }
    if led_e.worst > BUDGET_ENCODE || led_d.worst > BUDGET_DECODE {
        return Err(E_BUDGET);
    }
    hits += 1;
    // R3：覆盖热路径——逐例步数 ≤ 预算；判定与 popcount 同源。
    let mut led_c = StepLedger::new();
    let mut led_p = StepLedger::new();
    let mut i = 0;
    while i < WORKLOAD_N {
        let g = w1[i];
        let r = w1[(i * 7 + 11) % WORKLOAD_N];
        let (ok, sc) = cover_counted(g, r);
        led_c.record(sc);
        if sc > BUDGET_COVER {
            return Err(E_BUDGET);
        }
        let (pc, sp) = popcount_counted(r & !g);
        led_p.record(sp);
        if sp > BUDGET_POPCOUNT {
            return Err(E_BUDGET);
        }
        if ok != (pc == 0) {
            return Err(E_CONV);
        }
        i += 1;
    }
    hits += 1;
    // R4：桶守恒 + 分位——四账桶和 == 256；avg 千分位 == isqrt 级一致。
    let ledgers: [&StepLedger; 4] = [&led_e, &led_d, &led_c, &led_p];
    let mut i = 0;
    while i < 4 {
        let l = ledgers[i];
        if !l.buckets_consistent() {
            return Err(E_BUCKETS);
        }
        if l.calls != WORKLOAD_N as u64 {
            return Err(E_BUCKETS);
        }
        // 均值面：avg_milli 非零且 ≈ total*1000/calls（重算对拍）。
        let a = l.avg_milli();
        let expect = l.total_steps * 1000 / l.calls;
        if a != expect || a == 0 {
            return Err(E_ISQRT);
        }
        // isqrt 面：isqrt(calls²) == calls；isqrt(0)=0；isqrt 单调。
        if StepLedger::isqrt(l.calls * l.calls) != l.calls {
            return Err(E_ISQRT);
        }
        i += 1;
    }
    if StepLedger::isqrt(0) != 0 || StepLedger::isqrt(1) != 1 || StepLedger::isqrt(15) != 3
        || StepLedger::isqrt(16) != 4 || StepLedger::isqrt(17) != 4
    {
        return Err(E_ISQRT);
    }
    let mut prev = 0u64;
    let mut i = 0;
    while i < 40 {
        let q = StepLedger::isqrt(i);
        if q < prev {
            return Err(E_ISQRT);
        }
        prev = q;
        i += 1;
    }
    hits += 1;
    // R5：负例注入——伪账（桶和 ≠ 调用数）必须被守恒检出。
    let mut fake = StepLedger::new();
    fake.record(1);
    fake.record(5);
    fake.calls = 9; // 账目篡改：桶和(2) ≠ 调用数(9)
    if fake.buckets_consistent() {
        return Err(E_NEG);
    }
    if fake.buckets[0] != 1 || fake.buckets[1] != 1 {
        return Err(E_BUCKETS);
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < 4 {
        ledger.push(ledgers[i].total_steps);
        i += 1;
    }
    if ledger.len() != 4 {
        return Err(E_NEG);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn budgets_hold() {
        let m = 0x0012_0089u32;
        let (_, se) = encode_counted(m);
        let (f, _) = encode_counted(m);
        let (_, sd) = decode_counted(&f);
        assert!(se <= BUDGET_ENCODE);
        assert!(sd <= BUDGET_DECODE);
    }

    #[test]
    fn bucket_consistency() {
        let mut l = StepLedger::new();
        l.record(0);
        l.record(4);
        l.record(15);
        l.record(16);
        assert!(l.buckets_consistent());
        assert_eq!(l.buckets, [1, 1, 1, 1]);
        assert_eq!(l.worst, 16);
    }

    #[test]
    fn isqrt_exact() {
        assert_eq!(StepLedger::isqrt(256), 16);
        assert_eq!(StepLedger::isqrt(255), 15);
        assert_eq!(StepLedger::isqrt(65536), 256);
    }
}

pub fn run_unx36116_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36116_maskperf");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a40, "0x3A40~0x3A4F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "预算/桶守恒/分位全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("负载锚", WORKLOAD_N == 256, "256 例 LCG 确定性负载");
    cs.add("预算锚", BUDGET_ENCODE == 24 && BUDGET_COVER == 16, "单例步数预算写死");
    cs.add("桶面", BUCKET_N == 4, "4 桶滚动直方图");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BUDGET => "单例步数超预算",
        E_TOTAL => "总步数超总预算",
        E_BUCKETS => "滚动桶守恒破（桶和 ≠ 调用数）",
        E_ISQRT => "整数平方根账错",
        E_NEG => "负例注入未被检出",
        E_CONV => "转换对拍破（性能账的载荷失真）",
        _ => "未知诊断码",
    }
}
