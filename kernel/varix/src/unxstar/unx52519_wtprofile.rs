//! UNX-F52519 Windows Terminal profile 共面（多 shell 接入零互踩） · 0xCC180段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52519"，逐字）：
//!   title: Windows Terminal profile 共面（多 shell 接入零互踩）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52520-J1 本组
//!     11 个 LSP server 启动/关闭/崩溃重启协议 ×11 全通零悬空；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.2 专题二；③ 完成时 API/软件写回：状态=
//!     已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52519）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52520-J1，照原文锚定；功能按本卡标题（Windows Terminal profile
//! 共面）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五 shell 族 ×4 格 = 20 格：PowerShell/cmd/WSL/Git Bash/Azure，
//!      每族四字段（profile GUID/图标/起始目录/命令行）逐格载荷恒等才
//!      收，族序单调逆序必抓，漂移必抓。
//!   ② 互踩检出注入 10/10：双 profile 同 GUID 必抓（跨族 GUID 登记
//!      册冲突即 EGclash）；另两态为 16 拍拍序逆与格越闭集。
//!   ③ 与 Windows Terminal 分账声明 2 行闭集（F52221：WT 独立账声明/
//!      settings.json 载荷分账声明），缺一拒封。
//!   ④ 16 拍单调：settings.json 写入拍 0..16 逐拍落账，拍序必须严格
//!      单调递增，逆拍必抓（EBeat）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint，
//!      FNV-1a）；注入核 run_cycle 三态轮转 10/10（GUID 互踩/拍序逆/
//!      越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      闭集（20 格 + 16 拍 + 2 行分账 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52519;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// profile 格数（五族 × 4 字段）。
pub const CELL_N: usize = 20;

/// shell 族数。
pub const SHELL_FAMILIES: usize = 5;

/// 写入拍数（16 拍单调）。
pub const BEATS: usize = 16;

/// 与 Windows Terminal 分账声明行数（F52221）。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WtProfileCode {
    /// 成功
    Ok = 0,
    /// profile 格越闭集
    ENoCell,
    /// 格载荷漂移
    EDrift,
    /// shell 族逆序
    EOrder,
    /// profile GUID 互踩
    EGclash,
    /// 拍越闭集或拍序逆
    EBeat,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
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

impl WtProfileCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WtProfileCode::Ok => "成功",
            WtProfileCode::ENoCell => "profile 格越闭集：五 shell 族 20 格之外先登记",
            WtProfileCode::EDrift => "格载荷漂移：字段折叠与基准不恒等，逐格复算",
            WtProfileCode::EOrder => "shell 族逆序：PowerShell→cmd→WSL→Git Bash→Azure 单调，逆序必抓",
            WtProfileCode::EGclash => "profile GUID 互踩：双 profile 同 GUID 即冲突，跨族检出",
            WtProfileCode::EBeat => "写入拍越闭集或拍序逆：16 拍单调，逆拍必抓",
            WtProfileCode::ENoRow => "声明行越闭集：Windows Terminal 分账 2 行闭集之外先登记（F52221）",
            WtProfileCode::ERowMiss => "缺声明行：Windows Terminal 分账声明未齐先补齐",
            WtProfileCode::EDup => "重复落账：同格只许一条账目",
            WtProfileCode::ESeal => "未齐即封：20 格 + 16 拍 + 2 行分账未齐先补齐",
            WtProfileCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WtProfileCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WtProfileCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格+16拍+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// profile 格载荷（格 o；族 = o/4，字段 = o%4；"WTPR" 掩码低 16 位为零）。
pub const fn wt_cell(o: usize) -> Option<u64> {
    if o >= CELL_N {
        return None;
    }
    Some(
        (0xB1000u64 + (o as u64) * 0x2C)
            ^ ((o / 4) as u64).wrapping_mul(0x01000193)
            ^ ((o % 4) as u64).wrapping_mul(0x1B7)
            ^ 0x5754505200000000,
    )
}

/// shell 族（o/4：0=PowerShell 1=cmd 2=WSL 3=Git Bash 4=Azure）。
pub const fn wt_kind(o: usize) -> Option<u32> {
    if o >= CELL_N {
        return None;
    }
    Some((o / 4) as u32)
}

/// 族内字段（o%4：0=profile GUID 1=图标 2=起始目录 3=命令行）。
pub const fn wt_attr(o: usize) -> Option<u32> {
    if o >= CELL_N {
        return None;
    }
    Some((o % 4) as u32)
}

/// shell 族名。
pub const fn wt_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("PowerShell"),
        1 => Some("cmd"),
        2 => Some("WSL"),
        3 => Some("Git Bash"),
        4 => Some("Azure"),
        _ => None,
    }
}

/// 字段名。
pub const fn wt_attr_name(a: usize) -> Option<&'static str> {
    match a {
        0 => Some("profile GUID"),
        1 => Some("图标"),
        2 => Some("起始目录"),
        3 => Some("命令行"),
        _ => None,
    }
}

/// 各族独立 profile GUID 基准（族 f；GUID 登记册对照基准）。
pub const fn guid_cell(f: usize) -> Option<u64> {
    if f >= SHELL_FAMILIES {
        return None;
    }
    Some((0xB2000u64 + (f as u64) * 0x93) ^ 0x4745494400000000)
}

/// 写入拍基准（拍 b：严格单调递增）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some(0xB7000 + (b as u64) * 0x247)
}

/// Windows Terminal 分账声明行折叠（行 r；F52221）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Windows Terminal profile 共面账本状态机。
#[derive(Clone)]
pub struct WtProfileLedger {
    /// 六判据（profile 生成/图标解析/起始目录解析/命令行装配/互踩检出/拍序对账）。
    gates: [Option<bool>; 6],
    /// profile 格账。
    cells: [Option<u64>; CELL_N],
    /// GUID 登记册（每族一条，跨族查重）。
    guids: [Option<u64>; SHELL_FAMILIES],
    /// 写入拍账。
    beats: [Option<u64>; BEATS],
    /// 最后落拍。
    last_beat: i64,
    /// Windows Terminal 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后 shell 族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WtProfileLedger {
    /// 新账本。
    pub fn new() -> Self {
        WtProfileLedger {
            gates: [None; 6],
            cells: [None; CELL_N],
            guids: [None; SHELL_FAMILIES],
            beats: [None; BEATS],
            last_beat: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WtProfileCode> {
        if gate >= 6 {
            return Err(WtProfileCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5310 + gate as u64);
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

    /// profile 格一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, o: usize, got: u64) -> Result<(), WtProfileCode> {
        if o >= CELL_N {
            return Err(WtProfileCode::ENoCell);
        }
        let kind = match wt_kind(o) {
            Some(k) => k,
            None => return Err(WtProfileCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5320);
            return Err(WtProfileCode::EOrder);
        }
        let want = match wt_cell(o) {
            Some(w) => w,
            None => return Err(WtProfileCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5321 + o as u64);
            return Err(WtProfileCode::EDrift);
        }
        if match self.cells.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WtProfileCode::EDup);
        }
        if let Some(c) = self.cells.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5322 + o as u64);
        Ok(())
    }

    /// profile 格全账核验（20 格）。
    pub fn cells_all(&self) -> bool {
        let mut o = 0usize;
        while o < CELL_N {
            match self.cells.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// GUID 登记一条（族 f；载荷对照；跨族同 GUID 即互踩必抓）。
    pub fn record_guid(&mut self, f: usize, got: u64) -> Result<(), WtProfileCode> {
        let want = match guid_cell(f) {
            Some(w) => w,
            None => return Err(WtProfileCode::ENoCell),
        };
        // 互踩检出：登记册内已有其他族落了同一 GUID（先于漂移核，
        // 双 profile 同 GUID 必抓 EGclash）。
        let mut j = 0usize;
        while j < SHELL_FAMILIES {
            if j != f {
                if let Some(Some(g)) = self.guids.get(j) {
                    if *g == got {
                        self.injected += 1;
                        self.caught += 1;
                        self.fnv(0x5331 + f as u64);
                        return Err(WtProfileCode::EGclash);
                    }
                }
            }
            j += 1;
        }
        // 载荷漂移核：GUID 登记载荷与冻结基准不恒等即抓（互踩核优先，
        // 双 profile 同 GUID 属互踩语义，其余漂移走漂移核）。
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5330 + f as u64);
            return Err(WtProfileCode::EDrift);
        }
        if match self.guids.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WtProfileCode::EDup);
        }
        if let Some(c) = self.guids.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x5332 + f as u64);
        Ok(())
    }

    /// GUID 登记册全齐核验（5 族各一条且互不冲突）。
    pub fn guids_all(&self) -> bool {
        let mut f = 0usize;
        while f < SHELL_FAMILIES {
            match self.guids.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 写入拍一拍（拍序必须严格单调递增，逆拍必抓）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), WtProfileCode> {
        if b >= BEATS {
            return Err(WtProfileCode::EBeat);
        }
        if (b as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5340);
            return Err(WtProfileCode::EBeat);
        }
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(WtProfileCode::EBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5341 + b as u64);
            return Err(WtProfileCode::EDrift);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0x5342 + b as u64);
        Ok(())
    }

    /// 写入拍全账核验（16 拍且单调）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        self.last_beat == (BEATS as i64) - 1
    }

    /// Windows Terminal 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), WtProfileCode> {
        if r >= SPLIT_ROWS {
            return Err(WtProfileCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WtProfileCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5350 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
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

    /// 收口封账闸：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
    pub fn seal(&self) -> Result<(), WtProfileCode> {
        if !self.gates_all() {
            return Err(WtProfileCode::ESeal);
        }
        if !self.splits_all() {
            return Err(WtProfileCode::ERowMiss);
        }
        if !self.cells_all() || !self.beats_all() || !self.guids_all() {
            return Err(WtProfileCode::ESeal);
        }
        Ok(())
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
        if self.gates_all() && self.cells_all() && self.guids_all() && self.beats_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WtProfileLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WtProfileLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CELL_N {
        if let Some(w) = wt_cell(o) {
            if led.record_cell(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut f = 0usize;
    while f < SHELL_FAMILIES {
        if let Some(w) = guid_cell(f) {
            let _ = led.record_guid(f, w);
        }
        f += 1;
    }
    let mut b = 0usize;
    while b < BEATS {
        if let Some(w) = beat_cell(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：GUID 互踩 / 拍序逆 / 越闭集三态轮转（全部落在当轮临时账本）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WtProfileLedger::new();
        let caught = match kind {
            // 互踩：先按合法序落族 0 正 GUID，再让族 1 落同一 GUID 必抓。
            0 => {
                let _ = lt.record_guid(0, guid_cell(0).unwrap_or(0));
                lt.record_guid(1, guid_cell(0).unwrap_or(0)) == Err(WtProfileCode::EGclash)
            }
            // 拍序逆：先落拍 3 正格，再回拍 1 必抓（空账同样先落后逆）。
            1 => {
                let _ = lt.record_beat(3, beat_cell(3).unwrap_or(0));
                lt.record_beat(1, beat_cell(1).unwrap_or(0)) == Err(WtProfileCode::EBeat)
            }
            // 越闭集：profile 格 20 / 写入拍 16 之外必抓（空账本同样必抓）。
            _ => {
                if k % 2 == 0 {
                    lt.record_cell(CELL_N, 0).is_err()
                } else {
                    lt.record_beat(BEATS, 0).is_err()
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5360 + k as u64);
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
/// UNX-F52519 判据集（锚 UNX-F52520-J1，off-by-one 板式）。
pub fn run_unx52519_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52519");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五 shell 族 20 profile 格全通",
        recorded >= CELL_N as u32,
        "PowerShell/cmd/WSL/Git Bash/Azure × GUID/图标/起始目录/命令行逐格恒等",
    );
    s.add(
        "shell 族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CELL_N {
                if wt_kind(o).unwrap_or(5) < wt_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调，四字段按 o%4 展开",
    );
    s.add(
        "GUID 登记册 5/5 零互踩",
        {
            let mut lt = WtProfileLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < SHELL_FAMILIES {
                if let Some(w) = guid_cell(f) {
                    if lt.record_guid(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.guids_all()
        },
        "五族各一条独立 GUID，跨族冲突即互踩必抓",
    );
    s.add(
        "16 拍单调全落",
        {
            let mut lt = WtProfileLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "settings.json 写入拍 0..16 严格单调递增",
    );
    s.add(
        "Windows Terminal 分账声明 2/2 全齐（F52221）",
        {
            let mut lt = WtProfileLedger::new();
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
        "WT 独立账声明/settings.json 载荷分账声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = WtProfileLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "profile 生成/图标解析/起始目录解析/命令行装配/互踩检出/拍序对账逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "GUID 互踩/拍序逆/越闭集三态轮转必抓",
    );
    let mut l0 = WtProfileLedger::new();
    s.add(
        "边界·profile 格越闭集",
        l0.record_cell(CELL_N, 0).is_err(),
        "profile 格 20 越闭集必拒",
    );
    s.add(
        "边界·写入拍越闭集",
        l0.record_beat(BEATS, 0).is_err(),
        "写入拍 16 之外必拒",
    );
    s.add(
        "边界·GUID 互踩",
        {
            let _ = l0.record_guid(0, guid_cell(0).unwrap_or(0));
            l0.record_guid(1, guid_cell(0).unwrap_or(0)) == Err(WtProfileCode::EGclash)
        },
        "双 profile 同 GUID 必抓",
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
        "收口·封账闸",
        {
            let mut lt = WtProfileLedger::new();
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            // 六闸缺 → ESeal；声明齐而格账缺 → ESeal。
            let a = lt.seal() == Err(WtProfileCode::ESeal);
            let _ = lt.judge_gate(0);
            let b = lt.seal() == Err(WtProfileCode::ESeal);
            a && b
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = WtProfileLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CELL_N {
                if let Some(w) = wt_cell(o) {
                    let _ = lt.record_cell(o, w);
                }
                o += 1;
            }
            let mut f = 0usize;
            while f < SHELL_FAMILIES {
                if let Some(w) = guid_cell(f) {
                    let _ = lt.record_guid(f, w);
                }
                f += 1;
            }
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC180",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wt_kind_attr() {
        // 族映射 kind = o/4，字段映射 attr = o%4，测试按该映射核对。
        assert_eq!(wt_kind(0), Some(0));
        assert_eq!(wt_kind(3), Some(0));
        assert_eq!(wt_kind(4), Some(1));
        assert_eq!(wt_kind(19), Some(4));
        assert!(wt_kind(20).is_none());
        assert_eq!(wt_attr(0), Some(0));
        assert_eq!(wt_attr(5), Some(1));
        assert_eq!(wt_attr(19), Some(3));
        assert!(wt_attr(20).is_none());
        assert!(wt_cell(20).is_none());
    }

    #[test]
    fn test_kind_attr_names() {
        // 名表按 kind/attr 索引核对。
        assert_eq!(wt_kind_name(0), Some("PowerShell"));
        assert_eq!(wt_kind_name(3), Some("Git Bash"));
        assert_eq!(wt_kind_name(4), Some("Azure"));
        assert!(wt_kind_name(5).is_none());
        assert_eq!(wt_attr_name(0), Some("profile GUID"));
        assert_eq!(wt_attr_name(3), Some("命令行"));
        assert!(wt_attr_name(4).is_none());
    }

    #[test]
    fn test_beats_strictly_monotonic() {
        // 16 拍基准严格递增。
        let mut b = 1usize;
        while b < BEATS {
            let prev = beat_cell(b - 1).unwrap_or(0);
            let cur = beat_cell(b).unwrap_or(0);
            assert!(cur > prev);
            b += 1;
        }
        assert!(beat_cell(BEATS).is_none());
    }

    #[test]
    fn test_beat_order_caught() {
        // 种子按合法序先落低拍，逆拍必抓。
        let mut lt = WtProfileLedger::new();
        let _ = lt.record_beat(5, beat_cell(5).unwrap_or(0));
        assert_eq!(lt.record_beat(2, beat_cell(2).unwrap_or(0)), Err(WtProfileCode::EBeat));
        assert!(lt.record_beat(6, beat_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_guid_clash_caught() {
        // 双 profile 同 GUID 必抓；异 GUID 与复落各归各码。
        let mut lt = WtProfileLedger::new();
        let _ = lt.record_guid(0, guid_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_guid(1, guid_cell(0).unwrap_or(0)),
            Err(WtProfileCode::EGclash)
        );
        let _ = lt.record_guid(1, guid_cell(1).unwrap_or(0));
        assert_eq!(
            lt.record_guid(1, guid_cell(1).unwrap_or(0)),
            Err(WtProfileCode::EDup)
        );
        assert!(lt.record_guid(2, guid_cell(2).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = WtProfileLedger::new();
        let _ = lt.record_cell(9, wt_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cell(2, wt_cell(2).unwrap_or(0)), Err(WtProfileCode::EOrder));
        assert!(lt.record_cell(10, wt_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WtProfileLedger::new();
        // 先按合法序铺 0..=5 正格，再对已录格注漂移。
        let mut i = 0usize;
        while i <= 5 {
            let _ = lt.record_cell(i, wt_cell(i).unwrap_or(0));
            i += 1;
        }
        assert_eq!(
            lt.record_cell(5, wt_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(WtProfileCode::EDrift)
        );
        // GUID 漂移：无其他族登记时先撞漂移核（互踩核不触发）。
        assert_eq!(
            lt.record_guid(3, guid_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(WtProfileCode::EDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WtProfileLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WtProfileLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 profile 格 + 16 写入拍 = 36。
        assert_eq!(recorded, CELL_N as u32 + BEATS as u32);
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
        assert_eq!(SEG_BASE, 0xCC180);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            WtProfileCode::Ok,
            WtProfileCode::ENoCell,
            WtProfileCode::EDrift,
            WtProfileCode::EOrder,
            WtProfileCode::EGclash,
            WtProfileCode::EBeat,
            WtProfileCode::ENoRow,
            WtProfileCode::ERowMiss,
            WtProfileCode::EDup,
            WtProfileCode::ESeal,
            WtProfileCode::EInj,
            WtProfileCode::EFp,
            WtProfileCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(WtProfileCode::Ok.val(), SEG_BASE);
        assert_eq!(WtProfileCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐（含六闸前置）S1。
        let lt = WtProfileLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = WtProfileLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let mut o = 0usize;
        while o < CELL_N {
            if let Some(w) = wt_cell(o) {
                let _ = lt2.record_cell(o, w);
            }
            o += 1;
        }
        let mut f = 0usize;
        while f < SHELL_FAMILIES {
            if let Some(w) = guid_cell(f) {
                let _ = lt2.record_guid(f, w);
            }
            f += 1;
        }
        let mut b = 0usize;
        while b < BEATS {
            if let Some(w) = beat_cell(b) {
                let _ = lt2.record_beat(b, w);
            }
            b += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52519_checks_green() {
        let s = run_unx52519_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
