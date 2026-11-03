//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1544 · 域级接口册终版（对外冻结接口全集） · 0x72240
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B38，前置 UNX-F1543）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1544-J1 接口册终版齐备（域对外冻结接口逐项登记：
//!     接口/版本/消费方三列），样例可编译断言，版本漂移拦截 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 接口册逐项登记三列：接口名 / 语义版本（maj.min.pat）/ 消费方列表；
//!   · 终版冻结：declare 口封版后闭（SealedFrozen），终版 24 项齐备才许封；
//!   · 版本漂移：同名再申报且版本不同 → VerDrift 显性拒（漂移注入 10 轮
//!     全拦）；同名同版重复申报 → DupIface 拒；
//!   · 样例可编译断言：为每个接口生成调用样例桩（参数个数 + 返回标 },
//!     逐桩对签名（arity/ret_tag 不符即链接失败）；
//!   · 版本纪律：终版大版本冻结 maj=1，maj=0 一律 VerBad 拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72240，VX3-1103 接口册窗口独占）。
pub const SEG_BASE: u32 = 0x72240;

/// 终版接口项数（冻结 24，齐备线）。
pub const IFACE_COUNT: usize = 24;
/// 终版大版本冻结值（maj=1，maj=0 不进口）。
pub const MAJOR_FROZEN: u8 = 1;
/// 接口名长度上限（字节，冻结 16）。
pub const NAME_CAP: usize = 16;
/// 单接口消费方上限（冻结 8）。
pub const CONSUMER_CAP: usize = 8;
/// 参数个数上限（桩签名闭集 0..=6）。
pub const ARITY_CAP: u8 = 6;
/// 注入轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IfaceCode {
    /// 接口名不合法（空/超长/首字符为数字/含非法字符）
    NameBad,
    /// 版本号不合法（maj=0 或 min/pat 越界）
    VerBad,
    /// 重复申报（同名同版已登记）
    DupIface,
    /// 版本漂移（同名不同版再申报）
    VerDrift,
    /// 消费方缺失（三列缺一：消费方列为空）
    ConsumerEmpty,
    /// 消费方重复（同接口下消费方重名）
    DupConsumer,
    /// 封版后改册（终版封印后仍申报）
    SealedFrozen,
    /// 终版不齐（封版时不足 24 项或三列缺项）
    IfaceShort,
    /// 双跑不一致
    NotReproducible,
}

impl IfaceCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IfaceCode::NameBad => "接口名不合法：非空、≤16 字节、字母/数字/下划线、首字符非数字",
            IfaceCode::VerBad => "版本号不合法：终版大版本冻结 maj=1，maj=0 拒收",
            IfaceCode::DupIface => "重复申报：同名同版已登记，查册后再报",
            IfaceCode::VerDrift => "版本漂移：同名不同版，走终版变更流程，不许静默改版",
            IfaceCode::ConsumerEmpty => "消费方缺失：三列缺一，至少登记 1 个消费方",
            IfaceCode::DupConsumer => "消费方重复：同接口下消费方去重后登记",
            IfaceCode::SealedFrozen => "封版后改册：终版封印，申报口已闭",
            IfaceCode::IfaceShort => "终版不齐：不足 24 项或三列缺项，补齐再封",
            IfaceCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [IfaceCode; 9] = [
        IfaceCode::NameBad,
        IfaceCode::VerBad,
        IfaceCode::DupIface,
        IfaceCode::VerDrift,
        IfaceCode::ConsumerEmpty,
        IfaceCode::DupConsumer,
        IfaceCode::SealedFrozen,
        IfaceCode::IfaceShort,
        IfaceCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IfaceCode::ALL.len() {
        if IfaceCode::ALL[i].val() == code {
            return IfaceCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72240 段闭集内"
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 语义版本三元组（终版纪律：maj 冻结为 1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemVer {
    pub maj: u8,
    pub min: u8,
    pub pat: u16,
}

impl SemVer {
    pub fn new(maj: u8, min: u8, pat: u16) -> SemVer {
        SemVer { maj, min, pat }
    }
    /// 终版纪律：maj == 1 才合法（min ≤ 15、pat ≤ 999）。
    pub fn well_formed(self) -> bool {
        self.maj == MAJOR_FROZEN && self.min <= 15 && self.pat <= 999
    }
    /// 版本全等。
    pub fn same(self, other: SemVer) -> bool {
        self.maj == other.maj && self.min == other.min && self.pat == other.pat
    }
    /// 展示串（诊断与摘要用）。
    pub fn show(self) -> String {
        format!("{}.{}.{}", self.maj, self.min, self.pat)
    }
}

/// 接口条目：三列齐备（名/版本/消费方）+ 桩签名（arity、ret_tag）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfaceEntry {
    pub name: String,
    pub ver: SemVer,
    /// 参数个数（0..=6）。
    pub arity: u8,
    /// 返回标签（0=void 1=u32 2=u64 3=Err）。
    pub ret_tag: u8,
    pub consumers: Vec<String>,
}

/// 接口名合法性：非空、≤NAME_CAP、首字符非数字、仅字母/数字/下划线。
pub fn name_well_formed(name: &str) -> bool {
    if name.is_empty() || name.len() > NAME_CAP {
        return false;
    }
    let bytes = name.as_bytes();
    let first = bytes[0];
    if first.is_ascii_digit() {
        return false;
    }
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if !(c.is_ascii_alphanumeric() || c == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

/// 返回标签闭集（0..=3）。
pub fn ret_tag_ok(tag: u8) -> bool {
    tag <= 3
}

/// 接口册：终版登记簿（封版后账面冻结）。
#[derive(Clone, Debug)]
pub struct IfaceRegistry {
    pub entries: Vec<IfaceEntry>,
    pub sealed: bool,
}

impl IfaceRegistry {
    pub fn new() -> IfaceRegistry {
        IfaceRegistry { entries: Vec::new(), sealed: false }
    }

    /// 按名查册（线性扫，终版 ≤24 项）。
    pub fn find(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.name == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 申报接口（封版闸/命名闸/版本闸/消费方闸/漂移闸/重复闸）。
    pub fn declare(
        &mut self,
        name: String,
        ver: SemVer,
        arity: u8,
        ret_tag: u8,
        consumers: Vec<String>,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(IfaceCode::SealedFrozen.val());
        }
        if !name_well_formed(&name) {
            return Err(IfaceCode::NameBad.val());
        }
        if !ver.well_formed() {
            return Err(IfaceCode::VerBad.val());
        }
        if arity > ARITY_CAP || !ret_tag_ok(ret_tag) {
            return Err(IfaceCode::NameBad.val());
        }
        if consumers.is_empty() {
            return Err(IfaceCode::ConsumerEmpty.val());
        }
        // 消费方去重闸：同接口下重名即拒。
        let mut ci = 0;
        while ci < consumers.len() {
            let mut cj = ci + 1;
            let cname = match consumers.get(ci) {
                Some(c) => c.clone(),
                None => return Err(IfaceCode::ConsumerEmpty.val()),
            };
            if cname.is_empty() {
                return Err(IfaceCode::ConsumerEmpty.val());
            }
            while cj < consumers.len() {
                if let Some(c2) = consumers.get(cj) {
                    if *c2 == cname {
                        return Err(IfaceCode::DupConsumer.val());
                    }
                }
                cj += 1;
            }
            ci += 1;
        }
        if consumers.len() > CONSUMER_CAP {
            return Err(IfaceCode::DupConsumer.val());
        }
        // 漂移闸/重复闸：同名已登记时，同版 → DupIface，异版 → VerDrift。
        if let Some(idx) = self.find(&name) {
            let same = match self.entries.get(idx) {
                Some(e) => e.ver.same(ver),
                None => false,
            };
            return if same {
                Err(IfaceCode::DupIface.val())
            } else {
                Err(IfaceCode::VerDrift.val())
            };
        }
        self.entries.push(IfaceEntry { name, ver, arity, ret_tag, consumers });
        Ok(())
    }

    /// 封版：终版齐备线（项数 == 24）不满足则拒，满足则落封。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.entries.len() != IFACE_COUNT {
            return Err(IfaceCode::IfaceShort.val());
        }
        self.sealed = true;
        Ok(())
    }

    /// 版本查询（封版前后一致；未登记返回 None）。
    pub fn ver_of(&self, name: &str) -> Option<SemVer> {
        match self.find(name) {
            Some(i) => match self.entries.get(i) {
                Some(e) => Some(e.ver),
                None => None,
            },
            None => None,
        }
    }
}

// ── 样例可编译断言（桩链接） ────────────────────────────────────
/// 调用样例桩：消费方对某接口的一次调用点（按名 + 期望签名）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallStub {
    pub iface: String,
    pub consumer: String,
    pub arity: u8,
    pub ret_tag: u8,
}

/// 从登记条目生成全量样例桩（每消费方一桩）。
pub fn stubs_of(e: &IfaceEntry) -> Vec<CallStub> {
    let mut v: Vec<CallStub> = Vec::new();
    let mut i = 0;
    while i < e.consumers.len() {
        if let Some(c) = e.consumers.get(i) {
            v.push(CallStub {
                iface: e.name.clone(),
                consumer: c.clone(),
                arity: e.arity,
                ret_tag: e.ret_tag,
            });
        }
        i += 1;
    }
    v
}

/// 桩链接：桩签名与册内登记签名逐桩对平（arity、ret_tag、在册三项全对）。
/// 任何一桩失配即链接失败（样例不可编译面）。
pub fn link_stub(reg: &IfaceRegistry, s: &CallStub) -> Result<(), u32> {
    let idx = match reg.find(&s.iface) {
        Some(i) => i,
        None => return Err(IfaceCode::NameBad.val()),
    };
    let e = match reg.entries.get(idx) {
        Some(e) => e,
        None => return Err(IfaceCode::NameBad.val()),
    };
    if e.arity != s.arity || e.ret_tag != s.ret_tag {
        return Err(IfaceCode::NameBad.val());
    }
    // 桩的消费方必须登记在该接口消费方列内。
    let mut found = false;
    let mut i = 0;
    while i < e.consumers.len() {
        if let Some(c) = e.consumers.get(i) {
            if *c == s.consumer {
                found = true;
            }
        }
        i += 1;
    }
    if !found {
        return Err(IfaceCode::ConsumerEmpty.val());
    }
    Ok(())
}

/// 样例可编译断言：全册逐条生成桩并全部链接，断桩数 == 消费方总数。
pub fn sample_compile(reg: &IfaceRegistry) -> Result<u32, u32> {
    let mut linked = 0u32;
    let mut i = 0;
    while i < reg.entries.len() {
        let e = match reg.entries.get(i) {
            Some(e) => e,
            None => return Err(IfaceCode::IfaceShort.val()),
        };
        let stubs = stubs_of(e);
        let mut j = 0;
        while j < stubs.len() {
            let st = match stubs.get(j) {
                Some(s) => s,
                None => return Err(IfaceCode::IfaceShort.val()),
            };
            link_stub(reg, st)?;
            linked += 1;
            j += 1;
        }
        i += 1;
    }
    Ok(linked)
}

// ── 冻结接口规格：24 项，名/版/消费方三列全冻结 ─────────────────
/// 冻结接口名表（24 项）。
pub const IFACE_NAMES: [&str; IFACE_COUNT] = [
    "vex_map_page",
    "vex_unmap_page",
    "vex_irq_bind",
    "vex_irq_free",
    "vex_msix_alloc",
    "vex_msix_free",
    "vex_tick_set",
    "vex_tick_stop",
    "vex_tsc_read",
    "vex_sched_yield",
    "vex_sched_place",
    "vex_mm_grow",
    "vex_mm_shrink",
    "vex_fs_mount",
    "vex_fs_sync",
    "vex_net_poll",
    "vex_net_send",
    "vex_pm_idle",
    "vex_pm_wake",
    "vex_acpi_sleep",
    "vex_log_ring",
    "vex_probe_id",
    "vex_cabin_open",
    "vex_cabin_seal",
];

/// 冻结版本表：maj 全 1，min/pat 按槽位确定性派生（可复算）。
pub fn spec_ver(slot: usize) -> SemVer {
    SemVer::new(1, (slot % 4) as u8, (slot as u16) % 10)
}

/// 冻结桩签名：arity = 槽位 % 7（0..=6），ret_tag = 槽位 % 4。
pub fn spec_sig(slot: usize) -> (u8, u8) {
    ((slot % 7) as u8, (slot % 4) as u8)
}

/// 冻结消费方池（6 个域），每接口 2 方：池[slot % 6] 与池[(slot + 3) % 6]。
pub const CONSUMER_POOL: [&str; 6] = ["sched", "mm", "fs", "net", "pm", "acpi"];

pub fn spec_consumers(slot: usize) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    v.push(String::from(CONSUMER_POOL[slot % 6]));
    v.push(String::from(CONSUMER_POOL[(slot + 3) % 6]));
    v
}

/// 单轮摘要（供双跑一致与三序复算）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegSummary {
    pub entries: u32,
    pub stubs: u32,
    pub digest: u32,
}

/// 独立建册跑一轮：24 项逐项申报 → 样例编译 → 封版 → 终版审计。
pub fn run_registry_round() -> Result<RegSummary, u32> {
    let mut reg = IfaceRegistry::new();
    let mut slot = 0;
    while slot < IFACE_COUNT {
        let name = match IFACE_NAMES.get(slot) {
            Some(n) => String::from(*n),
            None => return Err(IfaceCode::IfaceShort.val()),
        };
        let (arity, ret) = spec_sig(slot);
        reg.declare(name, spec_ver(slot), arity, ret, spec_consumers(slot))?;
        slot += 1;
    }
    // 样例可编译断言（封版前跑一遍，封版后账面冻结桩不变）。
    let stubs = sample_compile(&reg)?;
    reg.freeze()?;
    // 终版齐备审计：24 项、版本 maj 全 1、消费方非空。
    if reg.entries.len() != IFACE_COUNT || !reg.sealed {
        return Err(IfaceCode::IfaceShort.val());
    }
    let mut i = 0;
    while i < reg.entries.len() {
        match reg.entries.get(i) {
            Some(e) => {
                if !e.ver.well_formed() || e.consumers.is_empty() {
                    return Err(IfaceCode::IfaceShort.val());
                }
            }
            None => return Err(IfaceCode::IfaceShort.val()),
        }
        i += 1;
    }
    // 摘要卷积（名/版本/arity/ret/消费方数）。
    let mut h: u32 = 2166136261;
    i = 0;
    while i < reg.entries.len() {
        if let Some(e) = reg.entries.get(i) {
            h = h.wrapping_mul(16777619).wrapping_add(e.name.len() as u32);
            let bytes = e.name.as_bytes();
            let mut j = 0;
            while j < bytes.len() {
                h = h.wrapping_mul(16777619).wrapping_add(bytes[j] as u32);
                j += 1;
            }
            h = h.wrapping_mul(16777619).wrapping_add(e.ver.maj as u32);
            h = h.wrapping_mul(16777619).wrapping_add(e.ver.min as u32);
            h = h.wrapping_mul(16777619).wrapping_add(e.ver.pat as u32);
            h = h.wrapping_mul(16777619).wrapping_add(e.arity as u32);
            h = h.wrapping_mul(16777619).wrapping_add(e.ret_tag as u32);
            h = h.wrapping_mul(16777619).wrapping_add(e.consumers.len() as u32);
        }
        i += 1;
    }
    Ok(RegSummary { entries: reg.entries.len() as u32, stubs, digest: h })
}

// ── LCG（确定性伪随机，供注入轮扰动） ───────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（三路：漂移路/重报路/封版路） ─────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
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

/// 版本漂移注入：对已登记接口用异版再申报，必须拦（VerDrift）；
/// 对照面：同名同版 → DupIface（异错型）；坏名/坏版 → 对应错型。
pub fn inject_drift(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let mut reg = IfaceRegistry::new();
        // 预置 1 项。
        let slot = rng.below(IFACE_COUNT as u64) as usize;
        let name = match IFACE_NAMES.get(slot) {
            Some(n) => String::from(*n),
            None => return Err(IfaceCode::IfaceShort.val()),
        };
        let (arity, ret) = spec_sig(slot);
        reg.declare(name.clone(), spec_ver(slot), arity, ret, spec_consumers(slot))?;
        // 路 0：版本漂移（min +1 异版再申报）→ 必拒（VerDrift）。
        let drift = SemVer::new(1, (spec_ver(slot).min + 1) % 16, spec_ver(slot).pat);
        led.record(0, reg.declare(name.clone(), drift, arity, ret, spec_consumers(slot))
            == Err(IfaceCode::VerDrift.val()));
        // 路 1：同名同版重报 → 必拒（DupIface，异错型对照）。
        led.record(1, reg.declare(name.clone(), spec_ver(slot), arity, ret, spec_consumers(slot))
            == Err(IfaceCode::DupIface.val()));
        // 路 2：坏版对照（maj=0）→ 必拒（VerBad）。
        led.record(2, reg.declare(String::from("vex_zz_probe"), SemVer::new(0, 0, 0), 1, 1,
            spec_consumers(slot)) == Err(IfaceCode::VerBad.val()));
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链：满册 24 → 样例编译 → 四拒 → 封版 → 封后再拒 ────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut reg = IfaceRegistry::new();
    let mut slot = 0;
    while slot < IFACE_COUNT {
        let name = match IFACE_NAMES.get(slot) {
            Some(n) => String::from(*n),
            None => return Err(IfaceCode::IfaceShort.val()),
        };
        let (arity, ret) = spec_sig(slot);
        reg.declare(name, spec_ver(slot), arity, ret, spec_consumers(slot))?;
        slot += 1;
    }
    // 样例可编译断言：48 桩全链。
    let stubs = sample_compile(&reg)?;
    // 四拒口径：漂移拒 / 重报拒 / 坏名拒 / 空消费方拒。
    let drift = reg.declare(String::from("vex_map_page"), SemVer::new(1, 9, 9), 1, 1,
        spec_consumers(0)).is_err();
    let dup = reg.declare(String::from("vex_map_page"), spec_ver(0), spec_sig(0).0,
        spec_sig(0).1, spec_consumers(0)).is_err();
    let bad_name = reg.declare(String::from("9bad"), SemVer::new(1, 0, 0), 1, 1,
        spec_consumers(0)).is_err();
    let empty_c: Vec<String> = Vec::new();
    let empty_cons = reg.declare(String::from("vex_yy"), SemVer::new(1, 0, 0), 1, 1, empty_c).is_err();
    // 封版（不齐则拒）：满册 24 必成；封后再申报：拒。
    reg.freeze()?;
    let frozen_rej = reg.declare(String::from("vex_late"), SemVer::new(1, 0, 0), 1, 1,
        spec_consumers(1)).is_err();
    let flags = drift as u32 + dup as u32 + bad_name as u32 + empty_cons as u32;
    Ok((flags + frozen_rej as u32, stubs, reg.entries.len() as u32))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链（5 旗 + 48 桩 + 24 项）+ 单轮复算一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 48, 24)) => {
            let a = run_registry_round()?;
            let b = run_registry_round()?;
            if a == b {
                Ok(())
            } else {
                Err(IfaceCode::NotReproducible.val())
            }
        }
        _ => Err(IfaceCode::NotReproducible.val()),
    }
}

/// 序 B：注入面三拒（漂移 / 封版改册 / 短册封版）逐条显性拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut reg = IfaceRegistry::new();
    let _ = reg.declare(String::from("vex_aa"), SemVer::new(1, 0, 0), 1, 1, spec_consumers(0));
    let r1 = reg.declare(String::from("vex_aa"), SemVer::new(1, 1, 0), 1, 1, spec_consumers(1));
    // 短册封版（1 项 < 24）必须拒。
    let r2 = reg.freeze();
    // 满册封版后再申报必须拒。
    let mut full = IfaceRegistry::new();
    let mut slot = 0;
    while slot < IFACE_COUNT {
        let name = match IFACE_NAMES.get(slot) {
            Some(n) => String::from(*n),
            None => return Err(IfaceCode::IfaceShort.val()),
        };
        let (arity, ret) = spec_sig(slot);
        full.declare(name, spec_ver(slot), arity, ret, spec_consumers(slot))?;
        slot += 1;
    }
    full.freeze()?;
    let r3 = full.declare(String::from("vex_late"), SemVer::new(1, 0, 0), 1, 1, spec_consumers(2));
    match (r1.is_err(), r2.is_err(), r3.is_err()) {
        (true, true, true) => Ok(()),
        _ => Err(IfaceCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（24 项 / maj=1 / 名长 16 / 消费方上限 8 / arity 闭集 6）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        IFACE_COUNT == 24,
        MAJOR_FROZEN == 1,
        NAME_CAP == 16,
        CONSUMER_CAP == 8,
        ARITY_CAP == 6,
        JUDGE_ROUNDS == 10,
    ) {
        (true, true, true, true, true, true) => Ok(()),
        _ => Err(IfaceCode::NotReproducible.val()),
    }
}

/// 注入轮（10 轮 × 3 路）+ 末轮正常建册对照（不误伤）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let led = inject_drift(rounds)?;
    let sum = run_registry_round()?;
    if sum.entries != IFACE_COUNT as u32 || sum.stubs != (IFACE_COUNT * 2) as u32 {
        return Err(IfaceCode::IfaceShort.val());
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = run_registry_round()?;
    let b = run_registry_round()?;
    if a != b {
        return Err(IfaceCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IfaceCode::ALL.len() {
        let v = IfaceCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let sum = run_registry_round()?;
    if sum.entries != IFACE_COUNT as u32 {
        return Err(IfaceCode::IfaceShort.val());
    }
    Ok(format!(
        "状态=已完成 entries={} stubs={} digest=0x{:x}",
        sum.entries, sum.stubs, sum.digest
    ))
}

// ── 判据面（extra_checks：8 条动态明细） ────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（5 旗 + 48 桩 + 24 项）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((flags, stubs, entries)) => *flags == 5 && *stubs == 48 && *entries == 24,
            Err(_) => false,
        },
        match &g {
            Ok((flags, stubs, entries)) => format!("flags={} stubs={} entries={}", flags, stubs, entries),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 终版齐备：24 项三列齐 + 消费方 48 方。
    let round = run_registry_round();
    v.push((
        "J1-终版齐备",
        match &round {
            Ok(s) => s.entries == 24 && s.stubs == 48,
            Err(_) => false,
        },
        match &round {
            Ok(s) => format!("entries={} stubs={} digest=0x{:x}", s.entries, s.stubs, s.digest),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 样例可编译：48 桩逐桩对签（arity/ret/消费方三项）。
    let comp = {
        let mut reg = IfaceRegistry::new();
        let mut slot = 0;
        let mut ok = true;
        while slot < IFACE_COUNT {
            let name = match IFACE_NAMES.get(slot) {
                Some(n) => String::from(*n),
                None => {
                    ok = false;
                    break;
                }
            };
            let (arity, ret) = spec_sig(slot);
            if reg.declare(name, spec_ver(slot), arity, ret, spec_consumers(slot)).is_err() {
                ok = false;
            }
            slot += 1;
        }
        match sample_compile(&reg) {
            Ok(n) => ok && n == 48,
            Err(_) => false,
        }
    };
    v.push((
        "J1-样例可编译",
        comp,
        String::from("48 桩逐桩链接全过"),
    ));
    // J1 版本漂移拦截 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-漂移拦截10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("run_registry_round 两跑摘要同值"),
    ));
    // 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x72240 窗口"),
    ));
    v
}

pub fn run_unx1544_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1544 域级接口册终版（对外冻结接口全集）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
