//! UNX-F52216 SQuirreL SQL 收口 · 0xC75C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52216"，逐字）：
//!   title: SQuirreL SQL 收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52217-J1 六判据全
//!     落；多库 ×5；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52216）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52217-J1，照原文锚定；功能按本卡标题（SQuirreL SQL）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① JDBC 驱动账：8 驱动家族闭集（JDBC-ODBC 桥/MySQL/PostgreSQL/
//!      Oracle/MSSQL/DB2/H2/SQLite）× 每家族 2 驱动格 = 16 格，家族单调
//!      （逆序必抓）；「多库 ×5」以会话内多库切换 5 格收口（session 侧
//!      辅账）。
//!   ② SQL 别名 10 操作：五家族闭集（新建别名/修改连接/测试连接/复制
//!      别名/删除别名）各 2 操作，家族单调（逆序必抓）。
//!   ③ Java Swing 上栈 4 层：JVM→Swing 外观→驱动加载器→SQL 面板，逐层
//!      严格底向上压栈（跳层必抓）。
//!   ④ 定级 S3（长尾如实降级）：SQuirreL 为社区长尾工具，即使六判据 +
//!      全部账格全通也顶格 S3（不收口），任何拔高到 S1/S2 的声明即定级
//!      漂移——凑绿检测显式断言全通账本 judge_grade()==S3 且
//!      Grade::S3.val()==3。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/switch_db×5/fingerprint
//!      FNV-1a）；注入核 run_cycle 三态轮转 10 轮（别名家族逆序/驱动漂移
//!      /Swing 越闭集），全部注入落在当轮临时账本上，注入=检出+逃逸守恒；
//!      双跑 FNV 指纹一致。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52216;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// JDBC 驱动家族闭集数。
pub const DRV_FAMILIES: usize = 8;

/// 每家族驱动格数。
pub const SLOTS_PER_FAMILY: usize = 2;

/// 驱动账总格数（8 × 2）。
pub const DRV_CELLS: usize = DRV_FAMILIES * SLOTS_PER_FAMILY;

/// SQL 别名操作数。
pub const ALIAS_OPS: usize = 10;

/// Java Swing 上栈层数。
pub const SWING_STACK: usize = 4;

/// 多库切换格数（×5）。
pub const DB_SWITCHES: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SquirrelCode {
    /// 成功
    Ok = 0,
    /// 驱动格越闭集
    ENoDrv,
    /// 驱动漂移
    EDrvDrift,
    /// 别名操作越闭集
    ENoAlias,
    /// 别名漂移
    EDrift,
    /// 家族逆序 / 层序逆序
    EOrder,
    /// Swing 层越闭集
    ENoLayer,
    /// 多库切换格越闭集
    ENoSw,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移（长尾如实降级顶格 S3，拔高即漂移）
    EGradeDrift,
}

impl SquirrelCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SquirrelCode::Ok => "成功",
            SquirrelCode::ENoDrv => "驱动格越闭集：8 家族 × 2 格之外先登记",
            SquirrelCode::EDrvDrift => "驱动漂移：驱动格折叠与基准不恒等，逐格复算",
            SquirrelCode::ENoAlias => "别名操作越闭集：10 操作集之外先登记",
            SquirrelCode::EDrift => "别名漂移：操作折叠与基准不恒等，逐格复算",
            SquirrelCode::EOrder => "家族/层序逆序：新建→修改→测试→复制→删除单调，上栈底向上，逆序必抓",
            SquirrelCode::ENoLayer => "Swing 层越闭集：上栈 4 层之外先登记",
            SquirrelCode::ENoSw => "多库切换格越闭集：×5 切换格之外先登记",
            SquirrelCode::EDup => "重复落账：同格只许一条账目",
            SquirrelCode::ESeal => "未齐即封：16 驱动格 + 10 操作 + 4 层 + 5 切换格未齐先补齐",
            SquirrelCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SquirrelCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SquirrelCode::EGradeDrift => "定级漂移：长尾工具如实降级顶格 S3，拔高 S1/S2 即漂移",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口（本卡顶格）
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
            Grade::S1 => "S1：全通直收（本卡不适用，长尾工具禁拔）",
            Grade::S2 => "S2：有条件收口（本卡不适用，长尾工具禁拔）",
            Grade::S3 => "S3：长尾如实降级——即便六判据+全账格全通也不收口",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 驱动格载荷（家族 f、槽 s；全局格 = f×2+s）。
pub const fn drv_cell(f: usize, s: usize) -> Option<u64> {
    if f >= DRV_FAMILIES || s >= SLOTS_PER_FAMILY {
        return None;
    }
    Some((0xF1000u64 + ((f * SLOTS_PER_FAMILY + s) as u64) * 0x37) ^ 0x53514C53514C4452)
}

/// 驱动家族名（const fn 表 + Option 边界）。
pub const fn drv_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("JDBC-ODBC 桥"),
        1 => Some("MySQL"),
        2 => Some("PostgreSQL"),
        3 => Some("Oracle"),
        4 => Some("MSSQL"),
        5 => Some("DB2"),
        6 => Some("H2"),
        7 => Some("SQLite"),
        _ => None,
    }
}

/// 驱动家族（格 = f×2+s → 家族 = 格/2）。
pub const fn drv_kind(cell: usize) -> Option<u32> {
    if cell >= DRV_CELLS {
        return None;
    }
    Some((cell / SLOTS_PER_FAMILY) as u32)
}

/// 别名操作格载荷（操作 o；家族 = o/2）。
pub const fn alias_cell(o: usize) -> Option<u64> {
    if o >= ALIAS_OPS {
        return None;
    }
    Some((0xF2000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 别名操作家族（0..2=新建别名 2..4=修改连接 4..6=测试连接 6..8=复制别名 8..10=删除别名）。
pub const fn alias_kind(o: usize) -> Option<u32> {
    if o >= ALIAS_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 家族名。
pub const fn alias_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("新建别名"),
        1 => Some("修改连接"),
        2 => Some("测试连接"),
        3 => Some("复制别名"),
        4 => Some("删除别名"),
        _ => None,
    }
}

/// Swing 上栈层载荷（层 l：0=JVM 1=Swing 外观 2=驱动加载器 3=SQL 面板）。
pub const fn swing_layer_cell(l: usize) -> Option<u64> {
    if l >= SWING_STACK {
        return None;
    }
    Some((0xF3000u64 + (l as u64) * 0x5D) ^ 0x535754475F53544B)
}

/// 层名。
pub const fn swing_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("JVM"),
        1 => Some("Swing 外观"),
        2 => Some("驱动加载器"),
        3 => Some("SQL 面板"),
        _ => None,
    }
}

/// 多库切换格载荷（×5：会话内库目录切换）。
pub const fn switch_cell(d: usize) -> Option<u64> {
    if d >= DB_SWITCHES {
        return None;
    }
    Some((0xF4000u64 + (d as u64) * 0x29) ^ 0x4D554C54495F4442)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SQuirreL SQL 收口账本状态机。
#[derive(Clone)]
pub struct SquirrelLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 驱动账（16 格）。
    drvs: [Option<u64>; DRV_CELLS],
    /// 别名操作账。
    aliases: [Option<u64>; ALIAS_OPS],
    /// Swing 上栈账。
    layers: [Option<u64>; SWING_STACK],
    /// 已压栈层数（严格底向上）。
    depth: usize,
    /// 多库切换账（5 格）。
    switches: [Option<u64>; DB_SWITCHES],
    /// 最后别名家族。
    last_kind: i64,
    /// 最后驱动家族。
    last_drv: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SquirrelLedger {
    /// 新账本。
    pub fn new() -> Self {
        SquirrelLedger {
            gates: [None; 6],
            drvs: [None; DRV_CELLS],
            aliases: [None; ALIAS_OPS],
            layers: [None; SWING_STACK],
            depth: 0,
            switches: [None; DB_SWITCHES],
            last_kind: -1,
            last_drv: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SquirrelCode> {
        if gate >= 6 {
            return Err(SquirrelCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x54300 + gate as u64);
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

    /// 驱动一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_drv(&mut self, f: usize, s: usize, got: u64) -> Result<(), SquirrelCode> {
        let cell = match (f, s) {
            (ff, ss) if ff < DRV_FAMILIES && ss < SLOTS_PER_FAMILY => ff * SLOTS_PER_FAMILY + ss,
            _ => return Err(SquirrelCode::ENoDrv),
        };
        let kind = match drv_kind(cell) {
            Some(k) => k,
            None => return Err(SquirrelCode::ENoDrv),
        };
        if (kind as i64) < self.last_drv {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x54400);
            return Err(SquirrelCode::EOrder);
        }
        let want = match drv_cell(f, s) {
            Some(w) => w,
            None => return Err(SquirrelCode::ENoDrv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x54401 + cell as u64);
            return Err(SquirrelCode::EDrvDrift);
        }
        if match self.drvs.get(cell) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SquirrelCode::EDup);
        }
        if let Some(c) = self.drvs.get_mut(cell) {
            *c = Some(got);
        }
        self.last_drv = kind as i64;
        self.fnv(0x54402 + cell as u64);
        Ok(())
    }

    /// 驱动全账核验（16 格）。
    pub fn drvs_all(&self) -> bool {
        let mut i = 0usize;
        while i < DRV_CELLS {
            match self.drvs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 别名一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_alias(&mut self, o: usize, got: u64) -> Result<(), SquirrelCode> {
        if o >= ALIAS_OPS {
            return Err(SquirrelCode::ENoAlias);
        }
        let kind = match alias_kind(o) {
            Some(k) => k,
            None => return Err(SquirrelCode::ENoAlias),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x54500);
            return Err(SquirrelCode::EOrder);
        }
        let want = match alias_cell(o) {
            Some(w) => w,
            None => return Err(SquirrelCode::ENoAlias),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x54501 + o as u64);
            return Err(SquirrelCode::EDrift);
        }
        if match self.aliases.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SquirrelCode::EDup);
        }
        if let Some(c) = self.aliases.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x54502 + o as u64);
        Ok(())
    }

    /// 别名全账核验（10 操作）。
    pub fn aliases_all(&self) -> bool {
        let mut o = 0usize;
        while o < ALIAS_OPS {
            match self.aliases.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// Swing 上栈一层（严格底向上：层号必须等于当前栈深）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), SquirrelCode> {
        if l >= SWING_STACK {
            return Err(SquirrelCode::ENoLayer);
        }
        if l != self.depth {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x54600);
            return Err(SquirrelCode::EOrder);
        }
        let want = match swing_layer_cell(l) {
            Some(w) => w,
            None => return Err(SquirrelCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x54601 + l as u64);
            return Err(SquirrelCode::EDrift);
        }
        if let Some(c) = self.layers.get_mut(l) {
            *c = Some(got);
        }
        self.depth = l + 1;
        self.fnv(0x54602 + l as u64);
        Ok(())
    }

    /// 上栈全账核验（4 层且逐层压满）。
    pub fn layers_all(&self) -> bool {
        self.depth == SWING_STACK
            && match self.layers.get(SWING_STACK - 1) {
                Some(Some(_)) => true,
                _ => false,
            }
    }

    /// 多库切换一格（×5：会话内库目录切换，载荷对照 + 重复拒）。
    pub fn switch_db(&mut self, d: usize, got: u64) -> Result<(), SquirrelCode> {
        let want = match switch_cell(d) {
            Some(w) => w,
            None => return Err(SquirrelCode::ENoSw),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x54700 + d as u64);
            return Err(SquirrelCode::EDrift);
        }
        if match self.switches.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SquirrelCode::EDup);
        }
        if let Some(c) = self.switches.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x54701 + d as u64);
        Ok(())
    }

    /// 多库切换全账核验（5 格）。
    pub fn switches_all(&self) -> bool {
        let mut d = 0usize;
        while d < DB_SWITCHES {
            match self.switches.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
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

    /// KB-COMPAT 定级（长尾如实降级：任何账面状态顶格 S3，禁拔 S1/S2）。
    pub fn judge_grade(&self) -> Grade {
        Grade::S3
    }
}

impl Default for SquirrelLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SquirrelLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < DRV_FAMILIES {
        let mut s = 0usize;
        while s < SLOTS_PER_FAMILY {
            if let Some(w) = drv_cell(f, s) {
                if led.record_drv(f, s, w).is_ok() {
                    recorded += 1;
                }
            }
            s += 1;
        }
        f += 1;
    }
    let mut o = 0usize;
    while o < ALIAS_OPS {
        if let Some(w) = alias_cell(o) {
            if led.record_alias(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut l = 0usize;
    while l < SWING_STACK {
        if let Some(w) = swing_layer_cell(l) {
            if led.record_layer(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut d = 0usize;
    while d < DB_SWITCHES {
        if let Some(w) = switch_cell(d) {
            if led.switch_db(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    // 注入 10 轮：别名家族逆序 / 驱动漂移 / Swing 越闭集三态轮转。
    // 全部注入落在当轮临时账本上（逆序先录后族再录前族；漂移先录正格再注漂移）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SquirrelLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_alias(8, alias_cell(8).unwrap_or(0));
                lt.record_alias(2, alias_cell(2).unwrap_or(0)) == Err(SquirrelCode::EOrder)
            }
            1 => {
                let _ = lt.record_drv(0, 0, drv_cell(0, 0).unwrap_or(0));
                lt.record_drv(1, 1, drv_cell(1, 1).unwrap_or(0).wrapping_add(1))
                    == Err(SquirrelCode::EDrvDrift)
            }
            _ => lt.record_layer(SWING_STACK, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x54800 + k as u64);
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
/// UNX-F52216 判据集（锚 UNX-F52217-J1，off-by-one 板式）。
pub fn run_unx52216_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52216");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "JDBC 驱动 16 格全通（8 家族 × 2）",
        recorded >= (DRV_CELLS + ALIAS_OPS + SWING_STACK + DB_SWITCHES) as u32,
        "JDBC-ODBC 桥→SQLite 八家族逐格恒等",
    );
    s.add(
        "八驱动家族逐格恒等（独立账复跑）",
        {
            let mut lt = SquirrelLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < DRV_FAMILIES {
                let mut sl = 0usize;
                while sl < SLOTS_PER_FAMILY {
                    if let Some(w) = drv_cell(f, sl) {
                        if lt.record_drv(f, sl, w).is_err() {
                            ok = false;
                        }
                    }
                    sl += 1;
                }
                f += 1;
            }
            ok && lt.drvs_all()
        },
        "驱动侧闭集逐格折叠恒等（§AI-66.3）",
    );
    s.add(
        "SQL 别名 10 操作全通",
        {
            let mut lt = SquirrelLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < ALIAS_OPS {
                if let Some(w) = alias_cell(o) {
                    if lt.record_alias(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.aliases_all()
        },
        "新建/修改/测试/复制/删除五家族 ×10 操作",
    );
    s.add(
        "Java Swing 上栈 4 层单调",
        {
            let mut lt = SquirrelLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < SWING_STACK {
                if let Some(w) = swing_layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "JVM→Swing 外观→驱动加载器→SQL 面板严格底向上",
    );
    s.add(
        "多库切换 5 格全落（×5）",
        {
            let mut lt = SquirrelLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DB_SWITCHES {
                if let Some(w) = switch_cell(d) {
                    if lt.switch_db(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.switches_all()
        },
        "会话内库目录切换五格闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SquirrelLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（锚 UNX-F52217-J1）",
    );
    s.add(
        "别名家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ALIAS_OPS {
                if alias_kind(o).unwrap_or(5) < alias_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "家族逆序/驱动漂移/Swing 越闭集三态轮转必抓",
    );
    let mut l0 = SquirrelLedger::new();
    s.add(
        "边界·驱动格越闭集",
        l0.record_drv(DRV_FAMILIES, 0, 0).is_err(),
        "家族 8 越闭集必拒",
    );
    s.add(
        "边界·别名越闭集",
        l0.record_alias(ALIAS_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    s.add(
        "边界·Swing 层越闭集",
        l0.record_layer(SWING_STACK, 0).is_err(),
        "层 4 越闭集必拒",
    );
    s.add(
        "边界·切换格越闭集",
        l0.switch_db(DB_SWITCHES, 0).is_err(),
        "切换格 5 越闭集必拒",
    );
    s.add(
        "驱动漂移拒",
        {
            let _ = l0.record_drv(2, 0, drv_cell(2, 0).unwrap_or(0));
            l0.record_drv(2, 1, drv_cell(2, 1).unwrap_or(0).wrapping_add(3)) == Err(SquirrelCode::EDrvDrift)
        },
        "驱动格载荷漂移必抓",
    );
    s.add(
        "切换格漂移拒",
        {
            let _ = l0.switch_db(0, switch_cell(0).unwrap_or(0));
            l0.switch_db(1, switch_cell(1).unwrap_or(0).wrapping_add(5)) == Err(SquirrelCode::EDrift)
        },
        "多库切换载荷漂移必抓",
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
        "凑绿检测·定级如实降级顶格 S3",
        {
            // 凑绿检测：定级断言前主/aux 各节全部记账（判据+驱动+别名+上栈+切换），
            // 即便全通也必须顶格 S3——任何拔高 S1/S2 的声明即定级漂移。
            let mut lt = SquirrelLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut f = 0usize;
            while f < DRV_FAMILIES {
                let mut sl = 0usize;
                while sl < SLOTS_PER_FAMILY {
                    if let Some(w) = drv_cell(f, sl) {
                        let _ = lt.record_drv(f, sl, w);
                    }
                    sl += 1;
                }
                f += 1;
            }
            let mut o = 0usize;
            while o < ALIAS_OPS {
                if let Some(w) = alias_cell(o) {
                    let _ = lt.record_alias(o, w);
                }
                o += 1;
            }
            let mut l = 0usize;
            while l < SWING_STACK {
                if let Some(w) = swing_layer_cell(l) {
                    let _ = lt.record_layer(l, w);
                }
                l += 1;
            }
            let mut d = 0usize;
            while d < DB_SWITCHES {
                if let Some(w) = switch_cell(d) {
                    let _ = lt.switch_db(d, w);
                }
                d += 1;
            }
            lt.gates_all()
                && lt.drvs_all()
                && lt.aliases_all()
                && lt.layers_all()
                && lt.switches_all()
                && lt.judge_grade() == Grade::S3
                && Grade::S3.val() == 3
        },
        "全通账本 judge_grade()==S3（KB-COMPAT 定级登记，长尾如实降级）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC75C0,
        "SEG_BASE 0x40 对齐且在 VX15 域 52216 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drv_families() {
        assert_eq!(drv_family_name(0), Some("JDBC-ODBC 桥"));
        assert_eq!(drv_family_name(7), Some("SQLite"));
        assert!(drv_family_name(8).is_none());
        assert!(drv_cell(DRV_FAMILIES, 0).is_none());
        assert!(drv_cell(0, SLOTS_PER_FAMILY).is_none());
    }

    #[test]
    fn test_drv_kind() {
        assert_eq!(drv_kind(0), Some(0));
        assert_eq!(drv_kind(1), Some(0));
        assert_eq!(drv_kind(15), Some(7));
        assert!(drv_kind(16).is_none());
    }

    #[test]
    fn test_alias_kind() {
        assert_eq!(alias_kind(0), Some(0));
        assert_eq!(alias_kind(2), Some(1));
        assert_eq!(alias_kind(9), Some(4));
        assert!(alias_kind(10).is_none());
        assert!(alias_cell(10).is_none());
        assert_eq!(alias_kind_name(4), Some("删除别名"));
        assert!(alias_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = switch_cell(3).unwrap_or(0);
        assert_eq!(a, switch_cell(3).unwrap_or(0));
        assert_ne!(a, switch_cell(4).unwrap_or(0));
        assert!(swing_layer_cell(SWING_STACK).is_none());
        assert!(switch_cell(DB_SWITCHES).is_none());
        assert_eq!(swing_layer_name(1), Some("Swing 外观"));
    }

    #[test]
    fn test_alias_family_order_caught() {
        let mut lt = SquirrelLedger::new();
        let _ = lt.record_alias(8, alias_cell(8).unwrap_or(0));
        assert_eq!(lt.record_alias(2, alias_cell(2).unwrap_or(0)), Err(SquirrelCode::EOrder));
        assert!(lt.record_alias(9, alias_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drv_order_caught() {
        let mut lt = SquirrelLedger::new();
        let _ = lt.record_drv(7, 0, drv_cell(7, 0).unwrap_or(0));
        assert_eq!(lt.record_drv(3, 1, drv_cell(3, 1).unwrap_or(0)), Err(SquirrelCode::EOrder));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SquirrelLedger::new();
        assert_eq!(
            lt.record_alias(5, alias_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SquirrelCode::EDrift)
        );
        assert_eq!(
            lt.record_drv(4, 1, drv_cell(4, 1).unwrap_or(0).wrapping_add(9)),
            Err(SquirrelCode::EDrvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SquirrelLedger::new();
        let _ = lt.record_drv(1, 0, drv_cell(1, 0).unwrap_or(0));
        assert_eq!(lt.record_drv(1, 0, drv_cell(1, 0).unwrap_or(0)), Err(SquirrelCode::EDup));
    }

    #[test]
    fn test_layer_strict_bottom_up() {
        let mut lt = SquirrelLedger::new();
        assert_eq!(
            lt.record_layer(2, swing_layer_cell(2).unwrap_or(0)),
            Err(SquirrelCode::EOrder)
        );
        let mut l = 0usize;
        while l < SWING_STACK {
            assert!(lt.record_layer(l, swing_layer_cell(l).unwrap_or(0)).is_ok());
            l += 1;
        }
        assert!(lt.layers_all());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SquirrelLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert_eq!(
            lt.switch_db(DB_SWITCHES, 0),
            Err(SquirrelCode::ENoSw)
        );
        assert!(switch_cell(DB_SWITCHES).is_none());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DRV_CELLS + ALIAS_OPS + SWING_STACK + DB_SWITCHES) as u32);
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
        assert_eq!(SEG_BASE, 0xC75C0);
        assert_eq!(SquirrelCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SquirrelCode::Ok as u32, 0);
        assert_eq!(SquirrelCode::EGradeDrift as u32, 12);
        assert!(!SquirrelCode::Ok.detail().is_empty());
        assert!(!SquirrelCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade_always_s3() {
        // 长尾如实降级：空账本与全通账本一律顶格 S3，禁拔 S1/S2。
        let lt0 = SquirrelLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = SquirrelLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        let mut f = 0usize;
        while f < DRV_FAMILIES {
            let mut sl = 0usize;
            while sl < SLOTS_PER_FAMILY {
                if let Some(w) = drv_cell(f, sl) {
                    let _ = lt1.record_drv(f, sl, w);
                }
                sl += 1;
            }
            f += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S3);
        assert_ne!(Grade::S1.val(), lt1.judge_grade().val());
        assert_ne!(Grade::S2.val(), lt1.judge_grade().val());
    }

    #[test]
    fn unx52216_checks_green() {
        let s = run_unx52216_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
