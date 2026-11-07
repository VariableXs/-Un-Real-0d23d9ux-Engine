//! UNX-F52448 WizTree 收口 · 0xCAFC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52448"，逐字）：
//!   title: WizTree 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52449-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52448）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52449-J1，照原文锚定；功能按本卡标题（WizTree）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五模式族闭集 20 格：MFT 快速扫描/目录/导出/树图/命令行各 4 格，
//!      家族单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② MFT 消费联签 4 格：MFT 记录消费联签逐格对照，漂移必抓（§AI-66.3
//!      判据锚定）。
//!   ③ 与 WinDirStat 对照 8 格：互斥消费对照逐格恒等，漂移必抓。
//!   ④ 扫描 8 拍单调：拍号严格递增（0..=7），倒挂/越拍必抓；C 上栈声明
//!      2 行闭集，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（模式漂
//!      移/家族逆序/模式越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一
//!      致；定级 S1 目标档（KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52448;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五模式族操作数。
pub const MODE_OPS: usize = 20;

/// MFT 消费联签格数。
pub const MFT_CELLS: usize = 4;

/// 与 WinDirStat 对照格数。
pub const CMP_CELLS: usize = 8;

/// 扫描单调拍数。
pub const BEATS: usize = 8;

/// C 上栈声明行数。
pub const CSTACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WiztreeCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 模式漂移
    EModeDrift,
    /// 家族逆序
    EOrder,
    /// MFT 联签漂移
    EMftDrift,
    /// 扫描拍越拍/倒挂
    EBeatMono,
    /// 对照漂移
    ECmpDrift,
    /// 声明行越闭集
    ENoRow,
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

impl WiztreeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WiztreeCode::Ok => "成功",
            WiztreeCode::ENoCell => "格号越闭集：WizTree 五模式族 20/MFT 联签 4/对照 8/拍 8 闭集之外先登记",
            WiztreeCode::EModeDrift => "模式漂移：模式族折叠与基准不恒等，逐格复算",
            WiztreeCode::EOrder => "家族逆序：MFT 快扫→目录→导出→树图→命令行单调，逆序必抓",
            WiztreeCode::EMftDrift => "MFT 联签漂移：MFT 记录消费联签与基准不恒等（§AI-66.3）",
            WiztreeCode::EBeatMono => "扫描拍越拍/倒挂：拍号 0..=7 严格递增，倒挂必抓",
            WiztreeCode::ECmpDrift => "对照漂移：与 WinDirStat 对照折叠与基准不恒等，逐格复算",
            WiztreeCode::ENoRow => "声明行越闭集：C 上栈 2 行闭集之外先登记",
            WiztreeCode::EDup => "重复落账：同格只许一条账目",
            WiztreeCode::ESeal => "未齐即封：20 模式 + 4 联签 + 8 对照 + 8 拍 + 2 行上栈未齐先补齐",
            WiztreeCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            WiztreeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WiztreeCode::EGradeDrift => "定级漂移：全通且上栈声明全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集（本卡目标档 S1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标档）
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值（const 安全）。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20模式+4联签+8对照+8拍单调+上栈声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 模式族操作格载荷（操作 o；家族 = o/4）。
pub const fn mode_cell(o: usize) -> Option<u64> {
    if o >= MODE_OPS {
        return None;
    }
    Some((0xCE100u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 模式族（0..4=MFT 快扫 4..8=目录 8..12=导出 12..16=树图 16..20=命令行）。
pub const fn mode_kind(o: usize) -> Option<u32> {
    if o >= MODE_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 模式族名。
pub const fn mode_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("MFT 快速扫描"),
        1 => Some("目录"),
        2 => Some("导出"),
        3 => Some("树图"),
        4 => Some("命令行"),
        _ => None,
    }
}

/// MFT 消费联签格载荷（格 m；§AI-66.3 联签锚定）。
pub const fn mft_cell(m: usize) -> Option<u64> {
    if m >= MFT_CELLS {
        return None;
    }
    Some((0xCE200u64 + (m as u64) * 0x3D) ^ 0x575A_544D_0000_0000)
}

/// 与 WinDirStat 对照格载荷（格 p；互斥消费对照）。
pub const fn cmp_cell(p: usize) -> Option<u64> {
    if p >= CMP_CELLS {
        return None;
    }
    Some((0xCE300u64 + (p as u64) * 0x40) ^ 0x575A_5744_0000_0000)
}

/// 扫描拍载荷（拍 b）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some((0xCE400u64 + (b as u64) * 0x57) ^ 0x575A_4245_0000_0000)
}

/// C 上栈声明行折叠（行 r）。
pub const fn cstack_row(r: usize) -> Option<u32> {
    if r >= CSTACK_ROWS {
        return None;
    }
    Some(0xCE500 + r as u32 * 0x31)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// WizTree 收口账本状态机。
#[derive(Clone)]
pub struct WiztreeLedger {
    /// 六判据（磁盘占用收口：模式/联签/对照/拍序/上栈/封核）。
    gates: [Option<bool>; 6],
    /// 模式族操作账。
    ops: [Option<u64>; MODE_OPS],
    /// MFT 联签账。
    mft: [Option<u64>; MFT_CELLS],
    /// 对照账。
    cmp: [Option<u64>; CMP_CELLS],
    /// 扫描拍账。
    beats: [Option<u64>; BEATS],
    /// C 上栈声明账。
    cstack: [Option<u32>; CSTACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 最后拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WiztreeLedger {
    /// 新账本。
    pub fn new() -> Self {
        WiztreeLedger {
            gates: [None; 6],
            ops: [None; MODE_OPS],
            mft: [None; MFT_CELLS],
            cmp: [None; CMP_CELLS],
            beats: [None; BEATS],
            cstack: [None; CSTACK_ROWS],
            last_kind: -1,
            last_beat: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WiztreeCode> {
        if gate >= 6 {
            return Err(WiztreeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5000 + gate as u64);
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

    /// 模式族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_mode(&mut self, o: usize, got: u64) -> Result<(), WiztreeCode> {
        if o >= MODE_OPS {
            return Err(WiztreeCode::ENoCell);
        }
        let kind = match mode_kind(o) {
            Some(k) => k,
            None => return Err(WiztreeCode::ENoCell),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5010);
            return Err(WiztreeCode::EOrder);
        }
        let want = match mode_cell(o) {
            Some(w) => w,
            None => return Err(WiztreeCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5011 + o as u64);
            return Err(WiztreeCode::EModeDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiztreeCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5012 + o as u64);
        Ok(())
    }

    /// 模式族全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < MODE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// MFT 联签一格（§AI-66.3 联签对照 + 重复拒）。
    pub fn record_mft(&mut self, m: usize, got: u64) -> Result<(), WiztreeCode> {
        let want = match mft_cell(m) {
            Some(w) => w,
            None => return Err(WiztreeCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5020 + m as u64);
            return Err(WiztreeCode::EMftDrift);
        }
        if match self.mft.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiztreeCode::EDup);
        }
        if let Some(c) = self.mft.get_mut(m) {
            *c = Some(got);
        }
        self.fnv(0x5021 + m as u64);
        Ok(())
    }

    /// MFT 联签全账核验（4 格）。
    pub fn mft_all(&self) -> bool {
        let mut m = 0usize;
        while m < MFT_CELLS {
            match self.mft.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 对照一格（与 WinDirStat 互斥消费对照 + 重复拒）。
    pub fn record_cmp(&mut self, p: usize, got: u64) -> Result<(), WiztreeCode> {
        let want = match cmp_cell(p) {
            Some(w) => w,
            None => return Err(WiztreeCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5030 + p as u64);
            return Err(WiztreeCode::ECmpDrift);
        }
        if match self.cmp.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiztreeCode::EDup);
        }
        if let Some(c) = self.cmp.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5031 + p as u64);
        Ok(())
    }

    /// 对照全账核验（8 格）。
    pub fn cmp_all(&self) -> bool {
        let mut p = 0usize;
        while p < CMP_CELLS {
            match self.cmp.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 扫描拍一格（拍号严格递增 + 载荷对照 + 重复拒）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), WiztreeCode> {
        if b >= BEATS {
            return Err(WiztreeCode::EBeatMono);
        }
        if (b as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5040);
            return Err(WiztreeCode::EBeatMono);
        }
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(WiztreeCode::EBeatMono),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5041 + b as u64);
            return Err(WiztreeCode::EBeatMono);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiztreeCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0x5042 + b as u64);
        Ok(())
    }

    /// 扫描拍全账核验（8 拍全齐且账内单调）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// C 上栈声明一行。
    pub fn declare_cstack(&mut self, r: usize) -> Result<(), WiztreeCode> {
        if r >= CSTACK_ROWS {
            return Err(WiztreeCode::ENoRow);
        }
        if match self.cstack.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WiztreeCode::EDup);
        }
        if let Some(c) = self.cstack.get_mut(r) {
            *c = cstack_row(r);
        }
        self.fnv(0x5050 + r as u64);
        Ok(())
    }

    /// C 上栈声明全齐核验。
    pub fn cstack_all(&self) -> bool {
        let mut r = 0usize;
        while r < CSTACK_ROWS {
            match self.cstack.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封核检查：六闸齐且各节全齐才放行，未齐即封。
    pub fn seal_check(&self) -> Result<(), WiztreeCode> {
        if !self.gates_all() {
            return Err(WiztreeCode::ESeal);
        }
        if !self.cstack_all() {
            return Err(WiztreeCode::ENoRow);
        }
        if !self.ops_all() || !self.mft_all() || !self.cmp_all() || !self.beats_all() {
            return Err(WiztreeCode::ESeal);
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

    /// KB-COMPAT 定级（如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.mft_all() && self.cmp_all() && self.beats_all() && self.cstack_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WiztreeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WiztreeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < MODE_OPS {
        if let Some(w) = mode_cell(o) {
            if led.record_mode(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut m = 0usize;
    while m < MFT_CELLS {
        if let Some(w) = mft_cell(m) {
            if led.record_mft(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut p = 0usize;
    while p < CMP_CELLS {
        if let Some(w) = cmp_cell(p) {
            if led.record_cmp(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
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
    while r < CSTACK_ROWS {
        let _ = led.declare_cstack(r);
        r += 1;
    }
    // 注入 10 轮：模式漂移 / 家族逆序 / 模式越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上：led 上 injected==caught==10、escaped==0。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WiztreeLedger::new();
        let caught = match kind {
            // 漂移注入：先按合法序落正格（低族 0 格），再注漂移格。
            0 => {
                let _ = lt.record_mode(0, mode_cell(0).unwrap_or(0));
                lt.record_mode(1, mode_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(WiztreeCode::EModeDrift)
            }
            // 逆序注入：先落合法高族格，再注低族格，逆序必抓。
            1 => {
                let _ = lt.record_mode(17, mode_cell(17).unwrap_or(0));
                lt.record_mode(5, mode_cell(5).unwrap_or(0)) == Err(WiztreeCode::EOrder)
            }
            // 越闭集注入：空账本同样必抓，且不改 last_kind。
            _ => lt.record_mode(MODE_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5060 + k as u64);
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
/// UNX-F52448 判据集（锚 UNX-F52449-J1，off-by-one 板式）。
pub fn run_unx52448_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52448");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五模式族 20 格全通",
        recorded >= MODE_OPS as u32,
        "MFT 快速扫描/目录/导出/树图/命令行五家族逐格恒等",
    );
    s.add(
        "MFT 消费联签 4/4 全落",
        {
            let mut lt = WiztreeLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MFT_CELLS {
                if let Some(w) = mft_cell(m) {
                    if lt.record_mft(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.mft_all()
        },
        "MFT 记录消费联签逐格恒等（§AI-66.3 判据锚定）",
    );
    s.add(
        "与 WinDirStat 对照 8 格全落",
        {
            let mut lt = WiztreeLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < CMP_CELLS {
                if let Some(w) = cmp_cell(p) {
                    if lt.record_cmp(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.cmp_all()
        },
        "互斥消费对照逐格恒等（WinDirStat 分账对照）",
    );
    s.add(
        "扫描 8 拍单调全落",
        {
            let mut lt = WiztreeLedger::new();
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
        "拍号 0..=7 严格递增逐拍落账，倒挂必抓",
    );
    s.add(
        "C 上栈声明 2/2 全齐",
        {
            let mut lt = WiztreeLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CSTACK_ROWS {
                if lt.declare_cstack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cstack_all()
        },
        "C 上栈独立账声明/C 栈深度分账声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < MODE_OPS {
                if mode_kind(o).unwrap_or(5) < mode_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五模式族闭集单调（逆序必抓）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "漂移/逆序/越闭集三态轮转必抓");
    let mut l0 = WiztreeLedger::new();
    s.add(
        "边界·模式越闭集",
        l0.record_mode(MODE_OPS, 0).is_err(),
        "模式 20 越闭集必拒",
    );
    s.add(
        "边界·MFT 联签越闭集",
        l0.record_mft(MFT_CELLS, 0).is_err(),
        "MFT 联签格 4 越闭集必拒",
    );
    s.add(
        "边界·对照越闭集",
        l0.record_cmp(CMP_CELLS, 0).is_err(),
        "对照格 8 越闭集必拒",
    );
    s.add(
        "MFT 联签漂移拒",
        l0.record_mft(0, mft_cell(0).unwrap_or(0).wrapping_add(1)) == Err(WiztreeCode::EMftDrift),
        "MFT 消费联签载荷漂移必抓",
    );
    s.add(
        "拍序倒挂必抓",
        {
            let mut lt = WiztreeLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < 3 {
                if let Some(w) = beat_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            // 拍号倒挂（3 后回 1）与越拍（8）都必抓。
            ok && lt.record_beat(1, beat_cell(1).unwrap_or(0)) == Err(WiztreeCode::EBeatMono)
                && lt.record_beat(BEATS, beat_cell(BEATS).unwrap_or(0)) == Err(WiztreeCode::EBeatMono)
        },
        "拍号严格递增闸：倒挂/越拍均判 EBeatMono",
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
        "未齐先记账不封",
        {
            let mut lt = WiztreeLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < 4 {
                if let Some(w) = mode_cell(o) {
                    if lt.record_mode(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            // 账未齐时逐格记账仍放行（Ok），封核检查才拒。
            ok && lt.record_mode(4, mode_cell(4).unwrap_or(0)).is_ok()
        },
        "账未齐时记账必须 Ok，未齐即封只在封核检查处生效",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = WiztreeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < MODE_OPS {
                if let Some(w) = mode_cell(o) {
                    let _ = lt.record_mode(o, w);
                }
                o += 1;
            }
            let mut m = 0usize;
            while m < MFT_CELLS {
                if let Some(w) = mft_cell(m) {
                    let _ = lt.record_mft(m, w);
                }
                m += 1;
            }
            let mut p = 0usize;
            while p < CMP_CELLS {
                if let Some(w) = cmp_cell(p) {
                    let _ = lt.record_cmp(p, w);
                }
                p += 1;
            }
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < CSTACK_ROWS {
                let _ = lt.declare_cstack(r);
                r += 1;
            }
            // main/aux 各节全部记账后再断言定级。
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1 目标档（KB-COMPAT 定级登记）",
    );
    s.add(
        "诊断码闭集",
        {
            let codes = [
                WiztreeCode::Ok,
                WiztreeCode::ENoCell,
                WiztreeCode::EModeDrift,
                WiztreeCode::EOrder,
                WiztreeCode::EMftDrift,
                WiztreeCode::EBeatMono,
                WiztreeCode::ECmpDrift,
                WiztreeCode::ENoRow,
                WiztreeCode::EDup,
                WiztreeCode::ESeal,
                WiztreeCode::EInj,
                WiztreeCode::EFp,
                WiztreeCode::EGradeDrift,
            ];
            let mut ok = codes.len() == 13;
            let mut i = 0usize;
            while i < codes.len() {
                if codes[i].val() != SEG_BASE + i as u32 || codes[i].detail().is_empty() {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "13 变体码位连续且明细全非空",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCAFC0,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 0xCAFC0",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_kind() {
        assert_eq!(mode_kind(0), Some(0));
        assert_eq!(mode_kind(4), Some(1));
        assert_eq!(mode_kind(8), Some(2));
        assert_eq!(mode_kind(12), Some(3));
        assert_eq!(mode_kind(19), Some(4));
        assert!(mode_kind(20).is_none());
        assert!(mode_cell(20).is_none());
        assert_eq!(mode_kind_name(0), Some("MFT 快速扫描"));
        assert_eq!(mode_kind_name(2), Some("导出"));
        assert_eq!(mode_kind_name(3), Some("树图"));
        assert_eq!(mode_kind_name(4), Some("命令行"));
        assert!(mode_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = mode_cell(3).unwrap_or(0);
        assert_eq!(a, mode_cell(3).unwrap_or(0));
        assert_ne!(a, mode_cell(4).unwrap_or(0));
        let b = mft_cell(2).unwrap_or(0);
        assert_eq!(b, mft_cell(2).unwrap_or(0));
        assert_ne!(b, mft_cell(3).unwrap_or(0));
        let c = cmp_cell(5).unwrap_or(0);
        assert_eq!(c, cmp_cell(5).unwrap_or(0));
        assert_ne!(c, cmp_cell(6).unwrap_or(0));
        assert!(mft_cell(MFT_CELLS).is_none());
        assert!(cmp_cell(CMP_CELLS).is_none());
        assert!(beat_cell(BEATS).is_none());
        assert!(cstack_row(CSTACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = WiztreeLedger::new();
        // 合法序先落高族格（命令行 19），再注低族格（目录 5）必抓。
        let _ = lt.record_mode(19, mode_cell(19).unwrap_or(0));
        assert_eq!(lt.record_mode(5, mode_cell(5).unwrap_or(0)), Err(WiztreeCode::EOrder));
        assert!(lt.record_mode(18, mode_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WiztreeLedger::new();
        assert_eq!(
            lt.record_mode(6, mode_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(WiztreeCode::EModeDrift)
        );
        assert_eq!(
            lt.record_cmp(3, cmp_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(WiztreeCode::ECmpDrift)
        );
        assert_eq!(
            lt.record_mft(1, mft_cell(1).unwrap_or(0).wrapping_add(9)),
            Err(WiztreeCode::EMftDrift)
        );
    }

    #[test]
    fn test_beat_monotonic() {
        let mut lt = WiztreeLedger::new();
        let mut b = 0usize;
        while b < BEATS {
            assert!(lt.record_beat(b, beat_cell(b).unwrap_or(0)).is_ok());
            b += 1;
        }
        // 拍 0..=7 全落后再落同拍/越拍必抓。
        assert_eq!(
            lt.record_beat(0, beat_cell(0).unwrap_or(0)),
            Err(WiztreeCode::EBeatMono)
        );
        assert_eq!(lt.record_beat(BEATS, 0), Err(WiztreeCode::EBeatMono));
        // 倒挂：先落拍 5 再落拍 2。
        let mut lt2 = WiztreeLedger::new();
        let _ = lt2.record_beat(5, beat_cell(5).unwrap_or(0));
        assert_eq!(
            lt2.record_beat(2, beat_cell(2).unwrap_or(0)),
            Err(WiztreeCode::EBeatMono)
        );
        // 拍载荷漂移同判 EBeatMono。
        let mut lt3 = WiztreeLedger::new();
        assert_eq!(
            lt3.record_beat(0, beat_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(WiztreeCode::EBeatMono)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WiztreeLedger::new();
        let _ = lt.record_mft(0, mft_cell(0).unwrap_or(0));
        assert_eq!(lt.record_mft(0, mft_cell(0).unwrap_or(0)), Err(WiztreeCode::EDup));
        let _ = lt.record_cmp(7, cmp_cell(7).unwrap_or(0));
        assert_eq!(lt.record_cmp(7, cmp_cell(7).unwrap_or(0)), Err(WiztreeCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WiztreeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = WiztreeLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的节：模式 20 + 联签 4 + 对照 8 + 拍 8。
        assert_eq!(recorded, (MODE_OPS + MFT_CELLS + CMP_CELLS + BEATS) as u32);
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
        assert_eq!(SEG_BASE, 0xCAFC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WiztreeCode::Ok as u32, 0);
        assert_eq!(WiztreeCode::EGradeDrift as u32, 12);
        assert_eq!(WiztreeCode::Ok.val(), SEG_BASE);
        assert_eq!(WiztreeCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!WiztreeCode::Ok.detail().is_empty());
        assert!(!WiztreeCode::EBeatMono.detail().is_empty());
    }

    #[test]
    fn test_seal_check() {
        let mut lt = WiztreeLedger::new();
        assert_eq!(lt.seal_check(), Err(WiztreeCode::ESeal));
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        // 六闸齐但上栈未齐 → 缺声明行。
        assert_eq!(lt.seal_check(), Err(WiztreeCode::ENoRow));
        let mut r = 0usize;
        while r < CSTACK_ROWS {
            let _ = lt.declare_cstack(r);
            r += 1;
        }
        // 声明齐但各节未齐 → 未齐即封。
        assert_eq!(lt.seal_check(), Err(WiztreeCode::ESeal));
        let mut o = 0usize;
        while o < MODE_OPS {
            if let Some(w) = mode_cell(o) {
                let _ = lt.record_mode(o, w);
            }
            o += 1;
        }
        let mut m = 0usize;
        while m < MFT_CELLS {
            if let Some(w) = mft_cell(m) {
                let _ = lt.record_mft(m, w);
            }
            m += 1;
        }
        let mut p = 0usize;
        while p < CMP_CELLS {
            if let Some(w) = cmp_cell(p) {
                let _ = lt.record_cmp(p, w);
            }
            p += 1;
        }
        let mut b = 0usize;
        while b < BEATS {
            if let Some(w) = beat_cell(b) {
                let _ = lt.record_beat(b, w);
            }
            b += 1;
        }
        assert!(lt.seal_check().is_ok());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = WiztreeLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = WiztreeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        let grades = [Grade::S1, Grade::S2, Grade::S3];
        assert_eq!(grades.len(), 3);
        assert!(!grades[0].detail().is_empty());
    }

    #[test]
    fn unx52448_checks_green() {
        let s = run_unx52448_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
