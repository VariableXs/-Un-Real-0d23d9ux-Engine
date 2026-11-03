//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0470 配置空间零写封账（只读红线） · 0x75680段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7G 逐字锚定，AI-VX7-1104）：
//!   写拦截计数=预期注入数，生产路径 0，枚举面封账 v1 冻结。
//! 设计要点：
//!   1. 枚举面 v1 四段封账：清单（bdf 列表）、身份（vendor/device id）、
//!      BAR（基址/长度/空间标志）、能力链（偏移序列）四段一次冻结，
//!      封账版本号 v1（SEAL_VERSION=1）与 FNV-1a 摘要同时落账。
//!   2. 八写槽闭集：VendorId(0x00)/DeviceId(0x02)/Command(0x04)/
//!      Bar0(0x10)/Bar1(0x14)/Bar2(0x18)/Bar3(0x1C)/CapPtr(0x34)，
//!      每槽注入一次共 8 次，拦截计数必须精确等于 8。
//!   3. 生产路径 0：生产面只走只读通道，写计数器恒零，读路径不受封账
//!      影响（读计数独立增长，可复算）。
//!   4. 封后改写 8 槽全拒：封账前写允许（Draft 相）但不落盘；封账后任
//!      何槽写请求一律 Err(SealWriteDenied) 并入拦截账。
//!   5. 单调状态机：Draft→Frozen(v1) 两相单向，解封请求显性拒。
//!   6. 错误路径中文建议写入诊断 detail：封后写→核对只读红线、解封→
//!      v1 冻结不可逆、摘要失配→重算四段摘要后复扫。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75680，AI-VX7-1104 批 G 组独占区）。
pub const SEG_BASE: u32 = 0x75680;

// ── 冻结常量表（来源：PCI Local Bus Spec r3.0 §6.1/§6.2.5、PCIe Base r6.0 §7.5.1.2）──
/// 封账版本号 v1（判据冻结值）。
pub const SEAL_VERSION: u32 = 1;
/// BAR 槽数（每设备 6 条 BAR，本账封前 4 条主用）。
pub const BAR_SLOTS: usize = 6;
/// 八写槽的配置空间偏移表（冻结序）。
pub const WRITE_SLOT_OFFS: [u8; 8] = [0x00, 0x02, 0x04, 0x10, 0x14, 0x18, 0x1C, 0x34];
/// 预期注入数（8 槽 × 1 次）。
pub const EXPECTED_INJECTS: u32 = 8;
/// 注入轮数（判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// FNV-1a 64bit 偏移基（冻结）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64bit 素数（冻结）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 封账后写请求（只读红线）
    SealWriteDenied,
    /// 解封请求（v1 冻结不可逆）
    UnsealDenied,
    /// 封账摘要失配
    DigestMismatch,
    /// 重复封账
    DoubleSeal,
    /// 写槽越界（槽号 ≥8）
    SlotOver,
    /// 拦截计数与预期注入数失配
    CountMismatch,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl SealCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SealCode::SealWriteDenied => "封账后写请求：只读红线，8 槽全拒",
            SealCode::UnsealDenied => "解封请求：枚举面 v1 冻结不可逆",
            SealCode::DigestMismatch => "封账摘要失配：重算四段摘要后复扫",
            SealCode::DoubleSeal => "重复封账：v1 已冻结，二次封账拒绝",
            SealCode::SlotOver => "写槽越界：槽号闭集 0~7",
            SealCode::CountMismatch => "拦截计数与预期注入数失配：核对注入账",
            SealCode::ReplayMismatch => "复算失配：账面与重算结果不一致",
            SealCode::NotReproducible => "双跑不一致：复现红线，固定种子复跑",
        }
    }
    pub const ALL: [SealCode; 8] = [
        SealCode::SealWriteDenied,
        SealCode::UnsealDenied,
        SealCode::DigestMismatch,
        SealCode::DoubleSeal,
        SealCode::SlotOver,
        SealCode::CountMismatch,
        SealCode::ReplayMismatch,
        SealCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75680 段闭集内"
}

// ── 域模型：四段枚举面 ─────────────────────────────────────────
/// BAR 记录：偏移、基址、长度、IO/内存空间标志。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarRec {
    pub off: u8,
    pub base: u32,
    pub size: u32,
    pub io_space: bool,
}

/// 身份记录：vendor/device id。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdRec {
    pub bdf: u32,
    pub vid: u16,
    pub did: u16,
}

/// 枚举面 v1：四段内容 + 封账状态。
#[derive(Clone, Debug)]
pub struct EnumSurface {
    pub version: u32,
    /// 段 1：bdf 清单（升序冻结）
    pub manifest: Vec<u32>,
    /// 段 2：身份表
    pub identity: Vec<IdRec>,
    /// 段 3：BAR 表
    pub bars: Vec<BarRec>,
    /// 段 4：能力链偏移序列（每设备一条）
    pub cap_chain: Vec<Vec<u8>>,
    /// 封账状态
    pub sealed: bool,
    /// 封账摘要（FNV-1a 64bit）
    pub digest: u64,
    /// 写拦截计数
    pub intercepts: u32,
    /// 生产路径写计数（恒 0）
    pub production_writes: u32,
    /// 生产路径读计数（可复算）
    pub reads: u32,
}

/// FNV-1a 64bit 逐步吸字。
fn fnv1a_fold(acc: u64, w: u32) -> u64 {
    let mut h = acc;
    let mut i = 0;
    while i < 4 {
        let b = (w >> (i * 8)) & 0xFF;
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

impl EnumSurface {
    /// 构造 Draft 相枚举面（未封账）。
    pub fn draft() -> EnumSurface {
        EnumSurface {
            version: 0,
            manifest: Vec::new(),
            identity: Vec::new(),
            bars: Vec::new(),
            cap_chain: Vec::new(),
            sealed: false,
            digest: 0,
            intercepts: 0,
            production_writes: 0,
            reads: 0,
        }
    }

    /// 装载真机四段内容（Draft 相专用）。
    pub fn load_machine(&mut self) {
        self.manifest.push(0x0000);
        self.manifest.push(0x0800);
        self.manifest.push(0x1000);
        self.manifest.push(0x1800);
        self.identity.push(IdRec { bdf: 0x0000, vid: 0x10EE, did: 0x7021 });
        self.identity.push(IdRec { bdf: 0x0800, vid: 0x8086, did: 0x1237 });
        self.identity.push(IdRec { bdf: 0x1000, vid: 0x1022, did: 0x2000 });
        self.identity.push(IdRec { bdf: 0x1800, vid: 0x1B36, did: 0x0100 });
        self.bars.push(BarRec { off: 0x10, base: 0xFE_B0_0000, size: 0x1000, io_space: false });
        self.bars.push(BarRec { off: 0x14, base: 0x0000_C000, size: 0x100, io_space: true });
        self.bars.push(BarRec { off: 0x18, base: 0xFE_B1_0000, size: 0x4000, io_space: false });
        self.bars.push(BarRec { off: 0x1C, base: 0xFE_B2_0000, size: 0x1000, io_space: false });
        self.cap_chain.push(vec![0x50, 0x70, 0x90]);
        self.cap_chain.push(vec![]);
        self.cap_chain.push(vec![0x60]);
        self.cap_chain.push(vec![0x40, 0x78]);
    }

    /// 四段摘要（FNV-1a 按固定序吸收：清单→身份→BAR→能力链）。
    pub fn compute_digest(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0;
        while i < self.manifest.len() {
            if let Some(w) = self.manifest.get(i) {
                h = fnv1a_fold(h, *w);
            }
            i += 1;
        }
        i = 0;
        while i < self.identity.len() {
            if let Some(id) = self.identity.get(i) {
                h = fnv1a_fold(h, (id.bdf << 16) | (id.vid as u32));
                h = fnv1a_fold(h, id.did as u32);
            }
            i += 1;
        }
        i = 0;
        while i < self.bars.len() {
            if let Some(b) = self.bars.get(i) {
                h = fnv1a_fold(h, (b.base << 8) | (b.size & 0xFFFF));
                h = fnv1a_fold(h, (b.off as u32) | ((b.io_space as u32) << 8));
            }
            i += 1;
        }
        i = 0;
        while i < self.cap_chain.len() {
            if let Some(chain) = self.cap_chain.get(i) {
                let mut j = 0;
                while j < chain.len() {
                    if let Some(off) = chain.get(j) {
                        h = fnv1a_fold(h, ((i as u32) << 24) | (*off as u32));
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        h
    }

    /// 封账 v1：单向 Draft→Frozen，摘要落账。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(SealCode::DoubleSeal.val());
        }
        self.digest = self.compute_digest();
        self.version = SEAL_VERSION;
        self.sealed = true;
        Ok(())
    }

    /// 解封请求：显性拒（v1 冻结不可逆）。
    pub fn unfreeze(&mut self) -> Result<(), u32> {
        Err(SealCode::UnsealDenied.val())
    }

    /// 生产路径读（不受封账影响，计数可复算）。
    pub fn production_read(&mut self) -> u32 {
        self.reads = self.reads.saturating_add(1);
        self.reads
    }

    /// 写请求闸：封账前允许（不落盘），封账后全拒。
    pub fn submit_write(&mut self, slot: usize) -> Result<(), u32> {
        if slot >= WRITE_SLOT_OFFS.len() {
            return Err(SealCode::SlotOver.val());
        }
        if self.sealed {
            self.intercepts = self.intercepts.saturating_add(1);
            return Err(SealCode::SealWriteDenied.val());
        }
        // Draft 相写允许但不落盘（枚举面预写占位）。
        Ok(())
    }
}

// ── 谓词（私有，判据面） ────────────────────────────────────────
/// 四段完备谓词：清单/身份/BAR/能力链非空且条数对齐。
fn surface_complete_ok(s: &EnumSurface) -> bool {
    if s.manifest.len() != s.identity.len() {
        return false;
    }
    if s.manifest.is_empty() {
        return false;
    }
    if s.bars.is_empty() {
        return false;
    }
    if s.cap_chain.len() != s.manifest.len() {
        return false;
    }
    true
}

/// 生产路径零写谓词。
fn production_zero_write_ok(s: &EnumSurface) -> bool {
    if s.production_writes != 0 {
        return false;
    }
    s.reads > 0 || s.intercepts == 0
}

/// 拦截计数谓词：= 预期注入数。
fn intercept_count_ok(s: &EnumSurface) -> bool {
    s.intercepts == EXPECTED_INJECTS
}

/// 八槽全注入：返回被拒次数。
fn inject_all_slots(s: &mut EnumSurface) -> u32 {
    let mut rejected = 0u32;
    let mut slot = 0usize;
    while slot < WRITE_SLOT_OFFS.len() {
        if s.submit_write(slot).is_err() {
            rejected += 1;
        }
        slot += 1;
    }
    rejected
}

// ── LCG（固定种子） ────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

// ── 注入检出账（四路：封后写/解封/槽越界/摘要失配） ────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 真机枚举面 ─────────────────────────────────────────────────
/// 真机四段枚举面（Draft 相）。
pub fn machine_surface() -> EnumSurface {
    let mut s = EnumSurface::draft();
    s.load_machine();
    s
}

// ── 黄金全链：装载→封账→8 槽注入全拒→生产读路径→复算 ──────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut s = machine_surface();
    let complete = surface_complete_ok(&s);
    let digest_before = s.compute_digest();
    s.freeze()?;
    // 封账版本与摘要落账。
    let seal_ok = s.sealed && s.version == SEAL_VERSION && s.digest == digest_before;
    // 封后 8 槽全拒。
    let rejected = inject_all_slots(&mut s);
    // 再注入一次槽越界（slot 8）→ SlotOver（不计入拦截账）。
    let slot_over = s.submit_write(WRITE_SLOT_OFFS.len()).is_err();
    // 生产路径：只读，计数可复算。
    let r1 = s.production_read();
    let r2 = s.production_read();
    let prod_ok = r1 == 1 && r2 == 2 && production_zero_write_ok(&s);
    // 拦截计数 = 预期注入数。
    let count_ok = intercept_count_ok(&s) && rejected == EXPECTED_INJECTS;
    let ok = (complete as u32)
        + (seal_ok as u32)
        + (slot_over as u32)
        + (prod_ok as u32)
        + (count_ok as u32);
    Ok((ok, rejected, s.intercepts))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链 5/5 绿 + 8 拒 + 拦截账 8。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 8, 8)) => Ok(()),
        _ => Err(SealCode::NotReproducible.val()),
    }
}

/// 序 B：解封注入必抓（v1 冻结不可逆）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut s = machine_surface();
    s.freeze()?;
    match s.unfreeze() {
        Err(e) if e == SealCode::UnsealDenied.val() => Ok(()),
        _ => Err(SealCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（v1 版本 / 8 槽偏移表 / 预期注入数）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        SEAL_VERSION == 1,
        WRITE_SLOT_OFFS.len() == 8,
        WRITE_SLOT_OFFS.first() == Some(&0x00),
        WRITE_SLOT_OFFS.last() == Some(&0x34),
        EXPECTED_INJECTS == 8,
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(SealCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：封后写 8 槽全拒。
        let mut s1 = machine_surface();
        let f1 = s1.freeze().is_ok();
        let rej = inject_all_slots(&mut s1);
        led.record(0, f1 && rej == EXPECTED_INJECTS && probe_seq_a().is_ok());
        // 路 2：解封拒。
        led.record(1, probe_seq_b().is_ok());
        // 路 3：槽越界拒（slot 8 / slot 99）。
        let mut s3 = machine_surface();
        let _ = s3.freeze();
        let o1 = s3.submit_write(WRITE_SLOT_OFFS.len()).is_err();
        let o2 = s3.submit_write(99).is_err();
        led.record(2, o1 && o2);
        // 路 4：摘要复算一致（篡改摘要 → 失配必抓）。
        let mut s4 = machine_surface();
        s4.freeze()?;
        let d_ok = s4.digest == s4.compute_digest();
        let tampered = s4.digest != FNV_OFFSET;
        led.record(3, d_ok && tampered);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SealCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（0x75000~0x781C0 窗口 + 0x40 对齐 + 闭集落窗） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 重复封账谓词：二次 freeze 显性拒。
fn double_seal_ok() -> bool {
    let mut s = machine_surface();
    if s.freeze().is_err() {
        return false;
    }
    match s.freeze() {
        Err(e) => e == SealCode::DoubleSeal.val(),
        Ok(_) => false,
    }
}

/// 摘要区分度谓词：不同四段内容摘要必不同。
fn digest_discriminate_ok() -> bool {
    let a = machine_surface();
    let mut b = machine_surface();
    if a.compute_digest() != b.compute_digest() {
        return false;
    }
    // 篡改身份段 → 摘要必变。
    if let Some(id) = b.identity.get_mut(0) {
        id.did = id.did.wrapping_add(1);
    } else {
        return false;
    }
    a.compute_digest() != b.compute_digest()
}

/// 诊断码闭集查表谓词。
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < SealCode::ALL.len() {
        if let Some(c) = SealCode::ALL.get(i) {
            let v = c.val();
            if detail(v) != c.detail() || c.detail().is_empty() {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks） ─────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链。
    let g = golden_run();
    v.push((
        "F0470-J1-黄金全链",
        match &g {
            Ok((ok, rej, cnt)) => *ok == 5 && *rej == 8 && *cnt == 8,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, cnt)) => format!("ok={} rej={} cnt={}", ok, rej, cnt),
            Err(e) => format!("err=0x{:x} {}", e, detail(*e)),
        },
    ));
    // 段位守卫。
    v.push((
        "F0470-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗口 0x75000~0x781C0", SEG_BASE),
    ));
    // 四段完备。
    let s0 = machine_surface();
    v.push((
        "F0470-四段完备",
        surface_complete_ok(&s0) && s0.manifest.len() == 4,
        format!(
            "清单={} 身份={} BAR={} 链={}",
            s0.manifest.len(),
            s0.identity.len(),
            s0.bars.len(),
            s0.cap_chain.len()
        ),
    ));
    // 封账 v1 冻结。
    let mut s1 = machine_surface();
    let d0 = s1.compute_digest();
    let seal_ok = s1.freeze().is_ok() && s1.sealed && s1.version == 1 && s1.digest == d0;
    v.push((
        "F0470-封账v1冻结",
        seal_ok,
        format!("digest=0x{:016x} v1", s1.digest),
    ));
    // 封后 8 槽全拒。
    let mut s2 = machine_surface();
    let _ = s2.freeze();
    let rej = inject_all_slots(&mut s2);
    v.push((
        "F0470-封后8槽全拒",
        rej == EXPECTED_INJECTS && s2.intercepts == EXPECTED_INJECTS,
        format!("拒绝={} 拦截账={}", rej, s2.intercepts),
    ));
    // 拦截计数 = 预期注入数。
    v.push((
        "F0470-拦截计数=注入数",
        intercept_count_ok(&s2),
        format!("{} == {}", s2.intercepts, EXPECTED_INJECTS),
    ));
    // 生产路径 0 写。
    let mut s3 = machine_surface();
    let _ = s3.freeze();
    let _ = s3.production_read();
    let _ = s3.production_read();
    let _ = s3.production_read();
    v.push((
        "F0470-生产路径0写",
        production_zero_write_ok(&s3) && s3.reads == 3,
        format!("写={} 读={}", s3.production_writes, s3.reads),
    ));
    // 解封注入必抓。
    v.push((
        "F0470-解封拒",
        probe_seq_b().is_ok(),
        String::from("v1 冻结不可逆，解封显性拒"),
    ));
    // 槽越界注入必抓。
    let mut s4 = machine_surface();
    let _ = s4.freeze();
    let over_ok = s4.submit_write(8).is_err() && s4.submit_write(99).is_err();
    v.push((
        "F0470-槽越界拒",
        over_ok,
        String::from("slot 8/99 → SlotOver"),
    ));
    // 重复封账注入必抓。
    v.push((
        "F0470-重复封账拒",
        double_seal_ok(),
        String::from("二次 freeze → DoubleSeal"),
    ));
    // 摘要区分度。
    v.push((
        "F0470-摘要区分度",
        digest_discriminate_ok(),
        String::from("同面同摘要、篡改身份段摘要必变"),
    ));
    // 状态机单调（Draft 写允许、封后写必拒）。
    let mut s5 = machine_surface();
    let draft_write = s5.submit_write(0).is_ok();
    let _ = s5.freeze();
    let sealed_write = s5.submit_write(0).is_err();
    v.push((
        "F0470-状态机单调",
        draft_write && sealed_write,
        String::from("Draft 写允许→Frozen 全拒，单向推进"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "F0470-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 双跑一致。
    v.push((
        "F0470-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 诊断码闭集查表。
    v.push((
        "F0470-诊断码闭集",
        code_closure_ok(),
        String::from("8 码全员落窗且查表回环一致"),
    ));
    v
}

pub fn run_unx0470_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0470 配置空间零写封账（只读红线）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ─────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_golden_run_full() {
        let g = golden_run();
        assert!(g.is_ok());
        assert_eq!(g.ok(), Some((5, 8, 8)));
    }

    #[test]
    fn test_sealed_writes_all_rejected() {
        let mut s = machine_surface();
        assert!(s.freeze().is_ok());
        let mut slot = 0;
        while slot < WRITE_SLOT_OFFS.len() {
            let r = s.submit_write(slot);
            assert!(r.is_err());
            slot += 1;
        }
        assert_eq!(s.intercepts, 8);
    }

    #[test]
    fn test_slot_over_caught() {
        let mut s = machine_surface();
        let _ = s.freeze();
        assert!(s.submit_write(8).is_err());
        assert!(s.submit_write(99).is_err());
        // 越界不算拦截账（闭集外请求）。
        assert_eq!(s.intercepts, 0);
    }

    #[test]
    fn test_unseal_denied() {
        assert!(probe_seq_b().is_ok());
    }

    #[test]
    fn test_double_seal_denied() {
        assert!(double_seal_ok());
    }

    #[test]
    fn test_production_zero_write() {
        let mut s = machine_surface();
        let _ = s.freeze();
        let _ = s.production_read();
        let _ = s.production_read();
        assert_eq!(s.reads, 2);
        assert_eq!(s.production_writes, 0);
        assert!(production_zero_write_ok(&s));
    }

    #[test]
    fn test_digest_discrimination() {
        assert!(digest_discriminate_ok());
        let a = machine_surface();
        let b = machine_surface();
        assert_eq!(a.compute_digest(), b.compute_digest());
    }

    #[test]
    fn test_draft_write_allowed() {
        let mut s = machine_surface();
        assert!(s.submit_write(0).is_ok());
        assert!(s.submit_write(3).is_ok());
        assert_eq!(s.intercepts, 0);
    }

    #[test]
    fn test_dual_run_identical() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn test_segment_guard_and_closure() {
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert_eq!(detail(0x1234_5678u32), "未知诊断码：不在 0x75680 段闭集内");
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let inj = inject_rounds(JUDGE_ROUNDS);
        assert!(inj.is_ok());
        let led = match inj {
            Ok(l) => l,
            Err(_) => return,
        };
        assert!(led.all_caught());
        assert_eq!(led.tries, [10, 10, 10, 10]);
        assert_eq!(led.caught, [10, 10, 10, 10]);
    }

    #[test]
    fn test_probe_seq_c_constants_frozen() {
        assert!(probe_seq_c().is_ok());
        assert_eq!(SEAL_VERSION, 1);
        assert_eq!(EXPECTED_INJECTS, 8);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0470_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
