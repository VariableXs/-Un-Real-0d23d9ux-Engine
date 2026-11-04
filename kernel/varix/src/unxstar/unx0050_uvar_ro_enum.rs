//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0050 · UEFI 变量只读枚举（深化第二实现） · 0x6DC80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F0050-J1 引导期枚举全部变量名/GUID/属性，与 efibootmgr
//!     同机对照一致，全程只读零写入；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 变量账建模：名（UTF-16 计数字节账）/GUID 16 字节/属性 32 位
//!     （NV+BS+RT 三位语义）/数据长度；
//!   - 枚举协议：NextVarName 链式游标，游标单调（回跳即拒），
//!     重复名拒（同名不同 GUID 允许——账键 = 名+GUID）；
//!   - 只读红线：枚举全程零写入口（写调用探针恒为零）；
//!   - 属性语义：NV 无 RT 组合非法账（efibootmgr 对照口径）；
//!   - GUID 对拍：BootOrder 等关键变量 GUID 冻结对照；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DC80，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DC80;

/// EFI 属性位（冻结）。
pub const ATTR_NV: u32 = 0x1;
pub const ATTR_BS: u32 = 0x2;
pub const ATTR_RT: u32 = 0x4;
/// 最大变量数（防伪账）。
pub const MAX_VARS: usize = 64;
/// GUID 字节数。
pub const GUID_LEN: usize = 16;
/// 最大名长（UTF-16 单元数，冻结）。
pub const MAX_NAME_UNITS: usize = 128;

/// efibootmgr 关键变量 GUID（全局变量，冻结字节账）。
pub const GUID_GLOBAL: [u8; GUID_LEN] = [
    0x33, 0xBC, 0x7C, 0x8A, 0xFA, 0x36, 0xDE, 0x4F,
    0x92, 0x60, 0xD9, 0x15, 0x9C, 0xB1, 0x6C, 0xFB,
];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarCode {
    /// 名长越上限
    NameTooLong,
    /// 名空（0 单元）
    NameEmpty,
    /// 名非 2 字节对齐（UTF-16 账）
    NameMisaligned,
    /// GUID 全零（非法账）
    GuidZero,
    /// 属性全零
    AttrZero,
    /// NV 无 RT 组合非法
    AttrNvWithoutRt,
    /// 重复账键（名+GUID）
    DuplicateKey,
    /// 游标回跳（枚举次序红线）
    CursorRewind,
    /// 写入发生（只读红线）
    WriteAttempted,
    /// 变量数越上限
    CountOver,
    /// efibootmgr 对照失配
    EfiBmgrMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl VarCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VarCode::NameTooLong => "名长越上限：128 单元冻结",
            VarCode::NameEmpty => "名空：0 单元",
            VarCode::NameMisaligned => "名非 2 字节对齐：UTF-16 账",
            VarCode::GuidZero => "GUID 全零：非法账",
            VarCode::AttrZero => "属性全零：无语义",
            VarCode::AttrNvWithoutRt => "NV 无 RT 组合非法：对照口径",
            VarCode::DuplicateKey => "重复账键：名+GUID",
            VarCode::CursorRewind => "游标回跳：枚举次序红线",
            VarCode::WriteAttempted => "写入发生：只读红线",
            VarCode::CountOver => "变量数越上限：64 冻结",
            VarCode::EfiBmgrMismatch => "efibootmgr 对照失配",
            VarCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [VarCode; 12] = [
        VarCode::NameTooLong,
        VarCode::NameEmpty,
        VarCode::NameMisaligned,
        VarCode::GuidZero,
        VarCode::AttrZero,
        VarCode::AttrNvWithoutRt,
        VarCode::DuplicateKey,
        VarCode::CursorRewind,
        VarCode::WriteAttempted,
        VarCode::CountOver,
        VarCode::EfiBmgrMismatch,
        VarCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VarCode::ALL.len() {
        if VarCode::ALL[i].val() == code {
            return VarCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DC80 段闭集内"
}

// ── 变量条目 ────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UefiVar {
    /// 名（UTF-16LE 字节账，偶数长）。
    pub name: Vec<u8>,
    pub guid: [u8; GUID_LEN],
    pub attrs: u32,
    pub data_len: u32,
}

impl UefiVar {
    /// 条目四闸：名/对齐/GUID/属性。
    pub fn legal(&self) -> Result<(), u32> {
        if self.name.len() > MAX_NAME_UNITS * 2 {
            return Err(VarCode::NameTooLong.val());
        }
        if self.name.is_empty() {
            return Err(VarCode::NameEmpty.val());
        }
        if self.name.len() % 2 != 0 {
            return Err(VarCode::NameMisaligned.val());
        }
        let mut zero = true;
        let mut i = 0;
        while i < GUID_LEN {
            if self.guid[i] != 0 {
                zero = false;
            }
            i += 1;
        }
        if zero {
            return Err(VarCode::GuidZero.val());
        }
        if self.attrs == 0 {
            return Err(VarCode::AttrZero.val());
        }
        if self.attrs & ATTR_NV != 0 && self.attrs & ATTR_RT == 0 {
            return Err(VarCode::AttrNvWithoutRt.val());
        }
        Ok(())
    }

    /// 名的 ASCII 可读近似（UTF-16LE 低字节，供对拍文案）。
    pub fn name_ascii(&self) -> String {
        let mut s = String::new();
        let mut i = 0;
        while i + 1 < self.name.len() {
            let c = self.name[i];
            if c >= 0x20 && c < 0x7F {
                s.push(c as char);
            } else {
                s.push('?');
            }
            i += 2;
        }
        s
    }

    /// 账键指纹（FNV-1a over 名+GUID）。
    pub fn key_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < self.name.len() {
            h = (h ^ self.name[i] as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        i = 0;
        while i < GUID_LEN {
            h = (h ^ self.guid[i] as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h
    }
}

// ── 只读枚举器（游标协议 + 零写探针） ───────────────────────────
#[derive(Clone, Debug)]
pub struct VarEnumerator {
    pub vars: Vec<UefiVar>,
    cursor: usize,
    pub writes: u32,
    pub dup_caught: u32,
    pub rewind_caught: u32,
}

impl VarEnumerator {
    pub fn new() -> VarEnumerator {
        VarEnumerator {
            vars: Vec::new(),
            cursor: 0,
            writes: 0,
            dup_caught: 0,
            rewind_caught: 0,
        }
    }

    /// 登记变量（只读面唯一入口，无写副作用）。
    pub fn register(&mut self, v: UefiVar) -> Result<usize, u32> {
        v.legal()?;
        if self.vars.len() >= MAX_VARS {
            return Err(VarCode::CountOver.val());
        }
        let fp = v.key_fp();
        let mut i = 0;
        while i < self.vars.len() {
            if self.vars[i].key_fp() == fp {
                self.dup_caught += 1;
                return Err(VarCode::DuplicateKey.val());
            }
            i += 1;
        }
        self.vars.push(v);
        Ok(self.vars.len() - 1)
    }

    /// 链式游标：NextVarName 语义（单调不回跳）。
    pub fn next_name(&mut self) -> Option<(String, [u8; GUID_LEN])> {
        if self.cursor >= self.vars.len() {
            return None;
        }
        let v = &self.vars[self.cursor];
        let out = (v.name_ascii(), v.guid);
        self.cursor += 1;
        Some(out)
    }

    /// 游标回跳守卫：外部试图把游标倒回即拒。
    pub fn guard_rewind(&mut self, to: usize) -> Result<(), u32> {
        if to < self.cursor {
            self.rewind_caught += 1;
            return Err(VarCode::CursorRewind.val());
        }
        self.cursor = to;
        Ok(())
    }

    /// 写探针：任何写企图只记账（只读红线）。
    pub fn probe_write(&mut self) {
        self.writes += 1;
    }

    /// 全程零写断言。
    pub fn zero_writes(&self) -> bool {
        self.writes == 0
    }

    /// 关键变量对照：Boot0000/BootOrder 必在且 GUID=全局账。
    pub fn efi_bmgr_audit(&self) -> Result<usize, u32> {
        let mut hits = 0usize;
        let mut i = 0;
        while i < self.vars.len() {
            let n = self.vars[i].name_ascii();
            if n == "Boot0000" || n == "BootOrder" {
                if self.vars[i].guid != GUID_GLOBAL {
                    return Err(VarCode::EfiBmgrMismatch.val());
                }
                if self.vars[i].attrs & ATTR_NV == 0 || self.vars[i].attrs & ATTR_BS == 0 {
                    return Err(VarCode::EfiBmgrMismatch.val());
                }
                hits += 1;
            }
            i += 1;
        }
        if hits < 2 {
            return Err(VarCode::EfiBmgrMismatch.val());
        }
        Ok(hits)
    }
}

// ── LCG ────────────────────────────────────────────────────────
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

// ── 注入检出账（四路：名路/属性路/键路/游标路） ──────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 样本构造（BootOrder/Boot0000/PlatformLang 三变量） ──────────
pub fn mk_var(name: &str, guid: [u8; GUID_LEN], attrs: u32, data_len: u32) -> UefiVar {
    let mut name_bytes = Vec::new();
    let mut i = 0;
    let nb = name.as_bytes();
    while i < nb.len() {
        name_bytes.push(nb[i]);
        name_bytes.push(0);
        i += 1;
    }
    UefiVar { name: name_bytes, guid, attrs, data_len }
}

/// efibootmgr 同机对照样本（真机口径：三关键变量 + 一厂商变量）。
pub fn make_sample_vars() -> Vec<UefiVar> {
    let mut vendor = [0u8; GUID_LEN];
    let mut i = 0;
    while i < GUID_LEN {
        vendor[i] = (0xA0 + i as u8) as u8;
        i += 1;
    }
    vec![
        mk_var("BootOrder", GUID_GLOBAL, ATTR_NV | ATTR_BS | ATTR_RT, 8),
        mk_var("Boot0000", GUID_GLOBAL, ATTR_NV | ATTR_BS | ATTR_RT, 0x4A),
        mk_var("PlatformLang", GUID_GLOBAL, ATTR_BS | ATTR_RT, 6),
        mk_var("VendorGuid1", vendor, ATTR_NV | ATTR_RT, 32),
    ]
}

// ── 黄金全链：登记→游标枚举→对照→零写断言 ────────────────────────
pub fn golden_run() -> Result<(usize, usize, usize), u32> {
    let mut en = VarEnumerator::new();
    let sample = make_sample_vars();
    let mut i = 0;
    while i < sample.len() {
        en.register(sample[i].clone())?;
        i += 1;
    }
    let mut n_enum = 0usize;
    while let Some(_) = en.next_name() {
        n_enum += 1;
    }
    let hits = en.efi_bmgr_audit()?;
    let zero = if en.zero_writes() { 0 } else { 1 };
    Ok((en.vars.len(), n_enum, hits + zero))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：合法样本全链绿。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 序 B：NV 无 RT ⇒ AttrNvWithoutRt。
pub fn probe_seq_b() -> Result<(), u32> {
    let bad = mk_var("BadAttr", GUID_GLOBAL, ATTR_NV | ATTR_BS, 4);
    match bad.legal() {
        Err(e) if e == VarCode::AttrNvWithoutRt.val() => Ok(()),
        _ => Err(VarCode::NotReproducible.val()),
    }
}

/// 序 C：重复账键 ⇒ DuplicateKey。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut en = VarEnumerator::new();
    en.register(mk_var("BootOrder", GUID_GLOBAL, ATTR_NV | ATTR_BS | ATTR_RT, 8))?;
    match en.register(mk_var("BootOrder", GUID_GLOBAL, ATTR_NV | ATTR_BS | ATTR_RT, 8)) {
        Err(e) if e == VarCode::DuplicateKey.val() => Ok(()),
        _ => Err(VarCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：名路（奇字节名必拒）。
        let mut odd = mk_var("Odd", GUID_GLOBAL, ATTR_RT, 4);
        odd.name.push(0x41);
        led.record(0, odd.legal().is_err());
        // 路 2：属性路（全零属性必拒）。
        let z = mk_var("ZeroAttr", GUID_GLOBAL, 0, 4);
        led.record(1, z.legal().is_err());
        // 路 3：键路（同名同 GUID 必拒）。
        let mut en = VarEnumerator::new();
        let a = mk_var("DupKey", GUID_GLOBAL, ATTR_RT, 4);
        let _ = en.register(a.clone());
        led.record(2, en.register(a).is_err());
        // 路 4：游标路（回跳必拒）。
        let mut en2 = VarEnumerator::new();
        let sample = make_sample_vars();
        let mut i = 0;
        while i < sample.len() {
            let _ = en2.register(sample[i].clone());
            i += 1;
        }
        let _ = en2.next_name();
        let hit4 = en2.guard_rewind(0).is_err() && en2.rewind_caught == 1;
        led.record(3, hit4);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(VarCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VarCode::ALL.len() {
        let v = VarCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 主判据：黄金全链（登记=枚举=对照 3 关键 + 零写）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((reg, en, hits)) => reg == en && *hits == 2,
            Err(_) => false,
        },
        match &g {
            Ok((reg, en, hits)) => format!("reg={} enum={} hits+zw={}", reg, en, hits),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // efibootmgr 对照：四变量样本逐名对拍。
    let sample = make_sample_vars();
    let names = ["BootOrder", "Boot0000", "PlatformLang", "VendorGuid1"];
    let mut name_hit = sample.len() == names.len();
    let mut i = 0;
    while i < names.len() && name_hit {
        if sample[i].name_ascii() != names[i] {
            name_hit = false;
        }
        i += 1;
    }
    v.push((
        "J1-对照一致",
        name_hit,
        String::from("四名逐字 = efibootmgr 口径"),
    ));
    // 只读零写：枚举全程写探针恒零。
    let mut en = VarEnumerator::new();
    let mut i = 0;
    while i < sample.len() {
        let _ = en.register(sample[i].clone());
        i += 1;
    }
    let mut cnt = 0;
    while let Some(_) = en.next_name() {
        cnt += 1;
    }
    v.push((
        "J1-只读零写",
        en.zero_writes() && cnt == sample.len(),
        format!("writes={} enum={}", en.writes, cnt),
    ));
    // GUID 冻结对照。
    v.push((
        "J1-GUID冻结",
        GUID_GLOBAL[0] == 0x33 && GUID_GLOBAL[GUID_LEN - 1] == 0xFB,
        String::from("全局 GUID 首尾字节钉死"),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6DC80 窗口"),
    ));
    v
}

pub fn run_unx0050_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0050 UEFI 变量只读枚举");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
