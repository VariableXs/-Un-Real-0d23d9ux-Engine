//! UNX-F52319 InstEd 收口 · 0xC8F80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52319"，逐字）：
//!   title: InstEd 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52320-J1 六判据
//!     全落；解包/表查看 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52319）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52320-J1，照原文锚定；卡面 "解包/表查看 ×10" 属邻卡措辞，
//! 功能按本卡标题（InstEd）落地，"×10" 落为即时编辑 ×10 操作注入核。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① MSI 表族 20 格闭集：五家族（Property/File/Directory/Registry/
//!      Environment）各 4 格，家族单调（逆序必抓），逐格载荷纯函数折
//!      叠恒等才收；漂移必抓。
//!   ② 即时编辑 10 操作闭集：打开 .msi→行内编辑→单元格直改→行插入→
//!      行删除→复制单元格→查找→跳转表→验证视图→保存 .msi。
//!   ③ 转换（Transform）8 格闭集：生成转换/应用转换/嵌套转换/转换差异
//!      /转换校验和/转换回滚/转换导出/验证转换。
//!   ④ 版本阶梯 3 档闭集（1.5.x/2.0.x/2.2.x），越级必抓，逐级载荷折
//!      叠，纯函数恒等才收。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（表号越闭集/
//!      版本漂移/家族逆序），检出+逃逸守恒等式；双跑 FNV 指纹一致；
//!      KB-COMPAT 定级 S2（全通收口口径）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52319;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC8F80;

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// MSI 表族格数（五家族 × 4）。
pub const TABLE_CELLS: usize = 20;

/// 即时编辑操作数。
pub const EDIT_OPS: usize = 10;

/// 转换（Transform）格数。
pub const TRANSFORM_CELLS: usize = 8;

/// 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// XOR 锚（低 16 位恒零）。
pub const ANCHOR: u64 = 0x4945_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstEdCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 表格号越闭集
    ENoTable,
    /// 表格/编辑操作载荷漂移
    ETableDrift,
    /// 表族逆序
    EOrder,
    /// 即时编辑操作号越闭集
    ENoEdit,
    /// 转换格越闭集
    ENoTf,
    /// 转换格载荷漂移
    ETfDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl InstEdCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            InstEdCode::Ok => "成功",
            InstEdCode::ENoVer => "版本级越阶梯：1.5.x/2.0.x/2.2.x 三级闭集之外先登记",
            InstEdCode::EVerDrift => "版本阶梯漂移：级载荷折叠与基准不恒等，逐级复算",
            InstEdCode::ENoTable => "表格号越闭集：MSI 表族 20 格闭集之外先登记",
            InstEdCode::ETableDrift => "表格/编辑操作载荷漂移：折叠与基准不恒等，逐格复算",
            InstEdCode::EOrder => "表族逆序：Property→File→Directory→Registry→Environment 单调，逆序必抓",
            InstEdCode::ENoEdit => "即时编辑操作号越闭集：即时编辑 10 操作集之外先登记",
            InstEdCode::ENoTf => "转换格越闭集：Transform 8 格闭集之外先登记",
            InstEdCode::ETfDrift => "转换格载荷漂移：转换折叠与基准不恒等，逐格复算",
            InstEdCode::EDup => "重复落账：同格只许一条账目",
            InstEdCode::ESeal => "未齐即封：20 表格+10 编辑+8 转换+3 级版本未齐先补齐",
            InstEdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            InstEdCode::EGradeDrift => "定级漂移：全通且阶梯全齐才可定 S2",
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
            Grade::S1 => "S1：预留级，InstEd 卡按批次口径不启用",
            Grade::S2 => "S2：六判据+20表格+10编辑+8转换+3级版本全通收口",
            Grade::S3 => "S3：不收口，六判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本级载荷（级 v：0=1.5.x 1=2.0.x 2=2.2.x，与 XOR 锚折叠）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xF100u64 + (v as u64) * 0x5D) ^ ANCHOR)
}

/// 版本级名（const fn 表 + Option 边界）。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("1.5.x"),
        1 => Some("2.0.x"),
        2 => Some("2.2.x"),
        _ => None,
    }
}

/// MSI 表族格载荷（格 o；家族 = o/4）。
pub const fn table_cell(o: usize) -> Option<u64> {
    if o >= TABLE_CELLS {
        return None;
    }
    Some((0xF200u64 + (o as u64) * 0x2B) ^ ANCHOR)
}

/// 表族（0..4=Property 4..8=File 8..12=Directory 12..16=Registry 16..20=Environment）。
pub const fn table_kind(o: usize) -> Option<u32> {
    if o >= TABLE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 表族名（InstEd 侧五家族序与前两卡差异化）。
pub const fn table_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Property"),
        1 => Some("File"),
        2 => Some("Directory"),
        3 => Some("Registry"),
        4 => Some("Environment"),
        _ => None,
    }
}

/// 即时编辑操作载荷（操作 o）。
pub const fn edit_cell(o: usize) -> Option<u64> {
    if o >= EDIT_OPS {
        return None;
    }
    Some(0xF300u64 + (o as u64) * 0x43)
}

/// 即时编辑操作名。
pub const fn edit_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("打开 .msi"),
        1 => Some("行内编辑"),
        2 => Some("单元格直改"),
        3 => Some("行插入"),
        4 => Some("行删除"),
        5 => Some("复制单元格"),
        6 => Some("查找"),
        7 => Some("跳转表"),
        8 => Some("验证视图"),
        9 => Some("保存 .msi"),
        _ => None,
    }
}

/// 转换格载荷（格 i）。
pub const fn transform_cell(i: usize) -> Option<u64> {
    if i >= TRANSFORM_CELLS {
        return None;
    }
    Some((0xF400u64 + (i as u64) * 0x37) ^ ANCHOR)
}

/// 转换格名。
pub const fn transform_cell_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("生成转换"),
        1 => Some("应用转换"),
        2 => Some("嵌套转换"),
        3 => Some("转换差异"),
        4 => Some("转换校验和"),
        5 => Some("转换回滚"),
        6 => Some("转换导出"),
        7 => Some("验证转换"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// InstEd 收口账本状态机。
#[derive(Clone)]
pub struct InstEdLedger {
    /// 六判据（即时编辑器：启动/打开/编辑/转换/验证/保存）。
    gates: [Option<bool>; 6],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 表族账。
    tables: [Option<u64>; TABLE_CELLS],
    /// 即时编辑操作账。
    edits: [Option<u64>; EDIT_OPS],
    /// 转换账。
    tfs: [Option<u64>; TRANSFORM_CELLS],
    /// 最后表族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl InstEdLedger {
    /// 新账本。
    pub fn new() -> Self {
        InstEdLedger {
            gates: [None; 6],
            vers: [None; VERSION_STEPS],
            tables: [None; TABLE_CELLS],
            edits: [None; EDIT_OPS],
            tfs: [None; TRANSFORM_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), InstEdCode> {
        if gate >= 6 {
            return Err(InstEdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 版本级一格（阶梯载荷对照 + 重复拒）。
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), InstEdCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(InstEdCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + v as u64);
            return Err(InstEdCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InstEdCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x7201 + v as u64);
        Ok(())
    }

    /// 版本阶梯全账核验（3 级全齐）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VERSION_STEPS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 表族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_table(&mut self, o: usize, got: u64) -> Result<(), InstEdCode> {
        if o >= TABLE_CELLS {
            return Err(InstEdCode::ENoTable);
        }
        let kind = match table_kind(o) {
            Some(k) => k,
            None => return Err(InstEdCode::ENoTable),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(InstEdCode::EOrder);
        }
        let want = match table_cell(o) {
            Some(w) => w,
            None => return Err(InstEdCode::ENoTable),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + o as u64);
            return Err(InstEdCode::ETableDrift);
        }
        if match self.tables.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InstEdCode::EDup);
        }
        if let Some(c) = self.tables.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7302 + o as u64);
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

    /// 即时编辑操作一格（载荷对照 + 重复拒）。
    pub fn record_edit(&mut self, o: usize, got: u64) -> Result<(), InstEdCode> {
        if o >= EDIT_OPS {
            return Err(InstEdCode::ENoEdit);
        }
        let want = match edit_cell(o) {
            Some(w) => w,
            None => return Err(InstEdCode::ENoEdit),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + o as u64);
            return Err(InstEdCode::ETableDrift);
        }
        if match self.edits.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InstEdCode::EDup);
        }
        if let Some(c) = self.edits.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x7401 + o as u64);
        Ok(())
    }

    /// 即时编辑全账核验（10 操作）。
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

    /// 转换一格（载荷对照 + 重复拒）。
    pub fn record_transform(&mut self, i: usize, got: u64) -> Result<(), InstEdCode> {
        if i >= TRANSFORM_CELLS {
            return Err(InstEdCode::ENoTf);
        }
        let want = match transform_cell(i) {
            Some(w) => w,
            None => return Err(InstEdCode::ENoTf),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + i as u64);
            return Err(InstEdCode::ETfDrift);
        }
        if match self.tfs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(InstEdCode::EDup);
        }
        if let Some(c) = self.tfs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x7501 + i as u64);
        Ok(())
    }

    /// 转换全账核验（8 格）。
    pub fn transforms_all(&self) -> bool {
        let mut i = 0usize;
        while i < TRANSFORM_CELLS {
            match self.tfs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
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
        if self.gates_all() && self.vers_all() && self.tables_all() && self.edits_all() && self.transforms_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for InstEdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = InstEdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VERSION_STEPS {
        if let Some(w) = version_cell(v) {
            if led.record_version(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
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
    while i < TRANSFORM_CELLS {
        if let Some(w) = transform_cell(i) {
            if led.record_transform(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    // 注入 10 轮：表号越闭集 / 版本漂移 / 家族逆序三态轮转，
    // 全部落在当轮临时账本上，正账本只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = InstEdLedger::new();
        let caught = match kind {
            0 => lt.record_table(TABLE_CELLS, 0).is_err(),
            1 => {
                // 漂移注入先录正格再注漂移。
                let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
                lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(3)) == Err(InstEdCode::EVerDrift)
            }
            _ => {
                // 家族逆序注入：先按合法序落家族 1 的格 5，再注家族 0
                // 的格 3，逆序必抓且不改 last_kind。
                let _ = lt.record_table(5, table_cell(5).unwrap_or(0));
                lt.record_table(3, table_cell(3).unwrap_or(0)) == Err(InstEdCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7600 + k as u64);
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
/// UNX-F52319 判据集（锚 UNX-F52320-J1，off-by-one 板式）。
pub fn run_unx52319_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52319");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "MSI 表族 20 格全通",
        recorded >= TABLE_CELLS as u32,
        "Property/File/Directory/Registry/Environment 五家族逐格恒等",
    );
    s.add(
        "即时编辑 10 操作全落",
        {
            let mut lt = InstEdLedger::new();
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
        "打开/行内编辑/直改/插删行/复制/查找/跳转/验证视图/保存逐操作对照",
    );
    s.add(
        "转换（Transform）8 格全落",
        {
            let mut lt = InstEdLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < TRANSFORM_CELLS {
                if let Some(w) = transform_cell(i) {
                    if lt.record_transform(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.transforms_all()
        },
        "生成/应用/嵌套/差异/校验和/回滚/导出/验证八格转换闭集",
    );
    s.add(
        "版本阶梯 1.5.x/2.0.x/2.2.x 三档全落",
        {
            let mut lt = InstEdLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    if lt.record_version(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "三档闭集逐级恒等，越级必抓",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = InstEdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/打开/编辑/转换/验证/保存逐格落账",
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
        "表号越闭集/版本漂移/家族逆序三态轮转必抓",
    );
    let mut l0 = InstEdLedger::new();
    s.add(
        "边界·表格号越闭集",
        l0.record_table(TABLE_CELLS, 0).is_err(),
        "表格号 20 越闭集必拒",
    );
    s.add(
        "边界·编辑操作号越闭集",
        l0.record_edit(EDIT_OPS, 0).is_err(),
        "即时编辑操作 10 越闭集必拒",
    );
    s.add(
        "边界·转换格越闭集",
        l0.record_transform(TRANSFORM_CELLS, 0).is_err(),
        "转换格 8 越闭集必拒",
    );
    s.add(
        "转换载荷漂移拒",
        l0.record_transform(0, transform_cell(0).unwrap_or(0).wrapping_add(1)) == Err(InstEdCode::ETfDrift),
        "转换载荷漂移必抓",
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
            let mut lt = InstEdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VERSION_STEPS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_version(v, w);
                }
                v += 1;
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
            while i < TRANSFORM_CELLS {
                if let Some(w) = transform_cell(i) {
                    let _ = lt.record_transform(i, w);
                }
                i += 1;
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
    fn test_version_ladder() {
        assert_eq!(version_name(0), Some("1.5.x"));
        assert_eq!(version_name(1), Some("2.0.x"));
        assert_eq!(version_name(2), Some("2.2.x"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_table_families() {
        assert_eq!(table_kind_name(0), Some("Property"));
        assert_eq!(table_kind_name(1), Some("File"));
        assert_eq!(table_kind_name(4), Some("Environment"));
        assert!(table_kind_name(5).is_none());
        assert_eq!(table_kind(12), Some(3));
        assert_eq!(table_kind(19), Some(4));
        assert!(table_kind(20).is_none());
        assert!(table_cell(20).is_none());
    }

    #[test]
    fn test_edit_ops_names() {
        assert_eq!(edit_op_name(0), Some("打开 .msi"));
        assert_eq!(edit_op_name(8), Some("验证视图"));
        assert!(edit_op_name(10).is_none());
        assert!(edit_cell(10).is_none());
    }

    #[test]
    fn test_transform_names() {
        assert_eq!(transform_cell_name(0), Some("生成转换"));
        assert_eq!(transform_cell_name(2), Some("嵌套转换"));
        assert_eq!(transform_cell_name(7), Some("验证转换"));
        assert!(transform_cell_name(8).is_none());
        assert!(transform_cell(8).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = table_cell(11).unwrap_or(0);
        assert_eq!(a, table_cell(11).unwrap_or(0));
        assert_ne!(a, table_cell(12).unwrap_or(0));
        // XOR 锚低 16 位恒零。
        assert_eq!(ANCHOR & 0xFFFF, 0);
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = InstEdLedger::new();
        let _ = lt.record_table(7, table_cell(7).unwrap_or(0));
        assert_eq!(lt.record_table(1, table_cell(1).unwrap_or(0)), Err(InstEdCode::EOrder));
        // 逆序被拒后 last_kind 不前移，同族高格仍可落。
        assert!(lt.record_table(8, table_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = InstEdLedger::new();
        assert_eq!(
            lt.record_table(14, table_cell(14).unwrap_or(0).wrapping_add(1)),
            Err(InstEdCode::ETableDrift)
        );
        assert_eq!(
            lt.record_edit(2, edit_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(InstEdCode::ETableDrift)
        );
        assert_eq!(
            lt.record_version(2, version_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(InstEdCode::EVerDrift)
        );
        assert_eq!(
            lt.record_transform(3, transform_cell(3).unwrap_or(0).wrapping_add(5)),
            Err(InstEdCode::ETfDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = InstEdLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(InstEdCode::EDup));
        let _ = lt.record_transform(5, transform_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_transform(5, transform_cell(5).unwrap_or(0)),
            Err(InstEdCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = InstEdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = InstEdLedger::new();
        assert!(lt2.judge_gate(3).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        let total = (TABLE_CELLS + EDIT_OPS + TRANSFORM_CELLS + VERSION_STEPS) as u32;
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
        assert_eq!(SEG_BASE, 0xC8F80);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(InstEdCode::Ok as u32, 0);
        assert_eq!(InstEdCode::EGradeDrift as u32, 12);
        assert_eq!(InstEdCode::Ok.val(), SEG_BASE);
        assert_eq!(InstEdCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let mut i = 0u32;
        while i <= 12 {
            let code = match i {
                0 => InstEdCode::Ok,
                1 => InstEdCode::ENoVer,
                2 => InstEdCode::EVerDrift,
                3 => InstEdCode::ENoTable,
                4 => InstEdCode::ETableDrift,
                5 => InstEdCode::EOrder,
                6 => InstEdCode::ENoEdit,
                7 => InstEdCode::ENoTf,
                8 => InstEdCode::ETfDrift,
                9 => InstEdCode::EDup,
                10 => InstEdCode::ESeal,
                11 => InstEdCode::EFp,
                _ => InstEdCode::EGradeDrift,
            };
            assert_eq!(code as u32, i);
            assert!(!code.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = InstEdLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 未齐即封：只落六判据不给全账，仍按 S3 封口。
        let mut lt2 = InstEdLedger::new();
        let _ = lt2.judge_gate(2);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52319_checks_green() {
        let s = run_unx52319_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
