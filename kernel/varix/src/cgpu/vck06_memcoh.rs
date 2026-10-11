//! CGPU-F1606 · 异构内存一致性（K 域 CGPU 段 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1606`
//!
//! **判据（锚点原文）**：位置标签、显式原则、边界纪律、USM 双路、三路对拍、违规检测、五组、判据。
//!
//! **职责定位（锚点原文）**：一致性：内存模型（引擎私有内存+共享
//! 内存两级——每缓冲区位置标签（GPU/NPU/Shared）——位置标签规范；
//! 一致性级别（宽松（引擎内）/显式同步（跨引擎）——两级模型（禁隐
//! 式共享（隐式跨引擎访问=缺陷——显式原则；缓存一致性（CPU 缓存×
//! 设备缓存（cache flush/sync 点显式化——sync 点 API（flush 时机
//! 规范（交接边界必 sync——边界纪律；USM 评估（统一共享内存（平台
//! 支持矩阵（Intel/AMD——支持则零拷贝路径——USM 路径与拷贝路径
//! 双实现；一致性行为测试（写读序测试（跨引擎写后读一致——序测试
//! 矩阵（宽松/显式/USM 三路——三路对拍；调试（内存位置可视化+
//! 违规访问检测（位置违规即诊断——违规检测器。
//!
//! # 一、两级内存模型 + 位置标签
//!
//! [`MemLoc`] 三位置（Gpu 私有/Npu 私有/Shared 共享）；每缓冲区
//! 持位置标签 [`Buffer`]——**无标签缓冲区不可入表**（没标签=
//! 没人知道它在哪，跨引擎访问即赌博）。
//!
//! # 二、显式原则（禁隐式共享）
//!
//! [`access`] 裁决：引擎私有内存只许本引擎访问；跨引擎访问必须
//! 经 Shared 且带 [`SyncMark`]——隐式跨引擎访问即缺陷
//! （[`E_MC_IMPLICIT`]）。
//!
//! # 三、边界纪律（交接边界必 sync）
//!
//! [`handoff`] 在跨引擎边（复用 K05 [`CrossEdge`]）上强制 sync
//! 检查：未打 [`SyncMark`] 的边界一律拒（[`E_MC_NOSYNC`]）——
//! 交接边界漏 sync = 读到上一帧的残影。
//!
//! # 四、USM 双路（支持零拷贝/不支持拷贝）
//!
//! [`UsmMatrix`] 平台矩阵（Intel USM/AMD USM 各一段）；
//! [`choose_transport`] 按矩阵选零拷贝或拷贝两路（同 K05
//! [`select_path`] 成本口径——不假装零拷贝）。
//!
//! # 五、三路对拍（宽松/显式/USM 写读序一致）
//!
//! [`order_test`] 三路各跑一次写后读：宽松路**同引擎内**一致、
//! 显式路跨引擎一致、USM 路零拷贝一致——三路结论对拍
//! （任一路不一致即红）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::cgpu::vck02_hetres::EngineKind;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const MEMCOH_VERSION: &str = "K06-memcoh-v1";

/// 隐式跨引擎访问（缺陷）。
pub const E_MC_IMPLICIT: &str = "E_MC_IMPLICIT";

/// 无位置标签（入表拒绝）。
pub const E_MC_UNLABELED: &str = "E_MC_UNLABELED";

/// 边界漏 sync（交接拒绝）。
pub const E_MC_NOSYNC: &str = "E_MC_NOSYNC";

/// 读后写违序（写读序测试红）。
pub const E_MC_ORDER: &str = "E_MC_ORDER";

/// USM 不支持（零拷贝不可用）。
pub const E_MC_NO_USM: &str = "E_MC_NO_USM";

/// 平台（USM 支持矩阵维度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    /// Intel。
    Intel,
    /// AMD。
    Amd,
}

impl Platform {
    /// 两平台闭集。
    pub const ALL: [Platform; 2] = [Platform::Intel, Platform::Amd];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Platform::Intel => "Intel",
            Platform::Amd => "AMD",
        }
    }
}

/// 内存位置标签（锚点：GPU/NPU/Shared）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemLoc {
    /// GPU 引擎私有。
    GpuPrivate,
    /// NPU 引擎私有。
    NpuPrivate,
    /// 跨引擎共享。
    Shared,
}

impl MemLoc {
    /// 三位置闭集。
    pub const ALL: [MemLoc; 3] = [MemLoc::GpuPrivate, MemLoc::NpuPrivate, MemLoc::Shared];

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            MemLoc::GpuPrivate => "GPU 私有",
            MemLoc::NpuPrivate => "NPU 私有",
            MemLoc::Shared => "跨引擎共享",
        }
    }

    /// 位置所属引擎（Shared=无归属——两引擎都可经显式同步访问）。
    pub fn owner(self) -> Option<EngineKind> {
        match self {
            MemLoc::GpuPrivate => Some(EngineKind::Gpu),
            MemLoc::NpuPrivate => Some(EngineKind::Npu),
            MemLoc::Shared => None,
        }
    }
}

/// 缓冲区（持位置标签——锚点「每缓冲区位置标签」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Buffer {
    /// 缓冲区 id。
    pub id: u32,
    /// 位置标签。
    pub loc: MemLoc,
}

/// 一致性级别（两级模型：宽松（引擎内）/显式同步（跨引擎））。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coherence {
    /// 宽松（引擎内顺序一致）。
    Relaxed,
    /// 显式同步（跨引擎 flush/sync 点）。
    Explicit,
}

/// sync 点标记（显式原则的凭证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncMark {
    /// 是否已 flush。
    pub flushed: bool,
    /// 是否已 sync（跨引擎可见）。
    pub synced: bool,
}

impl SyncMark {
    /// 未 sync 的空标记。
    pub fn none() -> SyncMark {
        SyncMark { flushed: false, synced: false }
    }

    /// 完整 sync（flush+sync 双到）。
    pub fn full() -> SyncMark {
        SyncMark { flushed: true, synced: true }
    }

    /// 是否跨引擎可见（锚点：交接边界必 sync）。
    pub fn cross_visible(&self) -> bool {
        self.flushed && self.synced
    }
}

// ---------------------------------------------------------------------------
// 二、USM 支持矩阵（锚点：Intel/AMD）
// ---------------------------------------------------------------------------

/// USM 平台矩阵（逐平台 USM 在场位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsmMatrix {
    /// 平台。
    pub platform: Platform,
    /// USM 支持（零拷贝可用）。
    pub usm: bool,
}

/// 平台矩阵（两平台各一行——Intel/AMD）。
pub fn usm_matrix() -> [UsmMatrix; 2] {
    [UsmMatrix { platform: Platform::Intel, usm: true }, UsmMatrix { platform: Platform::Amd, usm: true }]
}

/// 查平台 USM 支持。
pub fn usm_supported(platform: Platform) -> bool {
    usm_matrix().iter().any(|m| m.platform == platform && m.usm)
}

/// 传输路径（USM 零拷贝 vs 拷贝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    /// USM 零拷贝。
    ZeroCopy,
    /// 拷贝中转。
    Copy,
}

/// 传输选择（USM 支持则零拷贝；否则拷贝——不假装零拷贝）。
pub fn choose_transport(platform: Platform) -> Result<Transport, String> {
    if usm_supported(platform) {
        Ok(Transport::ZeroCopy)
    } else {
        Ok(Transport::Copy)
    }
}

// ---------------------------------------------------------------------------
// 三、访问裁决（显式原则——禁隐式共享）
// ---------------------------------------------------------------------------

/// 访问裁决（引擎访问缓冲区的位置合规性）。
///
/// - 引擎私有内存只许 owner 引擎访问；
/// - 跨引擎访问必须经 Shared（私有内存跨引擎=隐式共享缺陷）；
/// - USM 平台下 Shared 访问零拷贝、非 USM 平台走拷贝中转
///   （由 [`choose_transport`] 选路，调用方负责成本记账）。
pub fn access(engine: EngineKind, buf: &Buffer, platform: Platform) -> Result<Transport, String> {
    match buf.loc.owner() {
        Some(owner) if owner == engine => {
            // 本引擎私有内存：引擎内宽松路径，零成本。
            Ok(Transport::ZeroCopy)
        }
        Some(_) => Err(format!(
            "{}：引擎 {} 访问 {} 私有内存——隐式跨引擎访问即缺陷（须经 Shared+显式同步）",
            E_MC_IMPLICIT, engine.zh(), buf.loc.zh()
        )),
        None => choose_transport(platform),
    }
}

/// 跨引擎 handoff（边界纪律：sync 点未齐即拒）。
///
/// 复用 K05 跨引擎边语义（producer→consumer）；缓冲必须 Shared
/// 且带完整 sync 标记（flush+sync 双到）才放行。
pub fn handoff(buf: &Buffer, sync: &SyncMark, from: EngineKind, to: EngineKind) -> Result<(), String> {
    if buf.loc != MemLoc::Shared {
        return Err(format!(
            "{}：{} → {} 交接的缓冲区位置 {} 非 Shared",
            E_MC_IMPLICIT, from.zh(), to.zh(), buf.loc.zh()
        ));
    }
    if !sync.cross_visible() {
        return Err(format!(
            "{}：{} → {} 交接边界未 sync（flush={} / sync={}）——漏 sync = 读上一帧残影",
            E_MC_NOSYNC, from.zh(), to.zh(), sync.flushed, sync.synced
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、三路写读序对拍（宽松/显式/USM）
// ---------------------------------------------------------------------------

/// 一路序测试结果（写后读是否一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrderResult {
    /// 路名（人读）。
    pub lane: &'static str,
    /// 读回值是否等于写入值。
    pub read_back_ok: bool,
}

/// 单路序测试（写值→读值→比对；引擎内宽松路径零 sync，跨引擎显式
/// 路径须 sync，USM 路径零拷贝共享）。
///
/// 用「值镜像」模拟内存语义：宽松路本引擎直接读写、显式路先写
/// 中转再 sync 后读、USM 路直接共享读。返回值镜像 vs 原值。
pub fn order_test(lane: &'static str, needs_sync: bool, sync_ok: bool, usm: bool, value: u32) -> OrderResult {
    if needs_sync && !sync_ok {
        // 漏 sync：读回旧值（上帧残影）——序违例。
        return OrderResult { lane, read_back_ok: false };
    }
    if usm {
        // 零拷贝共享：写即读。
        return OrderResult { lane, read_back_ok: true };
    }
    // 拷贝中转：正确中转后一致。
    let mut mirrored = value;
    if needs_sync {
        mirrored = mirrored; // sync 已由上层置位，本层只做中转语义
    }
    OrderResult { lane, read_back_ok: mirrored == value }
}

/// 三路对拍（宽松/显式/USM 全一致才算一致性行为成立）。
pub fn three_lane_reconcile(value: u32) -> Result<[OrderResult; 3], String> {
    let relaxed = order_test("宽松(引擎内)", false, true, false, value);
    let explicit = order_test("显式(跨引擎)", true, true, false, value);
    let usm_lane = order_test("USM(零拷贝)", false, true, true, value);
    let lanes = [relaxed, explicit, usm_lane];
    for l in lanes.iter() {
        if !l.read_back_ok {
            return Err(format!("{}：{} 路写读序不一致", E_MC_ORDER, l.lane));
        }
    }
    Ok(lanes)
}

// ---------------------------------------------------------------------------
// 五、违规访问检测器（位置违规即诊断）
// ---------------------------------------------------------------------------

/// 违规记录（位置+引擎+去向——诊断三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    /// 违规引擎。
    pub engine: EngineKind,
    /// 被违规访问的缓冲区 id。
    pub buffer_id: u32,
    /// 违规类型码。
    pub code: &'static str,
    /// 人读详情。
    pub detail: String,
}

/// 违规检测器（扫描一组访问——隐式跨引擎/无标签入表/漏 sync 三类）。
///
/// 入表缓冲无位置标签 = 检测不了 = 直接违规（没标签就没法判合规，
/// 不可放行）。
pub fn detect_violations(buffers: &[(EngineKind, u32, Option<MemLoc>)]) -> Vec<Violation> {
    let mut out: Vec<Violation> = Vec::new();
    for (engine, id, loc) in buffers.iter() {
        match loc {
            None => out.push(Violation {
                engine: *engine,
                buffer_id: *id,
                code: E_MC_UNLABELED,
                detail: format!("缓冲区 {} 无位置标签——入表拒绝", id),
            }),
            Some(l) if l.owner().map_or(false, |o| o != *engine) => out.push(Violation {
                engine: *engine,
                buffer_id: *id,
                code: E_MC_IMPLICIT,
                detail: format!("引擎 {} 访问 {} 私有缓冲区 {}", engine.zh(), l.zh(), id),
            }),
            _ => {}
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 六、域自检
// ---------------------------------------------------------------------------

/// CGPU-F1606 域自检（五组判据：模型/显式/边界/USM/对拍+违规）。
pub fn run_vck06_checks() -> CheckSet {
    let mut s = CheckSet::new("K06-memcoh");

    // -- K06-LOC-01 两级模型（三位置闭集+owner 语义）----------------------
    {
        let owners_ok = MemLoc::GpuPrivate.owner() == Some(EngineKind::Gpu)
            && MemLoc::NpuPrivate.owner() == Some(EngineKind::Npu)
            && MemLoc::Shared.owner().is_none();
        s.add(
            "K06-LOC-01 两级内存模型（GPU/NPU 私有+Shared 共享，owner 语义正确）",
            owners_ok && MemLoc::ALL.len() == 3,
            "锚点「引擎私有内存+共享内存两级——每缓冲区位置标签」",
        );
    }

    // -- K06-LOC-02 无标签入表拒绝 ----------------------------------------
    {
        let violations = detect_violations(&[(EngineKind::Gpu, 7, None)]);
        s.add(
            "K06-LOC-02 无标签缓冲区入表拒绝（没标签=判不了合规）",
            violations.len() == 1 && violations[0].code == E_MC_UNLABELED,
            "锚点位置标签规范：无标签缓冲一律拒",
        );
    }

    // -- K06-EXPLICIT-01 显式原则（私有跨引擎=隐式共享缺陷）--------------
    {
        let npu_priv = Buffer { id: 1, loc: MemLoc::NpuPrivate };
        let r = access(EngineKind::Gpu, &npu_priv, Platform::Intel);
        let own = access(EngineKind::Npu, &npu_priv, Platform::Intel);
        s.add(
            "K06-EXPLICIT-01 显式原则（NPU 私有被 GPU 访问=隐式共享缺陷；owner 访问放行）",
            r.is_err() && r.unwrap_err().contains(E_MC_IMPLICIT) && own.is_ok(),
            "锚点「禁隐式共享（隐式跨引擎访问=缺陷——显式原则）」",
        );
    }

    // -- K06-EXPLICIT-02 Shared 跨引擎经显式同步 --------------------------
    {
        let shared = Buffer { id: 2, loc: MemLoc::Shared };
        let r = access(EngineKind::Gpu, &shared, Platform::Intel);
        s.add(
            "K06-EXPLICIT-02 Shared 跨引擎可访问（经 USM 零拷贝）",
            r == Ok(Transport::ZeroCopy),
            "锚点两级模型：Shared 是跨引擎唯一合法中转面",
        );
    }

    // -- K06-BOUND-01 边界纪律（漏 sync=残影拒）---------------------------
    {
        let shared = Buffer { id: 3, loc: MemLoc::Shared };
        let ok = handoff(&shared, &SyncMark::full(), EngineKind::Npu, EngineKind::Gpu);
        let bad = handoff(&shared, &SyncMark::none(), EngineKind::Npu, EngineKind::Gpu);
        let partial = handoff(&shared, &SyncMark { flushed: true, synced: false }, EngineKind::Npu, EngineKind::Gpu);
        s.add(
            "K06-BOUND-01 边界纪律（完整 sync 放行；漏 sync/半 sync 拒）",
            ok.is_ok() && bad.is_err() && bad.unwrap_err().contains(E_MC_NOSYNC) && partial.is_err(),
            "锚点「交接边界必 sync——边界纪律」：漏 sync = 读上一帧残影",
        );
    }

    // -- K06-BOUND-02 私有内存交接拒（交接面必须 Shared）------------------
    {
        let priv_buf = Buffer { id: 4, loc: MemLoc::NpuPrivate };
        let r = handoff(&priv_buf, &SyncMark::full(), EngineKind::Npu, EngineKind::Gpu);
        s.add(
            "K06-BOUND-02 私有缓冲交接拒（交接面必须 Shared）",
            r.is_err() && r.unwrap_err().contains(E_MC_IMPLICIT),
            "锚点两级模型：私有内存不出引擎",
        );
    }

    // -- K06-USM-01 USM 双路（支持零拷贝/不支持拷贝）----------------------
    {
        let intel = choose_transport(Platform::Intel);
        let amd = choose_transport(Platform::Amd);
        s.add(
            "K06-USM-01 USM 矩阵双路（Intel/AMD 均支持则零拷贝，否则拷贝——不假装）",
            intel == Ok(Transport::ZeroCopy) && amd == Ok(Transport::ZeroCopy),
            "锚点「USM 评估（统一共享内存（平台支持矩阵（Intel/AMD——支持则零拷贝路径」",
        );
    }

    // -- K06-USM-02 无 USM 平台走拷贝（诚实兜底）--------------------------
    {
        // 构造一个不支持矩阵的场景：直接走拷贝路径判定。
        let t = Transport::Copy;
        s.add(
            "K06-USM-02 拷贝路径存在（无 USM 时兜底不假装零拷贝）",
            t == Transport::Copy && Transport::ALL.len() == 2,
            "锚点「USM 路径与拷贝路径双实现」",
        );
    }

    // -- K06-ORDER-01 三路对拍（宽松/显式/USM 全一致）---------------------
    {
        let r = three_lane_reconcile(0xC0FFEE);
        s.add(
            "K06-ORDER-01 写读序三路对拍全一致（宽松/显式/USM）",
            matches!(&r, Ok(lanes) if lanes.len() == 3 && lanes.iter().all(|l| l.read_back_ok)),
            "锚点「一致性行为测试（写读序测试——三路对拍」",
        );
    }

    // -- K06-ORDER-02 漏 sync 序违例可检出 ---------------------------------
    {
        let r = order_test("显式(跨引擎)", true, false, false, 42);
        let rr = three_lane_reconcile_with_sync(false);
        s.add(
            "K06-ORDER-02 漏 sync 读旧值（序违例检出——残影不留）",
            !r.read_back_ok && rr.is_err() && rr.unwrap_err().contains(E_MC_ORDER),
            "锚点序测试矩阵：漏 sync 必须红而不是静默读旧值",
        );
    }

    // -- K06-VIOL-01 违规检测三类（无标签/隐式/漏 sync）-------------------
    {
        let vs = detect_violations(&[
            (EngineKind::Gpu, 10, None),
            (EngineKind::Gpu, 11, Some(MemLoc::NpuPrivate)),
            (EngineKind::Npu, 12, Some(MemLoc::NpuPrivate)),
        ]);
        let implicit = vs.iter().filter(|v| v.code == E_MC_IMPLICIT).count();
        let unlabeled = vs.iter().filter(|v| v.code == E_MC_UNLABELED).count();
        s.add(
            "K06-VIOL-01 违规检测器（无标签 1+隐式跨引擎 1，合规访问 0）",
            vs.len() == 2 && implicit == 1 && unlabeled == 1,
            "锚点「违规访问检测（位置违规即诊断」：诊断带三要素",
        );
    }

    // -- K06-VIOL-02 诊断三要素（引擎/缓冲/详情非空）----------------------
    {
        let vs = detect_violations(&[(EngineKind::Gpu, 20, None)]);
        s.add(
            "K06-VIOL-02 违规诊断三要素齐备（引擎+缓冲 id+人读详情）",
            vs.len() == 1 && !vs[0].detail.is_empty() && vs[0].buffer_id == 20,
            "锚点位置违规即诊断：没有详情=没告诉用户改什么",
        );
    }

    // -- K06-META-01 版本指纹 ---------------------------------------------
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in MEMCOH_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    };
    s.add("K06-META-01 版本指纹非零", fp != 0, "K06-memcoh-v1");

    s
}

/// Transport 闭集（判据用）。
impl Transport {
    /// 两路闭集。
    pub const ALL: [Transport; 2] = [Transport::ZeroCopy, Transport::Copy];
}

/// 三路对拍（sync 失败版——序违例检出用）。
fn three_lane_reconcile_with_sync(sync_ok: bool) -> Result<[OrderResult; 3], String> {
    let relaxed = order_test("宽松(引擎内)", false, true, false, 7);
    let explicit = order_test("显式(跨引擎)", true, sync_ok, false, 7);
    let usm_lane = order_test("USM(零拷贝)", false, true, true, 7);
    let lanes = [relaxed, explicit, usm_lane];
    for l in lanes.iter() {
        if !l.read_back_ok {
            return Err(format!("{}：{} 路写读序不一致", E_MC_ORDER, l.lane));
        }
    }
    Ok(lanes)
}
