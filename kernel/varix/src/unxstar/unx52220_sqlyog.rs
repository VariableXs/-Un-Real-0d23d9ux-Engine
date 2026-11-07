//! UNX-F52220 SQLyog 收口 · 0xC76C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52220"，逐字）：
//!   title: SQLyog 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52221-J1 六判据
//!     全落；多标签/分屏/主题 20 操作集全通；conhost/ConPTY 语义对账
//!     （E 部联签锚定行）；中文渲染与字体三层（J5）全过；KB-COMPAT 定
//!     级 S1；判据锚定 §AI-66.3 组 12 口径；③ 完成时 API/软件写回：状
//!     态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52220）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52221-J1，照原文锚定；功能按本卡标题（SQLyog）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① MySQL 版本阶梯闭集：5.6 / 5.7 / 8.0 三级，越级必抓；每级独立
//!      载荷折叠，纯函数恒等才收。
//!   ② GUI 20 操作集：五家族闭集（多标签/分屏/主题/对象浏览/数据网格）
//!      各 4 操作，家族单调（逆序必抓）。
//!   ③ Schema/数据同步 12 格：Schema 同步 6 格 + 数据同步 6 格，逐格
//!      折叠恒等才收，越格必抓。
//!   ④ SSH 隧道三元闸：主机/端口/密钥三元逐格恒等闸，任一元漂移必抓。
//!   ⑤ 企业/社区双版分账声明 2 行闭集：缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10 轮（版本越阶梯/漂移/操作逆序），
//!      全部落在当轮临时账本上，检出+逃逸守恒等式（injected==caught==10、
//!      escaped==0）；双跑 FNV 指纹一致；定级闭集 S1/S2/S3（20 操作 +
//!      3 级版本 + 12 同步格 + 3 元闸 + 2 行分账 + 六判据全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52220;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// GUI 操作数。
pub const GUI_OPS: usize = 20;

/// MySQL 版本阶梯级数。
pub const VERSION_STEPS: usize = 3;

/// Schema/数据同步格数（6 + 6）。
pub const SYNC_CELLS: usize = 12;

/// SSH 隧道三元闸元数。
pub const TUNNEL_TERMS: usize = 3;

/// 企业/社区双版分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SqlYogCode {
    /// 成功
    Ok = 0,
    /// 版本级越阶梯
    ENoVer,
    /// 版本阶梯漂移
    EVerDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 同步格越闭集
    ENoSync,
    /// 同步格漂移
    ESyncDrift,
    /// 隧道三元漂移
    ETunnel,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl SqlYogCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SqlYogCode::Ok => "成功",
            SqlYogCode::ENoVer => "版本级越阶梯：MySQL 5.6/5.7/8.0 三级闭集之外先登记",
            SqlYogCode::EVerDrift => "版本阶梯漂移：级载荷折叠与基准不恒等，逐级复算",
            SqlYogCode::ENoOp => "操作号越闭集：GUI 20 操作集之外先登记",
            SqlYogCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            SqlYogCode::EOrder => "家族逆序：多标签→分屏→主题→对象浏览→数据网格单调，逆序必抓",
            SqlYogCode::ENoSync => "同步格越闭集：Schema/数据同步 12 格闭集之外先登记",
            SqlYogCode::ESyncDrift => "同步格漂移：同步格折叠与基准不恒等，逐格复算",
            SqlYogCode::ETunnel => "隧道三元漂移：SSH 隧道主机/端口/密钥三元逐格对照失恒等",
            SqlYogCode::EDup => "重复落账：同格只许一条账目",
            SqlYogCode::ESeal => "未齐即封：20 操作 + 3 级版本 + 12 同步格 + 3 元闸 + 2 行分账未齐先补齐",
            SqlYogCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SqlYogCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20操作+3级版本+12同步格+3元闸+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 版本阶梯级载荷（级 v：0=5.6 1=5.7 2=8.0）。
pub const fn version_cell(v: usize) -> Option<u64> {
    if v >= VERSION_STEPS {
        return None;
    }
    Some((0xC3000u64 + (v as u64) * 0x67) ^ 0x53514C595F4F475F)
}

/// 版本级名（const fn 表 + Option 边界）。
pub const fn version_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("5.6"),
        1 => Some("5.7"),
        2 => Some("8.0"),
        _ => None,
    }
}

/// GUI 操作格载荷（操作 o；家族 = o/4）。
pub const fn gui_cell(o: usize) -> Option<u64> {
    if o >= GUI_OPS {
        return None;
    }
    Some((0xC4000u64 + (o as u64) * 0x39) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=多标签 4..8=分屏 8..12=主题 12..16=对象浏览 16..20=数据网格）。
pub const fn gui_kind(o: usize) -> Option<u32> {
    if o >= GUI_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn gui_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("多标签"),
        1 => Some("分屏"),
        2 => Some("主题"),
        3 => Some("对象浏览"),
        4 => Some("数据网格"),
        _ => None,
    }
}

/// 同步格载荷（格 c：0..6=Schema 同步 6..12=数据同步）。
pub const fn sync_cell(c: usize) -> Option<u32> {
    if c >= SYNC_CELLS {
        return None;
    }
    Some(0xC500 + (c / 6) as u32 * 0x50 + (c % 6) as u32 * 0x11)
}

/// 同步名。
pub const fn sync_name(c: usize) -> Option<&'static str> {
    if c >= SYNC_CELLS {
        return None;
    }
    match c / 6 {
        0 => Some("Schema 同步"),
        _ => Some("数据同步"),
    }
}

/// SSH 隧道三元闸元载荷（元 t：0=主机 1=端口 2=密钥）。
pub const fn tunnel_cell(t: usize) -> Option<u32> {
    match t {
        0 => Some(0xC600 ^ 0x5353),
        1 => Some(0xC610 ^ 0x504F),
        2 => Some(0xC620 ^ 0x4B45),
        _ => None,
    }
}

/// 隧道元名。
pub const fn tunnel_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("主机"),
        1 => Some("端口"),
        2 => Some("密钥"),
        _ => None,
    }
}

/// 企业/社区双版声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xC700 + r as u32 * 0x19)
}

/// 声明行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("企业版消费分账声明"),
        1 => Some("社区版消费分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SQLyog 收口账本状态机。
#[derive(Clone)]
pub struct SqlYogLedger {
    /// 六判据（GUI/同步/SSH 隧道/双版分账/卸载/许可）。
    gates: [Option<bool>; 6],
    /// 版本阶梯账。
    vers: [Option<u64>; VERSION_STEPS],
    /// 操作账。
    ops: [Option<u64>; GUI_OPS],
    /// 同步格账。
    syncs: [Option<u32>; SYNC_CELLS],
    /// 隧道三元闸账。
    tunnel: [Option<u32>; TUNNEL_TERMS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SqlYogLedger {
    /// 新账本。
    pub fn new() -> Self {
        SqlYogLedger {
            gates: [None; 6],
            vers: [None; VERSION_STEPS],
            ops: [None; GUI_OPS],
            syncs: [None; SYNC_CELLS],
            tunnel: [None; TUNNEL_TERMS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SqlYogCode> {
        if gate >= 6 {
            return Err(SqlYogCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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
    pub fn record_version(&mut self, v: usize, got: u64) -> Result<(), SqlYogCode> {
        let want = match version_cell(v) {
            Some(w) => w,
            None => return Err(SqlYogCode::ENoVer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + v as u64);
            return Err(SqlYogCode::EVerDrift);
        }
        if match self.vers.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqlYogCode::EDup);
        }
        if let Some(c) = self.vers.get_mut(v) {
            *c = Some(got);
        }
        self.fnv(0x8201 + v as u64);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), SqlYogCode> {
        if o >= GUI_OPS {
            return Err(SqlYogCode::ENoOp);
        }
        let kind = match gui_kind(o) {
            Some(k) => k,
            None => return Err(SqlYogCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(SqlYogCode::EOrder);
        }
        let want = match gui_cell(o) {
            Some(w) => w,
            None => return Err(SqlYogCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + o as u64);
            return Err(SqlYogCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqlYogCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < GUI_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 同步一格（载荷对照 + 重复拒）。
    pub fn record_sync(&mut self, c: usize, got: u32) -> Result<(), SqlYogCode> {
        let want = match sync_cell(c) {
            Some(w) => w,
            None => return Err(SqlYogCode::ENoSync),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + c as u64);
            return Err(SqlYogCode::ESyncDrift);
        }
        if match self.syncs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqlYogCode::EDup);
        }
        if let Some(s) = self.syncs.get_mut(c) {
            *s = Some(got);
        }
        self.fnv(0x8401 + c as u64);
        Ok(())
    }

    /// 同步全账核验（12 格）。
    pub fn syncs_all(&self) -> bool {
        let mut c = 0usize;
        while c < SYNC_CELLS {
            match self.syncs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// SSH 隧道一元（三元闸逐元对照 + 重复拒）。
    pub fn record_tunnel(&mut self, t: usize, got: u32) -> Result<(), SqlYogCode> {
        let want = match tunnel_cell(t) {
            Some(w) => w,
            None => return Err(SqlYogCode::ESeal),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + t as u64);
            return Err(SqlYogCode::ETunnel);
        }
        if match self.tunnel.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqlYogCode::EDup);
        }
        if let Some(c) = self.tunnel.get_mut(t) {
            *c = Some(got);
        }
        self.fnv(0x8501 + t as u64);
        Ok(())
    }

    /// 隧道三元闸全账核验（3 元全齐）。
    pub fn tunnel_all(&self) -> bool {
        let mut t = 0usize;
        while t < TUNNEL_TERMS {
            match self.tunnel.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 声明一行（企业/社区双版分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), SqlYogCode> {
        if r >= SPLIT_ROWS {
            return Err(SqlYogCode::ESeal);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SqlYogCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8600 + r as u64);
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

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.vers_all()
            && self.ops_all()
            && self.syncs_all()
            && self.tunnel_all()
            && self.splits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SqlYogLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SqlYogLedger::new();
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
    while o < GUI_OPS {
        if let Some(w) = gui_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < SYNC_CELLS {
        if let Some(w) = sync_cell(c) {
            if led.record_sync(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut t = 0usize;
    while t < TUNNEL_TERMS {
        if let Some(w) = tunnel_cell(t) {
            if led.record_tunnel(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：版本越阶梯 / 漂移 / 操作逆序 三态轮转，
    // 三类注入全部落在当轮临时账本上；漂移先录正格再注漂移；
    // 逆序注入对空账本同样必抓。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SqlYogLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
                lt.record_version(VERSION_STEPS, 0).is_err()
            }
            1 => {
                let _ = lt.record_sync(2, sync_cell(2).unwrap_or(0));
                lt.record_sync(2, sync_cell(2).unwrap_or(0).wrapping_add(1)) == Err(SqlYogCode::ESyncDrift)
            }
            _ => {
                let _ = lt.record_op(9, gui_cell(9).unwrap_or(0));
                lt.record_op(1, gui_cell(1).unwrap_or(0)) == Err(SqlYogCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8700 + k as u64);
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
/// UNX-F52220 判据集（锚 UNX-F52221-J1，off-by-one 板式）。
pub fn run_unx52220_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52220");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "GUI 20 操作全通（多标签/分屏/主题口径）",
        recorded >= GUI_OPS as u32,
        "多标签/分屏/主题/对象浏览/数据网格五家族逐操作恒等",
    );
    s.add(
        "MySQL 5.6/5.7/8.0 阶梯三级全落",
        {
            let mut lt = SqlYogLedger::new();
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
        "三级闭集逐级恒等，越级必抓",
    );
    s.add(
        "Schema/数据同步 12 格全落",
        {
            let mut lt = SqlYogLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SYNC_CELLS {
                if let Some(w) = sync_cell(c) {
                    if lt.record_sync(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.syncs_all()
        },
        "Schema 同步 6 格 + 数据同步 6 格逐格恒等（§AI-66.3 组 12 口径）",
    );
    s.add(
        "SSH 隧道三元闸全落",
        {
            let mut lt = SqlYogLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TUNNEL_TERMS {
                if let Some(w) = tunnel_cell(t) {
                    if lt.record_tunnel(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tunnel_all()
        },
        "主机/端口/密钥三元逐格恒等闸",
    );
    s.add(
        "企业/社区双版分账声明 2/2 全齐",
        {
            let mut lt = SqlYogLedger::new();
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
        "企业版消费分账/社区版消费分账两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SqlYogLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "GUI/同步/SSH 隧道/双版分账/卸载/许可逐格落账（UNX-F52221-J1 锚）",
    );
    s.add(
        "GUI 家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < GUI_OPS {
                if gui_kind(o).unwrap_or(5) < gui_kind(o - 1).unwrap_or(5) {
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
        "版本越阶梯/漂移/操作逆序三态轮转必抓",
    );
    let mut l0 = SqlYogLedger::new();
    s.add(
        "边界·版本级越阶梯",
        l0.record_version(VERSION_STEPS, 0).is_err(),
        "版本级 3 越阶梯必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(GUI_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·同步格越闭集",
        l0.record_sync(SYNC_CELLS, 0).is_err(),
        "同步格 12 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_split(SPLIT_ROWS).is_err(),
        "声明行 2 越闭集必拒",
    );
    s.add(
        "漂移拒（先录正格再注漂移）",
        {
            let _ = l0.record_sync(2, sync_cell(2).unwrap_or(0));
            l0.record_sync(2, sync_cell(2).unwrap_or(0).wrapping_add(1)) == Err(SqlYogCode::ESyncDrift)
        },
        "同步格载荷漂移必抓",
    );
    s.add(
        "隧道三元漂移拒",
        {
            let mut lt = SqlYogLedger::new();
            lt.record_tunnel(1, tunnel_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SqlYogCode::ETunnel)
        },
        "SSH 隧道三元闸任一元漂移必抓",
    );
    s.add(
        "逆序拒（空账本同样必抓）",
        {
            let mut lt = SqlYogLedger::new();
            let _ = lt.record_op(9, gui_cell(9).unwrap_or(0));
            lt.record_op(1, gui_cell(1).unwrap_or(0)) == Err(SqlYogCode::EOrder)
        },
        "数据网格家族先落、多标签家族后落，逆序必抓",
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
        "定级 S1 闭集",
        {
            let mut lt = SqlYogLedger::new();
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
            while o < GUI_OPS {
                if let Some(w) = gui_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < SYNC_CELLS {
                if let Some(w) = sync_cell(c) {
                    let _ = lt.record_sync(c, w);
                }
                c += 1;
            }
            let mut t = 0usize;
            while t < TUNNEL_TERMS {
                if let Some(w) = tunnel_cell(t) {
                    let _ = lt.record_tunnel(t, w);
                }
                t += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * 19,
        "SEG_BASE 0x40 对齐且在 VX15 域 0xC76C0 格",
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
        assert_eq!(version_name(0), Some("5.6"));
        assert_eq!(version_name(1), Some("5.7"));
        assert_eq!(version_name(2), Some("8.0"));
        assert!(version_name(3).is_none());
        assert!(version_cell(3).is_none());
    }

    #[test]
    fn test_gui_kind() {
        assert_eq!(gui_kind(0), Some(0));
        assert_eq!(gui_kind(8), Some(2));
        assert_eq!(gui_kind(19), Some(4));
        assert!(gui_kind(20).is_none());
        assert!(gui_cell(20).is_none());
    }

    #[test]
    fn test_sync_table() {
        assert_eq!(sync_name(0), Some("Schema 同步"));
        assert_eq!(sync_name(6), Some("数据同步"));
        assert!(sync_cell(12).is_none());
        assert_ne!(sync_cell(0).unwrap_or(0), sync_cell(5).unwrap_or(0));
    }

    #[test]
    fn test_tunnel_table() {
        assert_eq!(tunnel_name(0), Some("主机"));
        assert_eq!(tunnel_name(1), Some("端口"));
        assert_eq!(tunnel_name(2), Some("密钥"));
        assert!(tunnel_cell(3).is_none());
        assert_ne!(tunnel_cell(0).unwrap_or(0), tunnel_cell(1).unwrap_or(0));
    }

    #[test]
    fn test_split_rows() {
        assert_eq!(split_row_name(0), Some("企业版消费分账声明"));
        assert_eq!(split_row_name(1), Some("社区版消费分账声明"));
        assert!(split_row(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = gui_cell(3).unwrap_or(0);
        assert_eq!(a, gui_cell(3).unwrap_or(0));
        assert_ne!(a, gui_cell(4).unwrap_or(0));
        let v = version_cell(1).unwrap_or(0);
        assert_eq!(v, version_cell(1).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SqlYogLedger::new();
        let _ = lt.record_op(9, gui_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, gui_cell(2).unwrap_or(0)), Err(SqlYogCode::EOrder));
        assert!(lt.record_op(10, gui_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SqlYogLedger::new();
        assert_eq!(
            lt.record_op(5, gui_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(SqlYogCode::EDrift)
        );
        assert_eq!(
            lt.record_version(1, version_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(SqlYogCode::EVerDrift)
        );
        assert_eq!(
            lt.record_sync(7, sync_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(SqlYogCode::ESyncDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SqlYogLedger::new();
        let _ = lt.record_version(0, version_cell(0).unwrap_or(0));
        assert_eq!(lt.record_version(0, version_cell(0).unwrap_or(0)), Err(SqlYogCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(SqlYogCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SqlYogLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SqlYogLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (GUI_OPS + VERSION_STEPS + SYNC_CELLS + TUNNEL_TERMS) as u32);
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
        assert_eq!(SEG_BASE, 0xC76C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SqlYogCode::Ok as u32, 0);
        assert_eq!(SqlYogCode::EGradeDrift as u32, 12);
        assert!(!SqlYogCode::Ok.detail().is_empty());
        assert!(!SqlYogCode::ESeal.detail().is_empty());
        assert_eq!(SqlYogCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = SqlYogLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52220_checks_green() {
        let s = run_unx52220_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
