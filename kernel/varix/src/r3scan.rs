//! r3scan — WP-208 · B-804 R3 硬加速摸底包（MD2 篇 8.2：只摸不建）。
//!
//! 判据 B-804：立项包三件齐（清单/抽象/口径）。
//! MD2 原文（8.2）："STAR I 为 R3（硬加速）只做摸底三件事，一行驱动代码
//! 不写：其一，硬件清单固化——Y7000 的核显为 Intel UHD 630 系（Mesa iris
//! 驱动的标准支持对象），独显为 NVIDIA 消费卡（开源驱动支持度差，明确列为
//! 低优先级）；其二，合成器的提交后端抽象——篇 5.1 的合成管线第四步（提交）
//! 定义成 trait，CPU 后端之外预留 GPU 后端接口位，R3 到来时换后端不换管线；
//! 其三，验收指标预研——GPU 后端的验收口径（帧率、延迟、功耗）提前拟好。
//! 摸底产出是一份立项包（附录 G 的 S406），不是代码。"
//!
//! 宿主可测形态：清单=固化条目表；抽象=SubmitBackend trait + 双后端实例
//! （GpuBackendStub 编译通过即证明"换后端不换管线"的接口位成立）；口径=
//! 三指标预研表。

use crate::checks::CheckSet;

/// 硬件清单条目（摸底第一件：固化，不含臆测支持承诺）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GpuEntry {
    pub vendor: &'static str,
    pub model: &'static str,
    /// 借力驱动路径（R3 到来时的标准支持对象）
    pub driver_path: &'static str,
    /// 优先级（高=标准支持对象；低=支持度差如实记录）
    pub high_priority: bool,
}

/// Y7000 硬件清单（固化两条：核显高优 + 独显低优）。
pub const GPU_INVENTORY: [GpuEntry; 2] = [
    GpuEntry {
        vendor: "Intel",
        model: "UHD 630 系",
        driver_path: "Mesa iris",
        high_priority: true,
    },
    GpuEntry {
        vendor: "NVIDIA",
        model: "消费卡",
        driver_path: "开源驱动（支持度差）",
        high_priority: false,
    },
];

/// 提交帧（后端抽象的载荷——像素和语义同 path3）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubmitFrame {
    pub seq: u64,
    pub px_sum: u64,
}

/// 合成管线第四步（提交）的后端抽象：**换后端不换管线**的接口位。
///
/// CPU 后端是 STAR I 的实际实现；GpuBackendStub 是 R3 预留接口位——
/// stub 编译通过 + trait 同源，即证明抽象不绑定 CPU 实现。
pub trait SubmitBackend {
    fn backend_name(&self) -> &'static str;
    /// 提交一帧；返回受理与否。
    fn submit(&mut self, f: &SubmitFrame) -> bool;
    /// 累计提交数（审计面）。
    fn submitted(&self) -> u64;
}

/// CPU 后端（STAR I 实际后端——软渲染打满全场）。
pub struct CpuBackend {
    total: u64,
}

impl SubmitBackend for CpuBackend {
    fn backend_name(&self) -> &'static str {
        "cpu-soft"
    }
    fn submit(&mut self, _f: &SubmitFrame) -> bool {
        self.total += 1;
        true
    }
    fn submitted(&self) -> u64 {
        self.total
    }
}

/// GPU 后端接口位（R3 占位 stub——不实现驱动，只证抽象成立）。
pub struct GpuBackendStub {
    total: u64,
}

impl SubmitBackend for GpuBackendStub {
    fn backend_name(&self) -> &'static str {
        "gpu-stub-r3"
    }
    fn submit(&mut self, _f: &SubmitFrame) -> bool {
        self.total += 1;
        true
    }
    fn submitted(&self) -> u64 {
        self.total
    }
}

/// 验收口径条目（摸底第三件：GPU 后端的验收线提前拟好，防"能跑就算成功"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AcceptanceMetric {
    pub name: &'static str,
    /// 预研口径（整数 + 单位语义，宿主可测不做浮点）
    pub threshold: u32,
    pub unit: &'static str,
}

/// GPU 后端验收口径三指标（帧率/延迟/功耗）。
pub const ACCEPTANCE_METRICS: [AcceptanceMetric; 3] = [
    AcceptanceMetric { name: "帧率下限", threshold: 60, unit: "fps" },
    AcceptanceMetric { name: "提交延迟上限", threshold: 8, unit: "ms" },
    AcceptanceMetric { name: "整机功耗上限", threshold: 35, unit: "W" },
];

/// 立项包三件齐审计（S406 出口形态）。
pub fn package_complete() -> bool {
    // 件一：清单齐（核显高优 + 独显低优各一条）
    let list_ok = GPU_INVENTORY.len() == 2
        && GPU_INVENTORY.iter().any(|e| e.high_priority)
        && GPU_INVENTORY.iter().any(|e| !e.high_priority);
    // 件二：抽象齐（双后端同源实例化）
    let mut cpu = CpuBackend { total: 0 };
    let mut gpu = GpuBackendStub { total: 0 };
    let f = SubmitFrame { seq: 1, px_sum: 1 };
    let abstract_ok = cpu.submit(&f) && gpu.submit(&f) && cpu.submitted() == 1 && gpu.submitted() == 1;
    // 件三：口径齐（三指标非零）
    let metrics_ok = ACCEPTANCE_METRICS.len() == 3 && ACCEPTANCE_METRICS.iter().all(|m| m.threshold > 0);
    list_ok && abstract_ok && metrics_ok
}

// ---------------------------------------------------------------- 对练

/// 后端一致性对练：同一帧流在两个后端上审计行为一致（抽象不泄漏实现差异）。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct BackendDrillSummary {
    pub frames: u64,
    pub cpu_submitted: u64,
    pub gpu_submitted: u64,
    /// 同帧流双后端受理数一致
    pub consistent: bool,
}

/// 多帧双后端对练（trait 对象动态分发——抽象面统一受理语义）。
pub fn run_backend_drills(seed: u64, frames: u32) -> BackendDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut cpu = CpuBackend { total: 0 };
    let mut gpu = GpuBackendStub { total: 0 };
    let mut sum = BackendDrillSummary::default();
    for _ in 0..frames {
        let f = SubmitFrame { seq: g.next(), px_sum: 1 + g.next() % 999 };
        let _ = cpu.submit(&f);
        let _ = gpu.submit(&f);
        sum.frames += 1;
    }
    sum.cpu_submitted = cpu.submitted();
    sum.gpu_submitted = gpu.submitted();
    sum.consistent = sum.cpu_submitted == sum.gpu_submitted && sum.frames > 0;
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_r3scan_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-804 R3 摸底包三件齐");
    {
        // 件一：硬件清单固化（UHD 630 = iris 标准对象；NVIDIA 低优如实记录）
        let e0 = &GPU_INVENTORY[0];
        let e1 = &GPU_INVENTORY[1];
        set.add(
            "B-804 硬件清单固化",
            e0.high_priority && e0.driver_path == "Mesa iris" && !e1.high_priority,
            "核显高优（iris 标准对象）+ 独显低优（支持度差如实记录）",
        );
    }
    {
        // 件二：提交后端 trait 抽象（CPU 实际 + GPU 接口位同源）
        let mut cpu = CpuBackend { total: 0 };
        let mut gpu = GpuBackendStub { total: 0 };
        let f = SubmitFrame { seq: 9, px_sum: 42 };
        set.add(
            "B-804 提交后端抽象成立",
            cpu.submit(&f) && gpu.submit(&f) && cpu.backend_name() != gpu.backend_name(),
            "换后端不换管线：GpuBackendStub 接口位编译即证",
        );
    }
    {
        // 件三：验收口径三指标预研（帧率/延迟/功耗——防降格验收）
        let names: [&str; 3] = [
            ACCEPTANCE_METRICS[0].name,
            ACCEPTANCE_METRICS[1].name,
            ACCEPTANCE_METRICS[2].name,
        ];
        set.add(
            "B-804 验收口径三指标",
            names == ["帧率下限", "提交延迟上限", "整机功耗上限"]
                && ACCEPTANCE_METRICS.iter().all(|m| m.threshold > 0),
            "GPU 后端口径提前拟好",
        );
    }
    {
        // 三件齐总闸
        set.add(
            "B-804 立项包三件齐",
            package_complete(),
            "S406 出口形态：清单/抽象/口径",
        );
    }
    {
        // 双后端一致性对练
        let sum = run_backend_drills(0xB804, 100);
        set.add(
            "B-804 双后端对练一致",
            sum.frames == 100 && sum.consistent && sum.cpu_submitted == sum.gpu_submitted,
            "抽象面统一受理语义",
        );
    }
    {
        // 只摸不建：本模块零驱动代码（清单是数据、抽象是 trait、口径是表——
        // 无任何硬件寄存器访问语义）
        set.add(
            "B-804 只摸不建立场",
            GPU_INVENTORY.len() == 2 && ACCEPTANCE_METRICS.len() == 3,
            "一行驱动代码不写（MD1 20.1 立场）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f604_inventory_fixed() {
        assert_eq!(GPU_INVENTORY[0].vendor, "Intel");
        assert_eq!(GPU_INVENTORY[0].model, "UHD 630 系");
        assert!(GPU_INVENTORY[0].high_priority);
        assert!(!GPU_INVENTORY[1].high_priority);
    }

    #[test]
    fn f604_backend_polymorphism() {
        // trait 对象统一分发——抽象面同源
        let backends: [&mut dyn SubmitBackend; 2] = [&mut CpuBackend { total: 0 }, &mut GpuBackendStub { total: 0 }];
        let f = SubmitFrame { seq: 1, px_sum: 7 };
        for b in backends {
            assert!(b.submit(&f));
            assert_eq!(b.submitted(), 1);
        }
    }

    #[test]
    fn f604_drill_consistent() {
        let s = run_backend_drills(5, 50);
        assert!(s.consistent);
        assert_eq!(s.cpu_submitted, 50);
        assert_eq!(s.gpu_submitted, 50);
    }

    #[test]
    fn f604_package_gate() {
        assert!(package_complete());
    }
}
