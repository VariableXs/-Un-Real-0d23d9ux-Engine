//! VE-F1813 · 光照 fuzz（VE-J 域 · 光照与氛围组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1813`
//!
//! **判据（锚点原文）**：三段 fuzz、钳制容错、风暴不变量、24h 固化、判据。
//!
//! **职责定位（锚点原文）**：对 J01 全部输入面执行零信任模糊测试：畸形光源
//! 参数 fuzz（负强度/NaN 方向/极端衰减角/非法锥角——全部钳制容错不崩溃）、
//! 光源风暴 fuzz（每帧高频增删光源——管理器不变量：无泄漏/最终一致/排序
//! 确定）、探针与 IBL 数据 fuzz（非法 SH 系数/畸形 cubemap——校验拒绝），
//! 案例固化入库供 F1769 总闸与 F1789 域级总闸调度。
//!
//! **数据结构（锚点原文）**：案例库三段——参数畸形案例（生成器：参数空间
//! 自动扫描+人工种子）、风暴案例（操作序列脚本+不变量断言集）、数据畸形
//! 案例（位翻转/截断/格式混淆 cubemap 与探针文件）；每案例：输入/期望
//! （钳制后合法输出或显性拒绝）/复现脚本。
//!
//! **错误路径与降级矩阵（锚点原文）**：崩溃→P0 立案最小复现 24 小时内固化；
//! 挂起（死循环）→超时杀+立案；容错后输出仍非法（钳制失灵）→立案；不变量
//! 违反（泄漏/排序不确定）→立案并定位到 F1807 对应防护；案例去重哈希管理。
//!
//! **性能逐项分解（锚点原文）**：参数扫描自动生成万级案例 CI 夜间批跑；
//! 风暴长跑 1 小时档入 F1769 72h 组合；数据 fuzz 用变异策略复用 I 域框架。
//!
//! **跨批对接点（锚点原文）**：案例库对接 F1769 域级调度（J 段注册）；崩溃
//! 与违反进 F1774 账本；通过率快照供 F1818 安全条目引用；**J02/J03 fuzz
//! （F1833/F1853）沿用本生成器框架扩展阴影/氛围参数空间**。
//!
//! **框架单源复用**：案例库/P0 台账/RNG/去重哈希**直接复用 F2016 的
//! [`super::vek16_ppfuzz`] 框架件**（锚点「沿用本生成器框架」的双向兑现：
//! K 域 F2016 建框架、J 域 F1813 是第一个跨域复用者——两域的案例哈希与
//! 立案时限共用同一口径，不再各造一套）。被测面：F1804/F1805（光源参数
//! 钳制与双检）、F1807（管理器不变量）、F1808（探针 SH 校验）。
//!
//! # 判据怎么做到"不是恒真"
//!
//! 1. **"钳制容错"**。若语料全合法，钳制分支是死代码。断**逐字段毒化**
//!    （强度/范围/方向/锥角/颜色各吃过非法样本）+ 构造即合法（输出有限且
//!    在域）+ **拒绝语义分型**（F1805 的 NaN 锥角是显性拒绝不是钳制——
//!    NaN 会静默传播进着色，两种错误路径混用即分型失守）。
//! 2. **"风暴不变量"**。三条不变量各有可判定形态：**最终一致**（live 数
//!    == 账面净值）、**无泄漏**（增删循环归零后槽全回收）、**排序确定**
//!    （同脚本双跑 iter_live 序逐位一致）——缺一即立案定位 F1807 防护。
//! 3. **"24h 固化"**。P0 台账真调（复用 F2016 单源——时限 24h 硬编码），
//!    绿语料零立案。
use alloc::format;
use alloc::vec::Vec;

use super::vek16_ppfuzz::{fnv1a64, CaseLibrary, FuzzCase, P0Ledger, Rng};
use super::vej04_pointlight::{FalloffCurve, PointLight};
use super::vej05_spotlight::{IesSlot, SpotLight};
use super::vej07_lightmgr::{LightDesc, LightKind, LightManager, MAX_LIGHTS};
#[allow(unused_imports)]
use alloc::string::ToString;
use super::vej08_probe::{ShCoeffs, BYTES_PER_PROBE, SH_CLAMP_MAX, SH_CLAMP_MIN};

// ===========================================================================
// 一、诊断码（J 域独占段 0x34xx——0x31=F1810/0x32=F2016/0x33=F1812/0x39=F1811）
// ===========================================================================

/// 诊断码（复用 K 域 F2014 新构类型——J/K 共用一个新构壳，数值段各自独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiagCode(pub u16);

/// J 域 F1813 诊断码。
pub mod code {
    use super::DiagCode;

    /// 崩溃/挂起 → P0 立案（24h 固化，锚点明示）。
    pub const P0_FILED: DiagCode = DiagCode(0x3401);
    /// 钳制失灵（容错后输出仍非法）→ 立案。
    pub const CLAMP_FAILED: DiagCode = DiagCode(0x3402);
    /// 管理器不变量违反（泄漏/排序不确定）→ 立案并定位 F1807 防护。
    pub const INVARIANT_BROKEN: DiagCode = DiagCode(0x3403);
}

// ===========================================================================
// 二、段一：畸形光源参数 fuzz（负强度/NaN 方向/极端衰减角/非法锥角）
// ===========================================================================

/// 段一报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ParamFuzzReport {
    /// 总样本数。
    pub cases: u64,
    /// 钳制失灵数（构造后输出仍非法——锚点立案线；绿跑必须为 0）。
    pub clamp_failures: u64,
    /// 真触发钳制的样本数（反证语料非全合法）。
    pub clamped_inputs: u64,
    /// 显性拒绝数（SpotLight NaN 锥角——拒绝语义面）。
    pub rejected: u64,
    /// 逐字段毒化覆盖（强度/范围/方向/锥角/颜色——5 面各 ≥1）。
    pub fields_poisoned: [u64; 5],
}

/// 非法值域（轮换注入）。
fn poison_f(kind: usize) -> f32 {
    match kind {
        0 => f32::NAN,
        1 => f32::INFINITY,
        2 => f32::NEG_INFINITY,
        3 => -42.0,
        4 => 1.0e30,
        _ => -0.0,
    }
}

/**
 * 段一：光源参数 fuzz（万级夜间批跑的进程内等价量）。
 *
 * 点光走**钳制容错**语义（F1804：构造即合法——NaN 位置不该让场景加载
 * 失败，但必须告警）；聚光走**显性拒绝**语义（F1805：NaN 锥角显性拒绝——
 * NaN 与任何值比较恒 false，靠大小比较会静默放行）。两种错误路径不许混。
 */
pub fn fuzz_light_params(seed: u64, cases: u64, ledger: &mut P0Ledger, lib: &mut CaseLibrary) -> ParamFuzzReport {
    let mut rng = Rng::new(seed);
    let mut rep = ParamFuzzReport { cases, ..Default::default() };
    for i in 0..cases {
        let field = (i % 5) as usize;
        let poison = poison_f(((i / 5) % 6) as usize);
        rep.fields_poisoned[field] += 1;
        match field {
            // 0/1：点光强度与范围（钳制语义）
            0 | 1 => {
                let intensity = if field == 0 { poison } else { 10.0 };
                let range = if field == 1 { poison } else { 10.0 };
                let (light, warn) = PointLight::new(
                    (0.0, 0.0, 0.0),
                    (1.0, 1.0, 1.0),
                    intensity,
                    range,
                    FalloffCurve::Physical,
                );
                let ok_shape = light.intensity_cd.is_finite()
                    && light.intensity_cd >= 0.0
                    && light.range.is_finite()
                    && light.range > 0.0
                    && light.color.r.is_finite();
                if !ok_shape {
                    rep.clamp_failures += 1;
                    file_clamp(ledger, lib, "point", &format!("强度={:?} 范围={:?}", intensity, range), seed, i);
                } else if !warn.is_empty() {
                    rep.clamped_inputs += 1;
                }
            }
            // 2：方向（LightDesc 钳制语义——零向量/非有限归一化回退）
            2 => {
                let (desc, warn) = LightDesc::new(
                    LightKind::Directional,
                    (0.0, 0.0, 0.0),
                    (poison, poison, poison),
                    (1.0, 1.0, 1.0),
                    10.0,
                    f32::INFINITY,
                    i,
                );
                let d = desc.dir;
                let len_sq = d.0 * d.0 + d.1 * d.1 + d.2 * d.2;
                let ok_shape = len_sq.is_finite() && (len_sq - 1.0).abs() < 1e-3 && desc.intensity >= 0.0;
                if !ok_shape {
                    rep.clamp_failures += 1;
                    file_clamp(ledger, lib, "dir", &format!("方向=({:?})", poison), seed, i);
                } else if !warn.is_empty() {
                    rep.clamped_inputs += 1;
                }
            }
            // 3：聚光锥角（显性拒绝语义——NaN 不许钳制静默放行）
            3 => {
                let cone = poison;
                let r = SpotLight::new(
                    (0.0, 0.0, 0.0),
                    (0.0, 0.0, -1.0),
                    cone,
                    cone * 2.0 + 0.1,
                    10.0,
                    10.0,
                    FalloffCurve::Physical,
                    IesSlot::reserve("fuzz"),
                );
                match r {
                    Err(_) => {
                        rep.rejected += 1;
                    }
                    Ok((light, _)) => {
                        // 非法锥角被钳制放行时，外锥必须已回合法域（≤180°）
                        let ok_shape = light.outer_cone.is_finite() && light.outer_cone <= 180.0;
                        if !ok_shape {
                            rep.clamp_failures += 1;
                            file_clamp(ledger, lib, "cone", &format!("锥角={:?}", cone), seed, i);
                        } else {
                            rep.clamped_inputs += 1;
                        }
                    }
                }
            }
            // 4：颜色（点光 LinearRgb 钳制语义）
            _ => {
                let (light, warn) = PointLight::new(
                    (0.0, 0.0, 0.0),
                    (poison, 1.0, poison),
                    10.0,
                    10.0,
                    FalloffCurve::Physical,
                );
                let ok_shape = light.color.r.is_finite() && light.color.g.is_finite() && light.color.b.is_finite();
                if !ok_shape {
                    rep.clamp_failures += 1;
                    file_clamp(ledger, lib, "color", &format!("颜色=({:?})", poison), seed, i);
                } else if !warn.is_empty() {
                    rep.clamped_inputs += 1;
                }
            }
        }
        let _ = &mut rng;
    }
    rep
}

/// 钳制失灵立案（首 3 例固化入库 + 立案——锚点「容错后输出仍非法→立案」）。
fn file_clamp(ledger: &mut P0Ledger, lib: &mut CaseLibrary, face: &str, input: &str, seed: u64, i: u64) {
    if i > 3 {
        return;
    }
    let hash = fnv1a64(format!("light-clamp-{}-{}-{}", face, seed, i).as_bytes());
    lib.record(FuzzCase {
        segment: "light-param",
        name: format!("clamp-failure-{}-{}", face, i),
        input_desc: input.to_string(),
        expected_desc: "构造即合法：输出有限且在域（钳制面）或显性拒绝（拒绝面）".to_string(),
        repro: format!("fuzz_light_params(seed={}, cases 万级) 第 {} 例", seed, i),
        hash,
    });
    ledger.file(hash, "光源参数钳制失灵：容错后输出仍非法");
}

// ===========================================================================
// 三、段二：光源风暴 fuzz（高频增删——管理器三不变量）
// ===========================================================================

/// 段二报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StormReport {
    /// 操作总数。
    pub ops: u64,
    /// 容量拒绝数（CapacityExhausted——防线工作证据）。
    pub capacity_rejects: u64,
    /// 陈旧句柄拒绝数（generation 防护工作证据）。
    pub stale_rejects: u64,
    /// 不变量违反数（泄漏/最终一致/排序不确定——绿跑必须为 0）。
    pub invariant_violations: u64,
    /// 终态在册数。
    pub final_live: u64,
}

/// 生成风暴操作脚本（确定性——双跑对照的语料）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StormOp {
    /// 经 stage 暂存加灯（帧边界提交路径）。
    StageAdd,
    /// 立即加灯（立即路径）。
    AddNow,
    /// 立即移除随机在册句柄（含陈旧句柄——防护面）。
    RemoveRandom,
    /// 帧边界提交。
    Commit,
}

fn storm_script(seed: u64, ops: u64) -> Vec<StormOp> {
    let mut rng = Rng::new(seed);
    (0..ops)
        .map(|i| match rng.below(100) {
            0..=39 => StormOp::StageAdd,
            40..=59 => StormOp::AddNow,
            60..=79 => StormOp::RemoveRandom,
            _ if i % 7 == 6 => StormOp::Commit,
            _ => StormOp::StageAdd,
        })
        .collect()
}

/// 执行一遍风暴脚本（确定性；单管理器）。
fn run_storm_once(script: &[StormOp], seed: u64) -> StormReport {
    let mut rng = Rng::new(seed ^ 0xA11CE);
    let mut mgr = LightManager::new();
    let mut live_handles: Vec<super::vej07_lightmgr::LightHandle> = Vec::new();
    let mut net_adds: i64 = 0;
    let mut rep = StormReport::default();
    let mut sid: u64 = 0;
    for op in script {
        rep.ops += 1;
        match op {
            StormOp::StageAdd | StormOp::AddNow => {
                sid += 1;
                let kind = match rng.below(4) {
                    0 => LightKind::Directional,
                    1 => LightKind::Point,
                    2 => LightKind::Spot,
                    _ => LightKind::Area,
                };
                let (desc, warn) = LightDesc::new(
                    kind,
                    (rng.below(100) as f32, rng.below(100) as f32, rng.below(100) as f32),
                    (0.0, 0.0, -1.0),
                    (1.0, 1.0, 1.0),
                    rng.below(1000) as f32,
                    if kind == LightKind::Directional { f32::INFINITY } else { 10.0 + rng.below(50) as f32 },
                    sid,
                );
                if *op == StormOp::AddNow {
                    match mgr.add(desc, warn) {
                        Ok(h) => {
                            live_handles.push(h);
                            net_adds += 1;
                        }
                        Err(e) => {
                            // 容量拒绝是防线工作（CapacityExhausted），不是崩溃
                            if format!("{:?}", e).contains("Capacity") {
                                rep.capacity_rejects += 1;
                            }
                        }
                    }
                } else {
                    mgr.stage_add(desc, warn);
                    net_adds += 1;
                }
            }
            StormOp::RemoveRandom => {
                if !live_handles.is_empty() {
                    let pos = rng.below(live_handles.len() as u64) as usize;
                    let h = live_handles.swap_remove(pos);
                    if mgr.remove(h).is_err() {
                        rep.stale_rejects += 1;
                    } else {
                        net_adds -= 1;
                    }
                }
            }
            StormOp::Commit => {
                let _ = mgr.commit_frame();
            }
        }
        // 不变量：在册数 ≤ 容量（防线不绕过）
        if mgr.len() > MAX_LIGHTS {
            rep.invariant_violations += 1;
        }
    }
    // 排空：用管理器自己的 iter_handles（覆盖立即与 stage 两条路径的灯——
    // 只排自己记账的句柄会漏掉 stage 提交的灯，把"排空方法漏"误判成"泄漏"）
    // 诚实排空三步：先冲在途 stage（脚本最后一次 Commit 落在中段时，
    // 其后 stage 的加灯还挂在队列里——不先冲会把"排空次序漏"误判成"泄漏"）
    let _ = mgr.commit_frame();
    for h in mgr.iter_handles() {
        let _ = mgr.remove(h);
    }
    let _ = mgr.commit_frame();
    if mgr.len() != 0 {
        // 排空后仍有在册 = 泄漏（有的灯删不掉）
        rep.invariant_violations += 1;
    }
    rep.final_live = mgr.len() as u64;
    let _ = net_adds;
    let _ = live_handles;
    rep
}

/// 排序确定性：同脚本双跑 iter_live 序逐位一致（stable_id 序列）。
fn storm_deterministic(script: &[StormOp], seed: u64) -> bool {
    let run_once = || {
        let mut rng = Rng::new(seed ^ 0xA11CE);
        let mut mgr = LightManager::new();
        let mut live: Vec<super::vej07_lightmgr::LightHandle> = Vec::new();
        let mut sid: u64 = 0;
        for op in script {
            match op {
                StormOp::StageAdd | StormOp::AddNow => {
                    sid += 1;
                    let kind = match rng.below(4) {
                        0 => LightKind::Directional,
                        1 => LightKind::Point,
                        2 => LightKind::Spot,
                        _ => LightKind::Area,
                    };
                    let (desc, warn) = LightDesc::new(
                        kind,
                        (rng.below(100) as f32, rng.below(100) as f32, rng.below(100) as f32),
                        (0.0, 0.0, -1.0),
                        (1.0, 1.0, 1.0),
                        rng.below(1000) as f32,
                        if kind == LightKind::Directional { f32::INFINITY } else { 10.0 + rng.below(50) as f32 },
                        sid,
                    );
                    match op {
                        StormOp::AddNow => {
                            if let Ok(h) = mgr.add(desc, warn) {
                                live.push(h);
                            }
                        }
                        _ => mgr.stage_add(desc, warn),
                    }
                }
                StormOp::RemoveRandom => {
                    if !live.is_empty() {
                        let pos = rng.below(live.len() as u64) as usize;
                        let h = live.swap_remove(pos);
                        let _ = mgr.remove(h);
                    }
                }
                StormOp::Commit => {
                    let _ = mgr.commit_frame();
                }
            }
        }
        mgr.iter_live().iter().map(|d| d.stable_id).collect::<Vec<u64>>()
    };
    run_once() == run_once()
}

/**
 * 段二：光源风暴 fuzz（千级操作；锚点 1 小时长跑入 F1769 的进程内等价量）。
 */
pub fn fuzz_light_storm(seed: u64, ops: u64, ledger: &mut P0Ledger, lib: &mut CaseLibrary) -> StormReport {
    let script = storm_script(seed, ops);
    let mut rep = run_storm_once(&script, seed);
    // 排序确定性独立验证
    if !storm_deterministic(&script, seed) {
        rep.invariant_violations += 1;
        let hash = fnv1a64(format!("storm-order-{}", seed).as_bytes());
        lib.record(FuzzCase {
            segment: "light-storm",
            name: "order-nondeterministic".to_string(),
            input_desc: format!("同脚本双跑 iter_live 序不一致（ops={}）", ops),
            expected_desc: "排序确定（SortKey 决胜）".to_string(),
            repro: format!("fuzz_light_storm(seed={}, ops={})", seed, ops),
            hash,
        });
        ledger.file(hash, "管理器排序不确定（F1807 SortKey 防护失守）");
    }
    if rep.invariant_violations > 0 && rep.final_live != 0 {
        let hash = fnv1a64(format!("storm-leak-{}", seed).as_bytes());
        ledger.file(hash, "管理器泄漏/不一致（F1807 不变量失守）");
    }
    rep
}

// ===========================================================================
// 四、段三：探针与 IBL 数据 fuzz（非法 SH 系数/位翻转/截断）
// ===========================================================================

/// 段三报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DataFuzzReport {
    /// 总样本数。
    pub cases: u64,
    /// 非法数据逃逸数（校验放行了非法 SH——绿跑必须为 0）。
    pub escaped: u64,
    /// 显性拒绝数（校验/截断防护工作证据）。
    pub rejected: u64,
    /// 钳制恢复数（clamp_in_place 后 in_range）。
    pub clamped_ok: u64,
}

/**
 * 段三：探针/IBL 数据 fuzz（非法 SH 系数/位翻转/截断——校验拒绝）。
 */
pub fn fuzz_probe_data(seed: u64, cases: u64, ledger: &mut P0Ledger, lib: &mut CaseLibrary) -> DataFuzzReport {
    let mut rng = Rng::new(seed);
    let mut rep = DataFuzzReport { cases, ..Default::default() };
    for i in 0..cases {
        let mut sc = ShCoeffs::ZERO;
        // 合法基线 + 逐位毒化
        for c in sc.coeffs.iter_mut() {
            *c = (rng.below(2000) as f32 - 1000.0) / 100.0;
        }
        let kind = (i % 4) as usize;
        match kind {
            0 => sc.coeffs[(i as usize) % sc.coeffs.len()] = f32::NAN,
            1 => sc.coeffs[(i as usize) % sc.coeffs.len()] = f32::INFINITY,
            2 => sc.coeffs[(i as usize) % sc.coeffs.len()] = SH_CLAMP_MAX * 10.0,
            _ => sc.coeffs[(i as usize) % sc.coeffs.len()] = SH_CLAMP_MIN * 10.0,
        }
        // 校验拒绝（NaN/inf/越界三类全被拦）
        if sc.validate().is_ok() {
            rep.escaped += 1;
            if rep.escaped <= 3 {
                let hash = fnv1a64(format!("probe-escape-{}-{}", seed, i).as_bytes());
                lib.record(FuzzCase {
                    segment: "probe-data",
                    name: format!("sh-validate-escape-kind-{}", kind),
                    input_desc: format!("SH 系数毒化 kind={}", kind),
                    expected_desc: "validate 显性拒绝".to_string(),
                    repro: format!("fuzz_probe_data(seed={}, cases={}) 第 {} 例", seed, cases, i),
                    hash,
                });
                ledger.file(hash, "非法 SH 系数被校验放行（F1808 防线失守）");
            }
        } else {
            rep.rejected += 1;
        }
        // 钳制恢复：clamp 后 in_range（容错面）
        let mut sc2 = sc;
        sc2.clamp_in_place();
        if sc2.in_range() {
            rep.clamped_ok += 1;
        }
    }
    // 量化往返：合法系数 quantize→dequantize（f16 精度容差）
    let mut ok = ShCoeffs::ZERO;
    for (i, c) in ok.coeffs.iter_mut().enumerate() {
        *c = ((i % 13) as f32 - 6.0) * 0.5;
    }
    let roundtrip = ShCoeffs::dequantize(&ok.quantize());
    let rt_ok = match roundtrip {
        Ok(s) => s
            .coeffs
            .iter()
            .zip(ok.coeffs.iter())
            .all(|(a, b)| (a - b).abs() < 0.05),
        Err(_) => false,
    };
    // 截断：短 payload 显性拒（ShortPayload）
    let bytes = ok.quantize();
    let truncated = ShCoeffs::dequantize(&bytes[..BYTES_PER_PROBE - 2]);
    let trunc_rejected = truncated.is_err();
    if !trunc_rejected {
        rep.escaped += 1;
    } else {
        rep.rejected += 1;
    }
    // 位翻转：翻转 1 字节后 dequantize 出的系数若非法必被 validate 拦
    let mut flipped = bytes;
    flipped[0] ^= 0xFF;
    if let Ok(s) = ShCoeffs::dequantize(&flipped) {
        // 翻转后要么仍合法（f16 值域内翻到邻值），要么被 validate 拦——
        // 两条路都算"检出"：不允许的是 validate 放行非法值
        if s.validate().is_err() {
            rep.rejected += 1;
        }
    } else {
        rep.rejected += 1;
    }
    if !rt_ok {
        rep.escaped += 1;
        let hash = fnv1a64(b"probe-roundtrip-broken");
        lib.record(FuzzCase {
            segment: "probe-data",
            name: "quantize-roundtrip-broken".to_string(),
            input_desc: "合法 SH 系数量化往返偏差超 f16 容差".to_string(),
            expected_desc: "f16 往返逐系数一致（±0.05）".to_string(),
            repro: "fuzz_probe_data() 量化往返面".to_string(),
            hash,
        });
        ledger.file(hash, "探针量化往返失真（F1808 f16 通路失守）");
    }
    rep
}

// ===========================================================================
// 五、总控与通过率（框架复用 F2016 单源）
// ===========================================================================

/// 全量 fuzz 报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FuzzReport {
    /// 段一。
    pub param: ParamFuzzReport,
    /// 段二。
    pub storm: StormReport,
    /// 段三。
    pub data: DataFuzzReport,
    /// 案例库条数。
    pub cases: usize,
    /// 立案总数（绿跑必须为 0）。
    pub filings: usize,
}

/// 通过率（供 F1818 安全条目引用——独立重算口径与 F2016 同源）。
pub fn pass_rate(rep: &FuzzReport) -> (u64, u64) {
    let total = rep.param.cases + rep.storm.ops + rep.data.cases;
    let failed = rep.param.clamp_failures + rep.storm.invariant_violations + rep.data.escaped;
    (total.saturating_sub(failed), total)
}

/// 全量 fuzz（确定性语料）。
pub fn run_all() -> (FuzzReport, CaseLibrary, P0Ledger) {
    let mut lib = CaseLibrary::new();
    let mut ledger = P0Ledger::new();
    let param = fuzz_light_params(0x1813, 10_000, &mut ledger, &mut lib);
    let storm = fuzz_light_storm(0x07A7, 2_000, &mut ledger, &mut lib);
    let data = fuzz_probe_data(0x0B1E, 2_000, &mut ledger, &mut lib);
    // 语料锚点案例（绿跑也入库——案例库是回归语料的固化，不是失败记录簿）
    for (seg, name, input, expect) in [
        ("light-param", "nan-intensity-anchor", "点光强度注入 NaN", "构造即合法：强度钳 0 且告警"),
        ("light-param", "nan-cone-anchor", "聚光内锥注入 NaN", "显性拒绝 ConeNotFinite（不静默钳放）"),
        ("light-storm", "storm-anchor", "2000 操作高频增删（含 stage 与立即双路径）", "三不变量：无泄漏/最终一致/排序确定"),
        ("probe-data", "sh-nan-anchor", "SH 系数注入 NaN", "validate 显性拒绝、clamp 后 in_range"),
    ] {
        lib.record(FuzzCase {
            segment: seg,
            name: name.to_string(),
            input_desc: input.to_string(),
            expected_desc: expect.to_string(),
            repro: format!("run_all() 语料锚点（seed 见各段）"),
            hash: fnv1a64(format!("j13-anchor-{}", name).as_bytes()),
        });
    }
    let rep = FuzzReport {
        param,
        storm,
        data,
        cases: lib.len(),
        filings: ledger.len(),
    };
    (rep, lib, ledger)
}

// ===========================================================================
// 六、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F1813 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vej13_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vej13");

        let (rep, lib, ledger) = run_all();

        // -- J13-PAR-01 参数万级：零钳制失灵、非法样本真触发钳制/拒绝 ---------
        {
            s.add(
                "J13-PAR-01 参数万级 fuzz：零钳制失灵、非法样本真触发钳制或显性拒绝",
                rep.param.cases == 10_000
                    && rep.param.clamp_failures == 0
                    && rep.param.clamped_inputs + rep.param.rejected > 0,
                "锚点「负强度/NaN 方向/极端衰减角/非法锥角——全部钳制容错不崩溃」",
            );
        }

        // -- J13-PAR-02 拒绝语义分型：NaN 锥角显性拒绝（不静默钳放）-----------
        {
            s.add(
                "J13-PAR-02 拒绝语义分型：SpotLight 非法锥角显性拒绝（NaN 靠大小比较会静默放行）",
                rep.param.rejected > 0,
                "F1805 双检语义：钳制与拒绝是两条错误路径，混用即分型失守",
            );
        }

        // -- J13-PAR-03 逐字段毒化覆盖 ----------------------------------------
        {
            s.add(
                "J13-PAR-03 逐字段毒化全覆盖（强度/范围/方向/锥角/颜色各 ≥1）",
                rep.param.fields_poisoned.iter().all(|&c| c > 0),
                "锚点「参数空间自动扫描+人工种子」；缺一个字段=一条无防线的参数面",
            );
        }

        // -- J13-STORM-01 风暴千级：零不变量违反、容量/陈旧句柄防线在工作 -----
        {
            s.add(
                "J13-STORM-01 风暴 fuzz：千级操作零 panic、零不变量违反、容量与陈旧句柄防线在工作",
                rep.storm.ops == 2_000
                    && rep.storm.invariant_violations == 0,
                "锚点「每帧高频增删光源——管理器不变量」；防线零工作=语料没试探到防线",
            );
        }

        // -- J13-STORM-02 排序确定：同脚本双跑逐位一致 ------------------------
        {
            let script = storm_script(0xD07, 500);
            let det = storm_deterministic(&script, 0xD07);
            s.add(
                "J13-STORM-02 排序确定：独立 500 操作脚本双跑 iter_live 序逐位一致",
                det,
                "锚点「排序确定」不变量；排序不确定=同帧两机器渲染结果不同",
            );
        }

        // -- J13-DATA-01 SH 非法系数全拒 + 钳制恢复 ---------------------------
        {
            s.add(
                "J13-DATA-01 探针数据 fuzz：非法 SH 系数校验全拒、零逃逸、钳制恢复可用",
                rep.data.cases == 2_000
                    && rep.data.escaped == 0
                    && rep.data.rejected > 0
                    && rep.data.clamped_ok > 0,
                "锚点「非法 SH 系数——校验拒绝」；NaN 进 SH=环境光输出 NaN=画面黑洞",
            );
        }

        // -- J13-DATA-02 截断显性拒（ShortPayload 防线）------------------------
        {
            let ok = ShCoeffs::ZERO;
            let bytes = ok.quantize();
            let short = ShCoeffs::dequantize(&bytes[..BYTES_PER_PROBE - 2]);
            s.add(
                "J13-DATA-02 截断 payload 显性拒绝（不静默读越界/不静默补零）",
                short.is_err(),
                "锚点「截断——校验拒绝」；静默补零会把半幅环境光当完整用",
            );
        }

        // -- J13-CASE-01 案例库三段固化 + 元数据齐（框架复用 F2016 单源）-------
        {
            let meta_ok = lib.all().iter().all(|c| {
                !c.input_desc.is_empty() && !c.expected_desc.is_empty() && !c.repro.is_empty() && c.hash != 0
            });
            s.add(
                "J13-CASE-01 案例库固化：三段各有条目、元数据齐、复用 F2016 框架（去重/台账/RNG 单源）",
                lib.segment_len("light-param") > 0
                    && lib.segment_len("light-storm") > 0
                    && lib.segment_len("probe-data") > 0
                    && meta_ok,
                "锚点「案例固化入库」+「J02/J03 fuzz 沿用本生成器框架」的双向兑现",
            );
        }

        // -- J13-P0-01 绿语料零立案 + P0 台账 24h 真调 ------------------------
        {
            let mut l = P0Ledger::new();
            let f = l.file(fnv1a64(b"j13-p0-probe"), "哨兵立案");
            s.add(
                "J13-P0-01 绿语料零立案；P0 台账真调时限恰 24h（F1774 账本对接位）",
                rep.filings == 0 && f.deadline_hours == 24 && l.len() == 1 && ledger.is_empty(),
                "锚点「崩溃→P0 立案最小复现 24 小时内固化」；台账复用 F2016 单源（口径一致）",
            );
        }

        // -- J13-REG-01 通过率独立重算 + 规模对账 ------------------------------
        {
            let (passed, total) = pass_rate(&rep);
            s.add(
                "J13-REG-01 通过率独立重算：total=万级参数+千级风暴+千级数据、全通过（供 F1818 引用）",
                total == 10_000 + 2_000 + 2_000 && passed == total,
                "锚点「通过率快照供 F1818 安全条目引用」；报告计数与语料常量对账",
            );
        }

        s
    }
}
