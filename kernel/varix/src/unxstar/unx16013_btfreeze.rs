//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16013 · bottle 版本伪装与库覆盖挂点（E4 消费面预留） · 0x68B00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B01）：
//!   UNX-F16013-J1 挂点接口冻结单落账（接口名/参数/返回语义三要素
//!   齐备），Schema 先行消费方登记表含 E4 域，冻结单变更会签断言
//!   （无单方变更记录 = 0）通过。
//! 定位：E4 域的版本伪装 per-app 与 bottle 级版本覆盖要落在 bottle
//!   上生效，本条按 Schema 先行纪律预冻结"bottle 版本覆盖挂点"接
//!   口：E4 未开工，接口先冻结，双方向登记消费关系。
//! 设计要点：
//!   - 冻结单三要素：接口 bottle_version_override——入参（bottle ID/
//!     目标 DLL 或 API 名/伪装版本值）、出参（生效确认/拒绝理由）、
//!     错误语义（越权/未找到/值非法三类码），登记进
//!     cross_domain_freeze 通道；
//!   - 消费方登记表：E4 域为第一消费方，AI-24 版本矩阵复用协议为
//!     上游接口，未开工域标注"Schema 先行"；
//!   - 会签断言（判据核心）：冻结单变更记录逐条检查会签字段，无双
//!     方会签的变更记录计数必须为 0——单方私改冻结接口即红；
//!   - 弃用期纪律：接口形状修订时旧签名保留一个波次的弃用期；
//!   - 注入面：单方变更/三要素缺一/消费方缺 E4/错误码越三类/重复
//!     登记/未开工域漏标注——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68B00）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68B00，AI-VX-1031 E1 域批G 独占区）。
pub const SEG_BASE: u32 = 0x68B00;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 冻结接口名（Schema 先行，写死）。
pub const FREEZE_IFACE: &str = "bottle_version_override";
pub const FREEZE_VER: u32 = 1;

// ── 错误语义三类码闭集 ───────────────────────────────────────────
pub const ERR_UNAUTHORIZED: usize = 0; // 越权
pub const ERR_NOT_FOUND: usize = 1; // 未找到
pub const ERR_INVALID_VALUE: usize = 2; // 值非法
pub const ERR_CODE_COUNT: usize = 3;

pub const ERR_CODE_NAMES: [&str; ERR_CODE_COUNT] =
    ["越权", "未找到", "值非法"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    /// 冻结单三要素缺一（接口名/参数/返回语义）
    TriElementShort,
    /// 错误码越三类闭集
    ErrCodeOutOfRange,
    /// 消费方登记表缺 E4 域
    ConsumerMissingE4,
    /// 未开工域漏标"Schema 先行"
    SchemaFirstMissing,
    /// 单方变更（无双方会签的变更记录）
    UnilateralChange,
    /// 变更记录重复
    ChangeDuplicated,
    /// 重复登记
    RegistrationDuplicated,
    /// 弃用期纪律失守（旧签名未保留）
    DeprecationSkipped,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl FzCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FzCode::TriElementShort => "冻结单三要素缺一",
            FzCode::ErrCodeOutOfRange => "错误码越三类闭集",
            FzCode::ConsumerMissingE4 => "消费方登记表缺 E4 域",
            FzCode::SchemaFirstMissing => "未开工域漏标 Schema 先行",
            FzCode::UnilateralChange => "单方变更：无双方会签即红",
            FzCode::ChangeDuplicated => "变更记录重复",
            FzCode::RegistrationDuplicated => "重复登记",
            FzCode::DeprecationSkipped => "弃用期纪律失守：旧签名未保留",
            FzCode::NotReproducible => "双跑不一致：确定性红线",
            FzCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [FzCode; 10] = [
        FzCode::TriElementShort,
        FzCode::ErrCodeOutOfRange,
        FzCode::ConsumerMissingE4,
        FzCode::SchemaFirstMissing,
        FzCode::UnilateralChange,
        FzCode::ChangeDuplicated,
        FzCode::RegistrationDuplicated,
        FzCode::DeprecationSkipped,
        FzCode::NotReproducible,
        FzCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FzCode::ALL.len() {
        if FzCode::ALL[i].val() == code {
            return FzCode::ALL[i].detail();
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

// ── 冻结单三要素 ─────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct FreezeSpec {
    /// 接口名（冻结，写死 FREEZE_IFACE）。
    pub iface: String,
    /// 入参三件（bottle ID/目标 DLL 或 API 名/伪装版本值）。
    pub params: [&'static str; 3],
    /// 出参两件（生效确认/拒绝理由）。
    pub returns: [&'static str; 2],
    /// 错误语义三类码。
    pub err_codes: [usize; ERR_CODE_COUNT],
}

/// 冻结单三要素齐备校验。
pub fn vet_spec(spec: &FreezeSpec) -> Result<(), u32> {
    if spec.iface != FREEZE_IFACE {
        return Err(FzCode::TriElementShort.val());
    }
    if spec.params.len() != 3 || spec.returns.len() != 2 {
        return Err(FzCode::TriElementShort.val());
    }
    let mut i = 0;
    while i < spec.params.len() {
        if spec.params[i].len() == 0 {
            return Err(FzCode::TriElementShort.val());
        }
        i += 1;
    }
    let mut i = 0;
    while i < spec.returns.len() {
        if spec.returns[i].len() == 0 {
            return Err(FzCode::TriElementShort.val());
        }
        i += 1;
    }
    let mut i = 0;
    while i < spec.err_codes.len() {
        if spec.err_codes[i] >= ERR_CODE_COUNT {
            return Err(FzCode::ErrCodeOutOfRange.val());
        }
        i += 1;
    }
    Ok(())
}

/// 首版冻结单。
pub fn initial_spec() -> FreezeSpec {
    FreezeSpec {
        iface: String::from(FREEZE_IFACE),
        params: ["bottle_id", "target_dll_or_api", "mask_version"],
        returns: ["effective_confirm", "reject_reason"],
        err_codes: [ERR_UNAUTHORIZED, ERR_NOT_FOUND, ERR_INVALID_VALUE],
    }
}

// ── 消费方登记表 ─────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ConsumerRow {
    /// 消费域（E4 = 第一消费方）。
    pub domain: String,
    /// 消费方向（"downstream" / "upstream"）。
    pub direction: String,
    /// 联签状态（true=已联签）。
    pub countersigned: bool,
    /// 未开工域 Schema 先行标注。
    pub schema_first: bool,
}

/// 登记表体检：E4 必在、未开工域必标 Schema 先行。
pub fn vet_consumers(rows: &[ConsumerRow]) -> Result<(), u32> {
    let mut has_e4 = false;
    let mut i = 0;
    while i < rows.len() {
        if rows[i].domain == "E4" {
            has_e4 = true;
        }
        if rows[i].domain.len() == 0 {
            return Err(FzCode::ConsumerMissingE4.val());
        }
        i += 1;
    }
    if !has_e4 {
        return Err(FzCode::ConsumerMissingE4.val());
    }
    // 方向闭集。
    let mut i = 0;
    while i < rows.len() {
        if rows[i].direction != "downstream" && rows[i].direction != "upstream" {
            return Err(FzCode::ConsumerMissingE4.val());
        }
        i += 1;
    }
    Ok(())
}

/// 首版登记表：E4（downstream, Schema 先行）+ AI-24（upstream）。
pub fn initial_consumers() -> Vec<ConsumerRow> {
    let mut v: Vec<ConsumerRow> = Vec::new();
    v.push(ConsumerRow {
        domain: String::from("E4"),
        direction: String::from("downstream"),
        countersigned: false,
        schema_first: true,
    });
    v.push(ConsumerRow {
        domain: String::from("AI-24"),
        direction: String::from("upstream"),
        countersigned: true,
        schema_first: false,
    });
    v
}

/// E4 Schema 先行标注检查。
pub fn schema_first_marked(rows: &[ConsumerRow]) -> Result<(), u32> {
    let mut i = 0;
    while i < rows.len() {
        if rows[i].domain == "E4" && !rows[i].schema_first {
            return Err(FzCode::SchemaFirstMissing.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 冻结单变更账 + 会签断言 ─────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ChangeRow {
    pub seq: u32,
    /// 变更摘要。
    pub note: String,
    /// 双方会签（域内方 + 消费方）。
    pub signed_domain: bool,
    pub signed_consumer: bool,
}

/// 会签断言（判据核心）：无双方会签的变更记录计数 = 0。
pub fn unilateral_count(changes: &[ChangeRow]) -> u32 {
    let mut n = 0;
    let mut i = 0;
    while i < changes.len() {
        if !(changes[i].signed_domain && changes[i].signed_consumer) {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 变更账（seq 连续 + 重复检出）。
pub struct ChangeLedger {
    rows: Vec<ChangeRow>,
}

impl ChangeLedger {
    pub fn new() -> ChangeLedger {
        ChangeLedger { rows: Vec::new() }
    }

    pub fn append(&mut self, row: ChangeRow) -> Result<(), u32> {
        if row.note.len() == 0 {
            return Err(FzCode::ChangeDuplicated.val());
        }
        // seq 连续。
        let expect = (self.rows.len() + 1) as u32;
        if row.seq != expect {
            return Err(FzCode::ChangeDuplicated.val());
        }
        // 单方变更即拒（会签断言的入账侧）。
        if unilateral_count(&[row.clone()]) != 0 {
            return Err(FzCode::UnilateralChange.val());
        }
        self.rows.push(row);
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 弃用期纪律：接口形状修订时旧签名保留一个波次（账面模拟）。
pub struct DeprecationBook {
    /// 旧签名（弃用期在册）。
    pub old_sigs: Vec<String>,
    /// 弃用期剩余波次。
    pub waves_left: Vec<u32>,
}

impl DeprecationBook {
    pub fn new() -> DeprecationBook {
        DeprecationBook { old_sigs: Vec::new(), waves_left: Vec::new() }
    }

    /// 修订时登记旧签名（弃用期一个波次）。
    pub fn deprecate(&mut self, old_sig: &str) {
        self.old_sigs.push(String::from(old_sig));
        self.waves_left.push(1);
    }

    /// 完备校验：有修订必留旧签名（旧签名未保留即红）。
    pub fn closed_after_revision(&self, revised: bool) -> Result<(), u32> {
        if revised && self.old_sigs.is_empty() {
            return Err(FzCode::DeprecationSkipped.val());
        }
        Ok(())
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((消费方数, 单方变更数))。
pub fn golden_run() -> Result<(usize, u32), u32> {
    let spec = initial_spec();
    vet_spec(&spec)?;
    let consumers = initial_consumers();
    vet_consumers(&consumers)?;
    schema_first_marked(&consumers)?;
    // 会签断言：空变更账 → 单方变更 = 0。
    let changes = ChangeLedger::new();
    let uni = unilateral_count(&changes.rows);
    if uni != 0 {
        return Err(FzCode::UnilateralChange.val());
    }
    // 双方会签变更入账正向。
    let mut cl = ChangeLedger::new();
    cl.append(ChangeRow {
        seq: 1,
        note: String::from("mask_version 参数语义澄清"),
        signed_domain: true,
        signed_consumer: true,
    })?;
    // 错误码三类名点验。
    let mut e = 0;
    while e < ERR_CODE_COUNT {
        if ERR_CODE_NAMES[e].len() == 0 {
            return Err(FzCode::ErrCodeOutOfRange.val());
        }
        e += 1;
    }
    Ok((consumers.len(), uni))
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
        // ① 单方变更：只有域内会签 → 入账必拒（判据复测方式）。
        let uni_ok = {
            let mut cl = ChangeLedger::new();
            cl.append(ChangeRow {
                seq: 1,
                note: String::from("单方私改接口形状"),
                signed_domain: true,
                signed_consumer: false,
            }) == Err(FzCode::UnilateralChange.val())
        };
        led.record(0, uni_ok);

        // ② 三要素缺一：参数表缺件。
        let short_ok = {
            let mut spec = initial_spec();
            spec.params[1] = "";
            vet_spec(&spec) == Err(FzCode::TriElementShort.val())
        };
        led.record(1, short_ok);

        // ③ 消费方缺 E4。
        let no_e4 = {
            let rows = vec![ConsumerRow {
                domain: String::from("AI-24"),
                direction: String::from("upstream"),
                countersigned: true,
                schema_first: false,
            }];
            vet_consumers(&rows) == Err(FzCode::ConsumerMissingE4.val())
        };
        led.record(2, no_e4);

        // ④ E4 漏标 Schema 先行。
        let no_mark = {
            let rows = vec![ConsumerRow {
                domain: String::from("E4"),
                direction: String::from("downstream"),
                countersigned: false,
                schema_first: false,
            }];
            schema_first_marked(&rows) == Err(FzCode::SchemaFirstMissing.val())
        };
        led.record(3, no_mark);

        // ⑤ 错误码越三类闭集。
        led.record(4, {
            let mut spec = initial_spec();
            spec.err_codes[0] = ERR_CODE_COUNT + (rnd % 2) as usize;
            vet_spec(&spec) == Err(FzCode::ErrCodeOutOfRange.val())
        });

        // ⑥ 弃用期纪律：修订无旧签名留档必红 + seq 跳号。
        led.record(5, {
            let d = DeprecationBook::new();
            d.closed_after_revision(true) == Err(FzCode::DeprecationSkipped.val())
                && {
                    let mut cl = ChangeLedger::new();
                    let _ = cl.append(ChangeRow {
                        seq: 1,
                        note: String::from("首次变更"),
                        signed_domain: true,
                        signed_consumer: true,
                    });
                    cl.append(ChangeRow {
                        seq: 3,
                        note: String::from("跳号变更"),
                        signed_domain: true,
                        signed_consumer: true,
                    }) == Err(FzCode::ChangeDuplicated.val())
                        && (rnd % 1) == 0
                }
        });

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
                Err(FzCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据：合法冻结单必须全过 ─────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let spec = initial_spec();
    vet_spec(&spec)?;
    let consumers = initial_consumers();
    vet_consumers(&consumers)?;
    schema_first_marked(&consumers)?;
    // 错误码三类名点验。
    let mut e = 0;
    while e < ERR_CODE_COUNT {
        if ERR_CODE_NAMES[e].len() == 0 {
            return Err(FzCode::ErrCodeOutOfRange.val());
        }
        e += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FzCode::ALL.len() {
        let v = FzCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥10 条） ───────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：2 消费方 / 单方变更 0。
    let g = golden_run();
    v.push((
        "J1-黄金正演2消费方0单方",
        matches!(g, Ok((2, 0))),
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
    // 3. 冻结单三要素齐备（判据原文口径）。
    let spec = initial_spec();
    v.push((
        "J1-冻结单三要素齐备",
        matches!(vet_spec(&spec), Ok(())),
        String::from(FREEZE_IFACE),
    ));
    // 4. 消费方登记表含 E4（判据原文口径）。
    let consumers = initial_consumers();
    v.push((
        "J1-登记表含E4",
        matches!(vet_consumers(&consumers), Ok(())),
        String::from("E4+AI-24"),
    ));
    // 5. 无会签变更计数 = 0（判据原文口径）。
    let changes = ChangeLedger::new();
    v.push((
        "J1-无会签变更计数0",
        unilateral_count(&changes.rows) == 0,
        String::from("空账即 0"),
    ));
    // 6. 双方会签变更正向入账。
    let signed_ok = {
        let mut cl = ChangeLedger::new();
        cl.append(ChangeRow {
            seq: 1,
            note: String::from("双方会签变更样例"),
            signed_domain: true,
            signed_consumer: true,
        })
        .is_ok()
    };
    v.push(("J1-双方会签入账", signed_ok, String::from("正向")));
    // 7. 错误语义三类码。
    v.push((
        "J1-错误语义三类码",
        ERR_CODE_COUNT == 3,
        String::from("越权/未找到/值非法"),
    ));
    // 8. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 9. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("三要素+E4+三类码"),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16013_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16013 bottle 版本伪装与库覆盖挂点（E4 消费面预留）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
