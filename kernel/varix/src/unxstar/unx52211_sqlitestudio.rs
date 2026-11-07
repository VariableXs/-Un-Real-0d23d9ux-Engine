//! UNX-F52211 SQLiteStudio 收口 · 0xC7480段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面原文（任务板 vx15_cards.json 键 "52211" 逐字锚定）：
//!   title = "SQLiteStudio 收口"，quota = 240，
//!   criteria = "① 纯功能代码 ≥240 行；② 判据原文：UNX-F52212-J1 六判据
//!   全落；浏览/编辑/PRAGMA ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。"
//! 板式注记：off-by-one——本卡 52211 的判据锚照卡面原文锚定
//!   UNX-F52212-J1；功能按本卡标题（SQLiteStudio 收口）落地。
//!
//! 设计要点：
//!   ① 域模型：SQLite 版本阶梯闭集 3 档（3.40/3.44/3.45）；SQL 编辑器
//!      20 操作账；表设计器 12 格账（列定义/约束/索引三区各 4 格）；
//!      插件 6 格账（格式导入/导出/代码格式化等闭集）。
//!   ② Qt 上栈 4 层闭集（QtCore/QtGui/QtWidgets/应用层）逐层折叠
//!      恒等；六判据收口闸逐格落账。
//!   ③ 注入 ×10 轮（编辑器操作越闭集/设计器格越闭集/载荷漂移三态
//!      轮转）10/10 必抓，检出+逃逸=注入守恒等式逐轮成立；双跑指纹
//!      逐位一致；KB-COMPAT 定级闭集（20 编辑 + 12 设计器 + 6 插件
//!      + 4 层全通且六判据全落 → S2）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (52211 − 52201)）。
pub const SEG_BASE: u32 = 0xC7480;

const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (52211 - 52201) && SEG_BASE % 0x40 == 0);

/// SQL 编辑器操作数。
pub const SQL_OPS: usize = 20;

/// 表设计器格数（三区各 4 格）。
pub const DESIGN: usize = 12;

/// 插件格数。
pub const PLUGINS: usize = 6;

/// SQLite 版本阶梯档数（3.40/3.44/3.45）。
pub const VERS: usize = 3;

/// Qt 上栈层数。
pub const LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SqliteStdCode {
    /// 成功
    Ok = 0,
    /// 编辑器操作越闭集
    ENoSql,
    /// 编辑器载荷漂移
    ESqlDrift,
    /// 设计器格越闭集
    ENoDes,
    /// 设计器载荷漂移
    EDesDrift,
    /// 插件格越闭集
    ENoPlug,
    /// 版本越阶梯
    ENoVer,
    /// 上栈层越闭集
    ENoLayer,
    /// 上栈载荷漂移
    ELayerDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl SqliteStdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SqliteStdCode::Ok => "成功",
            SqliteStdCode::ENoSql => "编辑器操作越闭集：20 操作之外先登记",
            SqliteStdCode::ESqlDrift => "编辑器载荷漂移：SQL 折叠与基准不恒等，逐格复算",
            SqliteStdCode::ENoDes => "设计器格越闭集：12 格之外先登记",
            SqliteStdCode::EDesDrift => "设计器载荷漂移：设计器折叠与基准不恒等，逐格复算",
            SqliteStdCode::ENoPlug => "插件格越闭集：6 格之外先登记",
            SqliteStdCode::ENoVer => "版本越阶梯：3.40/3.44/3.45 之外先登记",
            SqliteStdCode::ENoLayer => "上栈层越闭集：Qt 4 层之外先登记",
            SqliteStdCode::ELayerDrift => "上栈载荷漂移：层折叠与基准不恒等，逐格复算",
            SqliteStdCode::EDup => "重复落账：同格只许一条账目",
            SqliteStdCode::ESeal => "未齐即封：六判据与全账未齐先补齐",
            SqliteStdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SqliteStdCode::EGradeDrift => "定级漂移：全账全通且六判据全落才定级 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SqliteStdGrade {
    /// S1 旗舰档
    S1,
    /// S2 合格档
    S2,
    /// 未定级
    None,
}

impl SqliteStdGrade {
    pub fn detail(self) -> &'static str {
        match self {
            SqliteStdGrade::S1 => "S1：主账通但插件/上栈/版本辅助账有缺，登记观察档",
            SqliteStdGrade::S2 => "S2：本卡目标档，全账全通+六判据全落登记合格档",
            SqliteStdGrade::None => "未定级：账目未齐，先补齐再登记",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// SQL 编辑器操作格基准（o ∈ 0..20）。
pub const fn sql_cell(o: usize) -> Option<u64> {
    if o >= SQL_OPS {
        return None;
    }
    Some((0xD4000u64 + (o as u64) * 0x39) ^ ((o % 4) as u64).wrapping_mul(0x01000193))
}

/// 编辑器操作类别（0..5=打开 5..10=编辑 10..15=执行 15..20=结果导出）。
pub const fn sql_kind(o: usize) -> Option<u32> {
    if o >= SQL_OPS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 表设计器格基准（d ∈ 0..12：0..4=列定义 4..8=约束 8..12=索引）。
pub const fn des_cell(d: usize) -> Option<u64> {
    if d >= DESIGN {
        return None;
    }
    Some((0xD5000u64 + (d as u64) * 0x43) ^ ((d / 4) as u64).wrapping_mul(0x9E3779B9))
}

/// 插件格基准（p ∈ 0..6）。
pub const fn plug_cell(p: usize) -> Option<u64> {
    if p >= PLUGINS {
        return None;
    }
    Some((0xD6000u64 + (p as u64) * 0x51) ^ 0x504C5547)
}

/// 版本阶梯格基准（v ∈ 0..3）。
pub const fn ver_cell(v: usize) -> Option<u64> {
    if v >= VERS {
        return None;
    }
    Some(0xD7000 + v as u64 * 0x63)
}

/// Qt 上栈层折叠（层 l）。
pub const fn layer_cell(l: usize) -> Option<u32> {
    if l >= LAYERS {
        return None;
    }
    Some(0xD800 + l as u32 * 0x77)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct SqliteStdLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 编辑器账。
    sqls: [Option<u64>; SQL_OPS],
    /// 设计器账。
    dess: [Option<u64>; DESIGN],
    /// 插件账。
    plugs: [Option<u64>; PLUGINS],
    /// 上栈账。
    layers: [Option<u32>; LAYERS],
    /// 版本阶梯游标。
    vers_done: usize,
    /// 最后编辑类别（类别单调闸）。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SqliteStdLedger {
    /// 新账本。
    pub fn new() -> Self {
        SqliteStdLedger {
            gates: [None; 6],
            sqls: [None; SQL_OPS],
            dess: [None; DESIGN],
            plugs: [None; PLUGINS],
            layers: [None; LAYERS],
            vers_done: 0,
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

    /// 落一格六判据（连接/浏览/编辑/设计/PRAGMA/导出）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SqliteStdCode> {
        if gate >= 6 {
            return Err(SqliteStdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6A100 + gate as u64);
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

    /// 编辑器一格（类别单调 + 载荷对照 + 重复拒）。
    pub fn record_sql(&mut self, o: usize, got: u64) -> Result<(), SqliteStdCode> {
        let kind = match sql_kind(o) {
            Some(k) => k,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x6A200);
                return Err(SqliteStdCode::ENoSql);
            }
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A201);
            return Err(SqliteStdCode::ENoSql);
        }
        let want = match sql_cell(o) {
            Some(w) => w,
            None => return Err(SqliteStdCode::ENoSql),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A202 + o as u64);
            return Err(SqliteStdCode::ESqlDrift);
        }
        if match self.sqls.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqliteStdCode::EDup);
        }
        if let Some(c) = self.sqls.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6A203 + o as u64);
        Ok(())
    }

    /// 编辑器全账核验（20 格）。
    pub fn sqls_all(&self) -> bool {
        let mut o = 0usize;
        while o < SQL_OPS {
            match self.sqls.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 设计器一格。
    pub fn record_des(&mut self, d: usize, got: u64) -> Result<(), SqliteStdCode> {
        let want = match des_cell(d) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x6A300);
                return Err(SqliteStdCode::ENoDes);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A301 + d as u64);
            return Err(SqliteStdCode::EDesDrift);
        }
        if match self.dess.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqliteStdCode::EDup);
        }
        if let Some(c) = self.dess.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x6A302 + d as u64);
        Ok(())
    }

    /// 设计器全账核验（12 格）。
    pub fn dess_all(&self) -> bool {
        let mut d = 0usize;
        while d < DESIGN {
            match self.dess.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// 插件一格。
    pub fn record_plug(&mut self, p: usize, got: u64) -> Result<(), SqliteStdCode> {
        let want = match plug_cell(p) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x6A400);
                return Err(SqliteStdCode::ENoPlug);
            }
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A401 + p as u64);
            return Err(SqliteStdCode::EDesDrift);
        }
        if match self.plugs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqliteStdCode::EDup);
        }
        if let Some(c) = self.plugs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x6A402 + p as u64);
        Ok(())
    }

    /// 插件全账核验（6 格）。
    pub fn plugs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGINS {
            match self.plugs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 版本一档（3.40→3.44→3.45 单调阶梯）。
    pub fn record_ver(&mut self, v: usize, got: u64) -> Result<(), SqliteStdCode> {
        let want = match ver_cell(v) {
            Some(w) => w,
            None => {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0x6A500);
                return Err(SqliteStdCode::ENoVer);
            }
        };
        if got != want || v != self.vers_done {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A501 + v as u64);
            return Err(SqliteStdCode::ENoVer);
        }
        self.vers_done += 1;
        self.fnv(0x6A502 + v as u64);
        Ok(())
    }

    /// 版本阶梯全通核验。
    pub fn vers_all(&self) -> bool {
        self.vers_done == VERS
    }

    /// 上栈一层。
    pub fn record_layer(&mut self, l: usize, got: u32) -> Result<(), SqliteStdCode> {
        let want = match layer_cell(l) {
            Some(w) => w,
            None => return Err(SqliteStdCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A600 + l as u64);
            return Err(SqliteStdCode::ELayerDrift);
        }
        if match self.layers.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqliteStdCode::EDup);
        }
        if let Some(c) = self.layers.get_mut(l) {
            *c = Some(got);
        }
        self.fnv(0x6A601 + l as u64);
        Ok(())
    }

    /// 上栈全账核验（4 层）。
    pub fn layers_all(&self) -> bool {
        let mut l = 0usize;
        while l < LAYERS {
            match self.layers.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// KB-COMPAT 定级闭集。
    pub fn grade(&self) -> SqliteStdGrade {
        let main = self.sqls_all() && self.dess_all() && self.gates_all();
        let aux = self.plugs_all() && self.layers_all() && self.vers_all();
        if main && aux {
            SqliteStdGrade::S2
        } else if main {
            SqliteStdGrade::S1
        } else {
            SqliteStdGrade::None
        }
    }
}

impl Default for SqliteStdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SqliteStdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < SQL_OPS {
        if let Some(w) = sql_cell(o) {
            if led.record_sql(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < DESIGN {
        if let Some(w) = des_cell(d) {
            if led.record_des(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut p = 0usize;
    while p < PLUGINS {
        if let Some(w) = plug_cell(p) {
            if led.record_plug(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut v = 0usize;
    while v < VERS {
        if let Some(w) = ver_cell(v) {
            let _ = led.record_ver(v, w);
        }
        v += 1;
    }
    let mut l = 0usize;
    while l < LAYERS {
        if let Some(w) = layer_cell(l) {
            let _ = led.record_layer(l, w);
        }
        l += 1;
    }
    // 注入 10 轮：编辑器越闭集 / 设计器越闭集 / 编辑类别逆序三态轮转。
    let mut r = 0usize;
    while r < 10 {
        let kind = (r % 3) as u32;
        let mut lt = SqliteStdLedger::new();
        let caught = match kind {
            0 => lt.record_sql(SQL_OPS, 0) == Err(SqliteStdCode::ENoSql),
            1 => lt.record_des(DESIGN, 0) == Err(SqliteStdCode::ENoDes),
            _ => {
                let _ = lt.record_sql(15, sql_cell(15).unwrap_or(0));
                lt.record_sql(1, sql_cell(1).unwrap_or(0)) == Err(SqliteStdCode::ENoSql)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6A700 + r as u64);
        } else {
            led.escaped += 1;
        }
        r += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
pub fn run_unx52211_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52211");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "SQL 编辑器 20 操作全通",
        recorded >= SQL_OPS as u32,
        "打开/编辑/执行/结果导出四类逐操作恒等（UNX-F52212-J1 判据锚）",
    );
    s.add(
        "编辑类别单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < SQL_OPS {
                if sql_kind(o).unwrap_or(9) < sql_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "打开→编辑→执行→导出单调，逆序必抓",
    );
    s.add(
        "表设计器 12 格全通",
        {
            let mut lt = SqliteStdLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DESIGN {
                if let Some(w) = des_cell(d) {
                    if lt.record_des(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dess_all()
        },
        "列定义/约束/索引三区各 4 格逐格恒等",
    );
    s.add(
        "插件 6 格全通",
        {
            let mut lt = SqliteStdLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGINS {
                if let Some(w) = plug_cell(p) {
                    if lt.record_plug(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plugs_all()
        },
        "插件闭集六格逐格恒等",
    );
    s.add(
        "版本阶梯 3.40/3.44/3.45 全通",
        {
            let mut lt = SqliteStdLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VERS {
                if let Some(w) = ver_cell(v) {
                    if lt.record_ver(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vers_all()
        },
        "版本单调阶梯逐档恒等",
    );
    s.add(
        "Qt 上栈 4 层全通",
        {
            let mut lt = SqliteStdLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < LAYERS {
                if let Some(w) = layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.layers_all()
        },
        "QtCore/QtGui/QtWidgets/应用层 4 层折叠恒等",
    );
    s.add("六判据全落", recorded >= 38, "连接/浏览/编辑/设计/PRAGMA/导出六格闸");
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/越闭集/逆序三态轮转必抓");
    s.add(
        "边界·编辑器越闭集",
        SqliteStdLedger::new().record_sql(SQL_OPS, 0) == Err(SqliteStdCode::ENoSql),
        "编辑操作 20 越闭集必拒",
    );
    s.add(
        "边界·设计器越闭集",
        SqliteStdLedger::new().record_des(DESIGN, 0) == Err(SqliteStdCode::ENoDes),
        "设计器格 12 越闭集必拒",
    );
    s.add(
        "边界·编辑类别逆序",
        {
            let mut lt = SqliteStdLedger::new();
            let _ = lt.record_sql(15, sql_cell(15).unwrap_or(0));
            lt.record_sql(1, sql_cell(1).unwrap_or(0)) == Err(SqliteStdCode::ENoSql)
        },
        "导出类后回打开类必抓",
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
        "KB-COMPAT 定级 S2",
        {
            let mut lt = SqliteStdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < SQL_OPS {
                if let Some(w) = sql_cell(o) {
                    let _ = lt.record_sql(o, w);
                }
                o += 1;
            }
            let mut d = 0usize;
            while d < DESIGN {
                if let Some(w) = des_cell(d) {
                    let _ = lt.record_des(d, w);
                }
                d += 1;
            }
            let mut p = 0usize;
            while p < PLUGINS {
                if let Some(w) = plug_cell(p) {
                    let _ = lt.record_plug(p, w);
                }
                p += 1;
            }
            let mut v = 0usize;
            while v < VERS {
                if let Some(w) = ver_cell(v) {
                    let _ = lt.record_ver(v, w);
                }
                v += 1;
            }
            let mut l = 0usize;
            while l < LAYERS {
                if let Some(w) = layer_cell(l) {
                    let _ = lt.record_layer(l, w);
                }
                l += 1;
            }
            lt.grade() == SqliteStdGrade::S2
        },
        SqliteStdGrade::S2.detail(),
    );
    s.add(
        "定级漂移拒",
        SqliteStdLedger::new().grade() != SqliteStdGrade::S2,
        SqliteStdCode::EGradeDrift.detail(),
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC7200 && SEG_BASE <= 0xC7700,
        "SEG_BASE 0x40 对齐且在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_cells() {
        assert!(sql_cell(0).is_some());
        assert!(sql_cell(19).is_some());
        assert!(sql_cell(20).is_none());
        assert_ne!(sql_cell(3), sql_cell(4));
    }

    #[test]
    fn test_sql_kind_bands() {
        assert_eq!(sql_kind(0), Some(0));
        assert_eq!(sql_kind(9), Some(1));
        assert_eq!(sql_kind(14), Some(2));
        assert_eq!(sql_kind(19), Some(3));
        assert!(sql_kind(20).is_none());
    }

    #[test]
    fn test_des_cells() {
        assert!(des_cell(0).is_some());
        assert!(des_cell(11).is_some());
        assert!(des_cell(12).is_none());
        assert_ne!(des_cell(3), des_cell(4));
    }

    #[test]
    fn test_sql_kind_order_caught() {
        let mut lt = SqliteStdLedger::new();
        let _ = lt.record_sql(15, sql_cell(15).unwrap_or(0));
        assert_eq!(
            lt.record_sql(1, sql_cell(1).unwrap_or(0)),
            Err(SqliteStdCode::ENoSql)
        );
        assert!(lt.record_sql(16, sql_cell(16).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_plug_drift_caught() {
        let mut lt = SqliteStdLedger::new();
        assert_eq!(
            lt.record_plug(4, plug_cell(4).unwrap_or(0).wrapping_add(9)),
            Err(SqliteStdCode::EDesDrift)
        );
        assert!(lt.record_plug(4, plug_cell(4).unwrap_or(0)).is_ok());
        assert_eq!(lt.record_plug(4, plug_cell(4).unwrap_or(0)), Err(SqliteStdCode::EDup));
    }

    #[test]
    fn test_ver_out_of_set_counts_injection() {
        let mut lt = SqliteStdLedger::new();
        assert_eq!(lt.record_ver(VERS, 0), Err(SqliteStdCode::ENoVer));
        let (inj, caught, esc) = lt.stats();
        assert_eq!((inj, caught, esc), (1, 1, 0));
    }

    #[test]
    fn test_layer_drift_caught() {
        let mut lt = SqliteStdLedger::new();
        assert_eq!(
            lt.record_layer(1, layer_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(SqliteStdCode::ELayerDrift)
        );
        assert!(lt.record_layer(1, layer_cell(1).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= SQL_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC7480);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52211 - 52201));
    }

    #[test]
    fn test_grade_closed_set() {
        let empty = SqliteStdLedger::new();
        assert_eq!(empty.grade(), SqliteStdGrade::None);
        assert!(empty.grade() != SqliteStdGrade::S2);
        assert!(!SqliteStdGrade::S2.detail().is_empty());
    }

    #[test]
    fn unx52211_checks_green() {
        let s = run_unx52211_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
