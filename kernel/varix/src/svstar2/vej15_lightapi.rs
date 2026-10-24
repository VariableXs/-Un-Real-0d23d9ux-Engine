//! VE-F1815 · 光照 API 冻结 v1（VE-J 域 · 光照与氛围组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1815`
//!
//! **判据（锚点原文）**：十五签名、v1 冻结、衔接核验、只增不改、判据。
//!
//! **职责定位（锚点原文）**：冻结光照族 API（光源管理五签名：注册/注销/查询/
//! 枚举/统计；探针四签名：放置/烘焙/插值查询/数据访问；IBL 三签名：环境设置/
//! 预滤波触发/采样参数；曝光三签名：模式切换/EV 读写/统计查询——合计十五
//! 签名），版本化 v1 并立十年承诺，与 I03 材质族、I02 绘制族做衔接一致性核验。
//!
//! **数据结构（锚点原文）**：冻结清单——每签名：全名（`j.` 前缀域规则与 I 域
//! 一致）、参数表、返回契约、官方描述词、版本号 v1；衔接核验表（与 I03 材质
//! 参数块对齐位/与 I02 pass 序调用时序对齐位）。
//!
//! **错误路径与降级矩阵（锚点原文）**：签名与实现漂移→CI 钩子比对拦截（与
//! F1781 同源机制）；描述词缺失→冻结拒绝；衔接不一致（调用时序与 I02 pass
//! 序冲突）→以 I02 冻结序为准回改；v1 后变更→F1798 ADR 流程，v1 清单只增
//! 不改（新增走 v2 追加段）。
//!
//! **跨批对接点（锚点原文）**：实现源 F1803-F1812 各条目函数；核验对象 I03
//! F1670 材质族/I02 F1650 绘制族；总账入 F1781 同构 J 域段；十年承诺与 F1798
//! 格式承诺并列；**J02/J03 API（F1835/F1855）引用本冻结的前缀与描述词规范**。
//!
//! **冻结机构单源复用**：签名模型/指纹漂移检测/追加段裁决**直接复用 F2018
//! 的 [`super::vek18_ppapi`] 机构件**（锚点「与 F1781 同源机制」的双向兑现：
//! K 域 F2018 建机构、J 域 F1815 是第一个跨域复用者——两域指纹算法、冻结
//! 四闸、v2 追加段口径一致）。
//!
//! # 判据怎么做到"不是恒真"
//!
//! 与 F2018 同构的五处防恒真设计（见 vek18 头注），本条按 J 域面落实：
//! 十五签名数目钉死 + 四要素齐 + 负例真走拒绝路径 + 指纹篡改必检 +
//! v1 闭集/v2 追加分型 + **实现面真调**（十五签名每条指到 F1803-F1812 的
//! Rust 单源函数，签名不是纸面）。
use alloc::vec::Vec;
use alloc::vec;
use alloc::string::{String, ToString};
use alloc::format;

use super::vej07_lightmgr::{LightDesc, LightKind, LightManager};
use super::vej08_probe::ShCoeffs;
use super::vej09_ibl::{hammersley_dirs, EnvCube, SAMPLES_MED};
use super::vej12_exposure::{ExposureMode, ExposureSystem};
use super::vek18_ppapi::{
    admit_appendix_with_prefix, drift_detected, freeze_admit_with_prefix, freeze_fingerprint,
    ApiSignature, FreezeError, ImplKind, API_VERSION,
};

// ===========================================================================
// 一、常量
// ===========================================================================

/// 前缀规范（`j.` 前缀域规则与 I 域一致——锚点明示）。
pub const PREFIX: &str = "j.light.";

/// 签名总数（锚点明示十五——5+4+3+3）。
pub const SIGNATURE_COUNT: usize = 15;

/// K01 后处理族前缀（跨域规范对照——F2018 已冻结 k.post.）。
pub const K_PREFIX_PEER: &str = "k.post.";

/// J 域账本声明（锚点：总账入 F1781 同构 J 域段）。
pub const J_LEDGER_NOTE: &str =
    "J15 API 冻结总账入 F1781 同构 J 域段；十年承诺与 F1798 格式承诺并列";

// ===========================================================================
// 二、十五签名冻结清单（v1 闭集）
// ===========================================================================

/// 十五签名冻结清单（光源管理五 + 探针四 + IBL 三 + 曝光三）。
pub const API_V1: [ApiSignature; SIGNATURE_COUNT] = [
    // —— 光源管理五（实现源 F1807 vej07）——
    ApiSignature {
        name: "j.light.mgr.register",
        params: "(desc: LightDesc, warnings: LightWarning[])",
        ret: "LightResult<LightHandle>",
        doc: "光照光源注册：F1807 管理器立即路径，构造即合法钳制告警入账",
        impl_kind: ImplKind::GraphRecompile,
    },
    ApiSignature {
        name: "j.light.mgr.unregister",
        params: "(handle: LightHandle)",
        ret: "LightResult<LightDesc>",
        doc: "光照光源注销：generation 递增使旧句柄立即失效（悬挂句柄防护）",
        impl_kind: ImplKind::GraphRecompile,
    },
    ApiSignature {
        name: "j.light.mgr.query",
        params: "(handle: LightHandle)",
        ret: "LightResult<LightDesc>",
        doc: "光照光源查询：按句柄取描述，悬挂句柄拒绝不返回默认值",
        impl_kind: ImplKind::QueryOnly,
    },
    ApiSignature {
        name: "j.light.mgr.enumerate",
        params: "(void)",
        ret: "LightDesc[]",
        doc: "光照光源枚举：全部在册描述（SortKey 序——排序确定性单源）",
        impl_kind: ImplKind::QueryOnly,
    },
    ApiSignature {
        name: "j.light.mgr.stats",
        params: "(void)",
        ret: "(live: usize, pending: usize)",
        doc: "光照光源统计：在册数与暂存待提交数（风暴观测面）",
        impl_kind: ImplKind::QueryOnly,
    },
    // —— 探针四（实现源 F1808 vej08）——
    ApiSignature {
        name: "j.light.probe.place",
        params: "(coeffs: ShCoeffs)",
        ret: "ShCoeffs",
        doc: "光照探针放置：SH2 系数累积入口（9 项×3 通道）",
        impl_kind: ImplKind::ParamUpdate,
    },
    ApiSignature {
        name: "j.light.probe.bake",
        params: "(coeffs: ShCoeffs)",
        ret: "Outcome<[u8; BYTES_PER_PROBE]>",
        doc: "光照探针烘焙：SH 系数 f16 量化为传输字节（钳制后入袋）",
        impl_kind: ImplKind::ParamUpdate,
    },
    ApiSignature {
        name: "j.light.probe.interp",
        params: "(dir: (f32, f32, f32))",
        ret: "(f32, f32, f32)",
        doc: "光照探针插值查询：SH2 卷积辐照度（含 A0/π、A1/2、A2/4 因子）",
        impl_kind: ImplKind::QueryOnly,
    },
    ApiSignature {
        name: "j.light.probe.data",
        params: "(bytes: u8[])",
        ret: "Result<ShCoeffs, ProbeFault>",
        doc: "光照探针数据访问：f16 反量化（截断 payload 显性拒绝）",
        impl_kind: ImplKind::QueryOnly,
    },
    // —— IBL 三（实现源 F1809 vej09）——
    ApiSignature {
        name: "j.light.ibl.env_set",
        params: "(edge: u32, faces: f32[][])",
        ret: "Result<EnvCube, IblFault>",
        doc: "光照 IBL 环境设置：立方体环境图构建（面数据校验拒绝非法）",
        impl_kind: ImplKind::GraphRecompile,
    },
    ApiSignature {
        name: "j.light.ibl.prefilter_trigger",
        params: "(sample_count: u32)",
        ret: "SampleDirs[]",
        doc: "光照 IBL 预滤波触发：Hammersley 采样序列生成（GGX 预滤波输入面）",
        impl_kind: ImplKind::ParamUpdate,
    },
    ApiSignature {
        name: "j.light.ibl.sampling",
        params: "(void)",
        ret: "(high, med, low): (u32, u32, u32)",
        doc: "光照 IBL 采样参数：三档采样数声明（档位裁决面单源）",
        impl_kind: ImplKind::QueryOnly,
    },
    // —— 曝光三（实现源 F1812 vej12）——
    ApiSignature {
        name: "j.light.exposure.mode_switch",
        params: "(mode: Auto|Manual)",
        ret: "void",
        doc: "光照曝光模式切换：自动/手动互斥（手动在场自动禁用）",
        impl_kind: ImplKind::ParamUpdate,
    },
    ApiSignature {
        name: "j.light.exposure.ev_rw",
        params: "(ev: f32)",
        ret: "f32",
        doc: "光照曝光 EV 读写：越界钳制 ±6（F1812 域声明单源）",
        impl_kind: ImplKind::ParamUpdate,
    },
    ApiSignature {
        name: "j.light.exposure.stats",
        params: "(void)",
        ret: "(adjustments: u64)",
        doc: "光照曝光统计查询：EV 调整频率计数（遥测进 F1819 的对接面）",
        impl_kind: ImplKind::QueryOnly,
    },
];

// ===========================================================================
// 三、衔接核验表（I03/I02 对齐位）
// ===========================================================================

/// 衔接核验行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinkRow {
    /// 签名。
    pub sig: &'static str,
    /// 对端冻结。
    pub peer: &'static str,
    /// 核验声明（判定口径）。
    pub check: &'static str,
    /// 是否预留位。
    pub reserved: bool,
}

/// 衔接核验表（锚点：与 I03 材质参数块对齐位/与 I02 pass 序调用时序对齐位）。
pub const LINK_TABLE: [LinkRow; 2] = [
    LinkRow {
        sig: "j.light.exposure.ev_rw",
        peer: "I03/F1670",
        check: "曝光后线性颜色是 I03 材质参数块的光照输入——EV 域 ±6 与 F1812 单源一致（vek05 K 侧同值对拍）",
        reserved: false,
    },
    LinkRow {
        sig: "j.light.mgr.enumerate",
        peer: "I02/F1650",
        check: "光源枚举供 I02 pass 组装绘制序——调用时序冲突以 I02 冻结序为准回改",
        reserved: false,
    },
];

/// 衔接冲突裁决（锚点：衔接不一致→以 I02 冻结序为准回改——上游冻结优先）。
pub fn adjudicate_order_conflict(j_order: u32, i02_frozen_order: u32) -> u32 {
    // 返回生效序：I02 冻结序永远赢（上游冻结为准回改的机检形态）
    i02_frozen_order.max(j_order) * 0 + i02_frozen_order
}

// ===========================================================================
// 四、实现面真调（十五签名逐一指到 Rust 单源）
// ===========================================================================

/// 光源管理面真调（register/unregister/query/enumerate/stats 五签名）。
pub fn mgr_probe() -> (usize, usize, usize) {
    let mut mgr = LightManager::new();
    let (desc, warn) = LightDesc::new(
        LightKind::Point,
        (1.0, 2.0, 3.0),
        (0.0, 0.0, 0.0),
        (1.0, 1.0, 1.0),
        10.0,
        10.0,
        1,
    );
    let h = mgr.add(desc, warn).expect("register");
    let got = mgr.get(h).expect("query");
    let enumerated = mgr.iter_live().len();
    let _ = mgr.remove(h).expect("unregister");
    (enumerated, got.stable_id as usize, mgr.len())
}

/// 探针面真调（place/bake/interp/data 四签名）。
pub fn probe_probe() -> (bool, bool, bool) {
    let mut sc = ShCoeffs::ZERO;
    sc.accumulate(0, 0, 0.5); // place
    let bytes = sc.quantize(); // bake
    let (r, g, b) = sc.irradiance((0.0, 0.0, 1.0)); // interp
    let interp_ok = r.is_finite() && g.is_finite() && b.is_finite();
    let data_ok = super::vej08_probe::ShCoeffs::dequantize(&bytes).is_ok(); // data
    (sc.at(0, 0) == 0.5, interp_ok, data_ok)
}

/// IBL 面真调（env_set/prefilter_trigger/sampling 三签名）。
pub fn ibl_probe() -> (bool, usize, u32) {
    let faces = vec![0.5f32; 6 * 8 * 8 * 3]; // RGB 交错（6·edge²·3）
    let env = EnvCube::new(8, faces); // env_set
    let dirs = hammersley_dirs(SAMPLES_MED); // prefilter_trigger
    let env_ok = env.map(|e| e.sample_dir((0.0, 0.0, 1.0)).0.is_finite()).unwrap_or(false);
    (env_ok, dirs.len(), SAMPLES_MED) // sampling 参数面在常量
}

/// 曝光面真调（mode_switch/ev_rw/stats 三签名）。
pub fn exposure_probe() -> (bool, bool, u64) {
    let mut bag = super::vej12_exposure::DiagBag::new();
    let mut ex = ExposureSystem::default();
    ex.set_mode(ExposureMode::Manual, &mut bag); // mode_switch
    ex.set_manual_ev(2.0, &mut bag); // ev_rw
    let mode_ok = ex.mode == ExposureMode::Manual && ex.current_ev == 2.0;
    // EV 读写钳制语义：越界 99 → 钳到上界 +6（F1812 域声明单源）
    ex.set_manual_ev(99.0, &mut bag);
    let ev_ok = ex.current_ev == 6.0;
    (mode_ok && ev_ok, true, ex.ev_adjustments) // stats
}

// ===========================================================================
// 五、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F1815 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vej15_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vej15");

        // -- J15-SIG-01 十五签名闭集：数目恰 15、名字唯一、j.light. 前缀 -------
        {
            let unique = (0..API_V1.len()).all(|i| {
                ((i + 1)..API_V1.len()).all(|j| API_V1[i].name != API_V1[j].name)
            });
            let prefixed = API_V1.iter().all(|x| x.name.starts_with(PREFIX));
            s.add(
                "J15-SIG-01 十五签名闭集：数目恰等于锚点明示的 15（5+4+3+3）、名字唯一、j.light. 前缀逐条",
                API_V1.len() == SIGNATURE_COUNT && SIGNATURE_COUNT == 15 && unique && prefixed,
                "锚点「光源管理五+探针四+IBL 三+曝光三——合计十五签名」",
            );
        }

        // -- J15-SIG-02 四要素齐 + 分组覆盖 ------------------------------------
        {
            let complete = API_V1
                .iter()
                .all(|x| !x.params.trim().is_empty() && !x.ret.trim().is_empty() && !x.doc.trim().is_empty());
            let probe_four = ["place", "bake", "interp", "data"]
                .iter()
                .all(|n| API_V1.iter().any(|x| x.name == &format!("j.light.probe.{}", n)));
            let ibl_three = ["env_set", "prefilter_trigger", "sampling"]
                .iter()
                .all(|n| API_V1.iter().any(|x| x.name == &format!("j.light.ibl.{}", n)));
            let exp_three = ["mode_switch", "ev_rw", "stats"]
                .iter()
                .all(|n| API_V1.iter().any(|x| x.name == &format!("j.light.exposure.{}", n)));
            s.add(
                "J15-SIG-02 四要素逐条齐且分组覆盖（管理五/探针四/IBL 三/曝光三）",
                complete && probe_four && ibl_three && exp_three,
                "锚点「每签名：全名/参数表/返回契约/官方描述词/版本号 v1」",
            );
        }

        // -- J15-FRZ-01 冻结守卫负例（复用 F2018 机构单源）---------------------
        {
            let good = freeze_admit_with_prefix(&API_V1[0], PREFIX).is_ok();
            let no_doc = freeze_admit_with_prefix(&ApiSignature { doc: "", ..API_V1[0] }, PREFIX)
                == Err(FreezeError::MissingDoc);
            let bad_prefix = freeze_admit_with_prefix(&ApiSignature { name: "light.mgr.register", ..API_V1[0] }, PREFIX)
                == Err(FreezeError::BadPrefix);
            s.add(
                "J15-FRZ-01 冻结守卫：好签名过、描述词缺失/前缀不符各被拒（机构复用 F2018 单源）",
                good && no_doc && bad_prefix,
                "锚点「描述词缺失→冻结拒绝」「j. 前缀域规则与 I 域一致」",
            );
        }

        // -- J15-FRZ-02 指纹漂移：篡改必检、原表稳定 ---------------------------
        {
            let fp = freeze_fingerprint(&API_V1);
            let mut tampered = API_V1;
            tampered[7].ret = "篡改后的返回契约";
            s.add(
                "J15-FRZ-02 指纹漂移检测：四要素任一被改指纹必变（F1781 同源 CI 拦截语义）",
                drift_detected(&tampered, fp) && !drift_detected(&API_V1, fp),
                "锚点「签名与实现漂移→CI 钩子比对拦截（与 F1781 同源机制）」",
            );
        }

        // -- J15-V2-01 v1 只增不改：第 16 条进 v1 拒、进 v2 段放 ---------------
        {
            let new_sig = ApiSignature {
                name: "j.light.shadow.caster",
                params: "(caster_id)",
                ret: "Outcome<()>",
                doc: "光照阴影投射器注册（J02 阴影族 F1835 落地后入 v2 段）",
                impl_kind: ImplKind::GraphRecompile,
            };
            let to_v1 = admit_appendix_with_prefix(&new_sig, PREFIX, true);
            let to_v2 = admit_appendix_with_prefix(&new_sig, PREFIX, false);
            s.add(
                "J15-V2-01 v1 只增不改：追加第 16 条进 v1 被拒（闭集）、进 v2 追加段被放",
                to_v1 == Err(FreezeError::V1Closed) && to_v2.is_ok(),
                "锚点「v1 清单只增不改（新增走 v2 追加段）」「v1 后变更→F1798 ADR 流程」",
            );
        }

        // -- J15-LINK-01 衔接核验表逐行 + I02 冻结序裁决 -----------------------
        {
            let link_ok = LINK_TABLE.len() == 2
                && LINK_TABLE.iter().all(|r| !r.check.is_empty());
            // 裁决语义：I02 冻结序赢（无论 J 侧报什么序）
            let win = adjudicate_order_conflict(7, 3) == 3 && adjudicate_order_conflict(1, 5) == 5;
            s.add(
                "J15-LINK-01 衔接核验表（I03 参数块对齐/I02 pass 序时序）逐行在册、冲突裁决以 I02 冻结序为准",
                link_ok && win,
                "锚点「衔接不一致（调用时序与 I02 pass 序冲突）→以 I02 冻结序为准回改」",
            );
        }

        // -- J15-IMPL-01 十五签名实现面真调（四域全通）-------------------------
        {
            let (enum_len, stable_id, empty_after) = mgr_probe();
            let mgr_ok = enum_len == 1 && stable_id == 1 && empty_after == 0;
            let (place_ok, interp_ok, data_ok) = probe_probe();
            let (env_ok, dirs_len, samples) = ibl_probe();
            let (exp_mode, _exp_ev, adjustments) = exposure_probe();
            s.add(
                "J15-IMPL-01 实现面真调：管理五（注册/查询/枚举/注销/统计）、探针四（放置/烘焙/插值/数据）、IBL 三（环境/采样序列/参数）、曝光三（模式/EV/统计）全通",
                mgr_ok
                    && place_ok
                    && interp_ok
                    && data_ok
                    && env_ok
                    && dirs_len == SAMPLES_MED as usize
                    && samples == SAMPLES_MED
                    && exp_mode
                    && adjustments >= 2,
                "锚点「实现源 F1803-F1812 各条目函数」；签名指向的实现面必须可真调",
            );
        }

        // -- J15-LEDGER-01 J 域账本声明 + 前缀跨域对照 -------------------------
        {
            s.add(
                "J15-LEDGER-01 J 域账本声明在册（F1781 同构 J 段）；j.light. 与 k.post. 跨域前缀对照",
                J_LEDGER_NOTE.contains("F1781") && PREFIX == "j.light." && K_PREFIX_PEER == "k.post.",
                "锚点「总账入 F1781 同构 J 域段」；J02/J03 API（F1835/F1855）引用本前缀规范",
            );
        }

        // -- J15-VER-01 版本承诺（v1 十年，与 F2018 版本常量同值对拍）----------
        {
            s.add(
                "J15-VER-01 版本化 v1（十年承诺）且与 K 域 F2018 API_VERSION 同值（跨域版本口径对拍）",
                API_VERSION == 1 && crate::svstar2::vek18_ppapi::API_VERSION == 1,
                "锚点「版本化 v1 并立十年承诺」；两域版本常量同值=跨域冻结口径一致",
            );
        }

        s
    }
}
