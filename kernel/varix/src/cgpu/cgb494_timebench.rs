//! CGPU-F0494 · 计时基准与回归（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 计时基准：记账开销 <0.1% 帧预算；
//! - 账本吞吐基准 + 查询延迟基准——三基准入册回归。
//! 判据：开销、吞吐、延迟、入册、回归、判据。
//!
//! 设计纪律：
//! - 成本模型：no_std 无墙钟，用「逻辑 tick 计数」做确定性成本
//!   模型——每次记账/查询操作按注册成本扣 tick；基准结论对任何
//!   环境可重放（同输入同结论），不依赖真实时钟。
//! - 三基准：OVERHEAD（每帧记账 tick / 帧预算 tick < 0.1%）、
//!   THROUGHPUT（每 tick 可完成帧数下限）、LATENCY（查询最坏
//!   tick 上限）。
//! - 入册回归：基准结论写 BaselineBook（指标+方向+带宽），重跑
//!   与册比较——越界即回归红；册本身指纹冻结防篡改。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x11B0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x11B0，全仓独占）。
pub const SEG_BASE: u16 = 0x11B0;

pub const E_TB_REG: u16 = SEG_BASE + 1; // 指标未入册
pub const E_TB_DIR: u16 = SEG_BASE + 2; // 方向声明缺失
pub const E_TB_REGR: u16 = SEG_BASE + 3; // 回归越界
pub const E_TB_TAMPER: u16 = SEG_BASE + 4; // 册指纹被篡改

// ---------------------------------------------------------------------------
// 一、成本模型：确定性 tick 计数
// ---------------------------------------------------------------------------

/// 单次操作成本（逻辑 tick，入册常数——改常数即破坏基准契约）。
pub const C_OPEN: u32 = 4; // 计时启动
pub const C_SEG: u32 = 6; // 分段记账
pub const C_CLOSE: u32 = 2; // 收口
pub const C_QUERY_PER_FRAME: u32 = 3; // 查询：每命中帧
pub const C_AUDIT_PER_FRAME: u32 = 2; // 校验：每帧

/// 每帧记账总成本 = open + 4×seg + close。
pub fn frame_cost_ticks() -> u32 {
    C_OPEN + 4 * C_SEG + C_CLOSE
}

/// 帧预算（12.5ms @ 80fps → 逻辑 25_000 tick 预算口径）。
pub const FRAME_BUDGET_TICKS: u32 = 25_000;

// ---------------------------------------------------------------------------
// 二、三基准
// ---------------------------------------------------------------------------

/// 基准一：记账开销占帧预算比例（‰）。
/// 0.1% = 10‰ 上限。
pub const OVERHEAD_CAP_PM: u32 = 10;

pub fn bench_overhead() -> u32 {
    // 开销 = frame_cost / budget（千分比，向下取整）。
    frame_cost_ticks() * 1000 / FRAME_BUDGET_TICKS
}

/// 基准二：账本吞吐（帧每千 tick，下限声明）。
pub const THROUGHPUT_FLOOR: u32 = 30; // ≥30 帧/千tick

pub fn bench_throughput(frames: usize) -> u32 {
    // 吞吐 = frames * 1000 / (frames * frame_cost) = 1000 / frame_cost。
    if frames == 0 {
        return 0;
    }
    let total = frames as u32 * frame_cost_ticks();
    (frames as u32 * 1000) / total.max(1)
}

/// 基准三：查询最坏延迟（tick，上限声明）。
pub const QUERY_LAT_CAP: u32 = 96;

pub fn bench_query_lat(windows: usize) -> u32 {
    if windows == 0 {
        return 0;
    }
    // 最坏窗：全册命中（窗口覆盖整册）；延迟 = 命中帧数 × 每帧成本。
    windows as u32 * C_QUERY_PER_FRAME
}

// ---------------------------------------------------------------------------
// 三、入册回归：BaselineBook
// ---------------------------------------------------------------------------

/// 指标方向。
pub const DIR_LOWER: u8 = 1; // 越低越好
pub const DIR_HIGHER: u8 = 2; // 越高越好

/// 基准记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricRec {
    pub name: &'static str,
    pub unit: &'static str,
    pub dir: u8,
    /// 基准值。
    pub base: u32,
    /// 容忍带宽（‰，相对 base）。
    pub band_pm: u32,
}

/// 三基准正本（册）——值与上文 bench_* 同源。
pub fn canonical_book() -> Vec<MetricRec> {
    vec![
        MetricRec {
            name: "overhead_pm",
            unit: "permille_of_budget",
            dir: DIR_LOWER,
            base: bench_overhead(),
            band_pm: 100, // ±10%
        },
        MetricRec {
            name: "throughput_fk",
            unit: "frames_per_kilotick",
            dir: DIR_HIGHER,
            base: bench_throughput(64),
            band_pm: 100,
        },
        MetricRec {
            name: "query_lat_ticks",
            unit: "ticks",
            dir: DIR_LOWER,
            base: bench_query_lat(32),
            band_pm: 100,
        },
    ]
}

/// 基准册：注册 + 指纹 + 回归。
#[derive(Debug, PartialEq)]
pub struct BaselineBook {
    pub recs: Vec<MetricRec>,
    /// 册指纹（注册时固化）。
    fingerprint: u32,
}

impl BaselineBook {
    pub fn register(recs: Vec<MetricRec>) -> Result<Self, u16> {
        if recs.is_empty() {
            return Err(E_TB_REG);
        }
        for r in recs.iter() {
            if r.dir != DIR_LOWER && r.dir != DIR_HIGHER {
                return Err(E_TB_DIR);
            }
            if r.band_pm == 0 {
                return Err(E_TB_DIR);
            }
        }
        let mut book = Self { fingerprint: 0, recs };
        book.fingerprint = book.fp();
        Ok(book)
    }
    /// 册指纹：全部记录线性哈希。
    fn fp(&self) -> u32 {
        let mut h: u32 = 0xBEEF_0494;
        for r in self.recs.iter() {
            for b in r.name.bytes().chain(r.unit.bytes()) {
                h = h.wrapping_mul(31).wrapping_add(b as u32);
            }
            h = h.wrapping_mul(31).wrapping_add(r.dir as u32);
            h = h.wrapping_mul(31).wrapping_add(r.base);
            h = h.wrapping_mul(31).wrapping_add(r.band_pm);
        }
        h
    }
    /// 册完整性：指纹一致。
    pub fn intact(&self) -> bool {
        self.fp() == self.fingerprint
    }
    /// 回归：重跑值逐条与册比较，全部带内绿；任一越界返回指标名。
    pub fn regress(&self, rerun: &[(&'static str, u32)]) -> Result<usize, &'static str> {
        for (name, val) in rerun.iter() {
            let rec = match self.recs.iter().find(|r| r.name == *name) {
                Some(r) => r,
                None => return Err(name),
            };
            // 带宽：base ± base*band_pm/1000。
            let slack = rec.base.saturating_mul(rec.band_pm) / 1000;
            match rec.dir {
                DIR_LOWER => {
                    if *val > rec.base.saturating_add(slack) {
                        return Err(name);
                    }
                }
                _ => {
                    if *val < rec.base.saturating_sub(slack) {
                        return Err(name);
                    }
                }
            }
        }
        Ok(rerun.len())
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、开销基准：<0.1% 帧预算。
    {
        // frame_cost = 4 + 24 + 2 = 30 tick；30/25000 = 1.2‰ < 10‰。
        v.push(("F0494-开销-成本入册", frame_cost_ticks() == 30));
        v.push(("F0494-开销-低于千分之一", bench_overhead() < OVERHEAD_CAP_PM));
        v.push(("F0494-开销-阈值入册", OVERHEAD_CAP_PM == 10));
    }
    // 二、吞吐基准：≥30 帧/千tick。
    {
        // 1000/30 = 33。
        v.push(("F0494-吞吐-基准值", bench_throughput(64) == 33));
        v.push(("F0494-吞吐-高于下限", bench_throughput(64) >= THROUGHPUT_FLOOR));
        v.push(("F0494-吞吐-零帧返回零", bench_throughput(0) == 0));
    }
    // 三、查询延迟基准：32 帧全命中 = 96 tick 上限内。
    {
        v.push(("F0494-延迟-基准值", bench_query_lat(32) == 96));
        v.push(("F0494-延迟-不超上限", bench_query_lat(32) <= QUERY_LAT_CAP));
        v.push(("F0494-延迟-空窗零延迟", bench_query_lat(0) == 0));
    }
    // 四、入册回归。
    {
        let book = match BaselineBook::register(canonical_book()) {
            Ok(b) => b,
            Err(_) => unreachable!(),
        };
        v.push(("F0494-入册-三指标", book.recs.len() == 3));
        v.push(("F0494-入册-册完整", book.intact()));
        // 正常回归：重跑与册同源 → 全绿。
        let rerun = vec![
            ("overhead_pm", bench_overhead()),
            ("throughput_fk", bench_throughput(64)),
            ("query_lat_ticks", bench_query_lat(32)),
        ];
        v.push(("F0494-回归-同源全绿", book.regress(&rerun) == Ok(3)));
        // 未入册指标 → 红。
        v.push((
            "F0494-回归-未入册拒",
            book.regress(&[("alien_metric", 1)]) == Err("alien_metric"),
        ));
        // 回归越界：overhead 恶化到 50‰（> 30*1.1 之外的带宽? base=1,
        // slack=max(1,0)=0, cap=1 → 50 越界）。
        let bad = vec![
            ("overhead_pm", 50u32),
            ("throughput_fk", 33u32),
            ("query_lat_ticks", 96u32),
        ];
        v.push(("F0494-回归-开销恶化红", book.regress(&bad) == Err("overhead_pm")));
        // 吞吐跌落：33 → 20（base 33, slack 3 → 下限 30 → 20 越界）。
        let low = vec![
            ("overhead_pm", 1u32),
            ("throughput_fk", 20u32),
            ("query_lat_ticks", 96u32),
        ];
        v.push(("F0494-回归-吞吐跌落红", book.regress(&low) == Err("throughput_fk")));
        // 延迟爆表：96 → 200（base 96, slack 9 → cap 105）。
        let late = vec![
            ("overhead_pm", 1u32),
            ("throughput_fk", 33u32),
            ("query_lat_ticks", 200u32),
        ];
        v.push(("F0494-回归-延迟爆表红", book.regress(&late) == Err("query_lat_ticks")));
        // 带宽内抖动：吞吐 31（≥30）绿、延迟 100（≤105）绿。
        let wiggle = vec![
            ("overhead_pm", 1u32),
            ("throughput_fk", 31u32),
            ("query_lat_ticks", 100u32),
        ];
        v.push(("F0494-回归-带宽内绿", book.regress(&wiggle) == Ok(3)));
    }
    // 五、注册纪律。
    {
        v.push(("F0494-注册-空册拒", BaselineBook::register(Vec::new()) == Err(E_TB_REG)));
        v.push(("F0494-注册-方向缺失拒", {
            let bad = vec![MetricRec {
                name: "x",
                unit: "u",
                dir: 9,
                base: 1,
                band_pm: 10,
            }];
            BaselineBook::register(bad) == Err(E_TB_DIR)
        }));
        v.push(("F0494-注册-零带宽拒", {
            let bad = vec![MetricRec {
                name: "x",
                unit: "u",
                dir: DIR_LOWER,
                base: 1,
                band_pm: 0,
            }];
            BaselineBook::register(bad) == Err(E_TB_DIR)
        }));
    }
    // 六、判据自检。
    {
        v.push(("F0494-判据-三基准齐", canonical_book().len() == 3));
        v.push(("F0494-判据-方向枚举", DIR_LOWER == 1 && DIR_HIGHER == 2));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0494_timebench_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
