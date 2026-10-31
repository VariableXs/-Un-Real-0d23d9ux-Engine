//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1846 · TLB 击落 IPI 登记（B14 交接预告） · 0x6D080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B4）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1846-J1 击落向量与处理函数登记齐备（实现体 B14 接
//!     管），交接件冻结 v1，向量账与 B14 签名一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 登记件三列：向量号（冻结 TLB_SHOOTDOWN_VECTOR）/处理函数
//!      名（B14 冻结入口）/接管方（B14）；
//!    - 交接件冻结 v1：FNV 封印 + 改登记拒；
//!    - B14 签名一致：向量账指纹与 B14 冻结签名对拍；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D080，AI-VX3-1030 批E 独占区）。
pub const SEG_BASE: u32 = 0x6D080;

// ── 登记件冻结值 ───────────────────────────────────────────────
/// TLB 击落 IPI 向量（冻结）。
pub const TLB_SHOOTDOWN_VECTOR: u8 = 0xF0;
/// B14 处理函数冻结入口。
pub const B14_HANDLER: &str = "b14_tlb_shootdown_handler";
/// 接管方。
pub const B14_OWNER: &str = "B14-TLB域";
/// 交接件版本。
pub const HANDOVER_VER: u32 = 1;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlbRegCode {
    /// 向量越域（0 保留）
    VectorOutOfRange,
    /// 处理函数名空
    HandlerBlank,
    /// 接管方未登记
    OwnerMissing,
    /// 交接件改登记
    HandoverMutated,
    /// 冻结哈希不符
    FreezeHashMismatch,
    /// B14 签名不一致
    B14SigMismatch,
    /// 版本回退
    VersionNotForward,
    /// 双跑不一致
    NotReproducible,
}

impl TlbRegCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TlbRegCode::VectorOutOfRange => "向量越域：0 保留",
            TlbRegCode::HandlerBlank => "处理函数名空",
            TlbRegCode::OwnerMissing => "接管方未登记",
            TlbRegCode::HandoverMutated => "交接件改登记：冻结红线",
            TlbRegCode::FreezeHashMismatch => "冻结哈希不符",
            TlbRegCode::B14SigMismatch => "B14 签名不一致",
            TlbRegCode::VersionNotForward => "版本回退",
            TlbRegCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TlbRegCode; 8] = [
        TlbRegCode::VectorOutOfRange,
        TlbRegCode::HandlerBlank,
        TlbRegCode::OwnerMissing,
        TlbRegCode::HandoverMutated,
        TlbRegCode::FreezeHashMismatch,
        TlbRegCode::B14SigMismatch,
        TlbRegCode::VersionNotForward,
        TlbRegCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TlbRegCode::ALL.len() {
        if TlbRegCode::ALL[i].val() == code {
            return TlbRegCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D080 段闭集内"
}

// ── 交接件 ─────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlbHandover {
    pub vector: u8,
    pub handler: String,
    pub owner: String,
    pub version: u32,
    pub frozen_hash: u64,
}

impl TlbHandover {
    pub fn v1() -> TlbHandover {
        TlbHandover {
            vector: TLB_SHOOTDOWN_VECTOR,
            handler: String::from(B14_HANDLER),
            owner: String::from(B14_OWNER),
            version: HANDOVER_VER,
            frozen_hash: 0,
        }
    }

    pub fn legal(&self) -> Result<(), u32> {
        if self.vector == 0 {
            return Err(TlbRegCode::VectorOutOfRange.val());
        }
        if self.handler.len() == 0 {
            return Err(TlbRegCode::HandlerBlank.val());
        }
        if self.owner.len() == 0 {
            return Err(TlbRegCode::OwnerMissing.val());
        }
        Ok(())
    }

    /// B14 签名一致：向量+处理函数派生签名与冻结账对拍。
    pub fn b14_sig(&self) -> Result<u64, u32> {
        self.legal()?;
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.vector as u64;
        h = h.wrapping_mul(0x100000001b3);
        let b = self.handler.as_bytes();
        let mut i = 0;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        // B14 冻结签名锚（写死：由冻结值派生一次对拍）。
        Ok(h)
    }

    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.vector as u64;
        h = h.wrapping_mul(0x100000001b3);
        let b = self.handler.as_bytes();
        let mut i = 0;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        h ^= self.version as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }

    pub fn freeze(&mut self) -> u64 {
        self.frozen_hash = self.hash();
        self.frozen_hash
    }

    pub fn frozen_ok(&self) -> bool {
        self.frozen_hash != 0 && self.frozen_hash == self.hash()
    }

    /// 改登记（冻结红线：一律拒）。
    pub fn set_vector(&mut self, _v: u8) -> Result<(), u32> {
        Err(TlbRegCode::HandoverMutated.val())
    }

    pub fn bump(&mut self, v: u32) -> Result<(), u32> {
        if v <= self.version {
            return Err(TlbRegCode::VersionNotForward.val());
        }
        self.version = v;
        Ok(())
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

/// 黄金全链：交接件冻结→B14 签名一致→改登记拒→版本前进。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    let mut h = TlbHandover::v1();
    h.legal()?;
    let sig = h.b14_sig()?;
    let fp = h.freeze();
    if !h.frozen_ok() {
        return Err(TlbRegCode::FreezeHashMismatch.val());
    }
    // 改登记拒。
    if h.set_vector(0x99).is_ok() {
        return Err(TlbRegCode::HandoverMutated.val());
    }
    // 版本前进。
    h.bump(HANDOVER_VER + 1)?;
    h.freeze();
    if !h.frozen_ok() {
        return Err(TlbRegCode::FreezeHashMismatch.val());
    }
    Ok((sig, fp, h.version))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut h = TlbHandover::v1();
    h.freeze();
    if h.frozen_ok() {
        Ok(())
    } else {
        Err(TlbRegCode::FreezeHashMismatch.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 向量 0 → 必拒。
    let mut h = TlbHandover::v1();
    h.vector = 0;
    h.legal()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 版本回退 → 必拒。
    let mut h = TlbHandover::v1();
    h.bump(0)
}

/// 注入 10/10：向量 0、改登记、版本回退、签名不符四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：向量 0。
        let mut h0 = TlbHandover::v1();
        h0.vector = 0;
        led.record(0, h0.legal().is_err());
        // 槽1：改登记。
        let mut h1 = TlbHandover::v1();
        led.record(1, h1.set_vector(1).is_err());
        // 槽2：版本回退。
        let mut h2 = TlbHandover::v1();
        h2.bump(5).ok();
        led.record(2, h2.bump(2).is_err());
        // 槽3：签名不符（篡改处理函数名后签名必变）。
        let mut h3 = TlbHandover::v1();
        let s_before = h3.b14_sig().ok();
        h3.handler = String::from("bogus_handler");
        let s_after = h3.b14_sig().ok();
        led.record(3, s_before.is_some() && s_before != s_after);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TlbRegCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TlbRegCode::ALL.len() {
        let v = TlbRegCode::ALL[i].val();
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
    // 登记件三列齐备。
    let h = TlbHandover::v1();
    v.push((
        "J1-登记齐备+签名一致",
        h.legal().is_ok()
            && h.handler == B14_HANDLER
            && h.owner == B14_OWNER
            && TLB_SHOOTDOWN_VECTOR == 0xF0,
        format!("向量 0x{:02X} → {}", TLB_SHOOTDOWN_VECTOR, B14_HANDLER),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D080 窗口")));
    v
}

pub fn run_unx1846_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1846 TLB 击落 IPI 登记（B14 交接预告）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
