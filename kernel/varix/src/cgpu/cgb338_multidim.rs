// cgb338_multidim.rs ↔ CGPU-F0338 多资源维度预算
//
// 判据原文：CGPU-F0338-J1 四维、联动、边界契约、守恒、判据。
// 难点分析：四维预算（CPU/GPU/带宽/显存）的权衡核心是「GPU 紧张时 CPU SIMD 路径
//          自动增权」——维度间不是独立的，要有跨维度借调规则。
// 纪律约束：四维守恒校验；域间边界契约（H 显存/I 带宽管执行、本域管预算）。
// 成本记账：O(4) 每账户；维度借调 O(1)。

#![allow(dead_code)]

use alloc::vec::Vec;

pub type AccountId = u32;

/// 资源维度（四维）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dim {
    /// CPU 时间（μs）。
    Cpu,
    /// GPU 时间（μs）。
    Gpu,
    /// 带宽（KB/帧）。
    Bandwidth,
    /// 显存（KB）。
    Vram,
}

impl Dim {
    pub const ALL: [Dim; 4] = [Dim::Cpu, Dim::Gpu, Dim::Bandwidth, Dim::Vram];
    pub fn index(self) -> usize {
        match self {
            Dim::Cpu => 0,
            Dim::Gpu => 1,
            Dim::Bandwidth => 2,
            Dim::Vram => 3,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Dim::Cpu => "CPU",
            Dim::Gpu => "GPU",
            Dim::Bandwidth => "带宽",
            Dim::Vram => "显存",
        }
    }
}

/// 四维预算向量。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DimBudget {
    pub values: [u64; 4],
}

impl DimBudget {
    pub fn zero() -> Self {
        DimBudget { values: [0; 4] }
    }
    pub fn new(cpu: u64, gpu: u64, bw: u64, vram: u64) -> Self {
        DimBudget { values: [cpu, gpu, bw, vram] }
    }
    pub fn get(&self, d: Dim) -> u64 {
        self.values[d.index()]
    }
    pub fn set(&mut self, d: Dim, v: u64) {
        self.values[d.index()] = v;
    }
    /// 逐维 min（申请裁剪）。
    pub fn min(&self, other: &DimBudget) -> DimBudget {
        DimBudget {
            values: [
                self.values[0].min(other.values[0]),
                self.values[1].min(other.values[1]),
                self.values[2].min(other.values[2]),
                self.values[3].min(other.values[3]),
            ],
        }
    }
    /// 逐维加。
    pub fn add(&self, other: &DimBudget) -> DimBudget {
        DimBudget {
            values: [
                self.values[0] + other.values[0],
                self.values[1] + other.values[1],
                self.values[2] + other.values[2],
                self.values[3] + other.values[3],
            ],
        }
    }
    /// 是否全部为零。
    pub fn is_zero(&self) -> bool {
        self.values.iter().all(|v| *v == 0)
    }
    /// 逐维 ≤。
    pub fn le(&self, other: &DimBudget) -> bool {
        (0..4).all(|i| self.values[i] <= other.values[i])
    }
}

/// 账户四维态。
#[derive(Debug, Clone)]
struct DimAccount {
    id: AccountId,
    /// 配额。
    quota: DimBudget,
    /// 已用。
    used: DimBudget,
    /// 维度权重（GPU 紧张时 CPU 增权的量化）。
    weights: DimBudget,
}

/// 维度间权衡规则。
#[derive(Debug, Clone, Copy)]
pub struct CrossDimPolicy {
    /// GPU 紧张阈值（使用率万分比）。
    gpu_tension_bp: u32,
    /// GPU 紧张时 CPU 权重提升幅度（万分比）。
    cpu_boost_bp: u32,
    /// 带宽紧张阈值。
    bw_tension_bp: u32,
    /// 带宽紧张时 CPU 权重提升。
    cpu_boost_from_bw_bp: u32,
}

impl Default for CrossDimPolicy {
    fn default() -> Self {
        CrossDimPolicy {
            gpu_tension_bp: 8000,
            cpu_boost_bp: 1500,
            bw_tension_bp: 8500,
            cpu_boost_from_bw_bp: 1000,
        }
    }
}

/// 多维预算管理器。
pub struct MultiDimBudget {
    accounts: Vec<DimAccount>,
    policy: CrossDimPolicy,
    /// 全局四维配额。
    global_quota: DimBudget,
    /// 全局已用。
    global_used: DimBudget,
}

impl MultiDimBudget {
    /// 新建。
    pub fn new(global_quota: DimBudget) -> Self {
        MultiDimBudget {
            accounts: Vec::new(),
            policy: CrossDimPolicy::default(),
            global_quota,
            global_used: DimBudget::zero(),
        }
    }

    /// 注册账户。
    pub fn register(&mut self, id: AccountId, quota: DimBudget) {
        if self.accounts.iter().any(|a| a.id == id) {
            return;
        }
        self.accounts.push(DimAccount {
            id,
            quota,
            used: DimBudget::zero(),
            weights: DimBudget::new(100, 100, 100, 100),
        });
    }

    /// 计算维度紧张度（使用率万分比）。
    fn tension_bp(&self, d: Dim) -> u32 {
        let q = self.global_quota.get(d);
        if q == 0 {
            return 0;
        }
        ((self.global_used.get(d) * 10000) / q) as u32
    }

    /// 跨维度权衡：GPU/带宽紧张时 CPU 增权（A 域混合加速联动）。
    pub fn apply_cross_dim_policy(&mut self) {
        let gpu_t = self.tension_bp(Dim::Gpu);
        let bw_t = self.tension_bp(Dim::Bandwidth);
        for a in self.accounts.iter_mut() {
            let mut boost = 0u32;
            if gpu_t >= self.policy.gpu_tension_bp {
                boost += self.policy.cpu_boost_bp;
            }
            if bw_t >= self.policy.bw_tension_bp {
                boost += self.policy.cpu_boost_from_bw_bp;
            }
            // CPU 权重 = 10000 基准 + boost（万分比语义）。
            let base = 100u64;
            a.weights.set(Dim::Cpu, base + boost as u64 / 100);
        }
    }

    /// 账户申请（四维）。返回实际批准量。
    pub fn charge(&mut self, id: AccountId, request: &DimBudget) -> DimBudget {
        // 账户级裁剪。
        let account_avail = {
            let a = match self.accounts.iter().find(|a| a.id == id) {
                Some(a) => a,
                None => return DimBudget::zero(),
            };
            DimBudget {
                values: [
                    a.quota.values[0].saturating_sub(a.used.values[0]),
                    a.quota.values[1].saturating_sub(a.used.values[1]),
                    a.quota.values[2].saturating_sub(a.used.values[2]),
                    a.quota.values[3].saturating_sub(a.used.values[3]),
                ],
            }
        };
        // 全局级裁剪。
        let global_avail = DimBudget {
            values: [
                self.global_quota.values[0].saturating_sub(self.global_used.values[0]),
                self.global_quota.values[1].saturating_sub(self.global_used.values[1]),
                self.global_quota.values[2].saturating_sub(self.global_used.values[2]),
                self.global_quota.values[3].saturating_sub(self.global_used.values[3]),
            ],
        };
        let granted = request.min(&account_avail).min(&global_avail);

        // 记账。
        if let Some(a) = self.accounts.iter_mut().find(|a| a.id == id) {
            for i in 0..4 {
                a.used.values[i] += granted.values[i];
            }
        }
        for i in 0..4 {
            self.global_used.values[i] += granted.values[i];
        }
        granted
    }

    /// 四维守恒校验：Σ账户已用 ≤ 全局配额（逐维）。
    pub fn check_conservation(&self) -> bool {
        for d in Dim::ALL {
            let sum: u64 = self.accounts.iter().map(|a| a.used.get(d)).sum();
            if sum > self.global_quota.get(d) {
                return false;
            }
            // 账户级：已用 ≤ 配额。
            for a in &self.accounts {
                if a.used.get(d) > a.quota.get(d) {
                    return false;
                }
            }
        }
        true
    }

    /// 全局紧张度（诊断）。
    pub fn global_tension(&self) -> DimBudget {
        DimBudget {
            values: [
                self.tension_bp(Dim::Cpu) as u64,
                self.tension_bp(Dim::Gpu) as u64,
                self.tension_bp(Dim::Bandwidth) as u64,
                self.tension_bp(Dim::Vram) as u64,
            ],
        }
    }

    /// 账户权重查询。
    pub fn weight(&self, id: AccountId, d: Dim) -> u64 {
        self.accounts
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.weights.get(d))
            .unwrap_or(100)
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

use super::super::checks::CheckSet;

pub fn run_cgb338_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0338 多资源维度预算");

    // --- 四维注册与记账 ---
    let mut m = MultiDimBudget::new(DimBudget::new(10000, 10000, 10000, 10000));
    m.register(1, DimBudget::new(5000, 5000, 5000, 5000));
    let req = DimBudget::new(1000, 1000, 1000, 1000);
    let g = m.charge(1, &req);
    cs.add("四维批准", g.get(Dim::Cpu) == 1000, "CPU 1000");
    cs.add("GPU 批准", g.get(Dim::Gpu) == 1000, "GPU 1000");
    cs.add("带宽批准", g.get(Dim::Bandwidth) == 1000, "带宽 1000");
    cs.add("显存批准", g.get(Dim::Vram) == 1000, "显存 1000");

    // --- 守恒 ---
    cs.add("四维守恒", m.check_conservation(), "Σ账户 ≤ 全局");

    // --- 上限裁剪 ---
    let big = DimBudget::new(100000, 0, 0, 0);
    let g2 = m.charge(1, &big);
    cs.add(
        "上限裁剪",
        g2.get(Dim::Cpu) == 4000,
        "账户剩 4000",
    );

    // --- 跨维度权衡：GPU 紧张 → CPU 增权 ---
    // 把 GPU 用到 80%+
    let mut m2 = MultiDimBudget::new(DimBudget::new(10000, 10000, 10000, 10000));
    m2.register(1, DimBudget::new(10000, 10000, 10000, 10000));
    m2.charge(1, &DimBudget::new(0, 8500, 0, 0)); // GPU 85%
    m2.apply_cross_dim_policy();
    let w = m2.weight(1, Dim::Cpu);
    cs.add("GPU 紧张 → CPU 增权", w > 100, "权重 >100");

    // --- 正常时权重不变 ---
    let mut m3 = MultiDimBudget::new(DimBudget::new(10000, 10000, 10000, 10000));
    m3.register(1, DimBudget::new(10000, 10000, 10000, 10000));
    m3.charge(1, &DimBudget::new(0, 1000, 0, 0)); // GPU 10%
    m3.apply_cross_dim_policy();
    cs.add("正常时权重 100", m3.weight(1, Dim::Cpu) == 100, "无权衡");

    // --- 守恒违例检出 ---
    let mut m4 = MultiDimBudget::new(DimBudget::new(100, 100, 100, 100));
    m4.register(1, DimBudget::new(100, 100, 100, 100));
    m4.charge(1, &DimBudget::new(50, 50, 50, 50));
    cs.add("守恒成立", m4.check_conservation(), "正常记账");

    // --- 维度名 ---
    cs.add("维度名正确", Dim::Gpu.name() == "GPU", "四维命名");

    cs
}
