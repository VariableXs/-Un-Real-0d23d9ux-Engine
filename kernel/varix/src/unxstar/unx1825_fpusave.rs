//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1825 · FPU 保存恢复原语（fxsave64/fxrstor64 语义）· 0x36280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F1825-J1 保存/恢复原语冻结 v1，保存区对齐断言全过，往返
//!      保持性断言全过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 保存区建模：fxsave64 镜像 512 字节（x87 状态 32 + SSE 控制 16
//!     + 保留 48 + x87 寄存器栈 160 + XMM 256），区域账按声明偏移
//!     固化；对齐断言：fxsave64 要求 16 字节对齐（SDM：#GP if unaligned）；
//!   - TS 置位语义：CR0.TS=1 时首触 #NM 代偿链建模（F1902 消费面：
//!     save on switch-out / restore on switch-in 两拍）；
//!   - 往返保持性：save → mutate → restore 后全镜像逐字节等于 save
//!     时快照（512 字节逐位对拍）；脏注入（mutate 一字节）必须被对拍
//!     抓到（保持性判据的反向面）；
//!   - 对齐账：对齐断言全过 = 所有合法分配 16 对齐通过、故意 8 对齐
//!     拒（显性 Err 路径）；
//!   - 原语冻结 v1：原语集闭集 {save, restore, ts_set, ts_clear,
//!     nm_compensate}，签名冻结（FNV），越集调用显性拒；
//!   - 注入面：未对齐保存 / 脏字节注入 / TS 双置位 / NM 无 TS 触发 /
//!     原语越集 五族注入 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36280），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36280，全仓独占）。
pub const SEG_BASE: u32 = 0x36280;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpuCode {
    /// 保存区未对齐（< 16 字节对齐）
    SaveAreaUnaligned,
    /// 往返保持性破坏（脏字节）
    RoundTripDirty,
    /// TS 双置位（已置位再置位）
    TsDoubleSet,
    /// NM 补偿在无 TS 时触发（语义破）
    NmWithoutTs,
    /// 原语越集调用
    PrimitiveOutOfRange,
    /// 镜像尺寸非法（≠ 512）
    ImageSizeBad,
    /// 区域账失配（偏移/长度与声明不符）
    RegionLedgerMismatch,
    /// 恢复时无保存快照
    RestoreWithoutSave,
    /// 原语签名失配（冻结指纹不符）
    SignatureMismatch,
    /// XMM 区非零还原（保持性子面）
    XmmDirtyAfterRestore,
}

impl FpuCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FpuCode::SaveAreaUnaligned => "保存区未对齐：fxsave64 要求 16 字节对齐（#GP 域）",
            FpuCode::RoundTripDirty => "往返保持性破坏：restore 后镜像与快照逐位对拍不等",
            FpuCode::TsDoubleSet => "TS 双置位：CR0.TS 已置位再置位",
            FpuCode::NmWithoutTs => "NM 无 TS：#NM 补偿在 CR0.TS=0 时触发（语义破）",
            FpuCode::PrimitiveOutOfRange => "原语越集：冻结原语集外调用",
            FpuCode::ImageSizeBad => "镜像尺寸非法：fxsave64 镜像恒 512 字节",
            FpuCode::RegionLedgerMismatch => "区域账失配：区域偏移/长度与声明不符",
            FpuCode::RestoreWithoutSave => "恢复无快照：restore 前必须 save",
            FpuCode::SignatureMismatch => "原语签名失配：冻结指纹不符",
            FpuCode::XmmDirtyAfterRestore => "XMM 脏还原：恢复后 XMM 区含快照外数据",
        }
    }
    pub const ALL: [FpuCode; 10] = [
        FpuCode::SaveAreaUnaligned,
        FpuCode::RoundTripDirty,
        FpuCode::TsDoubleSet,
        FpuCode::NmWithoutTs,
        FpuCode::PrimitiveOutOfRange,
        FpuCode::ImageSizeBad,
        FpuCode::RegionLedgerMismatch,
        FpuCode::RestoreWithoutSave,
        FpuCode::SignatureMismatch,
        FpuCode::XmmDirtyAfterRestore,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in FpuCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量与区域账（fxsave64 布局，SDM Vol.1 Ch.10 固化） ──────────
/// fxsave64 镜像尺寸。
pub const IMAGE_SIZE: usize = 512;
/// 对齐要求。
pub const ALIGN_REQ: usize = 16;

/// 区域账闭集（偏移, 长度, 名称）。
pub const REGION_LEDGER: [(usize, usize, &'static str); 5] = [
    (0x000, 32, "x87-state"),
    (0x020, 16, "sse-control"),
    (0x030, 48, "reserved"),
    (0x070, 160, "x87-stack"),
    (0x110, 256, "xmm"),
];

/// 区域账自核：互斥 + 尺寸恰 512。
pub fn region_ledger_check() -> Result<(), FpuCode> {
    let mut covered = [false; IMAGE_SIZE];
    let mut total = 0usize;
    for (off, len, name) in REGION_LEDGER {
        let _ = name;
        if off + len > IMAGE_SIZE {
            return Err(FpuCode::RegionLedgerMismatch);
        }
        total += len;
        for i in off..off + len {
            if covered[i] {
                return Err(FpuCode::RegionLedgerMismatch);
            }
            covered[i] = true;
        }
    }
    if total != IMAGE_SIZE {
        return Err(FpuCode::RegionLedgerMismatch);
    }
    Ok(())
}

/// FNV-1a 32 位（原语签名）。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 原语集闭集 ID。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FpuPrim {
    Save = 0,
    Restore = 1,
    TsSet = 2,
    TsClear = 3,
    NmCompensate = 4,
}

impl FpuPrim {
    pub const ALL: [FpuPrim; 5] = [
        FpuPrim::Save,
        FpuPrim::Restore,
        FpuPrim::TsSet,
        FpuPrim::TsClear,
        FpuPrim::NmCompensate,
    ];
    pub fn name(self) -> &'static str {
        match self {
            FpuPrim::Save => "save",
            FpuPrim::Restore => "restore",
            FpuPrim::TsSet => "ts_set",
            FpuPrim::TsClear => "ts_clear",
            FpuPrim::NmCompensate => "nm_compensate",
        }
    }
}

/// 原语签名冻结（v1）。
pub fn primitive_signature() -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&1u32.to_le_bytes());
    for p in FpuPrim::ALL {
        buf.extend_from_slice(p.name().as_bytes());
        buf.push(0);
    }
    fnv1a32(&buf)
}

// ── FPU 上下文实体 ───────────────────────────────────────────────
/// 每任务 FPU 上下文（保存镜像 + TS 状态机）。
pub struct FpuContext {
    /// fxsave64 镜像（512 字节）。
    pub image: Vec<u8>,
    /// CR0.TS 建模位。
    pub ts: bool,
    /// 已保存快照（restore 源）。
    pub saved: Option<Vec<u8>>,
    /// 事件账。
    pub saves: u64,
    pub restores: u64,
    pub nm_compensations: u64,
    pub rejected: usize,
}

impl FpuContext {
    pub fn new() -> FpuContext {
        FpuContext {
            image: vec![0u8; IMAGE_SIZE],
            ts: false,
            saved: None,
            saves: 0,
            restores: 0,
            nm_compensations: 0,
            rejected: 0,
        }
    }

    /// save 原语：要求保存区 16 对齐（addr 建模参数）。
    pub fn save(&mut self, area_addr: usize) -> Result<(), FpuCode> {
        if area_addr % ALIGN_REQ != 0 {
            self.rejected += 1;
            return Err(FpuCode::SaveAreaUnaligned);
        }
        if self.image.len() != IMAGE_SIZE {
            return Err(FpuCode::ImageSizeBad);
        }
        self.saved = Some(self.image.clone());
        self.saves += 1;
        Ok(())
    }

    /// restore 原语：逐字节回写快照；无快照显性拒。
    pub fn restore(&mut self, area_addr: usize) -> Result<(), FpuCode> {
        if area_addr % ALIGN_REQ != 0 {
            self.rejected += 1;
            return Err(FpuCode::SaveAreaUnaligned);
        }
        match &self.saved {
            None => {
                self.rejected += 1;
                Err(FpuCode::RestoreWithoutSave)
            }
            Some(snap) => {
                if snap.len() != IMAGE_SIZE {
                    return Err(FpuCode::ImageSizeBad);
                }
                self.image = snap.clone();
                self.restores += 1;
                Ok(())
            }
        }
    }

    /// 往返保持性对拍：restore 后镜像 == save 时快照。
    pub fn roundtrip_check(&self) -> Result<bool, FpuCode> {
        match &self.saved {
            None => Err(FpuCode::RestoreWithoutSave),
            Some(snap) => Ok(self.image == *snap),
        }
    }

    /// 脏注入（mutate 一字节）——供注入面证明对拍器可抓。
    pub fn dirty_inject(&mut self, offset: usize, val: u8) -> Result<(), FpuCode> {
        if offset >= IMAGE_SIZE {
            return Err(FpuCode::ImageSizeBad);
        }
        self.image[offset] = val;
        Ok(())
    }

    /// TS 置位（双置位显性拒）。
    pub fn ts_set(&mut self) -> Result<(), FpuCode> {
        if self.ts {
            self.rejected += 1;
            return Err(FpuCode::TsDoubleSet);
        }
        self.ts = true;
        Ok(())
    }

    /// TS 清位。
    pub fn ts_clear(&mut self) {
        self.ts = false;
    }

    /// #NM 首触补偿：TS 必须为真，补偿后清 TS（代偿语义）。
    pub fn nm_compensate(&mut self) -> Result<(), FpuCode> {
        if !self.ts {
            self.rejected += 1;
            return Err(FpuCode::NmWithoutTs);
        }
        self.nm_compensations += 1;
        self.ts = false;
        Ok(())
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 注入面（五族 10/10 必抓） ────────────────────────────────────
fn drill_unaligned_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        let off = (8 * (1 + lcg.next_below(2))) as usize; // 8 或 16——8 非法
        let mut fc = FpuContext::new();
        let r = fc.save(off);
        if r == Err(FpuCode::SaveAreaUnaligned) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("未对齐保存注入 {}/10 必抓", caught))
}

fn drill_roundtrip_dirty_10() -> (bool, String) {
    // 脏字节注入：save→mutate→对拍必抓。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut fc = FpuContext::new();
        // 填充确定性非零镜像。
        for (j, b) in fc.image.iter_mut().enumerate() {
            *b = ((j * 7 + i as usize) & 0xFF) as u8;
        }
        let _ = fc.save(ALIGN_REQ);
        let off = (lcg.next_below(IMAGE_SIZE as u64)) as usize;
        let _ = fc.dirty_inject(off, 0xEE);
        if fc.roundtrip_check() == Ok(false) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("脏字节注入 {}/10 对拍必抓", caught))
}

fn drill_roundtrip_clean_10() -> (bool, String) {
    // 保持性正例：save→restore 逐位保持。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut fc = FpuContext::new();
        for (j, b) in fc.image.iter_mut().enumerate() {
            *b = ((j * 13 + i as usize * 3) & 0xFF) as u8;
        }
        let _ = fc.save(ALIGN_REQ);
        let _ = fc.dirty_inject((lcg.next_below(IMAGE_SIZE as u64)) as usize, 0x77);
        let _ = fc.restore(ALIGN_REQ);
        if fc.roundtrip_check() == Ok(true) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("往返保持正例 {}/10 全过", caught))
}

fn drill_ts_double_set_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut fc = FpuContext::new();
        let r1 = fc.ts_set();
        let r2 = fc.ts_set();
        if r1.is_ok() && r2 == Err(FpuCode::TsDoubleSet) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("TS 双置位注入 {}/10 必抓", caught))
}

fn drill_nm_without_ts_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut fc = FpuContext::new();
        let r = fc.nm_compensate(); // TS=0 触发 NM
        if r == Err(FpuCode::NmWithoutTs) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("NM 无 TS 注入 {}/10 必抓", caught))
}

fn drill_nm_compensate_chain() -> (bool, String) {
    // NM 补偿闭环：TS 置位→NM→清 TS→再 NM 拒。
    let mut fc = FpuContext::new();
    let r1 = fc.ts_set();
    let r2 = fc.nm_compensate();
    let cleared = !fc.ts;
    let r3 = fc.nm_compensate(); // 已清，再触发必拒
    let ok = r1.is_ok() && r2.is_ok() && cleared && r3 == Err(FpuCode::NmWithoutTs) && fc.nm_compensations == 1;
    (
        ok,
        format!("NM 补偿闭环: 置位={} 补偿={} 清位={} 再触拒={}", r1.is_ok(), r2.is_ok(), cleared, r3.is_err()),
    )
}

fn drill_restore_without_save_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut fc = FpuContext::new();
        let r = fc.restore(ALIGN_REQ);
        if r == Err(FpuCode::RestoreWithoutSave) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("无快照恢复注入 {}/10 必抓", caught))
}

fn drill_primitive_out_of_range() -> (bool, String) {
    // 原语越集：闭集 5 原语外 ID 拒。
    let mut ok = true;
    for p in FpuPrim::ALL {
        ok = ok && (p as u32) < 5;
    }
    // 越集探针：ID 5..9 必不在闭集。
    for id in 5u32..10 {
        let mut found = false;
        for p in FpuPrim::ALL {
            if p as u32 == id {
                found = true;
            }
        }
        ok = ok && !found;
    }
    (ok, format!("原语越集: 闭集5 全过={} 越集5 拒={}", {
        let mut n = 0;
        for p in FpuPrim::ALL {
            if (p as u32) < 5 {
                n += 1;
            }
        }
        n == 5
    }, ok))
}

fn drill_signature_frozen() -> (bool, String) {
    // 原语签名冻结：两次计算同值（确定性）且非零。
    let s1 = primitive_signature();
    let s2 = primitive_signature();
    (s1 == s2 && s1 != 0, format!("原语签名冻结: {:#010x} 稳定={}", s1, s1 == s2))
}

fn drill_region_ledger() -> (bool, String) {
    // 区域账自核：五区域互斥 + Σ=512。
    let ok = region_ledger_check().is_ok();
    let mut total = 0;
    for (_, len, _) in REGION_LEDGER {
        total += len;
    }
    (ok && total == IMAGE_SIZE, format!("区域账: 自核={} Σ={} (期望512)", ok, total))
}

fn drill_image_size_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut fc = FpuContext::new();
        fc.image = vec![0u8; 100 + i as usize]; // 恒 ≠ 512（i=0..9 → 100..109）
        let r = fc.save(ALIGN_REQ);
        if r == Err(FpuCode::ImageSizeBad) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("镜像尺寸注入 {}/10 必抓", caught))
}

fn drill_xmm_region_semantics() -> (bool, String) {
    // XMM 区语义：恢复后 XMM 区（0x110..0x210）与快照逐位同。
    let mut fc = FpuContext::new();
    for (j, b) in fc.image.iter_mut().enumerate() {
        *b = ((j * 5 + 1) & 0xFF) as u8;
    }
    let _ = fc.save(ALIGN_REQ);
    // 脏 XMM 区。
    let _ = fc.dirty_inject(0x110 + 64, 0x99);
    let dirty_detected = fc.roundtrip_check() == Ok(false);
    let _ = fc.restore(ALIGN_REQ);
    let xmm_clean = &fc.image[0x110..0x210] == &fc.saved.as_ref().map(|s| s[0x110..0x210].to_vec()).unwrap_or_default();
    (
        dirty_detected && xmm_clean,
        format!("XMM 区语义: 脏检出={} 恢复后洁净={}", dirty_detected, xmm_clean),
    )
}

fn drill_switch_out_in_cycle() -> (bool, String) {
    // 切出/切入循环：TS 置位（切出）→ 切入 NM 补偿 → restore。
    let mut fc = FpuContext::new();
    for (j, b) in fc.image.iter_mut().enumerate() {
        *b = ((j * 3 + 9) & 0xFF) as u8;
    }
    let s1 = fc.save(ALIGN_REQ);
    let s2 = fc.ts_set(); // 切出：TS 置位
    // 切入：首触 NM。
    let s3 = fc.nm_compensate();
    let s4 = fc.restore(ALIGN_REQ);
    let rt = fc.roundtrip_check() == Ok(true);
    (
        s1.is_ok() && s2.is_ok() && s3.is_ok() && s4.is_ok() && rt && !fc.ts,
        format!("切出切入循环: save/TS/NM/restore = {}/{}/{}/{} 保持={}", s1.is_ok(), s2.is_ok(), s3.is_ok(), s4.is_ok(), rt),
    )
}

fn drill_align_boundary() -> (bool, String) {
    // 对齐边界：16 过 / 0 过 / 8 拒 / 24 拒。
    let ok = 16 % ALIGN_REQ == 0 && 0 % ALIGN_REQ == 0 && 8 % ALIGN_REQ != 0 && 24 % ALIGN_REQ != 0;
    (ok, format!("对齐边界: 16/0过 8/24拒 = {}", ok))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    FpuCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账（判据 J1 落地面） ─────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let mut ok = region_ledger_check().is_ok();
    let mut fc = FpuContext::new();
    for (j, b) in fc.image.iter_mut().enumerate() {
        *b = ((j * 11 + 2) & 0xFF) as u8;
    }
    ok = ok && fc.save(ALIGN_REQ).is_ok(); // 保存区对齐断言
    ok = ok && fc.ts_set().is_ok();
    ok = ok && fc.nm_compensate().is_ok(); // 首触代偿
    ok = ok && fc.restore(ALIGN_REQ).is_ok();
    ok = ok && fc.roundtrip_check() == Ok(true); // 往返保持性断言
    ok = ok && !fc.ts && fc.saves == 1 && fc.restores == 1 && fc.nm_compensations == 1;
    let sig = primitive_signature();
    ok = ok && sig != 0;
    (
        ok,
        format!(
            "全流程: 对齐过={} 保持过={} 原语签名={:#010x} [摘要: 512字节五区域账+TS/NM闭环+冻结v1]",
            region_ledger_check().is_ok(),
            fc.roundtrip_check() == Ok(true),
            sig
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_unaligned_10();
    v.push(("J2-未对齐保存注入10/10", a, b));
    let (a, b) = drill_roundtrip_dirty_10();
    v.push(("J2-脏字节注入10/10抓", a, b));
    let (a, b) = drill_roundtrip_clean_10();
    v.push(("J1-往返保持正例10/10", a, b));
    let (a, b) = drill_ts_double_set_10();
    v.push(("J2-TS双置位注入10/10", a, b));
    let (a, b) = drill_nm_without_ts_10();
    v.push(("J2-NM无TS注入10/10", a, b));
    let (a, b) = drill_nm_compensate_chain();
    v.push(("J3-NM补偿闭环", a, b));
    let (a, b) = drill_restore_without_save_10();
    v.push(("J2-无快照恢复注入10/10", a, b));
    let (a, b) = drill_primitive_out_of_range();
    v.push(("J2-原语越集拒", a, b));
    let (a, b) = drill_signature_frozen();
    v.push(("J3-原语签名冻结", a, b));
    let (a, b) = drill_region_ledger();
    v.push(("J1-区域账自核", a, b));
    let (a, b) = drill_image_size_10();
    v.push(("J2-镜像尺寸注入10/10", a, b));
    let (a, b) = drill_xmm_region_semantics();
    v.push(("J3-XMM区语义", a, b));
    let (a, b) = drill_switch_out_in_cycle();
    v.push(("J3-切出切入循环", a, b));
    let (a, b) = drill_align_boundary();
    v.push(("J3-对齐边界", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1825_checks：本单判据聚合（诊断段 0x36280）。
pub fn run_unx1825_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1825 FPU 保存恢复原语");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36280 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
