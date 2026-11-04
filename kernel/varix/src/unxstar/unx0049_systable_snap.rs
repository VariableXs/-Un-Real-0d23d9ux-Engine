//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0049 · UEFI SystemTable 只读快照 · 0x6DC40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F0049-J1 引导期对 SystemTable 各指针快照落 BootManifest，
//!     退出后不再访问（越界访问断言为零）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - SystemTable 建模：签名 "IBI SYST"（0x54535953_20494249 低
//!     64 位账面取 u32 'IBIS' 口径）/修订/表头长度/CRC；
//!   - 五指针快照：Runtime/BootServices/RuntimeServices/ConOut/
//!     StdErr，登记后只读（BootManifest 固化账）；
//!   - 退出后访问断言：BootServices 指针在 EBS 后进封禁账，任何
//!     取用即记违例（越界访问断言为零）；
//!   - 快照等价：快照账与源账逐字段对拍，漂移即拒；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DC40，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DC40;

/// EFI SystemTable 签名（'IBIS' 小端口径，64 位签名低半）。
pub const ST_SIG: u32 = 0x53534249;
/// SystemTable 头长度（冻结）。
pub const ST_HEADER_LEN: usize = 24;
/// 五指针快照槽数。
pub const SNAP_SLOTS: usize = 5;

/// 快照槽位语义（冻结次序）。
pub const SLOT_RUNTIME: usize = 0;
pub const SLOT_BOOT: usize = 1;
pub const SLOT_RUNTIME_SVC: usize = 2;
pub const SLOT_CONOUT: usize = 3;
pub const SLOT_STDERR: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapCode {
    /// 签名错（非 IBI SYST）
    SigBad,
    /// 表头长度不足
    HeaderShort,
    /// CRC 低字节不符（HeaderCRC 账）
    CrcBad,
    /// 修订版本低于 2.0
    RevisionLow,
    /// 指针槽空（必填指针为 0）
    NullPointer,
    /// 固化后改写（BootManifest 红线）
    ManifestMutated,
    /// 退出后访问 BootServices（封禁红线）
    BootSvcAfterExit,
    /// 快照与源漂移
    SnapshotDrift,
    /// 越界槽位读取
    SlotOutOfRange,
    /// 双跑不一致
    NotReproducible,
}

impl SnapCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SnapCode::SigBad => "签名错：非 IBI SYST",
            SnapCode::HeaderShort => "表头长度不足：24 字节冻结",
            SnapCode::CrcBad => "CRC 低字节不符：头账",
            SnapCode::RevisionLow => "修订低于 2.0：拒老表",
            SnapCode::NullPointer => "指针槽空：必填指针为 0",
            SnapCode::ManifestMutated => "固化后改写：Manifest 红线",
            SnapCode::BootSvcAfterExit => "退出后访问 BootServices",
            SnapCode::SnapshotDrift => "快照与源漂移：对拍失配",
            SnapCode::SlotOutOfRange => "越界槽位读取",
            SnapCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SnapCode; 10] = [
        SnapCode::SigBad,
        SnapCode::HeaderShort,
        SnapCode::CrcBad,
        SnapCode::RevisionLow,
        SnapCode::NullPointer,
        SnapCode::ManifestMutated,
        SnapCode::BootSvcAfterExit,
        SnapCode::SnapshotDrift,
        SnapCode::SlotOutOfRange,
        SnapCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SnapCode::ALL.len() {
        if SnapCode::ALL[i].val() == code {
            return SnapCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DC40 段闭集内"
}

// ── SystemTable 字节账解析 ──────────────────────────────────────
#[derive(Clone, Debug)]
pub struct SystemTableRaw {
    pub revision: u32,
    pub header_crc: u8,
    /// 五指针（u64 物理账面）。
    pub ptrs: [u64; SNAP_SLOTS],
}

impl SystemTableRaw {
    /// 从字节账解析（逐字段校验）。
    pub fn parse(bytes: &[u8]) -> Result<SystemTableRaw, u32> {
        if bytes.len() < ST_HEADER_LEN + 8 * SNAP_SLOTS {
            return Err(SnapCode::HeaderShort.val());
        }
        let sig = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if sig != ST_SIG {
            return Err(SnapCode::SigBad.val());
        }
        let revision = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
        // 2.0 ⇒ 0x0002_0000（高 16 位主版本）。
        if (revision >> 16) < 2 {
            return Err(SnapCode::RevisionLow.val());
        }
        let header_crc = bytes[20];
        // 头 CRC：头 24 字节（除 CRC 位）和补码账。
        let mut sum: u8 = 0;
        let mut i = 0;
        while i < ST_HEADER_LEN {
            if i != 20 {
                sum = sum.wrapping_add(bytes[i]);
            }
            i += 1;
        }
        if header_crc != sum.wrapping_neg() {
            return Err(SnapCode::CrcBad.val());
        }
        let mut ptrs = [0u64; SNAP_SLOTS];
        let mut k = 0;
        while k < SNAP_SLOTS {
            let off = ST_HEADER_LEN + k * 8;
            ptrs[k] = u64::from_le_bytes([
                bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3],
                bytes[off + 4], bytes[off + 5], bytes[off + 6], bytes[off + 7],
            ]);
            k += 1;
        }
        // 必填槽位：Runtime/Boot/RuntimeSvc 三槽不许为 0。
        let mut s = 0;
        while s < 3 {
            if ptrs[s] == 0 {
                return Err(SnapCode::NullPointer.val());
            }
            s += 1;
        }
        Ok(SystemTableRaw { revision, header_crc, ptrs })
    }
}

// ── BootManifest 固化账（只增不改） ─────────────────────────────
#[derive(Clone, Debug)]
pub struct BootManifest {
    pub ptrs: [u64; SNAP_SLOTS],
    pub sealed: bool,
    pub exited: bool,
    pub violations: u32,
    pub mutate_caught: u32,
}

impl BootManifest {
    /// 引导期快照落账（一次固化）。
    pub fn snapshot(raw: &SystemTableRaw) -> Result<BootManifest, u32> {
        Ok(BootManifest {
            ptrs: raw.ptrs,
            sealed: true,
            exited: false,
            violations: 0,
            mutate_caught: 0,
        })
    }

    /// 只读读取：封禁槽位（Boot）在 EBS 后拒。
    pub fn read_ptr(&mut self, slot: usize) -> Result<u64, u32> {
        if slot >= SNAP_SLOTS {
            return Err(SnapCode::SlotOutOfRange.val());
        }
        if self.exited && slot == SLOT_BOOT {
            self.violations += 1;
            return Err(SnapCode::BootSvcAfterExit.val());
        }
        Ok(self.ptrs[slot])
    }

    /// ExitBootServices：进入退出态（封禁生效）。
    pub fn mark_exited(&mut self) {
        self.exited = true;
    }

    /// 改写检测：快照账逐槽对拍，任一漂移记违例。
    pub fn drift_audit(&mut self, fresh: &SystemTableRaw) -> bool {
        let mut i = 0;
        while i < SNAP_SLOTS {
            if fresh.ptrs[i] != self.ptrs[i] {
                self.mutate_caught += 1;
                return false;
            }
            i += 1;
        }
        true
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

// ── 注入检出账（四路：表路/槽路/封禁路/固化路） ──────────────────
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

// ── 样本构造（合法 SystemTable 字节账） ─────────────────────────
pub fn make_systable() -> Vec<u8> {
    let total = ST_HEADER_LEN + 8 * SNAP_SLOTS;
    let mut b = vec![0u8; total];
    b[0..4].copy_from_slice(&ST_SIG.to_le_bytes());
    b[4..8].copy_from_slice(&(total as u32).to_le_bytes());
    b[8..12].copy_from_slice(&0x0002_0064u32.to_le_bytes()); // 2.100
    let bases = [0x7C00_0000u64, 0x7C10_0000u64, 0x7C20_0000u64, 0x7C30_0000u64, 0x7C40_0000u64];
    let mut k = 0;
    while k < SNAP_SLOTS {
        let off = ST_HEADER_LEN + k * 8;
        b[off..off + 8].copy_from_slice(&bases[k].to_le_bytes());
        k += 1;
    }
    let mut sum: u8 = 0;
    let mut i = 0;
    while i < ST_HEADER_LEN {
        if i != 20 {
            sum = sum.wrapping_add(b[i]);
        }
        i += 1;
    }
    b[20] = sum.wrapping_neg();
    b
}

// ── 黄金全链：解析→快照→只读读齐→EBS→封禁→对拍 ──────────────────
pub fn golden_run() -> Result<(u64, u32, u32), u32> {
    let raw = SystemTableRaw::parse(&make_systable())?;
    let mut man = BootManifest::snapshot(&raw)?;
    let rt = man.read_ptr(SLOT_RUNTIME)?;
    let mut n_read = 0u32;
    let mut s = 0;
    while s < SNAP_SLOTS {
        let _ = man.read_ptr(s)?;
        n_read += 1;
        s += 1;
    }
    man.mark_exited();
    // 退出后只读非封禁槽（Runtime/ConOut/StdErr 只读镜像）；
    // Boot 槽封禁后不再访问——黄金路径零违例（J1 语义）。
    let _ = man.read_ptr(SLOT_RUNTIME_SVC)?;
    let drift_ok = man.drift_audit(&raw);
    let bad = if drift_ok { 0 } else { 1 };
    Ok((rt, n_read, bad))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：合法样本全链绿。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 序 B：CRC 破坏 ⇒ CrcBad。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut b = make_systable();
    b[12] = b[12].wrapping_add(1);
    match SystemTableRaw::parse(&b) {
        Err(e) if e == SnapCode::CrcBad.val() => Ok(()),
        _ => Err(SnapCode::NotReproducible.val()),
    }
}

/// 序 C：必填槽 0 清零 ⇒ NullPointer。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut b = make_systable();
    let mut z = 0;
    while z < 8 {
        b[ST_HEADER_LEN + z] = 0;
        z += 1;
    }
    match SystemTableRaw::parse(&b) {
        Err(e) if e == SnapCode::NullPointer.val() => Ok(()),
        _ => Err(SnapCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：表路（签名破坏必拒）。
        let mut b = make_systable();
        b[0] ^= 0xFF;
        led.record(0, SystemTableRaw::parse(&b).is_err());
        // 路 2：槽路（越界槽读取必拒）。
        let raw = match SystemTableRaw::parse(&make_systable()) {
            Ok(v) => v,
            Err(_) => {
                led.record(1, true);
                continue;
            }
        };
        let mut man = BootManifest::snapshot(&raw)?;
        led.record(1, man.read_ptr(SNAP_SLOTS + 3).is_err());
        // 路 3：封禁路（EBS 后读 Boot 槽必拒且记账）。
        let mut man2 = BootManifest::snapshot(&raw)?;
        man2.mark_exited();
        let hit3 = match man2.read_ptr(SLOT_BOOT) {
            Err(e) if e == SnapCode::BootSvcAfterExit.val() => man2.violations == 1,
            _ => false,
        };
        led.record(2, hit3);
        // 路 4：固化路（源漂移必被对拍抓出）。
        let mut bad = raw.clone();
        bad.ptrs[SLOT_CONOUT] += 0x10;
        let hit4 = !man2.drift_audit(&bad);
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
        return Err(SnapCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SnapCode::ALL.len() {
        let v = SnapCode::ALL[i].val();
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
    // 主判据：黄金全链（五槽读齐 + 零违例 + 对拍零漂移）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((_, n_read, bad)) => *n_read == SNAP_SLOTS as u32 && *bad == 0,
            Err(_) => false,
        },
        match &g {
            Ok((rt, n, bad)) => format!("rt=0x{:x} reads={} bad={}", rt, n, bad),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 快照等价：快照账逐槽等于源账。
    let raw = SystemTableRaw::parse(&make_systable());
    let eq_hit = match &raw {
        Ok(r) => match BootManifest::snapshot(r) {
            Ok(mut m) => m.drift_audit(r),
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push((
        "J1-快照等价",
        eq_hit,
        String::from("五槽逐一对拍零漂移"),
    ));
    // 越界访问断言为零：退出前 Boot 槽可读、退出后必拒。
    let ban_hit = match &raw {
        Ok(r) => {
            let mut m = match BootManifest::snapshot(r) {
                Ok(m) => m,
                Err(_) => return_vfalse(),
            };
            let pre = m.read_ptr(SLOT_BOOT).is_ok();
            m.mark_exited();
            pre && m.read_ptr(SLOT_BOOT).is_err() && m.violations == 1
        }
        Err(_) => false,
    };
    v.push((
        "J1-退出封禁",
        ban_hit,
        String::from("EBS 前 1 绿 + EBS 后 1 红零容忍"),
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
        String::from("0x6DC40 窗口"),
    ));
    v
}

fn return_vfalse() -> BootManifest {
    BootManifest {
        ptrs: [0; SNAP_SLOTS],
        sealed: false,
        exited: false,
        violations: 1,
        mutate_caught: 0,
    }
}

pub fn run_unx0049_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0049 UEFI SystemTable 只读快照");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
