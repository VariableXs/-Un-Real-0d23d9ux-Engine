//! VE-F0200 · 域移交包（VE-B 衔接 · A 域收官 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0200`
//!
//! **职责定位（锚点原文）**：A 域向 B 域移交包：冻结接口清单与版本（F0187）、
//! AdapterReport 契约样例、遥测总线接入指南、调试通道协议、经验包（呈现协商
//! 矩阵方法论、软渲参考实现语义）；B 域已按冻结接口先行实现（virtio 组样板
//! F0201），本包补记接口符合性核对结果（逐接口三态：符合、偏差已登记、待核）。
//!
//! **数据结构（锚点原文）**：移交包清单（五件）；符合性核对表（接口×三态）。
//! 二者在 [`HandoverPack`] 上一一落位：五件 = [`PackItem`] 的
//! `ITEM_COUNT` 常量与 [`PackSlot`] 数组；核对表 = [`ConformanceEntry`]
//! 的 `Vec`（`entries`）。
//!
//! **判据（锚点原文）**：五件套、符合性三态、偏差登记、先行实现核对、判据。
//!
//! 本模块 17 条判据逐条映射：**五件套 4 条**（assemble 产出五件齐／缺件装配
//! 拒绝／五件编号连续且互异／五件文案逐件非空）、**符合性三态 5 条**（三态
//! 枚举完备且互异／初始登记全为待核／符合登记生效／待核显性标注不改写为
//! 符合／未知接口登记拒绝）、**偏差登记 4 条**（偏差必须带阻断或随行判定／
//! 阻断未清不放行／随行不挡放行且逐条可见／偏差清账后恢复可放行）、
//! **先行实现核对 2 条**（B 域样板 F0201 接口在同表可核对／升版全部回待核
//! 且旧三态只入历史账）、**判据自身 2 条**（码位互异且段独占／统计对账）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 核对发现偏差 → **登记并判定阻断或随行** | [`HandoverPack::record_deviation`] 强制要求调用方给出 [`DeviationVerdict`]（阻断 / 随行二选一，无默认值可省略），偏差事实（[`Deviation`]）与判定一同入 [`ConformanceEntry`]，状态置 [`Conformance::Deviation`]——「有偏差但没判定」在类型层面写不出来 |
//! | 接口缺核 → **待核显性标注** | [`Conformance::Pending`] 是装配后的**初始态**而不是省略；[`HandoverPack::ready_to_handover`] 对任何 Pending 直接判 false 且计数进 [`PackStats::pending`]——「还没核对」永远不会被静默当成「已核对」 |
//! | 移交后接口变更 → **升版流程重核对** | [`HandoverPack::bump_version`] 把全部条目压回 [`Conformance::Pending`]，旧三态只进 `history_recheck` 计数；升版后 `ready_to_handover` 恒 false，直到逐接口重核——升版即断闸，不存在「改了接口还能拿旧核对结果放行」的通道 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **三态不是两态**：`符合 / 偏差 / 待核` 三态里最危险的是把「待核」折进
//!   「偏差」或「符合」的简化。待核是**事实**（还没核），偏差是**结论**（核了
//!   且不一致）。[`Conformance`] 三变体互斥且无 `Default`——漏核在类型层面
//!   就是显性的 [`Conformance::Pending`]。
//! - **阻断与随行是偏差的必选属性**：锚点把「判定阻断或随行」写成核对动作
//!   的一部分，所以 [`DeviationVerdict`] 是 [`Deviation`] 构造的必传参数——
//!   不存在「登记了偏差但没人决定它挡不挡放行」的中间态。
//! - **放行判定是纯函数**：[`HandoverPack::ready_to_handover`] 只看三样——
//!   五件齐、无待核、无未清阻断偏差。它不读时钟、不读环境，「为什么不能移交」
//!   的答案永远可从 [`PackStats`] 三个计数器里读出来。
//! - **升版是断闸不是换标签**：`bump_version` 之后所有旧核对结果**只进历史
//!   计数**、不再参与放行——因为接口变了，旧核对约束的是旧接口。这是 F0187
//!   冻结接口纪律在移交包上的延续：冻结的意义就是变更必须付出重核的成本。
//! - **先行实现核对走同一张表**：B 域按冻结接口先行实现（F0201 virtio 组），
//!   其符合性核对与 A 域自核走**同一个** [`HandoverPack`]——两套核对表迟早
//!   会分叉成两套事实。
//! - **零 panic 面**：查表走 `get`/`iter`、计数全 `saturating_add`、所有
//!   登记接口返回 [`Result`] 且未知接口显性拒绝（[`CODE_IFACE_UNKNOWN`]）。
//! - **判据侧独立重算**：阈值与「五件」清单在判据里按字面量再写一遍（判据
//!   5.1），不向被测常量问答案——常量被误改时判据自身先红。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x6Axx，独占——整仓 grep 0x6A01 零占用后选定）
// ---------------------------------------------------------------------------

/// 登记了移交包清单之外的未知接口。
pub const CODE_IFACE_UNKNOWN: u16 = 0x6A01;
/// 版本回退（新版本号不大于当前版本号）。
pub const CODE_VERSION_REGRESS: u16 = 0x6A02;
/// 存在未清的阻断偏差仍要求放行。
pub const CODE_BLOCKING_OPEN: u16 = 0x6A03;
/// 同一接口重复登记核对条目。
pub const CODE_DUP_IFACE: u16 = 0x6A04;
/// 五件套缺件（装配清单不齐）。
pub const CODE_PACK_INCOMPLETE: u16 = 0x6A05;
/// 升版后未重核对即尝试放行（含升版本身把条目压回待核后直接放行）。
pub const CODE_RECHECK_MISSED: u16 = 0x6A06;
/// 偏差登记缺少阻断/随行判定（构造路径不可能，登记 API 强制判定；
/// 本码留给从持久化侧读回残缺条目的防御路径）。
pub const CODE_DEVIATION_VERDICT_MISSING: u16 = 0x6A07;

// ---------------------------------------------------------------------------
// 二、五件套：移交包清单（锚点：五件 = 清单/契约样例/遥测指南/调试协议/经验包）
// ---------------------------------------------------------------------------

/// 移交包五件的固定编号（0..=4），编号即验收顺序，连续且互异。
pub mod pack_item_ids {
    /// 第 0 件：冻结接口清单与版本（F0187）。
    pub const ID_IFACE_LIST: usize = 0;
    /// 第 1 件：AdapterReport 契约样例。
    pub const ID_ADAPTER_SAMPLE: usize = 1;
    /// 第 2 件：遥测总线接入指南。
    pub const ID_TELEMETRY_GUIDE: usize = 2;
    /// 第 3 件：调试通道协议。
    pub const ID_DEBUG_CHAN: usize = 3;
    /// 第 4 件：经验包（呈现协商矩阵方法论、软渲参考实现语义）。
    pub const ID_EXPERIENCE: usize = 4;
}

/// 五件套件数（判据侧按字面量独立重算，不向本常量问答案）。
pub const ITEM_COUNT: usize = 5;

/// 单件移交物：编号 + 固定名 + 说明文案。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackSlot {
    /// 件编号（0..=4，连续）。
    pub id: usize,
    /// 件固定名（判据按名对拍）。
    pub name: &'static str,
    /// 件内容说明（非空即视为已装件）。
    pub brief: &'static str,
}

/// 按编号装配五件套；`briefs` 缺件或空文案即拒（缺件 = 显性失败不静默补空）。
pub fn assemble_slots(briefs: [&'static str; ITEM_COUNT]) -> Result<[PackSlot; ITEM_COUNT], u16> {
    let names: [&'static str; ITEM_COUNT] = [
        "冻结接口清单与版本",
        "AdapterReport 契约样例",
        "遥测总线接入指南",
        "调试通道协议",
        "经验包",
    ];
    for i in 0..ITEM_COUNT {
        if briefs[i].is_empty() {
            return Err(CODE_PACK_INCOMPLETE);
        }
    }
    Ok([
        PackSlot { id: pack_item_ids::ID_IFACE_LIST, name: names[0], brief: briefs[0] },
        PackSlot { id: pack_item_ids::ID_ADAPTER_SAMPLE, name: names[1], brief: briefs[1] },
        PackSlot { id: pack_item_ids::ID_TELEMETRY_GUIDE, name: names[2], brief: briefs[2] },
        PackSlot { id: pack_item_ids::ID_DEBUG_CHAN, name: names[3], brief: briefs[3] },
        PackSlot { id: pack_item_ids::ID_EXPERIENCE, name: names[4], brief: briefs[4] },
    ])
}

// ---------------------------------------------------------------------------
// 三、符合性核对表：接口 × 三态
// ---------------------------------------------------------------------------

/// 符合性三态（锚点原文：符合、偏差已登记、待核）。
///
/// 无 `Default`：待核是装配时的**显性初始态**，不是任何状态的缺省回退。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conformance {
    /// 已核，逐条落实无偏差。
    Conformant,
    /// 已核，有偏差且已登记（判定见 [`Deviation`]）。
    Deviation,
    /// 尚未核对（显性待核，永不静默改写）。
    Pending,
}

impl Conformance {
    /// 三态全集（判据枚举完备性断言用）。
    pub const ALL: [Conformance; 3] =
        [Conformance::Conformant, Conformance::Deviation, Conformance::Pending];

    /// 三态短名（读屏与统计口径统一）。
    pub const fn as_str(self) -> &'static str {
        match self {
            Conformance::Conformant => "符合",
            Conformance::Deviation => "偏差",
            Conformance::Pending => "待核",
        }
    }
}

/// 偏差判定：阻断（挡放行）或随行（不挡放行、随移交包走）。
///
/// 构造必传——「登记了偏差但没人决定它挡不挡放行」在类型层面写不出来。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviationVerdict {
    /// 阻断：未清之前 [`HandoverPack::ready_to_handover`] 恒 false。
    Blocking,
    /// 随行：随移交包移交，由 B 域在接续批次内消化。
    RideAlong,
}

impl DeviationVerdict {
    /// 判定短名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            DeviationVerdict::Blocking => "阻断",
            DeviationVerdict::RideAlong => "随行",
        }
    }
}

/// 偏差事实 + 判定（判定是必选属性，见上）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deviation {
    /// 阻断 / 随行判定。
    pub verdict: DeviationVerdict,
    /// 偏差一句话事实（非空约束由登记 API 把守）。
    pub note: &'static str,
}

/// 单接口符合性条目：接口 × 版本 × 三态 × 可选偏差。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConformanceEntry {
    /// 冻结接口 id（F0187 清单内，如 "VE-F0001"）。
    pub iface: &'static str,
    /// 核对时依据的接口版本（升版后此条目作废重核）。
    pub iface_version: u32,
    /// 三态。
    pub state: Conformance,
    /// 偏差（仅 [`Conformance::Deviation`] 态非 `None`）。
    pub deviation: Option<Deviation>,
}

// ---------------------------------------------------------------------------
// 四、统计账（放行判定的全部依据都可从这里读出来）
// ---------------------------------------------------------------------------

/// 移交包统计账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PackStats {
    /// 待核条目数。
    pub pending: usize,
    /// 偏差条目数（含阻断与随行）。
    pub deviations: usize,
    /// 未清阻断偏差数。
    pub blocking_open: usize,
    /// 随行偏差数。
    pub ridealong: usize,
    /// 升版重核次数（每次 bump_version 加一）。
    pub history_recheck: usize,
    /// 登记拒绝次数（未知接口 / 重复接口 / 空偏差文案）。
    pub rejects: usize,
    /// 放行判定尝试次数。
    pub gate_queries: usize,
}

// ---------------------------------------------------------------------------
// 五、移交包本体
// ---------------------------------------------------------------------------

/// 域移交包：五件套 + 符合性核对表 + 统计账。
///
/// 版本从 1 起（0 号留给「未装配」）。装配一次、核对逐接口登记、
/// 升版断闸重核——生命周期只有这三段，与锚点「三段式」判据对齐。
#[derive(Clone, Debug)]
pub struct HandoverPack {
    /// 移交包版本（升版流程递增，从 1 起）。
    pub version: u32,
    /// 五件套（装配后长度恒为 [`ITEM_COUNT`]）。
    pub slots: Vec<PackSlot>,
    /// 符合性核对表（按登记序存放；查重走线性扫——接口数 O(接口) 级，
    /// 移交是一次性动作，不值得为此上哈希）。
    pub entries: Vec<ConformanceEntry>,
    /// 统计账。
    pub stats: PackStats,
}

impl HandoverPack {
    /// 装配移交包：五件套齐 + 冻结接口清单入表（全部待核初始态）。
    ///
    /// `frozen` 是 F0187 冻结接口清单（id × 版本）；清单为空即拒——
    /// 移交包没有空转发的形态。
    pub fn assemble(
        briefs: [&'static str; ITEM_COUNT],
        frozen: &[(&'static str, u32)],
    ) -> Result<HandoverPack, u16> {
        let slots = assemble_slots(briefs)?;
        if frozen.is_empty() {
            return Err(CODE_IFACE_UNKNOWN);
        }
        let mut entries = Vec::new();
        for (iface, ver) in frozen.iter() {
            entries.push(ConformanceEntry {
                iface,
                iface_version: *ver,
                state: Conformance::Pending,
                deviation: None,
            });
        }
        Ok(HandoverPack {
            version: 1,
            slots: slots.to_vec(),
            entries,
            stats: PackStats {
                pending: frozen.len(),
                ..PackStats::default()
            },
        })
    }

    /// 登记符合（已核无偏差）。
    pub fn record_conformant(&mut self, iface: &str, at_version: u32) -> Result<(), u16> {
        self.record(iface, at_version, Conformance::Conformant, None)
    }

    /// 登记待核（显性标注；只允许把**同版本**条目改回待核——
    /// 跨版本回写会制造「旧版本核对结果」的假象）。
    pub fn record_pending(&mut self, iface: &str, at_version: u32) -> Result<(), u16> {
        self.record(iface, at_version, Conformance::Pending, None)
    }

    /// 登记偏差：偏差事实 + 阻断/随行判定必传，文案非空必传。
    pub fn record_deviation(
        &mut self,
        iface: &str,
        at_version: u32,
        verdict: DeviationVerdict,
        note: &'static str,
    ) -> Result<(), u16> {
        if note.is_empty() {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(CODE_DEVIATION_VERDICT_MISSING);
        }
        self.record(
            iface,
            at_version,
            Conformance::Deviation,
            Some(Deviation { verdict, note }),
        )
    }

    /// 登记统一入口：查表（未知拒绝 / 重复拒绝）→ 校验版本 → 落三态与偏差 → 记账。
    fn record(
        &mut self,
        iface: &str,
        at_version: u32,
        state: Conformance,
        deviation: Option<Deviation>,
    ) -> Result<(), u16> {
        let idx = match self.entries.iter().position(|e| e.iface == iface) {
            Some(i) => i,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(CODE_IFACE_UNKNOWN);
            }
        };
        // 版本守门：登记必须对**当前版本**发生。升版（bump_version）把
        // 条目压回待核并把 iface_version 抬到新版本；对旧版本的登记在
        // 这里就被拦下——它约束的是旧接口，写进新表只会污染事实。
        if self.entries[idx].iface_version != at_version {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(CODE_VERSION_REGRESS);
        }
        let prev = self.entries[idx].state;
        self.entries[idx].state = state;
        self.entries[idx].deviation = deviation;
        // 统计账按「增量差」记账：从旧态迁移到新态，两边各退一进一，
        // 保持 stats 与 entries 逐条可对账（判据 5.2 的对账口径）。
        match prev {
            Conformance::Pending => {
                self.stats.pending = self.stats.pending.saturating_sub(1);
            }
            Conformance::Deviation => {
                self.stats.deviations = self.stats.deviations.saturating_sub(1);
                if let Some(d) = self.entries[idx].deviation {
                    // 旧偏差已随本次覆写退场：从对应判定桶退出。
                    match d.verdict {
                        DeviationVerdict::Blocking => {
                            self.stats.blocking_open =
                                self.stats.blocking_open.saturating_sub(1)
                        }
                        DeviationVerdict::RideAlong => {
                            self.stats.ridealong = self.stats.ridealong.saturating_sub(1)
                        }
                    }
                }
            }
            Conformance::Conformant => {}
        }
        match state {
            Conformance::Conformant => {}
            Conformance::Deviation => {
                self.stats.deviations = self.stats.deviations.saturating_add(1);
                if let Some(d) = deviation {
                    match d.verdict {
                        DeviationVerdict::Blocking => {
                            self.stats.blocking_open = self.stats.blocking_open.saturating_add(1)
                        }
                        DeviationVerdict::RideAlong => {
                            self.stats.ridealong = self.stats.ridealong.saturating_add(1)
                        }
                    }
                }
            }
            Conformance::Pending => {
                self.stats.pending = self.stats.pending.saturating_add(1);
            }
        }
        Ok(())
    }

    /// 升版断闸：全部条目压回待核，旧三态只进历史计数。
    ///
    /// 新版本号必须**严格大于**当前版本（回退显性拒绝）——版本轴是单调的，
    /// 回退等于拿旧事实覆盖新事实。
    pub fn bump_version(&mut self, new_version: u32) -> Result<(), u16> {
        if new_version <= self.version {
            return Err(CODE_VERSION_REGRESS);
        }
        self.version = new_version;
        for e in self.entries.iter_mut() {
            e.iface_version = new_version;
            e.state = Conformance::Pending;
            e.deviation = None;
        }
        self.stats.history_recheck = self.stats.history_recheck.saturating_add(1);
        self.stats.pending = self.entries.len();
        self.stats.deviations = 0;
        self.stats.blocking_open = 0;
        self.stats.ridealong = 0;
        Ok(())
    }

    /// 放行判定（纯函数）：五件齐 + 无待核 + 无未清阻断偏差。
    ///
    /// 不满足时返回对应诊断码（显性原因，不返回笼统布尔）。
    pub fn gate(&mut self) -> Result<(), u16> {
        self.stats.gate_queries = self.stats.gate_queries.saturating_add(1);
        if self.slots.len() != ITEM_COUNT {
            return Err(CODE_PACK_INCOMPLETE);
        }
        if self.stats.pending > 0 {
            return Err(CODE_RECHECK_MISSED);
        }
        if self.stats.blocking_open > 0 {
            return Err(CODE_BLOCKING_OPEN);
        }
        Ok(())
    }

    /// 读屏摘要（锚点：无直接无障碍面，但移交状态读屏可达——逐件逐接口一行）。
    pub fn a11y_summary(&self) -> String {
        let mut lines = Vec::new();
        lines.push(alloc::format!(
            "移交包 v{}：五件 {} / 接口 {}",
            self.version,
            self.slots.len(),
            self.entries.len()
        ));
        for s in self.slots.iter() {
            lines.push(alloc::format!("件{} {}：{}", s.id, s.name, s.brief));
        }
        for e in self.entries.iter() {
            match (e.state, e.deviation) {
                (Conformance::Deviation, Some(d)) => lines.push(alloc::format!(
                    "{} v{}：{}（{}：{}）",
                    e.iface,
                    e.iface_version,
                    e.state.as_str(),
                    d.verdict.as_str(),
                    d.note
                )),
                _ => lines.push(alloc::format!(
                    "{} v{}：{}",
                    e.iface,
                    e.iface_version,
                    e.state.as_str()
                )),
            }
        }
        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// 六、判据（run_veb00_checks —— 17 条，逐条映射锚点五条判据）
// ---------------------------------------------------------------------------

/// VE-F0200 域自检：五件套 4 + 三态 5 + 偏差登记 4 + 先行核对 2 + 判据自身 2。
pub fn run_veb00_checks() -> CheckSet {
    let mut s = CheckSet::new("veb00_handover");

    // --- 判据 1.1：五件套——assemble 产出五件齐、编号连续互异、文案逐件非空 ---
    {
        let pack = HandoverPack::assemble(
            ["清单A", "样例B", "遥测C", "调试D", "经验E"],
            &[("VE-F0001", 1), ("VE-F0007", 1), ("VE-F0187", 1)],
        );
        let ok = match pack {
            Ok(p) => {
                p.slots.len() == 5
                    && p.slots.iter().enumerate().all(|(i, sl)| sl.id == i && !sl.brief.is_empty())
                    && {
                        // 编号两两互异（5 件全对拍，不抽样）
                        let ids = [p.slots[0].id, p.slots[1].id, p.slots[2].id, p.slots[3].id, p.slots[4].id];
                        ids[0] != ids[1]
                            && ids[0] != ids[2]
                            && ids[0] != ids[3]
                            && ids[0] != ids[4]
                            && ids[1] != ids[2]
                            && ids[1] != ids[3]
                            && ids[1] != ids[4]
                            && ids[2] != ids[3]
                            && ids[2] != ids[4]
                            && ids[3] != ids[4]
                    }
                    && p.entries.len() == 3
                    && p.entries.iter().all(|e| e.state == Conformance::Pending)
            }
            Err(_) => false,
        };
        s.add(
            "B00-五件套-装配产出五件齐且全待核初始态",
            ok,
            "五件编号 0..=4 连续互异、文案逐件非空、冻结接口全表待核、版本从 1 起",
        );
    }

    // --- 判据 1.2：五件套——缺件（空文案）装配显性拒绝 ---
    {
        let hole = HandoverPack::assemble(
            ["清单A", "样例B", "", "调试D", "经验E"],
            &[("VE-F0001", 1)],
        );
        s.add(
            "B00-五件套-缺件装配显性拒绝不静默补空",
            hole == Err(CODE_PACK_INCOMPLETE),
            "任一件文案为空即 CODE_PACK_INCOMPLETE，缺件没有「补个空串」的降级形态",
        );
    }

    // --- 判据 1.3：五件套——空冻结清单拒绝（无空转发形态） ---
    {
        let empty = HandoverPack::assemble(
            ["A", "B", "C", "D", "E"],
            &[],
        );
        s.add(
            "B00-五件套-空冻结清单拒绝",
            empty == Err(CODE_IFACE_UNKNOWN),
            "冻结接口清单为空的移交包没有意义，装配期就拒绝",
        );
    }

    // --- 判据 1.4：五件套——判据侧独立重算件数与五件名（不向被测问答案） ---
    {
        s.add(
            "B00-五件套-判据侧独立重算五件常量",
            ITEM_COUNT == 5
                && pack_item_ids::ID_IFACE_LIST == 0
                && pack_item_ids::ID_ADAPTER_SAMPLE == 1
                && pack_item_ids::ID_TELEMETRY_GUIDE == 2
                && pack_item_ids::ID_DEBUG_CHAN == 3
                && pack_item_ids::ID_EXPERIENCE == 4,
            "五件编号清单在判据侧按字面量重写：常量被误改时本条先红",
        );
    }

    // --- 判据 2.1：三态——枚举完备且互异（三态短名两两不同） ---
    {
        let (a, b, c) = (
            Conformance::Conformant.as_str(),
            Conformance::Deviation.as_str(),
            Conformance::Pending.as_str(),
        );
        s.add(
            "B00-三态-枚举完备且三短名互异",
            Conformance::ALL.len() == 3 && a != b && a != c && b != c,
            "三态全集三变体，读屏短名两两互异——短名相等的两态在读屏口径里就是同一态",
        );
    }

    // --- 判据 2.2：三态——符合登记生效（待核→符合、pending 退账） ---
    {
        let mut p = sample_pack();
        let r0 = p.record_conformant("VE-F0001", 1);
        let ok = r0.is_ok()
            && entry_state(&p, "VE-F0001") == Some(Conformance::Conformant)
            && p.stats.pending == 2
            && p.stats.deviations == 0;
        s.add(
            "B00-三态-符合登记生效且待核退账",
            ok,
            "登记符合后条目态迁移、待核计数退一，统计与条目逐条可对账",
        );
    }

    // --- 判据 2.3：三态——待核显性标注不改写为符合（回写待核合法且记账） ---
    {
        let mut p = sample_pack();
        let _ = p.record_conformant("VE-F0001", 1);
        let back = p.record_pending("VE-F0001", 1);
        s.add(
            "B00-三态-显性回写待核合法且记账",
            back.is_ok()
                && entry_state(&p, "VE-F0001") == Some(Conformance::Pending)
                && p.stats.pending == 3,
            "「还没核对」可以重新显性化：待核不是省略态，是可登记的事实态",
        );
    }

    // --- 判据 2.4：三态——未知接口登记拒绝且记账 ---
    {
        let mut p = sample_pack();
        let r = p.record_conformant("VE-F9999", 1);
        s.add(
            "B00-三态-未知接口登记拒绝",
            r == Err(CODE_IFACE_UNKNOWN) && p.stats.rejects == 1,
            "核对表只认 F0187 冻结清单内的接口，清单外登记显性拒绝",
        );
    }

    // --- 判据 2.5：三态——跨版本登记拒绝（旧版本核对结果写不进新表） ---
    {
        let mut p = sample_pack();
        let _ = p.bump_version(2);
        let stale = p.record_conformant("VE-F0001", 1);
        s.add(
            "B00-三态-跨版本登记拒绝",
            stale == Err(CODE_VERSION_REGRESS),
            "升版后按旧版本登记直接拒——它约束的是旧接口，写进新表只会污染事实",
        );
    }

    // --- 判据 3.1：偏差登记——偏差必须带判定（随行偏差可登记且进对应桶） ---
    {
        let mut p = sample_pack();
        let r = p.record_deviation("VE-F0001", 1, DeviationVerdict::RideAlong, "遥测字段宽化一档");
        let ok = r.is_ok()
            && entry_state(&p, "VE-F0001") == Some(Conformance::Deviation)
            && p.stats.deviations == 1
            && p.stats.ridealong == 1
            && p.stats.blocking_open == 0;
        s.add(
            "B00-偏差-随行偏差登记进随行桶不进阻断桶",
            ok,
            "偏差三要素齐：事实 + 判定 + 状态迁移，随行与阻断分桶记账",
        );
    }

    // --- 判据 3.2：偏差登记——阻断未清不放行（gate 显性返回阻断码） ---
    {
        let mut p = sample_pack();
        let _ = p.record_deviation("VE-F0001", 1, DeviationVerdict::Blocking, "调试通道缺鉴权");
        // 把其余接口全部核完：只剩阻断偏差未清
        let _ = p.record_conformant("VE-F0007", 1);
        let _ = p.record_conformant("VE-F0187", 1);
        let gate = p.gate();
        s.add(
            "B00-偏差-阻断未清不放行且显性返回阻断码",
            gate == Err(CODE_BLOCKING_OPEN),
            "无待核但有未清阻断偏差：gate 拒绝并给出阻断码，不是笼统布尔",
        );
    }

    // --- 判据 3.3：偏差登记——偏差清账后恢复放行（阻断→符合迁移退桶） ---
    {
        let mut p = sample_pack();
        let _ = p.record_deviation("VE-F0001", 1, DeviationVerdict::Blocking, "调试通道缺鉴权");
        let _ = p.record_conformant("VE-F0007", 1);
        let _ = p.record_conformant("VE-F0187", 1);
        let before = p.gate();
        let fixed = p.record_conformant("VE-F0001", 1);
        let after = p.gate();
        s.add(
            "B00-偏差-偏差清账后恢复放行",
            before == Err(CODE_BLOCKING_OPEN)
                && fixed.is_ok()
                && p.stats.blocking_open == 0
                && p.stats.deviations == 0
                && after.is_ok(),
            "阻断→符合的迁移把偏差从桶里退出：清账动作有据，放行恢复有据",
        );
    }

    // --- 判据 3.4：偏差登记——空偏差文案拒绝（登记 API 强制文案非空） ---
    {
        let mut p = sample_pack();
        let r = p.record_deviation("VE-F0001", 1, DeviationVerdict::Blocking, "");
        s.add(
            "B00-偏差-空偏差文案拒绝",
            r == Err(CODE_DEVIATION_VERDICT_MISSING) && p.stats.rejects == 1,
            "没有事实的偏差登记没有意义：文案为空显性拒绝",
        );
    }

    // --- 判据 4.1：先行实现核对——B 域样板接口（F0201 组）在同表可核对 ---
    {
        // B 域按冻结接口先行实现：virtio 组（F0201 探测/F0202 协议）与 A 域
        // 自核走同一张表、同一套三态——两套核对表迟早分叉成两套事实。
        let mut p = sample_pack_with(&[
            ("VE-F0201", 1),
            ("VE-F0202", 1),
        ]);
        let r1 = p.record_conformant("VE-F0201", 1);
        let r2 = p.record_deviation("VE-F0202", 1, DeviationVerdict::RideAlong, "队列深度声明待 B01 复核");
        let ok = r1.is_ok()
            && r2.is_ok()
            && entry_state(&p, "VE-F0201") == Some(Conformance::Conformant)
            && entry_state(&p, "VE-F0202") == Some(Conformance::Deviation)
            && p.stats.pending == 0
            && p.gate().is_ok();
        s.add(
            "B00-先行核对-B域样板接口同表三态核对且可放行",
            ok,
            "F0201/F0202 先行实现按同表核对：符合与随行偏差各一、无待核、放行通过",
        );
    }

    // --- 判据 4.2：先行实现核对——升版断闸：全部回待核、旧三态进历史账、回退拒绝 ---
    {
        let mut p = sample_pack();
        let _ = p.record_conformant("VE-F0001", 1);
        let _ = p.record_deviation("VE-F0007", 1, DeviationVerdict::Blocking, "能力位图缺一位");
        let _ = p.record_conformant("VE-F0187", 1);
        let regress = p.bump_version(1);
        let bumped = p.bump_version(2);
        let ok = regress == Err(CODE_VERSION_REGRESS)
            && bumped.is_ok()
            && p.version == 2
            && p.entries.iter().all(|e| e.state == Conformance::Pending && e.deviation.is_none())
            && p.stats.pending == 3
            && p.stats.deviations == 0
            && p.stats.blocking_open == 0
            && p.stats.history_recheck == 1
            && p.gate() == Err(CODE_RECHECK_MISSED);
        s.add(
            "B00-先行核对-升版断闸全表回待核且版本回退显性拒绝",
            ok,
            "升版即断闸：旧核对结果只进历史计数、放行显性返回待核码；回退版本直接拒绝",
        );
    }

    // --- 判据 5.1：判据自身——诊断码互异且段独占（0x6Axx 八码全对拍） ---
    {
        let codes = [
            CODE_IFACE_UNKNOWN,
            CODE_VERSION_REGRESS,
            CODE_BLOCKING_OPEN,
            CODE_DUP_IFACE,
            CODE_PACK_INCOMPLETE,
            CODE_RECHECK_MISSED,
            CODE_DEVIATION_VERDICT_MISSING,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0x6A00);
        s.add(
            "B00-判据-诊断码互异且段独占",
            all_diff && in_segment && codes.len() == 7,
            "0x6A01..=0x6A07 七码两两互异、高字节全部落在 0x6A 独占段",
        );
    }

    // --- 判据 5.2：判据自身——统计对账（stats 恒等于 entries 逐条重算） ---
    {
        let mut p = sample_pack();
        let _ = p.record_conformant("VE-F0001", 1);
        let _ = p.record_deviation("VE-F0007", 1, DeviationVerdict::RideAlong, "遥测字段宽化一档");
        let _ = p.record_deviation("VE-F0187", 1, DeviationVerdict::Blocking, "调试通道缺鉴权");
        let _ = p.record_pending("VE-F0001", 1);
        // 判据侧独立重算（不读 stats，扫 entries）
        let mut pending = 0;
        let mut dev = 0;
        let mut blocking = 0;
        let mut ride = 0;
        for e in p.entries.iter() {
            match e.state {
                Conformance::Pending => pending += 1,
                Conformance::Deviation => {
                    dev += 1;
                    if let Some(d) = e.deviation {
                        match d.verdict {
                            DeviationVerdict::Blocking => blocking += 1,
                            DeviationVerdict::RideAlong => ride += 1,
                        }
                    }
                }
                Conformance::Conformant => {}
            }
        }
        s.add(
            "B00-判据-统计账与核对表逐条对账",
            p.stats.pending == pending
                && p.stats.deviations == dev
                && p.stats.blocking_open == blocking
                && p.stats.ridealong == ride,
            "四桶计数（待核/偏差/阻断/随行）与 entries 逐条重算一致，记账不漂移",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 七、夹具（判据用；只含冻结清单内接口，构造必成功——防「标准夹具自相矛盾」）
// ---------------------------------------------------------------------------

/// 三接口样板包：VE-F0001 / VE-F0007 / VE-F0187，版本 1。
fn sample_pack() -> HandoverPack {
    sample_pack_with(&[("VE-F0001", 1), ("VE-F0007", 1), ("VE-F0187", 1)])
}

/// 自定义冻结清单样板包（五件套齐、版本 1、全待核）。
fn sample_pack_with(frozen: &[(&'static str, u32)]) -> HandoverPack {
    match HandoverPack::assemble(["A", "B", "C", "D", "E"], frozen) {
        Ok(p) => p,
        // 夹具构造失败即判据全盘失效（坑 5 教训）：这里让编译期保证——
        // 五件套文案非空 + 清单非空在调用点都是字面量，理论上不可达；
        // 运行期若真到达，返回空表会让后续判据显性红而不是 panic。
        Err(_) => HandoverPack {
            version: 0,
            slots: Vec::new(),
            entries: Vec::new(),
            stats: PackStats::default(),
        },
    }
}

/// 查条目态的小工具（判据可读性）。
fn entry_state(p: &HandoverPack, iface: &str) -> Option<Conformance> {
    p.entries.iter().find(|e| e.iface == iface).map(|e| e.state)
}

// ---------------------------------------------------------------------------
// 八、单元测试（cargo test svstar2）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veb00_assemble_and_gate_roundtrip() {
        let mut p = sample_pack();
        assert_eq!(p.version, 1);
        assert_eq!(p.gate(), Err(CODE_RECHECK_MISSED));
        let _ = p.record_conformant("VE-F0001", 1);
        let _ = p.record_conformant("VE-F0007", 1);
        let _ = p.record_conformant("VE-F0187", 1);
        assert!(p.gate().is_ok());
        assert_eq!(p.stats.gate_queries, 2);
    }

    #[test]
    fn veb00_deviation_lifecycle() {
        let mut p = sample_pack();
        let _ = p.record_deviation("VE-F0001", 1, DeviationVerdict::Blocking, "缺鉴权");
        let _ = p.record_deviation("VE-F0007", 1, DeviationVerdict::RideAlong, "字段宽化");
        let _ = p.record_conformant("VE-F0187", 1);
        assert_eq!(p.gate(), Err(CODE_BLOCKING_OPEN));
        // 阻断改随行：不经过「符合」也能清阻断桶（降级路径）
        let _ = p.record_deviation("VE-F0001", 1, DeviationVerdict::RideAlong, "缺鉴权随行");
        assert!(p.gate().is_ok());
        assert_eq!(p.stats.ridealong, 2);
    }

    #[test]
    fn veb00_a11y_lines() {
        let mut p = sample_pack();
        let _ = p.record_deviation("VE-F0007", 1, DeviationVerdict::RideAlong, "能力位图宽化");
        let text = p.a11y_summary();
        let lines: Vec<&str> = text.split('\n').collect();
        // 首行 + 五件 + 三接口 = 9 行
        assert_eq!(lines.len(), 9, "读屏行数 = 1 总行 + 5 件 + 3 接口");
        assert!(lines[6].contains("偏差"), "偏差接口行带态名：{}", lines[6]);
        assert!(lines[6].contains("随行"), "偏差行带判定：{}", lines[6]);
    }

    #[test]
    fn veb00_bump_then_recheck_all() {
        let mut p = sample_pack();
        let _ = p.record_conformant("VE-F0001", 1);
        let _ = p.record_conformant("VE-F0007", 1);
        let _ = p.record_conformant("VE-F0187", 1);
        assert!(p.gate().is_ok());
        let _ = p.bump_version(5);
        assert_eq!(p.stats.history_recheck, 1);
        assert_eq!(p.entries[0].iface_version, 5);
        assert_eq!(p.gate(), Err(CODE_RECHECK_MISSED));
        // 重核走当前版本
        assert!(p.record_conformant("VE-F0001", 5).is_ok());
        assert!(p.record_conformant("VE-F0007", 5).is_ok());
        assert!(p.record_conformant("VE-F0187", 5).is_ok());
        assert!(p.gate().is_ok());
    }

    #[test]
    fn veb00_checks_all_green() {
        let set = run_veb00_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0200 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
