//! CGPU-F1604 · 异构能力探测与登记（K 域 CGPU 段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1604`
//!
//! **判据（锚点原文）**：五步、实现矩阵、缓存键、隔离、空态、诚实、六组、判据。
//!
//! **职责定位（锚点原文）**：能力探测：探测流程（启动探测：枚举可用
//! 引擎→逐引擎能力查询→能力登记表构建——探测五步；探测实现矩阵
//! （NPU：Windows CIM/Intel NPU 驱动接口/AMD XDNA；DSP：平台音频
//! 引擎探测；ISP/MEDIA：驱动查询——矩阵表）；能力缓存（探测结果
//! 缓存（设备不变则复用——缓存键（驱动版本+设备 ID——失效重探；
//! 部分失败（某引擎探测失败→该引擎标记不可用（其余照常——失败
//! 隔离；能力变更（驱动更新→能力表失效重探——热失效复用 F1506 模式；
//! 虚拟化（无 NPU 虚拟机→空能力表+显性——空态语义；诚实登记
//! （能力探测不到=不支持（不猜不造——不造假复用；测试（五步/矩阵/
//! 缓存/隔离/空态/不造假六组）。
//!
//! # 一、探测五步（状态机，步序钉死）
//!
//! [`ProbeStage`] 五态闭集（枚举→查询→登记→缓存→就绪）；步序
//! [`legal_next`] 单移：回退跳步都是越权（没枚举就查询=对空设备
//! 发问）。
//!
//! # 二、能力登记表（真调 vck02 [`EngineKind`]/[`HetCaps`]）
//!
//! [`CapRegistry`] 持每引擎 [`EngineCaps`]（能力真值+驱动版本+设备
//! ID+可用性）；`register` 幂等（同引擎重登=覆盖更新，不重复
//! 入账——重账会让"装了两块卡"这种幻觉成立）。
//!
//! # 三、缓存键与失效（驱动版本+设备 ID 复合键）
//!
//! [`cache_key`] 为 `driver@device` 复合串；`cache_valid` 键变即
//! 失效（复用 F1506 热失效模式：驱动更新=能力表作废重探）。
//!
//! # 四、部分失败隔离
//!
//! [`ProbeFailure`] 分型（超时/查无/忙/不可用）；失败引擎标记
//! 不可用（`available=false`）但条目保留（"探测失败"≠"设备不存在"
//! ——抹掉条目等于谎报空态）；其余引擎照常登记。
//!
//! # 五、空态与诚实登记
//!
//! 无 NPU 虚拟机 → 空能力表 [`CapRegistry::is_empty`] + 显性声明
//! （`virtualized_empty` 标记）；诚实红线 [`honesty_rule`]：探测
//! 不到=不支持，不问猜测不造数据（`RegistrationError` 拒收）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::cgpu::vck02_hetres::{EngineKind, HetCaps, ENGINE_KINDS, FEAT_BATCH};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const PROBE_VERSION: &str = "K04-probe-v1";

/// 超时（探测轮询预算）。
pub const E_PROBE_TIMEOUT: &str = "E_PROBE_TIMEOUT";

/// 查无此设备。
pub const E_PROBE_NO_DEVICE: &str = "E_PROBE_NO_DEVICE";

/// 设备忙（重试窗口未到）。
pub const E_PROBE_BUSY: &str = "E_PROBE_BUSY";

/// 不可用（虚拟化空态/被策略禁用）。
pub const E_PROBE_UNAVAILABLE: &str = "E_PROBE_UNAVAILABLE";

/// 诚实红线：探测不到=不支持（不猜不造）。
pub const E_PROBE_FABRICATED: &str = "E_PROBE_FABRICATED";

/// 步序越权（跳步查询）。
pub const E_PROBE_SEQUENCE: &str = "E_PROBE_SEQUENCE";

/// 探测五步（锚点「探测五步」）。默认态为流水线起点 `Enumerate`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProbeStage {
    /// 枚举可用引擎。
    #[default]
    Enumerate,
    /// 逐引擎能力查询。
    Query,
    /// 能力登记表构建。
    Register,
    /// 探测结果缓存。
    Cache,
    /// 就绪（登记表可用）。
    Ready,
}

impl ProbeStage {
    /// 五态闭集（顺序即册内步序）。
    pub const ALL: [ProbeStage; 5] = [
        ProbeStage::Enumerate,
        ProbeStage::Query,
        ProbeStage::Register,
        ProbeStage::Cache,
        ProbeStage::Ready,
    ];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ProbeStage::Enumerate => "枚举可用引擎",
            ProbeStage::Query => "逐引擎能力查询",
            ProbeStage::Register => "能力登记表构建",
            ProbeStage::Cache => "探测结果缓存",
            ProbeStage::Ready => "就绪",
        }
    }
}

/// 合法步序（当前 → 下一态；None=终态或越权）。
///
/// 越权拦截的意义：没枚举就查询等于对着不存在的设备发问，返回的
/// 「查无」被误读成"设备坏了"而不是"顺序错了"。
pub const fn legal_next(cur: ProbeStage) -> Option<ProbeStage> {
    match cur {
        ProbeStage::Enumerate => Some(ProbeStage::Query),
        ProbeStage::Query => Some(ProbeStage::Register),
        ProbeStage::Register => Some(ProbeStage::Cache),
        ProbeStage::Cache => Some(ProbeStage::Ready),
        ProbeStage::Ready => None,
    }
}

/// 探测失败原因（分型——锚点「失败隔离」的分类面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeFailure {
    /// 轮询超时（设备应答窗口超预算）。
    Timeout,
    /// 查无此设备（驱动返回不存在）。
    NoDevice,
    /// 设备忙。
    Busy,
    /// 虚拟化空态（无 NPU 虚拟机）。
    VirtualizedEmpty,
}

impl ProbeFailure {
    /// 错误码（每型专属码）。
    pub fn code(self) -> &'static str {
        match self {
            ProbeFailure::Timeout => E_PROBE_TIMEOUT,
            ProbeFailure::NoDevice => E_PROBE_NO_DEVICE,
            ProbeFailure::Busy => E_PROBE_BUSY,
            ProbeFailure::VirtualizedEmpty => E_PROBE_UNAVAILABLE,
        }
    }

    /// 人读详情。
    pub fn zh(self) -> &'static str {
        match self {
            ProbeFailure::Timeout => "探测轮询超时",
            ProbeFailure::NoDevice => "驱动查无此设备",
            ProbeFailure::Busy => "设备忙（重试窗口未到）",
            ProbeFailure::VirtualizedEmpty => "虚拟化空态（无 NPU 虚拟机）",
        }
    }
}

/// 登记错误（诚实红线的拒收型）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistrationError {
    /// 伪造能力（把猜的填进表）。
    Fabricated {
        /// 引擎名。
        engine: String,
    },
    /// 步序越权（未枚举即登记）。
    Sequence {
        /// 当前步。
        at: ProbeStage,
    },
}

// ---------------------------------------------------------------------------
// 二、能力登记表
// ---------------------------------------------------------------------------

/// 单引擎登记条目（能力真值+驱动/设备键+可用性）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineCaps {
    /// 引擎类型。
    pub engine: EngineKind,
    /// 探测所得能力（真值）。
    pub caps: HetCaps,
    /// 驱动版本（缓存键成分——驱动更新=作废重探）。
    pub driver_version: String,
    /// 设备 ID（缓存键成分）。
    pub device_id: String,
    /// 是否可用（部分失败引擎保留条目但置 false）。
    pub available: bool,
}

impl EngineCaps {
    /// 缓存键（驱动版本+设备 ID 复合——F1506 热失效模式的键）。
    pub fn cache_key(&self) -> String {
        format!("{}@{}", self.driver_version, self.device_id)
    }

    /// 缓存是否对当前环境有效（键全等才算复用）。
    pub fn cache_valid(&self, driver_version: &str, device_id: &str) -> bool {
        self.driver_version == driver_version && self.device_id == device_id
    }
}

/// 能力登记表（四引擎上限——按 [`ENGINE_KINDS`] 闭集）。
#[derive(Clone, Debug, Default)]
pub struct CapRegistry {
    entries: Vec<EngineCaps>,
    /// 虚拟化空态显性标记（无 NPU 虚拟机 → true）。
    pub virtualized_empty: bool,
    /// 当前探测步（状态机游标）。
    pub stage: ProbeStage,
}

impl CapRegistry {
    /// 空登记表。
    pub fn new() -> CapRegistry {
        CapRegistry {
            entries: Vec::new(),
            virtualized_empty: false,
            stage: ProbeStage::Enumerate,
        }
    }

    /// 推进一步（越权跳步拒绝——锚点步序纪律）。
    pub fn advance(&mut self) -> Result<ProbeStage, RegistrationError> {
        match legal_next(self.stage) {
            Some(n) => {
                self.stage = n;
                Ok(n)
            }
            None => Err(RegistrationError::Sequence { at: self.stage }),
        }
    }

    /// 登记/覆盖一个引擎条目（幂等：同引擎重登覆盖不重复入账）。
    pub fn register(&mut self, entry: EngineCaps) -> Result<(), RegistrationError> {
        if self.stage != ProbeStage::Register {
            return Err(RegistrationError::Sequence { at: self.stage });
        }
        if let Some(slot) = self.entries.iter_mut().find(|e| e.engine == entry.engine) {
            *slot = entry;
        } else {
            self.entries.push(entry);
        }
        Ok(())
    }

    /// 查引擎条目。
    pub fn get(&self, engine: EngineKind) -> Option<&EngineCaps> {
        self.entries.iter().find(|e| e.engine == engine)
    }

    /// 在册条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空表（虚拟化空态）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 标记探测失败（保留条目置不可用——失败≠不存在）。
    pub fn mark_failed(&mut self, engine: EngineKind, why: ProbeFailure) -> Result<(), String> {
        let slot = self
            .entries
            .iter_mut()
            .find(|e| e.engine == engine)
            .ok_or_else(|| format!("{}：条目 {} 不在簿", E_PROBE_NO_DEVICE, engine.zh()))?;
        if slot.available {
            slot.available = false;
            let _ = why;
            Ok(())
        } else {
            Err(format!("{}：{} 已标记不可用（重复失败不覆盖原因）", why.code(), engine.zh()))
        }
    }

    /// 驱动/设备键失效重探（F1506 热失效模式：键变=表作废）。
    pub fn invalidate_on_key_change(&mut self, driver_version: &str, device_id: &str) -> usize {
        let mut dropped = 0usize;
        self.entries.retain(|e| {
            let keep = e.cache_valid(driver_version, device_id);
            if !keep {
                dropped += 1;
            }
            keep
        });
        if self.entries.is_empty() {
            self.virtualized_empty = true;
        }
        dropped
    }

    /// 汇总（读屏可达）。
    pub fn summary(&self) -> String {
        format!(
            "K04-probe: {} 条/空态={}/步={}",
            self.entries.len(),
            self.virtualized_empty,
            self.stage.zh()
        )
    }
}

/// 诚实登记规则（探测不到=不支持，不猜不造）。
///
/// 「不造假复用」：在无真实探测结果处填占位能力（比如给虚拟机编
/// 一个 NPU）会让上层按不存在的能力选路——一次虚假的能力登记
/// 比一次探测失败贵得多。
pub fn honesty_rule(engine: EngineKind, actually_probed: bool) -> Result<(), RegistrationError> {
    if !actually_probed {
        return Err(RegistrationError::Fabricated { engine: engine.zh().to_string() });
    }
    Ok(())
}

/// 探测实现矩阵行（NPU/DSP/ISP/MEDIA 各自的探测接口声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixRow {
    /// 引擎类型。
    pub engine: EngineKind,
    /// Windows 探测接口（CIM/NPU 驱动/XDNA/音频/驱动查询）。
    pub win_interface: &'static str,
    /// 其他平台接口（探测面占位）。
    pub other_platform: &'static str,
}

/// 探测实现矩阵（锚点：NPU Windows CIM/Intel NPU 驱动接口/AMD XDNA；
/// DSP 平台音频引擎；ISP/MEDIA 驱动查询）。
pub fn impl_matrix() -> [MatrixRow; 4] {
    [
        MatrixRow { engine: EngineKind::Npu, win_interface: "Windows CIM/Intel NPU 驱动接口/AMD XDNA", other_platform: "平台 NPU 运行时查询" },
        MatrixRow { engine: EngineKind::Dsp, win_interface: "平台音频引擎探测", other_platform: "平台 DSP 运行时查询" },
        MatrixRow { engine: EngineKind::Isp, win_interface: "驱动查询（ISP/MEDIA）", other_platform: "平台 ISP 运行时查询" },
        MatrixRow { engine: EngineKind::Media, win_interface: "驱动查询（ISP/MEDIA）", other_platform: "平台 MEDIA 运行时查询" },
    ]
}

// ---------------------------------------------------------------------------
// 三、探测流程（五步走完的组装）
// ---------------------------------------------------------------------------

/// 探测输入（每个引擎的应答——None=探测失败）。
#[derive(Clone, Copy, Debug)]
pub struct ProbeReply {
    /// 引擎类型。
    pub engine: EngineKind,
    /// 应答（None=该引擎探测失败）。
    pub caps: Option<HetCaps>,
    /// 失败原因（caps 为 None 时非 None）。
    pub failure: Option<ProbeFailure>,
}

/// 跑一次完整探测（五步；部分失败隔离——失败引擎保留条目置不可用）。
pub fn run_probe(
    replies: &[ProbeReply],
    driver_version: &str,
    device_id: &str,
) -> Result<CapRegistry, String> {
    let mut reg = CapRegistry::new();
    // 1) 枚举可用引擎：应答清单即枚举结果。
    reg.advance().map_err(|e| seq_err(e))?; // Enumerate → Query
    // 2) 逐引擎能力查询 + 3) 登记：失败引擎也留条目（标记不可用）。
    reg.advance().map_err(|e| seq_err(e))?; // Query → Register
    let mut real = 0usize;
    for r in replies.iter() {
        match r.caps {
            Some(caps) => {
                honesty_rule(r.engine, true).map_err(fab_err)?;
                reg.register(EngineCaps {
                    engine: r.engine,
                    caps,
                    driver_version: driver_version.to_string(),
                    device_id: device_id.to_string(),
                    available: true,
                })
                .map_err(|e| seq_err(e))?;
                real += 1;
            }
            None => {
                // 失败隔离：条目保留、可用=false（探测失败≠设备不存在）。
                reg.register(EngineCaps {
                    engine: r.engine,
                    caps: HetCaps {
                        compute_level: 0,
                        memory_bytes: 0,
                        power_coeff_ppm: 0,
                        latency_profile: 3,
                        feature_mask: 0,
                    },
                    driver_version: driver_version.to_string(),
                    device_id: device_id.to_string(),
                    available: false,
                })
                .map_err(|e| seq_err(e))?;
            }
        }
    }
    if real == 0 {
        // 空态语义：无任何真实能力（虚拟化）= 空表+显性。
        reg.virtualized_empty = true;
    }
    // 4) 缓存：键已写入条目（cache_key 随时可算，不另设缓存层）。
    reg.advance().map_err(|e| seq_err(e))?; // Register → Cache
    // 5) 就绪。
    reg.advance().map_err(|e| seq_err(e))?; // Cache → Ready
    Ok(reg)
}

/// 步序错误渲染。
fn seq_err(e: RegistrationError) -> String {
    match e {
        RegistrationError::Sequence { at } => {
            format!("{}：步序越权（停在 {}）", E_PROBE_SEQUENCE, at.zh())
        }
        RegistrationError::Fabricated { engine } => {
            format!("{}：伪造能力（{}）", E_PROBE_FABRICATED, engine)
        }
    }
}

/// 伪造错误渲染。
fn fab_err(e: RegistrationError) -> String {
    seq_err(e)
}

// ---------------------------------------------------------------------------
// 四、域自检
// ---------------------------------------------------------------------------

/// CGPU-F1604 域自检（六组判据：五步/矩阵/缓存/隔离/空态/诚实）。
pub fn run_vck04_checks() -> CheckSet {
    let mut s = CheckSet::new("K04-probe");

    // -- K04-STEP-01 五步走通（步序全链 + 终态无下一）----------------------
    {
        let mut cur = ProbeStage::Enumerate;
        let mut chain = Vec::new();
        chain.push(cur);
        let mut ok = true;
        while let Some(n) = legal_next(cur) {
            chain.push(n);
            cur = n;
            if chain.len() > 6 {
                ok = false;
                break;
            }
        }
        s.add(
            "K04-STEP-01 五步链完整且终态封闭（枚举→查询→登记→缓存→就绪）",
            ok && chain.len() == 5 && chain[4] == ProbeStage::Ready && legal_next(ProbeStage::Ready).is_none(),
            "锚点「探测五步」；步序状态机终态必须封闭",
        );
    }

    // -- K04-STEP-02 越权跳步拒绝 -----------------------------------------
    {
        let mut reg = CapRegistry::new();
        let r = reg.register(EngineCaps {
            engine: EngineKind::Npu,
            caps: HetCaps {
                compute_level: 5,
                memory_bytes: 1 << 30,
                power_coeff_ppm: 1200,
                latency_profile: 2,
                feature_mask: FEAT_BATCH,
            },
            driver_version: "1.0".to_string(),
            device_id: "dev0".to_string(),
            available: true,
        });
        s.add(
            "K04-STEP-02 越权跳步拒绝（未枚举即登记 = 对空设备登记）",
            matches!(r, Err(RegistrationError::Sequence { at: ProbeStage::Enumerate })),
            "锚点步序纪律：查询/登记只能在枚举后",
        );
    }

    // -- K04-MAT-01 实现矩阵四行（NPU/DSP/ISP/MEDIA 全覆盖+接口非空）-------
    {
        let m = impl_matrix();
        s.add(
            "K04-MAT-01 实现矩阵四行齐（Windows 接口/其他平台非空）",
            m.len() == ENGINE_KINDS.len()
                && m.iter().all(|r| !r.win_interface.is_empty() && !r.other_platform.is_empty()),
            "锚点「探测实现矩阵」：NPU CIM/XDNA、DSP 音频引擎、ISP/MEDIA 驱动查询",
        );
    }

    // -- K04-CACHE-01 缓存键=驱动版本+设备 ID（复合）----------------------
    {
        let e = EngineCaps {
            engine: EngineKind::Npu,
            caps: HetCaps {
                compute_level: 5,
                memory_bytes: 1 << 30,
                power_coeff_ppm: 1200,
                latency_profile: 2,
                feature_mask: FEAT_BATCH,
            },
            driver_version: "31.0.101.1".to_string(),
            device_id: "PCI\\VEN_8086".to_string(),
            available: true,
        };
        let key = e.cache_key();
        s.add(
            "K04-CACHE-01 缓存键复合（驱动版本@设备 ID）",
            key == "31.0.101.1@PCI\\VEN_8086"
                && e.cache_valid("31.0.101.1", "PCI\\VEN_8086")
                && !e.cache_valid("31.0.101.2", "PCI\\VEN_8086")
                && !e.cache_valid("31.0.101.1", "PCI\\VEN_8087"),
            "锚点「缓存键（驱动版本+设备 ID——失效重探）」：驱动或设备任一变化即失效",
        );
    }

    // -- K04-CACHE-02 键变失效重探（F1506 热失效模式）----------------------
    {
        let mut reg = CapRegistry::new();
        reg.register(EngineCaps {
            engine: EngineKind::Npu,
            caps: HetCaps {
                compute_level: 5,
                memory_bytes: 1 << 30,
                power_coeff_ppm: 1200,
                latency_profile: 2,
                feature_mask: FEAT_BATCH,
            },
            driver_version: "1.0".to_string(),
            device_id: "dev0".to_string(),
            available: true,
        })
        .ok();
        reg.register(EngineCaps {
            engine: EngineKind::Dsp,
            caps: HetCaps {
                compute_level: 4,
                memory_bytes: 1 << 28,
                power_coeff_ppm: 800,
                latency_profile: 0,
                feature_mask: 0,
            },
            driver_version: "1.0".to_string(),
            device_id: "dev0".to_string(),
            available: true,
        })
        .ok();
        let dropped = reg.invalidate_on_key_change("2.0", "dev0");
        s.add(
            "K04-CACHE-02 驱动更新键变失效重探（全表作废+空态显性）",
            dropped == 2 && reg.is_empty() && reg.virtualized_empty,
            "锚点「能力变更（驱动更新→能力表失效重探——热失效复用 F1506 模式）」",
        );
    }

    // -- K04-ISO-01 部分失败隔离（失败引擎置不可用、其余照常）-------------
    {
        let good = HetCaps {
            compute_level: 5,
            memory_bytes: 1 << 30,
            power_coeff_ppm: 1200,
            latency_profile: 2,
            feature_mask: FEAT_BATCH,
        };
        let replies = [
            ProbeReply { engine: EngineKind::Npu, caps: Some(good), failure: None },
            ProbeReply {
                engine: EngineKind::Dsp,
                caps: None,
                failure: Some(ProbeFailure::Timeout),
            },
        ];
        let reg = run_probe(&replies, "1.0", "dev0");
        let ok = match &reg {
            Ok(r) => {
                r.len() == 2
                    && r.get(EngineKind::Npu).map(|e| e.available).unwrap_or(false)
                    && r.get(EngineKind::Dsp).map(|e| !e.available).unwrap_or(true)
            }
            Err(_) => false,
        };
        s.add(
            "K04-ISO-01 部分失败隔离（失败引擎条目保留+置不可用，NPU 照常登记）",
            ok,
            "锚点「部分失败（某引擎探测失败→该引擎标记不可用（其余照常——失败隔离）」：失败≠不存在",
        );
    }

    // -- K04-ISO-02 失败码分型（四型专属码）------------------------------
    {
        let codes = [
            ProbeFailure::Timeout,
            ProbeFailure::NoDevice,
            ProbeFailure::Busy,
            ProbeFailure::VirtualizedEmpty,
        ];
        let mut all = Vec::new();
        for c in codes.iter() {
            all.push(c.code().to_string());
        }
        s.add(
            "K04-ISO-02 失败码四型分型（超时/查无/忙/空态各有专属码）",
                all.len() == 4
                    && all[0] != all[1]
                    && all[1] != all[2]
                    && all[2] != all[3]
                    && codes[0].code() == E_PROBE_TIMEOUT,
            "锚点失败隔离的分型面：一种笼统的失败等于没有隔离",
        );
    }

    // -- K04-EMPTY-01 虚拟化空态（无 NPU 虚拟机=空表+显性）---------------
    {
        let replies = [ProbeReply {
            engine: EngineKind::Npu,
            caps: None,
            failure: Some(ProbeFailure::VirtualizedEmpty),
        }];
        let reg = run_probe(&replies, "0.0", "vm0");
        s.add(
            "K04-EMPTY-01 虚拟化空态显性（无 NPU 虚拟机 → 空能力表+标记）",
            matches!(&reg, Ok(r) if r.is_empty() && r.virtualized_empty),
            "锚点「虚拟化（无 NPU 虚拟机→空能力表+显性——空态语义）」：空表必须显性，不可静默冒充无探测",
        );
    }

    // -- K04-HONEST-01 诚实登记红线（探测不到=不支持，不猜不造）------------
    {
        let fake = honesty_rule(EngineKind::Npu, false);
        let real = honesty_rule(EngineKind::Npu, true);
        s.add(
            "K04-HONEST-01 诚实红线（未探测即登记=伪造拒收）",
            matches!(fake, Err(RegistrationError::Fabricated { .. })) && real.is_ok(),
            "锚点「诚实登记（能力探测不到=不支持（不猜不造——不造假复用）」",
        );
    }

    // -- K04-HONEST-02 流程不造假（run_probe 全失败不填占位能力）----------
    {
        let replies = [
            ProbeReply { engine: EngineKind::Npu, caps: None, failure: Some(ProbeFailure::NoDevice) },
            ProbeReply { engine: EngineKind::Dsp, caps: None, failure: Some(ProbeFailure::NoDevice) },
        ];
        let reg = run_probe(&replies, "1.0", "dev0");
        let ok = match &reg {
            Ok(r) => r.is_empty() && r.virtualized_empty,
            Err(_) => false,
        };
        s.add(
            "K04-HONEST-02 全失败流程零编造（不填占位能力，空表+显性）",
            ok,
            "锚点「不造假复用」：流程层不给未探测引擎填任何能力真值",
        );
    }

    // -- K04-DRV-01 驱动接口矩阵与登记键同源（矩阵行引擎=登记表引擎）-----
    {
        let m = impl_matrix();
        let engines: Vec<EngineKind> = m.iter().map(|r| r.engine).collect();
        s.add(
            "K04-DRV-01 矩阵引擎集=引擎闭集（四引擎一个个对上）",
                engines.len() == ENGINE_KINDS.len()
                    && engines.iter().all(|e| ENGINE_KINDS.contains(e)),
            "矩阵与登记表共用 ENGINE_KINDS 单源（两处枚举各写各的迟早对不上）",
        );
    }

    // -- K04-META-01 版本指纹非零 -----------------------------------------
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in PROBE_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    s.add("K04-META-01 版本指纹非零", fp != 0, "K04-probe-v1");

    s
}
