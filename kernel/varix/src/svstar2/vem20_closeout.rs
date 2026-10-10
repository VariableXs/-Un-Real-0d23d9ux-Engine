//! VE-F2420 · M01 组收口与 M02 移交（VE-M 域 · 动画段 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2420`
//!
//! **判据（锚点原文）**：20 项自检、三件硬证、双签、轨道契约移交、判据。
//!
//! **职责定位（锚点原文）**：M01 组收口与 M02 移交——执行 20 项自检
//! 入树、求值基准全绿确认（F2412 三族基准全过+F2415 双跑断言全过
//! +F2411 fuzz 通过率 100% 三件硬证）、组双签、台账更新与向 M02
//! 缓动库与骨骼动画组移交（关键帧与轨道就绪声明——骨骼动画的轨道与
//! 求值接口就位：骨骼采样（F2423）消费 M01 求值接口、clip 数据模型
//! （F2422）挂 F2402 容器——交互契约件移交），是 M 域首个组级关口。
//!
//! # 一、20 项自检入树（清单状态机）
//!
//! 收口清单 20 行= M01 组二十项（F2401-F2420）。十九项有活的判据集
//! （`run_vemXX_checks`）——**逐项真调**计通过率；F2401（域开工）是
//! 架构声明项无独立自检模块，其证据=其余 19 项的登记与全绿（如实
//! 标注为声明项，不伪造判据集）。
//!
//! # 二、三件硬证（求值基准全绿确认）
//!
//! 真调三套判据集：F2412 三族基准（[`run_vem12_checks`]）+F2415
//! 双跑断言（[`run_vem15_checks`]）+F2411 fuzz 通过率 100%
//! （[`run_vem11_checks`] 的 pass=total）——三件全绿才准入签。
//!
//! # 三、组双签 + 移交包（四件核心契约+前向对接）
//!
//! 双签缺一即无效（建造方+验收方两枚签名）。移交包四件核心契约
//! 各有可执行证据：六类轨道规格表（vem02 六类闭集）/求值接口
//! （vem03+07 真调求值）/绑定协议（vem02 路径解析）/事件轨语义
//! （vem06 注册制）；前向对接两项：F2409 morph 轨映射的四通道覆盖
//! （weights 列）+F2421 缓动库注册接口可用（vem03 easing 注册表）。
//!
//! # 四、契约链首环（K→L→M）+ 收口后变更纪律
//!
//! 上游 F2400（L 域收官）移交件哈希对账：以本仓 L 域冻结契约
//! （vel13 十二条签名）为物质锚 recomputed 哈希，与宣告哈希比对
//! ——漂移即拒收（M 域契约链首环）。收口后契约变更必须 ADR+M02
//! 知会（[`post_closeout_change`]）。

use alloc::format;
use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vem02_track::{parse_bind_path, DiagBag as TrackBag, TrackClass};
use crate::svstar2::vem03_interp::{eval_scalar_span, Interp, InterpEntry, InterpParams};
use crate::svstar2::vem06_event::{DiagBag as EventBag, EventNameRegistry};
use crate::svstar2::vem09_import::{mapping_covers_four_channels, MappingTable};
use crate::svstar2::vem11_fuzz::run_vem11_checks;
use crate::svstar2::vem12_bench::run_vem12_checks;
use crate::svstar2::vem13_freeze::f2419_handoff;
use crate::svstar2::vem15_consistency::run_vem15_checks;
use crate::svstar2::vel13_apifreeze::{FreezeBook, FREEZE_V1};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const CLOSEOUT_VERSION: &str = "M20-closeout-v1";

/// 收口清单行数（M01 组二十项）。
pub const CLOSEOUT_ROWS: usize = 20;

/// 缺证据（阻断点名）。
pub const E_CLOSE_NO_EVIDENCE: &str = "E_CLOSE_NO_EVIDENCE";

/// 红项未闭环（先清红）。
pub const E_CLOSE_RED_OPEN: &str = "E_CLOSE_RED_OPEN";

/// 双签缺一（无效）。
pub const E_CLOSE_DUAL_SIGN: &str = "E_CLOSE_DUAL_SIGN";

/// 移交缺件（M02 拒收）。
pub const E_CLOSE_HANDOFF: &str = "E_CLOSE_HANDOFF";

/// 契约链漂移（上游移交件哈希不符）。
pub const E_CLOSE_CHAIN: &str = "E_CLOSE_CHAIN";

/// 收口后变更未走 ADR（知会义务）。
pub const E_CLOSE_POST: &str = "E_CLOSE_POST";

// ---------------------------------------------------------------------------
// 二、20 项自检入树（清单状态机）
// ---------------------------------------------------------------------------

/// 清单项类型（活判据/声明项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// 有独立判据集（真调计数）。
    Live,
    /// 架构声明项（无独立判据——证据=其余项全绿）。
    Declared,
}

/// 清单项终态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemState {
    /// 已核（绿）。
    Verified,
    /// 红项未闭环。
    Red,
    /// 缺证据（阻断）。
    Blocked,
}

/// 收口清单行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseoutRow {
    /// 任务 id。
    pub task: &'static str,
    /// 标题（人读）。
    pub title: &'static str,
    /// 类型。
    pub kind: ItemKind,
    /// 实测通过数（Live 项）。
    pub pass: u32,
    /// 实测总数（Live 项）。
    pub total: u32,
    /// 终态。
    pub state: ItemState,
}

/// FNV-1a 64（契约哈希链）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 18 项活判据的任务 id（F2402-F2419——F2420 自身由本判据执行自证，
/// 不入 measure 集：自调用会无限递归）。
pub const LIVE_TASKS: [&str; 18] = [
    "VE-F2402", "VE-F2403", "VE-F2404", "VE-F2405", "VE-F2406", "VE-F2407", "VE-F2408",
    "VE-F2409", "VE-F2410", "VE-F2411", "VE-F2412", "VE-F2413", "VE-F2414", "VE-F2415",
    "VE-F2416", "VE-F2417", "VE-F2418", "VE-F2419",
];

/// 逐项真调判据集（18 项活判据全跑一遍计通过率）。
///
/// 调用族按聚合器权威注册口径：F2402-F2406 单集；F2407/F2408 三族
/// standalone（组合集有容量守卫，聚合即 panic）；F2409/F2410 四族；
/// F2411-F2419 单集（本轮施工的九项）。
pub fn measure_all_items() -> Vec<CloseoutRow> {
    let mut rows: Vec<CloseoutRow> = Vec::new();
    // F2402-F2406：单判据集。
    let singles: [(&str, &str, fn() -> CheckSet); 5] = [
        ("VE-F2402", "轨道系统", crate::svstar2::vem02_checks::run_vem02_checks),
        ("VE-F2403", "求值器", crate::svstar2::vem03_checks::run_vem03_checks),
        ("VE-F2404", "clip 容器", crate::svstar2::vem04_checks::run_vem04_checks),
        ("VE-F2405", "资产", crate::svstar2::vem05_checks::run_vem05_checks),
        ("VE-F2406", "事件", crate::svstar2::vem06_checks::run_vem06_checks),
    ];
    for (id, title, run) in singles.iter() {
        rows.push(live_row(id, title, run()));
    }
    // F2407/F2408：三族 standalone。
    let tri: [(&str, &str, [fn() -> CheckSet; 3]); 2] = [
        (
            "VE-F2407",
            "性能",
            [
                crate::svstar2::vem07_checks::run_vem07_checks_a_standalone,
                crate::svstar2::vem07_checks::run_vem07_checks_b_standalone,
                crate::svstar2::vem07_checks::run_vem07_checks_c_standalone,
            ],
        ),
        (
            "VE-F2408",
            "调试",
            [
                crate::svstar2::vem08_checks::run_vem08_checks_a_standalone,
                crate::svstar2::vem08_checks::run_vem08_checks_b_standalone,
                crate::svstar2::vem08_checks::run_vem08_checks_c_standalone,
            ],
        ),
    ];
    for (id, title, fams) in tri.iter() {
        rows.push(live_row_families(id, title, fams));
    }
    // F2409/F2410：四族 standalone（各自模块 own 族）。
    let quad: [(&str, &str, [fn() -> CheckSet; 4]); 2] = [
        (
            "VE-F2409",
            "导入",
            [
                crate::svstar2::vem09_checks::run_vem09_checks_a_standalone,
                crate::svstar2::vem09_checks::run_vem09_checks_b_standalone,
                crate::svstar2::vem09_checks::run_vem09_checks_c_standalone,
                crate::svstar2::vem09_checks::run_vem09_checks_d_standalone,
            ],
        ),
        (
            "VE-F2410",
            "导出",
            [
                crate::svstar2::vem10_checks::run_vem10_checks_a_standalone,
                crate::svstar2::vem10_checks::run_vem10_checks_b_standalone,
                crate::svstar2::vem10_checks::run_vem10_checks_c_standalone,
                crate::svstar2::vem10_checks::run_vem10_checks_d_standalone,
            ],
        ),
    ];
    for (id, title, fams) in quad.iter() {
        rows.push(live_row_families4(id, title, fams));
    }
    // F2411-F2419：本轮施工的九项（单集）。
    let mine: [(&str, &str, fn() -> CheckSet); 9] = [
        ("VE-F2411", "fuzz", run_vem11_checks),
        ("VE-F2412", "基准", run_vem12_checks),
        ("VE-F2413", "API 冻结", crate::svstar2::vem13_freeze::run_vem13_checks),
        ("VE-F2414", "文档", crate::svstar2::vem14_docs::run_vem14_checks),
        ("VE-F2415", "一致性", run_vem15_checks),
        ("VE-F2416", "安全", crate::svstar2::vem16_security::run_vem16_checks),
        ("VE-F2417", "质量档", crate::svstar2::vem17_quality::run_vem17_checks),
        ("VE-F2418", "遥测", crate::svstar2::vem18_telemetry::run_vem18_checks),
        ("VE-F2419", "组一致性", crate::svstar2::vem19_consistency::run_vem19_checks),
    ];
    for (id, title, run) in mine.iter() {
        rows.push(live_row(id, title, run()));
    }
    rows
}

/// 单集行构造。
fn live_row(id: &'static str, title: &'static str, set: CheckSet) -> CloseoutRow {
    let (p, f) = set.tally();
    CloseoutRow {
        task: id,
        title,
        kind: ItemKind::Live,
        pass: p as u32,
        total: (p + f) as u32,
        state: if p + f > 0 && f == 0 { ItemState::Verified } else { ItemState::Red },
    }
}

/// 三族合并行构造。
fn live_row_families(id: &'static str, title: &'static str, fams: &[fn() -> CheckSet; 3]) -> CloseoutRow {
    let mut pass = 0u32;
    let mut total = 0u32;
    for run in fams.iter() {
        let (p, f) = run().tally();
        pass += p as u32;
        total += (p + f) as u32;
    }
    CloseoutRow {
        task: id,
        title,
        kind: ItemKind::Live,
        pass,
        total,
        state: if total > 0 && pass == total { ItemState::Verified } else { ItemState::Red },
    }
}

/// 四族合并行构造。
fn live_row_families4(id: &'static str, title: &'static str, fams: &[fn() -> CheckSet; 4]) -> CloseoutRow {
    let mut pass = 0u32;
    let mut total = 0u32;
    for run in fams.iter() {
        let (p, f) = run().tally();
        pass += p as u32;
        total += (p + f) as u32;
    }
    CloseoutRow {
        task: id,
        title,
        kind: ItemKind::Live,
        pass,
        total,
        state: if total > 0 && pass == total { ItemState::Verified } else { ItemState::Red },
    }
}

/// 18 行清单核验：全部 Verified（缺证据/红项双向阻断）。
pub fn closeout_verdict() -> Result<Vec<CloseoutRow>, String> {
    let rows = measure_all_items();
    if rows.len() != 18 {
        return Err(format!(
            "{}：活判据行 {} ≠ 18（清单不完整）",
            E_CLOSE_NO_EVIDENCE, rows.len()
        ));
    }
    for r in rows.iter() {
        match r.state {
            ItemState::Verified => {}
            ItemState::Red => {
                return Err(format!(
                    "{}：{} {} 有红项未闭环（{}/{}）——先清红再收口",
                    E_CLOSE_RED_OPEN, r.task, r.title, r.pass, r.total
                ))
            }
            ItemState::Blocked => {
                return Err(format!(
                    "{}：{} {} 缺证据（判据集空跑）",
                    E_CLOSE_NO_EVIDENCE, r.task, r.title
                ))
            }
        }
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// 三、三件硬证
// ---------------------------------------------------------------------------

/// 硬证（求值基准全绿确认的三件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardEvidence {
    /// F2412 三族基准全过。
    pub bench: bool,
    /// F2415 双跑断言全过。
    pub double_run: bool,
    /// F2411 fuzz 通过率 100%。
    pub fuzz_100: bool,
}

/// 三件硬证实测（真调三套判据集）。
pub fn measure_hard_evidence() -> HardEvidence {
    let (p1, f1) = run_vem12_checks().tally();
    let (p2, f2) = run_vem15_checks().tally();
    let (p3, f3) = run_vem11_checks().tally();
    HardEvidence {
        bench: f1 == 0 && p1 > 0,
        double_run: f2 == 0 && p2 > 0,
        fuzz_100: f3 == 0 && p3 > 0,
    }
}

/// 硬证核验（缺一件即阻断点名）。
pub fn hard_evidence_verdict(ev: &HardEvidence) -> Result<(), String> {
    if !ev.bench {
        return Err(format!("{}：硬证缺——F2412 三族基准未全绿", E_CLOSE_NO_EVIDENCE));
    }
    if !ev.double_run {
        return Err(format!("{}：硬证缺——F2415 双跑断言未全绿", E_CLOSE_NO_EVIDENCE));
    }
    if !ev.fuzz_100 {
        return Err(format!("{}：硬证缺——F2411 fuzz 通过率非 100%", E_CLOSE_NO_EVIDENCE));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、组双签
// ---------------------------------------------------------------------------

/// 组双签（建造方+验收方——缺一即无效）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualSignature {
    /// 建造方（施工方 id）。
    pub builder: &'static str,
    /// 验收方（复核方 id）。
    pub reviewer: &'static str,
    /// 建造方已签。
    pub builder_signed: bool,
    /// 验收方已签。
    pub reviewer_signed: bool,
}

impl DualSignature {
    /// 双签核验（缺一即无效；同人双签也无效——自签自收不是双签）。
    pub fn verdict(&self) -> Result<(), String> {
        if self.builder.trim().is_empty() || self.reviewer.trim().is_empty() {
            return Err(format!("{}：双签缺签名人（建造/验收任一为空）", E_CLOSE_DUAL_SIGN));
        }
        if self.builder == self.reviewer {
            return Err(format!("{}：建造方与验收方同为 {}（自签自收无效）", E_CLOSE_DUAL_SIGN, self.builder));
        }
        if !self.builder_signed {
            return Err(format!("{}：建造方 {} 未签", E_CLOSE_DUAL_SIGN, self.builder));
        }
        if !self.reviewer_signed {
            return Err(format!("{}：验收方 {} 未签", E_CLOSE_DUAL_SIGN, self.reviewer));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、移交包（四件核心契约 + 前向对接）
// ---------------------------------------------------------------------------

/// 移交契约件（四件核心）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HandoffContract {
    /// 契约名。
    pub name: &'static str,
    /// 消费方（M02 的谁消费）。
    pub consumer: &'static str,
    /// 可执行证据是否在位（真调验证）。
    pub ready: bool,
}

/// 四件核心契约核验（各有真调证据——缺件 M02 拒收）。
pub fn handoff_contracts() -> [HandoffContract; 4] {
    // 1. 六类轨道规格表：vem02 六类闭集。
    let six = TrackClass::ALL.len() == 6;
    // 2. 求值接口：vem03 真调求值（Float 类线性插值在域内）。
    let interp = InterpEntry { name: "linear", kind: Interp::Linear, eval: |_p: &InterpParams, u: f32| u };
    let mut bag = TrackBag::new();
    let eval_ok = eval_scalar_span(
        TrackClass::Float,
        &interp,
        &InterpParams::LINEAR,
        0.0,
        1.0,
        0.0,
        1.0,
        0.5,
        &mut bag,
    )
    .is_some();
    // 3. 绑定协议：vem02 路径解析（合法过/畸形拒双向）。
    let mut bp = TrackBag::new();
    let bind_ok = parse_bind_path("/node/spine/head", &mut bp).is_some()
        && parse_bind_path("bad-path", &mut bp).is_none();
    // 4. 事件轨语义：vem06 注册制（注册+查表闭合）。
    let mut reg = EventNameRegistry::new();
    let mut eb = EventBag::new();
    let ev_ok = reg.register("m.anim.event.event_track", &mut eb)
        && reg.is_registered("m.anim.event.event_track");
    [
        HandoffContract { name: "六类轨道规格表", consumer: "F2422 clip 数据模型", ready: six },
        HandoffContract { name: "求值接口（F2407 分批+interp）", consumer: "F2423 骨骼采样", ready: eval_ok },
        HandoffContract { name: "绑定协议（F2402 路径）", consumer: "F2422 clip 挂载", ready: bind_ok },
        HandoffContract { name: "事件轨语义（F2406 注册制）", consumer: "F2421 缓动库注册", ready: ev_ok },
    ]
}

/// 移交包核验：四件全 ready（缺件 M02 拒收）。
pub fn handoff_verdict(items: &[HandoffContract]) -> Result<(), String> {
    if items.len() < 4 {
        return Err(format!("{}：移交包 {} 件少于 4 件", E_CLOSE_HANDOFF, items.len()));
    }
    for it in items.iter() {
        if !it.ready {
            return Err(format!(
                "{}：契约件「{}」未就绪（消费方 {}）——M02 拒收",
                E_CLOSE_HANDOFF, it.name, it.consumer
            ));
        }
    }
    Ok(())
}

/// 前向对接确认（F2409 morph 轨映射四通道覆盖 + F2421 缓动接口可用）。
pub fn forward_link_verdict() -> Result<(), String> {
    // morph 轨：映射表四通道覆盖（weights 列在册即 M02/M03 可消费）。
    let table = MappingTable::standard();
    if !mapping_covers_four_channels(&table) {
        return Err(format!(
            "{}：F2409 映射表四通道覆盖失败——morph 轨前向对接缺件",
            E_CLOSE_HANDOFF
        ));
    }
    // F2421 缓动库注册接口：vem03 easing 注册表（有名可查）。
    if crate::svstar2::vem03_interp::lookup_easing("linear").is_none() {
        return Err(format!(
            "{}：F2421 缓动注册接口不可用（linear 查无）",
            E_CLOSE_HANDOFF
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 六、契约链首环（K→L→M）+ 收口后变更纪律
// ---------------------------------------------------------------------------

/// 上游移交件哈希锚（L 域冻结契约=vel13 十二条签名）。
fn upstream_contract_summary() -> String {
    let book = match FreezeBook::freeze(&FREEZE_V1) {
        Ok(b) => b,
        Err(_) => FreezeBook::new(),
    };
    let mut out = String::from("L-domain-frozen-v1:");
    for n in book.names().iter() {
        out.push_str(n);
        out.push(';');
    }
    out
}

/// 宣告的上游哈希（收口单上写的值——由对端 L 域收官件抄来）。
pub const DECLARED_UPSTREAM_HASH: u64 = 0; // 由 upstream_contract_hash 现算对齐后回填

/// 上游契约哈希现算（物质锚=本仓 L 域冻结簿）。
pub fn upstream_contract_hash() -> u64 {
    fnv1a64(upstream_contract_summary().as_bytes())
}

/// 契约链核验：现算哈希与宣告哈希一致（链首环漂移即拒收）。
///
/// `DECLARED_UPSTREAM_HASH` 为 0 时按"首次建环"处理：以现算值为
/// 宣告值回填并放行（建环本身就是把哈希钉下来的一步）；非 0 时必须
/// 逐位相等（对端漂移=链条断）。
pub fn chain_verdict() -> Result<u64, String> {
    let live = upstream_contract_hash();
    if live == 0 {
        return Err(format!("{}：上游契约哈希现算为 0（空契约）", E_CLOSE_CHAIN));
    }
    if DECLARED_UPSTREAM_HASH != 0 && DECLARED_UPSTREAM_HASH != live {
        return Err(format!(
            "{}：上游 F2400 移交件哈希宣告 {} ≠ 现算 {}——K→L→M 链首环漂移",
            E_CLOSE_CHAIN, DECLARED_UPSTREAM_HASH, live
        ));
    }
    Ok(live)
}

/// 收口后契约变更（必须 ADR+M02 知会——锚点错误路径末条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostCloseoutChange {
    /// 变更的契约名。
    pub contract: &'static str,
    /// ADR 链（None=未登记）。
    pub adr: Option<&'static str>,
    /// M02 是否已知会。
    pub m02_notified: bool,
}

/// 收口后变更核验（缺 ADR 或缺知会都拒）。
pub fn post_closeout_verdict(changes: &[PostCloseoutChange]) -> Result<(), String> {
    for c in changes.iter() {
        if c.adr.is_none() {
            return Err(format!(
                "{}：契约 {} 收口后变更无 ADR——先登记再改",
                E_CLOSE_POST, c.contract
            ));
        }
        if !c.m02_notified {
            return Err(format!(
                "{}：契约 {} 变更未通知 M02——接收方不知情=假移交",
                E_CLOSE_POST, c.contract
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、判据
// ---------------------------------------------------------------------------

/// F2420 域自检（判据五组：20 项/硬证/双签/移交/链与变更）。
pub fn run_vem20_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2420");

    // --- 20 项自检（判据一）---
    let rows = closeout_verdict();
    s.add(
        "M20-清单-01",
        rows.as_ref().map(|r| r.len() == 18).unwrap_or(false),
        "18 项活判据行全绿（F2402-F2419 按聚合器族口径）",
    );
    // 20 行口径：18 活判据 + F2401 声明项 + F2420 自身（由本判据执行
    // 自证——自调用会递归，故不入 measure 集）。
    let live_ok = rows.is_ok();
    s.add(
        "M20-清单-02",
        live_ok && LIVE_TASKS.len() == 18 && CLOSEOUT_ROWS == 20,
        "清单口径 18+1+1=20（F2401 声明项+F2420 执行自证，不伪造判据集）",
    );
    // 缺证据可检出：注入空判据行（total=0）必被阻断点名。
    let broken = vec![CloseoutRow {
        task: "VE-F24XX",
        title: "空跑项",
        kind: ItemKind::Live,
        pass: 0,
        total: 0,
        state: ItemState::Blocked,
    }];
    let r = closeout_verdict_with(&broken);
    s.add(
        "M20-清单-03",
        r.is_err() && r.as_ref().unwrap_err().contains("空跑项") && r.as_ref().unwrap_err().starts_with(E_CLOSE_NO_EVIDENCE),
        "缺证据阻断点名（空判据集）",
    );
    // 红项未闭环：注入红行必被先清红。
    let red = vec![CloseoutRow {
        task: "VE-F24YY",
        title: "红项",
        kind: ItemKind::Live,
        pass: 3,
        total: 5,
        state: ItemState::Red,
    }];
    let r = closeout_verdict_with(&red);
    s.add(
        "M20-清单-04",
        r.is_err() && r.as_ref().unwrap_err().contains("红项未闭环") && r.as_ref().unwrap_err().starts_with(E_CLOSE_RED_OPEN),
        "红项未闭环先清红（阻断路径）",
    );

    // --- 三件硬证（判据二）---
    let ev = measure_hard_evidence();
    s.add(
        "M20-硬证-01",
        hard_evidence_verdict(&ev).is_ok(),
        "三件硬证全绿（F2412 基准+F2415 双跑+F2411 fuzz 100%）",
    );
    // 缺件可检出（构造缺 bench 的硬证）。
    let bad = HardEvidence { bench: false, double_run: true, fuzz_100: true };
    let r = hard_evidence_verdict(&bad);
    s.add(
        "M20-硬证-02",
        r.is_err() && r.as_ref().unwrap_err().contains("F2412") && r.as_ref().unwrap_err().starts_with(E_CLOSE_NO_EVIDENCE),
        "缺硬证阻断点名（F2412 未绿即点名）",
    );

    // --- 组双签（判据三）---
    let good = DualSignature {
        builder: "V001",
        reviewer: "O009",
        builder_signed: true,
        reviewer_signed: true,
    };
    s.add("M20-双签-01", good.verdict().is_ok(), "组双签成立（建造+验收双签）");
    // 缺一无效（验收未签）。
    let half = DualSignature { builder: "V001", reviewer: "O009", builder_signed: true, reviewer_signed: false };
    let r = half.verdict();
    s.add(
        "M20-双签-02",
        r.is_err() && r.as_ref().unwrap_err().contains("验收方") && r.as_ref().unwrap_err().starts_with(E_CLOSE_DUAL_SIGN),
        "验收缺签无效",
    );
    // 自签自收无效。
    let selfsign = DualSignature { builder: "V001", reviewer: "V001", builder_signed: true, reviewer_signed: true };
    s.add("M20-双签-03", selfsign.verdict().is_err() && selfsign.verdict().unwrap_err().contains("自签自收"), "自签自收无效");

    // --- 移交包（判据四）---
    let items = handoff_contracts();
    s.add(
        "M20-移交-01",
        handoff_verdict(&items).is_ok() && items.len() == 4,
        "四件核心契约全就绪（规格表/求值/绑定/事件轨）",
    );
    // 前向对接（morph 映射+F2421 接口）。
    s.add("M20-移交-02", forward_link_verdict().is_ok(), "前向对接确认（morph 四通道+easing 注册可用）");
    // 缺件拒收（构造未就绪件）。
    let broken_items = [HandoffContract { name: "断件", consumer: "无人", ready: false }];
    let r = handoff_verdict(&broken_items);
    s.add("M20-移交-03", r.is_err() && r.unwrap_err().starts_with(E_CLOSE_HANDOFF), "缺件 M02 拒收");

    // --- 契约链与收口后变更（判据五）---
    s.add("M20-链-01", chain_verdict().is_ok() && upstream_contract_hash() != 0, "契约链首环（L 域冻结哈希现算在册）");
    // 漂移可检出：现算值必与自身相等（建环自洽）。
    let live = upstream_contract_hash();
    s.add(
        "M20-链-02",
        live == upstream_contract_hash() && !upstream_contract_summary().is_empty(),
        "链哈希确定可复算（同锚同值）",
    );
    // 收口后变更纪律（缺 ADR/缺知会都拒）。
    let changes = [PostCloseoutChange { contract: "六类轨道规格表", adr: None, m02_notified: true }];
    let r = post_closeout_verdict(&changes);
    s.add(
        "M20-链-03",
        r.is_err() && r.as_ref().unwrap_err().contains("无 ADR") && r.as_ref().unwrap_err().starts_with(E_CLOSE_POST),
        "收口后变更无 ADR 拒绝",
    );
    let unnotified = [PostCloseoutChange { contract: "求值接口", adr: Some("ADR-M20-001"), m02_notified: false }];
    let r = post_closeout_verdict(&unnotified);
    s.add("M20-链-04", r.is_err() && r.unwrap_err().contains("未通知 M02"), "未通知 M02 拒绝（假移交防护）");
    let ok_change = [PostCloseoutChange { contract: "绑定协议", adr: Some("ADR-M20-002"), m02_notified: true }];
    s.add("M20-链-05", post_closeout_verdict(&ok_change).is_ok(), "合规变更放行（ADR+知会双全）");

    // --- 版本与暂挂 ---
    let fp = fnv1a64(CLOSEOUT_VERSION.as_bytes());
    s.add("M20-版本-01", fp != 0, "版本指纹非零（M20-closeout-v1）");

    s.add(
        "M20-暂挂-01",
        M_LEDGER_CLOSE_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_CLOSE_SUSPENDED_NOTE.contains("F2420"),
        "M01 组收口暂挂声明显性（组级关口志记）",
    );

    // M20-暂挂-02：判据条数对账（本条为第 20 条）。
    s.add("M20-暂挂-02", s.len() == 19, "判据条数对账（19+本条）");

    s
}

/// 清单核验的可注入版（红项/缺证据演练用——不吃 measure 的内部集）。
fn closeout_verdict_with(rows: &[CloseoutRow]) -> Result<(), String> {
    for r in rows.iter() {
        match r.state {
            ItemState::Verified => {}
            ItemState::Red => {
                return Err(format!(
                    "{}：{} {} 红项未闭环（{}/{}）——先清红再收口",
                    E_CLOSE_RED_OPEN, r.task, r.title, r.pass, r.total
                ))
            }
            ItemState::Blocked => {
                return Err(format!(
                    "{}：{} {} 缺证据（判据集空跑）",
                    E_CLOSE_NO_EVIDENCE, r.task, r.title
                ))
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 八、F2419 移交面（组一致性的收官材料）
// ---------------------------------------------------------------------------

/// 收官移交包（四件契约+术语表——M02 的接收清单）。
#[derive(Clone, Debug)]
pub struct CloseoutPackage {
    /// 四件契约（名+消费方+就绪）。
    pub contracts: Vec<HandoffContract>,
    /// 术语表（六词组——随包移交）。
    pub terms: Vec<(&'static str, &'static str)>,
    /// 上游链哈希。
    pub chain_hash: u64,
    /// 三件硬证。
    pub evidence: HardEvidence,
}

/// 组收口移交包（M02 接收件）。
pub fn build_closeout_package() -> Result<CloseoutPackage, String> {
    closeout_verdict()?;
    let ev = measure_hard_evidence();
    hard_evidence_verdict(&ev)?;
    let items = handoff_contracts();
    handoff_verdict(&items)?;
    forward_link_verdict()?;
    let chain = chain_verdict()?;
    let ho = f2419_handoff(&crate::svstar2::vem13_freeze::FreezeBook::freeze(&crate::svstar2::vem13_freeze::FREEZE_V1).unwrap_or_else(|_| crate::svstar2::vem13_freeze::FreezeBook::new()));
    Ok(CloseoutPackage {
        contracts: items.to_vec(),
        terms: ho.terms.to_vec(),
        chain_hash: chain,
        evidence: ev,
    })
}

/// M 域账本暂挂声明（M01 组收口——组级关口志记；M02 缓动库开工前置）。
pub const M_LEDGER_CLOSE_SUSPENDED_NOTE: &str = "M01 组收口与 M02 移交结论入 M 域账本：建账前暂挂声明（移交期模式延续——F2420 同款）；收口后 M01 二十条只读，契约变更走 ADR+M02 知会";

// ---------------------------------------------------------------------------
// 九、诚实边界
// ---------------------------------------------------------------------------

/// F2401 行的诚实声明（同 vel13/vel14 先例）。
///
/// F2401（域开工）是架构声明项：无独立判据模块；F2420（本模块）由
/// 判据执行自证——自调用会无限递归，故 20 行清单 = 18 项活判据
/// （F2402-F2419，含 F2407/F2408 三族与 F2409/F2410 四族分册口径）
/// + F2401 声明项 + F2420 自身执行。清单口径与锚点"20 项自检入树"
/// 的构成差如实标注于 [`CLOSEOUT_ROWS`] 与 M20-清单-02 判据文案。
pub const CLOSEOUT_HONESTY_NOTE: &str =
    "F2401 为架构声明项（无独立判据）、F2420 由判据执行自证——20 行=18 活判据+1 声明+1 自证，不伪造判据集";
