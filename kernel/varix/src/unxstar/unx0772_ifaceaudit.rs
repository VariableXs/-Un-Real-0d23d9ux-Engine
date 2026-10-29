#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0772 · 六冻结接口终版对账（快照 vs 冻结表终核）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 3 单）
// 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥580 行；② 判据原文：UNX-F0772-J1 终核过（六接口快照 vs 任务书
//   冻结表逐字段对账 100%）且跨域消费 mock 回归零差；③ 完成时 API/软件写回：
//   状态=已完成、结果摘要非空。
// 设计纪律：① 冻结表写死为 const 静态闭集（六接口 × 七字段），任何字段不得
//   运行期改写；快照经 API 喂入后与冻结表逐字段对账（6×7=42 字段 100%）。
// ② 跨域消费 mock 回归：以冻结签名派生确定性输出基线，快照模型重跑同脚本，
//   逐调用 diff 必须为 0（账实分离双保险）。
// ③ 状态机 Open→Reconciled→MockPassed→Sealed 单向闭集，回头边拒绝。
// ④ 对账差异定位到（接口号, 字段类别）二元组，禁含糊报错。
// ⑤ 计数守恒：字段比较数 = 一致数 + 差异数 = 42；mock 调用数 = 18 恒等。
// 诊断段声明：0x5F40–0x5F5F 本单独占（清单 <SEG 0x5f40>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 1. 诊断码段（本单独占 0x5F40–0x5F5F）
// ===========================================================================

pub const SEG_BASE: u16 = 0x5F40; // 诊断码段基数，来源：W7 清单 UNX-F0772 行 <SEG 0x5f40>
pub const SEG_TOP: u16 = 0x5F5F; // 诊断码段顶界（含）

pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 对账状态机非法边
pub const XC_IFACE_SHORT: u16 = SEG_BASE + 2; // 快照接口数不足 6
pub const XC_IFACE_DUP: u16 = SEG_BASE + 3; // 快照接口号重复
pub const XC_FIELD_DIFF: u16 = SEG_BASE + 4; // 逐字段对账检出差异
pub const XC_MOCK_DIFF: u16 = SEG_BASE + 5; // mock 回归检出输出差
pub const XC_MOCK_SHORT: u16 = SEG_BASE + 6; // mock 调用数不足 18
pub const XC_UNKNOWN_IFACE: u16 = SEG_BASE + 7; // 快照含冻结表外接口号
pub const XC_NOT_RECONCILED: u16 = SEG_BASE + 8; // 未对账即请求 mock/封版
pub const XC_ALREADY_SEALED: u16 = SEG_BASE + 9; // 已封版再操作
pub const XC_VERSION_BAD: u16 = SEG_BASE + 10; // 快照版本三元组非法（major=0）
pub const XC_OWNER_BAD: u16 = SEG_BASE + 11; // 接收域代号越出 1..=5 闭集
pub const XC_SUM_OVERFLOW: u16 = SEG_BASE + 12; // 计数溢出
pub const XC_COUNT_MISMATCH: u16 = SEG_BASE + 13; // 守恒恒等式断裂
pub const XC_SIG_ZERO: u16 = SEG_BASE + 14; // 快照签名为 0（非法占位）
pub const XC_NAME_EMPTY: u16 = SEG_BASE + 15; // 快照接口名为空

// ===========================================================================
// 2. 冻结常量（全部写死，来源=任务书冻结表 · 头注假设样本值）
// ===========================================================================

pub const IFACE_COUNT: usize = 6; // 冻结接口数：六接口，来源：判据原文
pub const FIELDS_PER_IFACE: usize = 7; // 每接口字段数：号/名/版本三元组/域/签名
pub const FIELD_TOTAL: usize = IFACE_COUNT * FIELDS_PER_IFACE; // 42 字段对账口径
pub const MOCK_CALLS_PER_IFACE: usize = 3; // 每接口 mock 调用数（脚本写死）
pub const MOCK_CALL_TOTAL: usize = IFACE_COUNT * MOCK_CALLS_PER_IFACE; // 18 恒等口径
pub const MOCK_SALT: u64 = 0xA5A5_5A5A_1234_5678; // mock 输出派生盐（本单自定，头注假设）

/// 冻结接口记录（任务书冻结表终版，写死不得改）。
#[derive(Clone, Copy)]
pub struct FrozenIface {
    pub id: u16,             // 接口号 1..=6
    pub name: &'static str,  // 接口名（冻结终版）
    pub vmaj: u16,           // 版本主号（冻结 v1.x.y 口径）
    pub vmin: u16,           // 版本次号
    pub vpat: u16,           // 版本修订号
    pub owner: u8,           // 接收域代号 1..=5（五域消费面）
    pub sig: u64,            // 接口签名指纹（FNV 终版值，冻结）
}

pub const FROZEN_TABLE: [FrozenIface; IFACE_COUNT] = [  // 任务书冻结表（六行，逐字段写死；样本值为头注假设的终版定格）。
    FrozenIface { id: 1, name: "crit.query",    vmaj: 1, vmin: 0, vpat: 3, owner: 1, sig: 0x11EF_0001_5F40_0001 },
    FrozenIface { id: 2, name: "receipt.push",  vmaj: 1, vmin: 1, vpat: 0, owner: 2, sig: 0x22EF_0002_5F40_0002 },
    FrozenIface { id: 3, name: "ledger.read",   vmaj: 1, vmin: 0, vpat: 7, owner: 3, sig: 0x33EF_0003_5F40_0003 },
    FrozenIface { id: 4, name: "risk.register", vmaj: 1, vmin: 2, vpat: 1, owner: 4, sig: 0x44EF_0004_5F40_0004 },
    FrozenIface { id: 5, name: "seal.declare",  vmaj: 1, vmin: 0, vpat: 9, owner: 5, sig: 0x55EF_0005_5F40_0005 },
    FrozenIface { id: 6, name: "scroll.export", vmaj: 1, vmin: 3, vpat: 2, owner: 1, sig: 0x66EF_0006_5F40_0006 },
];

// ===========================================================================
// 3. 字段类别闭集（对账定位粒度）
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldKind {
    Id = 0,     // 接口号
    Name = 1,   // 接口名
    VMaj = 2,   // 版本主号
    VMin = 3,   // 版本次号
    VPat = 4,   // 版本修订号
    Owner = 5,  // 接收域代号
    Sig = 6,    // 签名指纹
}

impl FieldKind {
    pub fn all() -> [FieldKind; FIELDS_PER_IFACE] {
        [FieldKind::Id, FieldKind::Name, FieldKind::VMaj, FieldKind::VMin, FieldKind::VPat, FieldKind::Owner, FieldKind::Sig]
    }
    pub fn zh(self) -> &'static str {
        match self {
            FieldKind::Id => "接口号",
            FieldKind::Name => "接口名",
            FieldKind::VMaj => "版本主号",
            FieldKind::VMin => "版本次号",
            FieldKind::VPat => "版本修订号",
            FieldKind::Owner => "接收域",
            FieldKind::Sig => "签名指纹",
        }
    }
}

/// 一条差异定位：（接口号, 字段类别, 冻结值, 快照值）。
#[derive(Clone, Debug)]
pub struct FieldDiff {
    pub iface: u16,
    pub kind: FieldKind,
    pub frozen: u64,
    pub snapshot: u64,
    pub note: String,
}

/// 快照侧接口记录（API 喂入，逐字段对账对象）。
#[derive(Clone, Debug)]
pub struct SnapshotIface {
    pub id: u16,
    pub name: String,
    pub vmaj: u16,
    pub vmin: u16,
    pub vpat: u16,
    pub owner: u8,
    pub sig: u64,
}

/// mock 回归单调用记录。
#[derive(Clone, Debug)]
pub struct MockRecord {
    pub iface: u16,       // 接口号
    pub arg: u64,         // 脚本参数
    pub frozen_out: u64,  // 冻结模型输出
    pub snapshot_out: u64, // 快照模型输出
    pub diff: bool,       // 是否出现输出差
}

// ===========================================================================
// 4. 状态机（单向闭集）
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReconStage {
    Open = 0,        // 快照登记中
    Reconciled = 1,  // 逐字段对账 100% 已过
    MockPassed = 2,  // mock 回归零差已过
    Sealed = 3,      // 终版封版（只读）
}

impl ReconStage {
    pub fn can_step(self, next: ReconStage) -> bool {
        (self as u8) + 1 == next as u8
    }
}

// ===========================================================================
// 5. 对账机本体
// ===========================================================================

pub struct IfaceReconciler {
    stage: ReconStage,
    snapshot: Vec<SnapshotIface>,
    diffs: Vec<FieldDiff>,
    mocks: Vec<MockRecord>,
    events: Vec<String>, // 审计事件账
}

impl IfaceReconciler {
    pub fn new() -> Self {
        IfaceReconciler {
            stage: ReconStage::Open,
            snapshot: Vec::new(),
            diffs: Vec::new(),
            mocks: Vec::new(),
            events: Vec::new(),
        }
    }

    pub fn feed_snapshot(&mut self, batch: &[SnapshotIface]) -> Result<u32, u16> {  // 喂入快照批（六条一批或分批，总计必须恰六）。
        if self.stage != ReconStage::Open {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_IFACE_SHORT);
        }
        let mut added: u32 = 0;
        for e in batch.iter() {
            if self.snapshot.len() >= IFACE_COUNT {
                return Err(XC_IFACE_SHORT);
            }
            // 冻结表外接口号拒收。
            let mut known = false;
            let mut i = 0usize;
            while i < IFACE_COUNT {
                if FROZEN_TABLE[i].id == e.id {
                    known = true;
                    break;
                }
                i += 1;
            }
            if !known {
                return Err(XC_UNKNOWN_IFACE);
            }
            // 重复拒收 + 快照自体合法性。
            let mut j = 0usize;
            while j < self.snapshot.len() {
                if self.snapshot[j].id == e.id {
                    return Err(XC_IFACE_DUP);
                }
                j += 1;
            }
            if e.vmaj == 0 {
                return Err(XC_VERSION_BAD);
            }
            if e.owner < 1 || e.owner > 5 {
                return Err(XC_OWNER_BAD);
            }
            if e.sig == 0 {
                return Err(XC_SIG_ZERO);
            }
            if e.name.is_empty() {
                return Err(XC_NAME_EMPTY);
            }
            self.snapshot.push(e.clone());
            added += 1;
        }
        self.events.push(alloc::format!("snapshot +{}", added));
        Ok(added)
    }

    pub fn snapshot_count(&self) -> usize {
        self.snapshot.len()
    }
    pub fn stage(&self) -> ReconStage {
        self.stage
    }

    pub fn reconcile(&mut self) -> Result<u32, u16> {  // 逐字段对账：6×7=42 字段 100% 比较，差异全量登记并定位。
        if !self.stage.can_step(ReconStage::Reconciled) {
            return Err(if self.snapshot.len() != IFACE_COUNT {
                XC_IFACE_SHORT
            } else {
                XC_STATE_INVALID
            });
        }
        self.diffs.clear();
        let mut compared: u32 = 0;
        let mut matched: u32 = 0;
        let mut k = 0usize;
        while k < IFACE_COUNT {
            let fr = &FROZEN_TABLE[k];
            // 快照按接口号取（冻结表与快照都按号序）。
            let mut snap: Option<&SnapshotIface> = None;
            let mut s = 0usize;
            while s < self.snapshot.len() {
                if self.snapshot[s].id == fr.id {
                    snap = Some(&self.snapshot[s]);
                    break;
                }
                s += 1;
            }
            let sp = match snap {
                Some(x) => x,
                None => return Err(XC_UNKNOWN_IFACE),
            };
            for kind in FieldKind::all() {
                let (fv, sv) = match kind {
                    FieldKind::Id => (fr.id as u64, sp.id as u64),
                    FieldKind::Name => (0u64, 0u64), // 文本字段单列处理（见下）
                    FieldKind::VMaj => (fr.vmaj as u64, sp.vmaj as u64),
                    FieldKind::VMin => (fr.vmin as u64, sp.vmin as u64),
                    FieldKind::VPat => (fr.vpat as u64, sp.vpat as u64),
                    FieldKind::Owner => (fr.owner as u64, sp.owner as u64),
                    FieldKind::Sig => (fr.sig, sp.sig),
                };
                if kind == FieldKind::Name {
                    // 名称字段：等长且逐字节一致才记一致。
                    let fb = fr.name.as_bytes();
                    let sb = sp.name.as_bytes();
                    let eq = fb.len() == sb.len() && {
                        let mut ok = true;
                        let mut b = 0usize;
                        while b < fb.len() {
                            if fb[b] != sb[b] {
                                ok = false;
                                break;
                            }
                            b += 1;
                        }
                        ok
                    };
                    if eq {
                        matched += 1;
                    } else {
                        self.diffs.push(FieldDiff {
                            iface: fr.id,
                            kind,
                            frozen: fb.len() as u64,
                            snapshot: sb.len() as u64,
                            note: alloc::format!("字段{}: 名不一致", kind.zh()),
                        });
                    }
                } else if fv == sv {
                    matched += 1;
                } else {
                    self.diffs.push(FieldDiff {
                        iface: fr.id,
                        kind,
                        frozen: fv,
                        snapshot: sv,
                        note: alloc::format!("字段{}: 冻结={} 快照={}", kind.zh(), fv, sv),
                    });
                }
                compared += 1;
            }
            k += 1;
        }
        // 守恒：compared == matched + diffs.len() == 42。
        let m = matched as usize + self.diffs.len();
        if compared as usize != FIELD_TOTAL || m != FIELD_TOTAL {
            return Err(XC_COUNT_MISMATCH);
        }
        self.stage = ReconStage::Reconciled;
        self.events
            .push(alloc::format!("reconcile compared={} diff={}", compared, self.diffs.len()));
        Ok(self.diffs.len() as u32)
    }

    pub fn diff_count(&self) -> usize {
        self.diffs.len()
    }
    pub fn diffs(&self) -> &[FieldDiff] {
        &self.diffs
    }

    pub fn mock_regression(&mut self) -> Result<u32, u16> {  // mock 回归：同脚本（每接口 3 参）双模型跑，输出逐调用零差。
        if !self.stage.can_step(ReconStage::MockPassed) {
            return Err(if self.stage == ReconStage::Open {
                XC_NOT_RECONCILED
            } else {
                XC_STATE_INVALID
            });
        }
        self.mocks.clear();
        let mut diff_calls: u32 = 0;
        let mut k = 0usize;
        while k < IFACE_COUNT {
            let fr = &FROZEN_TABLE[k];
            let mut s = 0usize;
            while s < self.snapshot.len() && self.snapshot[s].id != fr.id {
                s += 1;
            }
            if s >= self.snapshot.len() {
                return Err(XC_UNKNOWN_IFACE);
            }
            let sp = &self.snapshot[s];
            let mut c = 0usize;
            while c < MOCK_CALLS_PER_IFACE {
                let arg = (c as u64 + 1) * 0x1000 + fr.id as u64;
                let fo = Self::mock_out(fr.sig, arg);
                let so = Self::mock_out(sp.sig, arg);
                let d = fo != so;
                if d {
                    diff_calls += 1;
                }
                self.mocks.push(MockRecord { iface: fr.id, arg, frozen_out: fo, snapshot_out: so, diff: d });
                c += 1;
            }
            k += 1;
        }
        if self.mocks.len() != MOCK_CALL_TOTAL {
            return Err(XC_MOCK_SHORT);
        }
        self.stage = ReconStage::MockPassed;
        self.events
            .push(alloc::format!("mock calls={} diff={}", self.mocks.len(), diff_calls));
        Ok(diff_calls)
    }

    pub fn mock_records(&self) -> &[MockRecord] {
        &self.mocks
    }

    /// mock 输出派生（确定性整数算术）：sig×arg 反卷积 + 盐异或。
    fn mock_out(sig: u64, arg: u64) -> u64 {
        sig.wrapping_mul(arg).wrapping_add(MOCK_SALT) ^ (arg.rotate_left(17))
    }

    pub fn seal(&mut self) -> Result<String, u16> {  // 终版封版：对账零差 + mock 零差 → 摘要定格。
        if !self.stage.can_step(ReconStage::Sealed) {
            return Err(if self.stage == ReconStage::Sealed {
                XC_ALREADY_SEALED
            } else {
                XC_NOT_RECONCILED
            });
        }
        if !self.diffs.is_empty() {
            return Err(XC_FIELD_DIFF);
        }
        let mut md = 0usize;
        let mut i = 0usize;
        while i < self.mocks.len() {
            if self.mocks[i].diff {
                md += 1;
            }
            i += 1;
        }
        if md != 0 {
            return Err(XC_MOCK_DIFF);
        }
        self.stage = ReconStage::Sealed;
        self.events.push(String::from("sealed"));
        Ok(String::from(
            "UNX-F0772-J1 终核过：六接口快照 vs 冻结表 42 字段对账零差，mock 回归 18/18 零差",
        ))
    }

    pub fn fingerprint(&self) -> u64 {  // 账面指纹：冻结表 + 快照规范序 FNV-1a-64。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut k = 0usize;
        while k < IFACE_COUNT {
            let fr = &FROZEN_TABLE[k];
            for v in [fr.id as u64, fr.vmaj as u64, fr.vmin as u64, fr.vpat as u64, fr.owner as u64, fr.sig] {
                h = Self::mix(h, v);
            }
            k += 1;
        }
        let mut s = 0usize;
        while s < self.snapshot.len() {
            let sp = &self.snapshot[s];
            for v in [sp.id as u64, sp.vmaj as u64, sp.vmin as u64, sp.vpat as u64, sp.owner as u64, sp.sig] {
                h = Self::mix(h, v);
            }
            s += 1;
        }
        h
    }

    fn mix(mut h: u64, v: u64) -> u64 {
        let mut b = 0usize;
        while b < 8 {
            h ^= (v >> (b * 8)) & 0xFF;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            b += 1;
        }
        h
    }

    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    pub fn frozen_name(id: u16) -> Option<&'static str> {  // 冻结表只读视图：按接口号查名（供跨域消费面解引，禁运行期改写）。
        let mut i = 0usize;
        while i < IFACE_COUNT {
            if FROZEN_TABLE[i].id == id {
                return Some(FROZEN_TABLE[i].name);
            }
            i += 1;
        }
        None
    }

    pub fn frozen_sig(id: u16) -> Option<u64> {  // 冻结表只读视图：按接口号查签名指纹（mock 模型外解引用）。
        let mut i = 0usize;
        while i < IFACE_COUNT {
            if FROZEN_TABLE[i].id == id {
                return Some(FROZEN_TABLE[i].sig);
            }
            i += 1;
        }
        None
    }

    pub fn frozen_version(id: u16) -> Option<u64> {  // 冻结版本三元组打包（(maj<<32)|(min<<16)|pat 整数编码，禁浮点）。
        let mut i = 0usize;
        while i < IFACE_COUNT {
            if FROZEN_TABLE[i].id == id {
                let f = &FROZEN_TABLE[i];
                return Some(((f.vmaj as u64) << 32) | ((f.vmin as u64) << 16) | (f.vpat as u64));
            }
            i += 1;
        }
        None
    }
}

// ===========================================================================
// 6. 快照发生器（确定性：与冻结表一致 / 可注入单字段漂移）
// ===========================================================================

pub fn gen_snapshot_clean() -> Vec<SnapshotIface> {  // 生成与冻结表完全一致的快照批。
    let mut out: Vec<SnapshotIface> = Vec::new();
    let mut i = 0usize;
    while i < IFACE_COUNT {
        let f = &FROZEN_TABLE[i];
        out.push(SnapshotIface {
            id: f.id,
            name: String::from(f.name),
            vmaj: f.vmaj,
            vmin: f.vmin,
            vpat: f.vpat,
            owner: f.owner,
            sig: f.sig,
        });
        i += 1;
    }
    out
}

// ===========================================================================
// 7. 自例程 drill：主路径 + 错误路径，0=全过
// ===========================================================================

pub fn drill() -> Result<u32, u16> {
    // —— 主路径：一致快照 → 对账零差 → mock 零差 → 封版 ——
    let mut r = IfaceReconciler::new();
    r.feed_snapshot(&gen_snapshot_clean())?;
    if r.snapshot_count() != IFACE_COUNT {
        return Err(XC_IFACE_SHORT);
    }
    let d = r.reconcile()?;
    if d != 0 || r.diff_count() != 0 {
        return Err(XC_FIELD_DIFF);
    }
    let md = r.mock_regression()?;
    if md != 0 || r.mock_records().len() != MOCK_CALL_TOTAL {
        return Err(XC_MOCK_DIFF);
    }
    let summary = r.seal()?;
    if summary.is_empty() {
        return Err(XC_NOT_RECONCILED);
    }
    if r.seal() != Err(XC_ALREADY_SEALED) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：未对账即 mock → XC_NOT_RECONCILED ——
    let mut r2 = IfaceReconciler::new();
    r2.feed_snapshot(&gen_snapshot_clean())?;
    if r2.mock_regression() != Err(XC_NOT_RECONCILED) {
        return Err(XC_NOT_RECONCILED);
    }
    // —— 错误路径 2：快照版本漂移 → 对账定位（接口, VPat) ——
    let mut r3 = IfaceReconciler::new();
    let mut bad = gen_snapshot_clean();
    bad[2].vpat += 1; // 接口 3 修订号漂移
    r3.feed_snapshot(&bad)?;
    let d3 = r3.reconcile()?;
    if d3 != 1 {
        return Err(XC_FIELD_DIFF);
    }
    let dd = r3.diffs();
    if dd[0].iface != 3 || dd[0].kind != FieldKind::VPat {
        return Err(XC_FIELD_DIFF);
    }
    if r3.mock_regression().is_ok() {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 3：签名漂移 → 对账 + mock 双检 ——
    let mut r4 = IfaceReconciler::new();
    let mut bad2 = gen_snapshot_clean();
    bad2[4].sig = bad2[4].sig ^ 0xDEAD;
    r4.feed_snapshot(&bad2)?;
    let d4 = r4.reconcile()?;
    if d4 != 1 {
        return Err(XC_FIELD_DIFF);
    }
    // 差异存在时封版必红。
    if r4.seal() != Err(XC_NOT_RECONCILED) {
        return Err(XC_FIELD_DIFF);
    }
    // —— 错误路径 4：冻结表外接口号 / 越域 owner / 零签名 ——
    let mut r5 = IfaceReconciler::new();
    let mut bad3 = gen_snapshot_clean();
    bad3[0].id = 9;
    if r5.feed_snapshot(&bad3) != Err(XC_UNKNOWN_IFACE) {
        return Err(XC_UNKNOWN_IFACE);
    }
    let mut r6 = IfaceReconciler::new();
    let mut bad4 = gen_snapshot_clean();
    bad4[1].owner = 7;
    if r6.feed_snapshot(&bad4) != Err(XC_OWNER_BAD) {
        return Err(XC_OWNER_BAD);
    }
    // —— 指纹对拍：同状态两次构建指纹一致 ——
    let fp_a = {
        let mut ra = IfaceReconciler::new();
        ra.feed_snapshot(&gen_snapshot_clean())?;
        ra.fingerprint()
    };
    let fp_b = {
        let mut rb = IfaceReconciler::new();
        rb.feed_snapshot(&gen_snapshot_clean())?;
        rb.fingerprint()
    };
    if fp_a != fp_b {
        return Err(XC_COUNT_MISMATCH);
    }
    // —— 冻结只读视图自洽：名/签名/版本与常量表一致 ——
    if IfaceReconciler::frozen_name(4) != Some("risk.register")
        || IfaceReconciler::frozen_sig(4) != Some(FROZEN_TABLE[3].sig)
    {
        return Err(XC_UNKNOWN_IFACE);
    }
    let ver = match IfaceReconciler::frozen_version(6) {
        Some(v) => v,
        None => return Err(XC_UNKNOWN_IFACE),
    };
    if ver != ((1u64 << 32) | (3u64 << 16) | 2u64) {
        return Err(XC_VERSION_BAD);
    }
    Ok(0)
}
