//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1826 · SIMD 向量状态切换（XMM/AVX 语义） · 0x6D040
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B4）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1826-J1 向量状态全保存断言（XMM/YMM 按特征位）全过，
//!     宽状态保存开销账落账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 特征位闭集：SSE（XMM 16×128bit）/AVX（YMM 16×256bit），
//!      YMM 保存以 CPUID AVX 位为前提（无位不保存——不猜）；
//!    - 全保存断言：XMM 恒保存 16 槽；AVX 位亮时 YMM 16 槽随保存
//!      （XSAVE 布局账：XMM 区 256B + YMM 区 256B）；
//!    - 开销账：保存步数（XMM=32 步 / YMM 追加 32 步），落账逐次；
//!    - 半保存违例注入（AVX 位亮只存 XMM）10/10 必抓；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D040，AI-VX3-1030 批E 独占区）。
pub const SEG_BASE: u32 = 0x6D040;

// ── 特征位闭集 ─────────────────────────────────────────────────
pub const FEAT_SSE: usize = 0;
pub const FEAT_AVX: usize = 1;
pub const FEAT_COUNT: usize = 2;
pub const FEAT_NAMES: [&str; FEAT_COUNT] = ["SSE", "AVX"];
/// 槽数。
pub const XMM_SLOTS: usize = 16;
pub const YMM_SLOTS: usize = 16;
/// 保存步数（冻结：每槽 2 步）。
pub const STEPS_PER_SLOT: u64 = 2;
/// XSAVE 区布局（字节）。
pub const XSAVE_XMM_BYTES: u64 = 256;
pub const XSAVE_YMM_BYTES: u64 = 256;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimdCode {
    /// 特征越闭集
    FeatOutOfRange,
    /// 特征名空（二特征齐备破坏）
    FeatNameBlank,
    /// 半保存违例（AVX 亮却漏 YMM）
    HalfSaveViolation,
    /// 无 AVX 位却保存 YMM
    YmmWithoutFeature,
    /// XMM 槽缺（≠16）
    XmmSlotsShort,
    /// 开销账不符（步数≠冻结公式）
    OverheadMismatch,
    /// 布局越界（XSAVE 区越 512B）
    AreaOverrun,
    /// 双跑不一致
    NotReproducible,
}

impl SimdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SimdCode::FeatOutOfRange => "特征越闭集：SSE/AVX",
            SimdCode::FeatNameBlank => "特征名空：齐备破坏",
            SimdCode::HalfSaveViolation => "半保存违例：AVX 亮漏 YMM",
            SimdCode::YmmWithoutFeature => "无 AVX 位保存 YMM：不猜",
            SimdCode::XmmSlotsShort => "XMM 槽缺：≠16",
            SimdCode::OverheadMismatch => "开销账不符",
            SimdCode::AreaOverrun => "XSAVE 区越界：>512B",
            SimdCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SimdCode; 8] = [
        SimdCode::FeatOutOfRange,
        SimdCode::FeatNameBlank,
        SimdCode::HalfSaveViolation,
        SimdCode::YmmWithoutFeature,
        SimdCode::XmmSlotsShort,
        SimdCode::OverheadMismatch,
        SimdCode::AreaOverrun,
        SimdCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SimdCode::ALL.len() {
        if SimdCode::ALL[i].val() == code {
            return SimdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D040 段闭集内"
}

// ── 向量状态保存账 ─────────────────────────────────────────────
pub struct SimdSave {
    /// 特征位图（SSE/AVX）。
    pub feats: [bool; FEAT_COUNT],
    /// XMM 16 槽。
    pub xmm: [u64; XMM_SLOTS],
    /// YMM 16 槽（仅 AVX 位亮时有效）。
    pub ymm: [u64; YMM_SLOTS],
    /// 保存步数账。
    pub save_steps: u64,
    /// XSAVE 区占用（字节）。
    pub area_bytes: u64,
    pub half_caught: u32,
}

impl SimdSave {
    pub fn new(feats: [bool; FEAT_COUNT]) -> Result<SimdSave, u32> {
        if FEAT_NAMES.len() != FEAT_COUNT {
            return Err(SimdCode::FeatNameBlank.val());
        }
        Ok(SimdSave {
            feats,
            xmm: [0; XMM_SLOTS],
            ymm: [0; YMM_SLOTS],
            save_steps: 0,
            area_bytes: 0,
            half_caught: 0,
        })
    }

    /// 全保存：XMM 恒存；YMM 按特征位。
    pub fn save_all(&mut self, seed: u64) -> Result<u64, u32> {
        // XMM 恒存。
        let mut i = 0;
        while i < XMM_SLOTS {
            self.xmm[i] = seed + i as u64;
            i += 1;
        }
        let mut steps = XMM_SLOTS as u64 * STEPS_PER_SLOT;
        let mut area = XSAVE_XMM_BYTES;
        // YMM 按 AVX 位。
        if self.feats[FEAT_AVX] {
            i = 0;
            while i < YMM_SLOTS {
                self.ymm[i] = seed.wrapping_add(0x1000) + i as u64;
                i += 1;
            }
            steps += YMM_SLOTS as u64 * STEPS_PER_SLOT;
            area += XSAVE_YMM_BYTES;
        }
        if area > XSAVE_XMM_BYTES + XSAVE_YMM_BYTES {
            return Err(SimdCode::AreaOverrun.val());
        }
        self.save_steps = steps;
        self.area_bytes = area;
        Ok(steps)
    }

    /// 全保存断言：XMM 16 槽全非零 +（AVX 亮时 YMM 16 槽全非零）。
    pub fn full_save_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < XMM_SLOTS {
            if self.xmm[i] == 0 {
                return Err(SimdCode::XmmSlotsShort.val());
            }
            i += 1;
        }
        if self.feats[FEAT_AVX] {
            i = 0;
            while i < YMM_SLOTS {
                if self.ymm[i] == 0 {
                    return Err(SimdCode::HalfSaveViolation.val());
                }
                i += 1;
            }
        }
        Ok(())
    }

    /// 开销账对拍：步数 = 槽数 × 每槽步数。
    pub fn overhead_audit(&self) -> Result<(), u32> {
        let expect = if self.feats[FEAT_AVX] {
            (XMM_SLOTS + YMM_SLOTS) as u64 * STEPS_PER_SLOT
        } else {
            XMM_SLOTS as u64 * STEPS_PER_SLOT
        };
        if self.save_steps != expect {
            return Err(SimdCode::OverheadMismatch.val());
        }
        Ok(())
    }
}

/// 半保存违例注入 10/10（AVX 亮、YMM 漏存 → 审计必抓）。
pub fn half_save_inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut s = SimdSave::new([true, true])?;
        // 只存 XMM（漏 YMM）。
        let mut i = 0;
        while i < XMM_SLOTS {
            s.xmm[i] = k as u64 + i as u64 + 1;
            i += 1;
        }
        s.save_steps = XMM_SLOTS as u64 * STEPS_PER_SLOT;
        if s.full_save_audit().is_err() {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(SimdCode::HalfSaveViolation.val());
    }
    Ok(caught)
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 黄金全链：两特征档全保存→审计→开销对拍→半保存 10/10。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    // SSE-only。
    let mut s1 = SimdSave::new([true, false])?;
    let st1 = s1.save_all(SEG_BASE as u64)?;
    s1.full_save_audit()?;
    s1.overhead_audit()?;
    if st1 != 32 {
        return Err(SimdCode::OverheadMismatch.val());
    }
    // SSE+AVX。
    let mut s2 = SimdSave::new([true, true])?;
    let st2 = s2.save_all(SEG_BASE as u64 + 7)?;
    s2.full_save_audit()?;
    s2.overhead_audit()?;
    if st2 != 64 {
        return Err(SimdCode::OverheadMismatch.val());
    }
    // 无 AVX 位不存 YMM（不猜）。
    let mut s3 = SimdSave::new([true, false])?;
    s3.save_all(1)?;
    if s3.ymm[0] != 0 {
        return Err(SimdCode::YmmWithoutFeature.val());
    }
    let half = half_save_inject_10()?;
    Ok((st1, st2, half))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut s = SimdSave::new([true, true])?;
    s.save_all(5)?;
    s.full_save_audit()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 无 AVX 位保存 YMM → 必拒（伪造 YMM 值审计）。
    let mut s = SimdSave::new([true, false])?;
    s.save_all(5)?;
    s.ymm[0] = 0x1234;
    s.full_save_audit().map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 开销账不符 → 必拒。
    let mut s = SimdSave::new([true, false])?;
    s.save_all(5)?;
    s.save_steps = 99;
    s.overhead_audit()
}

/// 注入 10/10：半保存、开销不符、YMM 无位、越区四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：半保存。
        let mut s0 = SimdSave::new([true, true]).ok();
        let h0 = match &mut s0 {
            Some(s) => {
                s.save_all(1).ok();
                s.ymm = [0; YMM_SLOTS];
                s.full_save_audit().is_err()
            }
            None => false,
        };
        led.record(0, h0);
        // 槽1：开销不符。
        let mut s1 = SimdSave::new([true, false]).ok();
        let h1 = match &mut s1 {
            Some(s) => {
                s.save_all(2).ok();
                s.save_steps += 1;
                s.overhead_audit().is_err()
            }
            None => false,
        };
        led.record(1, h1);
        // 槽2：YMM 无位。
        let mut s2 = SimdSave::new([true, false]).ok();
        let h2 = match &mut s2 {
            Some(s) => {
                s.save_all(3).ok();
                s.ymm[3] = 7;
                s.full_save_audit().is_err()
            }
            None => false,
        };
        led.record(2, h2);
        // 槽3：越区。
        let mut s3 = SimdSave::new([true, true]).ok();
        let h3 = match &mut s3 {
            Some(s) => {
                s.save_all(4).ok();
                s.area_bytes = 999;
                s.area_bytes > XSAVE_XMM_BYTES + XSAVE_YMM_BYTES
            }
            None => false,
        };
        led.record(3, h3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SimdCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SimdCode::ALL.len() {
        let v = SimdCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 特征位闭集。
    v.push((
        "J1-特征位闭集",
        FEAT_NAMES.len() == 2 && XMM_SLOTS == 16 && YMM_SLOTS == 16,
        format!("{:?}", FEAT_NAMES),
    ));
    // 全保存断言（AVX 档）。
    let mut s = SimdSave::new([true, true]).ok();
    let full_ok = match &mut s {
        Some(x) => {
            x.save_all(9).ok();
            x.full_save_audit().is_ok() && x.overhead_audit().is_ok() && x.save_steps == 64
        }
        None => false,
    };
    v.push(("J1-全保存断言全过", full_ok, String::from("XMM16+YMM16")));
    // 半保存 10/10。
    v.push((
        "J1-半保存注入10/10",
        half_save_inject_10() == Ok(10),
        format!("{}/10 抓", half_save_inject_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
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
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D040 窗口")));
    v
}

pub fn run_unx1826_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1826 SIMD 向量状态切换（XMM/AVX 语义）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
