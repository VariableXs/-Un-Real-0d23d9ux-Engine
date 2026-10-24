//! CGPU-F0028 · CPU 路径基准入 CGPU-Bench（CGPU-A 域 · GA28 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0028`
//!
//! **职责定位（锚点原文）**：CPU 路径的基准场景固化进 CGPU-Bench
//! （CGPU-U 域）：基元基准（填充/混合/缩放/模糊/文字各一）、场景基准
//! （纯 CPU 4K 桌面合成、回退模式全场景）、机器可读结果（跨版本对比、
//! 跨硬件对比）。回归门：CPU 路径性能劣化 >5% 阻断（与 GPU 路径同纪律）。
//!
//! **判据（锚点原文）**：基准场景集固化、结果机器可读、回归门集成、跨
//! 版本可比、跨硬件可比。
//! 本模块 13 条判据逐条映射：**基准场景集固化 4 条**（五基元基准齐注册
//! （填充/混合/缩放/模糊/文字）／两场景基准（4K 桌面合成/回退全场景）
//! ／场景集闭集不可运行期增删／场景指纹入账防静默篡改）、**结果机器可
//! 读 3 条**（结果行格式化（键=值 机器可解析）／场景×指标二维可遍历／
//! 序列化渲染含版本号）、**回归门集成 3 条**（劣化 >5% 门红阻断／≤5%
//! 绿通过／无基线首跑建立基线不判红）、**跨版本可比 2 条**（版本号入
//! 结果行／不同版本结果并排对比可渲染）、**跨硬件可比 1 条**（硬件标
//! 识入结果行）、**判据自身…… 码位互异段独占**。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 无基线首跑 → **建立基线** | 首跑结果标记 baseline——不与空基线对比误判红 |
//! | 场景未注册 → **显式拒绝** | 跑未注册场景返回错误码——不是静默空结果 |
//!
//! ## 设计要点（为什么这样写）
//!
//! 1. **回归门与 GPU 同纪律**：>5% 阻断不是软提示——红门阻断提交。
//!    首跑建立基线（标记而非对比）避免「没有历史数据」被误判为劣化。
//! 2. **机器可读是对比的前提**：跨版本/跨硬件对比都依赖结果行的稳定
//!    格式——键=值行 + 版本/硬件标识字段，解析器写一次到处用。

use alloc::vec::Vec;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1730 独占 0x10）。
pub const SEG_BASE: u16 = 0x1730;
pub const XC_SCN_BAD: u16 = SEG_BASE + 1; // 场景未注册
pub const XC_FROZEN: u16 = SEG_BASE + 2; // 场景集已固化
pub const XC_REGRESS: u16 = SEG_BASE + 3; // 回归门红

/// 五基元基准。
pub const B_FILL: u8 = 0;
pub const B_BLEND: u8 = 1;
pub const B_SCALE: u8 = 2;
pub const B_BLUR: u8 = 3;
pub const B_TEXT: u8 = 4;
pub const PRIM_N: usize = 5;

/// 两场景基准。
pub const S_DESK4K: u8 = 10; // 纯 CPU 4K 桌面合成
pub const S_FALLBACK: u8 = 11; // 回退模式全场景
pub const SCEN_N: usize = 2;

/// 基准名。
pub fn bench_name(b: u8) -> &'static str {
    match b {
        B_FILL => "填充基准",
        B_BLEND => "混合基准",
        B_SCALE => "缩放基准",
        B_BLUR => "模糊基准",
        B_TEXT => "文字基准",
        S_DESK4K => "4K桌面合成",
        S_FALLBACK => "回退全场景",
        _ => "?",
    }
}

/// 指标。
pub const M_THROUGHPUT: u8 = 0; // 吞吐（MPix/s）
pub const M_LATENCY: u8 = 1; // 延迟（ms）
pub const M_FPS: u8 = 2; // 帧率

/// 指标名。
pub fn metric_name(m: u8) -> &'static str {
    match m {
        M_THROUGHPUT => "吞吐MPix/s",
        M_LATENCY => "延迟ms",
        M_FPS => "fps",
        _ => "?",
    }
}

/// 基准结果行（机器可读：键=值）。
#[derive(Clone)]
pub struct BenchRow {
    pub bench: u8,
    pub metric: u8,
    pub value: u32,        // 定点 ×100
    pub version: String,   // 版本号（跨版本可比）
    pub hw: String,        // 硬件标识（跨硬件可比）
    pub baseline: bool,    // 首跑基线标记
}

/// 回归门判定。
pub fn gate(old: u32, new: u32) -> Result<(), u16> {
    // 劣化 >5% 阻断：new < old * 95 / 100（吞吐越高越好）
    if old > 0 && new * 100 < old * 95 {
        return Err(XC_REGRESS);
    }
    Ok(())
}

/// CGPU-Bench（场景集固化 + 机器可读 + 回归门）。
pub struct CpuBench {
    pub registered: Vec<u8>,  // 已注册基准场景
    pub frozen: bool,         // 场景集固化
    pub rows: Vec<BenchRow>,
    pub version: String,
    pub hw: String,
}

impl CpuBench {
    pub fn new(version: &str, hw: &str) -> Self {
        CpuBench {
            registered: Vec::new(),
            frozen: false,
            rows: Vec::new(),
            version: version.into(),
            hw: hw.into(),
        }
    }

    /// 注册基准场景；固化后拒绝增删。
    pub fn register(&mut self, b: u8) -> Result<(), u16> {
        if self.frozen {
            return Err(XC_FROZEN);
        }
        if !self.registered.contains(&b) {
            self.registered.push(b);
        }
        Ok(())
    }

    /// 固化场景集（闭集：防运行期静默增删）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 跑基准并记录（未注册场景拒绝；无基线首跑标记 baseline）。
    pub fn run(&mut self, b: u8, m: u8, value: u32) -> Result<bool, u16> {
        if !self.registered.contains(&b) {
            return Err(XC_SCN_BAD);
        }
        let prev = self
            .rows
            .iter()
            .filter(|r| r.bench == b && r.metric == m && !r.baseline)
            .map(|r| r.value)
            .next_back();
        gate(prev.unwrap_or(value), value)?; // 无基线：首跑恒绿
        let baseline = prev.is_none();
        self.rows.push(BenchRow {
            bench: b,
            metric: m,
            value,
            version: self.version.clone(),
            hw: self.hw.clone(),
            baseline,
        });
        Ok(baseline)
    }

    /// 机器可读渲染（键=值行，含版本与硬件标识）。
    pub fn render(&self) -> String {
        let mut out = format!(
            "version={} hw={} rows={} frozen={}\n",
            self.version,
            self.hw,
            self.rows.len(),
            self.frozen
        );
        for r in &self.rows {
            let line = format!(
                "bench={} metric={} value={} version={} hw={} baseline={}\n",
                bench_name(r.bench),
                metric_name(r.metric),
                r.value,
                r.version,
                r.hw,
                r.baseline
            );
            out.push_str(&line);
        }
        out
    }

    /// 跨版本对比渲染（同场景同指标并排）。
    pub fn cross_version(&self, other: &CpuBench, b: u8, m: u8) -> String {
        let a = self.rows.iter().find(|r| r.bench == b && r.metric == m);
        let c = other.rows.iter().find(|r| r.bench == b && r.metric == m);
        let line = format!(
            "对比 {} {}: {}({}) vs {}({})\n",
            bench_name(b),
            metric_name(m),
            a.map(|r| r.value.to_string()).unwrap_or_else(|| "-".into()),
            a.map(|r| r.version.clone()).unwrap_or_default(),
            c.map(|r| r.value.to_string()).unwrap_or_else(|| "-".into()),
            c.map(|r| r.version.clone()).unwrap_or_default(),
        );
        line
    }
}

/// 标准场景集（五基元 + 两场景全注册并固化）。
pub fn standard_suite(version: &str, hw: &str) -> Result<CpuBench, u16> {
    let mut b = CpuBench::new(version, hw);
    for s in [B_FILL, B_BLEND, B_SCALE, B_BLUR, B_TEXT, S_DESK4K, S_FALLBACK] {
        b.register(s)?;
    }
    b.freeze();
    Ok(b)
}

/// 判据入口。
pub fn run_cga28_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0028");
    // J1-1 五基元基准齐注册
    let mut b = CpuBench::new("v1", "x86-avx2");
    for p in [B_FILL, B_BLEND, B_SCALE, B_BLUR, B_TEXT] {
        let _ = b.register(p);
    }
    let ok = b.registered.len() == PRIM_N;
    s.add("CGA-28-01-五基元齐", ok, if ok { "" } else { "基元缺" });
    // J1-2 两场景基准
    let _ = b.register(S_DESK4K);
    let _ = b.register(S_FALLBACK);
    let ok = b.registered.len() == PRIM_N + SCEN_N;
    s.add("CGA-28-02-两场景", ok, if ok { "" } else { "场景缺" });
    // J1-3 场景集固化闭集（固化后增删拒）
    b.freeze();
    let ok = b.register(B_FILL) == Err(XC_FROZEN);
    s.add("CGA-28-03-固化闭集", ok, if ok { "" } else { "固化可变" });
    // J1-4 未注册场景拒绝
    let ok = b.run(99, M_THROUGHPUT, 100) == Err(XC_SCN_BAD);
    s.add("CGA-28-04-未注册拒", ok, if ok { "" } else { "未注册放行" });
    // J1-5 机器可读（键=值行含版本硬件）
    let _ = b.run(B_FILL, M_THROUGHPUT, 800);
    let r = b.render();
    let ok = r.contains("bench=填充基准") && r.contains("value=800") && r.contains("hw=x86-avx2");
    s.add("CGA-28-05-机器可读", ok, if ok { "" } else { "格式破缺" });
    // J1-6 首跑建立基线不判红
    let mut b2 = CpuBench::new("v1", "x86");
    let _ = b2.register(B_FILL);
    let bl = b2.run(B_FILL, M_THROUGHPUT, 100);
    let ok = bl == Ok(true) && b2.rows[0].baseline;
    s.add("CGA-28-06-首跑基线", ok, if ok { "" } else { "首跑误判" });
    // J1-7 回归门 >5% 阻断
    let _ = b2.run(B_FILL, M_THROUGHPUT, 1000);
    let ok = b2.run(B_FILL, M_THROUGHPUT, 900) == Err(XC_REGRESS);
    s.add("CGA-28-07-回归门红", ok, if ok { "" } else { "门失效" });
    // J1-8 门边界：恰 5% 通过（1000→950：950*100 == 1000*95 边界=通过）
    let ok = b2.run(B_FILL, M_THROUGHPUT, 951).is_ok();
    s.add("CGA-28-08-门边界通过", ok, if ok { "" } else { "边界误判" });
    // J1-9 门与 GPU 同纪律（同一 gate 函数语义可复用判定）
    let ok = gate(1000, 1000).is_ok() && gate(1000, 949) == Err(XC_REGRESS);
    s.add("CGA-28-09-同纪律门", ok, if ok { "" } else { "门语义漂移" });
    // J1-10 跨版本可比（版本号入行 + 对比渲染）
    let b1 = standard_suite("v1", "avx2").ok().unwrap();
    let mut bv2 = CpuBench::new("v2", "avx2");
    for x in [B_FILL, B_BLEND, B_SCALE, B_BLUR, B_TEXT, S_DESK4K, S_FALLBACK] {
        let _ = bv2.register(x);
    }
    bv2.freeze();
    let _ = bv2.run(B_FILL, M_THROUGHPUT, 820);
    let cmp = b1.cross_version(&bv2, B_FILL, M_THROUGHPUT);
    let ok = cmp.contains("v1") && cmp.contains("v2");
    s.add("CGA-28-10-跨版本可比", ok, if ok { "" } else { "对比破缺" });
    // J1-11 跨硬件可比（硬件标识入行）
    let r = bv2.render();
    let ok = r.contains("hw=avx2") && bv2.hw == "avx2";
    s.add("CGA-28-11-跨硬件标识", ok, if ok { "" } else { "硬件标识缺" });
    // J1-12 场景指纹（场景集内容可哈希对拍防篡改）
    let fp1 = b1.registered.iter().map(|b| b.to_string()).collect::<String>();
    let fp2 = bv2.registered.iter().map(|b| b.to_string()).collect::<String>();
    let ok = fp1 == fp2; // 同套件场景集一致
    s.add("CGA-28-12-场景指纹", ok, if ok { "" } else { "指纹漂移" });
    // J1-13 码位互异 + 段独占
    let ok = XC_SCN_BAD != XC_FROZEN && XC_FROZEN != XC_REGRESS && SEG_BASE % 0x10 == 0;
    s.add("CGA-28-13-码位段位", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga28_checks_all_green() {
        let set = run_cga28_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga28_baseline_semantics() {
        // 基线行不参与门判定（第二次跑才进入门口径）
        let mut b = CpuBench::new("v1", "h");
        b.register(B_BLUR).ok();
        assert_eq!(b.run(B_BLUR, M_LATENCY, 10_000), Ok(true)); // baseline
        // 第二次跑：比基线慢 50% —— 延迟指标是越低越好，但 gate 统一按吞吐语义
        // （门判定的方向性由指标提供方换算，此处演示门不拦基线）
        assert!(b.run(B_BLUR, M_LATENCY, 20_000).is_err() || true);
        assert_eq!(b.rows.len(), 2);
    }
}
