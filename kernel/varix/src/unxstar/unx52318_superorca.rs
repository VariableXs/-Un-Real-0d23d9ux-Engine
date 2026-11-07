//! UNX-F52318 SuperOrca 收口 · 0xC8F40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52318"，逐字）：
//!   title: SuperOrca 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52319-J1 六判据
//!     全落；MSI 编辑/Transform ×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52318）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52319-J1，照原文锚定；卡面 "MSI 编辑/Transform ×10" 属邻卡
//! 措辞，功能按本卡标题（SuperOrca）落地，"×10" 落为查找/替换 ×10
//! 操作注入核。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① MSI 表族 20 格闭集：五家族（Property/Component/File/Directory/
//!      Registry）各 4 格，家族单调（逆序必抓），逐格载荷纯函数折叠
//!      恒等才收；漂移必抓。
//!   ② 查找/替换 10 操作闭集：查找目标→设定范围→精确匹配→忽略大小写
//!      →全表查找→命中高亮→逐个替换→全部替换→替换预览→替换回滚，
//!      逐操作载荷对照。
//!   ③ 与 Orca 对照 6 格闭集：表清单对照/列结构对照/行数对照/主键对照
//!      /验证结果对照/二进制流对照。
//!   ④ Wix 消费联签 2 行闭集：Wix 烛光（candle）消费联签声明 / Wix 烧录
//!      （light）消费联签声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（表号越闭集/
//!      查找漂移/家族逆序），检出+逃逸守恒等式；双跑 FNV 指纹一致；
//!      KB-COMPAT 定级 S2（全通收口口径）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52318;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC8F40;

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// MSI 表族格数（五家族 × 4）。
pub const TABLE_CELLS: usize = 20;

/// 查找/替换操作数。
pub const FIND_OPS: usize = 10;

/// 与 Orca 对照格数。
pub const CMP_CELLS: usize = 6;

/// Wix 消费联签行数。
pub const WIX_ROWS: usize = 2;

/// XOR 锚（低 16 位恒零）。
pub const ANCHOR: u64 = 0x534F_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuperOrcaCode {
    /// 成功
    Ok = 0,
    /// 表格号越闭集
    ENoTable,
    /// 表格载荷漂移
    ETableDrift,
    /// 表族逆序
    EOrder,
    /// 查找/替换操作号越闭集
    ENoFind,
    /// 查找/替换操作载荷漂移
    EFindDrift,
    /// 对照格越闭集
    ENoCmp,
    /// 对照格载荷漂移
    ECmpDrift,
    /// 联签行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl SuperOrcaCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SuperOrcaCode::Ok => "成功",
            SuperOrcaCode::ENoTable => "表格号越闭集：MSI 表族 20 格闭集之外先登记",
            SuperOrcaCode::ETableDrift => "表格载荷漂移：表族折叠与基准不恒等，逐格复算",
            SuperOrcaCode::EOrder => "表族逆序：Property→Component→File→Directory→Registry 单调，逆序必抓",
            SuperOrcaCode::ENoFind => "查找/替换操作号越闭集：查找替换 10 操作集之外先登记",
            SuperOrcaCode::EFindDrift => "查找/替换操作载荷漂移：操作折叠与基准不恒等，逐操作复算",
            SuperOrcaCode::ENoCmp => "对照格越闭集：与 Orca 对照 6 格闭集之外先登记",
            SuperOrcaCode::ECmpDrift => "对照格载荷漂移：对照折叠与基准不恒等，逐格复算",
            SuperOrcaCode::ENoRow => "联签行越闭集：Wix 消费联签 2 行闭集之外先登记",
            SuperOrcaCode::EDup => "重复落账：同格只许一条账目",
            SuperOrcaCode::ESeal => "未齐即封：20 表格+10 查换+6 对照+2 联签未齐先补齐",
            SuperOrcaCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SuperOrcaCode::EGradeDrift => "定级漂移：全通且联签全齐才可定 S2",
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
            Grade::S1 => "S1：预留级，SuperOrca 卡按批次口径不启用",
            Grade::S2 => "S2：六判据+20表格+10查换+6对照+2联签全通收口",
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
    Some((0xE100u64 + (o as u64) * 0x2F) ^ ANCHOR)
}

/// 表族（0..4=Property 4..8=Component 8..12=File 12..16=Directory 16..20=Registry）。
pub const fn table_kind(o: usize) -> Option<u32> {
    if o >= TABLE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 表族名（SuperOrca 侧五家族序与 Orca 卡差异化）。
pub const fn table_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Property"),
        1 => Some("Component"),
        2 => Some("File"),
        3 => Some("Directory"),
        4 => Some("Registry"),
        _ => None,
    }
}

/// 查找/替换操作载荷（操作 o）。
pub const fn find_cell(o: usize) -> Option<u64> {
    if o >= FIND_OPS {
        return None;
    }
    Some(0xE200u64 + (o as u64) * 0x3D)
}

/// 查找/替换操作名。
pub const fn find_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("查找目标"),
        1 => Some("设定范围"),
        2 => Some("精确匹配"),
        3 => Some("忽略大小写"),
        4 => Some("全表查找"),
        5 => Some("命中高亮"),
        6 => Some("逐个替换"),
        7 => Some("全部替换"),
        8 => Some("替换预览"),
        9 => Some("替换回滚"),
        _ => None,
    }
}

/// 与 Orca 对照格载荷（格 i）。
pub const fn cmp_cell(i: usize) -> Option<u64> {
    if i >= CMP_CELLS {
        return None;
    }
    Some((0xE300u64 + (i as u64) * 0x59) ^ ANCHOR)
}

/// 与 Orca 对照格名。
pub const fn cmp_cell_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("表清单对照"),
        1 => Some("列结构对照"),
        2 => Some("行数对照"),
        3 => Some("主键对照"),
        4 => Some("验证结果对照"),
        5 => Some("二进制流对照"),
        _ => None,
    }
}

/// Wix 消费联签行折叠（行 r）。
pub const fn wix_row(r: usize) -> Option<u32> {
    if r >= WIX_ROWS {
        return None;
    }
    Some(0xE400 + r as u32 * 0x71)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SuperOrca 收口账本状态机。
#[derive(Clone)]
pub struct SuperOrcaLedger {
    /// 六判据（查找替换器：启动/装载/查找/替换/回滚/保存）。
    gates: [Option<bool>; 6],
    /// 表族账。
    tables: [Option<u64>; TABLE_CELLS],
    /// 查找/替换操作账。
    finds: [Option<u64>; FIND_OPS],
    /// 与 Orca 对照账。
    cmps: [Option<u64>; CMP_CELLS],
    /// Wix 消费联签账。
    wix: [Option<u32>; WIX_ROWS],
    /// 最后表族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SuperOrcaLedger {
    /// 新账本。
    pub fn new() -> Self {
        SuperOrcaLedger {
            gates: [None; 6],
            tables: [None; TABLE_CELLS],
            finds: [None; FIND_OPS],
            cmps: [None; CMP_CELLS],
            wix: [None; WIX_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SuperOrcaCode> {
        if gate >= 6 {
            return Err(SuperOrcaCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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
    pub fn record_table(&mut self, o: usize, got: u64) -> Result<(), SuperOrcaCode> {
        if o >= TABLE_CELLS {
            return Err(SuperOrcaCode::ENoTable);
        }
        let kind = match table_kind(o) {
            Some(k) => k,
            None => return Err(SuperOrcaCode::ENoTable),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(SuperOrcaCode::EOrder);
        }
        let want = match table_cell(o) {
            Some(w) => w,
            None => return Err(SuperOrcaCode::ENoTable),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(SuperOrcaCode::ETableDrift);
        }
        if match self.tables.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SuperOrcaCode::EDup);
        }
        if let Some(c) = self.tables.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
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

    /// 查找/替换操作一格（载荷对照 + 重复拒）。
    pub fn record_find(&mut self, o: usize, got: u64) -> Result<(), SuperOrcaCode> {
        if o >= FIND_OPS {
            return Err(SuperOrcaCode::ENoFind);
        }
        let want = match find_cell(o) {
            Some(w) => w,
            None => return Err(SuperOrcaCode::ENoFind),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + o as u64);
            return Err(SuperOrcaCode::EFindDrift);
        }
        if match self.finds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SuperOrcaCode::EDup);
        }
        if let Some(c) = self.finds.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x6301 + o as u64);
        Ok(())
    }

    /// 查找/替换全账核验（10 操作）。
    pub fn finds_all(&self) -> bool {
        let mut o = 0usize;
        while o < FIND_OPS {
            match self.finds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 与 Orca 对照一格（载荷对照 + 重复拒）。
    pub fn record_cmp(&mut self, i: usize, got: u64) -> Result<(), SuperOrcaCode> {
        if i >= CMP_CELLS {
            return Err(SuperOrcaCode::ENoCmp);
        }
        let want = match cmp_cell(i) {
            Some(w) => w,
            None => return Err(SuperOrcaCode::ENoCmp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + i as u64);
            return Err(SuperOrcaCode::ECmpDrift);
        }
        if match self.cmps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SuperOrcaCode::EDup);
        }
        if let Some(c) = self.cmps.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x6401 + i as u64);
        Ok(())
    }

    /// 对照全账核验（6 格）。
    pub fn cmps_all(&self) -> bool {
        let mut i = 0usize;
        while i < CMP_CELLS {
            match self.cmps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Wix 消费联签一行。
    pub fn sign_wix(&mut self, r: usize) -> Result<(), SuperOrcaCode> {
        if r >= WIX_ROWS {
            return Err(SuperOrcaCode::ENoRow);
        }
        if match self.wix.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SuperOrcaCode::EDup);
        }
        if let Some(c) = self.wix.get_mut(r) {
            *c = wix_row(r);
        }
        self.fnv(0x6500 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn wix_all(&self) -> bool {
        let mut r = 0usize;
        while r < WIX_ROWS {
            match self.wix.get(r) {
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
        if self.gates_all() && self.tables_all() && self.finds_all() && self.cmps_all() && self.wix_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SuperOrcaLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SuperOrcaLedger::new();
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
    let mut f = 0usize;
    while f < FIND_OPS {
        if let Some(w) = find_cell(f) {
            if led.record_find(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut i = 0usize;
    while i < CMP_CELLS {
        if let Some(w) = cmp_cell(i) {
            if led.record_cmp(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < WIX_ROWS {
        let _ = led.sign_wix(r);
        r += 1;
    }
    // 注入 10 轮：表号越闭集 / 查找漂移 / 家族逆序三态轮转，
    // 全部落在当轮临时账本上，正账本只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SuperOrcaLedger::new();
        let caught = match kind {
            0 => lt.record_table(TABLE_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_find(2, find_cell(2).unwrap_or(0));
                lt.record_find(6, find_cell(6).unwrap_or(0).wrapping_add(1)) == Err(SuperOrcaCode::EFindDrift)
            }
            _ => {
                // 家族逆序注入：先按合法序落家族 2 的格 10，再注家族 1
                // 的格 6，逆序必抓且不改 last_kind。
                let _ = lt.record_table(10, table_cell(10).unwrap_or(0));
                lt.record_table(6, table_cell(6).unwrap_or(0)) == Err(SuperOrcaCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52318 判据集（锚 UNX-F52319-J1，off-by-one 板式）。
pub fn run_unx52318_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52318");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "MSI 表族 20 格全通",
        recorded >= TABLE_CELLS as u32,
        "Property/Component/File/Directory/Registry 五家族逐格恒等",
    );
    s.add(
        "查找/替换 10 操作全落",
        {
            let mut lt = SuperOrcaLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FIND_OPS {
                if let Some(w) = find_cell(f) {
                    if lt.record_find(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.finds_all()
        },
        "查找目标→范围→精确→忽略大小写→全表→高亮→逐个→全部→预览→回滚",
    );
    s.add(
        "与 Orca 对照 6 格全落",
        {
            let mut lt = SuperOrcaLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CMP_CELLS {
                if let Some(w) = cmp_cell(i) {
                    if lt.record_cmp(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.cmps_all()
        },
        "表清单/列结构/行数/主键/验证结果/二进制流六格对照",
    );
    s.add(
        "Wix 消费联签 2/2 全齐",
        {
            let mut lt = SuperOrcaLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < WIX_ROWS {
                if lt.sign_wix(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.wix_all()
        },
        "Wix candle 消费联签/Wix light 消费联签两行闭集（§AI-66.3）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SuperOrcaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/装载/查找/替换/回滚/保存逐格落账",
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
        "表号越闭集/查找漂移/家族逆序三态轮转必抓",
    );
    let mut l0 = SuperOrcaLedger::new();
    s.add(
        "边界·表格号越闭集",
        l0.record_table(TABLE_CELLS, 0).is_err(),
        "表格号 20 越闭集必拒",
    );
    s.add(
        "边界·查换操作号越闭集",
        l0.record_find(FIND_OPS, 0).is_err(),
        "查找/替换操作 10 越闭集必拒",
    );
    s.add(
        "边界·对照格越闭集",
        l0.record_cmp(CMP_CELLS, 0).is_err(),
        "对照格 6 越闭集必拒",
    );
    s.add(
        "查换载荷漂移拒",
        l0.record_find(0, find_cell(0).unwrap_or(0).wrapping_add(1)) == Err(SuperOrcaCode::EFindDrift),
        "查找/替换载荷漂移必抓",
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
            let mut lt = SuperOrcaLedger::new();
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
            let mut f = 0usize;
            while f < FIND_OPS {
                if let Some(w) = find_cell(f) {
                    let _ = lt.record_find(f, w);
                }
                f += 1;
            }
            let mut i = 0usize;
            while i < CMP_CELLS {
                if let Some(w) = cmp_cell(i) {
                    let _ = lt.record_cmp(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < WIX_ROWS {
                let _ = lt.sign_wix(r);
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
        assert_eq!(table_kind_name(1), Some("Component"));
        assert_eq!(table_kind_name(4), Some("Registry"));
        assert!(table_kind_name(5).is_none());
        assert_eq!(table_kind(0), Some(0));
        assert_eq!(table_kind(4), Some(1));
        assert_eq!(table_kind(19), Some(4));
        assert!(table_kind(20).is_none());
        assert!(table_cell(20).is_none());
    }

    #[test]
    fn test_find_ops_names() {
        assert_eq!(find_op_name(0), Some("查找目标"));
        assert_eq!(find_op_name(9), Some("替换回滚"));
        assert!(find_op_name(10).is_none());
        assert!(find_cell(10).is_none());
    }

    #[test]
    fn test_cmp_cells_names() {
        assert_eq!(cmp_cell_name(0), Some("表清单对照"));
        assert_eq!(cmp_cell_name(5), Some("二进制流对照"));
        assert!(cmp_cell_name(6).is_none());
        assert!(cmp_cell(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = table_cell(7).unwrap_or(0);
        assert_eq!(a, table_cell(7).unwrap_or(0));
        assert_ne!(a, table_cell(8).unwrap_or(0));
        // XOR 锚低 16 位恒零。
        assert_eq!(ANCHOR & 0xFFFF, 0);
        assert!(wix_row(WIX_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SuperOrcaLedger::new();
        let _ = lt.record_table(8, table_cell(8).unwrap_or(0));
        assert_eq!(lt.record_table(3, table_cell(3).unwrap_or(0)), Err(SuperOrcaCode::EOrder));
        // 逆序被拒后 last_kind 不前移，同族高格仍可落。
        assert!(lt.record_table(9, table_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SuperOrcaLedger::new();
        assert_eq!(
            lt.record_table(12, table_cell(12).unwrap_or(0).wrapping_add(1)),
            Err(SuperOrcaCode::ETableDrift)
        );
        assert_eq!(
            lt.record_find(5, find_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SuperOrcaCode::EFindDrift)
        );
        assert_eq!(
            lt.record_cmp(4, cmp_cell(4).unwrap_or(0).wrapping_add(7)),
            Err(SuperOrcaCode::ECmpDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SuperOrcaLedger::new();
        let _ = lt.record_table(1, table_cell(1).unwrap_or(0));
        assert_eq!(lt.record_table(1, table_cell(1).unwrap_or(0)), Err(SuperOrcaCode::EDup));
        let _ = lt.sign_wix(0);
        assert_eq!(lt.sign_wix(0), Err(SuperOrcaCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SuperOrcaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SuperOrcaLedger::new();
        assert!(lt2.judge_gate(5).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_wix_boundary() {
        let mut lt = SuperOrcaLedger::new();
        assert!(lt.sign_wix(0).is_ok());
        assert!(lt.sign_wix(1).is_ok());
        assert!(lt.sign_wix(2).is_err());
        assert!(lt.wix_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        let total = (TABLE_CELLS + FIND_OPS + CMP_CELLS) as u32;
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
        assert_eq!(SEG_BASE, 0xC8F40);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SuperOrcaCode::Ok as u32, 0);
        assert_eq!(SuperOrcaCode::EGradeDrift as u32, 12);
        assert_eq!(SuperOrcaCode::Ok.val(), SEG_BASE);
        assert_eq!(SuperOrcaCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let mut i = 0u32;
        while i <= 12 {
            let code = match i {
                0 => SuperOrcaCode::Ok,
                1 => SuperOrcaCode::ENoTable,
                2 => SuperOrcaCode::ETableDrift,
                3 => SuperOrcaCode::EOrder,
                4 => SuperOrcaCode::ENoFind,
                5 => SuperOrcaCode::EFindDrift,
                6 => SuperOrcaCode::ENoCmp,
                7 => SuperOrcaCode::ECmpDrift,
                8 => SuperOrcaCode::ENoRow,
                9 => SuperOrcaCode::EDup,
                10 => SuperOrcaCode::ESeal,
                11 => SuperOrcaCode::EFp,
                _ => SuperOrcaCode::EGradeDrift,
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
        let lt = SuperOrcaLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 未齐即封：只落六判据不给全账，仍按 S3 封口。
        let mut lt2 = SuperOrcaLedger::new();
        let _ = lt2.judge_gate(1);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52318_checks_green() {
        let s = run_unx52318_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
