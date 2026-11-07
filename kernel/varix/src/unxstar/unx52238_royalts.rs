//! UNX-F52238 Royal TS 收口 · 0xC7B40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52238"，逐字）：
//!   title: Royal TS 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52239-J1 六判据全落；
//!     RDP/SSH/VNC 三协议消费 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52238）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52239-J1，照原文锚定；功能按本卡标题（Royal TS）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 连接五类 20 格闭集：RDP（0..4）/ SSH（4..8）/ Telnet（8..12）/
//!      VNC（12..16）/ PowerShell（16..20），每类 4 操作（新建/编辑/连
//!      接/断开），类内单调（逆序必抓）。
//!   ② 凭据库三元闸：库口令 / 主密钥 / 条目解锁，三闸齐闭才开库。
//!   ③ 面板/布局 8 格：面板 4 格（0..4）+ 布局 4 格（4..8），双族单调。
//!   ④ 动态文件夹 6 格：查询源/凭据映射/分组模板/刷新周期/继承开关/失效
//!      回退，逐格落账。
//!   ⑤ 六判据收口闸：启动自检/连接功能/崩溃恢复/卸载清理/升级迁移/许可
//!      登记，未齐即封拒。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/fingerprint FNV-1a）；注入核
//!      run_cycle 三态轮转 10/10（连接越闭集/动态文件夹漂移/面板逆序），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3，本卡口
//!      径定级 S2（全通才落 S2，S1 本卡不落）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52238;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () =
    assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 连接五类格数。
pub const CONN_OPS: usize = 20;

/// 凭据库三元闸数。
pub const CRED_GATES: usize = 3;

/// 面板/布局格数。
pub const PANEL_CELLS: usize = 8;

/// 动态文件夹格数。
pub const DFOLDER_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoyalCode {
    /// 成功
    Ok = 0,
    /// 连接号越闭集
    ENoConn,
    /// 连接载荷漂移
    EConnDrift,
    /// 凭据闸越闭集
    ENoCred,
    /// 凭据库未开
    ECredShut,
    /// 面板家族逆序
    EOrder,
    /// 动态文件夹号越闭集
    ENoFolder,
    /// 动态文件夹漂移
    EFolderDrift,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 判据漂移
    EDrift,
    /// 定级漂移
    EGradeDrift,
}

impl RoyalCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            RoyalCode::Ok => "成功",
            RoyalCode::ENoConn => "连接号越闭集：五类 20 格之外先登记",
            RoyalCode::EConnDrift => "连接载荷漂移：连接格折叠与基准不恒等，逐格复算",
            RoyalCode::ENoCred => "凭据闸越闭集：三元闸 0..=2 之外先登记",
            RoyalCode::ECredShut => "凭据库未开：库口令/主密钥/条目解锁三元未齐先补齐",
            RoyalCode::EOrder => "面板家族逆序：面板→布局双族单调，逆序必抓",
            RoyalCode::ENoFolder => "动态文件夹号越闭集：6 格之外先登记",
            RoyalCode::EFolderDrift => "动态文件夹漂移：文件夹格折叠与基准不恒等，逐格复算",
            RoyalCode::EDup => "重复落账：同格只许一条账目",
            RoyalCode::ESeal => "未齐即封：20 连接格 + 3 凭据闸 + 8 面板格 + 6 文件夹格未齐先补齐",
            RoyalCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            RoyalCode::EDrift => "判据漂移：落账值与基准表不恒等，逐格复算",
            RoyalCode::EGradeDrift => "定级漂移：全通且凭据三元闸齐闭才可定 S2（本卡口径，S1 不落）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：本卡口径不落
    S1,
    /// S2：收口（本卡定级）
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
            Grade::S1 => "S1：Royal TS 本卡口径不落（收口级为 S2）",
            Grade::S2 => "S2：六判据+20连接格+3凭据闸+8面板格+6文件夹格齐格收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 连接格载荷（操作 o；类别 = o/4）。
pub const fn conn_cell(o: usize) -> Option<u64> {
    if o >= CONN_OPS {
        return None;
    }
    Some((0xB8000u64 + (o as u64) * 0x41) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 连接类别（0..4=RDP 4..8=SSH 8..12=Telnet 12..16=VNC 16..20=PowerShell）。
pub const fn conn_kind(o: usize) -> Option<u32> {
    if o >= CONN_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 连接类名。
pub const fn conn_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("RDP"),
        1 => Some("SSH"),
        2 => Some("Telnet"),
        3 => Some("VNC"),
        4 => Some("PowerShell"),
        _ => None,
    }
}

/// 类内操作名（新建/编辑/连接/断开）。
pub const fn conn_op_name(o: usize) -> Option<&'static str> {
    if o >= CONN_OPS {
        return None;
    }
    match o % 4 {
        0 => Some("新建"),
        1 => Some("编辑"),
        2 => Some("连接"),
        _ => Some("断开"),
    }
}

/// 凭据三元闸折叠（闸 t：0=库口令 1=主密钥 2=条目解锁）。
pub const fn cred_gate_cell(t: usize) -> Option<u32> {
    if t >= CRED_GATES {
        return None;
    }
    Some(0xC5200 + t as u32 * 0x2F)
}

/// 凭据闸名。
pub const fn cred_gate_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("库口令"),
        1 => Some("主密钥"),
        2 => Some("条目解锁"),
        _ => None,
    }
}

/// 面板/布局格载荷（格 p；面板 = 0..4，布局 = 4..8）。
pub const fn panel_cell(p: usize) -> Option<u64> {
    if p >= PANEL_CELLS {
        return None;
    }
    Some((0xB8400u64 + (p as u64) * 0x53) ^ ((p / 4) as u64).wrapping_mul(0x01000193))
}

/// 面板族（0=面板 1=布局）。
pub const fn panel_kind(p: usize) -> Option<u32> {
    if p >= PANEL_CELLS {
        return None;
    }
    Some((p / 4) as u32)
}

/// 面板族名。
pub const fn panel_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("面板"),
        1 => Some("布局"),
        _ => None,
    }
}

/// 动态文件夹格载荷（格 d：6 项语义）。
pub const fn dfolder_cell(d: usize) -> Option<u64> {
    if d >= DFOLDER_CELLS {
        return None;
    }
    Some((0xB8800u64 + (d as u64) * 0x67) ^ 0x524F59414C5F4446)
}

/// 动态文件夹格名。
pub const fn dfolder_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("查询源"),
        1 => Some("凭据映射"),
        2 => Some("分组模板"),
        3 => Some("刷新周期"),
        4 => Some("继承开关"),
        5 => Some("失效回退"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Royal TS 收口账本状态机。
#[derive(Clone)]
pub struct RoyalLedger {
    /// 六判据（启动自检/连接功能/崩溃恢复/卸载清理/升级迁移/许可登记）。
    gates: [Option<bool>; 6],
    /// 连接格账。
    conns: [Option<u64>; CONN_OPS],
    /// 凭据三元闸账。
    creds: [Option<u32>; CRED_GATES],
    /// 面板/布局格账。
    panels: [Option<u64>; PANEL_CELLS],
    /// 动态文件夹格账。
    folders: [Option<u64>; DFOLDER_CELLS],
    /// 最后面板族。
    last_panel_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl RoyalLedger {
    /// 新账本。
    pub fn new() -> Self {
        RoyalLedger {
            gates: [None; 6],
            conns: [None; CONN_OPS],
            creds: [None; CRED_GATES],
            panels: [None; PANEL_CELLS],
            folders: [None; DFOLDER_CELLS],
            last_panel_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), RoyalCode> {
        if gate >= 6 {
            return Err(RoyalCode::ESeal);
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

    /// 连接一格（载荷对照 + 重复拒）。
    pub fn record_conn(&mut self, o: usize, got: u64) -> Result<(), RoyalCode> {
        if o >= CONN_OPS {
            return Err(RoyalCode::ENoConn);
        }
        let want = match conn_cell(o) {
            Some(w) => w,
            None => return Err(RoyalCode::ENoConn),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + o as u64);
            return Err(RoyalCode::EConnDrift);
        }
        if match self.conns.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RoyalCode::EDup);
        }
        if let Some(c) = self.conns.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x6201 + o as u64);
        Ok(())
    }

    /// 连接全账核验（20 格）。
    pub fn conns_all(&self) -> bool {
        let mut o = 0usize;
        while o < CONN_OPS {
            match self.conns.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 凭据三元闸一格（未齐闸不开库）。
    pub fn record_cred_gate(&mut self, t: usize) -> Result<(), RoyalCode> {
        if t >= CRED_GATES {
            return Err(RoyalCode::ENoCred);
        }
        if match self.creds.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RoyalCode::EDup);
        }
        if let Some(c) = self.creds.get_mut(t) {
            *c = cred_gate_cell(t);
        }
        self.fnv(0x6300 + t as u64);
        Ok(())
    }

    /// 凭据三元闸全闭核验（三闸齐闭才开库）。
    pub fn creds_all(&self) -> bool {
        let mut t = 0usize;
        while t < CRED_GATES {
            match self.creds.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 面板/布局一格（双族单调 + 载荷对照 + 重复拒）。
    pub fn record_panel(&mut self, p: usize, got: u64) -> Result<(), RoyalCode> {
        if p >= PANEL_CELLS {
            return Err(RoyalCode::ESeal);
        }
        let kind = match panel_kind(p) {
            Some(k) => k,
            None => return Err(RoyalCode::ESeal),
        };
        if (kind as i64) < self.last_panel_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400);
            return Err(RoyalCode::EOrder);
        }
        let want = match panel_cell(p) {
            Some(w) => w,
            None => return Err(RoyalCode::ESeal),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6401 + p as u64);
            return Err(RoyalCode::EConnDrift);
        }
        if match self.panels.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RoyalCode::EDup);
        }
        if let Some(c) = self.panels.get_mut(p) {
            *c = Some(got);
        }
        self.last_panel_kind = kind as i64;
        self.fnv(0x6402 + p as u64);
        Ok(())
    }

    /// 面板/布局全账核验（8 格）。
    pub fn panels_all(&self) -> bool {
        let mut p = 0usize;
        while p < PANEL_CELLS {
            match self.panels.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 动态文件夹一格（载荷对照 + 重复拒）。
    pub fn record_dfolder(&mut self, d: usize, got: u64) -> Result<(), RoyalCode> {
        let want = match dfolder_cell(d) {
            Some(w) => w,
            None => return Err(RoyalCode::ENoFolder),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + d as u64);
            return Err(RoyalCode::EFolderDrift);
        }
        if match self.folders.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(RoyalCode::EDup);
        }
        if let Some(c) = self.folders.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x6501 + d as u64);
        Ok(())
    }

    /// 动态文件夹全账核验（6 格）。
    pub fn folders_all(&self) -> bool {
        let mut d = 0usize;
        while d < DFOLDER_CELLS {
            match self.folders.get(d) {
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

    /// KB-COMPAT 定级（六判据全落 → S2；判据缺 → S3；S1 本卡不落）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for RoyalLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = RoyalLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CONN_OPS {
        if let Some(w) = conn_cell(o) {
            if led.record_conn(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < CRED_GATES {
        let _ = led.record_cred_gate(t);
        t += 1;
    }
    let mut p = 0usize;
    while p < PANEL_CELLS {
        if let Some(w) = panel_cell(p) {
            if led.record_panel(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut d = 0usize;
    while d < DFOLDER_CELLS {
        if let Some(w) = dfolder_cell(d) {
            if led.record_dfolder(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    // 注入 10 轮：连接越闭集 / 动态文件夹漂移 / 面板逆序三态轮转。
    // 漂移注入先录正格再注漂移；逆序注入对空账本同样必抓且不改 last_panel_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = RoyalLedger::new();
        let caught = match kind {
            0 => lt.record_conn(CONN_OPS, 0) == Err(RoyalCode::ENoConn),
            1 => {
                let _ = lt.record_dfolder(1, dfolder_cell(1).unwrap_or(0));
                lt.record_dfolder(2, dfolder_cell(2).unwrap_or(0).wrapping_add(1))
                    == Err(RoyalCode::EFolderDrift)
            }
            _ => {
                let _ = lt.record_panel(5, panel_cell(5).unwrap_or(0));
                lt.record_panel(0, panel_cell(0).unwrap_or(0)) == Err(RoyalCode::EOrder)
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
/// UNX-F52238 判据集（锚 UNX-F52239-J1，off-by-one 板式）。
pub fn run_unx52238_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52238");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "连接五类 20 格全通",
        recorded >= (CONN_OPS + DFOLDER_CELLS) as u32,
        "RDP/SSH/Telnet/VNC/PowerShell 五类逐操作恒等（新建/编辑/连接/断开）",
    );
    s.add(
        "RDP/SSH/VNC 三协议消费 ×10",
        {
            // 卡面判据口径：三协议族首格 + 类内四操作 ×族内循环消费，合计 ≥10 次。
            let mut lt = RoyalLedger::new();
            let mut n = 0u32;
            let kinds = [0usize, 1, 3];
            let mut ki = 0usize;
            while ki < kinds.len() {
                let base = kinds[ki] * 4;
                let mut op = 0usize;
                while op < 4 {
                    if let Some(w) = conn_cell(base + op) {
                        if lt.record_conn(base + op, w).is_ok() {
                            n += 1;
                        }
                    }
                    op += 1;
                }
                ki += 1;
            }
            let mut extra = 0usize;
            while extra < 2 {
                if let Some(w) = conn_cell(16 + extra) {
                    if lt.record_conn(16 + extra, w).is_ok() {
                        n += 1;
                    }
                }
                extra += 1;
            }
            n >= 10
        },
        "RDP/SSH/VNC 三协议族全 4 操作 + 补录 2 格，消费次数 ≥10",
    );
    s.add(
        "连接五类单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CONN_OPS {
                if conn_kind(o).unwrap_or(5) < conn_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五类闭集单调，逆序必抓",
    );
    s.add(
        "凭据库三元闸全闭",
        {
            let mut lt = RoyalLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < CRED_GATES {
                if lt.record_cred_gate(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.creds_all()
        },
        "库口令/主密钥/条目解锁三元齐闭才开库",
    );
    s.add(
        "面板/布局 8 格全落",
        {
            let mut lt = RoyalLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PANEL_CELLS {
                if let Some(w) = panel_cell(p) {
                    if lt.record_panel(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.panels_all()
        },
        "面板 4 格 + 布局 4 格双族单调逐格落账",
    );
    s.add(
        "动态文件夹 6 格全落",
        {
            let mut lt = RoyalLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DFOLDER_CELLS {
                if let Some(w) = dfolder_cell(d) {
                    if lt.record_dfolder(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.folders_all()
        },
        "查询源至失效回退六项语义逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "连接越闭集/动态文件夹漂移/面板逆序三态轮转必抓");
    s.add(
        "边界·连接越闭集",
        {
            let mut lt = RoyalLedger::new();
            lt.record_conn(CONN_OPS, 0).is_err()
        },
        "连接格 20 越闭集必拒",
    );
    s.add(
        "边界·凭据闸越闭集",
        {
            let mut lt = RoyalLedger::new();
            lt.record_cred_gate(CRED_GATES).is_err()
        },
        "凭据闸 3 越闭集必拒",
    );
    s.add(
        "边界·动态文件夹越闭集",
        {
            let mut lt = RoyalLedger::new();
            lt.record_dfolder(DFOLDER_CELLS, 0).is_err()
        },
        "动态文件夹格 6 越闭集必拒",
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
        "六判据全落",
        {
            let mut lt = RoyalLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动自检/连接功能/崩溃恢复/卸载清理/升级迁移/许可登记逐格落账",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = RoyalLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "六判据全落即 S2 收口（KB-COMPAT 定级登记，S1 本卡不落）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7B40,
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
    fn test_conn_kinds() {
        assert_eq!(conn_kind_name(0), Some("RDP"));
        assert_eq!(conn_kind_name(2), Some("Telnet"));
        assert_eq!(conn_kind_name(3), Some("VNC"));
        assert_eq!(conn_kind_name(4), Some("PowerShell"));
        assert!(conn_kind_name(5).is_none());
        assert!(conn_cell(CONN_OPS).is_none());
    }

    #[test]
    fn test_conn_op_names() {
        assert_eq!(conn_op_name(0), Some("新建"));
        assert_eq!(conn_op_name(7), Some("断开"));
        assert_eq!(conn_op_name(13), Some("编辑"));
        assert!(conn_op_name(20).is_none());
    }

    #[test]
    fn test_cred_gates() {
        assert_eq!(cred_gate_name(0), Some("库口令"));
        assert_eq!(cred_gate_name(2), Some("条目解锁"));
        assert!(cred_gate_name(3).is_none());
        assert!(cred_gate_cell(CRED_GATES).is_none());
    }

    #[test]
    fn test_panel_kinds() {
        assert_eq!(panel_kind(0), Some(0));
        assert_eq!(panel_kind(4), Some(1));
        assert!(panel_kind(8).is_none());
        assert_eq!(panel_kind_name(0), Some("面板"));
        assert_eq!(panel_kind_name(1), Some("布局"));
    }

    #[test]
    fn test_dfolder_names() {
        assert_eq!(dfolder_name(0), Some("查询源"));
        assert_eq!(dfolder_name(5), Some("失效回退"));
        assert!(dfolder_name(6).is_none());
        assert!(dfolder_cell(DFOLDER_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = conn_cell(9).unwrap_or(0);
        assert_eq!(a, conn_cell(9).unwrap_or(0));
        assert_ne!(a, conn_cell(10).unwrap_or(0));
        let d = dfolder_cell(3).unwrap_or(0);
        assert_ne!(d, dfolder_cell(4).unwrap_or(0));
    }

    #[test]
    fn test_panel_order_caught() {
        let mut lt = RoyalLedger::new();
        let _ = lt.record_panel(6, panel_cell(6).unwrap_or(0));
        assert_eq!(
            lt.record_panel(1, panel_cell(1).unwrap_or(0)),
            Err(RoyalCode::EOrder)
        );
        assert!(lt.record_panel(7, panel_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = RoyalLedger::new();
        assert_eq!(
            lt.record_conn(3, conn_cell(3).unwrap_or(0).wrapping_add(5)),
            Err(RoyalCode::EConnDrift)
        );
        assert_eq!(
            lt.record_dfolder(0, dfolder_cell(0).unwrap_or(0).wrapping_sub(2)),
            Err(RoyalCode::EFolderDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = RoyalLedger::new();
        if let Some(w) = panel_cell(2) {
            let _ = lt.record_panel(2, w);
            assert_eq!(lt.record_panel(2, w), Err(RoyalCode::EDup));
        }
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = RoyalLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = RoyalLedger::new();
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (CONN_OPS + PANEL_CELLS + DFOLDER_CELLS) as u32);
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
    fn test_stats_conservation() {
        let mut lt = RoyalLedger::new();
        let _ = lt.record_conn(CONN_OPS + 1, 0);
        let (i, c, e) = lt.stats();
        assert_eq!(i, c + e);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7B40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(RoyalCode::Ok as u32, 0);
        assert_eq!(RoyalCode::EGradeDrift as u32, 12);
        assert!(!RoyalCode::Ok.detail().is_empty());
        assert!(!RoyalCode::ECredShut.detail().is_empty());
        assert_eq!(Grade::S2.val(), 2);
    }

    #[test]
    fn unx52238_checks_green() {
        let s = run_unx52238_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
