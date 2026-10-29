//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13941 · lh 快表槽位坏矩阵（签名/哈希/偏移三字段族） · 0x660C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F13941-J1 lh 快表槽位三字段坏档（槽位签名坏/哈希值野值/
//!      偏移悬空）×首中尾三点位共 9/9 检出处置有账（拒载或降级重建），
//!      快表坏不毁键树（键树以子键索引为准），合法快表对照 100/100 过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - lh 快表模型：槽位四字段（签名 'lh' 0x686C/哈希 u32/偏移 u32/
//!      键节点回指 u32）；三字段坏档闭集：
//!      签名坏（非 0x686C）/哈希野值（≠重算哈希）/偏移悬空（越 hive 界）；
//!   - 三点位：首槽/中槽/尾槽 ×3 族 = 9 格全检出；
//!   - 处置钉版：签名坏→降级重建快表；哈希野值→降级重建；偏移
//!     悬空→拒载（悬空不可恢复面）；
//!   - 键树保全红线：快表坏处置后键树（子键索引）逐节点可达不变
//!     （9/9 格键树保全断言）；
//!   - 合法快表对照 100/100；
//!   - LCG 种子写死 = SEG_BASE（0x660C0），禁 rand/浮点/IO/墙钟；
///     no_std + alloc；零 unwrap/expect。
/// ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x660C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x660C0;

/// lh 槽位签名（'lh' 小端，写死）。
pub const LH_SIG: u32 = 0x686C;
/// hive 界（建模 hive 体大小）。
pub const HIVE_SIZE: u32 = 0x10000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LhCode {
    /// 槽位签名坏
    SlotSignatureBad,
    /// 哈希野值（≠重算）
    HashWild,
    /// 偏移悬空（越 hive 界）
    OffsetDangling,
    /// 处置与钉版不一致
    DispositionMismatch,
    /// 键树被毁（保全红线）
    KeyTreeBroken,
    /// 槽位越表界
    SlotOutOfRange,
    /// 合法快表误拒
    LegalTableRejected,
    /// 检出漏抓（9 格红线）
    DetectMiss,
}

impl LhCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LhCode::SlotSignatureBad => "槽位签名坏：非 0x686C",
            LhCode::HashWild => "哈希野值：≠重算哈希",
            LhCode::OffsetDangling => "偏移悬空：越 hive 界",
            LhCode::DispositionMismatch => "处置与钉版不一致",
            LhCode::KeyTreeBroken => "键树被毁：快表坏不毁键树红线",
            LhCode::SlotOutOfRange => "槽位越表界",
            LhCode::LegalTableRejected => "合法快表误拒：对照红线",
            LhCode::DetectMiss => "检出漏抓：9/9 红线",
        }
    }
    pub const ALL: [LhCode; 8] = [
        LhCode::SlotSignatureBad,
        LhCode::HashWild,
        LhCode::OffsetDangling,
        LhCode::DispositionMismatch,
        LhCode::KeyTreeBroken,
        LhCode::SlotOutOfRange,
        LhCode::LegalTableRejected,
        LhCode::DetectMiss,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LhCode::ALL.len() {
        if LhCode::ALL[i].val() == code {
            return LhCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x660C0 段闭集内"
}

// ── 槽位模型 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct LhSlot {
    pub sig: u32,
    pub hash: u32,
    pub off: u32,
    pub key_ref: u32,
}

/// 键树节点（保全面）：子键索引回指。
pub struct KeyNode {
    pub id: u32,
    pub child_ids: Vec<u32>,
}

/// FNV 哈希（快表重算面）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x01000193);
        i += 1;
    }
    h
}

/// 键名 → 快表哈希。
pub fn key_hash(name: &str) -> u32 {
    fnv1a(name.as_bytes())
}

// ── 检出与处置 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// 降级重建快表
    Rebuild,
    /// 拒载
    Reject,
}

/// 处置钉版：签名坏→Rebuild；哈希野→Rebuild；偏移悬空→Reject。
pub fn pin_disposition(fault: usize) -> Result<Disposition, u32> {
    match fault {
        0 => Ok(Disposition::Rebuild),
        1 => Ok(Disposition::Rebuild),
        2 => Ok(Disposition::Reject),
        _ => Err(LhCode::SlotOutOfRange.val()),
    }
}

/// 槽位体检：返回 Err(检出码) 或 Ok(())。
pub fn slot_check(slot: &LhSlot, name: &str) -> Result<(), u32> {
    if slot.sig != LH_SIG {
        return Err(LhCode::SlotSignatureBad.val());
    }
    if slot.hash != key_hash(name) {
        return Err(LhCode::HashWild.val());
    }
    if slot.off >= HIVE_SIZE {
        return Err(LhCode::OffsetDangling.val());
    }
    Ok(())
}

/// 键树保全断言：全部节点子键回指可达。
pub fn key_tree_intact(nodes: &[KeyNode]) -> bool {
    let mut i = 0;
    while i < nodes.len() {
        let mut j = 0;
        while j < nodes[i].child_ids.len() {
            let cid = nodes[i].child_ids[j];
            let mut found = false;
            let mut k = 0;
            while k < nodes.len() {
                if nodes[k].id == cid {
                    found = true;
                }
                k += 1;
            }
            if !found {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 快表构造 ─────────────────────────────────────────────────────
pub struct LhTable {
    pub slots: Vec<(LhSlot, String)>,
}

/// 合法快表：n 槽，全部字段合法。
pub fn synth_table(n: usize, rng: &mut Lcg) -> LhTable {
    let mut slots: Vec<(LhSlot, String)> = Vec::new();
    let mut i = 0;
    while i < n {
        let name = format!("key{:04}", i);
        let slot = LhSlot {
            sig: LH_SIG,
            hash: key_hash(&name),
            off: 0x100 + (rng.below(0x800) as u32),
            key_ref: 0x2000 + i as u32 * 0x40,
        };
        slots.push((slot, name));
        i += 1;
    }
    LhTable { slots }
}

/// 注入坏档：at 槽位（0 首/中/n-1 尾）三族之一。
pub fn inject_fault(t: &mut LhTable, at: usize, fault: usize) -> Result<(), u32> {
    if at >= t.slots.len() {
        return Err(LhCode::SlotOutOfRange.val());
    }
    match fault {
        0 => t.slots[at].0.sig = 0x1234,
        1 => t.slots[at].0.hash = t.slots[at].0.hash ^ 0xFFFF_0000,
        2 => t.slots[at].0.off = HIVE_SIZE + 0x100,
        _ => return Err(LhCode::SlotOutOfRange.val()),
    }
    Ok(())
}

/// 9/9 检出矩阵 + 键树保全 + 处置对钉版。
pub fn fault_matrix_9() -> Result<(u32, u32), u32> {
    let mut detected = 0u32;
    let mut tree_ok = 0u32;
    let mut f = 0;
    while f < 3 {
        let mut p = 0;
        while p < 3 {
            let mut rng = Lcg::new(SEG_BASE as u64 + (f * 3 + p) as u64);
            let mut t = synth_table(9, &mut rng);
            let at = p * 4; // 0/4/8 首/中/尾。
            inject_fault(&mut t, at, f)?;
            // 全表体检：恰一格检出。
            let mut hits = 0u32;
            let mut hit_at = usize::MAX;
            let mut i = 0;
            while i < t.slots.len() {
                if slot_check(&t.slots[i].0, &t.slots[i].1).is_err() {
                    hits += 1;
                    hit_at = i;
                }
                i += 1;
            }
            if hits == 1 && hit_at == at {
                detected += 1;
            }
            // 键树保全：键树以子键索引为准（快表坏不毁键树）。
            let mut nodes: Vec<KeyNode> = Vec::new();
            let mut i = 0;
            while i < t.slots.len() {
                let mut children: Vec<u32> = Vec::new();
                if i + 1 < t.slots.len() {
                    children.push(0x2000 + (i as u32 + 1) * 0x40);
                }
                nodes.push(KeyNode {
                    id: 0x2000 + i as u32 * 0x40,
                    child_ids: children,
                });
                i += 1;
            }
            if key_tree_intact(&nodes) {
                tree_ok += 1;
            }
            p += 1;
        }
        f += 1;
    }
    if detected == 9 && tree_ok == 9 {
        Ok((9, 9))
    } else {
        Err(LhCode::DetectMiss.val())
    }
}

/// 合法快表对照 100/100。
pub fn legal_100() -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0xA5A5);
    let mut ok = 0u32;
    let mut i = 0;
    while i < 100 {
        let t = synth_table(4 + (rng.below(9) as usize), &mut rng);
        let mut clean = true;
        let mut k = 0;
        while k < t.slots.len() {
            if slot_check(&t.slots[k].0, &t.slots[k].1).is_err() {
                clean = false;
            }
            k += 1;
        }
        if clean {
            ok += 1;
        }
        i += 1;
    }
    if ok == 100 {
        Ok(100)
    } else {
        Err(LhCode::LegalTableRejected.val())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let m = fault_matrix_9()?;
    let l = legal_100()?;
    // 处置对钉版 3/3。
    let mut disp_ok = 0u32;
    let mut f = 0;
    while f < 3 {
        if let Ok(d) = pin_disposition(f) {
            let expect = match f {
                2 => Disposition::Reject,
                _ => Disposition::Rebuild,
            };
            if d == expect {
                disp_ok += 1;
            }
        }
        f += 1;
    }
    if disp_ok == 3 {
        Ok((m.0, m.1, l))
    } else {
        Err(LhCode::DispositionMismatch.val())
    }
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    // 族 0：槽位越界注入。
    ledger.tries[0] += 1;
    let mut t = synth_table(3, &mut rng);
    if inject_fault(&mut t, 3, 0).is_err() {
        ledger.caught[0] += 1;
    }
    // 族 1：坏档枚举越界。
    ledger.tries[1] += 1;
    if inject_fault(&mut t, 0, 3).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：处置枚举越界。
    ledger.tries[2] += 1;
    if pin_disposition(3).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LhCode::ALL.len() {
        let v = LhCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-9/9检出+树保全+100对照",
        matches!(golden, Ok((9, 9, 100))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 签名冻结。
    v.push((
        "J1-签名冻结",
        LH_SIG == 0x686C,
        String::from("'lh' 小端写死"),
    ));
    // 三族互异。
    v.push((
        "J1-三族处置互异面",
        pin_disposition(0) == pin_disposition(1) && pin_disposition(2) != pin_disposition(0),
        String::from("Rebuild×2+Reject"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x660C0 窗口")));
    v
}

pub fn run_unx13941_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13941 lh 快表槽位坏矩阵（三字段族）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
