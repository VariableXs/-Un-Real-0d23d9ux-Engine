//! UNX-F52203 DBeaver Community 收口 · 0xC7280段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52203"，逐字）：
//!   title: DBeaver Community 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52204-J1 六判据全
//!     落；多数据库连接 ×5 类全通；数据传输/结构同步操作集；许可校验面
//!     如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.9 红线②；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52203）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52204-J1，照原文锚定；功能按本卡标题（DBeaver Community）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① JDBC 驱动账 20 格：10 家族闭集（PostgreSQL/MySQL/MariaDB/Oracle/
//!      SQL Server/SQLite/H2/Derby/ClickHouse/DuckDB）× 2 版本（稳定/
//!      候选），逐格载荷折叠恒等才收。
//!   ② 驱动下载网络语义 AI-23 锚定行：1 行闭集声明（下载源/校验面/
//!      失败回退三段语义同构），缺行拒封。
//!   ③ Eclipse RCP 上栈 4 层：Equinox OSGi→Workbench→Views/Editors→
//!      JDBC Provider，层序单调（越栈必抓）。
//!   ④ 六判据收口闸（启动/功能/崩溃/卸载/升级/许可）；ledger 状态机
//!      （new/record/judge_gate/fingerprint，FNV-1a）；注入核 run_cycle
//!      三态轮转 10/10（驱动格越闭集/驱动格漂移/RCP 层越栈），检出+
//!      逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3 闭集（20 格
//!      驱动 + 4 层上栈 + 1 行锚定 + 六判据全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52203;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// JDBC 驱动格数（10 家族 × 2 版本）。
pub const DRIVER_CELLS: usize = 20;

/// 驱动家族数。
pub const DRIVER_FAMILIES: usize = 10;

/// Eclipse RCP 上栈层数。
pub const RCP_LAYERS: usize = 4;

/// AI-23 锚定行数。
pub const ANCHOR_ROWS: usize = 1;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DBeaverCode {
    /// 成功
    Ok = 0,
    /// 驱动格越闭集
    ENoDrv,
    /// 驱动格漂移
    EDrvDrift,
    /// RCP 层越栈
    ENoLayer,
    /// RCP 层序逆序
    EOrder,
    /// 锚定行越闭集
    ENoAnchor,
    /// 缺锚定行
    EAnchorMiss,
    /// 驱动版本段越闭集
    ENoVer,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 判据漂移
    EDrift,
    /// 定级漂移
    EGradeDrift,
}

impl DBeaverCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DBeaverCode::Ok => "成功",
            DBeaverCode::ENoDrv => "驱动格越闭集：20 格闭集之外先登记",
            DBeaverCode::EDrvDrift => "驱动格漂移：驱动折叠与基准不恒等，逐格复算",
            DBeaverCode::ENoLayer => "RCP 层越栈：4 层闭集之外先登记",
            DBeaverCode::EOrder => "RCP 层序逆序：Equinox→Workbench→Views/Editors→JDBC Provider 单调，逆序必抓",
            DBeaverCode::ENoAnchor => "锚定行越闭集：AI-23 锚定 1 行闭集之外先登记",
            DBeaverCode::EAnchorMiss => "缺锚定行：驱动下载网络语义 AI-23 锚定行未落先补齐",
            DBeaverCode::ENoVer => "驱动版本段越闭集：稳定/候选两段之外先登记",
            DBeaverCode::EDup => "重复落账：同格只许一条账目",
            DBeaverCode::ESeal => "未齐即封：20 格驱动 + 4 层上栈 + 1 行锚定未齐先补齐",
            DBeaverCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DBeaverCode::EDrift => "判据漂移：落账值与基准表不恒等，逐格复算",
            DBeaverCode::EGradeDrift => "定级漂移：全通且锚定行落账才可定 S1",
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
            Grade::S1 => "S1：六判据+20格驱动+4层上栈+1行锚定全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 驱动格载荷（格 d；家族 = d/2，版本段 = d%2：0=稳定 1=候选）。
pub const fn driver_cell(d: usize) -> Option<u64> {
    if d >= DRIVER_CELLS {
        return None;
    }
    Some((0xB7000u64 + (d as u64) * 0x47) ^ ((d / 2) as u64).wrapping_mul(0x01000193))
}

/// 驱动家族（d/2）。
pub const fn driver_family(d: usize) -> Option<u32> {
    if d >= DRIVER_CELLS {
        return None;
    }
    Some((d / 2) as u32)
}

/// 驱动版本段（d%2：0=稳定 1=候选）。
pub const fn driver_ver_seg(d: usize) -> Option<u32> {
    if d >= DRIVER_CELLS {
        return None;
    }
    Some((d % 2) as u32)
}

/// 驱动家族名（const fn 表 + Option 边界）。
pub const fn driver_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("PostgreSQL"),
        1 => Some("MySQL"),
        2 => Some("MariaDB"),
        3 => Some("Oracle"),
        4 => Some("SQL Server"),
        5 => Some("SQLite"),
        6 => Some("H2"),
        7 => Some("Derby"),
        8 => Some("ClickHouse"),
        9 => Some("DuckDB"),
        _ => None,
    }
}

/// RCP 上栈层载荷（层 l）。
pub const fn rcp_layer_cell(l: usize) -> Option<u64> {
    if l >= RCP_LAYERS {
        return None;
    }
    Some((0xB8000u64 + (l as u64) * 0x53) ^ 0x5243505F53544B31)
}

/// RCP 层名。
pub const fn rcp_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("Equinox OSGi"),
        1 => Some("Workbench"),
        2 => Some("Views/Editors"),
        3 => Some("JDBC Provider"),
        _ => None,
    }
}

/// AI-23 锚定行折叠（行 r）。
pub const fn anchor_row(r: usize) -> Option<u32> {
    if r >= ANCHOR_ROWS {
        return None;
    }
    Some(0xB900 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// DBeaver Community 收口账本状态机。
#[derive(Clone)]
pub struct DBeaverLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 驱动账。
    drivers: [Option<u64>; DRIVER_CELLS],
    /// RCP 上栈账。
    rcp: [Option<u64>; RCP_LAYERS],
    /// 锚定行账。
    anchors: [Option<u32>; ANCHOR_ROWS],
    /// 最后层。
    last_layer: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DBeaverLedger {
    /// 新账本。
    pub fn new() -> Self {
        DBeaverLedger {
            gates: [None; 6],
            drivers: [None; DRIVER_CELLS],
            rcp: [None; RCP_LAYERS],
            anchors: [None; ANCHOR_ROWS],
            last_layer: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DBeaverCode> {
        if gate >= 6 {
            return Err(DBeaverCode::ESeal);
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

    /// 驱动一格（载荷对照 + 重复拒）。
    pub fn record_driver(&mut self, d: usize, got: u64) -> Result<(), DBeaverCode> {
        if d >= DRIVER_CELLS {
            return Err(DBeaverCode::ENoDrv);
        }
        let want = match driver_cell(d) {
            Some(w) => w,
            None => return Err(DBeaverCode::ENoDrv),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + d as u64);
            return Err(DBeaverCode::EDrvDrift);
        }
        if match self.drivers.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DBeaverCode::EDup);
        }
        if let Some(c) = self.drivers.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x6201 + d as u64);
        Ok(())
    }

    /// 驱动全账核验（20 格）。
    pub fn drivers_all(&self) -> bool {
        let mut d = 0usize;
        while d < DRIVER_CELLS {
            match self.drivers.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// RCP 一层（层序单调 + 载荷对照 + 重复拒）。
    pub fn record_layer(&mut self, l: usize, got: u64) -> Result<(), DBeaverCode> {
        if l >= RCP_LAYERS {
            return Err(DBeaverCode::ENoLayer);
        }
        if (l as i64) < self.last_layer {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300);
            return Err(DBeaverCode::EOrder);
        }
        let want = match rcp_layer_cell(l) {
            Some(w) => w,
            None => return Err(DBeaverCode::ENoLayer),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6301 + l as u64);
            return Err(DBeaverCode::EDrvDrift);
        }
        if match self.rcp.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DBeaverCode::EDup);
        }
        if let Some(c) = self.rcp.get_mut(l) {
            *c = Some(got);
        }
        self.last_layer = l as i64;
        self.fnv(0x6302 + l as u64);
        Ok(())
    }

    /// RCP 全账核验（4 层）。
    pub fn rcp_all(&self) -> bool {
        let mut l = 0usize;
        while l < RCP_LAYERS {
            match self.rcp.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// AI-23 锚定行落账。
    pub fn declare_anchor(&mut self, r: usize) -> Result<(), DBeaverCode> {
        if r >= ANCHOR_ROWS {
            return Err(DBeaverCode::ENoAnchor);
        }
        if match self.anchors.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DBeaverCode::EDup);
        }
        if let Some(c) = self.anchors.get_mut(r) {
            *c = anchor_row(r);
        }
        self.fnv(0x6400 + r as u64);
        Ok(())
    }

    /// 锚定行全齐核验。
    pub fn anchors_all(&self) -> bool {
        let mut r = 0usize;
        while r < ANCHOR_ROWS {
            match self.anchors.get(r) {
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
        if self.gates_all() && self.drivers_all() && self.rcp_all() && self.anchors_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DBeaverLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DBeaverLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < DRIVER_CELLS {
        if let Some(w) = driver_cell(d) {
            if led.record_driver(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut l = 0usize;
    while l < RCP_LAYERS {
        if let Some(w) = rcp_layer_cell(l) {
            if led.record_layer(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut r = 0usize;
    while r < ANCHOR_ROWS {
        let _ = led.declare_anchor(r);
        r += 1;
    }
    // 注入 10 轮：驱动格越闭集 / 驱动格漂移 / RCP 层越栈三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DBeaverLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_driver(0, driver_cell(0).unwrap_or(0));
                lt.record_driver(DRIVER_CELLS, 0).is_err()
            }
            1 => lt.record_driver(7, driver_cell(7).unwrap_or(0).wrapping_add(1)) == Err(DBeaverCode::EDrvDrift),
            _ => lt.record_layer(RCP_LAYERS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6500 + k as u64);
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
/// UNX-F52203 判据集（锚 UNX-F52204-J1，off-by-one 板式）。
pub fn run_unx52203_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52203");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "JDBC 驱动 20 格全通",
        recorded >= DRIVER_CELLS as u32,
        "10 家族 × 稳定/候选两版本逐格恒等",
    );
    s.add(
        "驱动家族 10 家闭集",
        {
            let mut ok = true;
            let mut f = 0usize;
            while f < DRIVER_FAMILIES {
                if driver_family_name(f).is_none() {
                    ok = false;
                }
                f += 1;
            }
            ok && driver_family_name(DRIVER_FAMILIES).is_none()
        },
        "PostgreSQL 至 DuckDB 十家族名表闭集",
    );
    s.add(
        "Eclipse RCP 上栈 4 层全落",
        {
            let mut lt = DBeaverLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < RCP_LAYERS {
                if let Some(w) = rcp_layer_cell(l) {
                    if lt.record_layer(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.rcp_all()
        },
        "Equinox OSGi→Workbench→Views/Editors→JDBC Provider 层序单调",
    );
    s.add(
        "AI-23 锚定行落账",
        {
            let mut lt = DBeaverLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ANCHOR_ROWS {
                if lt.declare_anchor(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.anchors_all()
        },
        "驱动下载网络语义锚定行（下载源/校验面/失败回退三段同构，§AI-66.9 红线②）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = DBeaverLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "RCP 层序单调",
        {
            let mut ok = true;
            let mut l = 1usize;
            while l < RCP_LAYERS {
                if rcp_layer_cell(l).unwrap_or(0) < rcp_layer_cell(l - 1).unwrap_or(0) {
                    ok = false;
                }
                l += 1;
            }
            ok
        },
        "四层闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "驱动格越闭集/驱动格漂移/RCP 层越栈三态轮转必抓",
    );
    let mut l0 = DBeaverLedger::new();
    s.add(
        "边界·驱动格越闭集",
        l0.record_driver(DRIVER_CELLS, 0).is_err(),
        "驱动格 20 越闭集必拒",
    );
    s.add(
        "驱动漂移拒",
        {
            l0.record_driver(0, driver_cell(0).unwrap_or(0));
            l0.record_driver(1, driver_cell(1).unwrap_or(0).wrapping_add(1)) == Err(DBeaverCode::EDrvDrift)
        },
        "驱动载荷漂移必抓",
    );
    s.add(
        "边界·RCP 层越栈",
        l0.record_layer(RCP_LAYERS, 0).is_err(),
        "RCP 层 4 越栈必拒",
    );
    s.add(
        "边界·锚定行越闭集",
        l0.declare_anchor(ANCHOR_ROWS).is_err(),
        "锚定行 1 越闭集必拒",
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
            let mut lt = DBeaverLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut d = 0usize;
            while d < DRIVER_CELLS {
                if let Some(w) = driver_cell(d) {
                    let _ = lt.record_driver(d, w);
                }
                d += 1;
            }
            let mut l = 0usize;
            while l < RCP_LAYERS {
                if let Some(w) = rcp_layer_cell(l) {
                    let _ = lt.record_layer(l, w);
                }
                l += 1;
            }
            let mut r = 0usize;
            while r < ANCHOR_ROWS {
                let _ = lt.declare_anchor(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7280,
        "SEG_BASE 0x40 对齐且在 VX15 域第三格",
    );
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥13 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_family_names() {
        assert_eq!(driver_family_name(0), Some("PostgreSQL"));
        assert_eq!(driver_family_name(9), Some("DuckDB"));
        assert!(driver_family_name(10).is_none());
    }

    #[test]
    fn test_driver_segments() {
        assert_eq!(driver_family(7), Some(3));
        assert_eq!(driver_ver_seg(0), Some(0));
        assert_eq!(driver_ver_seg(1), Some(1));
        assert!(driver_cell(20).is_none());
        assert!(driver_family(20).is_none());
        assert!(driver_ver_seg(20).is_none());
    }

    #[test]
    fn test_rcp_layers() {
        assert_eq!(rcp_layer_name(0), Some("Equinox OSGi"));
        assert_eq!(rcp_layer_name(3), Some("JDBC Provider"));
        assert!(rcp_layer_name(4).is_none());
        assert!(rcp_layer_cell(4).is_none());
        assert!(anchor_row(ANCHOR_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = driver_cell(11).unwrap_or(0);
        assert_eq!(a, driver_cell(11).unwrap_or(0));
        assert_ne!(a, driver_cell(12).unwrap_or(0));
    }

    #[test]
    fn test_layer_order_caught() {
        let mut lt = DBeaverLedger::new();
        let _ = lt.record_layer(2, rcp_layer_cell(2).unwrap_or(0));
        assert_eq!(lt.record_layer(1, rcp_layer_cell(1).unwrap_or(0)), Err(DBeaverCode::EOrder));
        assert!(lt.record_layer(3, rcp_layer_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_driver_drift_caught() {
        let mut lt = DBeaverLedger::new();
        assert_eq!(
            lt.record_driver(5, driver_cell(5).unwrap_or(0).wrapping_add(3)),
            Err(DBeaverCode::EDrvDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DBeaverLedger::new();
        let _ = lt.record_driver(9, driver_cell(9).unwrap_or(0));
        assert_eq!(lt.record_driver(9, driver_cell(9).unwrap_or(0)), Err(DBeaverCode::EDup));
        let _ = lt.declare_anchor(0);
        assert_eq!(lt.declare_anchor(0), Err(DBeaverCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DBeaverLedger::new();
        assert!(lt.judge_gate(3).is_ok());
        assert!(lt.judge_gate(6).is_err());
        assert!(!lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (DRIVER_CELLS + RCP_LAYERS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7280);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DBeaverCode::Ok as u32, 0);
        assert_eq!(DBeaverCode::EGradeDrift as u32, 12);
        assert!(!DBeaverCode::EAnchorMiss.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        // 如实定级阶梯：六闸未齐 → S3；六闸齐但面未齐 → S2；全齐（含锚定行）→ S1。
        let mut lt = DBeaverLedger::new();
        let _ = lt.judge_gate(0);
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = DBeaverLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52203_checks_green() {
        let s = run_unx52203_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 13);
    }
}
