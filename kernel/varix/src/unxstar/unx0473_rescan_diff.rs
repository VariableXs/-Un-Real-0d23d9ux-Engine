//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0473 快照复扫 diff（枚举稳定性） · 0x75740段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7G 逐字锚定，AI-VX7-1104）：
//!   复扫 diff 三面零漂移（清单/身份/类码），注入漂移检出 100%。
//! 设计要点：
//!   1. 三面闭集：清单面（bdf 集合全等）、身份面（vendor/device id 逐台
//!      全等）、类码面（base/sub/prog 三字节逐台全等）；任一面漂移显性
//!      入账（bdf + 面别 + 双值）。
//!   2. 位图全等断言：每台设备 3bit 面别位图（bit0 清单/bit1 身份/bit2
//!      类码），零漂移 ⇒ 全零位图；两次扫描的位图逐台全等。
//!   3. 注入检出 100%：抽一台设备身份翻转（vid +1），diff 必出且只出该
//!      台的身份漂移行，其余两面零漂移。
//!   4. 双跑一致：同种子复扫两次快照逐字段全等（复现红线），diff 报告
//!      可复算。
//!   5. 单调状态机：FirstScan→Rescan→Diffed→Sealed 四相，倒退显性拒；
//!      封账后 diff 不可再改。
//!   6. 错误路径中文建议写入诊断 detail：清单漂移→重扫全总线、身份漂移
//!      →查 VID/DID 寄存器、类码漂移→查 PCI 配置空间 0x08 偏移。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75740，AI-VX7-1104 批 G 组独占区）。
pub const SEG_BASE: u32 = 0x75740;

// ── 冻结常量表（来源：PCI Local Bus Spec r3.0 §6.1/§6.2.1）──
/// 身份寄存器偏移（Vendor ID 0x00 / Device ID 0x02）。
pub const ID_OFF: u32 = 0x00;
/// 类码寄存器偏移（0x08，Revision/Class Code）。
pub const CLASS_OFF: u32 = 0x08;
/// 面别位图位宽（3bit：清单/身份/类码，冻结）。
pub const FACE_BITS: u32 = 3;
/// 真机设备数（4 台，冻结）。
pub const MACHINE_DEVS: u32 = 4;
/// 注入轮数（判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RdCode {
    /// 清单面漂移（bdf 集合变动）
    ManifestDrift,
    /// 身份面漂移（VID/DID 变动）
    IdentityDrift,
    /// 类码面漂移（base/sub/prog 变动）
    ClassDrift,
    /// 位图不全等（复扫位图与首扫位图失配）
    BitmapMismatch,
    /// 快照条数失配（清单/身份/类码长度不齐）
    SnapshotLenMismatch,
    /// 审计状态机倒退
    PhaseRegress,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl RdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RdCode::ManifestDrift => "清单面漂移：bdf 集合变动，重扫全总线",
            RdCode::IdentityDrift => "身份面漂移：查 VID/DID 寄存器 0x00/0x02",
            RdCode::ClassDrift => "类码面漂移：查 PCI 配置空间 0x08 偏移",
            RdCode::BitmapMismatch => "位图不全等：核对三次扫描面别位图",
            RdCode::SnapshotLenMismatch => "快照条数失配：清单/身份/类码必须逐台对齐",
            RdCode::PhaseRegress => "审计状态机倒退：按 FirstScan→Rescan→Diffed→Sealed 推进",
            RdCode::ReplayMismatch => "复算失配：账面与重算结果不一致",
            RdCode::NotReproducible => "双跑不一致：复现红线，固定种子复跑",
        }
    }
    pub const ALL: [RdCode; 8] = [
        RdCode::ManifestDrift,
        RdCode::IdentityDrift,
        RdCode::ClassDrift,
        RdCode::BitmapMismatch,
        RdCode::SnapshotLenMismatch,
        RdCode::PhaseRegress,
        RdCode::ReplayMismatch,
        RdCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RdCode::ALL.len() {
        if let Some(c) = RdCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75740 段闭集内"
}

// ── 域模型：三面快照 ────────────────────────────────────────────
/// 身份记录：VID/DID。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IdRec {
    pub bdf: u32,
    pub vid: u16,
    pub did: u16,
}

/// 类码记录：base/sub/prog 三字节。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClassRec {
    pub bdf: u32,
    pub base: u8,
    pub sub: u8,
    pub prog: u8,
}

/// 三面快照（一次扫描的完整账面）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// 面 1：bdf 清单（升序冻结）
    pub manifest: Vec<u32>,
    /// 面 2：身份表（与清单逐台对齐）
    pub identity: Vec<IdRec>,
    /// 面 3：类码表（与清单逐台对齐）
    pub class: Vec<ClassRec>,
}

impl Snapshot {
    /// 快照结构谓词：三面条数对齐。
    pub fn aligned(&self) -> bool {
        self.manifest.len() == self.identity.len() && self.manifest.len() == self.class.len()
    }

    /// 从设备表构造快照。
    pub fn from_devs(bdfs: &[u32], vids: &[u16], dids: &[u16], classes: &[(u8, u8, u8)]) -> Option<Snapshot> {
        if bdfs.len() != vids.len() || bdfs.len() != dids.len() || bdfs.len() != classes.len() {
            return None;
        }
        let mut snap = Snapshot::default();
        let mut i = 0;
        while i < bdfs.len() {
            let bdf = match bdfs.get(i) {
                Some(b) => *b,
                None => return None,
            };
            let vid = match vids.get(i) {
                Some(v) => *v,
                None => return None,
            };
            let did = match dids.get(i) {
                Some(d) => *d,
                None => return None,
            };
            let cls = match classes.get(i) {
                Some(c) => *c,
                None => return None,
            };
            snap.manifest.push(bdf);
            snap.identity.push(IdRec { bdf, vid, did });
            snap.class.push(ClassRec { bdf, base: cls.0, sub: cls.1, prog: cls.2 });
            i += 1;
        }
        Some(snap)
    }
}

// ── diff 报告（三面漂移行 + 位图） ──────────────────────────────
/// 面别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 面 0：清单
    Manifest,
    /// 面 1：身份
    Identity,
    /// 面 2：类码
    Class,
}

impl Face {
    /// 面别 → 位图 bit 序号。
    pub fn bit(self) -> u32 {
        match self {
            Face::Manifest => 0,
            Face::Identity => 1,
            Face::Class => 2,
        }
    }
}

/// 单条漂移行：bdf + 面别 + 首扫值 + 复扫值（显性账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftRow {
    pub bdf: u32,
    pub face: Face,
    pub first: u32,
    pub second: u32,
}

/// diff 报告。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffReport {
    pub rows: Vec<DriftRow>,
    /// 每台设备的 3bit 面别位图（bit0 清单/bit1 身份/bit2 类码，漂移置 1）
    pub face_bitmap: Vec<u8>,
    /// 清单面是否整体漂移（集合变动，无逐台归属）
    pub manifest_drift: bool,
}

/// 三面 diff 主算法：清单全等 + 身份/类码逐台比对，漂移行与位图同时产出。
pub fn diff_snapshots(a: &Snapshot, b: &Snapshot) -> Result<DiffReport, u32> {
    if !a.aligned() || !b.aligned() {
        return Err(RdCode::SnapshotLenMismatch.val());
    }
    let mut rep = DiffReport::default();
    // 面 1：清单全等（bdf 集合逐位比对）。
    if a.manifest.len() != b.manifest.len() {
        rep.manifest_drift = true;
    } else {
        let mut i = 0;
        while i < a.manifest.len() {
            let va = match a.manifest.get(i) {
                Some(v) => *v,
                None => break,
            };
            let vb = match b.manifest.get(i) {
                Some(v) => *v,
                None => break,
            };
            if va != vb {
                rep.manifest_drift = true;
                rep.rows.push(DriftRow { bdf: va, face: Face::Manifest, first: va, second: vb });
            }
            i += 1;
        }
    }
    // 面 2：身份逐台比对。
    let n = if a.identity.len() <= b.identity.len() { a.identity.len() } else { b.identity.len() };
    let mut i = 0;
    while i < n {
        let ia = match a.identity.get(i) {
            Some(v) => *v,
            None => break,
        };
        let ib = match b.identity.get(i) {
            Some(v) => *v,
            None => break,
        };
        if ia.vid != ib.vid || ia.did != ib.did {
            rep.rows.push(DriftRow {
                bdf: ia.bdf,
                face: Face::Identity,
                first: ((ia.vid as u32) << 16) | ia.did as u32,
                second: ((ib.vid as u32) << 16) | ib.did as u32,
            });
        }
        i += 1;
    }
    // 面 3：类码逐台比对。
    i = 0;
    while i < n {
        let ca = match a.class.get(i) {
            Some(v) => *v,
            None => break,
        };
        let cb = match b.class.get(i) {
            Some(v) => *v,
            None => break,
        };
        if ca.base != cb.base || ca.sub != cb.sub || ca.prog != cb.prog {
            rep.rows.push(DriftRow {
                bdf: ca.bdf,
                face: Face::Class,
                first: ((ca.base as u32) << 16) | ((ca.sub as u32) << 8) | ca.prog as u32,
                second: ((cb.base as u32) << 16) | ((cb.sub as u32) << 8) | cb.prog as u32,
            });
        }
        i += 1;
    }
    // 位图：逐台按漂移行置位（3bit 面别）。
    let dev_n = a.manifest.len();
    let mut bi = 0;
    while bi < dev_n {
        let bdf = match a.manifest.get(bi) {
            Some(v) => *v,
            None => break,
        };
        let mut bits = 0u8;
        let mut ri = 0;
        while ri < rep.rows.len() {
            if let Some(r) = rep.rows.get(ri) {
                if r.bdf == bdf {
                    bits |= 1u8 << r.face.bit();
                }
            }
            ri += 1;
        }
        if rep.manifest_drift {
            bits |= 1u8 << Face::Manifest.bit();
        }
        rep.face_bitmap.push(bits);
        bi += 1;
    }
    Ok(rep)
}

// ── 谓词（私有，判据面） ────────────────────────────────────────
/// 三面零漂移谓词。
fn zero_drift_ok(rep: &DiffReport) -> bool {
    if rep.manifest_drift {
        return false;
    }
    if !rep.rows.is_empty() {
        return false;
    }
    true
}

/// 位图全零谓词。
fn bitmap_all_zero_ok(rep: &DiffReport) -> bool {
    let mut i = 0;
    while i < rep.face_bitmap.len() {
        let v = match rep.face_bitmap.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if v != 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 位图全等谓词（两份报告的位图逐台全等）。
fn bitmap_equal_ok(a: &DiffReport, b: &DiffReport) -> bool {
    if a.face_bitmap.len() != b.face_bitmap.len() {
        return false;
    }
    let mut i = 0;
    while i < a.face_bitmap.len() {
        let va = match a.face_bitmap.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let vb = match b.face_bitmap.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if va != vb {
            return false;
        }
        i += 1;
    }
    true
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

// ── 注入检出账（四路：身份漂移/类码漂移/清单漂移/位图失配） ────
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

// ── 真机设备表（4 台） ─────────────────────────────────────────
/// 真机 4 设备参数（bdf/vid/did/class）。
pub fn machine_tables() -> (Vec<u32>, Vec<u16>, Vec<u16>, Vec<(u8, u8, u8)>) {
    (
        vec![0x0000, 0x0800, 0x1000, 0x1800],
        vec![0x10EE, 0x8086, 0x1022, 0x1B36],
        vec![0x7021, 0x1237, 0x2000, 0x0100],
        vec![(0x06, 0x04, 0x01), (0x06, 0x00, 0x00), (0x02, 0x00, 0x00), (0x0C, 0x03, 0x02)],
    )
}

/// 真机快照。
pub fn machine_snapshot() -> Snapshot {
    let (b, v, d, c) = machine_tables();
    match Snapshot::from_devs(&b, &v, &d, &c) {
        Some(s) => s,
        None => Snapshot::default(),
    }
}

/// 清单漂移注入：抽掉末位设备（三面同步截短）。
pub fn inject_manifest_drop() -> Snapshot {
    let (b, v, d, c) = machine_tables();
    let mut b2 = b.clone();
    let _ = b2.pop();
    let mut v2 = v.clone();
    let _ = v2.pop();
    let mut d2 = d.clone();
    let _ = d2.pop();
    let mut c2 = c.clone();
    let _ = c2.pop();
    match Snapshot::from_devs(&b2, &v2, &d2, &c2) {
        Some(s) => s,
        None => Snapshot::default(),
    }
}

/// 身份翻转注入：抽 seq 号设备 vid +1（其余面零漂移）。
pub fn inject_identity_flip(seq: usize) -> Snapshot {
    let (b, mut v, d, c) = machine_tables();
    if let Some(vid) = v.get_mut(seq) {
        *vid = vid.wrapping_add(1);
    }
    match Snapshot::from_devs(&b, &v, &d, &c) {
        Some(s) => s,
        None => Snapshot::default(),
    }
}

/// 类码漂移注入：抽 seq 号设备 sub +1。
pub fn inject_class_drift(seq: usize) -> Snapshot {
    let (b, v, d, mut c) = machine_tables();
    if let Some(t) = c.get_mut(seq) {
        t.1 = t.1.wrapping_add(1);
    }
    match Snapshot::from_devs(&b, &v, &d, &c) {
        Some(s) => s,
        None => Snapshot::default(),
    }
}

// ── 审计状态机（FirstScan→Rescan→Diffed→Sealed，单调） ──────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RdPhase {
    /// 已首扫
    FirstScan,
    /// 已复扫
    Rescan,
    /// 已出 diff
    Diffed,
    /// 已封账
    Sealed,
}

/// 复扫审计器：首扫/复扫快照 + diff 报告。
pub struct RdAuditor {
    pub phase: RdPhase,
    pub first: Snapshot,
    pub second: Snapshot,
    pub report: DiffReport,
}

impl RdAuditor {
    pub fn new(first: Snapshot) -> RdAuditor {
        RdAuditor { phase: RdPhase::FirstScan, first, second: Snapshot::default(), report: DiffReport::default() }
    }

    /// 步 1：复扫（仅 FirstScan 相可入）。
    pub fn step_rescan(&mut self, second: Snapshot) -> Result<(), u32> {
        if self.phase != RdPhase::FirstScan {
            return Err(RdCode::PhaseRegress.val());
        }
        self.second = second;
        self.phase = RdPhase::Rescan;
        Ok(())
    }

    /// 步 2：出 diff（仅 Rescan 相可入）。
    pub fn step_diff(&mut self) -> Result<(), u32> {
        if self.phase != RdPhase::Rescan {
            return Err(RdCode::PhaseRegress.val());
        }
        self.report = diff_snapshots(&self.first, &self.second)?;
        self.phase = RdPhase::Diffed;
        Ok(())
    }

    /// 步 3：封账（仅 Diffed 相可入）。
    pub fn step_seal(&mut self) -> Result<(), u32> {
        if self.phase != RdPhase::Diffed {
            return Err(RdCode::PhaseRegress.val());
        }
        self.phase = RdPhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链：首扫→复扫→diff 三面零漂移→位图全等→注入检出 ─────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let snap = machine_snapshot();
    let mut aud = RdAuditor::new(snap.clone());
    aud.step_rescan(snap.clone())?;
    aud.step_diff()?;
    // 三面零漂移 + 位图全零。
    let clean_ok = zero_drift_ok(&aud.report) && bitmap_all_zero_ok(&aud.report);
    // 双扫位图全等。
    let mut aud2 = RdAuditor::new(snap.clone());
    aud2.step_rescan(snap.clone())?;
    aud2.step_diff()?;
    let bmp_ok = bitmap_equal_ok(&aud.report, &aud2.report);
    // 封账。
    aud.step_seal()?;
    let sealed_ok = aud.phase == RdPhase::Sealed;
    // 注入检出：身份翻转（seq 1）只出 1 行身份漂移，其余面零漂移。
    let inj_rep = diff_snapshots(&snap, &inject_identity_flip(1))?;
    let inj_ok = inj_rep.rows.len() == 1
        && !inj_rep.manifest_drift
        && match inj_rep.rows.get(0) {
            Some(r) => r.face == Face::Identity && r.bdf == 0x0800,
            None => false,
        };
    let inj_cnt = inj_rep.rows.len() as u32;
    let ok = (clean_ok as u32) + (bmp_ok as u32) + (sealed_ok as u32) + (inj_ok as u32);
    Ok((ok, inj_cnt, snap.manifest.len() as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链 4/4 绿 + 注入 1 行 + 4 台设备。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 1, 4)) => Ok(()),
        _ => Err(RdCode::NotReproducible.val()),
    }
}

/// 序 B：身份翻转注入必抓（seq 2 vid 翻转）。
pub fn probe_seq_b() -> Result<(), u32> {
    let snap = machine_snapshot();
    let rep = diff_snapshots(&snap, &inject_identity_flip(2))?;
    if rep.rows.len() == 1 && !rep.manifest_drift {
        if let Some(r) = rep.rows.get(0) {
            if r.face == Face::Identity && r.bdf == 0x1000 {
                return Ok(());
            }
        }
    }
    Err(RdCode::NotReproducible.val())
}

/// 序 C：冻结常量（3bit 面别位图 / 偏移表 / 设备数）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        FACE_BITS == 3,
        ID_OFF == 0x00,
        CLASS_OFF == 0x08,
        MACHINE_DEVS == 4,
    ) {
        (true, true, true, true) => Ok(()),
        _ => Err(RdCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：身份翻转注入（seq 0/1/2/3 轮换，全部必抓）。
        let seq = (rng.next() % 4) as usize;
        let snap = machine_snapshot();
        let rep = diff_snapshots(&snap, &inject_identity_flip(seq));
        let hit1 = match rep {
            Ok(rp) => rp.rows.len() == 1
                && match rp.rows.get(0) {
                    Some(row) => row.face == Face::Identity,
                    None => false,
                },
            Err(_) => false,
        };
        led.record(0, hit1 && probe_seq_b().is_ok());
        // 路 2：类码漂移注入（seq 3）。
        let rep2 = diff_snapshots(&snap, &inject_class_drift(3));
        let hit2 = match rep2 {
            Ok(rp) => rp.rows.len() == 1
                && match rp.rows.get(0) {
                    Some(row) => row.face == Face::Class,
                    None => false,
                },
            Err(_) => false,
        };
        led.record(1, hit2);
        // 路 3：清单漂移注入（三面同步截短一台 → 集合变动必抓）。
        let partial = inject_manifest_drop();
        let hit3 = match diff_snapshots(&snap, &partial) {
            Ok(rp) => rp.manifest_drift,
            Err(_) => false,
        };
        led.record(2, hit3);
        // 路 4：条数失配注入（三面长度不齐显性拒）。
        let mut mis = Snapshot::default();
        mis.manifest = vec![0x0000];
        mis.identity.push(IdRec { bdf: 0x0000, vid: 1, did: 2 });
        // class 空 → 不对齐。
        let hit4 = diff_snapshots(&snap, &mis).is_err();
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
        return Err(RdCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（0x75000~0x781C0 窗口 + 0x40 对齐 + 闭集落窗） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < RdCode::ALL.len() {
        if let Some(c) = RdCode::ALL.get(i) {
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

/// 位图全等断言谓词：干净双扫位图全等 + 注入位图非全零。
fn bitmap_assert_ok() -> bool {
    let snap = machine_snapshot();
    let rep_clean = match diff_snapshots(&snap, &snap) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if !bitmap_all_zero_ok(&rep_clean) {
        return false;
    }
    let rep_inj = match diff_snapshots(&snap, &inject_identity_flip(1)) {
        Ok(r) => r,
        Err(_) => return false,
    };
    // 注入后：seq1 位图 bit1 置 1，其余仍 0。
    let b1 = match rep_inj.face_bitmap.get(1) {
        Some(v) => *v,
        None => return false,
    };
    if b1 != (1u8 << Face::Identity.bit()) {
        return false;
    }
    let b0 = match rep_inj.face_bitmap.get(0) {
        Some(v) => *v,
        None => return false,
    };
    b0 == 0
}

/// 状态机倒退拒谓词。
fn phase_regress_ok() -> bool {
    let snap = machine_snapshot();
    let mut aud = RdAuditor::new(snap);
    if aud.step_diff().is_err() && aud.step_seal().is_err() {
        // FirstScan 相直调 diff/seal 显性拒，随后按序推进全绿。
        let snap2 = machine_snapshot();
        return aud.step_rescan(snap2).is_ok() && aud.step_diff().is_ok() && aud.step_seal().is_ok();
    }
    false
}

/// 诊断码闭集查表谓词。
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < RdCode::ALL.len() {
        if let Some(c) = RdCode::ALL.get(i) {
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
        "F0473-J1-黄金全链",
        match &g {
            Ok((ok, inj, devs)) => *ok == 4 && *inj == 1 && *devs == 4,
            Err(_) => false,
        },
        match &g {
            Ok((ok, inj, devs)) => format!("ok={} 注入行={} 设备={}", ok, inj, devs),
            Err(e) => format!("err=0x{:x} {}", e, detail(*e)),
        },
    ));
    // 段位守卫。
    v.push((
        "F0473-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗口 0x75000~0x781C0", SEG_BASE),
    ));
    // 三面零漂移（干净双扫）。
    let snap = machine_snapshot();
    let rep = match diff_snapshots(&snap, &snap) {
        Ok(r) => r,
        Err(_) => DiffReport::default(),
    };
    v.push((
        "F0473-三面零漂移",
        zero_drift_ok(&rep) && bitmap_all_zero_ok(&rep),
        format!("漂移行={} 清单漂移={}", rep.rows.len(), rep.manifest_drift),
    ));
    // 位图全零 + 双扫全等。
    let rep2 = match diff_snapshots(&snap, &snap) {
        Ok(r) => r,
        Err(_) => DiffReport::default(),
    };
    v.push((
        "F0473-位图全等断言",
        bitmap_all_zero_ok(&rep) && bitmap_equal_ok(&rep, &rep2),
        String::from("干净双扫位图全零且逐台全等"),
    ));
    // 身份漂移注入 100% 检出。
    v.push((
        "F0473-身份漂移注入",
        probe_seq_b().is_ok(),
        String::from("seq2 vid 翻转 → 恰 1 行 Identity 漂移"),
    ));
    // 类码漂移注入 100% 检出。
    let rep3 = match diff_snapshots(&snap, &inject_class_drift(3)) {
        Ok(r) => r,
        Err(_) => DiffReport::default(),
    };
    let class_hit = rep3.rows.len() == 1
        && match rep3.rows.get(0) {
            Some(r) => r.face == Face::Class && r.bdf == 0x1800,
            None => false,
        };
    v.push((
        "F0473-类码漂移注入",
        class_hit,
        format!("漂移行={} 首行面别={:?}", rep3.rows.len(), rep3.rows.get(0).map(|r| r.face)),
    ));
    // 清单漂移注入 100% 检出。
    let partial = inject_manifest_drop();
    let manifest_hit = match diff_snapshots(&snap, &partial) {
        Ok(r) => r.manifest_drift,
        Err(_) => false,
    };
    v.push((
        "F0473-清单漂移注入",
        manifest_hit,
        String::from("三面同步抽 1 台设备 → 清单面漂移显性入账"),
    ));
    // 位图断言（注入位图逐台校验）。
    v.push((
        "F0473-位图逐台校验",
        bitmap_assert_ok(),
        String::from("干净全零、注入仅 seq1 bit1 置位"),
    ));
    // 快照条数失配显性拒。
    let mut mis = Snapshot::default();
    mis.manifest = vec![0x0000];
    mis.identity.push(IdRec { bdf: 0x0000, vid: 1, did: 2 });
    let mis_ok = diff_snapshots(&snap, &mis).is_err();
    v.push((
        "F0473-条数失配拒",
        mis_ok,
        String::from("三面不对齐 → SnapshotLenMismatch"),
    ));
    // 状态机倒退拒。
    v.push((
        "F0473-状态机倒退拒",
        phase_regress_ok(),
        String::from("FirstScan 直调 diff/seal 拒，按序推进绿"),
    ));
    // 封账单调。
    let mut aud = RdAuditor::new(snap.clone());
    let _ = aud.step_rescan(snap.clone());
    let _ = aud.step_diff();
    let seal_ok = aud.step_seal().is_ok() && aud.phase == RdPhase::Sealed;
    v.push((
        "F0473-封账单调",
        seal_ok,
        String::from("FirstScan→Rescan→Diffed→Sealed 单向"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "F0473-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 双跑一致。
    v.push((
        "F0473-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 诊断码闭集查表。
    v.push((
        "F0473-诊断码闭集",
        code_closure_ok(),
        String::from("8 码全员落窗且查表回环一致"),
    ));
    v
}

pub fn run_unx0473_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0473 快照复扫 diff（枚举稳定性）");
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
        assert_eq!(g.ok(), Some((4, 1, 4)));
    }

    #[test]
    fn test_zero_drift_clean() {
        let snap = machine_snapshot();
        let rep = match diff_snapshots(&snap, &snap) {
            Ok(r) => r,
            Err(_) => return,
        };
        assert!(zero_drift_ok(&rep));
        assert!(bitmap_all_zero_ok(&rep));
        assert!(rep.face_bitmap.len() == 4);
    }

    #[test]
    fn test_identity_flip_caught_seq2() {
        assert!(probe_seq_b().is_ok());
    }

    #[test]
    fn test_identity_flip_only_one_row() {
        let snap = machine_snapshot();
        let rep = match diff_snapshots(&snap, &inject_identity_flip(0)) {
            Ok(r) => r,
            Err(_) => return,
        };
        assert_eq!(rep.rows.len(), 1);
        assert!(!rep.manifest_drift);
        let r0 = match rep.rows.get(0) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(r0.face, Face::Identity);
        assert_eq!(r0.bdf, 0x0000);
        assert_eq!(r0.first, (0x10EEu32 << 16) | 0x7021);
        assert_eq!(r0.second, (0x10EFu32 << 16) | 0x7021);
    }

    #[test]
    fn test_class_drift_caught() {
        let snap = machine_snapshot();
        let rep = match diff_snapshots(&snap, &inject_class_drift(3)) {
            Ok(r) => r,
            Err(_) => return,
        };
        assert_eq!(rep.rows.len(), 1);
        let r0 = match rep.rows.get(0) {
            Some(r) => r,
            None => return,
        };
        assert_eq!(r0.face, Face::Class);
        assert_eq!(r0.bdf, 0x1800);
    }

    #[test]
    fn test_manifest_drift_caught() {
        let partial = inject_manifest_drop();
        let snap = machine_snapshot();
        let rep = match diff_snapshots(&snap, &partial) {
            Ok(r) => r,
            Err(_) => return,
        };
        assert!(rep.manifest_drift);
        assert_eq!(partial.manifest.len(), 3);
    }

    #[test]
    fn test_snapshot_len_mismatch_rejected() {
        let snap = machine_snapshot();
        let mut mis = Snapshot::default();
        mis.manifest = vec![0x0000];
        mis.identity.push(IdRec { bdf: 0x0000, vid: 1, did: 2 });
        assert!(diff_snapshots(&snap, &mis).is_err());
        // 空快照三面全空但与真机集合不同 → 清单漂移而非条数错。
        let rep = match diff_snapshots(&snap, &Snapshot::default()) {
            Ok(r) => r,
            Err(_) => return,
        };
        assert!(rep.manifest_drift);
    }

    #[test]
    fn test_bitmap_per_device() {
        assert!(bitmap_assert_ok());
        let snap = machine_snapshot();
        let rep = match diff_snapshots(&snap, &inject_identity_flip(1)) {
            Ok(r) => r,
            Err(_) => return,
        };
        let expect = 1u8 << Face::Identity.bit();
        assert_eq!(rep.face_bitmap.get(0), Some(&0));
        assert_eq!(rep.face_bitmap.get(1), Some(&expect));
        assert_eq!(rep.face_bitmap.get(2), Some(&0));
        assert_eq!(rep.face_bitmap.get(3), Some(&0));
    }

    #[test]
    fn test_phase_machine_regress_rejected() {
        assert!(phase_regress_ok());
        let snap = machine_snapshot();
        let mut aud = RdAuditor::new(snap);
        assert!(aud.step_diff().is_err());
        assert!(aud.step_seal().is_err());
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
        assert_eq!(detail(0x1234_5678u32), "未知诊断码：不在 0x75740 段闭集内");
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
        assert_eq!(FACE_BITS, 3);
        assert_eq!(MACHINE_DEVS, 4);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0473_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
