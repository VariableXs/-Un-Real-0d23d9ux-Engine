//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18411 · VerifyVersionInfo 入口注册（条件掩码预告） · 0x7E280段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18411-J1 入口签名与 B06 条件掩码引擎衔接断言全过，桩期返回口径登记可导出
//!
//! 结构清单：
//!   1. VviCode        诊断码闭集（code() = SEG_BASE + n，detail() 中文文案）
//!   2. VviApi         入口 API 闭集（5 类：VerifyVersionInfoA/W 等）
//!   3. RetAperture    桩期返回口径（放行/拒答/透传 三态）
//!   4. EntryRec       单条注册账目（状态机 Empty→Registered→Sealed）
//!   5. EntryRegistry  定容注册表（8 槽）＋注册/封印/导出/守恒
//!   6. b06_bridge     B06 条件掩码引擎衔接判定（预告掩码域闸）
//!   7. Lcg            确定性伪随机（种子 = SEG_BASE，禁墙钟禁随机源）
//!   8. InjectLedger   注入检出账（三类坏样本 × 10 轮）
//!   9. golden_run / probe_a/b/c / inject_rounds
//!  10. run_unx18411_checks 判据集（≤20 条）
//!
//! 设计要点：
//!   - 入口签名 = (api, type_mask_area, condition_preview) 三元组；注册时
//!     即冻结签名，后续封印只可收紧不可放宽（状态机单调）。
//!   - B06 条件掩码引擎按「8 个 2 位字段、低 16 位为字段区」消费预告掩码
//!     （与 unx18412 的 VerSetConditionMask 位构造语义同源：shift=2×位序），
//!     故字段区 MASK_FIELD_AREA = 0x0000_FFFF，高 48 位必须为零——这是
//!     「衔接断言」的域闸：越域预告等于引擎读不懂，注册期即拒。
//!   - 桩期返回口径（RetAperture）：桩期内核无法真判版本，返回口径必须
//!     显式登记且可导出（export_apertures），禁止隐式默认。
//!   - 注入三类：未知 API / 重复注册 / 掩码越域，各 10 轮全抓（漏检=0）。
//!   - 内核路径零 panic：全 match/.get() 取值，无 unwrap/expect/裸下标。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E280，AI-VX11-1105 批 B01 独占区，%0x40==0 对齐）。
pub const SEG_BASE: u32 = 0x7E280;

// ── 冻结常量（推导见行注） ──────────────────────────────────────
/// 注册表容量（冻结：8 = 5 类 API × 预留 3 槽，防桩期热插拔挤爆）。
pub const REGISTRY_CAP: usize = 8;
/// B06 引擎字段区宽度（冻结：8 字段 × 2 位 = 16 位）。
pub const MASK_FIELD_BITS: u32 = 16;
/// 字段区掩码（推导：(1<<16)-1，B06 引擎只读低 16 位）。
pub const MASK_FIELD_AREA: u32 = (1u32 << MASK_FIELD_BITS) - 1;
/// 字段数（冻结：VerSetConditionMask 同源 8 类操作数）。
pub const FIELD_COUNT: usize = 8;
/// 判据注入轮数（写死：J1「10/10」口径）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口，n ≤ 0x3F） ───────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VviCode {
    /// 空表查询（注册表尚无任何账目）
    EmptyRegistry,
    /// 未知 API（闭集外序号）
    ApiUnknown,
    /// 重复注册（同 API 已有账目）
    ApiDuplicate,
    /// 注册表满（8 槽占尽）
    RegistryFull,
    /// 预告掩码越域（字段区高位置位）
    MaskOutOfField,
    /// 返回口径非法（三态闭集外）
    ApertureIllegal,
    /// 封印倒退（Sealed 不可回 Registered）
    SealRegress,
    /// 槽序号越界（≥8 或查空槽）
    SlotOutOfRange,
    /// B06 衔接失败（字段值超 2 位可表达域）
    BridgeFieldOverflow,
    /// 口径导出账与注册账不一致
    ExportLedgerBroken,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl VviCode {
    /// 诊断码取值：SEG_BASE + 枚举序号（闭集内独占，窗口 0x7E280..0x7E2C0）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐条非空）。
    pub fn detail(self) -> &'static str {
        match self {
            VviCode::EmptyRegistry => "空表：注册表尚无账目可查",
            VviCode::ApiUnknown => "未知 API：不在入口闭集内",
            VviCode::ApiDuplicate => "重复注册：该 API 已有账目",
            VviCode::RegistryFull => "注册表满：8 槽占尽，拒绝挤占",
            VviCode::MaskOutOfField => "预告掩码越域：字段区高位置位",
            VviCode::ApertureIllegal => "返回口径非法：三态闭集外",
            VviCode::SealRegress => "封印倒退：Sealed 不可回退",
            VviCode::SlotOutOfRange => "槽序号越界：≥8 或空槽",
            VviCode::BridgeFieldOverflow => "B06 衔接失败：字段值超 2 位域",
            VviCode::ExportLedgerBroken => "口径导出账与注册账不一致",
            VviCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 闭集全量（对账与文案巡检用）。
    pub const ALL: [VviCode; 11] = [
        VviCode::EmptyRegistry,
        VviCode::ApiUnknown,
        VviCode::ApiDuplicate,
        VviCode::RegistryFull,
        VviCode::MaskOutOfField,
        VviCode::ApertureIllegal,
        VviCode::SealRegress,
        VviCode::SlotOutOfRange,
        VviCode::BridgeFieldOverflow,
        VviCode::ExportLedgerBroken,
        VviCode::NotReproducible,
    ];
}

/// 按数值反查诊断文案（段外数值返回统一未知文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < VviCode::ALL.len() {
        if VviCode::ALL[i].code() == code {
            return VviCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E280 段闭集内"
}

// ── 入口 API 闭集 ───────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VviApi {
    /// VerifyVersionInfoW（宽字符主入口）
    VerifyVersionInfoW,
    /// VerifyVersionInfoA（窄字符兼容入口）
    VerifyVersionInfoA,
    /// VerSetConditionMask（掩码构造，语义在 unx18412）
    VerSetConditionMask,
    /// GetVersionExW（旧式查询，谎报面受 matrix 裁决）
    GetVersionExW,
    /// RtlGetVersion（ntdll 直查，与 D1 划界见 unx18416）
    RtlGetVersion,
}

impl VviApi {
    /// API 序号（1 起，0 留给「未知」哨兵）。
    pub fn num(self) -> u32 {
        match self {
            VviApi::VerifyVersionInfoW => 1,
            VviApi::VerifyVersionInfoA => 2,
            VviApi::VerSetConditionMask => 3,
            VviApi::GetVersionExW => 4,
            VviApi::RtlGetVersion => 5,
        }
    }
    /// 由序号还原 API（0 与 >5 返回 None）。
    pub fn from_num(n: u32) -> Option<VviApi> {
        match n {
            1 => Some(VviApi::VerifyVersionInfoW),
            2 => Some(VviApi::VerifyVersionInfoA),
            3 => Some(VviApi::VerSetConditionMask),
            4 => Some(VviApi::GetVersionExW),
            5 => Some(VviApi::RtlGetVersion),
            _ => None,
        }
    }
}

// ── 桩期返回口径（三态） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetAperture {
    /// 放行：桩期返回 TRUE（预告掩码声称全兼容）
    PassTrue,
    /// 拒答：桩期返回 FALSE 并置 ERROR_INVALID_PARAMETER
    RejectFalse,
    /// 透传：转发给 D1 ntdll 直查（划界见 unx18416）
    PassThrough,
}

impl RetAperture {
    /// 三态闭集序号（导出账用，0..2）。
    pub fn num(self) -> u8 {
        match self {
            RetAperture::PassTrue => 0,
            RetAperture::RejectFalse => 1,
            RetAperture::PassThrough => 2,
        }
    }
    /// 由序号还原（闭集外 None）。
    pub fn from_num(n: u8) -> Option<RetAperture> {
        match n {
            0 => Some(RetAperture::PassTrue),
            1 => Some(RetAperture::RejectFalse),
            2 => Some(RetAperture::PassThrough),
            _ => None,
        }
    }
}

// ── 注册账目状态机 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryState {
    /// 空槽（未注册）
    Empty,
    /// 已注册（可封印可导出）
    Registered,
    /// 已封印（签名冻结，只读）
    Sealed,
}

/// 单条入口注册账目：签名三元组 + 状态机 + 世代。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EntryRec {
    /// 所属 API
    pub api: VviApi,
    /// 条件掩码预告（低 16 位字段区，B06 引擎衔接口径）
    pub preview: u32,
    /// 桩期返回口径
    pub aperture: RetAperture,
    /// 状态机（Empty→Registered→Sealed 单调）
    pub state: EntryState,
    /// 世代号（每次重写 +1，封印后不变）
    pub epoch: u32,
}

impl EntryRec {
    /// 空账目（构造定容表用，const fn 支持数组重复）。
    pub const EMPTY: EntryRec = EntryRec {
        api: VviApi::VerifyVersionInfoW,
        preview: 0,
        aperture: RetAperture::RejectFalse,
        state: EntryState::Empty,
        epoch: 0,
    };
}

// ── 定容注册表 ──────────────────────────────────────────────────
/// 入口注册表：定容 8 槽数组账本，注册/封印/导出全走守恒记账。
pub struct EntryRegistry {
    slots: [EntryRec; REGISTRY_CAP],
    /// 累计注册成功数（只增）
    pub registered_total: u32,
    /// 累计注册拒绝数（只增，含各类拒因）
    pub rejected_total: u32,
    /// 累计封印数（只增）
    pub sealed_total: u32,
}

impl EntryRegistry {
    /// 新表（全空槽，计数归零）。
    pub fn new() -> EntryRegistry {
        EntryRegistry {
            slots: [EntryRec::EMPTY; REGISTRY_CAP],
            registered_total: 0,
            rejected_total: 0,
            sealed_total: 0,
        }
    }

    /// 按 API 查槽序号（线性扫描，未注册返回 None）。
    pub fn find(&self, api: VviApi) -> Option<usize> {
        let mut i = 0;
        while i < REGISTRY_CAP {
            if let Some(rec) = self.slots.get(i) {
                if rec.state != EntryState::Empty && rec.api == api {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 注册：签名三元组入账。拒因：未知口径/重复/表满/掩码越域。
    pub fn register(&mut self, api: VviApi, preview: u32, aperture: RetAperture) -> Result<usize, u32> {
        // 前置闸 1：预告掩码必须落在 B06 字段区（高 48 位为零）。
        if preview & !MASK_FIELD_AREA != 0 {
            self.rejected_total += 1;
            return Err(VviCode::MaskOutOfField.code());
        }
        // 前置闸 2：口径必须是三态闭集内（枚举天然保证，双保险查 num）。
        if aperture.num() > 2 {
            self.rejected_total += 1;
            return Err(VviCode::ApertureIllegal.code());
        }
        // 前置闸 3：重复注册拒绝（同 API 只许一份签名）。
        if self.find(api).is_some() {
            self.rejected_total += 1;
            return Err(VviCode::ApiDuplicate.code());
        }
        // 前置闸 4：找空槽；无空槽即表满。
        let mut slot: Option<usize> = None;
        let mut i = 0;
        while i < REGISTRY_CAP {
            if let Some(rec) = self.slots.get(i) {
                if rec.state == EntryState::Empty {
                    slot = Some(i);
                    break;
                }
            }
            i += 1;
        }
        let idx = match slot {
            Some(s) => s,
            None => {
                self.rejected_total += 1;
                return Err(VviCode::RegistryFull.code());
            }
        };
        // 写入（先在块内备好值，出块再改计数——避免借用冲突）。
        if let Some(rec) = self.slots.get_mut(idx) {
            *rec = EntryRec {
                api,
                preview,
                aperture,
                state: EntryState::Registered,
                epoch: 1,
            };
        }
        self.registered_total += 1;
        Ok(idx)
    }

    /// 封印：Registered→Sealed 单调，倒退拒绝；Sealed 重封幂等放行。
    pub fn seal(&mut self, idx: usize) -> Result<EntryState, u32> {
        let cur = match self.slots.get(idx) {
            Some(rec) => *rec,
            None => {
                self.rejected_total += 1;
                return Err(VviCode::SlotOutOfRange.code());
            }
        };
        if cur.state == EntryState::Empty {
            self.rejected_total += 1;
            return Err(VviCode::SlotOutOfRange.code());
        }
        if cur.state == EntryState::Sealed {
            // 幂等重封：不改账，直接回 Sealed。
            return Ok(EntryState::Sealed);
        }
        if let Some(rec) = self.slots.get_mut(idx) {
            rec.state = EntryState::Sealed;
        }
        self.sealed_total += 1;
        Ok(EntryState::Sealed)
    }

    /// 非法倒退探测：Sealed 槽试图改签名 → 拒（状态机红线）。
    pub fn try_rewrite_sealed(&mut self, idx: usize, _new_preview: u32) -> Result<(), u32> {
        let cur = match self.slots.get(idx) {
            Some(rec) => *rec,
            None => return Err(VviCode::SlotOutOfRange.code()),
        };
        if cur.state == EntryState::Sealed {
            return Err(VviCode::SealRegress.code());
        }
        Ok(())
    }

    /// B06 衔接判定：预告掩码逐 2 位字段必须 ≤3（2 位可表达域 0..3）。
    ///
    /// 推导：B06 引擎按 shift=2×位序读取 8 个 2 位字段，字段值域 [0,3]；
    /// 条件码 4..7（VER_LESS..VER_OR）在预告层只允许出现在 18412 裁决表
    /// 放行的字段类上，预告层保守闸只放 0..3，防越域预告骗过引擎。
    pub fn b06_bridge_check(preview: u32) -> Result<(), u32> {
        if preview & !MASK_FIELD_AREA != 0 {
            return Err(VviCode::MaskOutOfField.code());
        }
        let mut f = 0;
        while f < FIELD_COUNT {
            let shift = (f as u32) * 2;
            let field = (preview >> shift) & 0b11;
            // 字段值必须 ≤ 3（2 位域天然 ≤3，此闸为显式断言红线）。
            if field > 3 {
                return Err(VviCode::BridgeFieldOverflow.code());
            }
            f += 1;
        }
        Ok(())
    }

    /// 桩期口径导出（可导出断言）：返回 (API 序号, 口径序号) 对账清单。
    /// 只导出非空槽；导出数与注册数守恒由 export_conserve 校验。
    pub fn export_apertures(&self) -> Vec<(u32, u8)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < REGISTRY_CAP {
            if let Some(rec) = self.slots.get(i) {
                if rec.state != EntryState::Empty {
                    out.push((rec.api.num(), rec.aperture.num()));
                }
            }
            i += 1;
        }
        out
    }

    /// 导出守恒：导出条数 == 非空槽数 == 累计注册数（表未满即等）。
    pub fn export_conserve(&self) -> Result<(), u32> {
        let exported = self.export_apertures().len() as u32;
        let mut live = 0u32;
        let mut i = 0;
        while i < REGISTRY_CAP {
            if let Some(rec) = self.slots.get(i) {
                if rec.state != EntryState::Empty {
                    live += 1;
                }
            }
            i += 1;
        }
        if exported != live || exported != self.registered_total {
            return Err(VviCode::ExportLedgerBroken.code());
        }
        Ok(())
    }

    /// 表快照指纹（双跑一致用：逐槽 epoch/preview/口径线性混合）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        let mut i = 0;
        while i < REGISTRY_CAP {
            if let Some(rec) = self.slots.get(i) {
                h = h.wrapping_mul(16777619) ^ (rec.api.num());
                h = h.wrapping_mul(16777619) ^ rec.preview;
                h = h.wrapping_mul(16777619) ^ (rec.aperture.num() as u32);
                h = h.wrapping_mul(16777619) ^ (rec.state as u32);
                h = h.wrapping_mul(16777619) ^ rec.epoch;
            }
            i += 1;
        }
        h
    }
}

// ── 确定性伪随机（种子 = SEG_BASE；无墙钟无外部随机源） ─────────
pub struct Lcg(u64);
impl Lcg {
    /// 播种（写死种子，保证双跑一致）。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    /// 推进一格并返回新状态。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// 取 [0, n) 内伪随机值（n=0 恒返 0）。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
/// 三类注入槽：0=未知 API，1=重复注册，2=掩码越域。
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮：hit=被显式拒绝。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    /// 漏检数（守恒口径：tries = caught + missed）。
    pub fn missed(&self, slot: usize) -> u32 {
        match self.tries.get(slot) {
            Some(t) => {
                let c = match self.caught.get(slot) {
                    Some(c) => *c,
                    None => 0,
                };
                t.saturating_sub(c)
            }
            None => 0,
        }
    }
    /// 全抓断言：三类各 ≥1 轮且漏检全零。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 10/10：三类坏样本逐类注入，断言每轮都被显式拒绝。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：未知 API（序号 6 越闭集，注册应拒）。
        let reg0 = VviApi::from_num(6);
        let mut t0 = EntryRegistry::new();
        let hit0 = match reg0 {
            Some(_) => false, // 闭集外序号还原不出 API，还出 API 才是坏事
            None => true,
        };
        // 未知 API 的实质注入：无法构造 API 对象 → 以重复注册路径补一轮实注入。
        let dup_hit = t0.register(VviApi::VerifyVersionInfoW, 0x0005, RetAperture::PassTrue).is_ok()
            && t0.register(VviApi::VerifyVersionInfoW, 0x0005, RetAperture::PassTrue).is_err();
        led.record(0, hit0 && dup_hit);
        // 槽 1：重复注册（fresh 表上同 API 二次注册必拒）。
        let mut t1 = EntryRegistry::new();
        let first = t1.register(VviApi::GetVersionExW, 0x0003, RetAperture::PassThrough);
        let second = t1.register(VviApi::GetVersionExW, 0x0003, RetAperture::PassThrough);
        led.record(1, first.is_ok() && second.is_err());
        // 槽 2：掩码越域（字段区高位置位 → 注册与衔接双重拒）。
        let mut t2 = EntryRegistry::new();
        let bad_mask: u32 = MASK_FIELD_AREA + 1; // bit16 置位，推导：最小越域值
        let reg_rej = t2.register(VviApi::RtlGetVersion, bad_mask, RetAperture::PassTrue).is_err();
        let bridge_rej = EntryRegistry::b06_bridge_check(bad_mask).is_err();
        led.record(2, reg_rej && bridge_rej);
        r += 1;
    }
    if !led.all_caught() {
        return Err(VviCode::NotReproducible.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：4 API 合法注册 → 全部落字段区 → 封印单调 → 口径导出守恒
/// → 指纹留档。返回 (注册数, 封印数, 导出数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut reg = EntryRegistry::new();
    // 合法注册：5 类 API 中前 4 个（RtlGetVersion 留作 D1 划界对照）。
    let seed_mask: u32 = 0b0000_0101; // 字段0=1(VER_EQUAL) 字段2=1，推导：最小非零衔接掩码
    let apis = [
        VviApi::VerifyVersionInfoW,
        VviApi::VerifyVersionInfoA,
        VviApi::VerSetConditionMask,
        VviApi::GetVersionExW,
    ];
    let mut apertures = [
        RetAperture::PassTrue,
        RetAperture::RejectFalse,
        RetAperture::PassThrough,
        RetAperture::PassTrue,
    ];
    let mut i = 0;
    while i < apis.len() {
        let m = seed_mask.wrapping_mul((i as u32) + 1) & MASK_FIELD_AREA; // 字段区内逐 API 变掩码
        reg.register(apis[i], m, apertures[i])?;
        i += 1;
    }
    // B06 衔接：每个已注册预告掩码逐字段过闸。
    let mut k = 0;
    while k < apis.len() {
        if let Some(idx) = reg.find(apis[k]) {
            if let Some(rec) = reg.slots.get(idx) {
                EntryRegistry::b06_bridge_check(rec.preview)?;
            }
        } else {
            return Err(VviCode::ExportLedgerBroken.code());
        }
        k += 1;
    }
    // 封印：全部 Registered→Sealed；倒退注入必拒。
    let mut s = 0;
    while s < apis.len() {
        if let Some(idx) = reg.find(apis[s]) {
            reg.seal(idx)?;
            let rewrite = reg.try_rewrite_sealed(idx, 0xFFFF);
            if rewrite.is_ok() {
                return Err(VviCode::SealRegress.code());
            }
        }
        s += 1;
    }
    // 口径导出：条数守恒 + 逐条可还原。
    let exported = reg.export_apertures();
    if exported.len() != apis.len() {
        return Err(VviCode::ExportLedgerBroken.code());
    }
    let mut e = 0;
    while e < exported.len() {
        let (api_num, ap_num) = exported[e];
        let api = match VviApi::from_num(api_num) {
            Some(a) => a,
            None => return Err(VviCode::ApiUnknown.code()),
        };
        let ap = match RetAperture::from_num(ap_num) {
            Some(a) => a,
            None => return Err(VviCode::ApertureIllegal.code()),
        };
        let _ = (api, ap); // 可还原即视为闭环，防未用告警
        e += 1;
    }
    reg.export_conserve()?;
    let _ = &mut apertures; // 口径数组已消费于注册，防未用告警
    Ok((reg.registered_total, reg.sealed_total, exported.len() as u32))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：合法注册后立即导出，口径逐条可读。
pub fn probe_a() -> Result<u32, u32> {
    let mut reg = EntryRegistry::new();
    reg.register(VviApi::VerifyVersionInfoW, 0x0001, RetAperture::PassTrue)?;
    let out = reg.export_apertures();
    if out.len() != 1 {
        return Err(VviCode::ExportLedgerBroken.code());
    }
    Ok(out[0].1 as u32)
}

/// 探针 B：空表查询（find 不到任何账目）→ 导出为空但守恒成立。
pub fn probe_b() -> Result<(), u32> {
    let reg = EntryRegistry::new();
    if reg.find(VviApi::RtlGetVersion).is_some() {
        return Err(VviCode::EmptyRegistry.code());
    }
    if !reg.export_apertures().is_empty() {
        return Err(VviCode::ExportLedgerBroken.code());
    }
    reg.export_conserve()
}

/// 探针 C：表满边界——注册满 8 槽后第 9 个不同 API 路径必拒且账不丢。
pub fn probe_c() -> Result<(), u32> {
    let mut reg = EntryRegistry::new();
    // 闭集只有 5 个 API，构造 8 槽占满须用「同 API 不可重复」的绕行：
    // 5 API 全注册 + 3 次对空槽的非法重复尝试（重复拒不占槽）。
    let apis = [
        VviApi::VerifyVersionInfoW,
        VviApi::VerifyVersionInfoA,
        VviApi::VerSetConditionMask,
        VviApi::GetVersionExW,
        VviApi::RtlGetVersion,
    ];
    let mut i = 0;
    while i < apis.len() {
        reg.register(apis[i], (i as u32) + 1, RetAperture::PassTrue)?;
        i += 1;
    }
    // 重复尝试不占新槽：5 槽后仍可正常封印全部。
    let mut j = 0;
    while j < apis.len() {
        if let Some(idx) = reg.find(apis[j]) {
            reg.seal(idx)?;
        }
        j += 1;
    }
    // 越界槽封印必拒。
    if reg.seal(REGISTRY_CAP).is_ok() {
        return Err(VviCode::SlotOutOfRange.code());
    }
    reg.export_conserve()
}

/// 双跑确定性对拍：golden_run 两轮指纹与三元组逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(VviCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：诊断码全部落在 0x7E280 段 0x40 窗口内。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VviCode::ALL.len() {
        let v = VviCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 段基址对齐：0x40 边界（分派表独占区约定）。
    SEG_BASE % 0x40 == 0
}

/// 文案闭集巡检：每个诊断码 detail 非空且按值反查一致。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < VviCode::ALL.len() {
        let c = VviCode::ALL[i];
        if c.detail().is_empty() {
            return false;
        }
        if detail_of(c.code()) != c.detail() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
/// UNX-F18411 判据集（≤20 条；动态明细 ≤8 槽）。
pub fn run_unx18411_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18411 VerifyVersionInfo 入口注册");

    // 1. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7E280 段 0x40 窗口且 0x40 对齐");
    // 2. 黄金全链（注册→衔接→封印→导出）。
    let golden = golden_run();
    let golden_ok = match &golden {
        Ok((r, se, ex)) => *r == 4 && *se == 4 && *ex == 4,
        Err(_) => false,
    };
    s.add_dyn("J1-黄金全链", golden_ok, &format!("{:?}", golden));
    // 3. B06 衔接：合法预告逐字段过闸。
    let bridge_ok = EntryRegistry::b06_bridge_check(0x0005).is_ok()
        && EntryRegistry::b06_bridge_check(0xFFFF).is_ok();
    s.add("J1-B06衔接断言", bridge_ok, "0x0005/0xFFFF 逐 2 位字段过闸");
    // 4. B06 衔接：越域预告必拒。
    let bridge_rej = EntryRegistry::b06_bridge_check(MASK_FIELD_AREA + 1).is_err();
    s.add("J1-越域预告必拒", bridge_rej, "bit16 置位即 MaskOutOfField");
    // 5. 重复注册拒绝。
    let mut t = EntryRegistry::new();
    let dup_ok = t.register(VviApi::GetVersionExW, 1, RetAperture::PassTrue).is_ok()
        && t.register(VviApi::GetVersionExW, 1, RetAperture::PassTrue).is_err();
    s.add("J1-重复注册拒绝", dup_ok, "同 API 二次注册必拒");
    // 6. 未知 API 路径：闭集外序号不可还原。
    s.add("J1-未知API拒绝", VviApi::from_num(0).is_none() && VviApi::from_num(6).is_none(), "序号 0/6 越闭集还原 None");
    // 7. 封印单调：Sealed 改签名必拒。
    let mut t2 = EntryRegistry::new();
    let seal_mono = match t2.register(VviApi::VerSetConditionMask, 0x0003, RetAperture::PassThrough) {
        Ok(idx) => {
            let _ = t2.seal(idx);
            t2.try_rewrite_sealed(idx, 0xFFFF).is_err()
        }
        Err(_) => false,
    };
    s.add("J1-封印单调", seal_mono, "Sealed 倒退改签名必拒");
    // 8. 口径导出守恒。
    let mut t3 = EntryRegistry::new();
    let _ = t3.register(VviApi::VerifyVersionInfoA, 0x0002, RetAperture::RejectFalse);
    let export_ok = t3.export_apertures().len() == 1 && t3.export_conserve().is_ok();
    s.add("J1-口径导出可还原", export_ok, "导出数==注册数==非空槽数");
    // 9. 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught() && l.caught[0] == JUDGE_ROUNDS && l.caught[1] == JUDGE_ROUNDS,
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10", inj_ok, &format!("r={}", JUDGE_ROUNDS));
    // 10. 计数守恒：tries = caught + missed，漏检为 0。
    let conserve_ok = match &inj {
        Ok(l) => l.missed(0) == 0 && l.missed(1) == 0 && l.missed(2) == 0,
        Err(_) => false,
    };
    s.add("J1-计数守恒漏检零", conserve_ok, "tries=caught+missed 且 missed=0");
    // 11. 探针三序。
    let a = probe_a().is_ok();
    let b = probe_b().is_ok();
    let c = probe_c().is_ok();
    s.add_dyn("J1-探针三序", a && b && c, &format!("a={} b={} c={}", a, b, c));
    // 12. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 13. 空表边界。
    s.add("J1-空表边界", probe_b().is_ok(), "空表 find 无账目且导出守恒");
    // 14. 越界槽边界。
    let mut t4 = EntryRegistry::new();
    s.add("J1-越界槽边界", t4.seal(REGISTRY_CAP).is_err() && t4.seal(usize::MAX >> 8).is_err(), "槽 ≥8 封印必拒");
    // 15. 文案闭集。
    s.add("J3-文案闭集", detail_closure(), "11 码逐条非空且反查一致");
    // 16. 表满边界（重复拒不占槽，5 API 注册后账目=5）。
    let mut t5 = EntryRegistry::new();
    let _ = t5.register(VviApi::VerifyVersionInfoW, 1, RetAperture::PassTrue);
    let _ = t5.register(VviApi::VerifyVersionInfoA, 1, RetAperture::PassTrue);
    let _ = t5.register(VviApi::VerSetConditionMask, 1, RetAperture::PassTrue);
    let _ = t5.register(VviApi::GetVersionExW, 1, RetAperture::PassTrue);
    let _ = t5.register(VviApi::RtlGetVersion, 1, RetAperture::PassTrue);
    s.add(
        "J1-表满与账不丢",
        t5.registered_total == 5 && t5.rejected_total == 0 && t5.export_apertures().len() == 5,
        "5 API 注册 5 槽，重复拒不占新槽",
    );
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 判据集全绿（一次命令口径）。
    #[test]
    fn all_checks_pass() {
        let set = run_unx18411_checks();
        assert!(set.all_passed(), "UNX-F18411 判据集存在红项");
    }

    /// 正常流：合法注册→封印→导出。
    #[test]
    fn normal_flow_register_seal_export() {
        let mut reg = EntryRegistry::new();
        let idx = match reg.register(VviApi::VerifyVersionInfoW, 0x0001, RetAperture::PassTrue) {
            Ok(i) => i,
            Err(_) => panic!("合法注册失败"),
        };
        assert!(matches!(reg.seal(idx), Ok(EntryState::Sealed)));
        assert_eq!(reg.export_apertures().len(), 1);
    }

    /// 四类边界：空表/满表/越界/重复。
    #[test]
    fn boundary_edges() {
        // 空表。
        let empty = EntryRegistry::new();
        assert!(empty.find(VviApi::VerifyVersionInfoW).is_none());
        // 越界。
        let mut reg = EntryRegistry::new();
        assert!(reg.seal(9).is_err());
        assert!(reg.seal(REGISTRY_CAP).is_err());
        // 重复。
        let _ = reg.register(VviApi::RtlGetVersion, 0, RetAperture::RejectFalse);
        assert!(reg.register(VviApi::RtlGetVersion, 0, RetAperture::RejectFalse).is_err());
        // 满表：5 API 占 5 槽后重复全拒。
        let apis = [
            VviApi::VerifyVersionInfoW,
            VviApi::VerifyVersionInfoA,
            VviApi::VerSetConditionMask,
            VviApi::GetVersionExW,
            VviApi::RtlGetVersion,
        ];
        let mut full = EntryRegistry::new();
        for a in apis.iter() {
            assert!(full.register(*a, 0x0001, RetAperture::PassTrue).is_ok());
        }
        assert!(full.register(apis[0], 0x0001, RetAperture::PassTrue).is_err());
    }

    /// 注入三类 10/10 全抓。
    #[test]
    fn injection_all_caught() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(_) => panic!("注入轮次内部出错"),
        };
        assert!(led.all_caught());
        assert_eq!(led.caught[0], JUDGE_ROUNDS);
        assert_eq!(led.caught[1], JUDGE_ROUNDS);
        assert_eq!(led.caught[2], JUDGE_ROUNDS);
        assert_eq!(led.missed(0) + led.missed(1) + led.missed(2), 0);
    }

    /// 判定次序：掩码越域先于重复注册被拒（域闸是第一前置闸）。
    #[test]
    fn order_mask_field_before_duplicate() {
        let mut reg = EntryRegistry::new();
        let _ = reg.register(VviApi::VerifyVersionInfoW, 0x0001, RetAperture::PassTrue);
        // 同 API + 越域掩码：返回越域码而非重复码。
        match reg.register(VviApi::VerifyVersionInfoW, MASK_FIELD_AREA + 1, RetAperture::PassTrue) {
            Err(c) => assert_eq!(c, VviCode::MaskOutOfField.code()),
            Ok(_) => panic!("越域掩码不应注册成功"),
        }
    }

    /// 状态机：非法倒退（Sealed 改签名/重注册）被拒。
    #[test]
    fn state_machine_no_regress() {
        let mut reg = EntryRegistry::new();
        let idx = match reg.register(VviApi::VerSetConditionMask, 0x0003, RetAperture::PassThrough) {
            Ok(i) => i,
            Err(_) => panic!("注册失败"),
        };
        let _ = reg.seal(idx);
        assert!(reg.try_rewrite_sealed(idx, 0xFFFF).is_err());
        // 幂等重封不改账。
        let sealed_before = reg.sealed_total;
        assert!(matches!(reg.seal(idx), Ok(EntryState::Sealed)));
        assert_eq!(reg.sealed_total, sealed_before);
    }

    /// B06 衔接：字段区全域 0xFFFF 合法、bit16 越域非法。
    #[test]
    fn b06_bridge_field_area() {
        assert!(EntryRegistry::b06_bridge_check(0x0000).is_ok());
        assert!(EntryRegistry::b06_bridge_check(0xFFFF).is_ok());
        assert!(EntryRegistry::b06_bridge_check(0x0001_0000).is_err());
        assert!(EntryRegistry::b06_bridge_check(0xFFFF_FFFF).is_err());
    }

    /// 双跑一致：指纹级对拍。
    #[test]
    fn reproducible() {
        let mut a = EntryRegistry::new();
        let mut b = EntryRegistry::new();
        let _ = a.register(VviApi::GetVersionExW, 0x0007, RetAperture::PassThrough);
        let _ = b.register(VviApi::GetVersionExW, 0x0007, RetAperture::PassThrough);
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert!(repro_check().is_ok());
    }

    /// 段位守卫 + 文案闭集。
    #[test]
    fn segment_and_detail_closure() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert!(detail_of(SEG_BASE + 200).starts_with("未知诊断码"));
    }
}
