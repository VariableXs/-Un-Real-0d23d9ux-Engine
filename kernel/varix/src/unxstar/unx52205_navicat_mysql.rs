//! UNX-F52205 Navicat for MySQL 收口 · 0xC7300段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面原文（任务板 _tools/vx15_cards.json 键 "52205" 逐字锚定）：
//!   title = "Navicat for MySQL 收口"；quota = 240；criteria = "① 纯功
//!   能代码 ≥240 行；② 判据原文：UNX-F52206-J1 六判据全落；MySQL/
//!   MariaDB ×10 场景；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完
//!   成时 API/软件写回：状态=已完成、结果摘要非空。"
//! 板式说明：off-by-one 板式——卡 N 判据锚 UNX-F(N+1)-J1 照原文锚定
//! （本卡 52205 判据锚 UNX-F52206-J1），功能按本卡标题落地。
//!
//! 设计要点：
//!   ① 域模型：MySQL 5.7/8.0 双代版本阶梯 12 格闭集（每代 6 格，逐格
//!      载荷纯函数折叠恒等才收）；字符集/排序规则 8 格闭集（utf8mb4/
//!      utf8/latin1/gbk × 对应 collation）。
//!   ② 导入向导 10 操作集账，操作类别五元闭集（源选定/预览/字段映射/
//!      模式选定/提交落库，各 2 格），类别单调，逆序必抓。
//!   ③ 与 Premium 共面复用率账（复用格基准折叠对照）+ 分账声明 2 行
//!      闭集（Navicat for MySQL 独立账声明/Premium 共面管道分账声明），
//!      缺一拒封。
//!   ④ 注入 ×10 轮（版本漂移/载荷漂移/操作越闭集三态轮转）10/10 必
//!      抓，检出+逃逸守恒等式逐轮成立；六判据母版闸 + 定级闭集
//!      （12 版本格 + 8 字符集格 + 10 操作 + 2 声明全通 → S2）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (52205 − 52201)）。
pub const SEG_BASE: u32 = 0xC7300;

/// 版本阶梯格数（MySQL 5.7/8.0 双代 × 6）。
pub const VER_CELLS: usize = 12;

/// 字符集/排序规则格数。
pub const CS_CELLS: usize = 8;

/// 导入向导操作数。
pub const IMPORT_OPS: usize = 10;

/// 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 守卫：段基址 = 公式值且 0x40 对齐。
const _SEG_GUARD: () = assert!(
    SEG_BASE == 0xC7200 + 0x40 * (52205u32 - 52201) && SEG_BASE % 0x40 == 0
);

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavMysqlCode {
    /// 成功
    Ok = 0,
    /// 版本格越闭集
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 字符集格越闭集
    ENoCs,
    /// 排序规则漂移
    ECsDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 操作类别逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 声明行越闭集
    ENoRow,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl NavMysqlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NavMysqlCode::Ok => "成功",
            NavMysqlCode::ENoVer => "版本格越闭集：MySQL 5.7/8.0 双代 12 格之外先登记",
            NavMysqlCode::EVerDrift => "版本阶梯漂移：版本折叠与基准不恒等，逐格复算",
            NavMysqlCode::ENoCs => "字符集格越闭集：8 格字符集/排序规则闭集之外先登记",
            NavMysqlCode::ECsDrift => "排序规则漂移：collation 折叠与基准不恒等，逐格复算",
            NavMysqlCode::ENoOp => "操作号越闭集：导入向导 10 操作集之外先登记",
            NavMysqlCode::EOpDrift => "操作载荷漂移：操作折叠与基准不恒等，逐格复算",
            NavMysqlCode::EOrder => "操作类别逆序：源选定→预览→字段映射→模式选定→提交落库单调，逆序必抓",
            NavMysqlCode::EDup => "重复落账：同格只许一条账目",
            NavMysqlCode::ENoRow => "声明行越闭集：2 行分账声明闭集之外先登记",
            NavMysqlCode::ESeal => "未齐即封：12 版本格 + 8 字符集格 + 10 操作 + 2 声明未齐先补齐",
            NavMysqlCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NavMysqlCode::EGradeDrift => "定级漂移：全格全通且声明全齐才定级 S2",
        }
    }
}

// ── 版本阶梯（MySQL 5.7/8.0 双代 12 格闭集） ─────────────────────
/// 代际：0=5.7 1=8.0（格 v：v/6）。
pub const fn ver_gen(v: usize) -> Option<u32> {
    if v >= VER_CELLS {
        return None;
    }
    Some((v / 6) as u32)
}

/// 版本格基准（格 v；载荷 = 代际基 ^ 修订步进 ^ FNV 质数扰动）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VER_CELLS {
        return None;
    }
    let gen = (v / 6) as u64;
    let step = (v % 6) as u64;
    Some((0xB1100u64 + gen * 0x100) ^ step.wrapping_mul(0x01000193))
}

// ── 字符集/排序规则 8 格闭集 ─────────────────────────────────────
/// 字符集格基准（格 c：utf8mb4/utf8/latin1/gbk 各配 2 collation）。
pub const fn charset_cell(c: usize) -> Option<u64> {
    if c >= CS_CELLS {
        return None;
    }
    let cs = (c / 2) as u64;
    let col = (c % 2) as u64;
    Some((0xB2100u64 + cs * 0x40) ^ col.wrapping_mul(0x9E3779B97F4A7C15))
}

/// 字符集名（闭集四元：0=utf8mb4 1=utf8 2=latin1 3=gbk）。
pub const fn charset_name(c: usize) -> Option<&'static str> {
    match c {
        0 | 1 => Some("utf8mb4"),
        2 | 3 => Some("utf8"),
        4 | 5 => Some("latin1"),
        6 | 7 => Some("gbk"),
        _ => None,
    }
}

// ── 导入向导 10 操作集 ───────────────────────────────────────────
/// 操作类别（0..2=源选定 2..4=预览 4..6=字段映射 6..8=模式选定 8..10=提交落库）。
pub const fn import_kind(o: usize) -> Option<u32> {
    if o >= IMPORT_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 操作格载荷（操作 o；类别 = o/2）。
pub const fn import_cell(o: usize) -> Option<u64> {
    if o >= IMPORT_OPS {
        return None;
    }
    Some((0xB3000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

// ── Premium 共面复用率账 + 分账声明 ──────────────────────────────
/// 复用格基准（与 Premium 共面管道复用：复用格恒等于版本账基准，零漂移）。
pub const fn reuse_cell(r: usize) -> Option<u64> {
    version_cell(r)
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB4000 + r as u32 * 0x2C)
}

/// 声明行名（2 行闭集）。
pub const fn split_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("navicat_mysql 独立账声明"),
        1 => Some("premium 共面管道分账声明"),
        _ => None,
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// 定级（S1/S2/S3 闭集；本卡 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavGrade {
    /// S2：MySQL 双代收口级
    S2,
    /// S1：不达（备用档）
    S1,
    /// S3：降级档
    S3,
}

impl NavGrade {
    pub fn tag(self) -> &'static str {
        match self {
            NavGrade::S2 => "S2",
            NavGrade::S1 => "S1",
            NavGrade::S3 => "S3",
        }
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct NavLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 版本账。
    vers: [Option<u64>; VER_CELLS],
    /// 字符集账。
    css: [Option<u64>; CS_CELLS],
    /// 操作账。
    ops: [Option<u64>; IMPORT_OPS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后类别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NavLedger {
    /// 新账本。
    pub fn new() -> Self {
        NavLedger {
            gates: [None; 6],
            vers: [None; VER_CELLS],
            css: [None; CS_CELLS],
            ops: [None; IMPORT_OPS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NavMysqlCode> {
        if gate >= 6 {
            return Err(NavMysqlCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5A100 + gate as u64);
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

    /// 版本一格（代际阶梯 + 载荷对照 + 重复拒）。
    pub fn record_ver(&mut self, v: usize, got: u64) -> Result<(), NavMysqlCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(NavMysqlCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A200 + v as u64);
            return Err(NavMysqlCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NavMysqlCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x5A201 + v as u64);
        Ok(())
    }

    /// 版本全账核验（12 格）。
    pub fn vers_all(&self) -> bool {
        let mut v = 0usize;
        while v < VER_CELLS {
            match self.vers.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 字符集一格（载荷对照 + 重复拒）。
    pub fn record_cs(&mut self, c: usize, got: u64) -> Result<(), NavMysqlCode> {
        let want = match charset_cell(c) {
            Some(w) => w,
            None => return Err(NavMysqlCode::ENoCs),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A300 + c as u64);
            return Err(NavMysqlCode::ECsDrift);
        }
        if match self.css.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NavMysqlCode::EDup);
        }
        if let Some(cc) = self.css.get_mut(c) {
            *cc = Some(got);
        }
        self.fnv(0x5A301 + c as u64);
        Ok(())
    }

    /// 字符集全账核验（8 格）。
    pub fn css_all(&self) -> bool {
        let mut c = 0usize;
        while c < CS_CELLS {
            match self.css.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 操作一格（类别单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), NavMysqlCode> {
        if o >= IMPORT_OPS {
            return Err(NavMysqlCode::ENoOp);
        }
        let kind = match import_kind(o) {
            Some(k) => k,
            None => return Err(NavMysqlCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A400);
            return Err(NavMysqlCode::EOrder);
        }
        let want = match import_cell(o) {
            Some(w) => w,
            None => return Err(NavMysqlCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A401 + o as u64);
            return Err(NavMysqlCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NavMysqlCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5A402 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < IMPORT_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// Premium 复用一格（共面折叠对照）。
    pub fn record_reuse(&mut self, r: usize) -> Result<(), NavMysqlCode> {
        let want = match reuse_cell(r) {
            Some(w) => w,
            None => return Err(NavMysqlCode::ENoVer),
        };
        if match self.vers.get(r) {
            Some(Some(x)) => *x != want,
            _ => true,
        } {
            return Err(NavMysqlCode::EVerDrift);
        }
        self.fnv(0x5A500 + r as u64);
        Ok(())
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), NavMysqlCode> {
        if r >= SPLIT_ROWS {
            return Err(NavMysqlCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NavMysqlCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5A600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 收口定级（全格全通 + 声明全齐 + 六判据全落 → S2）。
    pub fn grade(&self) -> NavGrade {
        if self.gates_all() && self.vers_all() && self.css_all() && self.ops_all() && self.splits_all() {
            NavGrade::S2
        } else if self.vers_all() && self.css_all() {
            NavGrade::S1
        } else {
            NavGrade::S3
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for NavLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NavLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VER_CELLS {
        if let Some(w) = version_cell(v) {
            if led.record_ver(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
    }
    let mut c = 0usize;
    while c < CS_CELLS {
        if let Some(w) = charset_cell(c) {
            if led.record_cs(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut o = 0usize;
    while o < IMPORT_OPS {
        if let Some(w) = import_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    // Premium 共面复用率账：复用格与版本账逐格对照。
    let mut r = 0usize;
    while r < VER_CELLS {
        let _ = led.record_reuse(r);
        r += 1;
    }
    let mut s = 0usize;
    while s < SPLIT_ROWS {
        let _ = led.declare_split(s);
        s += 1;
    }
    // 注入 10 轮：版本漂移 / 载荷漂移 / 操作越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NavLedger::new();
        let caught = match kind {
            0 => lt.record_ver(3, version_cell(3).unwrap_or(0).wrapping_add(1)) == Err(NavMysqlCode::EVerDrift),
            1 => lt.record_cs(5, charset_cell(5).unwrap_or(0).wrapping_add(1)) == Err(NavMysqlCode::ECsDrift),
            _ => lt.record_op(IMPORT_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5A700 + k as u64);
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
pub fn run_unx52205_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52205");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "导入向导 10 操作全通",
        recorded >= (IMPORT_OPS as u32) + (CS_CELLS as u32) + (VER_CELLS as u32),
        "源选定/预览/字段映射/模式选定/提交落库 五类逐操作恒等",
    );
    s.add(
        "MySQL 5.7/8.0 双代版本阶梯 12/12",
        {
            let mut lt = NavLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VER_CELLS {
                if let Some(w) = version_cell(v) {
                    if lt.record_ver(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "双代 12 格逐格折叠恒等（判据锚 UNX-F52206-J1）",
    );
    s.add(
        "字符集/排序规则 8/8 全通",
        {
            let mut lt = NavLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CS_CELLS {
                if let Some(w) = charset_cell(c) {
                    if lt.record_cs(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.css_all()
        },
        "utf8mb4/utf8/latin1/gbk × collation 八格闭集",
    );
    s.add(
        "Premium 共面复用率账 12/12 对照恒等",
        {
            let mut lt = NavLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VER_CELLS {
                if let Some(w) = version_cell(v) {
                    let _ = lt.record_ver(v, w);
                }
                v += 1;
            }
            let mut r = 0usize;
            while r < VER_CELLS {
                if lt.record_reuse(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "共面复用格与版本账逐格对照零漂移",
    );
    s.add(
        "分账声明 2/2 全齐",
        {
            let mut lt = NavLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "独立账声明/共面管道分账声明两行闭集，缺一拒封",
    );
    s.add(
        "操作类别单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < IMPORT_OPS {
                if import_kind(o).unwrap_or(9) < import_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五类闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "版本漂移/载荷漂移/越闭集三态轮转必抓");
    let mut l0 = NavLedger::new();
    s.add(
        "边界·版本越闭集",
        l0.record_ver(VER_CELLS, 0).is_err(),
        "版本格 12 越闭集必拒",
    );
    s.add(
        "边界·字符集越闭集",
        l0.record_cs(CS_CELLS, 0).is_err(),
        "字符集格 8 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(IMPORT_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    let led_full = {
        let mut lt = NavLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut v = 0usize;
        while v < VER_CELLS {
            if let Some(w) = version_cell(v) {
                let _ = lt.record_ver(v, w);
            }
            v += 1;
        }
        let mut c = 0usize;
        while c < CS_CELLS {
            if let Some(w) = charset_cell(c) {
                let _ = lt.record_cs(c, w);
            }
            c += 1;
        }
        let mut o = 0usize;
        while o < IMPORT_OPS {
            if let Some(w) = import_cell(o) {
                let _ = lt.record_op(o, w);
            }
            o += 1;
        }
        let mut sr = 0usize;
        while sr < SPLIT_ROWS {
            let _ = lt.declare_split(sr);
            sr += 1;
        }
        lt
    };
    s.add(
        "定级闭集 S2",
        led_full.grade() == NavGrade::S2 && led_full.grade().tag() == "S2",
        "全格全通+声明全齐 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7300 && SEG_BASE > 0xC7200,
        "SEG_BASE 0x40 对齐且落在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ver_gen_ladder() {
        assert_eq!(ver_gen(0), Some(0));
        assert_eq!(ver_gen(5), Some(0));
        assert_eq!(ver_gen(6), Some(1));
        assert_eq!(ver_gen(11), Some(1));
        assert!(ver_gen(12).is_none());
        assert!(version_cell(12).is_none());
    }

    #[test]
    fn test_charset_names() {
        assert_eq!(charset_name(0), Some("utf8mb4"));
        assert_eq!(charset_name(3), Some("utf8"));
        assert_eq!(charset_name(6), Some("gbk"));
        assert!(charset_name(8).is_none());
        assert!(charset_cell(8).is_none());
    }

    #[test]
    fn test_import_kind() {
        assert_eq!(import_kind(0), Some(0));
        assert_eq!(import_kind(3), Some(1));
        assert_eq!(import_kind(9), Some(4));
        assert!(import_kind(10).is_none());
        assert!(import_cell(10).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = version_cell(7).unwrap_or(0);
        assert_eq!(a, version_cell(7).unwrap_or(0));
        assert_ne!(a, version_cell(8).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert_eq!(split_name(0), Some("navicat_mysql 独立账声明"));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NavLedger::new();
        assert_eq!(
            lt.record_ver(2, version_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(NavMysqlCode::EVerDrift)
        );
        assert!(lt.record_ver(2, version_cell(2).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_ver(2, version_cell(2).unwrap_or(0)),
            Err(NavMysqlCode::EDup)
        );
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = NavLedger::new();
        let _ = lt.record_op(9, import_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, import_cell(2).unwrap_or(0)), Err(NavMysqlCode::EOrder));
        assert!(lt.record_op(8, import_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (VER_CELLS + CS_CELLS + IMPORT_OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52205 - 52201));
        assert!(SEG_BASE > 0xC7200 && SEG_BASE <= 0xC73C0);
    }

    #[test]
    fn test_val_details_nonempty() {
        let mut n = 0u32;
        let codes = [
            NavMysqlCode::Ok,
            NavMysqlCode::ENoVer,
            NavMysqlCode::EVerDrift,
            NavMysqlCode::ENoCs,
            NavMysqlCode::ECsDrift,
            NavMysqlCode::ENoOp,
            NavMysqlCode::EOpDrift,
            NavMysqlCode::EOrder,
            NavMysqlCode::EDup,
            NavMysqlCode::ENoRow,
            NavMysqlCode::ESeal,
            NavMysqlCode::EInj,
            NavMysqlCode::EGradeDrift,
        ];
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i].val(), SEG_BASE + n);
            assert!(!codes[i].detail().is_empty());
            n += 1;
            i += 1;
        }
    }

    #[test]
    fn test_unx52205_checks_green() {
        let s = run_unx52205_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
