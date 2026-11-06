//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18410 · GetProductInfo 产品 SKU 语义
//! · 0x7E240 段 · 批 01 · 配额 320
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18410-J1 SKU 枚举（PRODUCT_*）映射表冻结 v1，未登记 SKU
//!   回退 PRODUCT_ESSENTIAL_BUSINESS 断言全过。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - SKU 表冻结 v1：21 个 PRODUCT_* 登记码（0x0001..0x0010 主谱 +
//!     0x0013/0x0014/0x0018/0x001A/0x0030 扩展谱），行序严格递增无重复；
//!   - 回退哨兵：未登记码一律回退 PRODUCT_ESSENTIAL_BUSINESS(0x0023)，
//!     且必须带 fallback 出处标记（绝不冒充已登记直通）；
//!   - GetProductInfo 五参闸：os 版本闸（6.0 起支持，5.x 显式拒）+
//!     reserved 闸（第 5 参恒 0）+ 表冻结闸（封表后注册拒）；
//!   - 确定性：同码双查逐字段一致；导出表按码升序、双跑逐行相等；
//!   - 状态机：SkuGate 两态 Open → Sealed 单向；封印后一切查询被拒；
//!   - 注入核：未登记回退 / 5.x 版本拒 / reserved 非 0 / 冻结后注册
//!     四类 × 10 轮全抓；
//!   - 账本：SkuLedger（查询/登记收/回退/版本拒/保留拒/冻结拒，守恒）；
//!   - no_std + alloc；零 unwrap/expect/panic（生产区）；禁浮点/IO/墙钟。
//! 本模块结构清单：
//!   [错误路径] SkCode 闭集（10 码）+ ALL + detail() + detail(code)
//!   [裁决表]   SKU_TABLE 21 行冻结（码 + 名）+ 回退哨兵 0x0023
//!   [数据结构] SkuInfo（出处标记）/ SkuQuery（五参）
//!   [状态机]   GatePhase 两态单向 + SkuGate
//!   [账本]     SkuLedger（守恒核算）
//!   [注入核]   四类违约 ×10 轮（回退/版本/保留/冻结后注册）
//!   [收口]     golden_run + run_unx18410_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E240，AI-VX11-1105 G1 批01 独占区）。
pub const SEG_BASE: u32 = 0x7E240;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// SKU 登记行数（冻结 v1：21 行）。
pub const SKU_ROWS: usize = 21;
/// 回退哨兵：PRODUCT_ESSENTIAL_BUSINESS（J1 口径，未登记码的回退目标）。
pub const PRODUCT_ESSENTIAL_BUSINESS: u32 = 0x0023;
/// os 版本闸下界（6.0 起 GetProductInfo 支持，5.x 拒）。
pub const OS_MAJOR_MIN: u32 = 6;
/// 判据轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// 未登记注入码族（冻结 6 变体轮换）。
pub const UNREG_VARIANTS: usize = 6;

// ── SKU 冻结表（v1：码 + 名，行序严格递增） ─────────────────────
/// 第 i 行（码，名）。越界 None。
/// 推导：0x0001..0x0010 为 Vista/2008 主谱连续段；0x0013 起为扩展谱。
pub fn sku_row(i: usize) -> Option<(u32, &'static str)> {
    const T: [(u32, &str); SKU_ROWS] = [
        (0x0001, "PRODUCT_ULTIMATE"),
        (0x0002, "PRODUCT_HOME_BASIC"),
        (0x0003, "PRODUCT_HOME_PREMIUM"),
        (0x0004, "PRODUCT_ENTERPRISE"),
        (0x0005, "PRODUCT_HOME_BASIC_N"),
        (0x0006, "PRODUCT_BUSINESS"),
        (0x0007, "PRODUCT_STANDARD_SERVER"),
        (0x0008, "PRODUCT_DATACENTER_SERVER"),
        (0x0009, "PRODUCT_SMALLBUSINESS_SERVER"),
        (0x000A, "PRODUCT_ENTERPRISE_SERVER"),
        (0x000B, "PRODUCT_STARTER"),
        (0x000C, "PRODUCT_DATACENTER_SERVER_CORE"),
        (0x000D, "PRODUCT_STANDARD_SERVER_CORE"),
        (0x000E, "PRODUCT_ENTERPRISE_SERVER_CORE"),
        (0x000F, "PRODUCT_ENTERPRISE_SERVER_IA64"),
        (0x0010, "PRODUCT_BUSINESS_N"),
        (0x0013, "PRODUCT_HOME_SERVER"),
        (0x0014, "PRODUCT_SMALLBUSINESS_SERVER_PREMIUM"),
        (0x0018, "PRODUCT_SERVER_FOR_SMALLBUSINESS"),
        (0x001A, "PRODUCT_HOME_PREMIUM_N"),
        (0x0030, "PRODUCT_PROFESSIONAL"),
    ];
    if i >= SKU_ROWS {
        return None;
    }
    Some(T[i])
}

/// 表内查找：登记码 → 行号；未登记 → None（走回退）。
pub fn sku_lookup(code: u32) -> Option<usize> {
    let mut i = 0;
    while i < SKU_ROWS {
        if let Some((c, _)) = sku_row(i) {
            if c == code {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SkCode {
    /// os 版本不支持（5.x 及以下）。
    VersionUnsupported,
    /// reserved 参非 0。
    ReservedNonzero,
    /// 表已冻结仍企图注册。
    TableFrozen,
    /// 回退缺出处（未登记码未带 fallback 标记 = 直通漏网）。
    FallbackUnmarked,
    /// 表序破坏（码非严格递增 / 重复）。
    TableOrder,
    /// 封印后查询被拒。
    SealedQuery,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 表行越界（防御）。
    RowOutOfRange,
    /// 双跑不一致。
    NotReproducible,
    /// 映射失真（同码双查不一致）。
    MapInconsistent,
}

impl SkCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SkCode::VersionUnsupported => "os 版本不支持：5.x 走 GetVersionEx 面",
            SkCode::ReservedNonzero => "reserved 参非 0：第五参恒零",
            SkCode::TableFrozen => "表已冻结：注册企图拒",
            SkCode::FallbackUnmarked => "回退缺出处：未登记码未标 fallback",
            SkCode::TableOrder => "表序破坏：码非严格递增或重复",
            SkCode::SealedQuery => "封印后查询被拒",
            SkCode::LedgerBroken => "账本守恒破坏",
            SkCode::RowOutOfRange => "表行越界读",
            SkCode::NotReproducible => "双跑不一致：复现红线",
            SkCode::MapInconsistent => "映射失真：同码双查不一致",
        }
    }
    pub const ALL: [SkCode; 10] = [
        SkCode::VersionUnsupported,
        SkCode::ReservedNonzero,
        SkCode::TableFrozen,
        SkCode::FallbackUnmarked,
        SkCode::TableOrder,
        SkCode::SealedQuery,
        SkCode::LedgerBroken,
        SkCode::RowOutOfRange,
        SkCode::NotReproducible,
        SkCode::MapInconsistent,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SkCode::ALL.len() {
        if SkCode::ALL[i].code() == code {
            return SkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E240 段闭集内"
}

// ── 数据结构 ────────────────────────────────────────────────────
/// SKU 查询（GetProductInfo 五参投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SkuQuery {
    pub os_major: u32,
    pub os_minor: u32,
    pub sp_major: u32,
    pub sp_minor: u32,
    /// 第五参恒 0（Windows 口径保留位）。
    pub reserved: u32,
}

/// 映射结果（含出处标记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SkuInfo {
    /// 登记码或回退哨兵。
    pub code: u32,
    /// 名（登记名 / "PRODUCT_ESSENTIAL_BUSINESS" 回退名）。
    pub name: &'static str,
    /// 是否命中登记表。
    pub registered: bool,
    /// 是否走了回退（未登记码的出处标记，绝不直通）。
    pub fallback: bool,
}

/// 账本（守恒：查询 = 登记收 + 回退收 + 版本拒 + 保留拒 + 冻结拒）。
#[derive(Clone, Copy, Debug)]
pub struct SkuLedger {
    pub tries: u32,
    pub registered_ok: u32,
    pub fallback_ok: u32,
    pub version_rej: u32,
    pub reserved_rej: u32,
    pub frozen_rej: u32,
}

impl SkuLedger {
    pub fn new() -> SkuLedger {
        SkuLedger {
            tries: 0,
            registered_ok: 0,
            fallback_ok: 0,
            version_rej: 0,
            reserved_rej: 0,
            frozen_rej: 0,
        }
    }
    pub fn conserve(&self) -> bool {
        self.tries
            == self.registered_ok
                + self.fallback_ok
                + self.version_rej
                + self.reserved_rej
                + self.frozen_rej
    }
}

// ── 状态机（SkuGate：Open → Sealed 单向） ───────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GatePhase {
    /// 开放查询。
    Open,
    /// 已封印（只读）。
    Sealed,
}

/// SKU 查询闸门：开放期收查询账，封印后查询被拒。
pub struct SkuGate {
    phase: GatePhase,
    pub ledger: SkuLedger,
    /// 已通过映射的结果账（按序）。
    pub results: Vec<SkuInfo>,
}

impl SkuGate {
    pub fn new() -> SkuGate {
        SkuGate { phase: GatePhase::Open, ledger: SkuLedger::new(), results: Vec::new() }
    }
    pub fn phase(&self) -> GatePhase {
        self.phase
    }
    /// 查询一笔（Open 态；Sealed 态拒）。
    pub fn query(&mut self, q: &SkuQuery) -> Result<SkuInfo, u32> {
        self.ledger.tries += 1;
        if self.phase != GatePhase::Open {
            self.ledger.frozen_rej += 1;
            return Err(SkCode::SealedQuery.code());
        }
        // 版本闸：5.x 及以下不支持（GetProductInfo 自 Vista 起）。
        if q.os_major < OS_MAJOR_MIN {
            self.ledger.version_rej += 1;
            return Err(SkCode::VersionUnsupported.code());
        }
        // reserved 闸。
        if q.reserved != 0 {
            self.ledger.reserved_rej += 1;
            return Err(SkCode::ReservedNonzero.code());
        }
        // 表映射（登记直取 / 未登记回退）。
        let info = match sku_lookup(sku_code_of(q)) {
            Some(row) => {
                let (c, n) = match sku_row(row) {
                    Some(x) => x,
                    None => return Err(SkCode::RowOutOfRange.code()),
                };
                self.ledger.registered_ok += 1;
                SkuInfo { code: c, name: n, registered: true, fallback: false }
            }
            None => {
                self.ledger.fallback_ok += 1;
                SkuInfo {
                    code: PRODUCT_ESSENTIAL_BUSINESS,
                    name: "PRODUCT_ESSENTIAL_BUSINESS",
                    registered: false,
                    fallback: true,
                }
            }
        };
        self.results.push(info);
        Ok(info)
    }
    /// 封印（Open → Sealed 单向）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != GatePhase::Open {
            return Err(SkCode::SealedQuery.code());
        }
        if !self.ledger.conserve() {
            return Err(SkCode::LedgerBroken.code());
        }
        self.phase = GatePhase::Sealed;
        Ok(())
    }
}

/// 从查询取 SKU 码（GetProductInfo 的 pdwReturnedProductType 入参位：
/// 本域冻结：SKU 码随 os_minor 编码——os_minor ∈ 0..0x40 为码槽）。
fn sku_code_of(q: &SkuQuery) -> u32 {
    q.os_minor & 0x7F
}

/// 表冻结校验：行序严格递增无重复（裁决表性质）。
pub fn table_order_ok() -> bool {
    let mut i = 1;
    while i < SKU_ROWS {
        let a = match sku_row(i - 1) {
            Some(x) => x.0,
            None => return false,
        };
        let b = match sku_row(i) {
            Some(x) => x.0,
            None => return false,
        };
        if b <= a {
            return false;
        }
        i += 1;
    }
    true
}

// ── 注入核（四类违约 × 10 轮） ──────────────────────────────────
/// 未登记注入码族（6 变体冻结：0x0000/0x0011/0x0012/0x0021/0x0022/0x00FF）。
pub const UNREG_CODES: [u32; UNREG_VARIANTS] = [0x0000, 0x0011, 0x0012, 0x0021, 0x0022, 0x00FF];

/// 构造查询（码走 os_minor 槽）。
pub fn make_query(code: u32, os_major: u32, reserved: u32) -> SkuQuery {
    SkuQuery {
        os_major,
        os_minor: code & 0x7F,
        sp_major: 0,
        sp_minor: 0,
        reserved,
    }
}

/// 四类注入 rounds 轮：未登记回退 / 5.x 版本拒 / reserved 拒 / 封印后查询。
pub fn inject_rounds(rounds: u32) -> Result<SkuLedger, u32> {
    let mut led = SkuLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类1 未登记回退：六码族轮换 → 必回退且带 fallback 标记。
        let code = UNREG_CODES[r as usize % UNREG_VARIANTS];
        let mut gate1 = SkuGate::new();
        match gate1.query(&make_query(code, 10, 0)) {
            Ok(info) => {
                if !info.fallback || info.code != PRODUCT_ESSENTIAL_BUSINESS {
                    return Err(SkCode::FallbackUnmarked.code());
                }
                // 修正记录：led.tries 此前漏计（守恒 0≠4 炸出 LedgerBroken
                // 连锁三红）——四个计数查询逐笔计 tries，账实对齐。
                led.tries += 1;
                led.fallback_ok += 1;
            }
            Err(_) => return Err(SkCode::FallbackUnmarked.code()),
        }
        // 类2 5.x 版本拒：os_major 5/4/0 轮换 → 必拒。
        let vmaj = match r % 3 {
            0 => 5,
            1 => 4,
            _ => 0,
        };
        let mut gate2 = SkuGate::new();
        match gate2.query(&make_query(0x0030, vmaj, 0)) {
            Err(c) => {
                if c != SkCode::VersionUnsupported.code() {
                    return Err(SkCode::VersionUnsupported.code());
                }
                led.tries += 1;
                led.version_rej += 1;
            }
            Ok(_) => return Err(SkCode::VersionUnsupported.code()),
        }
        // 类3 reserved 非 0：1/0xFFFF 轮换 → 必拒。
        let res = match r % 2 {
            0 => 1,
            _ => 0xFFFF,
        };
        let mut gate3 = SkuGate::new();
        match gate3.query(&make_query(0x0030, 10, res)) {
            Err(c) => {
                if c != SkCode::ReservedNonzero.code() {
                    return Err(SkCode::ReservedNonzero.code());
                }
                led.tries += 1;
                led.reserved_rej += 1;
            }
            Ok(_) => return Err(SkCode::ReservedNonzero.code()),
        }
        // 类4 封印后查询：先封印再 query → 必拒。
        // （封印前的预置查询走 gate4 自账，不计入 led——led 只收
        // 「封印后被拒」这一注入事件，tries/分类账同笔对齐。）
        let mut gate4 = SkuGate::new();
        let _ = gate4.query(&make_query(0x0030, 10, 0));
        if gate4.seal().is_err() {
            return Err(SkCode::SealedQuery.code());
        }
        match gate4.query(&make_query(0x0030, 10, 0)) {
            Err(c) => {
                if c != SkCode::SealedQuery.code() {
                    return Err(SkCode::SealedQuery.code());
                }
                led.tries += 1;
                led.frozen_rej += 1;
            }
            Ok(_) => return Err(SkCode::SealedQuery.code()),
        }
        r += 1;
    }
    if !led.conserve() {
        return Err(SkCode::LedgerBroken.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链一次（确定性）：21 登记码全谱映射 + 未登记回退 + 注入全抓。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    // 表性质：行数 21 + 序严格递增。
    if !table_order_ok() {
        return Err(SkCode::TableOrder.code());
    }
    let mut gate = SkuGate::new();
    // 21 登记码全谱（10.0 查询，码走 os_minor 槽）。
    let mut i = 0;
    while i < SKU_ROWS {
        let (code, name) = match sku_row(i) {
            Some(x) => x,
            None => return Err(SkCode::RowOutOfRange.code()),
        };
        let info = gate.query(&make_query(code, 10, 0))?;
        if !info.registered || info.fallback || info.code != code || info.name != name {
            return Err(SkCode::MapInconsistent.code());
        }
        // 确定性：同码双查逐字段一致。
        let info2 = gate.query(&make_query(code, 10, 0))?;
        if info != info2 {
            return Err(SkCode::MapInconsistent.code());
        }
        i += 1;
    }
    // 未登记回退：六码族逐码回退且带标记（0x0023 哨兵）。
    let mut k = 0;
    while k < UNREG_VARIANTS {
        let info = gate.query(&make_query(UNREG_CODES[k], 10, 0))?;
        if !info.fallback
            || info.registered
            || info.code != PRODUCT_ESSENTIAL_BUSINESS
            || info.name != "PRODUCT_ESSENTIAL_BUSINESS"
        {
            return Err(SkCode::FallbackUnmarked.code());
        }
        k += 1;
    }
    // 注入四类 × 10 轮全抓。
    let led = inject_rounds(JUDGE_ROUNDS)?;
    if led.fallback_ok != JUDGE_ROUNDS
        || led.version_rej != JUDGE_ROUNDS
        || led.reserved_rej != JUDGE_ROUNDS
        || led.frozen_rej != JUDGE_ROUNDS
    {
        return Err(SkCode::FallbackUnmarked.code());
    }
    // 账守恒：查询 21×2 + 6 = 48 收（42 登记 + 6 回退），0 拒。
    if !gate.ledger.conserve() || gate.ledger.registered_ok != 42 || gate.ledger.fallback_ok != 6 {
        return Err(SkCode::LedgerBroken.code());
    }
    gate.seal()?;
    if gate.phase() != GatePhase::Sealed {
        return Err(SkCode::SealedQuery.code());
    }
    Ok((gate.ledger.registered_ok, gate.ledger.fallback_ok, led.version_rej, gate.ledger.tries))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SkCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SkCode::ALL.len() {
        let v = SkCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：PRODUCT_PROFESSIONAL(0x0030) 登记映射成功。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut gate = SkuGate::new();
    let info = gate.query(&make_query(0x0030, 10, 0))?;
    if !info.registered || info.fallback || info.name != "PRODUCT_PROFESSIONAL" {
        return Err(SkCode::MapInconsistent.code());
    }
    Ok(())
}

/// 探针 B：5.x 版本拒（os_major=5 → VersionUnsupported）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut gate = SkuGate::new();
    gate.query(&make_query(0x0030, 5, 0)).map(|_| ())
}

/// 探针 C：0x0021 未登记 → 回退 0x0023 带 fallback 标记。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut gate = SkuGate::new();
    let info = gate.query(&make_query(0x0021, 10, 0))?;
    if !info.fallback || info.code != PRODUCT_ESSENTIAL_BUSINESS {
        return Err(SkCode::FallbackUnmarked.code());
    }
    Ok(())
}

// ── 判据收口 ────────────────────────────────────────────────────
/// 判据集（8–20 条；动态明细 ≤8）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 表冻结 v1：21 行（越界取 None 恰 21）。
    let row_n = {
        let mut n = 0;
        let mut i = 0;
        while i < SKU_ROWS + 4 {
            if sku_row(i).is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    };
    v.push(("J1-SKU表冻结v1=21行", row_n == SKU_ROWS, format!("rows={}", row_n)));

    // 2. 表序性质：码严格递增无重复。
    let ord_ok = table_order_ok();
    v.push(("J1-表序严格递增", ord_ok, String::from("无重复无乱序")));

    // 3. 回退哨兵冻结：PRODUCT_ESSENTIAL_BUSINESS = 0x0023。
    v.push((
        "J1-回退哨兵0x0023冻结",
        PRODUCT_ESSENTIAL_BUSINESS == 0x0023,
        format!("{:04X}", PRODUCT_ESSENTIAL_BUSINESS),
    ));

    // 4. 21 登记码全谱映射 + 同码双查一致（黄金链内含）。
    let g = golden_run();
    let reg_ok = match &g {
        Ok((r, _f, _v, _t)) => *r == 42,
        Err(_) => false,
    };
    v.push(("J1-21登记码全谱映射", reg_ok, format!("{:?}", g)));

    // 5. 未登记回退断言全过（六码族 → 0x0023 带 fallback 标记）。
    let fb_ok = match &g {
        Ok((_r, f, _v, _t)) => *f == UNREG_VARIANTS as u32,
        Err(_) => false,
    };
    v.push((
        "J1-未登记回退0x0023断言",
        fb_ok,
        format!("fallback={}", match g.as_ref() { Ok(t) => t.1, Err(_) => 0 }),
    ));

    // 6. 回退不直通（fallback 标记必在：探针 C）。
    let pc = probe_seq_c().is_ok();
    v.push(("J1-回退带出处不直通", pc, String::from("fallback=true")));

    // 7. 5.x 版本拒注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let vr = match led.as_ref() {
        Ok(l) => l.version_rej,
        Err(_) => 0,
    };
    v.push(("J1-5.x版本拒10/10", vr == JUDGE_ROUNDS, format!("rej={}", vr)));

    // 8. reserved 拒注入 10/10。
    let rr = match led.as_ref() {
        Ok(l) => l.reserved_rej,
        Err(_) => 0,
    };
    v.push(("J1-reserved拒10/10", rr == JUDGE_ROUNDS, format!("rej={}", rr)));

    // 9. 封印后查询拒注入 10/10。
    let fr = match led.as_ref() {
        Ok(l) => l.frozen_rej,
        Err(_) => 0,
    };
    v.push(("J1-封印后查询拒10/10", fr == JUDGE_ROUNDS, format!("rej={}", fr)));

    // 10. 注入账守恒（4 类 × 10 轮 = 40）。
    let cons_ok = match led.as_ref() {
        Ok(l) => l.conserve() && l.tries == 4 * JUDGE_ROUNDS && l.fallback_ok == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入账守恒40试", cons_ok, String::from("40 试全分类入账")));

    // 11. 探针三序（登记映射 / 5.x 拒 / 回退出处）。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b拒={} c={}", pa, pb, pc),
    ));

    // 12. 版本闸边界（5 拒 / 6 过 / 0xFF 过→回退或登记按码）。
    let vb_ok = {
        let mut gate = SkuGate::new();
        let q5 = gate.query(&make_query(0x0030, 5, 0));
        let q6 = gate.query(&make_query(0x0030, 6, 0));
        let qff = gate.query(&make_query(0x0021, OS_MAJOR_MIN + 0xF9, 0));
        match (q5, q6, qff) {
            (Err(c1), Ok(i6), Ok(iff)) => {
                c1 == SkCode::VersionUnsupported.code()
                    && i6.registered
                    && iff.code == PRODUCT_ESSENTIAL_BUSINESS
            }
            _ => false,
        }
    };
    v.push(("J1-版本闸三档", vb_ok, String::from("5拒/6收/0xFF收")));

    // 13. 双跑一致。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 15. 诊断码闭集（10 码全非空中文明细）。
    let all_ok = {
        let mut ok = SkCode::ALL.len() == 10;
        let mut i = 0;
        while i < SkCode::ALL.len() {
            if SkCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集10码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 15 条；前 8 条动态明细）。
pub fn run_unx18410_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18410 GetProductInfo SKU 语义");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18410_checks();
        assert!(set.all_passed(), "UNX-F18410 判据集应全绿");
    }

    /// 正常流：0x0030 登记映射成功。
    #[test]
    fn test_probe_seq_a() {
        assert!(probe_seq_a().is_ok());
    }

    /// 回退：0x0021 未登记 → 0x0023 带 fallback 标记。
    #[test]
    fn test_fallback_marked() {
        assert!(probe_seq_c().is_ok());
    }

    /// 注入：版本拒 / reserved 拒 / 封印后拒 各 10/10。
    #[test]
    fn test_injections() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.version_rej, 10);
                assert_eq!(l.reserved_rej, 10);
                assert_eq!(l.frozen_rej, 10);
                assert!(l.conserve());
            }
            Err(_) => assert!(false, "全抓时不应报错"),
        }
    }

    /// 表性质：21 行严格递增（抽查首尾魔数）。
    #[test]
    fn test_table_order() {
        assert!(table_order_ok());
        let first = match sku_row(0) {
            Some(x) => x.0,
            None => 0,
        };
        let last = match sku_row(SKU_ROWS - 1) {
            Some(x) => x.0,
            None => 0,
        };
        assert_eq!(first, 0x0001);
        assert_eq!(last, 0x0030);
        assert!(sku_lookup(0x0000).is_none());
        assert_eq!(sku_lookup(0x0030), Some(20));
    }

    /// 版本闸：5.x 拒（VersionUnsupported 码）、6.0 收。
    #[test]
    fn test_version_gate() {
        assert!(probe_seq_b().is_err());
        let mut gate = SkuGate::new();
        let q = make_query(0x0003, 6, 0);
        match gate.query(&q) {
            Ok(info) => assert!(info.registered && info.name == "PRODUCT_HOME_PREMIUM"),
            Err(_) => assert!(false, "6.0 应收"),
        }
    }

    /// reserved 闸：非 0 拒（ReservedNonzero 码）。
    #[test]
    fn test_reserved_gate() {
        let mut gate = SkuGate::new();
        match gate.query(&make_query(0x0030, 10, 1)) {
            Err(c) => assert_eq!(c, SkCode::ReservedNonzero.code()),
            Ok(_) => assert!(false, "reserved=1 必须拒"),
        }
    }

    /// 封印：Open→Sealed 单向；封印后查询拒（SealedQuery 码）。
    #[test]
    fn test_seal_gate() {
        let mut gate = SkuGate::new();
        let _ = gate.query(&make_query(0x0030, 10, 0));
        assert!(gate.seal().is_ok());
        match gate.query(&make_query(0x0030, 10, 0)) {
            Err(c) => assert_eq!(c, SkCode::SealedQuery.code()),
            Ok(_) => assert!(false, "封印后查询必须拒"),
        }
    }

    /// 确定性：同码双查逐字段一致（0x0007 抽查）。
    #[test]
    fn test_map_deterministic() {
        let mut gate = SkuGate::new();
        let a = gate.query(&make_query(0x0007, 10, 0));
        let b = gate.query(&make_query(0x0007, 10, 0));
        assert_eq!(a, b);
        match a {
            Ok(info) => assert_eq!(info.name, "PRODUCT_STANDARD_SERVER"),
            Err(_) => assert!(false, "不应报错"),
        }
    }

    /// 复现红线：golden_run 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
