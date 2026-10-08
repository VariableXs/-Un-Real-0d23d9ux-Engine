//! VE-F1620 · I01 组收口与 I02 移交（VE-I 域 · I01 批次收口 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1620`
//!
//! **判据（锚点原文）**：入树、校验报告、双签、移交、判据。
//!
//! **职责定位（锚点原文）**：20 项自检入树（台账三态核对：全部样板完成）/
//! 几何校验全绿报告（F1612 三查+fuzz 全绿归档）/组双签/台账更新/向 I02
//! 顶点流水线组移交（几何底座就绪——数据结构先于流水线，移交清单两项带
//! 哈希）。收口六段体例（接口/主题/契约/测试账/性能账/遗留+双签）——
//! I01 组 19 项接口冻结清单在册。
//!
//! ## 一、收口是「证据入树」不是「宣布完成」
//!
//! 20 项自检逐条入树（[`SelfCheckTree`]），每条台账**三态核对**——完成 /
//! 遗留 / 移交，只许三态之一，「大概完成」不是状态；全部样板完成才满足
//! 收口前置（锚点：台账三态核对：全部样板完成）。遗留必须带去向（转
//! 移交或立追补），无去向的遗留=暗债。
//!
//! ## 二、校验报告归档：三查全绿 + fuzz 全绿是移交前置
//!
//! F1612 恶意网格三查（索引越界 / NaN 几何 / 超大属性值）逐查归档
//! （[`CheckReport`]），fuzz 全绿归档（[`CheckReport::fuzz_green`]）——
//! 校验不绿不移交（几何底座带着已知洞移交，I02 的流水线会在洞上建楼）。
//!
//! ## 三、19 项接口冻结清单 + 移交两项带哈希
//!
//! 接口冻结清单逐项登记（[`FREEZE_TABLE`]，名+来源单），清单指纹 FNV
//! 独立可重算；移交两项（vmesh 格式规范 / 几何能力面）各带内容哈希
//! （[`HandoffManifest`]）——I02 收到的底座可哈希对账，"大概就绪"不可验收。
//!
//! ## 四、六段体例 + 双签
//!
//! 收口册六段（接口/主题/契约/测试账/性能账/遗留）逐段齐备，段缺即
//! 不收口；双签（[`DualSign`]）author+reviewer 两签齐且互异才算收口
//! ——单人自签是收口撒谎。
//!
//! **对接**：上游 I01 全组（F1611~F1620）；下游 I02 顶点流水线组
//! （F1621~，在 vmesh 数据结构上运行）。零 panic 面、零 IO、零墙钟、
//! 无全局可变状态、no_std 零 std 依赖。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// I01 组自检入树条数。
pub const SELFCHECK_COUNT: usize = 20;

/// I01 组接口冻结清单条数。
pub const FREEZE_COUNT: usize = 19;

/// 收口六段体例段数。
pub const SECTION_COUNT: usize = 6;

/// 移交清单条数（两项带哈希）。
pub const HANDOFF_COUNT: usize = 2;

/// F1612 恶意网格三查名（索引越界/NaN 几何/超大属性值）。
pub const THREE_CHECKS: [&str; 3] = ["索引越界", "NaN 几何", "超大属性值"];

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x40xx 段；0x3Fxx 归 F6002）
// ---------------------------------------------------------------------------

/// F1620 诊断码。独占 `0x40xx` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseoutCode(pub u16);

impl CloseoutCode {
    /// 自检树条目状态非法（三态之外）。
    pub const BAD_STATE: CloseoutCode = CloseoutCode(0x4001);
    /// 自检树条目缺（不足 20 项）。
    pub const TREE_SHORT: CloseoutCode = CloseoutCode(0x4002);
    /// 校验报告不绿（三查或 fuzz 有红）。
    pub const REPORT_RED: CloseoutCode = CloseoutCode(0x4003);
    /// 六段体例段缺。
    pub const SECTION_MISSING: CloseoutCode = CloseoutCode(0x4004);
    /// 双签缺失或同签。
    pub const SIGN_BAD: CloseoutCode = CloseoutCode(0x4005);
    /// 移交清单哈希对账失败。
    pub const HASH_MISMATCH: CloseoutCode = CloseoutCode(0x4006);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn reason(self) -> String {
        match self {
            CloseoutCode::BAD_STATE => "自检条目状态非法：只许 完成/遗留/移交 三态".into(),
            CloseoutCode::TREE_SHORT => "自检树条目缺：20 项未入齐".into(),
            CloseoutCode::REPORT_RED => "校验报告不绿：三查或 fuzz 有红不移交".into(),
            CloseoutCode::SECTION_MISSING => "六段体例段缺：段不齐不收口".into(),
            CloseoutCode::SIGN_BAD => "双签缺失或同签：单人自签是收口撒谎".into(),
            CloseoutCode::HASH_MISMATCH => "移交清单哈希对账失败：底座不可验收".into(),
            CloseoutCode(_) => "未知收口诊断码".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、自检树 / 校验报告 / 冻结清单 / 移交清单（收口数据结构）
// ---------------------------------------------------------------------------

/// 台账三态（锚点：台账三态核对——只许三态之一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerState {
    /// 完成。
    Done,
    /// 遗留（必须带去向：转移交或立追补）。
    Legacy,
    /// 移交（随组收口移交 I02）。
    Handover,
}

/// 自检树条目：名 × 三态 × 去向说明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfCheckEntry {
    /// 条目名。
    pub name: String,
    /// 台账三态。
    pub state: LedgerState,
    /// 去向说明（Legacy 态必填非空——无去向的遗留=暗债）。
    pub dest: String,
}

/// 自检树（20 项入树；样板三态核对）。
#[derive(Debug, Default)]
pub struct SelfCheckTree {
    entries: Vec<SelfCheckEntry>,
}

impl SelfCheckTree {
    /// 空树。
    pub fn new() -> Self {
        SelfCheckTree {
            entries: Vec::new(),
        }
    }

    /// 入树（状态非法/重复名拒绝；Legacy 无去向拒绝）。
    pub fn add(&mut self, name: &str, state: LedgerState, dest: &str) -> Result<(), CloseoutCode> {
        if self.entries.iter().any(|e| e.name == name) {
            return Err(CloseoutCode::BAD_STATE);
        }
        if state == LedgerState::Legacy && dest.is_empty() {
            // 无去向的遗留=暗债：入树即拦
            return Err(CloseoutCode::BAD_STATE);
        }
        self.entries.push(SelfCheckEntry {
            name: name.into(),
            state,
            dest: dest.into(),
        });
        Ok(())
    }

    /// 入树条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空树判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 三态样板核对：条目齐 20 且全部三态内（类型系统保证三态，此处对账
    /// 数量与 Legacy 去向非空——全部样板完成才满足收口前置）。
    pub fn all_sampled(&self) -> bool {
        self.entries.len() == SELFCHECK_COUNT
            && self
                .entries
                .iter()
                .all(|e| !(e.state == LedgerState::Legacy && e.dest.is_empty()))
    }

    /// 三态分布计数（台账呈现）。
    pub fn state_counts(&self) -> (usize, usize, usize) {
        let mut d = 0;
        let mut l = 0;
        let mut h = 0;
        for e in &self.entries {
            match e.state {
                LedgerState::Done => d += 1,
                LedgerState::Legacy => l += 1,
                LedgerState::Handover => h += 1,
            }
        }
        (d, l, h)
    }
}

/// 校验报告（F1612 三查逐查归档 + fuzz 全绿）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckReport {
    /// 三查结果（与 [`THREE_CHECKS`] 一一对应，true=绿）。
    pub checks: [bool; 3],
    /// fuzz 全绿归档。
    pub fuzz_green: bool,
}

impl CheckReport {
    /// 全绿判定（三查全绿 + fuzz 绿——校验不绿不移交）。
    pub const fn all_green(&self) -> bool {
        self.checks[0] && self.checks[1] && self.checks[2] && self.fuzz_green
    }
}

/// 冻结接口条目（名 × 来源单号）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreezeEntry {
    /// 接口名。
    pub name: &'static str,
    /// 来源单（I01 组内单号）。
    pub from: &'static str,
}

/// I01 组 19 项接口冻结清单（表驱动；指纹可独立重算）。
pub const FREEZE_TABLE: [FreezeEntry; FREEZE_COUNT] = [
    FreezeEntry { name: "vmesh_header_schema", from: "F1620" },
    FreezeEntry { name: "vmesh_vertex_layout", from: "F1620" },
    FreezeEntry { name: "vmesh_face_layout", from: "F1620" },
    FreezeEntry { name: "vmesh_attr_layout", from: "F1620" },
    FreezeEntry { name: "vmesh_quant_policy", from: "F1620" },
    FreezeEntry { name: "mesh_validate_schema", from: "F1612" },
    FreezeEntry { name: "mesh_check_index_oob", from: "F1612" },
    FreezeEntry { name: "mesh_check_nan_geom", from: "F1612" },
    FreezeEntry { name: "mesh_check_huge_attr", from: "F1612" },
    FreezeEntry { name: "mesh_repair_policy", from: "F1612" },
    FreezeEntry { name: "mesh_stat_grid", from: "F1616" },
    FreezeEntry { name: "tool_contract_gate", from: "F1617" },
    FreezeEntry { name: "geofuzz_harness", from: "F1618" },
    FreezeEntry { name: "apifreeze_signatures", from: "F1618" },
    FreezeEntry { name: "geobench_loader", from: "F1619" },
    FreezeEntry { name: "geobench_quant", from: "F1619" },
    FreezeEntry { name: "geobench_simplify", from: "F1619" },
    FreezeEntry { name: "meshspec_spec_doc", from: "F1620" },
    FreezeEntry { name: "consistency_audit", from: "F1620" },
];

/// FNV-1a 64（指纹哈希；与全仓指纹口径同族，独立可重算）。
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 移交条目（名 × 内容哈希）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandoffItem {
    /// 移交名。
    pub name: &'static str,
    /// 内容哈希（FNV-1a，I02 收货对账用）。
    pub hash: u64,
}

/// 移交清单（两项带哈希：vmesh 格式规范 / 几何能力面）。
pub fn build_handoff() -> [HandoffItem; HANDOFF_COUNT] {
    // 项一：vmesh 格式规范指纹（头/顶点/面字节口径组合）
    let spec = alloc::format!(
        "vmesh:{}:{}:{}",
        super::vef20_meshspec::VMESH_HEADER_BYTES,
        super::vef20_meshspec::VMESH_VERT_BYTES,
        super::vef20_meshspec::VMESH_FACE_BYTES
    );
    let item1 = HandoffItem {
        name: "vmesh_format_spec",
        hash: fnv1a(spec.as_bytes()),
    };
    // 项二：几何能力面指纹（19 项冻结接口名列表）
    let mut caps = Vec::new();
    for f in FREEZE_TABLE.iter() {
        caps.extend_from_slice(f.name.as_bytes());
        caps.push(0x1F);
    }
    let item2 = HandoffItem {
        name: "geometry_capability_surface",
        hash: fnv1a(&caps),
    };
    [item1, item2]
}

/// 双签（author+reviewer 两签齐且互异）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DualSign {
    /// 作者签。
    pub author: String,
    /// 复核签。
    pub reviewer: String,
}

impl DualSign {
    /// 双签有效判定：两签非空且互异。
    pub fn valid(&self) -> bool {
        !self.author.is_empty() && !self.reviewer.is_empty() && self.author != self.reviewer
    }
}

// ---------------------------------------------------------------------------
// 四、CloseoutLedger 主结构（收口六段体例 + close 判定）
// ---------------------------------------------------------------------------

/// 收口六段体例（接口/主题/契约/测试账/性能账/遗留）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    /// 接口段。
    Interfaces,
    /// 主题段。
    Themes,
    /// 契约段。
    Contracts,
    /// 测试账段。
    TestLedger,
    /// 性能账段。
    PerfLedger,
    /// 遗留段。
    Legacy,
}

impl Section {
    /// 全集（顺序即体例序）。
    pub const fn all() -> [Section; SECTION_COUNT] {
        [
            Section::Interfaces,
            Section::Themes,
            Section::Contracts,
            Section::TestLedger,
            Section::PerfLedger,
            Section::Legacy,
        ]
    }

    /// 段名（读屏可达）。
    pub const fn label(self) -> &'static str {
        match self {
            Section::Interfaces => "接口",
            Section::Themes => "主题",
            Section::Contracts => "契约",
            Section::TestLedger => "测试账",
            Section::PerfLedger => "性能账",
            Section::Legacy => "遗留",
        }
    }
}

/// 收口册：六段 × 自检树 × 校验报告 × 双签 × 移交清单。
#[derive(Debug, Default)]
pub struct CloseoutLedger {
    sections: [bool; SECTION_COUNT],
    tree: SelfCheckTree,
    report: Option<CheckReport>,
    sign: Option<DualSign>,
    handoff_verified: bool,
}

impl CloseoutLedger {
    /// 空册。
    pub fn new() -> Self {
        CloseoutLedger {
            sections: [false; SECTION_COUNT],
            tree: SelfCheckTree::new(),
            report: None,
            sign: None,
            handoff_verified: false,
        }
    }

    /// 段落归档（逐段入册）。
    pub fn file_section(&mut self, sec: Section) {
        self.sections[sec as usize] = true;
    }

    /// 段落齐备性（六段逐段对账）。
    pub fn sections_complete(&self) -> bool {
        Section::all().iter().all(|s| self.sections[*s as usize])
    }

    /// 自检树只读视图。
    pub const fn tree(&self) -> &SelfCheckTree {
        &self.tree
    }

    /// 校验报告归档。
    pub fn file_report(&mut self, r: CheckReport) {
        self.report = Some(r);
    }

    /// 双签归档。
    pub fn file_sign(&mut self, s: DualSign) {
        self.sign = Some(s);
    }

    /// 移交哈希对账（两项重算比对；对过才置位）。
    pub fn verify_handoff(&mut self, expect: &[HandoffItem; HANDOFF_COUNT]) -> Result<(), CloseoutCode> {
        let actual = build_handoff();
        for (a, e) in actual.iter().zip(expect.iter()) {
            if a.hash != e.hash || a.name != e.name {
                return Err(CloseoutCode::HASH_MISMATCH);
            }
        }
        self.handoff_verified = true;
        Ok(())
    }

    /// 收口判定：六段齐 + 自检 20 项全样板 + 报告全绿 + 双签有效 + 移交对账。
    pub fn close(&self) -> Result<(), CloseoutCode> {
        if !self.sections_complete() {
            return Err(CloseoutCode::SECTION_MISSING);
        }
        if !self.tree.all_sampled() {
            return Err(CloseoutCode::TREE_SHORT);
        }
        match self.report {
            Some(r) if r.all_green() => {}
            _ => return Err(CloseoutCode::REPORT_RED),
        }
        match &self.sign {
            Some(sg) if sg.valid() => {}
            _ => return Err(CloseoutCode::SIGN_BAD),
        }
        if !self.handoff_verified {
            return Err(CloseoutCode::HASH_MISMATCH);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、域自检（判据：入树、校验报告、双签、移交、判据）
// ---------------------------------------------------------------------------

/// VE-F1620 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vef23_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vef23_closeout");

    // —— 判据一 · 入树：20 项三态入树，样板核对 ——
    let mut t = SelfCheckTree::new();
    let mut added = 0usize;
    for i in 0..SELFCHECK_COUNT {
        let name = alloc::format!("check-{}", i);
        let state = if i % 10 == 9 { LedgerState::Legacy } else { LedgerState::Done };
        let dest = if state == LedgerState::Legacy { "转 F1621 移交" } else { "" };
        if t.add(&name, state, dest).is_ok() {
            added += 1;
        }
    }
    let (d, l, h) = t.state_counts();
    let mut t2 = SelfCheckTree::new();
    let dark_debt = t2.add("遗留项", LedgerState::Legacy, "");
    s.add(
        "F1620-入树-三态样板与暗债拦截",
        added == SELFCHECK_COUNT
            && t.all_sampled()
            && d == 18 && l == 2 && h == 0
            && dark_debt == Err(CloseoutCode::BAD_STATE),
        "20 项入树三态分布对账；Legacy 无去向=暗债入树即拦（全部样板完成才收口）",
    );

    // —— 判据一 · 反向：树缺条目 close 拒绝 ——
    let led0 = CloseoutLedger::new();
    let short_close = led0.close();
    s.add(
        "F1620-入树-缺条目拒收口",
        short_close == Err(CloseoutCode::SECTION_MISSING),
        "空册 close 首拒段缺（六段体例前置）；树账在段齐后才轮到被核",
    );

    // —— 判据二 · 校验报告：三查+fuzz 全绿才放行 ——
    let green = CheckReport { checks: [true, true, true], fuzz_green: true };
    let red_nan = CheckReport { checks: [true, false, true], fuzz_green: true };
    let red_fuzz = CheckReport { checks: [true, true, true], fuzz_green: false };
    s.add(
        "F1620-校验报告-三查fuzz全绿",
        green.all_green() && !red_nan.all_green() && !red_fuzz.all_green(),
        "索引越界/NaN 几何/超大属性值三查与 fuzz 任一红即不放行（不绿不移交）",
    );

    // —— 判据二 · 反向：三查名与归档口径逐一对账 ——
    let names_match = THREE_CHECKS == ["索引越界", "NaN 几何", "超大属性值"];
    s.add(
        "F1620-校验报告-三查名对账",
        names_match && THREE_CHECKS.len() == 3,
        "三查名逐字同源 F1612 锚点（索引越界/NaN 几何/超大属性值）",
    );

    // —— 判据三 · 双签：两签齐且互异 ——
    let good = DualSign { author: "W011".into(), reviewer: "W007".into() };
    let self_sign = DualSign { author: "W011".into(), reviewer: "W011".into() };
    let empty = DualSign { author: "W011".into(), reviewer: String::new() };
    s.add(
        "F1620-双签-两签齐且互异",
        good.valid() && !self_sign.valid() && !empty.valid(),
        "author+reviewer 两签齐且互异；单人自签与缺签都拒（收口撒谎拦截）",
    );

    // —— 判据三 · 反向：冻结清单 19 项指纹可独立重算 ——
    let caps: Vec<u8> = FREEZE_TABLE
        .iter()
        .flat_map(|f| {
            let mut v = f.name.as_bytes().to_vec();
            v.push(0x1F);
            v
        })
        .collect();
    let expect_item2 = fnv1a(&caps);
    let handoff = build_handoff();
    s.add(
        "F1620-移交-19项指纹独立重算",
        FREEZE_TABLE.len() == FREEZE_COUNT
            && handoff[1].hash == expect_item2
            && FREEZE_TABLE.iter().all(|f| f.from.starts_with("F16")),
        "冻结清单 19 项全 I01 组来源；能力面哈希独立重算相等（指纹可验非口头）",
    );

    // —— 判据四 · 移交：两项哈希对账通过/篡改即拒 ——
    let mut led = CloseoutLedger::new();
    for sec in Section::all() {
        led.file_section(sec);
    }
    for i in 0..SELFCHECK_COUNT {
        let name = alloc::format!("item-{}", i);
        let _ = led.tree.add(&name, LedgerState::Done, "");
    }
    led.file_report(green);
    led.file_sign(good.clone());
    let ok_verify = led.verify_handoff(&build_handoff());
    let tampered = [
        HandoffItem { name: "vmesh_format_spec", hash: handoff[0].hash },
        HandoffItem { name: "geometry_capability_surface", hash: handoff[1].hash ^ 1 },
    ];
    let mut led2 = CloseoutLedger::new();
    let tamper_reject = led2.verify_handoff(&tampered);
    s.add(
        "F1620-移交-哈希对账篡改即拒",
        ok_verify.is_ok()
            && tamper_reject == Err(CloseoutCode::HASH_MISMATCH),
        "两项移交哈希重算比对通过；哈希被篡一位即 HASH_MISMATCH（底座可验收）",
    );

    // —— 判据四 · 收口全流程：齐备 close 通过、逐缺项拒绝 ——
    let full_close = led.close();
    let mut led3 = CloseoutLedger::new();
    for sec in Section::all() {
        led3.file_section(sec);
    }
    for i in 0..SELFCHECK_COUNT {
        let name = alloc::format!("item-{}", i);
        let _ = led3.tree.add(&name, LedgerState::Done, "");
    }
    led3.file_report(red_fuzz);
    led3.file_sign(good.clone());
    let _ = led3.verify_handoff(&build_handoff());
    let red_close = led3.close();
    let mut led4 = CloseoutLedger::new();
    for sec in Section::all() {
        led4.file_section(sec);
    }
    for i in 0..(SELFCHECK_COUNT - 1) {
        let name = alloc::format!("item-{}", i);
        let _ = led4.tree.add(&name, LedgerState::Done, "");
    }
    led4.file_report(green);
    led4.file_sign(good.clone());
    let _ = led4.verify_handoff(&build_handoff());
    let short_tree_close = led4.close();
    s.add(
        "F1620-收口-全流程逐门拒绝",
        full_close.is_ok()
            && red_close == Err(CloseoutCode::REPORT_RED)
            && short_tree_close == Err(CloseoutCode::TREE_SHORT),
        "五门全过才收口：段齐→树 20→报告绿→双签→移交对账；fuzz 红/树缺逐门拦",
    );

    // —— 判据五 · 元数据：码段互异 + 体例/清单口径 ——
    let codes = [
        CloseoutCode::BAD_STATE.code(),
        CloseoutCode::TREE_SHORT.code(),
        CloseoutCode::REPORT_RED.code(),
        CloseoutCode::SECTION_MISSING.code(),
        CloseoutCode::SIGN_BAD.code(),
        CloseoutCode::HASH_MISMATCH.code(),
    ];
    let mut uniq = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                uniq = false;
            }
        }
    }
    let sections_named = Section::all().iter().all(|x| !x.label().is_empty());
    s.add(
        "F1620-判据-码段互异且体例齐",
        uniq
            && codes.iter().all(|c| c & 0xFF00 == 0x4000)
            && sections_named
            && SECTION_COUNT == 6
            && HANDOFF_COUNT == 2,
        "六码全落 0x40xx 两两互异；六段体例段名齐；移交两项口径对账",
    );

    // —— 判据五 · 对账：vmesh 规范指纹三项字节口径单源 ——
    let h0 = handoff[0].hash;
    let h0_again = {
        let spec = alloc::format!(
            "vmesh:{}:{}:{}",
            super::vef20_meshspec::VMESH_HEADER_BYTES,
            super::vef20_meshspec::VMESH_VERT_BYTES,
            super::vef20_meshspec::VMESH_FACE_BYTES
        );
        fnv1a(spec.as_bytes())
    };
    s.add(
        "F1620-判据-vmesh指纹单源重算",
        h0 == h0_again && h0 != 0,
        "vmesh 规范指纹两次独立构造相等且非零（格式口径单源 ve f20_meshspec 常量）",
    );

    s
}
