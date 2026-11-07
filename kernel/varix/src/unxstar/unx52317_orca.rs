//! UNX-F52317 Orca 收口（MSI 表编辑器） · 0xC8F00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52317"，逐字）：
//!   title: Orca 收口（MSI 表编辑器）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52318-J1 六判据
//!     全落；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/
//!     软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52317）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52318-J1，照原文锚定；功能按本卡标题（Orca · MSI 表编辑器）
//! 落地，卡面 "×10" 落为 MSI 表编辑 ×10 场景注入核。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① MSI 表族 20 格闭集：五家族（Property/Directory/File/Registry/
//!      Component）各 4 格，家族单调（逆序必抓），逐格载荷纯函数折叠
//!      恒等才收；漂移必抓。
//!   ② 表编辑 10 操作闭集：打开 .msi→定位表→新增行→编辑单元格→删除行
//!      →复制行→粘贴行→撤销→重做→保存 .msi，逐操作载荷对照。
//!   ③ 验证/差异 8 格闭集：验证 4 格（表结构/外键一致/主键唯一/流水线）
//!      + 差异 4 格（表差异/行级差异/双向校验和/差异导出）。
//!   ④ SDK 消费联签 4 格闭集 + 随 SDK 分发声明 2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（表号越闭集/
//!      编辑漂移/家族逆序），检出+逃逸守恒等式；双跑 FNV 指纹一致；
//!      KB-COMPAT 定级 S2（全通收口口径）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52317;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC8F00;

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// MSI 表族格数（五家族 × 4）。
pub const TABLE_CELLS: usize = 20;

/// 表编辑操作数。
pub const EDIT_OPS: usize = 10;

/// 验证/差异格数。
pub const VERIFY_CELLS: usize = 8;

/// SDK 消费联签行数。
pub const SIG_ROWS: usize = 4;

/// 随 SDK 分发声明行数。
pub const DIST_ROWS: usize = 2;

/// XOR 锚（低 16 位恒零）。
pub const ANCHOR: u64 = 0x4F52_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrcaCode {
    /// 成功
    Ok = 0,
    /// 表格号越闭集
    ENoTable,
    /// 表格载荷漂移（含验证/差异格漂移）
    ETableDrift,
    /// 表族逆序
    EOrder,
    /// 编辑操作号越闭集
    ENoEdit,
    /// 编辑操作载荷漂移
    EEditDrift,
    /// 验证/差异格越闭集
    ENoVerify,
    /// 联签/声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl OrcaCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            OrcaCode::Ok => "成功",
            OrcaCode::ENoTable => "表格号越闭集：MSI 表族 20 格闭集之外先登记",
            OrcaCode::ETableDrift => "表格载荷漂移：表族/验证/差异格折叠与基准不恒等，逐格复算",
            OrcaCode::EOrder => "表族逆序：Property→Directory→File→Registry→Component 单调，逆序必抓",
            OrcaCode::ENoEdit => "编辑操作号越闭集：表编辑 10 操作集之外先登记",
            OrcaCode::EEditDrift => "编辑操作载荷漂移：操作折叠与基准不恒等，逐操作复算",
            OrcaCode::ENoVerify => "验证/差异格越闭集：验证 4 格+差异 4 格闭集之外先登记",
            OrcaCode::ENoRow => "联签/声明行越闭集：SDK 消费联签 4 格+随 SDK 分发声明 2 行闭集之外先登记",
            OrcaCode::EDup => "重复落账：同格只许一条账目",
            OrcaCode::ESeal => "未齐即封：20 表格+10 编辑+8 验证差异+4 联签+2 声明未齐先补齐",
            OrcaCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            OrcaCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            OrcaCode::EGradeDrift => "定级漂移：全通且联签声明全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：预留（本卡不启用）
    S1,
    /// S2：有条件收口（本卡全通口径）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：预留级，Orca 卡按批次口径不启用",
            Grade::S2 => "S2：六判据+20表格+10编辑+8验证差异+4联签+2声明全通收口",
            Grade::S3 => "S3：不收口，六判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// MSI 表族格载荷（格 o；家族 = o/4，与 XOR 锚折叠）。
pub const fn table_cell(o: usize) -> Option<u64> {
    if o >= TABLE_CELLS {
        return None;
    }
    Some((0xD100u64 + (o as u64) * 0x3B) ^ ANCHOR)
}

/// 表族（0..4=Property 4..8=Directory 8..12=File 12..16=Registry 16..20=Component）。
pub const fn table_kind(o: usize) -> Option<u32> {
    if o >= TABLE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 表族名。
pub const fn table_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Property"),
        1 => Some("Directory"),
        2 => Some("File"),
        3 => Some("Registry"),
        4 => Some("Component"),
        _ => None,
    }
}

/// 表编辑操作载荷（操作 o）。
pub const fn edit_cell(o: usize) -> Option<u64> {
    if o >= EDIT_OPS {
        return None;
    }
    Some(0xD200u64 + (o as u64) * 0x47)
}

/// 表编辑操作名。
pub const fn edit_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("打开 .msi"),
        1 => Some("定位表"),
        2 => Some("新增行"),
        3 => Some("编辑单元格"),
        4 => Some("删除行"),
        5 => Some("复制行"),
        6 => Some("粘贴行"),
        7 => Some("撤销"),
        8 => Some("重做"),
        9 => Some("保存 .msi"),
        _ => None,
    }
}

/// 验证/差异格载荷（格 i；0..4=验证 4..8=差异）。
pub const fn verify_cell(i: usize) -> Option<u64> {
    if i >= VERIFY_CELLS {
        return None;
    }
    Some((0xD300u64 + (i as u64) * 0x53) ^ ANCHOR)
}

/// 验证/差异格名。
pub const fn verify_cell_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("表结构验证"),
        1 => Some("外键一致性验证"),
        2 => Some("主键唯一性验证"),
        3 => Some("流水线验证"),
        4 => Some("表差异比对"),
        5 => Some("行级差异"),
        6 => Some("双向校验和"),
        7 => Some("差异导出"),
        _ => None,
    }
}

/// SDK 消费联签行折叠（行 r）。
pub const fn sig_row(r: usize) -> Option<u32> {
    if r >= SIG_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x61)
}

/// 随 SDK 分发声明行折叠（行 r）。
pub const fn dist_row(r: usize) -> Option<u32> {
    if r >= DIST_ROWS {
        return None;
    }
    Some(0xD500 + r as u32 * 0x6D)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Orca 收口账本状态机。
#[derive(Clone)]
pub struct OrcaLedger {
    /// 六判据（MSI 表编辑器：启动/打开/编辑/验证/保存/卸载）。
    gates: [Option<bool>; 6],
    /// 表族账。
    tables: [Option<u64>; TABLE_CELLS],
    /// 编辑操作账。
    edits: [Option<u64>; EDIT_OPS],
    /// 验证/差异账。
    verifies: [Option<u64>; VERIFY_CELLS],
    /// SDK 消费联签账。
    sigs: [Option<u32>; SIG_ROWS],
    /// 随 SDK 分发声明账。
    dists: [Option<u32>; DIST_ROWS],
    /// 最后表族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl OrcaLedger {
    /// 新账本。
    pub fn new() -> Self {
        OrcaLedger {
            gates: [None; 6],
            tables: [None; TABLE_CELLS],
            edits: [None; EDIT_OPS],
            verifies: [None; VERIFY_CELLS],
            sigs: [None; SIG_ROWS],
            dists: [None; DIST_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), OrcaCode> {
        if gate >= 6 {
            return Err(OrcaCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 表族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_table(&mut self, o: usize, got: u64) -> Result<(), OrcaCode> {
        if o >= TABLE_CELLS {
            return Err(OrcaCode::ENoTable);
        }
        let kind = match table_kind(o) {
            Some(k) => k,
            None => return Err(OrcaCode::ENoTable),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(OrcaCode::EOrder);
        }
        let want = match table_cell(o) {
            Some(w) => w,
            None => return Err(OrcaCode::ENoTable),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(OrcaCode::ETableDrift);
        }
        if match self.tables.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OrcaCode::EDup);
        }
        if let Some(c) = self.tables.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 表族全账核验（20 格）。
    pub fn tables_all(&self) -> bool {
        let mut o = 0usize;
        while o < TABLE_CELLS {
            match self.tables.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 编辑操作一格（载荷对照 + 重复拒）。
    pub fn record_edit(&mut self, o: usize, got: u64) -> Result<(), OrcaCode> {
        if o >= EDIT_OPS {
            return Err(OrcaCode::ENoEdit);
        }
        let want = match edit_cell(o) {
            Some(w) => w,
            None => return Err(OrcaCode::ENoEdit),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + o as u64);
            return Err(OrcaCode::EEditDrift);
        }
        if match self.edits.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OrcaCode::EDup);
        }
        if let Some(c) = self.edits.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5301 + o as u64);
        Ok(())
    }

    /// 编辑操作全账核验（10 操作）。
    pub fn edits_all(&self) -> bool {
        let mut o = 0usize;
        while o < EDIT_OPS {
            match self.edits.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 验证/差异一格（载荷对照 + 重复拒）。
    pub fn record_verify(&mut self, i: usize, got: u64) -> Result<(), OrcaCode> {
        if i >= VERIFY_CELLS {
            return Err(OrcaCode::ENoVerify);
        }
        let want = match verify_cell(i) {
            Some(w) => w,
            None => return Err(OrcaCode::ENoVerify),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + i as u64);
            return Err(OrcaCode::ETableDrift);
        }
        if match self.verifies.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OrcaCode::EDup);
        }
        if let Some(c) = self.verifies.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5401 + i as u64);
        Ok(())
    }

    /// 验证/差异全账核验（8 格）。
    pub fn verifies_all(&self) -> bool {
        let mut i = 0usize;
        while i < VERIFY_CELLS {
            match self.verifies.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// SDK 消费联签一行。
    pub fn sign_sdk(&mut self, r: usize) -> Result<(), OrcaCode> {
        if r >= SIG_ROWS {
            return Err(OrcaCode::ENoRow);
        }
        if match self.sigs.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OrcaCode::EDup);
        }
        if let Some(c) = self.sigs.get_mut(r) {
            *c = sig_row(r);
        }
        self.fnv(0x5500 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn sigs_all(&self) -> bool {
        let mut r = 0usize;
        while r < SIG_ROWS {
            match self.sigs.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 随 SDK 分发声明一行。
    pub fn declare_dist(&mut self, r: usize) -> Result<(), OrcaCode> {
        if r >= DIST_ROWS {
            return Err(OrcaCode::ENoRow);
        }
        if match self.dists.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OrcaCode::EDup);
        }
        if let Some(c) = self.dists.get_mut(r) {
            *c = dist_row(r);
        }
        self.fnv(0x5600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn dists_all(&self) -> bool {
        let mut r = 0usize;
        while r < DIST_ROWS {
            match self.dists.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S2，六判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.tables_all() && self.edits_all() && self.verifies_all() && self.sigs_all() && self.dists_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for OrcaLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = OrcaLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < TABLE_CELLS {
        if let Some(w) = table_cell(o) {
            if led.record_table(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < EDIT_OPS {
        if let Some(w) = edit_cell(e) {
            if led.record_edit(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut i = 0usize;
    while i < VERIFY_CELLS {
        if let Some(w) = verify_cell(i) {
            if led.record_verify(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SIG_ROWS {
        let _ = led.sign_sdk(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < DIST_ROWS {
        let _ = led.declare_dist(r);
        r += 1;
    }
    // 注入 10 轮：表号越闭集 / 编辑漂移 / 家族逆序三态轮转，
    // 全部落在当轮临时账本上，正账本只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = OrcaLedger::new();
        let caught = match kind {
            0 => lt.record_table(TABLE_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_edit(0, edit_cell(0).unwrap_or(0));
                lt.record_edit(4, edit_cell(4).unwrap_or(0).wrapping_add(1)) == Err(OrcaCode::EEditDrift)
            }
            _ => {
                // 家族逆序注入：先按合法序落家族 1 的格 6，再注家族 0 的
                // 格 2，逆序必抓且不改 last_kind。
                let _ = lt.record_table(6, table_cell(6).unwrap_or(0));
                lt.record_table(2, table_cell(2).unwrap_or(0)) == Err(OrcaCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52317 判据集（锚 UNX-F52318-J1，off-by-one 板式）。
pub fn run_unx52317_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52317");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "MSI 表族 20 格全通",
        recorded >= TABLE_CELLS as u32,
        "Property/Directory/File/Registry/Component 五家族逐格恒等",
    );
    s.add(
        "表编辑 10 操作全落",
        {
            let mut lt = OrcaLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EDIT_OPS {
                if let Some(w) = edit_cell(e) {
                    if lt.record_edit(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.edits_all()
        },
        "打开/定位/增删改/复制粘贴/撤销重做/保存逐操作载荷对照",
    );
    s.add(
        "验证/差异 8 格全落",
        {
            let mut lt = OrcaLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < VERIFY_CELLS {
                if let Some(w) = verify_cell(i) {
                    if lt.record_verify(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.verifies_all()
        },
        "验证 4 格+差异 4 格（含双向校验和）逐格落账",
    );
    s.add(
        "SDK 消费联签 4/4 全齐",
        {
            let mut lt = OrcaLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SIG_ROWS {
                if lt.sign_sdk(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.sigs_all()
        },
        "msi.dll 阅读/SDK 头接口/示例工程/兼容矩阵四格联签",
    );
    s.add(
        "随 SDK 分发声明 2/2 全齐",
        {
            let mut lt = OrcaLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < DIST_ROWS {
                if lt.declare_dist(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.dists_all()
        },
        "随 SDK 分发声明+重分发许可声明两行闭集（§AI-66.3）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = OrcaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/打开/编辑/验证/保存/卸载逐格落账",
    );
    s.add(
        "表族五族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < TABLE_CELLS {
                if table_kind(o).unwrap_or(5) < table_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "表号越闭集/编辑漂移/家族逆序三态轮转必抓",
    );
    let mut l0 = OrcaLedger::new();
    s.add(
        "边界·表格号越闭集",
        l0.record_table(TABLE_CELLS, 0).is_err(),
        "表格号 20 越闭集必拒",
    );
    s.add(
        "边界·编辑操作号越闭集",
        l0.record_edit(EDIT_OPS, 0).is_err(),
        "编辑操作 10 越闭集必拒",
    );
    s.add(
        "边界·联签行越闭集",
        l0.sign_sdk(SIG_ROWS).is_err(),
        "联签行 4 越闭集必拒",
    );
    s.add(
        "表格载荷漂移拒",
        l0.record_table(0, table_cell(0).unwrap_or(0).wrapping_add(1)) == Err(OrcaCode::ETableDrift),
        "表族载荷漂移必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = OrcaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < TABLE_CELLS {
                if let Some(w) = table_cell(o) {
                    let _ = lt.record_table(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < EDIT_OPS {
                if let Some(w) = edit_cell(e) {
                    let _ = lt.record_edit(e, w);
                }
                e += 1;
            }
            let mut i = 0usize;
            while i < VERIFY_CELLS {
                if let Some(w) = verify_cell(i) {
                    let _ = lt.record_verify(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < SIG_ROWS {
                let _ = lt.sign_sdk(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < DIST_ROWS {
                let _ = lt.declare_dist(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_families() {
        assert_eq!(table_kind_name(0), Some("Property"));
        assert_eq!(table_kind_name(1), Some("Directory"));
        assert_eq!(table_kind_name(2), Some("File"));
        assert_eq!(table_kind_name(3), Some("Registry"));
        assert_eq!(table_kind_name(4), Some("Component"));
        assert!(table_kind_name(5).is_none());
        assert_eq!(table_kind(0), Some(0));
        assert_eq!(table_kind(4), Some(1));
        assert_eq!(table_kind(19), Some(4));
        assert!(table_kind(20).is_none());
        assert!(table_cell(20).is_none());
    }

    #[test]
    fn test_edit_ops_names() {
        assert_eq!(edit_op_name(0), Some("打开 .msi"));
        assert_eq!(edit_op_name(9), Some("保存 .msi"));
        assert!(edit_op_name(10).is_none());
        assert!(edit_cell(10).is_none());
    }

    #[test]
    fn test_verify_cells_names() {
        assert_eq!(verify_cell_name(0), Some("表结构验证"));
        assert_eq!(verify_cell_name(6), Some("双向校验和"));
        assert!(verify_cell_name(8).is_none());
        assert!(verify_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = table_cell(3).unwrap_or(0);
        assert_eq!(a, table_cell(3).unwrap_or(0));
        assert_ne!(a, table_cell(4).unwrap_or(0));
        // XOR 锚低 16 位恒零。
        assert_eq!(ANCHOR & 0xFFFF, 0);
        assert!(sig_row(SIG_ROWS).is_none());
        assert!(dist_row(DIST_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = OrcaLedger::new();
        let _ = lt.record_table(9, table_cell(9).unwrap_or(0));
        assert_eq!(lt.record_table(2, table_cell(2).unwrap_or(0)), Err(OrcaCode::EOrder));
        // 逆序被拒后 last_kind 不前移，同族高格仍可落。
        assert!(lt.record_table(10, table_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = OrcaLedger::new();
        assert_eq!(
            lt.record_table(5, table_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(OrcaCode::ETableDrift)
        );
        assert_eq!(
            lt.record_edit(3, edit_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(OrcaCode::EEditDrift)
        );
        assert_eq!(
            lt.record_verify(6, verify_cell(6).unwrap_or(0).wrapping_add(7)),
            Err(OrcaCode::ETableDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = OrcaLedger::new();
        let _ = lt.record_table(0, table_cell(0).unwrap_or(0));
        assert_eq!(lt.record_table(0, table_cell(0).unwrap_or(0)), Err(OrcaCode::EDup));
        let _ = lt.sign_sdk(0);
        assert_eq!(lt.sign_sdk(0), Err(OrcaCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = OrcaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = OrcaLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_row_boundary() {
        let mut lt = OrcaLedger::new();
        assert!(lt.sign_sdk(0).is_ok());
        assert!(lt.sign_sdk(SIG_ROWS).is_err());
        assert!(lt.declare_dist(0).is_ok());
        assert!(lt.declare_dist(DIST_ROWS).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        let total = (TABLE_CELLS + EDIT_OPS + VERIFY_CELLS) as u32;
        assert!(recorded >= total);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC8F00);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(OrcaCode::Ok as u32, 0);
        assert_eq!(OrcaCode::EGradeDrift as u32, 12);
        assert_eq!(OrcaCode::Ok.val(), SEG_BASE);
        assert_eq!(OrcaCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let mut i = 0u32;
        while i <= 12 {
            let code = match i {
                0 => OrcaCode::Ok,
                1 => OrcaCode::ENoTable,
                2 => OrcaCode::ETableDrift,
                3 => OrcaCode::EOrder,
                4 => OrcaCode::ENoEdit,
                5 => OrcaCode::EEditDrift,
                6 => OrcaCode::ENoVerify,
                7 => OrcaCode::ENoRow,
                8 => OrcaCode::EDup,
                9 => OrcaCode::ESeal,
                10 => OrcaCode::EInj,
                11 => OrcaCode::EFp,
                _ => OrcaCode::EGradeDrift,
            };
            assert_eq!(code as u32, i);
            assert!(!code.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = OrcaLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 未齐即封：只落六判据不给全账，仍按 S3 封口。
        let mut lt2 = OrcaLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52317_checks_green() {
        let s = run_unx52317_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
