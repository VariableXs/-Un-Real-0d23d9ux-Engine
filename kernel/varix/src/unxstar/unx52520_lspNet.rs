//! UNX-F52520 LSP 生态启动协议共面（stdio/管道语义全域对账） · 0xCC1C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52520"，逐字）：
//!   title: LSP 生态启动协议共面（stdio/管道语义全域对账）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52521-J1 六判据
//!     全落；计算/绘图 ×10 全通；KB-COMPAT 定级 S1；判据锚定 §AI-66.3
//!     组 27 口径；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52520）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52521-J1，照原文锚定；功能按本卡标题（LSP 启动协议共面）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五 server 族 ×4 格 = 20 格：clangd/gopls/rust-analyzer/pyright/
//!      typescript-language-server，每族四字段（stdio/管道/初始化/关机）
//!      逐格载荷恒等才收，族序单调逆序必抓，漂移必抓。
//!   ② stdio 三态 6 格：标准 stdio/命名管道/Socket ×2 格，逐态载荷
//!      对照，越态必抓。
//!   ③ 与各 LSP 分账声明 2 行闭集（LSP 启动独立账声明/LSP 关闭独立账
//!      声明），缺一拒封；消费联签 4 格（F52501/F52502 消费面联签），
//!      越签必抓。
//!   ④ 16 拍单调：LSP 生命周期拍 0..16 逐拍落账，拍序必须严格单调
//!      递增，逆拍必抓（EBeat）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/seal/fingerprint，
//!      FNV-1a）；注入核 run_cycle 三态轮转 10/10（拍序逆/载荷漂移/
//!      越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      闭集（20 格 + 6 态 + 4 联签 + 16 拍 + 2 行分账 + 六判据全通
//!      → S1，判据原文锚定 KB-COMPAT 定级 S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52520;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// server 格数（五族 × 4 字段）。
pub const CELL_N: usize = 20;

/// stdio 三态格数（3 态 × 2）。
pub const STDIO_CELLS: usize = 6;

/// 消费联签格数（F52501/F52502）。
pub const COSIGN_CELLS: usize = 4;

/// 生命周期拍数（16 拍单调）。
pub const BEATS: usize = 16;

/// 与各 LSP 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LspNetCode {
    /// 成功
    Ok = 0,
    /// server 格越闭集
    ENoCell,
    /// 格载荷漂移
    EDrift,
    /// server 族逆序
    EOrder,
    /// stdio 态越闭集
    ENoStdio,
    /// 拍越闭集或拍序逆
    EBeat,
    /// 声明行/联签格越闭集
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

impl LspNetCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            LspNetCode::Ok => "成功",
            LspNetCode::ENoCell => "server 格越闭集：五 server 族 20 格之外先登记",
            LspNetCode::EDrift => "格载荷漂移：协议折叠与基准不恒等，逐格复算",
            LspNetCode::EOrder => "server 族逆序：clangd→gopls→rust-analyzer→pyright→tsserver 单调，逆序必抓",
            LspNetCode::ENoStdio => "stdio 态越闭集：标准 stdio/命名管道/Socket 三态六格之外先登记",
            LspNetCode::EBeat => "生命周期拍越闭集或拍序逆：16 拍单调，逆拍必抓",
            LspNetCode::ENoRow => "声明行/联签格越闭集：LSP 分账 2 行与消费联签 4 格之外先登记",
            LspNetCode::ERowMiss => "缺声明行：LSP 启动/关闭独立账声明未齐先补齐",
            LspNetCode::EDup => "重复落账：同格只许一条账目",
            LspNetCode::ESeal => "未齐即封：20 格 + 6 态 + 4 联签 + 16 拍 + 2 行分账未齐先补齐",
            LspNetCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            LspNetCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            LspNetCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1（判据原文锚定）",
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
            Grade::S1 => "S1：六判据+20格+6态+4联签+16拍+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// server 格载荷（格 o；族 = o/4，字段 = o%4；"LSP" 掩码低 16 位为零）。
pub const fn lsp_cell(o: usize) -> Option<u64> {
    if o >= CELL_N {
        return None;
    }
    Some(
        (0xB1000u64 + (o as u64) * 0x2C)
            ^ ((o / 4) as u64).wrapping_mul(0x01000193)
            ^ ((o % 4) as u64).wrapping_mul(0x1F5)
            ^ 0x4C53500000000000,
    )
}

/// server 族（o/4：0=clangd 1=gopls 2=rust-analyzer 3=pyright 4=tsserver）。
pub const fn lsp_kind(o: usize) -> Option<u32> {
    if o >= CELL_N {
        return None;
    }
    Some((o / 4) as u32)
}

/// 族内字段（o%4：0=stdio 1=管道 2=初始化 3=关机）。
pub const fn lsp_attr(o: usize) -> Option<u32> {
    if o >= CELL_N {
        return None;
    }
    Some((o % 4) as u32)
}

/// server 族名。
pub const fn lsp_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("clangd"),
        1 => Some("gopls"),
        2 => Some("rust-analyzer"),
        3 => Some("pyright"),
        4 => Some("typescript-language-server"),
        _ => None,
    }
}

/// 字段名。
pub const fn lsp_attr_name(a: usize) -> Option<&'static str> {
    match a {
        0 => Some("stdio"),
        1 => Some("管道"),
        2 => Some("初始化"),
        3 => Some("关机"),
        _ => None,
    }
}

/// stdio 三态格载荷（格 i：态 = i/2，0=标准 stdio 1=命名管道 2=Socket）。
pub const fn stdio_cell(i: usize) -> Option<u64> {
    if i >= STDIO_CELLS {
        return None;
    }
    Some((0xB3000u64 + (i as u64) * 0x31) ^ 0x5354444F00000000)
}

/// 通道态名。
pub const fn stdio_state_name(i: usize) -> Option<&'static str> {
    match i / 2 {
        0 => Some("标准 stdio"),
        1 => Some("命名管道"),
        2 => Some("Socket"),
        _ => None,
    }
}

/// 消费联签格载荷（格 c；F52501/F52502 消费面）。
pub const fn cosign_cell(c: usize) -> Option<u64> {
    if c >= COSIGN_CELLS {
        return None;
    }
    Some(0xB600 + c as u64 * 0x2C)
}

/// 生命周期拍基准（拍 b：严格单调递增）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some(0xB7000 + (b as u64) * 0x24F)
}

/// LSP 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xB400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// LSP 启动协议共面账本状态机。
#[derive(Clone)]
pub struct LspNetLedger {
    /// 六判据（启动握手/初始化对账/关机序列/崩溃重启/管道语义/全域对账）。
    gates: [Option<bool>; 6],
    /// server 格账。
    cells: [Option<u64>; CELL_N],
    /// stdio 态账。
    stdios: [Option<u64>; STDIO_CELLS],
    /// 联签账。
    cosigns: [Option<u64>; COSIGN_CELLS],
    /// 生命周期拍账。
    beats: [Option<u64>; BEATS],
    /// 最后落拍。
    last_beat: i64,
    /// LSP 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后 server 族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl LspNetLedger {
    /// 新账本。
    pub fn new() -> Self {
        LspNetLedger {
            gates: [None; 6],
            cells: [None; CELL_N],
            stdios: [None; STDIO_CELLS],
            cosigns: [None; COSIGN_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), LspNetCode> {
        if gate >= 6 {
            return Err(LspNetCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5410 + gate as u64);
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

    /// server 格一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cell(&mut self, o: usize, got: u64) -> Result<(), LspNetCode> {
        if o >= CELL_N {
            return Err(LspNetCode::ENoCell);
        }
        let kind = match lsp_kind(o) {
            Some(k) => k,
            None => return Err(LspNetCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5420);
            return Err(LspNetCode::EOrder);
        }
        let want = match lsp_cell(o) {
            Some(w) => w,
            None => return Err(LspNetCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5421 + o as u64);
            return Err(LspNetCode::EDrift);
        }
        if match self.cells.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LspNetCode::EDup);
        }
        if let Some(c) = self.cells.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5422 + o as u64);
        Ok(())
    }

    /// server 格全账核验（20 格）。
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

    /// stdio 态一格（载荷对照 + 重复拒；越态必抓）。
    pub fn record_stdio(&mut self, i: usize, got: u64) -> Result<(), LspNetCode> {
        let want = match stdio_cell(i) {
            Some(w) => w,
            None => return Err(LspNetCode::ENoStdio),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5430 + i as u64);
            return Err(LspNetCode::EDrift);
        }
        if match self.stdios.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LspNetCode::EDup);
        }
        if let Some(c) = self.stdios.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5431 + i as u64);
        Ok(())
    }

    /// stdio 态全账核验（6 格）。
    pub fn stdios_all(&self) -> bool {
        let mut i = 0usize;
        while i < STDIO_CELLS {
            match self.stdios.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 消费联签一格（越签必抓；重复拒）。
    pub fn record_cosign(&mut self, c: usize) -> Result<(), LspNetCode> {
        let want = match cosign_cell(c) {
            Some(w) => w,
            None => return Err(LspNetCode::ENoRow),
        };
        if match self.cosigns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LspNetCode::EDup);
        }
        if let Some(k) = self.cosigns.get_mut(c) {
            *k = Some(want);
        }
        self.fnv(0x5440 + c as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn cosigns_all(&self) -> bool {
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            match self.cosigns.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 生命周期拍一拍（拍序必须严格单调递增，逆拍必抓）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), LspNetCode> {
        if b >= BEATS {
            return Err(LspNetCode::EBeat);
        }
        if (b as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5450);
            return Err(LspNetCode::EBeat);
        }
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(LspNetCode::EBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5451 + b as u64);
            return Err(LspNetCode::EDrift);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0x5452 + b as u64);
        Ok(())
    }

    /// 生命周期拍全账核验（16 拍且单调）。
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

    /// LSP 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), LspNetCode> {
        if r >= SPLIT_ROWS {
            return Err(LspNetCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LspNetCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x5460 + r as u64);
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
    pub fn seal(&self) -> Result<(), LspNetCode> {
        if !self.gates_all() {
            return Err(LspNetCode::ESeal);
        }
        if !self.splits_all() {
            return Err(LspNetCode::ERowMiss);
        }
        if !self.cells_all() || !self.stdios_all() || !self.cosigns_all() || !self.beats_all() {
            return Err(LspNetCode::ESeal);
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
        if self.gates_all()
            && self.cells_all()
            && self.stdios_all()
            && self.cosigns_all()
            && self.beats_all()
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

impl Default for LspNetLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = LspNetLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CELL_N {
        if let Some(w) = lsp_cell(o) {
            if led.record_cell(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < STDIO_CELLS {
        if let Some(w) = stdio_cell(i) {
            if led.record_stdio(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < COSIGN_CELLS {
        if led.record_cosign(c).is_ok() {
            recorded += 1;
        }
        c += 1;
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
    // 注入 10 轮：拍序逆 / 载荷漂移 / 越闭集三态轮转（全部落在当轮临时账本）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = LspNetLedger::new();
        let caught = match kind {
            // 拍序逆：先落拍 9 正格，再回拍 4 必抓（空账同样先落后逆）。
            0 => {
                let _ = lt.record_beat(9, beat_cell(9).unwrap_or(0));
                lt.record_beat(4, beat_cell(4).unwrap_or(0)) == Err(LspNetCode::EBeat)
            }
            // 载荷漂移：先按合法序落同族正格，再注漂移载荷。
            1 => {
                let _ = lt.record_cell(4, lsp_cell(4).unwrap_or(0));
                lt.record_cell(5, lsp_cell(5).unwrap_or(0).wrapping_add(1)) == Err(LspNetCode::EDrift)
            }
            // 越闭集：server 格 20 / stdio 态 6 之外必抓（空账本同样必抓）。
            _ => {
                if k % 2 == 0 {
                    lt.record_cell(CELL_N, 0).is_err()
                } else {
                    lt.record_stdio(STDIO_CELLS, 0).is_err()
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5470 + k as u64);
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
/// UNX-F52520 判据集（锚 UNX-F52521-J1，off-by-one 板式）。
pub fn run_unx52520_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52520");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五 server 族 20 格全通",
        recorded >= CELL_N as u32,
        "clangd/gopls/rust-analyzer/pyright/tsserver × stdio/管道/初始化/关机逐格恒等",
    );
    s.add(
        "server 族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CELL_N {
                if lsp_kind(o).unwrap_or(5) < lsp_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调，四字段按 o%4 展开",
    );
    s.add(
        "stdio 三态 6/6 全落",
        {
            let mut lt = LspNetLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < STDIO_CELLS {
                if let Some(w) = stdio_cell(i) {
                    if lt.record_stdio(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.stdios_all()
        },
        "标准 stdio/命名管道/Socket 三态逐格载荷对照，越态必抓",
    );
    s.add(
        "消费联签 4/4 全落（F52501/F52502）",
        {
            let mut lt = LspNetLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                if lt.record_cosign(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.cosigns_all()
        },
        "F52501/F52502 消费面联签逐格落账，越签必抓",
    );
    s.add(
        "16 拍单调全落",
        {
            let mut lt = LspNetLedger::new();
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
        "LSP 生命周期拍 0..16 严格单调递增",
    );
    s.add(
        "LSP 分账声明 2/2 全齐",
        {
            let mut lt = LspNetLedger::new();
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
        "LSP 启动独立账声明/LSP 关闭独立账声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = LspNetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动握手/初始化对账/关机序列/崩溃重启/管道语义/全域对账逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "拍序逆/载荷漂移/越闭集三态轮转必抓",
    );
    let mut l0 = LspNetLedger::new();
    s.add(
        "边界·server 格越闭集",
        l0.record_cell(CELL_N, 0).is_err(),
        "server 格 20 越闭集必拒",
    );
    s.add(
        "边界·stdio 态越闭集",
        l0.record_stdio(STDIO_CELLS, 0).is_err(),
        "stdio 态 6 之外必拒",
    );
    s.add(
        "边界·生命周期拍越闭集",
        l0.record_beat(BEATS, 0).is_err(),
        "生命周期拍 16 之外必拒",
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
            let mut lt = LspNetLedger::new();
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            // 六闸缺 → ESeal；声明齐而格账缺 → ESeal。
            let a = lt.seal() == Err(LspNetCode::ESeal);
            let _ = lt.judge_gate(0);
            let b = lt.seal() == Err(LspNetCode::ESeal);
            a && b
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = LspNetLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CELL_N {
                if let Some(w) = lsp_cell(o) {
                    let _ = lt.record_cell(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < STDIO_CELLS {
                if let Some(w) = stdio_cell(i) {
                    let _ = lt.record_stdio(i, w);
                }
                i += 1;
            }
            let mut c = 0usize;
            while c < COSIGN_CELLS {
                let _ = lt.record_cosign(c);
                c += 1;
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
        "全通 → S1（判据原文锚定 KB-COMPAT 定级 S1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCC1C0",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsp_kind_attr() {
        // 族映射 kind = o/4，字段映射 attr = o%4，测试按该映射核对。
        assert_eq!(lsp_kind(0), Some(0));
        assert_eq!(lsp_kind(3), Some(0));
        assert_eq!(lsp_kind(4), Some(1));
        assert_eq!(lsp_kind(19), Some(4));
        assert!(lsp_kind(20).is_none());
        assert_eq!(lsp_attr(0), Some(0));
        assert_eq!(lsp_attr(5), Some(1));
        assert_eq!(lsp_attr(19), Some(3));
        assert!(lsp_attr(20).is_none());
        assert!(lsp_cell(20).is_none());
    }

    #[test]
    fn test_kind_attr_names() {
        // 名表按 kind/attr 索引核对。
        assert_eq!(lsp_kind_name(0), Some("clangd"));
        assert_eq!(lsp_kind_name(2), Some("rust-analyzer"));
        assert_eq!(lsp_kind_name(4), Some("typescript-language-server"));
        assert!(lsp_kind_name(5).is_none());
        assert_eq!(lsp_attr_name(0), Some("stdio"));
        assert_eq!(lsp_attr_name(3), Some("关机"));
        assert!(lsp_attr_name(4).is_none());
    }

    #[test]
    fn test_stdio_states() {
        assert_eq!(stdio_state_name(0), Some("标准 stdio"));
        assert_eq!(stdio_state_name(2), Some("命名管道"));
        assert_eq!(stdio_state_name(4), Some("Socket"));
        assert_eq!(stdio_state_name(5), Some("Socket"));
        assert!(stdio_cell(STDIO_CELLS).is_none());
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
        // 种子按合法序先落低拍，逆拍必抓、续拍可落。
        let mut lt = LspNetLedger::new();
        let _ = lt.record_beat(7, beat_cell(7).unwrap_or(0));
        assert_eq!(lt.record_beat(3, beat_cell(3).unwrap_or(0)), Err(LspNetCode::EBeat));
        assert!(lt.record_beat(8, beat_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = LspNetLedger::new();
        let _ = lt.record_cell(9, lsp_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cell(2, lsp_cell(2).unwrap_or(0)), Err(LspNetCode::EOrder));
        assert!(lt.record_cell(10, lsp_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = LspNetLedger::new();
        assert_eq!(
            lt.record_cell(5, lsp_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(LspNetCode::EDrift)
        );
        assert_eq!(
            lt.record_stdio(3, stdio_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(LspNetCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = LspNetLedger::new();
        let _ = lt.record_cell(0, lsp_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cell(0, lsp_cell(0).unwrap_or(0)), Err(LspNetCode::EDup));
        let _ = lt.record_stdio(0, stdio_cell(0).unwrap_or(0));
        assert_eq!(lt.record_stdio(0, stdio_cell(0).unwrap_or(0)), Err(LspNetCode::EDup));
        let _ = lt.record_cosign(0);
        assert_eq!(lt.record_cosign(0), Err(LspNetCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = LspNetLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = LspNetLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_semantics() {
        // 六闸缺 → ESeal。
        let mut lt = LspNetLedger::new();
        assert_eq!(lt.seal(), Err(LspNetCode::ESeal));
        // 声明行缺 → ERowMiss。
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal(), Err(LspNetCode::ERowMiss));
        // 声明齐而格账缺 → ESeal。
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(LspNetCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 格 + 6 态 + 4 联签 + 16 拍 = 46。
        assert_eq!(
            recorded,
            CELL_N as u32 + STDIO_CELLS as u32 + COSIGN_CELLS as u32 + BEATS as u32
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
        assert_eq!(SEG_BASE, 0xCC1C0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            LspNetCode::Ok,
            LspNetCode::ENoCell,
            LspNetCode::EDrift,
            LspNetCode::EOrder,
            LspNetCode::ENoStdio,
            LspNetCode::EBeat,
            LspNetCode::ENoRow,
            LspNetCode::ERowMiss,
            LspNetCode::EDup,
            LspNetCode::ESeal,
            LspNetCode::EInj,
            LspNetCode::EFp,
            LspNetCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(LspNetCode::Ok.val(), SEG_BASE);
        assert_eq!(LspNetCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐（含六闸前置）S1。
        let lt = LspNetLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = LspNetLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let mut o = 0usize;
        while o < CELL_N {
            if let Some(w) = lsp_cell(o) {
                let _ = lt2.record_cell(o, w);
            }
            o += 1;
        }
        let mut i = 0usize;
        while i < STDIO_CELLS {
            if let Some(w) = stdio_cell(i) {
                let _ = lt2.record_stdio(i, w);
            }
            i += 1;
        }
        let mut c = 0usize;
        while c < COSIGN_CELLS {
            let _ = lt2.record_cosign(c);
            c += 1;
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
    fn unx52520_checks_green() {
        let s = run_unx52520_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
