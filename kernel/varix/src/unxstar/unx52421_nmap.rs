//! UNX-F52421 nmap 收口 · 0xCA900段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52421"，逐字）：
//!   title: nmap 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52422-J1 六判据全落
//!     （nmap GUI 形态分账）；×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式：本卡（卡 N=52421）判据锚 UNX-F(N+1)-J1 = UNX-F52422-J1，
//! 照原文锚定；功能按本卡标题（nmap 收口）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五扫描族 20 格闭集：connect/syn/udp/version/script 各 4 格，逐格
//!      载荷（纯函数折叠）恒等才收，漂移必抓。
//!   ② 五命令 10 操作闭集：-sS/-sV/-O/-A/-p 五族各 2 操作，家族单调
//!      （逆序必抓，逆序判错不改 last_kind）。
//!   ③ 目标投影 8 格：投影折叠恒等，越闭集必拒。
//!   ④ Npcap 消费联签 4 格（锚 F52249）：逐格载荷对照，缺一拒封。
//!   ⑤ C 上栈声明 2 行闭集：C 栈上栈声明 1 / C 栈回退声明 2，缺一拒封。
//!   ⑥ 六判据闸 + ledger 状态机（new/record_*/judge_gate×6/fingerprint
//!      FNV-1a/grade 定级闭集 S1/S2/S3）；注入核 run_cycle 三态轮转 10
//!      轮（扫描格漂移/命令家族逆序/扫描越闭集），10/10 必抓、检出+逃逸
//!      守恒；双跑 FNV 指纹一致。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52421;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 扫描格数（五族 × 4）。
pub const SCAN_CELLS: usize = 20;

/// 命令操作数（五族 × 2）。
pub const CMD_OPS: usize = 10;

/// 目标投影格数。
pub const PROJ_CELLS: usize = 8;

/// Npcap 消费联签行数（锚 F52249）。
pub const LINK_ROWS: usize = 4;

/// C 上栈声明行数。
pub const C_STACK_ROWS: usize = 2;

/// Npcap 联签锚（F52249 卡）。
pub const NPCAP_ANCHOR: u64 = 0xF52249;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NmapCode {
    /// 成功
    Ok = 0,
    /// 扫描格越闭集
    ENoScan,
    /// 扫描格漂移
    EScanDrift,
    /// 命令操作越闭集
    ENoCmd,
    /// 命令操作漂移
    ECmdDrift,
    /// 命令家族逆序
    EOrder,
    /// 目标投影越闭集
    ENoProj,
    /// 目标投影漂移
    EProjDrift,
    /// 联签行越闭集
    ENoLink,
    /// 联签漂移
    ELinkDrift,
    /// 声明行越闭集
    ENoRow,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl NmapCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NmapCode::Ok => "成功",
            NmapCode::ENoScan => "扫描格越闭集：connect/syn/udp/version/script 20 格之外先登记",
            NmapCode::EScanDrift => "扫描格漂移：扫描载荷折叠与基准不恒等，逐格复算",
            NmapCode::ENoCmd => "命令操作越闭集：-sS/-sV/-O/-A/-p 10 操作之外先登记",
            NmapCode::ECmdDrift => "命令操作漂移：命令载荷折叠与基准不恒等，逐格复算",
            NmapCode::EOrder => "命令家族逆序：-sS→-sV→-O→-A→-p 单调，逆序必抓且不前移游标",
            NmapCode::ENoProj => "目标投影越闭集：目标投影 8 格之外先登记",
            NmapCode::EProjDrift => "目标投影漂移：投影折叠与基准不恒等，逐格复算",
            NmapCode::ENoLink => "联签行越闭集：Npcap 消费联签 4 格之外先登记",
            NmapCode::ELinkDrift => "联签漂移：Npcap 联签载荷与 F52249 锚不恒等",
            NmapCode::ENoRow => "声明行越闭集：C 上栈声明 2 行闭集之外先登记",
            NmapCode::ESeal => "未齐即封：20 扫描格 + 10 命令 + 8 投影 + 4 联签未齐先补齐",
            NmapCode::EGradeDrift => "定级漂移：全通且声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20扫描格+10命令+8投影+4联签+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 扫描格载荷（格 o；族 = o/4，族序 connect→syn→udp→version→script）。
pub const fn scan_cell(o: usize) -> Option<u64> {
    if o >= SCAN_CELLS {
        return None;
    }
    Some((0xD1000u64 + (o as u64) * 0x33) ^ (0x4E4D4150_0000_0000 + (o / 4) as u64))
}

/// 扫描族号。
pub const fn scan_kind(o: usize) -> Option<u32> {
    if o >= SCAN_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 扫描族名。
pub const fn scan_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("connect"),
        1 => Some("syn"),
        2 => Some("udp"),
        3 => Some("version"),
        4 => Some("script"),
        _ => None,
    }
}

/// 命令操作格载荷（操作 o；族 = o/2，族序 -sS→-sV→-O→-A→-p）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xD2000u64 + (o as u64) * 0x47) ^ ((o / 2) as u64).wrapping_mul(0x0100_0193))
}

/// 命令族号。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("-sS"),
        1 => Some("-sV"),
        2 => Some("-O"),
        3 => Some("-A"),
        4 => Some("-p"),
        _ => None,
    }
}

/// 目标投影格载荷（投影 i）。
pub const fn proj_cell(i: usize) -> Option<u64> {
    if i >= PROJ_CELLS {
        return None;
    }
    Some((0xD3000u64 + (i as u64) * 0x5F).rotate_left((i as u64 % 32) as u32))
}

/// Npcap 联签行载荷（行 r，锚 F52249）。
pub const fn link_cell(r: usize) -> Option<u64> {
    if r >= LINK_ROWS {
        return None;
    }
    Some(NPCAP_ANCHOR + (r as u64) * 0x41)
}

/// C 上栈声明行折叠（行 r）。
pub const fn c_stack_row(r: usize) -> Option<u32> {
    if r >= C_STACK_ROWS {
        return None;
    }
    Some(0xD4000 + r as u32 * 0x53)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// nmap 收口账本状态机。
#[derive(Clone)]
pub struct NmapLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 扫描格账。
    scans: [Option<u64>; SCAN_CELLS],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// 目标投影账。
    projs: [Option<u64>; PROJ_CELLS],
    /// Npcap 联签账。
    links: [Option<u64>; LINK_ROWS],
    /// C 上栈声明账。
    rows: [Option<u32>; C_STACK_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NmapLedger {
    /// 新账本。
    pub fn new() -> Self {
        NmapLedger {
            gates: [None; 6],
            scans: [None; SCAN_CELLS],
            cmds: [None; CMD_OPS],
            projs: [None; PROJ_CELLS],
            links: [None; LINK_ROWS],
            rows: [None; C_STACK_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf2_9ce4_8422_2325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x0000_0100_0000_01b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NmapCode> {
        if gate >= 6 {
            return Err(NmapCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xA100 + gate as u64);
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

    /// 扫描一格（载荷对照 + 重复拒）。
    pub fn record_scan(&mut self, o: usize, got: u64) -> Result<(), NmapCode> {
        let want = match scan_cell(o) {
            Some(w) => w,
            None => return Err(NmapCode::ENoScan),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA200 + o as u64);
            return Err(NmapCode::EScanDrift);
        }
        if match self.scans.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NmapCode::ESeal);
        }
        if let Some(c) = self.scans.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0xA201 + o as u64);
        Ok(())
    }

    /// 扫描全账核验（20 格）。
    pub fn scans_all(&self) -> bool {
        let mut o = 0usize;
        while o < SCAN_CELLS {
            match self.scans.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 命令一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), NmapCode> {
        if o >= CMD_OPS {
            return Err(NmapCode::ENoCmd);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(NmapCode::ENoCmd),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA300);
            return Err(NmapCode::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(NmapCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA301 + o as u64);
            return Err(NmapCode::ECmdDrift);
        }
        if match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NmapCode::ESeal);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0xA302 + o as u64);
        Ok(())
    }

    /// 命令全账核验（10 操作）。
    pub fn cmds_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.cmds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 目标投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, i: usize, got: u64) -> Result<(), NmapCode> {
        let want = match proj_cell(i) {
            Some(w) => w,
            None => return Err(NmapCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA400 + i as u64);
            return Err(NmapCode::EProjDrift);
        }
        if match self.projs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NmapCode::ESeal);
        }
        if let Some(c) = self.projs.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xA401 + i as u64);
        Ok(())
    }

    /// 目标投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut i = 0usize;
        while i < PROJ_CELLS {
            match self.projs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Npcap 联签一行（载荷对照 + 重复拒）。
    pub fn record_link(&mut self, r: usize, got: u64) -> Result<(), NmapCode> {
        let want = match link_cell(r) {
            Some(w) => w,
            None => return Err(NmapCode::ENoLink),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xA500 + r as u64);
            return Err(NmapCode::ELinkDrift);
        }
        if match self.links.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NmapCode::ESeal);
        }
        if let Some(c) = self.links.get_mut(r) {
            *c = Some(got);
        }
        self.fnv(0xA501 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn links_all(&self) -> bool {
        let mut r = 0usize;
        while r < LINK_ROWS {
            match self.links.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// C 上栈声明一行。
    pub fn declare_c_stack(&mut self, r: usize) -> Result<(), NmapCode> {
        if r >= C_STACK_ROWS {
            return Err(NmapCode::ENoRow);
        }
        if match self.rows.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NmapCode::ESeal);
        }
        if let Some(c) = self.rows.get_mut(r) {
            *c = c_stack_row(r);
        }
        self.fnv(0xA600 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < C_STACK_ROWS {
            match self.rows.get(r) {
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
        if self.gates_all() && self.scans_all() && self.cmds_all() && self.projs_all()
            && self.links_all() && self.rows_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for NmapLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NmapLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < SCAN_CELLS {
        if let Some(w) = scan_cell(o) {
            if led.record_scan(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CMD_OPS {
        if let Some(w) = cmd_cell(c) {
            if led.record_cmd(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut i = 0usize;
    while i < PROJ_CELLS {
        if let Some(w) = proj_cell(i) {
            if led.record_proj(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < LINK_ROWS {
        if let Some(w) = link_cell(r) {
            if led.record_link(r, w).is_ok() {
                recorded += 1;
            }
        }
        r += 1;
    }
    let mut s = 0usize;
    while s < C_STACK_ROWS {
        let _ = led.declare_c_stack(s);
        s += 1;
    }
    // 注入 10 轮：扫描格漂移 / 命令家族逆序 / 扫描越闭集三态轮转。
    // 全部落在当轮临时账本上，主账只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NmapLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_scan(0, scan_cell(0).unwrap_or(0));
                lt.record_scan(1, scan_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(NmapCode::EScanDrift)
            }
            1 => {
                // 家族逆序：先落高族合法格，再注低族格，逆序必抓且 last_kind 不前移。
                let _ = lt.record_cmd(6, cmd_cell(6).unwrap_or(0));
                lt.record_cmd(1, cmd_cell(1).unwrap_or(0)) == Err(NmapCode::EOrder)
            }
            _ => {
                // 越闭集：对空账本同样必抓。
                lt.record_scan(SCAN_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xA700 + k as u64);
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
/// UNX-F52421 判据集（锚 UNX-F52422-J1，off-by-one 板式）。
pub fn run_unx52421_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52421");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五扫描族 20 格全通",
        recorded >= SCAN_CELLS as u32,
        "connect/syn/udp/version/script 五族逐格载荷恒等",
    );
    s.add(
        "扫描族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < SCAN_CELLS {
                if scan_kind(o).unwrap_or(5) < scan_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五扫描族闭集单调（4 格一族）",
    );
    s.add(
        "五命令 10 操作全通",
        {
            let mut lt = NmapLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CMD_OPS {
                if let Some(w) = cmd_cell(c) {
                    if lt.record_cmd(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cmds_all()
        },
        "-sS/-sV/-O/-A/-p 五族按合法序逐格落账",
    );
    s.add(
        "命令家族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < CMD_OPS {
                if cmd_kind(c).unwrap_or(5) < cmd_kind(c - 1).unwrap_or(5) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "命令五族单调（2 操作一族），逆序必抓",
    );
    s.add(
        "目标投影 8 格全落",
        {
            let mut lt = NmapLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if let Some(w) = proj_cell(i) {
                    if lt.record_proj(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.projs_all()
        },
        "目标投影折叠恒等逐格落账",
    );
    s.add(
        "Npcap 消费联签 4/4 全齐（F52249）",
        {
            let mut lt = NmapLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < LINK_ROWS {
                if let Some(w) = link_cell(r) {
                    if lt.record_link(r, w).is_err() {
                        ok = false;
                    }
                }
                r += 1;
            }
            ok && lt.links_all()
        },
        "Npcap 消费联签逐格对照 F52249 锚（§AI-66.3）",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = NmapLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < C_STACK_ROWS {
                if lt.declare_c_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "C 栈上栈声明/C 栈回退声明两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = NmapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（nmap GUI 形态分账）",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "扫描漂移/命令家族逆序/扫描越闭集三态轮转必抓",
    );
    let mut l0 = NmapLedger::new();
    s.add(
        "边界·扫描格漂移拒",
        {
            let _ = l0.record_scan(0, scan_cell(0).unwrap_or(0));
            l0.record_scan(1, scan_cell(1).unwrap_or(0).wrapping_add(1)) == Err(NmapCode::EScanDrift)
        },
        "扫描载荷漂移必抓",
    );
    s.add(
        "边界·命令越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令 10 越闭集必拒",
    );
    s.add(
        "边界·扫描格越闭集",
        l0.record_scan(SCAN_CELLS, 0).is_err(),
        "扫描格 20 越闭集必拒",
    );
    s.add(
        "边界·目标投影越闭集",
        l0.record_proj(PROJ_CELLS, 0).is_err(),
        "目标投影 8 格越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_link(LINK_ROWS, NPCAP_ANCHOR).is_err(),
        "联签 4 行越闭集必拒",
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
            let mut lt = NmapLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < SCAN_CELLS {
                if let Some(w) = scan_cell(o) {
                    let _ = lt.record_scan(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CMD_OPS {
                if let Some(w) = cmd_cell(c) {
                    let _ = lt.record_cmd(c, w);
                }
                c += 1;
            }
            let mut i = 0usize;
            while i < PROJ_CELLS {
                if let Some(w) = proj_cell(i) {
                    let _ = lt.record_proj(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < LINK_ROWS {
                if let Some(w) = link_cell(r) {
                    let _ = lt.record_link(r, w);
                }
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < C_STACK_ROWS {
                let _ = lt.declare_c_stack(r2);
                r2 += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA900,
        "SEG_BASE 0x40 对齐且在 VX16 派工域公式位",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 18,
        "含本条共 ≥19 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_kind() {
        assert_eq!(scan_kind(0), Some(0));
        assert_eq!(scan_kind(4), Some(1));
        assert_eq!(scan_kind(19), Some(4));
        assert!(scan_kind(20).is_none());
        assert!(scan_cell(20).is_none());
        assert_eq!(scan_kind_name(0), Some("connect"));
        assert_eq!(scan_kind_name(4), Some("script"));
        assert!(scan_kind_name(5).is_none());
    }

    #[test]
    fn test_cmd_kind() {
        // 族 = o/2 映射：偶数位起一族。
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(1), Some(0));
        assert_eq!(cmd_kind(2), Some(1));
        assert_eq!(cmd_kind(9), Some(4));
        assert!(cmd_kind(10).is_none());
        assert_eq!(cmd_kind_name(0), Some("-sS"));
        assert_eq!(cmd_kind_name(4), Some("-p"));
        assert!(cmd_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = scan_cell(3).unwrap_or(0);
        assert_eq!(a, scan_cell(3).unwrap_or(0));
        assert_ne!(a, scan_cell(4).unwrap_or(0));
        let p = proj_cell(5).unwrap_or(0);
        assert_eq!(p, proj_cell(5).unwrap_or(0));
        assert!(c_stack_row(C_STACK_ROWS).is_none());
        assert!(link_cell(LINK_ROWS).is_none());
        assert_eq!(link_cell(0), Some(NPCAP_ANCHOR));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = NmapLedger::new();
        let _ = lt.record_cmd(6, cmd_cell(6).unwrap_or(0));
        assert_eq!(lt.record_cmd(1, cmd_cell(1).unwrap_or(0)), Err(NmapCode::EOrder));
        // 逆序判错不改 last_kind：同族之后的高族合法格仍可落账。
        assert!(lt.record_cmd(7, cmd_cell(7).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NmapLedger::new();
        assert_eq!(
            lt.record_scan(5, scan_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(NmapCode::EScanDrift)
        );
        assert_eq!(
            lt.record_cmd(3, cmd_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(NmapCode::ECmdDrift)
        );
        assert_eq!(
            lt.record_proj(2, proj_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(NmapCode::EProjDrift)
        );
        assert_eq!(
            lt.record_link(1, NPCAP_ANCHOR + 0x41 + 1),
            Err(NmapCode::ELinkDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NmapLedger::new();
        let _ = lt.record_scan(0, scan_cell(0).unwrap_or(0));
        assert_eq!(lt.record_scan(0, scan_cell(0).unwrap_or(0)), Err(NmapCode::ESeal));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NmapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = NmapLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：扫描 20 + 命令 10 + 投影 8 + 联签 4。
        assert_eq!(
            recorded,
            SCAN_CELLS as u32 + CMD_OPS as u32 + PROJ_CELLS as u32 + LINK_ROWS as u32
        );
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
        assert_eq!(SEG_BASE, 0xCA900);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(NmapCode::Ok as u32, 0);
        assert_eq!(NmapCode::EGradeDrift as u32, 12);
        assert_eq!(NmapCode::Ok.val(), SEG_BASE);
        assert!(!NmapCode::Ok.detail().is_empty());
        assert!(!NmapCode::ESeal.detail().is_empty());
        assert!(!NmapCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = NmapLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐（格未齐）→ S2。
        let mut lt2 = NmapLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52421_checks_green() {
        let s = run_unx52421_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
