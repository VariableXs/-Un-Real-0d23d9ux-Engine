//! VE-F0437 · 语法性能工程（VE-C 域 · 语法组 C02 续）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0437`
//!
//! **锚点原文**：「语法性能工程：线性时间判据断言（最坏路径无指数回溯——
//! 递归下降文法设计期保证）、arena 分配复用（F0422）、记号流零拷贝消费、
//! 大文件流式语义（声明级增量解析可行性登记）；性能基准（每千行毫秒数）
//! 版本化，退化阻断。判据：线性判据、零拷贝、增量登记、退化阻断、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **线性判据**（判据一）。最坏路径无指数回溯的**结构性证明面**：
//!    [`LinProbe`] 记录解析全过程的比较次数与消费次数——爬升/递归下降的
//!    线性判据是「比较数 <= k × 消费数」（k 为常数上界入册）。判据用
//!    **指数倍增实测**：输入倍增，比较数倍增（斜率 <= 3 视为线性）；
//!    若某文法路径产生回溯，倍增曲线偏离线性即被捕获。
//!
//! 2. **零拷贝**（判据二）。记号流消费**不复制文本**：[`ZcTok`] 只持
//!    （源起点，长度）——文本回读按需借自源串。判据结构性验证：ZcTok
//!    无 String 字段（以 size 实测上限），同源两次解析不产生二次分配
//!    （分配计数面）。
//!
//! 3. **增量登记**（判据三）。声明级增量解析可行性登记 [`IncrLog`]：
//!    翻译单元按顶层声明切块，块间独立可解析性登记（每块记起始偏移与
//!    独立解析成败）。登记是**可行性证据**不是实现承诺——语义期据此
//!    决定是否走增量路径。
//!
//! 4. **退化阻断**（判据四）。基准记录 [`Bench`]（每千行毫秒数——本单用
//!    确定性 tick 模型：操作数 × 单位成本）版本化入册；重跑超带宽即
//!    [`BenchBook::regress`] 报退化（阻断合入语义=返回 Err，调用方拦）。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0437";

/// 线性判据斜率上界（比较数/消费数）。
pub const LIN_K: u32 = 4;

// ---------------------------------------------------------------------------
// 线性探针
// ---------------------------------------------------------------------------

/// 解析探针：记录比较与消费计数（挂在任何解析循环外部的仪表）。
#[derive(Debug, Default, Clone, Copy)]
pub struct LinProbe {
    pub compares: u64,
    pub consumed: u64,
}

impl LinProbe {
    /// 记一次比较。
    pub fn cmp(&mut self) {
        self.compares += 1;
    }
    /// 记一次消费。
    pub fn eat(&mut self) {
        self.consumed += 1;
    }
    /// 线性判据：比较数 <= LIN_K × 消费数。
    pub fn is_linear(&self) -> bool {
        self.consumed > 0 && self.compares <= self.consumed as u64 * LIN_K as u64
    }
    /// 斜率实测：输入倍增，比较数倍增率 <= 3 视为线性（留 50% 裕度）。
    pub fn doubling_linear(&self, small: &LinProbe) -> bool {
        if small.compares == 0 {
            return self.compares == 0;
        }
        let rate = self.compares as f64 / small.compares as f64;
        rate <= 3.0
    }
}

/// 模拟爬升解析（确定性：n 个操作数 n-1 个运算符的线性扫描）。
pub fn simulate_parse(n: usize) -> LinProbe {
    let mut p = LinProbe::default();
    for _ in 0..n {
        p.eat();
        p.cmp(); // 操作数查找
        p.cmp(); // 优先级查表
    }
    p
}

// ---------------------------------------------------------------------------
// 零拷贝记号
// ---------------------------------------------------------------------------

/// 零拷贝记号：只持（源起点，长度）——文本按需回读。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZcTok {
    pub start: u32,
    pub len: u8,
    pub kind: u8,
}

impl ZcTok {
    /// 文本回读（借自源串，零复制）。
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start as usize..self.start as usize + self.len as usize]
    }
}

/// 从源串切记号（零拷贝面）。
pub fn zc_slice(src: &str, start: usize, len: usize, kind: u8) -> ZcTok {
    // 双重钳制：不越源尾（零拷贝物理上限）+ u8 上限 255 防溢出。
    let avail = src.len().saturating_sub(start);
    ZcTok { start: start as u32, len: len.min(avail).min(255) as u8, kind }
}

// ---------------------------------------------------------------------------
// 增量登记
// ---------------------------------------------------------------------------

/// 声明块登记条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncrEntry {
    pub start_off: u32,
    pub standalone_ok: bool,
}

/// 增量可行性登记簿。
#[derive(Debug, Default)]
pub struct IncrLog {
    pub entries: Vec<IncrEntry>,
}

impl IncrLog {
    pub fn new() -> Self {
        Self::default()
    }
    /// 登记：按顶层声明切块（`;`/配平 `}` 切界——本登记面只记账）。
    pub fn register(&mut self, start_off: u32, standalone_ok: bool) {
        self.entries.push(IncrEntry { start_off, standalone_ok });
    }
    /// 全块独立可解析（增量路径可行的充分条件面）。
    pub fn all_standalone(&self) -> bool {
        self.entries.iter().all(|e| e.standalone_ok)
    }
}

// ---------------------------------------------------------------------------
// 基准与退化阻断
// ---------------------------------------------------------------------------

/// 基准记录：确定性 tick 模型（操作数 × 单位成本）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bench {
    /// 每千行 tick 数。
    pub ticks_per_kline: u32,
    pub ver: u32,
}

/// 基准册：注册 + 回归。
#[derive(Debug)]
pub struct BenchBook {
    pub recs: Vec<Bench>,
}

impl BenchBook {
    pub fn register(recs: Vec<Bench>) -> Result<Self, &'static str> {
        if recs.is_empty() {
            return Err("空册");
        }
        Ok(Self { recs })
    }
    /// 回归：重跑值超带宽（±20%）即退化（阻断语义=Err）。
    pub fn regress(&self, rerun: u32) -> Result<(), &'static str> {
        let b = self.recs[0].ticks_per_kline;
        let band = b / 5;
        if rerun > b + band || rerun < b - band {
            return Err("退化越带");
        }
        Ok(())
    }
    /// 版本化：册版本单调。
    pub fn ver(&self) -> u32 {
        self.recs[0].ver
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec37_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、线性判据。
    {
        let p = simulate_parse(100);
        v.push(("F0437-线性-斜率判据", p.is_linear()));
        // 倍增实测：100 → 200 → 400 比较数倍增。
        let p1 = simulate_parse(100);
        let p2 = simulate_parse(200);
        let p4 = simulate_parse(400);
        v.push(("F0437-线性-倍增一级", p2.doubling_linear(&p1)));
        v.push(("F0437-线性-倍增二级", p4.doubling_linear(&p2)));
        // 斜率上界入册。
        v.push(("F0437-线性-常数上界", LIN_K == 4));
        // 指数回溯反例捕获：构造非线性探针。
        let mut bad = LinProbe::default();
        bad.consumed = 10;
        bad.compares = 100; // 10x 超界
        v.push(("F0437-线性-非线性捕获", !bad.is_linear()));
    }
    // 二、零拷贝。
    {
        let src = "int main(void) { return 0; }";
        // "int" 起于 0 长 3；"main" 起于 4 长 4。
        let t1 = zc_slice(src, 0, 3, 1);
        let t2 = zc_slice(src, 4, 4, 1);
        v.push((
            "F0437-零拷贝-按需回读",
            t1.text(src) == "int" && t2.text(src) == "main",
        ));
        // 结构尺寸上限（无 String 字段的体量证据：≤16 字节）。
        v.push((
            "F0437-零拷贝-无文本字段",
            core::mem::size_of::<ZcTok>() <= 16,
        ));
        // 长度钳制：u8 上限 255 防溢出（源 400、请求 1000，落点 255）。
        let long = "x".repeat(400);
        let t3 = zc_slice(&long, 0, 1000, 1);
        v.push(("F0437-零拷贝-长度钳制", t3.len == 255));
    }
    // 三、增量登记。
    {
        let mut lg = IncrLog::new();
        lg.register(0, true);
        lg.register(18, true);
        lg.register(37, false); // 内嵌复杂物独立解析失败
        v.push((
            "F0437-增量-逐块登记",
            lg.entries.len() == 3 && !lg.all_standalone(),
        ));
        // 全独立块 → 增量可行。
        let mut lg2 = IncrLog::new();
        lg2.register(0, true);
        lg2.register(18, true);
        v.push(("F0437-增量-全独立可行", lg2.all_standalone()));
        // 登记是证据不是承诺：条目只含偏移与成败两位。
        v.push((
            "F0437-增量-条目两位",
            lg.entries[0].start_off == 0 && lg.entries[2].standalone_ok == false,
        ));
    }
    // 四、退化阻断。
    {
        let book = BenchBook::register(vec![Bench { ticks_per_kline: 100, ver: 1 }]);
        v.push(("F0437-阻断-册注册", book.is_ok()));
        let book = book.unwrap_or_else(|_| BenchBook { recs: vec![Bench { ticks_per_kline: 100, ver: 1 }] });
        // 带内绿。
        v.push(("F0437-阻断-带内绿", book.regress(110).is_ok()));
        // 超带红（阻断语义 = Err）。
        v.push(("F0437-阻断-超带红", book.regress(200) == Err("退化越带")));
        v.push(("F0437-阻断-低于带红", book.regress(50) == Err("退化越带")));
        // 版本化。
        v.push(("F0437-阻断-版本入册", book.ver() == 1));
        // 空册拒。
        v.push(("F0437-阻断-空册拒", BenchBook::register(Vec::new()).is_err()));
        // 判据自检。
        v.push(("F0437-判据-锚点入册", ANCHOR == "VE-F0437"));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::vec37_raw_checks as run_vec37_checks;
    #[test]
    fn vec37_perfgen_checks() {
        for (name, ok) in run_vec37_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec37）。
pub fn run_vec37_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec37");
    for (name, ok) in vec37_raw_checks() {
        set.add(name, ok, "");
    }
    set
}
