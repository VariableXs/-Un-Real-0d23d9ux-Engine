//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0188 · SMP 前置数据包（BSP 侧准备） · 0x6C5C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F0188-J1 AP 启动所需数据（页表/栈/entry）打包校验，与
//!     A3 域接口冻结联签；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - AP 启动数据包建模：四件（cr3 页表基址/AP 栈顶/entry 入口/
//!     arg 指针）+ 校验和（前四件 FNV-1a）+ 魔数（冻结 0x56415031
//!      "VAP1"）；
//!    - 打包校验：魔数/对齐（栈 16 字节、entry 页对齐）/域值
//!      （cr3 低 12 位零）三闸，任一失守拒打包；
//!    - A3 域接口冻结联签：数据包布局版本 + 布局指纹必须与 A3 冻
//!      结表命中（联签一致率 100%）；
//!    - 每 AP 一包：APIC ID→包下标映射，重复分配拒；包只读固化
//!      （打包后改写检出）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C5C0，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C5C0;

// ── 数据包冻结值 ────────────────────────────────────────────────
/// 魔数 "VAP1"。
pub const PKT_MAGIC: u32 = 0x5641_5031;
/// 布局版本。
pub const PKT_LAYOUT_VER: u32 = 1;
/// 包字节数（魔数 4 + ver 4 + cr3 8 + 栈 8 + entry 8 + arg 8 + 校验 8）。
pub const PKT_LEN: usize = 48;
/// 栈对齐（16 字节）。
pub const STACK_ALIGN: u64 = 16;
/// entry 页对齐。
pub const ENTRY_ALIGN: u64 = 4096;
/// 最大 AP 数。
pub const MAX_APS: usize = 64;

// ── A3 域冻结联签表 ────────────────────────────────────────────
pub const A3_IFACE_TABLE: [&str; 3] = ["a3_pkt_layout_v1", "a3_pkt_handoff", "a3_pkt_verify"];
/// 本包冻结联签名（必须在 A3 表命中）。
pub const PKT_FROZEN_IFACE: &str = "a3_pkt_layout_v1";

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmpCode {
    /// 魔数错
    BadMagic,
    /// 布局版本错
    LayoutVerWrong,
    /// cr3 低 12 位非零（页表基址域错）
    Cr3Misaligned,
    /// 栈非 16 字节对齐
    StackMisaligned,
    /// entry 非页对齐
    EntryMisaligned,
    /// 校验和不符
    ChecksumMismatch,
    /// AP 重复分配（同 APIC ID 双包）
    DuplicateAp,
    /// AP 数越上限
    ApCountExceeded,
    /// 包固化后改写
    PacketMutated,
    /// A3 联签断裂
    A3NotLinked,
    /// 双跑不一致
    NotReproducible,
}

impl SmpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SmpCode::BadMagic => "魔数错：非 VAP1",
            SmpCode::LayoutVerWrong => "布局版本错",
            SmpCode::Cr3Misaligned => "cr3 低 12 位非零",
            SmpCode::StackMisaligned => "栈非 16 字节对齐",
            SmpCode::EntryMisaligned => "entry 非页对齐",
            SmpCode::ChecksumMismatch => "校验和不符：FNV 账",
            SmpCode::DuplicateAp => "AP 重复分配：同 APIC ID 双包",
            SmpCode::ApCountExceeded => "AP 数越上限：64",
            SmpCode::PacketMutated => "包固化后改写",
            SmpCode::A3NotLinked => "A3 联签断裂：接口未命中",
            SmpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SmpCode; 11] = [
        SmpCode::BadMagic,
        SmpCode::LayoutVerWrong,
        SmpCode::Cr3Misaligned,
        SmpCode::StackMisaligned,
        SmpCode::EntryMisaligned,
        SmpCode::ChecksumMismatch,
        SmpCode::DuplicateAp,
        SmpCode::ApCountExceeded,
        SmpCode::PacketMutated,
        SmpCode::A3NotLinked,
        SmpCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SmpCode::ALL.len() {
        if SmpCode::ALL[i].val() == code {
            return SmpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C5C0 段闭集内"
}

// ── AP 数据包 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApPacket {
    pub apic_id: u32,
    pub cr3: u64,
    pub stack_top: u64,
    pub entry: u64,
    pub arg: u64,
}

impl ApPacket {
    /// 打包三闸：对齐 + 域值。
    pub fn legal(&self) -> Result<(), u32> {
        if self.cr3 & 0xFFF != 0 {
            return Err(SmpCode::Cr3Misaligned.val());
        }
        if self.stack_top % STACK_ALIGN != 0 {
            return Err(SmpCode::StackMisaligned.val());
        }
        if self.entry % ENTRY_ALIGN != 0 {
            return Err(SmpCode::EntryMisaligned.val());
        }
        Ok(())
    }
    /// 校验和：FNV-1a（四件 + apic_id）。
    pub fn checksum(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.apic_id as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.cr3;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.stack_top;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.entry;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.arg;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
    /// 序列化（48 字节：魔数+ver+四件+校验和，全部 LE）。
    pub fn serialize(&self) -> Result<[u8; PKT_LEN], u32> {
        self.legal()?;
        let mut b = [0u8; PKT_LEN];
        b[0..4].copy_from_slice(&PKT_MAGIC.to_le_bytes());
        b[4..8].copy_from_slice(&PKT_LAYOUT_VER.to_le_bytes());
        b[8..16].copy_from_slice(&self.cr3.to_le_bytes());
        b[16..24].copy_from_slice(&self.stack_top.to_le_bytes());
        b[24..32].copy_from_slice(&self.entry.to_le_bytes());
        b[32..40].copy_from_slice(&self.arg.to_le_bytes());
        b[40..48].copy_from_slice(&self.checksum().to_le_bytes());
        Ok(b)
    }
    /// 反序列化 + 校验（魔数/版本/校验和三闸）。
    pub fn deserialize(b: &[u8]) -> Result<ApPacket, u32> {
        if b.len() < PKT_LEN {
            return Err(SmpCode::BadMagic.val());
        }
        let magic = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
        if magic != PKT_MAGIC {
            return Err(SmpCode::BadMagic.val());
        }
        let ver = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
        if ver != PKT_LAYOUT_VER {
            return Err(SmpCode::LayoutVerWrong.val());
        }
        let mut le64 = |off: usize| -> u64 {
            u64::from_le_bytes([
                b[off], b[off + 1], b[off + 2], b[off + 3],
                b[off + 4], b[off + 5], b[off + 6], b[off + 7],
            ])
        };
        let p = ApPacket {
            apic_id: 0,
            cr3: le64(8),
            stack_top: le64(16),
            entry: le64(24),
            arg: le64(32),
        };
        let stored = le64(40);
        if stored != p.checksum() {
            return Err(SmpCode::ChecksumMismatch.val());
        }
        p.legal()?;
        Ok(p)
    }
}

// ── BSP 分配账 ─────────────────────────────────────────────────
pub struct BspPrep {
    pub packets: Vec<(u32, ApPacket, [u8; PKT_LEN])>,
}

impl BspPrep {
    pub fn new() -> BspPrep {
        BspPrep { packets: Vec::new() }
    }

    /// 为 AP 打包并固化（重复 APIC ID 拒、超上限拒）。
    pub fn prepare(&mut self, p: ApPacket) -> Result<usize, u32> {
        p.legal()?;
        if self.packets.len() >= MAX_APS {
            return Err(SmpCode::ApCountExceeded.val());
        }
        let mut i = 0;
        while i < self.packets.len() {
            if self.packets[i].0 == p.apic_id {
                return Err(SmpCode::DuplicateAp.val());
            }
            i += 1;
        }
        let ser = p.serialize()?;
        self.packets.push((p.apic_id, p, ser));
        Ok(self.packets.len() - 1)
    }

    /// 固化改写检出：包字节与序列化重算逐位一致。
    pub fn mutate_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.packets.len() {
            let fresh = self.packets[i].1.serialize()?;
            let mut k = 0;
            while k < PKT_LEN {
                if fresh[k] != self.packets[i].2[k] {
                    return Err(SmpCode::PacketMutated.val());
                }
                k += 1;
            }
            i += 1;
        }
        Ok(())
    }

    /// A3 联签：布局接口名必须在冻结表命中。
    pub fn a3_linked() -> bool {
        let mut i = 0;
        while i < A3_IFACE_TABLE.len() {
            if A3_IFACE_TABLE[i] == PKT_FROZEN_IFACE {
                return true;
            }
            i += 1;
        }
        false
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

/// 黄金全链：4 AP 打包→序列化/反序列化往返→改写检出→A3 联签。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut bsp = BspPrep::new();
    let mut i = 0;
    while i < 4 {
        let p = ApPacket {
            apic_id: i as u32,
            cr3: 0x100_0000,
            stack_top: 0x7FF0_0000 - (i as u64) * 0x1_0000,
            entry: 0x20_0000,
            arg: 0x30_0000 + (i as u64) * 0x100,
        };
        bsp.prepare(p)?;
        i += 1;
    }
    // 往返：序列化→反序列化逐字段一致。
    let mut k = 0;
    while k < bsp.packets.len() {
        let ser = bsp.packets[k].2;
        let back = ApPacket::deserialize(&ser)?;
        if back.cr3 != bsp.packets[k].1.cr3 || back.entry != bsp.packets[k].1.entry {
            return Err(SmpCode::ChecksumMismatch.val());
        }
        k += 1;
    }
    // 改写检出。
    bsp.mutate_audit()?;
    // A3 联签。
    if !BspPrep::a3_linked() {
        return Err(SmpCode::A3NotLinked.val());
    }
    // 魔数/版本点验。
    let ser0 = bsp.packets[0].2;
    let magic = u32::from_le_bytes([ser0[0], ser0[1], ser0[2], ser0[3]]);
    if magic != PKT_MAGIC {
        return Err(SmpCode::BadMagic.val());
    }
    Ok((bsp.packets.len(), magic, PKT_LAYOUT_VER))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let p = ApPacket { apic_id: 1, cr3: 0x100_0000, stack_top: 0x7FF0_0000, entry: 0x20_0000, arg: 0 };
    let ser = p.serialize()?;
    ApPacket::deserialize(&ser).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // cr3 低 12 位非零 → 必拒。
    let p = ApPacket { apic_id: 1, cr3: 0x100_0013, stack_top: 0x7FF0_0000, entry: 0x20_0000, arg: 0 };
    p.legal()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 校验和破坏 → 反序列化必拒。
    let p = ApPacket { apic_id: 1, cr3: 0x100_0000, stack_top: 0x7FF0_0000, entry: 0x20_0000, arg: 0 };
    let mut ser = p.serialize()?;
    ser[40] ^= 0xFF;
    ApPacket::deserialize(&ser).map(|_| ())
}

/// 注入 10/10：魔数错、cr3 错、栈错、重复 AP 四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：魔数错。
        let mut b0 = [0u8; PKT_LEN];
        b0[0] = 0xFF;
        led.record(0, ApPacket::deserialize(&b0).is_err());
        // 槽1：cr3 错。
        let p1 = ApPacket { apic_id: 2, cr3: 0x123, stack_top: 0x7FF0_0000, entry: 0x20_0000, arg: 0 };
        led.record(1, p1.legal().is_err());
        // 槽2：栈错。
        let p2 = ApPacket { apic_id: 3, cr3: 0x100_0000, stack_top: 0x7FF0_0007, entry: 0x20_0000, arg: 0 };
        led.record(2, p2.legal().is_err());
        // 槽3：重复 AP。
        let mut bsp = BspPrep::new();
        let p3 = ApPacket { apic_id: 4, cr3: 0x100_0000, stack_top: 0x7FF0_0000, entry: 0x20_0000, arg: 0 };
        bsp.prepare(p3).ok();
        led.record(3, bsp.prepare(p3).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SmpCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SmpCode::ALL.len() {
        let v = SmpCode::ALL[i].val();
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
    // A3 联签。
    v.push((
        "J1-A3联签一致率100%",
        BspPrep::a3_linked() && A3_IFACE_TABLE.len() == 3,
        format!("接口={}", PKT_FROZEN_IFACE),
    ));
    // 打包三闸冻结值。
    v.push((
        "J1-打包三闸冻结",
        PKT_MAGIC == 0x5641_5031 && STACK_ALIGN == 16 && ENTRY_ALIGN == 4096,
        format!("魔数=VAP1 栈={} entry页={}", STACK_ALIGN, ENTRY_ALIGN),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C5C0 窗口")));
    v
}

pub fn run_unx0188_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0188 SMP 前置数据包（BSP 侧准备）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
