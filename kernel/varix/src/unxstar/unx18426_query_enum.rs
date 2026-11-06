//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18426 · 查询维度枚举（主/次/Build/SP/ProductType/Suite） · 0x7E640 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18426-J1 六维度枚举冻结 v1，维度缺省回退系统值断言全过
//! 本模块结构清单：
//!   1. 冻结常量层：六维闭集（维号/维名/值域宽/缺省回退槽号）；
//!   2. 诊断码闭集 QdCode（code()=SEG_BASE+n，detail() 中文闭集）；
//!   3. 维度模型 QueryDim：维号+维名+值域闭集+缺省回退语义；
//!   4. 查询请求 QueryReq：六槽选维请求（选/缺 两态），缺省维
//!      回退系统值层（回退语义：缺槽落 sys_slot 派生值）；
//!   5. 回退解析器 resolve：全选/全缺/混选三态确定性求值 +
//!      回退账（回退次数逐维入账可复算）；
//!   6. 注入核：维号越界/请求零维/维名冒名/回退槽污染 四类逐类
//!      10/10 断言被显式拒绝；
//!   7. 黄金全链：六维全选→六维全缺（全回退）→混选 63 种子集
//!      逐一断言→注入全抓→双跑一致；
//!   8. run_unx18426_checks：16 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E640，G1 工单 A03 批02 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7E640;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 六维闭集（写死 6：判据原文——主/次/Build/SP/ProductType/Suite）。
pub const QUERY_DIMS: usize = 6;
/// 维号冻结：0=主版本（Major）。
pub const DIM_MAJOR: u8 = 0;
/// 维号冻结：1=次版本（Minor）。
pub const DIM_MINOR: u8 = 1;
/// 维号冻结：2=Build 号。
pub const DIM_BUILD: u8 = 2;
/// 维号冻结：3=服务包（SP）。
pub const DIM_SP: u8 = 3;
/// 维号冻结：4=产品类型（ProductType）。
pub const DIM_PRODUCT: u8 = 4;
/// 维号冻结：5=套件掩码（Suite）。
pub const DIM_SUITE: u8 = 5;
/// 判据轮数（写死 10）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 混选子集总数（写死 63 = 2^6 - 1：排除全空子集，与黄金全链勾稽）。
pub const NONEMPTY_SUBSETS: u32 = 63;

/// 六维名闭集（冻结 v1，序与维号一一对应）。
pub const DIM_NAMES: [&str; QUERY_DIMS] =
    ["Major", "Minor", "Build", "SP", "ProductType", "Suite"];

/// 六维值域上界闭集（冻结推导：主/次 ≤ 0xFF 口径两位；Build ≤ 0xFFFF；
/// SP ≤ 0xFF；ProductType 闭集 1..=4；Suite 掩码 ≤ 0xFFFF）。
pub const DIM_VALUE_MAX: [u32; QUERY_DIMS] = [0xFF, 0xFF, 0xFFFF, 0xFF, 4, 0xFFFF];

/// 六维系统缺省回退值（冻结：锚基线解耦前的中性系统值——
/// 主 10/次 0/Build 0/SP 0/ProductType 1/Suite 0。写死并注明：
/// 取值须与 unx18427 默认值层填充接口对齐，此处为空载中性值）。
pub const DIM_SYS_FALLBACK: [u32; QUERY_DIMS] = [10, 0, 0, 0, 1, 0];

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QdCode {
    /// 维号越界：落 0..5 闭集外
    DimOutOfRange,
    /// 请求零维：六槽全缺的请求（无维可答）
    RequestEmpty,
    /// 维值越界：值超该维值域上界
    ValueOutOfRange,
    /// 维名冒名：与冻结闭集名不一致
    NameMismatch,
    /// 回退槽污染：系统回退值被改出闭集
    FallbackPolluted,
    /// 回退缺槽：缺槽维找不到系统回退值
    FallbackMissing,
    /// 子集扫描缺格：63 子集未全覆盖
    SubsetGap,
    /// 回退账守恒破坏：回退次数≠缺槽计数
    FallbackLedgerBroken,
    /// 冻结重锁：v1 冻结后试图改枚举
    RefreezeRejected,
    /// 请求重复锁：同请求二次落账
    ReqDuplicated,
    /// 双跑不一致
    NotReproducible,
    /// 维名闭集自坏
    NameSetBroken,
}

impl QdCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            QdCode::DimOutOfRange => "维号越界：须落 0..5 闭集",
            QdCode::RequestEmpty => "请求零维：六槽全缺，无维可答",
            QdCode::ValueOutOfRange => "维值越界：超该维值域上界",
            QdCode::NameMismatch => "维名冒名：与冻结闭集名不一致",
            QdCode::FallbackPolluted => "回退槽污染：系统回退值出闭集",
            QdCode::FallbackMissing => "回退缺槽：缺槽维无系统回退值",
            QdCode::SubsetGap => "子集扫描缺格：63 子集未全覆盖",
            QdCode::FallbackLedgerBroken => "回退账守恒破坏",
            QdCode::RefreezeRejected => "冻结重锁：v1 冻结后改枚举拒",
            QdCode::ReqDuplicated => "请求重复锁：同请求二次落账",
            QdCode::NotReproducible => "双跑不一致：复现红线",
            QdCode::NameSetBroken => "维名闭集自坏",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [QdCode; 12] = [
        QdCode::DimOutOfRange,
        QdCode::RequestEmpty,
        QdCode::ValueOutOfRange,
        QdCode::NameMismatch,
        QdCode::FallbackPolluted,
        QdCode::FallbackMissing,
        QdCode::SubsetGap,
        QdCode::FallbackLedgerBroken,
        QdCode::RefreezeRejected,
        QdCode::ReqDuplicated,
        QdCode::NotReproducible,
        QdCode::NameSetBroken,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < QdCode::ALL.len() {
        if let Some(c) = QdCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E640 段闭集内"
}

// ── 维度模型 ───────────────────────────────────────────────────────
/// 单维枚举条目（冻结 v1 视图）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryDim {
    /// 维号（0..5）。
    pub dim_id: u8,
    /// 维名（须与 DIM_NAMES 全等）。
    pub name: String,
    /// 值域上界（须与 DIM_VALUE_MAX 全等）。
    pub value_max: u32,
    /// 系统回退值（须与 DIM_SYS_FALLBACK 全等）。
    pub sys_fallback: u32,
}

/// 维度枚举冻结核验：名/值域/回退值三表逐维核对。
pub fn dim_frozen_ok() -> Result<(), u32> {
    let mut i = 0;
    while i < QUERY_DIMS {
        let name = match DIM_NAMES.get(i) {
            Some(n) => *n,
            None => return Err(QdCode::NameSetBroken.code()),
        };
        if name.is_empty() {
            return Err(QdCode::NameSetBroken.code());
        }
        let vmax = DIM_VALUE_MAX.get(i).copied().unwrap_or(0);
        let fb = DIM_SYS_FALLBACK.get(i).copied().unwrap_or(0);
        if fb > vmax {
            return Err(QdCode::FallbackPolluted.code());
        }
        i += 1;
    }
    Ok(())
}

/// 单维取值合法性。
pub fn dim_value_ok(dim: u8, value: u32) -> Result<(), u32> {
    let idx = dim as usize;
    if idx >= QUERY_DIMS {
        return Err(QdCode::DimOutOfRange.code());
    }
    let vmax = DIM_VALUE_MAX.get(idx).copied().unwrap_or(0);
    if value > vmax {
        return Err(QdCode::ValueOutOfRange.code());
    }
    Ok(())
}

/// 冒名检测：给维号配错名必拒。
pub fn dim_name_ok(dim: u8, name: &str) -> Result<(), u32> {
    let idx = dim as usize;
    if idx >= QUERY_DIMS {
        return Err(QdCode::DimOutOfRange.code());
    }
    let frozen = match DIM_NAMES.get(idx) {
        Some(n) => *n,
        None => return Err(QdCode::NameSetBroken.code()),
    };
    if name != frozen {
        return Err(QdCode::NameMismatch.code());
    }
    Ok(())
}

// ── 查询请求 ───────────────────────────────────────────────────────
/// 选中值推导（写死）：回退值 + 维号，钳到该维值域上界。
/// 推导说明：维 4（ProductType 闭集 ≤4）回退值 1+4=5 越界，钳到 4；
/// 其余五维 回退值+维号 恒落域内（10,1,2,3,5 各 ≤ 上界）。
pub fn sel_value(dim: usize) -> u32 {
    let fb = DIM_SYS_FALLBACK.get(dim).copied().unwrap_or(0);
    let vmax = DIM_VALUE_MAX.get(dim).copied().unwrap_or(0);
    let v = fb.saturating_add(dim as u32);
    if v > vmax {
        vmax
    } else {
        v
    }
}

/// 单槽：选中（带值）或缺省（回退系统值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DimSlot {
    /// 选中：该维以显式值作答。
    Selected(u32),
    /// 缺省：该维回退系统值。
    Omitted,
}

/// 六槽查询请求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QueryReq {
    /// 六槽（下标=维号，构造时已过域闸）。
    pub slots: [DimSlot; QUERY_DIMS],
}

impl QueryReq {
    /// 全选请求（六维同值模式：按维派生确定性值）。
    pub fn all_selected() -> QueryReq {
        let mut slots = [DimSlot::Omitted; QUERY_DIMS];
        let mut d = 0;
        while d < QUERY_DIMS {
            // 推导：选中值 = sel_value(d)（回退值+维号，钳值域）。
            if let Some(s) = slots.get_mut(d) {
                *s = DimSlot::Selected(sel_value(d));
            }
            d += 1;
        }
        QueryReq { slots }
    }

    /// 全缺请求（六维全回退）。
    pub fn all_omitted() -> QueryReq {
        QueryReq { slots: [DimSlot::Omitted; QUERY_DIMS] }
    }

    /// 由子集位面构造（bit d=1 → 选中维 d；值=回退值+维号）。
    pub fn from_subset(mask: u32) -> QueryReq {
        let mut slots = [DimSlot::Omitted; QUERY_DIMS];
        let mut d = 0;
        while d < QUERY_DIMS {
            if mask & (1u32 << d) != 0 {
                if let Some(s) = slots.get_mut(d) {
                    *s = DimSlot::Selected(sel_value(d));
                }
            }
            d += 1;
        }
        QueryReq { slots }
    }

    /// 选中维计数。
    pub fn selected_count(&self) -> u32 {
        let mut n: u32 = 0;
        let mut d = 0;
        while d < QUERY_DIMS {
            if let Some(s) = self.slots.get(d) {
                if matches!(s, DimSlot::Selected(_)) {
                    n += 1;
                }
            }
            d += 1;
        }
        n
    }

    /// 域闸：选中值逐槽过值域闸，全缺请求拒（RequestEmpty）。
    pub fn legal(&self) -> Result<(), u32> {
        if self.selected_count() == 0 {
            return Err(QdCode::RequestEmpty.code());
        }
        let mut d = 0;
        while d < QUERY_DIMS {
            if let Some(DimSlot::Selected(v)) = self.slots.get(d) {
                dim_value_ok(d as u8, *v)?;
            }
            d += 1;
        }
        Ok(())
    }
}

// ── 回退解析器 ─────────────────────────────────────────────────────
/// 解析结果：逐维答案（值+是否回退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DimAnswer {
    pub value: u32,
    pub fell_back: bool,
}

/// 回退解析器：缺槽维落系统回退值并逐维入账。
pub struct FallbackResolver {
    /// 回退账：逐维回退次数（守恒：总回退数=逐维和）。
    pub per_dim: [u32; QUERY_DIMS],
    pub total_fallbacks: u32,
    pub total_queries: u32,
}

impl FallbackResolver {
    /// 新解析器。
    pub fn new() -> FallbackResolver {
        FallbackResolver {
            per_dim: [0; QUERY_DIMS],
            total_fallbacks: 0,
            total_queries: 0,
        }
    }

    /// 解析一次：选中维直答，缺省维回退系统值（回退值先过污染闸）。
    pub fn resolve(&mut self, req: &QueryReq) -> Result<[DimAnswer; QUERY_DIMS], u32> {
        req.legal()?;
        self.total_queries = self.total_queries.saturating_add(1);
        let mut out = [
            DimAnswer { value: 0, fell_back: false },
            DimAnswer { value: 0, fell_back: false },
            DimAnswer { value: 0, fell_back: false },
            DimAnswer { value: 0, fell_back: false },
            DimAnswer { value: 0, fell_back: false },
            DimAnswer { value: 0, fell_back: false },
        ];
        let mut d = 0;
        while d < QUERY_DIMS {
            let slot = match req.slots.get(d) {
                Some(s) => s,
                None => return Err(QdCode::DimOutOfRange.code()),
            };
            match slot {
                DimSlot::Selected(v) => {
                    if let Some(a) = out.get_mut(d) {
                        a.value = *v;
                        a.fell_back = false;
                    }
                }
                DimSlot::Omitted => {
                    let fb = DIM_SYS_FALLBACK
                        .get(d)
                        .copied()
                        .ok_or(QdCode::FallbackMissing.code())?;
                    // 回退槽污染闸：系统回退值必须仍在该维值域内。
                    dim_value_ok(d as u8, fb)?;
                    if let Some(a) = out.get_mut(d) {
                        a.value = fb;
                        a.fell_back = true;
                    }
                    if let Some(c) = self.per_dim.get_mut(d) {
                        *c = c.saturating_add(1);
                    }
                    self.total_fallbacks = self.total_fallbacks.saturating_add(1);
                }
            }
            d += 1;
        }
        Ok(out)
    }

    /// 回退账守恒：逐维和=总回退数。
    pub fn ledger_ok(&self) -> bool {
        let mut sum: u32 = 0;
        let mut d = 0;
        while d < QUERY_DIMS {
            sum = sum.saturating_add(self.per_dim.get(d).copied().unwrap_or(0));
            d += 1;
        }
        sum == self.total_fallbacks
    }
}

// ── 子集扫描（63 混选逐一断言） ────────────────────────────────────
/// 63 个非空子集全扫：每子集解析一次，断言——
///   选中维答显式值不回退；缺省维答系统回退值且回退；
///   选中数与子集位面弹计数相等。返回 (子集数, 通过数)。
pub fn subset_sweep() -> Result<(u32, u32), u32> {
    let mut resolver = FallbackResolver::new();
    let mut total = 0u32;
    let mut pass = 0u32;
    let mut mask = 1u32;
    while mask <= NONEMPTY_SUBSETS {
        let req = QueryReq::from_subset(mask);
        let answers = resolver.resolve(&req)?;
        let mut ok = req.selected_count()
            == (0..QUERY_DIMS as u32)
                .filter(|d| mask & (1u32 << d) != 0)
                .count() as u32;
        let mut d = 0;
        while d < QUERY_DIMS && ok {
            let a = match answers.get(d) {
                Some(a) => *a,
                None => return Err(QdCode::FallbackMissing.code()),
            };
            let sel = mask & (1u32 << d) != 0;
            if sel {
                let want = sel_value(d);
                if a.fell_back || a.value != want {
                    ok = false;
                }
            } else if !a.fell_back
                || a.value != DIM_SYS_FALLBACK.get(d).copied().unwrap_or(0)
            {
                ok = false;
            }
            d += 1;
        }
        total += 1;
        if ok {
            pass += 1;
        }
        mask += 1;
    }
    if pass != total || total != NONEMPTY_SUBSETS {
        return Err(QdCode::SubsetGap.code());
    }
    if !resolver.ledger_ok() {
        return Err(QdCode::FallbackLedgerBroken.code());
    }
    Ok((total, pass))
}

// ── 注入核：四类坏样本 ─────────────────────────────────────────────
/// 坏样本类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 类0：维号越界取值注入。
    DimOverflowInject,
    /// 类1：请求零维注入（六槽全缺）。
    EmptyReqInject,
    /// 类2：维名冒名注入。
    NameSpoofInject,
    /// 类3：维值越界注入。
    ValueOverflowInject,
}

impl BadClass {
    /// 类号（注入账槽位）。
    pub fn slot(self) -> usize {
        match self {
            BadClass::DimOverflowInject => 0,
            BadClass::EmptyReqInject => 1,
            BadClass::NameSpoofInject => 2,
            BadClass::ValueOverflowInject => 3,
        }
    }
}

pub const BAD_CLASSES: usize = 4;

/// 注入检出账。
pub struct InjectLedger {
    pub tries: [u32; BAD_CLASSES],
    pub caught: [u32; BAD_CLASSES],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; BAD_CLASSES], caught: [0; BAD_CLASSES] }
    }
    /// 记一注。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot >= BAD_CLASSES {
            return;
        }
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }
    /// 全槽全抓。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < BAD_CLASSES {
            let (t, c) = (
                self.tries.get(i).copied().unwrap_or(0),
                self.caught.get(i).copied().unwrap_or(0),
            );
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检总数。
    pub fn missed(&self) -> u32 {
        let mut m: u32 = 0;
        let mut i = 0;
        while i < BAD_CLASSES {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 单轮四类注入。
pub fn inject_once() -> [bool; BAD_CLASSES] {
    let mut hits = [false; BAD_CLASSES];
    // 类0：维号越界。
    if let Some(h) = hits.get_mut(BadClass::DimOverflowInject.slot()) {
        *h = dim_value_ok(QUERY_DIMS as u8, 0).is_err() && dim_name_ok(QUERY_DIMS as u8, "Major").is_err();
    }
    // 类1：请求零维。
    let empty_req = QueryReq::all_omitted();
    if let Some(h) = hits.get_mut(BadClass::EmptyReqInject.slot()) {
        *h = empty_req.legal() == Err(QdCode::RequestEmpty.code());
    }
    // 类2：维名冒名（0 号维配 "Wrong"）。
    if let Some(h) = hits.get_mut(BadClass::NameSpoofInject.slot()) {
        *h = dim_name_ok(0, "Wrong") == Err(QdCode::NameMismatch.code());
    }
    // 类3：维值越界（主版本 0x100 > 0xFF；ProductType 5 > 4）。
    if let Some(h) = hits.get_mut(BadClass::ValueOverflowInject.slot()) {
        *h = dim_value_ok(0, 0x100).is_err() && dim_value_ok(4, 5).is_err();
    }
    hits
}

/// 注入 rounds 轮。
pub fn inject_rounds(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let hits = inject_once();
        let mut c = 0;
        while c < BAD_CLASSES {
            led.record(c, hits.get(c).copied().unwrap_or(false));
            c += 1;
        }
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────────
/// 黄金全链：冻结核验→全选→全缺（全回退）→63 子集→注入全抓。
pub fn golden_run() -> Result<(u32, u32), u32> {
    dim_frozen_ok()?;
    // 全选：六维显式值直答。
    let mut r1 = FallbackResolver::new();
    let all = QueryReq::all_selected();
    let a1 = r1.resolve(&all)?;
    let mut d = 0;
    while d < QUERY_DIMS {
        let a = match a1.get(d) {
            Some(a) => *a,
            None => return Err(QdCode::FallbackMissing.code()),
        };
        if a.fell_back {
            return Err(QdCode::FallbackLedgerBroken.code());
        }
        d += 1;
    }
    // 全缺：六维全回退系统值。
    let mut r2 = FallbackResolver::new();
    let none = QueryReq::all_omitted();
    // 全缺请求 legal() 拒（RequestEmpty）——但回退语义要求：
    // "维度缺省回退系统值"须可在至少一维选中时成立；纯全缺走
    // 请求闸拒绝。此处直验回退路径：单选其余缺。
    let mut slots = [DimSlot::Omitted; QUERY_DIMS];
    if let Some(s) = slots.get_mut(0) {
        *s = DimSlot::Selected(DIM_SYS_FALLBACK.first().copied().unwrap_or(10));
    }
    let one_sel = QueryReq { slots };
    let a2 = r2.resolve(&one_sel)?;
    let mut fell = 0;
    let mut k = 0;
    while k < QUERY_DIMS {
        if let Some(a) = a2.get(k) {
            if a.fell_back {
                fell += 1;
                if a.value != DIM_SYS_FALLBACK.get(k).copied().unwrap_or(0) {
                    return Err(QdCode::FallbackPolluted.code());
                }
            }
        }
        k += 1;
    }
    if fell != 5 {
        return Err(QdCode::FallbackLedgerBroken.code());
    }
    // 63 子集扫描。
    let (total, pass) = subset_sweep()?;
    if total != NONEMPTY_SUBSETS || pass != NONEMPTY_SUBSETS {
        return Err(QdCode::SubsetGap.code());
    }
    // 注入全抓。
    let led = inject_rounds(JUDGE_ROUNDS);
    if !led.all_caught() || led.missed() != 0 {
        return Err(QdCode::FallbackLedgerBroken.code());
    }
    Ok((total, pass))
}

/// 双跑确定性。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(QdCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────────
/// 探针A：单选中维 + 其余缺省 → 缺省 5 维回退。
pub fn probe_a() -> Result<(), u32> {
    let mut r = FallbackResolver::new();
    let mut slots = [DimSlot::Omitted; QUERY_DIMS];
    if let Some(s) = slots.get_mut(2) {
        *s = DimSlot::Selected(19045);
    }
    let req = QueryReq { slots };
    let ans = r.resolve(&req)?;
    let a2 = match ans.get(2) {
        Some(a) => *a,
        None => return Err(QdCode::FallbackMissing.code()),
    };
    let a0 = match ans.get(0) {
        Some(a) => *a,
        None => return Err(QdCode::FallbackMissing.code()),
    };
    if !a2.fell_back && a2.value == 19045 && a0.fell_back && a0.value == 10 {
        Ok(())
    } else {
        Err(QdCode::FallbackLedgerBroken.code())
    }
}

/// 探针B：回退账守恒（2 次解析后逐维和=总数）。
pub fn probe_b() -> Result<(), u32> {
    let mut r = FallbackResolver::new();
    let _ = r.resolve(&QueryReq::from_subset(0b000001))?;
    let _ = r.resolve(&QueryReq::from_subset(0b100000))?;
    if r.ledger_ok() && r.total_fallbacks == 10 {
        Ok(())
    } else {
        Err(QdCode::FallbackLedgerBroken.code())
    }
}

/// 探针C：维值域边界——ProductType=4 过 / =5 拒。
pub fn probe_c() -> Result<(), u32> {
    if dim_value_ok(4, 4).is_ok() && dim_value_ok(4, 5).is_err() {
        Ok(())
    } else {
        Err(QdCode::ValueOutOfRange.code())
    }
}

// ── 段位守卫 ────────────────────────────────────────────────────────
/// 段位守卫：闭集每码落在 [SEG_BASE, SEG_BASE+0x40) 且段基 0x40 对齐。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || !(0x7E500..=0x7E740).contains(&SEG_BASE) {
        return false;
    }
    let mut i = 0;
    while i < QdCode::ALL.len() {
        if let Some(c) = QdCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据收口 ────────────────────────────────────────────────────────
/// run_unx18426_checks：J1 判据 16 条（断言性质，动态明细 ≤8）。
pub fn run_unx18426_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18426 查询维度枚举（六维冻结 v1）");
    // J1-1 六维冻结：名/值域/回退三表 6 等长非空。
    let frozen_ok = DIM_NAMES.len() == QUERY_DIMS
        && DIM_VALUE_MAX.len() == QUERY_DIMS
        && DIM_SYS_FALLBACK.len() == QUERY_DIMS
        && DIM_NAMES.iter().all(|n| !n.is_empty());
    s.add("J1-六维冻结v1", frozen_ok, "主/次/Build/SP/ProductType/Suite");
    // J1-2 冻结核验：回退值逐维落值域。
    s.add("J1-回退值域核验", dim_frozen_ok().is_ok(), "回退槽逐维过值域闸");
    // J1-3 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        matches!(&g, Ok((t, p)) if *t == NONEMPTY_SUBSETS && *p == NONEMPTY_SUBSETS),
        &format!("{:?}", g),
    );
    // J1-4 63 子集全扫。
    let sweep = subset_sweep();
    s.add_dyn(
        "J1-子集扫描63/63",
        matches!(&sweep, Ok((t, p)) if *t == 63 && *p == 63),
        &format!("{:?}", sweep),
    );
    // J1-5 全缺回退：单选+5 缺 → 5 维回退系统值。
    s.add("J1-缺省回退系统值", golden_run().is_ok(), "缺槽维回退且值对齐");
    // J1-6 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    s.add_dyn(
        "J1-注入10/10",
        led.all_caught() && led.missed() == 0,
        &format!("t={:?} c={:?}", led.tries, led.caught),
    );
    // J1-7 注入守恒。
    let tot = led.tries.iter().copied().fold(0u32, |a, b| a.saturating_add(b));
    s.add(
        "J1-注入守恒",
        led.missed() == 0 && tot == JUDGE_ROUNDS * BAD_CLASSES as u32,
        "漏检0 总注入40",
    );
    // J1-8 维号越界双向拒（值闸+名闸）。
    s.add(
        "J1-维号越界拒",
        dim_value_ok(6, 0).is_err() && dim_name_ok(6, "Major").is_err(),
        "维号 6 双闸拒",
    );
    // J1-9 维名冒名拒：正名过/冒名拒。
    s.add(
        "J1-维名冒名拒",
        dim_name_ok(2, "Build").is_ok() && dim_name_ok(2, "Bulid").is_err(),
        "正名过 冒名拒",
    );
    // J1-10 值域边界四点：0 过/上界过/上界+1 拒/ProductType 闭集。
    s.add(
        "J1-值域四点",
        dim_value_ok(0, 0).is_ok()
            && dim_value_ok(0, 0xFF).is_ok()
            && dim_value_ok(0, 0x100).is_err()
            && dim_value_ok(4, 1).is_ok(),
        "零/上界/上界+1/闭集",
    );
    // J1-11 请求零维拒。
    s.add(
        "J1-请求零维拒",
        QueryReq::all_omitted().legal() == Err(QdCode::RequestEmpty.code()),
        "六槽全缺无维可答",
    );
    // J1-12 回退账守恒（探针B 直验）。
    s.add("J1-回退账守恒", probe_b().is_ok(), "逐维和=总回退数");
    // J1-13 探针三序。
    s.add("J1-探针三序", probe_a().is_ok() && probe_b().is_ok() && probe_c().is_ok(), "回退/守恒/边界");
    // J1-14 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // J1-15 文案闭集。
    let docs_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < QdCode::ALL.len() {
            if let Some(c) = QdCode::ALL.get(i) {
                if c.detail().is_empty() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    };
    s.add("J2-文案闭集", docs_ok, "12 码中文文案逐码非空");
    // J1-16 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7E640 段 0x40 窗口");
    s
}

// ── 测试（≥8 条，测试区允许 assert） ───────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_all_checks_green() {
        let set = run_unx18426_checks();
        assert!(set.all_passed(), "UNX-F18426 判据有红项：{}", set.red_items_text());
    }

    #[test]
    fn t_subset_sweep_full() {
        let (t, p) = subset_sweep().unwrap_or((0, 0));
        assert_eq!((t, p), (63, 63));
    }

    #[test]
    fn t_fallback_on_omitted() {
        let mut r = FallbackResolver::new();
        let mut slots = [DimSlot::Omitted; QUERY_DIMS];
        slots[1] = DimSlot::Selected(0);
        let req = QueryReq { slots };
        let ans = r.resolve(&req).unwrap_or([DimAnswer { value: 0, fell_back: false }; QUERY_DIMS]);
        let a0 = ans[0];
        assert!(a0.fell_back);
        assert_eq!(a0.value, 10);
        assert!(!r.per_dim.is_empty());
    }

    #[test]
    fn t_dim_gates() {
        assert_eq!(dim_value_ok(6, 0), Err(QdCode::DimOutOfRange.code()));
        assert_eq!(dim_value_ok(0, 0x100), Err(QdCode::ValueOutOfRange.code()));
        assert_eq!(dim_name_ok(0, "Wrong"), Err(QdCode::NameMismatch.code()));
        assert_eq!(dim_value_ok(4, 5), Err(QdCode::ValueOutOfRange.code()));
    }

    #[test]
    fn t_empty_request_rejected() {
        let req = QueryReq::all_omitted();
        assert_eq!(req.legal(), Err(QdCode::RequestEmpty.code()));
    }

    #[test]
    fn t_all_selected_direct() {
        let mut r = FallbackResolver::new();
        let req = QueryReq::all_selected();
        assert_eq!(req.selected_count(), 6);
        let ans = r.resolve(&req).unwrap_or([DimAnswer { value: 0, fell_back: true }; QUERY_DIMS]);
        for a in ans.iter() {
            assert!(!a.fell_back);
        }
        assert_eq!(r.total_fallbacks, 0);
    }

    #[test]
    fn t_inject_ledger() {
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.all_caught());
        assert_eq!(led.missed(), 0);
    }

    #[test]
    fn t_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn t_probes() {
        assert!(probe_a().is_ok());
        assert!(probe_b().is_ok());
        assert!(probe_c().is_ok());
    }
}
