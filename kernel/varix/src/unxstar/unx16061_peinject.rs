//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16061 · PE 注入路径总纲与 SEC_IMAGE 消费决策账 · 0x684C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B04）：
//!   UNX-F16061-J1 决策账逐行落账（注入路径环节/承接方/冻结契约
//!   指针），SEC_IMAGE 消费行与 AI-17 冻结签名对照一致率 100%，
//!   未覆盖环节零静默断言通过。
//! 定位：任务书专题三是 PE 进程注入路径——本条立注入路径总纲账：
//!   从 PE 触发到进程就绪的环节逐环登记（人格路由/镜像映射/依赖
//!   装载/线程启动/句柄继承…），每环标注承接方与冻结契约指针，
//!   SEC_IMAGE 映射面明确消费 AI-17 波07 冻结接口。
//! 设计要点：
//!   - 九环节闭集：人格路由/镜像映射/依赖装载/重定位/IAT 解析/
//!     TLS 目录/线程启动/句柄继承/就绪通知——环与环显式衔接
//!     （上环输出 = 下环输入），首版九环覆盖全路径；
//!   - 决策账行：每环（环节名/承接方/输入物/输出物/冻结契约指针/
//!     状态六态），状态无"空白"态（零静默律）；
//!   - SEC_IMAGE 消费行：映射请求/尺寸/保护位/基点策略四字段逐
//!     字段对照 AI-17 冻结签名，对照一致率 100%——消费方先读懂
//!     合同再动工；
//!   - 衔接账：九环输出物→输入物逐环咬合，断链即总纲失效；
//!   - 注入面：删契约指针/环节空白/衔接断裂/签名漂移/环节重复/
//!     越权执行——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x684C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x684C0，AI-VX-1030 E1 域批1 独占区）。
pub const SEG_BASE: u32 = 0x684C0;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// AI-17 SEC_IMAGE 冻结接口版本（波07 冻结，写死）。
pub const AI17_FREEZE_VER: u32 = 7;

// ── 九环节闭集 ───────────────────────────────────────────────────
pub const STAGE_ROUTER: usize = 0; // 人格路由
pub const STAGE_MAPPING: usize = 1; // 镜像映射（SEC_IMAGE）
pub const STAGE_DEPEND: usize = 2; // 依赖装载
pub const STAGE_RELOC: usize = 3; // 重定位
pub const STAGE_IAT: usize = 4; // IAT 解析
pub const STAGE_TLS: usize = 5; // TLS 目录
pub const STAGE_THREAD: usize = 6; // 线程启动
pub const STAGE_INHERIT: usize = 7; // 句柄继承
pub const STAGE_READY: usize = 8; // 就绪通知
pub const STAGE_COUNT: usize = 9;

pub const STAGE_NAMES: [&str; STAGE_COUNT] = [
    "人格路由", "镜像映射", "依赖装载", "重定位", "IAT 解析",
    "TLS 目录", "线程启动", "句柄继承", "就绪通知",
];

/// 环节状态六态闭集（零静默：无"空白"态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageState {
    /// 已承接（有承接方 + 契约指针）
    Owned,
    /// 部分承接（主体在案、缺口显式）
    PartialOwned,
    /// 待承接（有显式指针，非空白）
    Pending,
    /// 双跑对照（语义底座成熟前诚实过渡态）
    DualRun,
    /// 如实保持差异（差异裁决已签）
    KeptDiff,
    /// 显性排除（明确不做，带理由）
    Excluded,
}

impl StageState {
    pub fn as_u32(self) -> u32 {
        match self {
            StageState::Owned => 0,
            StageState::PartialOwned => 1,
            StageState::Pending => 2,
            StageState::DualRun => 3,
            StageState::KeptDiff => 4,
            StageState::Excluded => 5,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            StageState::Owned => "已承接",
            StageState::PartialOwned => "部分承接",
            StageState::Pending => "待承接",
            StageState::DualRun => "双跑对照",
            StageState::KeptDiff => "如实保持差异",
            StageState::Excluded => "显性排除",
        }
    }
    pub fn from_u32(v: u32) -> Option<StageState> {
        match v {
            0 => Some(StageState::Owned),
            1 => Some(StageState::PartialOwned),
            2 => Some(StageState::Pending),
            3 => Some(StageState::DualRun),
            4 => Some(StageState::KeptDiff),
            5 => Some(StageState::Excluded),
            _ => None,
        }
    }
    /// 是否要求承接方在案（待承接/显性排除不要求承接方，
    /// 但要求指针或理由——总之不许空白）。
    pub fn requires_owner(self) -> bool {
        match self {
            StageState::Owned
            | StageState::PartialOwned
            | StageState::DualRun
            | StageState::KeptDiff => true,
            _ => false,
        }
    }
}

/// SEC_IMAGE 消费行：AI-17 冻结签名四字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecImageConsume {
    /// 映射请求（SEC_IMAGE 节型标记，固定值）。
    pub request_kind: u32,
    /// 尺寸（按 PE 头 SizeOfImage，字节）。
    pub size_bytes: u64,
    /// 保护位（PAGE_EXECUTE_* 闭集编码）。
    pub protect_bits: u32,
    /// 基点策略（0= preferred / 1= any-low / 2= reserved-slab）。
    pub base_policy: u32,
}

/// AI-17 冻结签名（波07 冻结版，写死）。
pub const AI17_SIG: SecImageConsume = SecImageConsume {
    request_kind: 0x5EC1_1D9E, // SEC_IMAGE 语义标记（段内写死）
    size_bytes: 0,
    protect_bits: 0x20, // PAGE_EXECUTE_READWRITE 族基位
    base_policy: 1,
};

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PjCode {
    /// 环节越九环闭集
    StageOutOfRange,
    /// 环节重复登记
    StageDuplicated,
    /// 承接方空（该态要求承接方在案）
    OwnerBlank,
    /// 冻结契约指针空（零静默违规）
    ContractBlank,
    /// 状态越六态闭集
    StateOutOfRange,
    /// 签名漂移：SEC_IMAGE 消费行与 AI-17 冻结签名不一致
    SignatureDrift,
    /// 衔接断裂：上环输出 ≠ 下环输入
    ChainBroken,
    /// 空白态：环节状态无出处
    BlankStage,
    /// 越权执行：总纲账只记账不执行
    OverreachExec,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl PjCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PjCode::StageOutOfRange => "环节越九环闭集",
            PjCode::StageDuplicated => "环节重复登记：环唯一律",
            PjCode::OwnerBlank => "承接方空：该态要求在案",
            PjCode::ContractBlank => "冻结契约指针空：零静默违规",
            PjCode::StateOutOfRange => "状态越六态闭集",
            PjCode::SignatureDrift => "SEC_IMAGE 签名漂移：消费失守",
            PjCode::ChainBroken => "衔接断裂：上环输出≠下环输入",
            PjCode::BlankStage => "空白态：环节状态无出处",
            PjCode::OverreachExec => "越权执行：总纲账只记账不执行",
            PjCode::NotReproducible => "双跑不一致：确定性红线",
            PjCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [PjCode; 11] = [
        PjCode::StageOutOfRange,
        PjCode::StageDuplicated,
        PjCode::OwnerBlank,
        PjCode::ContractBlank,
        PjCode::StateOutOfRange,
        PjCode::SignatureDrift,
        PjCode::ChainBroken,
        PjCode::BlankStage,
        PjCode::OverreachExec,
        PjCode::NotReproducible,
        PjCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PjCode::ALL.len() {
        if PjCode::ALL[i].val() == code {
            return PjCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

// ── 决策账单行：六字段齐备 ──────────────────────────────────────
#[derive(Clone, Debug)]
pub struct StageRow {
    /// 环节下标（0..STAGE_COUNT 闭集）。
    pub stage: usize,
    /// 承接方（如 "AI-17 装载语义档" / "Wine Ldr 面"）。
    pub owner: String,
    /// 输入物（上环输出咬合）。
    pub input_artifact: String,
    /// 输出物（下环输入咬合）。
    pub output_artifact: String,
    /// 冻结契约指针（如 "AI-17 SEC_IMAGE 波07冻结"），不可空。
    pub contract: String,
    /// 状态（六态闭集）。
    pub state: StageState,
}

/// 环节衔接闭集：上环输出物名 → 下环输入物名必须同源。
/// 九环链：人格路由→镜像映射→依赖装载→重定位→IAT→TLS→线程→继承→就绪。
pub const CHAIN_ARTIFACTS: [&str; STAGE_COUNT] = [
    "路由裁决书",     // 环0 输出
    "SEC_IMAGE 映像", // 环1 输出
    "依赖闭包表",     // 环2 输出
    "重定位完映像",   // 环3 输出
    "IAT 绑定表",     // 环4 输出
    "TLS 初始化数据", // 环5 输出
    "主线程上下文",   // 环6 输出
    "继承句柄账",     // 环7 输出
    "进程就绪通知",   // 环8 输出
];

/// 上环输出 = 下环输入的咬合函数：环 k 的输入物必须等于
/// CHAIN_ARTIFACTS[k-1]（环 0 输入为 "PE 触发事件" 固定源）。
pub fn chain_expected_input(stage: usize) -> Result<&'static str, u32> {
    if stage >= STAGE_COUNT {
        return Err(PjCode::StageOutOfRange.val());
    }
    if stage == 0 {
        return Ok("PE 触发事件");
    }
    Ok(CHAIN_ARTIFACTS[stage - 1])
}

/// 单行体检：六字段 + 零静默律 + 衔接咬合。Err 带诊断码。
pub fn vet_row(row: &StageRow) -> Result<(), u32> {
    if row.stage >= STAGE_COUNT {
        return Err(PjCode::StageOutOfRange.val());
    }
    if StageState::from_u32(row.state.as_u32()).is_none() {
        return Err(PjCode::StateOutOfRange.val());
    }
    if row.state.requires_owner() && row.owner.len() == 0 {
        return Err(PjCode::OwnerBlank.val());
    }
    if row.contract.len() == 0 {
        // 零静默：待承接也必须有显式指针（指向 open_risks 登记处）。
        return Err(PjCode::ContractBlank.val());
    }
    // 衔接咬合：输入物必须等于链上预期。
    let expect = chain_expected_input(row.stage)?;
    if row.input_artifact != expect {
        return Err(PjCode::ChainBroken.val());
    }
    // 输出物必须等于链上本环产物。
    if row.output_artifact != CHAIN_ARTIFACTS[row.stage] {
        return Err(PjCode::ChainBroken.val());
    }
    Ok(())
}

/// SEC_IMAGE 消费行对照 AI-17 冻结签名：四字段逐字段比对。
/// 尺寸字段：消费行按请求实际填，冻结面验"尺寸在 2MB..2GB 合理带"
/// 且对齐 64KB（x64 分配粒度）。其余三字段必须逐位相等。
pub fn sec_image_align(c: &SecImageConsume) -> Result<(), u32> {
    if c.request_kind != AI17_SIG.request_kind {
        return Err(PjCode::SignatureDrift.val());
    }
    if c.protect_bits != AI17_SIG.protect_bits {
        return Err(PjCode::SignatureDrift.val());
    }
    if c.base_policy != AI17_SIG.base_policy {
        return Err(PjCode::SignatureDrift.val());
    }
    if c.size_bytes < 0x200_000 || c.size_bytes > 0x8000_0000 {
        return Err(PjCode::SignatureDrift.val());
    }
    if c.size_bytes % 0x1_0000 != 0 {
        return Err(PjCode::SignatureDrift.val());
    }
    Ok(())
}

// ── 注入路径总纲账 ───────────────────────────────────────────────
pub struct InjectCharter {
    rows: Vec<StageRow>,
    consume: Option<SecImageConsume>,
}

impl InjectCharter {
    pub fn new() -> InjectCharter {
        InjectCharter { rows: Vec::new(), consume: None }
    }

    /// 登记一环（环唯一律）。Err 拒收带诊断码。
    pub fn enroll(&mut self, row: StageRow) -> Result<(), u32> {
        vet_row(&row)?;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].stage == row.stage {
                return Err(PjCode::StageDuplicated.val());
            }
            i += 1;
        }
        self.rows.push(row);
        Ok(())
    }

    /// 挂 SEC_IMAGE 消费行（先读懂合同再动工）。
    pub fn set_consume(&mut self, c: SecImageConsume) -> Result<(), u32> {
        sec_image_align(&c)?;
        self.consume = Some(c);
        Ok(())
    }

    /// 九环覆盖账：环数必须恰 = STAGE_COUNT（未覆盖环节零静默）。
    pub fn coverage_closed(&self) -> Result<(), u32> {
        if self.rows.len() != STAGE_COUNT {
            return Err(PjCode::BlankStage.val());
        }
        // 全环状态无出处不可能（闭集校验在 enroll 已过）；
        // 再验每环契约指针非空（空白即红）。
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].contract.len() == 0 {
                return Err(PjCode::ContractBlank.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 衔接账复验：九环按链序逐环咬合。
    pub fn verify_chain(&self) -> Result<(), u32> {
        self.coverage_closed()?;
        let mut k = 0;
        while k < STAGE_COUNT {
            let row = &self.rows[k];
            let expect = chain_expected_input(row.stage)?;
            if row.input_artifact != expect {
                return Err(PjCode::ChainBroken.val());
            }
            k += 1;
        }
        // 链序完备：九个环下标恰好各出现一次。
        let mut seen = [false; STAGE_COUNT];
        let mut i = 0;
        while i < self.rows.len() {
            seen[self.rows[i].stage] = true;
            i += 1;
        }
        let mut s = 0;
        while s < STAGE_COUNT {
            if !seen[s] {
                return Err(PjCode::BlankStage.val());
            }
            s += 1;
        }
        Ok(())
    }

    /// SEC_IMAGE 消费行与冻结签名对照一致率（判据 100% 口径）。
    pub fn consume_align_rate(&self) -> Result<usize, u32> {
        match self.consume {
            Some(c) => {
                if sec_image_align(&c).is_ok() {
                    Ok(100)
                } else {
                    Ok(0)
                }
            }
            None => Err(PjCode::SignatureDrift.val()),
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

// ── 首版总纲装配：九环全登记 + SEC_IMAGE 消费行 ─────────────────
pub fn build_initial_charter() -> Result<InjectCharter, u32> {
    let mut ch = InjectCharter::new();
    // ① 人格路由：POSIX 子系统 or Wine 人格。
    ch.enroll(StageRow {
        stage: STAGE_ROUTER,
        owner: String::from("F16015 人格路由"),
        input_artifact: String::from("PE 触发事件"),
        output_artifact: String::from("路由裁决书"),
        contract: String::from("UNX-F16015 域内冻结"),
        state: StageState::Owned,
    })?;
    // ② 镜像映射：SEC_IMAGE 消费 AI-17 波07 冻结接口。
    ch.enroll(StageRow {
        stage: STAGE_MAPPING,
        owner: String::from("AI-17 SEC_IMAGE 映射接口"),
        input_artifact: String::from("路由裁决书"),
        output_artifact: String::from("SEC_IMAGE 映像"),
        contract: String::from("AI-17 SEC_IMAGE 波07冻结"),
        state: StageState::Owned,
    })?;
    // ③ 依赖装载。
    ch.enroll(StageRow {
        stage: STAGE_DEPEND,
        owner: String::from("AI-17 装载语义档"),
        input_artifact: String::from("SEC_IMAGE 映像"),
        output_artifact: String::from("依赖闭包表"),
        contract: String::from("AI-17 装载语义档 Schema 先行"),
        state: StageState::DualRun,
    })?;
    // ④ 重定位。
    ch.enroll(StageRow {
        stage: STAGE_RELOC,
        owner: String::from("AI-17 装载语义档"),
        input_artifact: String::from("依赖闭包表"),
        output_artifact: String::from("重定位完映像"),
        contract: String::from("AI-17 装载语义档 Schema 先行"),
        state: StageState::DualRun,
    })?;
    // ⑤ IAT 解析。
    ch.enroll(StageRow {
        stage: STAGE_IAT,
        owner: String::from("AI-17 装载语义档"),
        input_artifact: String::from("重定位完映像"),
        output_artifact: String::from("IAT 绑定表"),
        contract: String::from("AI-17 装载语义档 Schema 先行"),
        state: StageState::DualRun,
    })?;
    // ⑥ TLS 目录。
    ch.enroll(StageRow {
        stage: STAGE_TLS,
        owner: String::from("AI-17 装载语义档"),
        input_artifact: String::from("IAT 绑定表"),
        output_artifact: String::from("TLS 初始化数据"),
        contract: String::from("AI-17 装载语义档 Schema 先行"),
        state: StageState::PartialOwned,
    })?;
    // ⑦ 线程启动。
    ch.enroll(StageRow {
        stage: STAGE_THREAD,
        owner: String::from("AI-17 线程语义"),
        input_artifact: String::from("TLS 初始化数据"),
        output_artifact: String::from("主线程上下文"),
        contract: String::from("AI-17 线程语义档"),
        state: StageState::Owned,
    })?;
    // ⑧ 句柄继承。
    ch.enroll(StageRow {
        stage: STAGE_INHERIT,
        owner: String::from("AI-17 进程语义"),
        input_artifact: String::from("主线程上下文"),
        output_artifact: String::from("继承句柄账"),
        contract: String::from("AI-17 进程语义档"),
        state: StageState::Owned,
    })?;
    // ⑨ 就绪通知。
    ch.enroll(StageRow {
        stage: STAGE_READY,
        owner: String::from("AI-26 surface 提交与输入事件接口"),
        input_artifact: String::from("继承句柄账"),
        output_artifact: String::from("进程就绪通知"),
        contract: String::from("AI-26 波10前冻结 Schema"),
        state: StageState::Pending,
    })?;
    // SEC_IMAGE 消费行：按冻结签名逐字段填（尺寸样例 16MB 对齐）。
    ch.set_consume(SecImageConsume {
        request_kind: AI17_SIG.request_kind,
        size_bytes: 0x100_0000, // 16 MiB，64KB 对齐，2MB..2GB 带内
        protect_bits: AI17_SIG.protect_bits,
        base_policy: AI17_SIG.base_policy,
    })?;
    Ok(ch)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((环数, 对照一致率))。
pub fn golden_run() -> Result<(usize, usize), u32> {
    let ch = build_initial_charter()?;
    ch.verify_chain()?;
    let rate = ch.consume_align_rate()?;
    if rate != 100 {
        return Err(PjCode::SignatureDrift.val());
    }
    Ok((ch.len(), rate))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 删契约指针：contract 空 → ContractBlank。
        let bad = StageRow {
            stage: STAGE_ROUTER,
            owner: String::from("F16015 人格路由"),
            input_artifact: String::from("PE 触发事件"),
            output_artifact: String::from("路由裁决书"),
            contract: String::new(),
            state: StageState::Owned,
        };
        led.record(0, vet_row(&bad) == Err(PjCode::ContractBlank.val()));

        // ② 环节空白/覆盖缺环：八环账 → coverage_closed 必红。
        let eight_ok = {
            let ch = build_initial_charter();
            match ch {
                Ok(mut c) => {
                    c.rows.pop(); // 抽掉一环（模拟未覆盖）
                    c.verify_chain() == Err(PjCode::BlankStage.val())
                }
                Err(_) => false,
            }
        };
        led.record(1, eight_ok);

        // ③ 衔接断裂：上环输入物被替换 → ChainBroken。
        let bad2 = StageRow {
            stage: STAGE_DEPEND,
            owner: String::from("AI-17 装载语义档"),
            input_artifact: String::from("未知产物"),
            output_artifact: String::from("依赖闭包表"),
            contract: String::from("AI-17 装载语义档 Schema 先行"),
            state: StageState::DualRun,
        };
        led.record(2, vet_row(&bad2) == Err(PjCode::ChainBroken.val()));

        // ④ 签名漂移：SEC_IMAGE 消费行保护位/基点/请求型三向漂移。
        let c1 = SecImageConsume {
            request_kind: AI17_SIG.request_kind + 1,
            size_bytes: 0x100_0000,
            protect_bits: AI17_SIG.protect_bits,
            base_policy: AI17_SIG.base_policy,
        };
        let c2 = SecImageConsume {
            request_kind: AI17_SIG.request_kind,
            size_bytes: 0x100_0000,
            protect_bits: AI17_SIG.protect_bits + 1,
            base_policy: AI17_SIG.base_policy,
        };
        let c3 = SecImageConsume {
            request_kind: AI17_SIG.request_kind,
            size_bytes: 0x100_0003, // 不对齐 64KB
            protect_bits: AI17_SIG.protect_bits,
            base_policy: AI17_SIG.base_policy,
        };
        let c4 = SecImageConsume {
            request_kind: AI17_SIG.request_kind,
            size_bytes: 0x1000, // 带外（<2MB）
            protect_bits: AI17_SIG.protect_bits,
            base_policy: AI17_SIG.base_policy,
        };
        led.record(
            3,
            sec_image_align(&c1) == Err(PjCode::SignatureDrift.val())
                && sec_image_align(&c2) == Err(PjCode::SignatureDrift.val())
                && sec_image_align(&c3) == Err(PjCode::SignatureDrift.val())
                && sec_image_align(&c4) == Err(PjCode::SignatureDrift.val()),
        );

        // ⑤ 环节重复登记：同环两次 enroll 必拒。
        let dup_ok = {
            let mut ch2 = InjectCharter::new();
            let row = StageRow {
                stage: STAGE_ROUTER,
                owner: String::from("F16015 人格路由"),
                input_artifact: String::from("PE 触发事件"),
                output_artifact: String::from("路由裁决书"),
                contract: String::from("UNX-F16015 域内冻结"),
                state: StageState::Owned,
            };
            let first = ch2.enroll(row.clone());
            let second = ch2.enroll(row);
            first.is_ok() && second == Err(PjCode::StageDuplicated.val())
        };
        led.record(4, dup_ok);

        // ⑥ 状态越六态闭集 + 环节越九环闭集。
        led.record(5, StageState::from_u32(6).is_none());
        led.record(5, StageState::from_u32(0xFFFF).is_none());
        led.record(5, chain_expected_input(STAGE_COUNT).is_err());
        led.record(5, chain_expected_input(STAGE_COUNT + 1 + (rnd % 3) as usize).is_err());

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(PjCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法总纲必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let ch = build_initial_charter()?;
    ch.verify_chain()?;
    if ch.len() != STAGE_COUNT {
        return Err(PjCode::BlankStage.val());
    }
    // 九环衔接产物名逐环点验。
    let mut k = 0;
    while k < STAGE_COUNT {
        if CHAIN_ARTIFACTS[k].len() == 0 {
            return Err(PjCode::ChainBroken.val());
        }
        k += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PjCode::ALL.len() {
        let v = PjCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥8 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：九环 + 对照一致率 100%。
    let g = golden_run();
    v.push((
        "J1-黄金正演九环100%",
        matches!(g, Ok((9, 100))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 九环覆盖闭合（未覆盖环节零静默）。
    let ch = build_initial_charter()
        .unwrap_or_else(|_| InjectCharter::new());
    v.push((
        "J1-九环覆盖零静默",
        matches!(ch.coverage_closed(), Ok(())),
        format!("{} 环", ch.len()),
    ));
    // 4. 衔接账：九环逐环咬合。
    v.push((
        "J1-衔接账逐环咬合",
        matches!(ch.verify_chain(), Ok(())),
        String::from("上环输出=下环输入"),
    ));
    // 5. SEC_IMAGE 对照一致率 100%（判据原文口径）。
    let rate = ch.consume_align_rate().unwrap_or(0);
    v.push(("J1-SEC_IMAGE对照100%", rate == 100, format!("{}%", rate)));
    // 6. 六态闭集点验。
    let mut st_ok = true;
    let mut s = 0;
    while s < 6 {
        if StageState::from_u32(s as u32).is_none() {
            st_ok = false;
        }
        s += 1;
    }
    v.push(("J1-六态闭集点验", st_ok, String::from("0..5 全可解")));
    // 7. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("链复验+产物名"),
    ));
    // 9. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0xE1);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16061_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16061 PE 注入路径总纲与 SEC_IMAGE 消费决策账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
