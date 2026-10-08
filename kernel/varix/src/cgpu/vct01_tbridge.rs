//! CGPU-F3041 · T 域开工与 VE 对接总架构（CGPU-T 域 · T01 域开工与 VE 对接总架构组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3041`
//!
//! **判据（锚点原文）**：签收、定位声明、四段、兑现确认、四组、判据。
//!
//! **职责定位（锚点原文）**：域开工：T 域开工（签收 S10 移交包——VE 对接
//! 衔接包签收即衔接确认）；对接定位（VE 引擎与 CGPU 加速器的协议桥梁
//! ——定位声明）；四段架构（命令/状态/资产/事件——S10 预告兑现即兑现
//! 确认）；测试（签收/定位/四段/兑现四组）。
//!
//! # 一、签收即确认：S10 移交包七件逐件对账
//!
//! S 域收官移交包七件：①接口冻结清单（71 函数）②契约清单③资产清单
//! ④基线快照⑤遗留移交清单⑥**VE 对接衔接包**（多用户会话上下文在对接
//! 协议中的传递约定）⑦经验教训十条。[ack_handover] 只在七件齐时出签收
//! ——缺件签收是空头支票。衔接包的实质约定落为 [`SessionCtxTag`]：
//! 对接协议每条命令携带会话上下文标签，S 域「隔离不串户」纪律在跨域
//! 通路上的延续——上下文传递不串户。
//!
//! # 二、定位声明：协议桥梁，不是替代
//!
//! VE 引擎与 CGPU 加速器的关系是**桥梁**：VE 侧发起命令、CGPU 侧执行
//! 加速、状态经桥回传——CGPU 是 VE 的加速器而非平行引擎（定位声明，
//! [BRIDGE_POSITIONING] 在册）。桥梁语义决定四段职责的划分方向。
//!
//! # 三、四段架构：S10 预告的四段，T 组四批兑现
//!
//! 命令/状态/资产/事件四段闭集（S10 预告）——兑现确认的实质是**预告
//! 与批次的一一对应可机检**：命令→T02 命令通道协议（F3042 组）、状态→
//! T03 状态同步协议、资产→T04 资产互操作协议、事件→T05 事件流协议
//! （[SEGMENT_PLAN] 在册）。四段互异、四批区间无缝——预告不落空。
//!
//! # 四、预算在册：0.1ms 桥路预算
//!
//! 对接桥路的单命令处理预算 0.1ms（[BRIDGE_BUDGET_NS]，S81 小结在册）。
//! 预算是契约不是感觉：超预算即告警信号（F1416 预算理念）。
//!
//! ## 错误契约：独占 0x5A 细分段（0x52-0x59 为既有 cgpu 域占用）
//!
//! 零 panic 面：表驱动 + `Option`/`Result`，无 `unwrap`/`expect`。

// ===========================================================================
// 一、版本与诊断码（vct01 独占 0x5Axx 段）
// ===========================================================================

/// 版本标识（家族格式）。
pub const TBRIDGE_VERSION: &str = "T01-tbridge-v1";

/// 桥路单命令处理预算（纳秒；0.1ms——S81 小结在册口径）。
pub const BRIDGE_BUDGET_NS: u32 = 100_000;

/// 对接桥域诊断码（独占段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TbCode(pub u16);

impl TbCode {
    /// 移交包签收缺件（七件未齐不得签收）。
    pub const HANDOVER_INCOMPLETE: TbCode = TbCode(0x5A01);
    /// 会话上下文标签非法（session_id/user_tag 为零值哨兵）。
    pub const SESSION_TAG: TbCode = TbCode(0x5A02);
    /// 段序非法（四段闭集之外）。
    pub const SEGMENT_INVALID: TbCode = TbCode(0x5A03);
    /// 预算口径非法（0 纳秒预算无法承载任何命令）。
    pub const BUDGET: TbCode = TbCode(0x5A04);
    /// 命令信封不完整（缺段标识或缺会话标签）。
    pub const ENVELOPE: TbCode = TbCode(0x5A05);

    /// 短码。
    pub const fn code(self) -> u16 {
        self.0
    }
}

// ===========================================================================
// 二、S10 移交包签收（判据一：签收）
// ===========================================================================

/// 移交包件目（S 域收官移交包七件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoverItem {
    /// 件序（1..=7）。
    pub seq: u8,
    /// 件名。
    pub name: &'static str,
}

/// S10 移交包七件清单（F3040 收官在册口径；⑥为 VE 对接衔接包）。
pub const HANDOVER_MANIFEST: [HandoverItem; 7] = [
    HandoverItem { seq: 1, name: "接口冻结清单(71函数)" },
    HandoverItem { seq: 2, name: "契约清单" },
    HandoverItem { seq: 3, name: "资产清单" },
    HandoverItem { seq: 4, name: "基线快照" },
    HandoverItem { seq: 5, name: "遗留移交清单" },
    HandoverItem { seq: 6, name: "VE对接衔接包(会话上下文传递约定)" },
    HandoverItem { seq: 7, name: "经验教训十条" },
];

/// 签收单：七件齐备的确认产物。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoverAck {
    /// 签收件数（恒 7——齐件才出单）。
    pub acked: u8,
    /// 本单为 T01 域开工签收（对端=S10 收官）。
    pub from_batch: &'static str,
}

/// 签收：收到的件序集合必须**覆盖七件且无越界**——缺件不签、
/// 重复件序按集合语义去重后仍须齐七件。衔接确认 = 七件逐件在册。
pub fn ack_handover(received_seqs: &[u8]) -> Result<HandoverAck, TbCode> {
    let mut seen = [false; 7];
    let mut i = 0usize;
    while i < received_seqs.len() {
        let s = received_seqs[i] as usize;
        if s < 1 || s > 7 {
            // 越界件序不是移交包内容，静默吞掉=账实不符，直接拒。
            return Err(TbCode::HANDOVER_INCOMPLETE);
        }
        seen[s - 1] = true;
        i += 1;
    }
    let mut n = 0u8;
    let mut k = 0usize;
    while k < 7 {
        if seen[k] {
            n += 1;
        }
        k += 1;
    }
    if n < 7 {
        return Err(TbCode::HANDOVER_INCOMPLETE);
    }
    Ok(HandoverAck {
        acked: n,
        from_batch: "S10",
    })
}

// ===========================================================================
// 三、会话上下文标签（衔接包约定：上下文传递不串户）
// ===========================================================================

/// 会话上下文标签（S10 衔接包约定：对接协议每条命令携带，S 域隔离
/// 纪律的跨域延续——session_id 零值与 user_tag 零值都是哨兵位非法）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SessionCtxTag {
    /// 会话标识（非零）。
    pub session_id: u32,
    /// 用户标识（非零；S 域「隔离不串户」的跨域携带面）。
    pub user_tag: u32,
}

/// 标签合法性：双非零。零值哨兵用于「未携带」态的显性识别。
pub fn ctx_tag_ok(t: &SessionCtxTag) -> bool {
    t.session_id != 0 && t.user_tag != 0
}

/// 命令信封：进桥的每条命令 = 段标识 + 会话标签（信封不完整即拒，
/// 校验在入桥前一次完成——运行期热路径零校验纪律的入桥版）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CmdEnvelope {
    /// 段标识索引（0..=3 对应四段；闭集外=非法）。
    pub segment: u8,
    /// 会话上下文标签。
    pub ctx: SessionCtxTag,
}

/// 信封校验：段在闭集内且标签合法。
pub fn envelope_ok(e: &CmdEnvelope) -> bool {
    e.segment < 4 && ctx_tag_ok(&e.ctx)
}

// ===========================================================================
// 四、对接定位声明（判据二：定位声明）
// ===========================================================================

/// 定位声明三元组（在册、判据逐句 grep 可查）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Positioning {
    /// 发起方：VE 引擎（命令的来源）。
    pub initiator: &'static str,
    /// 执行方：CGPU 加速器（命令的加速执行）。
    pub executor: &'static str,
    /// 关系定位：协议桥梁（加速而非替代）。
    pub relation: &'static str,
}

/// 对接定位在册（锚点原文「VE 引擎与 CGPU 加速器的协议桥梁」逐字落位）。
pub const BRIDGE_POSITIONING: Positioning = Positioning {
    initiator: "VE引擎",
    executor: "CGPU加速器",
    relation: "协议桥梁(加速而非替代)",
};

// ===========================================================================
// 五、四段架构与兑现确认（判据三/四：四段、兑现确认）
// ===========================================================================

/// 四段闭集：命令/状态/资产/事件（S10 预告口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Segment {
    /// 命令段：VE→CGPU 的加速命令（→T02 命令通道协议）。
    Command,
    /// 状态段：CGPU→VE 的状态同步（→T03 状态同步协议）。
    State,
    /// 资产段：纹理/网格等资产的互操作（→T04 资产互操作协议）。
    Asset,
    /// 事件段：异步事件回流（→T05 事件流协议）。
    Event,
}

impl Segment {
    /// 段序（0..=3）→ 段（闭集外 None——非法段序不猜测）。
    pub fn from_index(i: u8) -> Option<Segment> {
        match i {
            0 => Some(Segment::Command),
            1 => Some(Segment::State),
            2 => Some(Segment::Asset),
            3 => Some(Segment::Event),
            _ => None,
        }
    }

    /// 段名（判据 grep 面）。
    pub const fn name(self) -> &'static str {
        match self {
            Segment::Command => "命令",
            Segment::State => "状态",
            Segment::Asset => "资产",
            Segment::Event => "事件",
        }
    }
}

/// 四段→四批兑现映射行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegmentPlanRow {
    /// 段。
    pub seg: Segment,
    /// 兑现批次（T 组）。
    pub batch: &'static str,
    /// 批次首单号。
    pub first_id: u16,
    /// 批次末单号。
    pub last_id: u16,
    /// 段职责一句话。
    pub duty: &'static str,
}

/// S10 预告四段 → T 组四批兑现计划（区间无缝、预告兑现的机检面）。
pub const SEGMENT_PLAN: [SegmentPlanRow; 4] = [
    SegmentPlanRow {
        seg: Segment::Command,
        batch: "T02命令通道协议",
        first_id: 3042,
        last_id: 3057,
        duty: "VE发起的加速命令经命令段入桥",
    },
    SegmentPlanRow {
        seg: Segment::State,
        batch: "T03状态同步协议",
        first_id: 3058,
        last_id: 3073,
        duty: "CGPU执行状态经状态段回传",
    },
    SegmentPlanRow {
        seg: Segment::Asset,
        batch: "T04资产互操作协议",
        first_id: 3074,
        last_id: 3089,
        duty: "纹理网格等资产双向互操作",
    },
    SegmentPlanRow {
        seg: Segment::Event,
        batch: "T05事件流协议",
        first_id: 3090,
        last_id: 3105,
        duty: "异步事件经事件段回流VE",
    },
];

/// 段→兑现行（闭集内必有映射；无映射=预告落空即缺陷）。
pub fn segment_plan(seg: Segment) -> Option<&'static SegmentPlanRow> {
    let mut i = 0usize;
    while i < SEGMENT_PLAN.len() {
        if SEGMENT_PLAN[i].seg == seg {
            return Some(&SEGMENT_PLAN[i]);
        }
        i += 1;
    }
    None
}

/// 兑现确认审计：四段互异 + 四批区间无缝衔接（前段末+1=后段首）
/// + 全部落在 T01 域开工后的 T 组单号带内——预告落空可机检。
pub fn plan_audit() -> bool {
    // 四段互异（两两不等）。
    let mut i = 0usize;
    while i < SEGMENT_PLAN.len() {
        let mut j = i + 1;
        while j < SEGMENT_PLAN.len() {
            if SEGMENT_PLAN[i].seg == SEGMENT_PLAN[j].seg {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 区间合法且无缝：首<末、相邻前末+1=后首。
    let mut k = 1usize;
    while k < SEGMENT_PLAN.len() {
        let prev = &SEGMENT_PLAN[k - 1];
        let cur = &SEGMENT_PLAN[k];
        if prev.first_id > prev.last_id || cur.first_id > cur.last_id {
            return false;
        }
        if prev.last_id + 1 != cur.first_id {
            return false;
        }
        k += 1;
    }
    // 兑现带起点：命令段首单必须紧跟本单（F3041 开工 → F3042 命令段）。
    SEGMENT_PLAN[0].first_id == 3042
}

// ===========================================================================
// 六、预算口径（0.1ms 在册——超预算是告警信号不是静默）
// ===========================================================================

/// 预算核验：给定单命令实测耗时（纳秒口径整数）是否在桥路预算内。
/// 预算 0 无法核验（无预算=无契约），显性拒绝。
pub fn within_budget(cost_ns: u32, budget_ns: u32) -> Result<bool, TbCode> {
    if budget_ns == 0 {
        return Err(TbCode::BUDGET);
    }
    Ok(cost_ns <= budget_ns)
}

/// 桥路自检入口签名（T01 组 API v1 的第一签名；后续批次在此追加）。
pub fn bridge_ready(envelopes: &[CmdEnvelope]) -> Result<usize, TbCode> {
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < envelopes.len() {
        if !envelope_ok(&envelopes[i]) {
            return Err(TbCode::ENVELOPE);
        }
        ok += 1;
        i += 1;
    }
    Ok(ok)
}
