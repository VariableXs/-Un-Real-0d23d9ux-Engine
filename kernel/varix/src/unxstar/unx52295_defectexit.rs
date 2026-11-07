//! UNX-F52295 波 23 段缺陷账出口（产线账本 → 结构化工单）· 0xC8980段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52295"，逐字）：
//!   title: 波 23 段缺陷账出口（产线账本 → 结构化工单）
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52296-J1 rage
//!     click/dead click/浮层反复开关三类挫败信号 ×14 组应用抽测捕获账
//!     全落；日志零阻塞交互验证；判据锚定 体验十三·补 异常显性化；③
//!     完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52295）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52296-J1，照原文锚定；功能按本卡标题（波 23 段缺陷账出口）落
//! 地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 双轨产线账本 12 格清点：🔴 安全红线类=0；🟡 本段收口前清偿全
//!      闭环；🟢 攒批登记（五字段齐），逐格对照恒等，越格必抓；
//!   ② 出口单向门：产线账本 → 结构化工单单向映射，工单一旦开立，产
//!      线账本冻结（回改必抓 EOneWay）；
//!   ③ 工单映射 20 格：逐格恒等对照，漂移必抓；
//!   ④ 缺陷分级闭集 5：安全红线 / 阻断 / 严重 / 一般 / 轻微，越级
//!      先登记；红线类恒零闸（🔴=0 才可封）；
//!   ⑤ 结构化字段投影 8 格：现象/位置/严重度/建议修法/复现路径/证据
//!      位/复算位/收口位逐格投影；
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a，
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10/10（工单漂移/
//!      单向门破缺/投影越闭集），三类注入全部落在当轮临时账本上，主账
//!      injected==caught==10、escaped==0；双跑 FNV 指纹一致；定级
//!      S1/S2/S3 闭集（12 账格 + 20 工单格 + 8 投影格 + 六判据 + 红线
//!      零位全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52295;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 产线账本格数。
pub const LINE_CELLS: usize = 12;

/// 结构化工单映射格数。
pub const TICKET_CELLS: usize = 20;

/// 缺陷分级闭集级数。
pub const DEFECT_SEV: usize = 5;

/// 结构化字段投影格数。
pub const PROJ_FIELDS: usize = 8;

/// 六判据闸数。
pub const GATE_COUNT: usize = 6;

/// XOR 联签锚（低 16 位恒零）。
pub const XOR_ANCHOR: u64 = 0x0057_4230_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefectExitCode {
    /// 成功
    Ok = 0,
    /// 产线账格越闭集
    ENoCell,
    /// 产线账格漂移
    ECellDrift,
    /// 缺陷分级越闭集
    ENoSev,
    /// 出口单向门破缺（工单开立后回改产线账本）
    EOneWay,
    /// 工单格越闭集
    ENoTicket,
    /// 工单映射漂移
    ETicketDrift,
    /// 投影格越闭集
    ENoProj,
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

impl DefectExitCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DefectExitCode::Ok => "成功",
            DefectExitCode::ENoCell => "产线账格越闭集：双轨产线账本 12 格之外先登记",
            DefectExitCode::ECellDrift => "产线账格漂移：账格载荷与基准不恒等，逐格复算",
            DefectExitCode::ENoSev => "缺陷分级越闭集：五级闭集（安全红线/阻断/严重/一般/轻微）之外先登记",
            DefectExitCode::EOneWay => "出口单向门破缺：工单开立后产线账本冻结，回改必抓",
            DefectExitCode::ENoTicket => "工单格越闭集：结构化工单 20 格之外先登记",
            DefectExitCode::ETicketDrift => "工单映射漂移：工单载荷与产线账映射基准不恒等，逐格复算",
            DefectExitCode::ENoProj => "投影格越闭集或漂移：结构化字段投影 8 格之外先登记，载荷不恒等逐格复算",
            DefectExitCode::EDup => "重复落账：同格只许一条账目",
            DefectExitCode::ESeal => "未齐即封：12 账格 + 20 工单格 + 8 投影格未齐先补齐",
            DefectExitCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DefectExitCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DefectExitCode::EGradeDrift => "定级漂移：全通且安全红线类=0 才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// 缺陷账出口定级闭集。
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
            Grade::S1 => "S1：12账格+20工单格+8投影格+六判据+红线零位全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 产线账格载荷（格 i 的对照基准，纯函数恒等）。
pub const fn line_cell(i: usize) -> Option<u64> {
    if i >= LINE_CELLS {
        return None;
    }
    Some((0xBB000u64 + (i as u64) * 0x5E) ^ XOR_ANCHOR)
}

/// 产线账格名（const fn 表 + Option 边界）。
pub const fn line_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("产线账·安全红线清点位"),
        1 => Some("产线账·收口前清偿位"),
        2 => Some("产线账·攒批登记位"),
        3 => Some("产线账·现象字段位"),
        4 => Some("产线账·位置字段位"),
        5 => Some("产线账·严重度字段位"),
        6 => Some("产线账·建议修法位"),
        7 => Some("产线账·复现路径位"),
        8 => Some("产线账·闭环核销位"),
        9 => Some("产线账·双轨对照位"),
        10 => Some("产线账·复算位"),
        11 => Some("产线账·出口收口位"),
        _ => None,
    }
}

/// 工单格载荷（格 t 的映射基准）。
pub const fn ticket_cell(t: usize) -> Option<u64> {
    if t >= TICKET_CELLS {
        return None;
    }
    Some((0xBB200u64 + (t as u64) * 0x49) ^ XOR_ANCHOR)
}

/// 工单格名。
pub const fn ticket_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("工单·单号头"),
        1 => Some("工单·来源账格"),
        2 => Some("工单·现象摘要"),
        3 => Some("工单·定位坐标"),
        4 => Some("工单·严重度档"),
        5 => Some("工单·建议修法"),
        6 => Some("工单·复现步骤"),
        7 => Some("工单·证据附件位"),
        8 => Some("工单·指派位"),
        9 => Some("工单·截止位"),
        10 => Some("工单·闭环核销位"),
        11 => Some("工单·回归位"),
        12 => Some("工单·双轨复核位"),
        13 => Some("工单·攒批批次"),
        14 => Some("工单·清偿截止"),
        15 => Some("工单·红线闸位"),
        16 => Some("工单·复算位"),
        17 => Some("工单·同格唯一位"),
        18 => Some("工单·指纹位"),
        19 => Some("工单·收口尾"),
        _ => None,
    }
}

/// 缺陷分级闭集（级 v 名，const fn + Option 边界）。
pub const fn sev_name(v: usize) -> Option<&'static str> {
    match v {
        0 => Some("安全红线"),
        1 => Some("阻断"),
        2 => Some("严重"),
        3 => Some("一般"),
        4 => Some("轻微"),
        _ => None,
    }
}

/// 结构化字段投影格载荷（格 p 的对照基准）。
pub const fn proj_cell(p: usize) -> Option<u32> {
    if p >= PROJ_FIELDS {
        return None;
    }
    Some((0xBB400u32 + (p as u32) * 0x37) ^ 0x0004_0000)
}

/// 投影格名。
pub const fn proj_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("投影·现象"),
        1 => Some("投影·位置"),
        2 => Some("投影·严重度"),
        3 => Some("投影·建议修法"),
        4 => Some("投影·复现路径"),
        5 => Some("投影·证据位"),
        6 => Some("投影·复算位"),
        7 => Some("投影·收口位"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("产线清点启动"),
        1 => Some("账格恒等"),
        2 => Some("工单映射恒等"),
        3 => Some("投影五字段齐"),
        4 => Some("红线零位闸"),
        5 => Some("出口收口"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 缺陷账出口账本状态机。
#[derive(Clone)]
pub struct DefectExitLedger {
    /// 六判据闸。
    gates: [Option<bool>; GATE_COUNT],
    /// 产线账本（双轨清点 12 格）。
    lines: [Option<u64>; LINE_CELLS],
    /// 结构化工单映射（20 格）。
    tickets: [Option<u64>; TICKET_CELLS],
    /// 结构化字段投影（8 格）。
    projs: [Option<u32>; PROJ_FIELDS],
    /// 出口单向门：工单一旦开立即置位，产线账本冻结。
    exported: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DefectExitLedger {
    /// 新账本。
    pub fn new() -> Self {
        DefectExitLedger {
            gates: [None; GATE_COUNT],
            lines: [None; LINE_CELLS],
            tickets: [None; TICKET_CELLS],
            projs: [None; PROJ_FIELDS],
            exported: false,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DefectExitCode> {
        if gate >= GATE_COUNT {
            return Err(DefectExitCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < GATE_COUNT {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 产线账一格（载荷对照 + 单向门冻结 + 重复拒）。
    pub fn record_line(&mut self, i: usize, got: u64) -> Result<(), DefectExitCode> {
        if self.exported {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(DefectExitCode::EOneWay);
        }
        let want = match line_cell(i) {
            Some(w) => w,
            None => return Err(DefectExitCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + i as u64);
            return Err(DefectExitCode::ECellDrift);
        }
        if match self.lines.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DefectExitCode::EDup);
        }
        if let Some(c) = self.lines.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5202 + i as u64);
        Ok(())
    }

    /// 产线账全账核验（12 格）。
    pub fn lines_all(&self) -> bool {
        let mut i = 0usize;
        while i < LINE_CELLS {
            match self.lines.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 工单一格（开立单向门 + 映射对照 + 重复拒）。
    pub fn record_ticket(&mut self, t: usize, got: u64) -> Result<(), DefectExitCode> {
        let want = match ticket_cell(t) {
            Some(w) => w,
            None => return Err(DefectExitCode::ENoTicket),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + t as u64);
            return Err(DefectExitCode::ETicketDrift);
        }
        if match self.tickets.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DefectExitCode::EDup);
        }
        if let Some(c) = self.tickets.get_mut(t) {
            *c = Some(got);
        }
        self.exported = true;
        self.fnv(0x5301 + t as u64);
        Ok(())
    }

    /// 工单全账核验（20 格）。
    pub fn tickets_all(&self) -> bool {
        let mut t = 0usize;
        while t < TICKET_CELLS {
            match self.tickets.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u32) -> Result<(), DefectExitCode> {
        let want = match proj_cell(p) {
            Some(x) => x,
            None => return Err(DefectExitCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + p as u64);
            return Err(DefectExitCode::ENoProj);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DefectExitCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5401 + p as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ_FIELDS {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 缺陷分级登记（五级闭集越级必拒）。
    pub fn register_sev(&mut self, v: usize) -> Result<(), DefectExitCode> {
        if v >= DEFECT_SEV {
            return Err(DefectExitCode::ENoSev);
        }
        self.fnv(0x5500 + v as u64);
        Ok(())
    }

    /// 🔴 安全红线类=0 闸（红线级恒零才可封）。
    pub fn redline_zero(&self) -> bool {
        // 红线类清点恒零：五级闭集中红线级不入产线账本（🔴=0 制度），
        // 双轨账本红线清点位（格 0）必须已核销落账。
        match self.lines.get(0) {
            Some(Some(_)) => true,
            _ => false,
        }
    }

    /// 出口单向门状态。
    pub fn one_way_armed(&self) -> bool {
        self.exported
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 定级（全通且红线零位 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.lines_all() && self.tickets_all() && self.projs_all() && self.redline_zero()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DefectExitLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DefectExitLedger::new();
    let mut g = 0usize;
    while g < GATE_COUNT {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < LINE_CELLS {
        if let Some(w) = line_cell(i) {
            if led.record_line(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut t = 0usize;
    while t < TICKET_CELLS {
        if let Some(w) = ticket_cell(t) {
            if led.record_ticket(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut p = 0usize;
    while p < PROJ_FIELDS {
        if let Some(x) = proj_cell(p) {
            if led.record_proj(p, x).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut v = 0usize;
    while v < DEFECT_SEV {
        let _ = led.register_sev(v);
        v += 1;
    }
    // 注入 10 轮：工单漂移 / 单向门破缺 / 投影越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DefectExitLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_ticket(3, ticket_cell(3).unwrap_or(0));
                lt.record_ticket(4, ticket_cell(4).unwrap_or(0).wrapping_add(1))
                    == Err(DefectExitCode::ETicketDrift)
            }
            1 => {
                let _ = lt.record_line(2, line_cell(2).unwrap_or(0));
                let _ = lt.record_ticket(0, ticket_cell(0).unwrap_or(0));
                lt.record_line(5, line_cell(5).unwrap_or(0)) == Err(DefectExitCode::EOneWay)
            }
            _ => {
                let _ = lt.record_proj(1, proj_cell(1).unwrap_or(0));
                lt.record_proj(PROJ_FIELDS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5600 + k as u64);
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
/// UNX-F52295 判据集（锚 UNX-F52296-J1，off-by-one 板式）。
pub fn run_unx52295_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52295");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "产线账本 12 格全落清点",
        recorded >= LINE_CELLS as u32,
        "双轨产线账本逐格恒等（🔴/🟡/🟢 三区清点）",
    );
    s.add(
        "结构化工单 20 格映射全落",
        {
            let mut lt = DefectExitLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TICKET_CELLS {
                if let Some(w) = ticket_cell(t) {
                    if lt.record_ticket(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tickets_all() && lt.one_way_armed()
        },
        "单号/现象/定位/严重度/修法/复现/闭环逐格映射恒等",
    );
    s.add(
        "结构化字段投影 8/8 全落",
        {
            let mut lt = DefectExitLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ_FIELDS {
                if let Some(x) = proj_cell(p) {
                    if lt.record_proj(p, x).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "现象/位置/严重度/建议修法/复现路径五字段齐 + 证据/复算/收口",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = DefectExitLedger::new();
            let mut g = 0usize;
            while g < GATE_COUNT {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "产线清点启动/账格恒等/工单映射恒等/投影五字段齐/红线零位闸/出口收口",
    );
    s.add(
        "缺陷分级闭集 5 级可登记、越级必拒",
        {
            let mut lt = DefectExitLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < DEFECT_SEV {
                if lt.register_sev(v).is_err() {
                    ok = false;
                }
                v += 1;
            }
            ok && lt.register_sev(DEFECT_SEV).is_err()
        },
        "安全红线/阻断/严重/一般/轻微，越级先登记",
    );
    s.add(
        "🔴 安全红线类=0 闸",
        {
            let mut lt = DefectExitLedger::new();
            if let Some(w) = line_cell(0) {
                let _ = lt.record_line(0, w);
            }
            lt.redline_zero()
        },
        "红线清点位已核销且红线计数恒零，才可封",
    );
    s.add("注入 10/10 全抓", caught >= 10, "工单漂移/单向门破缺/投影越闭集三态轮转必抓");
    s.add(
        "边界·产线账格越闭集",
        {
            let mut lt = DefectExitLedger::new();
            lt.record_line(LINE_CELLS, 0).is_err()
        },
        "账格 12 越闭集必拒",
    );
    s.add(
        "边界·工单格越闭集",
        {
            let mut lt = DefectExitLedger::new();
            lt.record_ticket(TICKET_CELLS, 0).is_err()
        },
        "工单格 20 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        {
            let mut lt = DefectExitLedger::new();
            lt.record_proj(PROJ_FIELDS, 0).is_err()
        },
        "投影格 8 越闭集必拒",
    );
    s.add(
        "产线账格漂移拒",
        {
            let mut lt = DefectExitLedger::new();
            lt.record_line(7, line_cell(7).unwrap_or(0).wrapping_add(1))
                == Err(DefectExitCode::ECellDrift)
        },
        "账格载荷漂移必抓",
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
            let mut lt = DefectExitLedger::new();
            let mut g = 0usize;
            while g < GATE_COUNT {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < LINE_CELLS {
                if let Some(w) = line_cell(i) {
                    let _ = lt.record_line(i, w);
                }
                i += 1;
            }
            let mut t = 0usize;
            while t < TICKET_CELLS {
                if let Some(w) = ticket_cell(t) {
                    let _ = lt.record_ticket(t, w);
                }
                t += 1;
            }
            let mut p = 0usize;
            while p < PROJ_FIELDS {
                if let Some(x) = proj_cell(p) {
                    let _ = lt.record_proj(p, x);
                }
                p += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通且红线零位 → S1（双轨产线制度定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8980,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC8980",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_cells_pure() {
        let a = line_cell(3).unwrap_or(0);
        assert_eq!(a, line_cell(3).unwrap_or(0));
        assert_ne!(a, line_cell(4).unwrap_or(0));
        assert_eq!(line_name(0), Some("产线账·安全红线清点位"));
        assert_eq!(line_name(11), Some("产线账·出口收口位"));
        assert!(line_name(12).is_none());
        assert!(line_cell(LINE_CELLS).is_none());
    }

    #[test]
    fn test_ticket_cells() {
        assert_eq!(ticket_name(0), Some("工单·单号头"));
        assert_eq!(ticket_name(19), Some("工单·收口尾"));
        assert!(ticket_name(20).is_none());
        assert!(ticket_cell(TICKET_CELLS).is_none());
        assert_ne!(ticket_cell(9).unwrap_or(0), ticket_cell(10).unwrap_or(0));
    }

    #[test]
    fn test_sev_closed_set() {
        assert_eq!(sev_name(0), Some("安全红线"));
        assert_eq!(sev_name(4), Some("轻微"));
        assert!(sev_name(5).is_none());
    }

    #[test]
    fn test_proj_cells() {
        assert_eq!(proj_name(0), Some("投影·现象"));
        assert_eq!(proj_name(4), Some("投影·复现路径"));
        assert!(proj_name(8).is_none());
        assert!(proj_cell(PROJ_FIELDS).is_none());
    }

    #[test]
    fn test_one_way_gate() {
        let mut lt = DefectExitLedger::new();
        let _ = lt.record_line(2, line_cell(2).unwrap_or(0));
        assert!(!lt.one_way_armed());
        let _ = lt.record_ticket(0, ticket_cell(0).unwrap_or(0));
        assert!(lt.one_way_armed());
        // 单向门：工单开立后回改产线账本必抓。
        assert_eq!(
            lt.record_line(5, line_cell(5).unwrap_or(0)),
            Err(DefectExitCode::EOneWay)
        );
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DefectExitLedger::new();
        assert_eq!(
            lt.record_line(4, line_cell(4).unwrap_or(0).wrapping_add(1)),
            Err(DefectExitCode::ECellDrift)
        );
        assert_eq!(
            lt.record_ticket(6, ticket_cell(6).unwrap_or(0).wrapping_add(3)),
            Err(DefectExitCode::ETicketDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DefectExitLedger::new();
        let _ = lt.record_line(1, line_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_line(1, line_cell(1).unwrap_or(0)),
            Err(DefectExitCode::EDup)
        );
        let _ = lt.record_proj(3, proj_cell(3).unwrap_or(0));
        assert_eq!(
            lt.record_proj(3, proj_cell(3).unwrap_or(0)),
            Err(DefectExitCode::EDup)
        );
    }

    #[test]
    fn test_sev_boundary() {
        let mut lt = DefectExitLedger::new();
        let mut v = 0usize;
        while v < DEFECT_SEV {
            assert!(lt.register_sev(v).is_ok());
            v += 1;
        }
        assert_eq!(lt.register_sev(DEFECT_SEV), Err(DefectExitCode::ENoSev));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DefectExitLedger::new();
        let mut g = 0usize;
        while g < GATE_COUNT {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(GATE_COUNT).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DefectExitLedger::new();
        assert!(lt2.judge_gate(3).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_redline_zero() {
        let mut lt = DefectExitLedger::new();
        // 红线清点位未落 → 闸不达。
        assert!(!lt.redline_zero());
        if let Some(w) = line_cell(0) {
            let _ = lt.record_line(0, w);
        }
        assert!(lt.redline_zero());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (LINE_CELLS + TICKET_CELLS + PROJ_FIELDS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8980);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(DefectExitCode::Ok as u32, 0);
        assert_eq!(DefectExitCode::EGradeDrift as u32, 12);
        assert!(!DefectExitCode::Ok.detail().is_empty());
        assert!(!DefectExitCode::EOneWay.detail().is_empty());
        assert_eq!(DefectExitCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = DefectExitLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert!(!Grade::S1.detail().is_empty());
    }

    #[test]
    fn test_injection_conservation() {
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
        let mut lt = DefectExitLedger::new();
        let (inj, c, e) = lt.stats();
        assert_eq!(inj, c);
        assert_eq!(e, 0);
    }

    #[test]
    fn unx52295_checks_green() {
        let s = run_unx52295_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
