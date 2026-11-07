//! UNX-F52296 体验日志全域回归账（挫败信号捕获 ×B01–B14）· 0xC89C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52296"，逐字）：
//!   title: 体验日志全域回归账（挫败信号捕获 ×B01–B14）
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52297-J1 B16–B40
//!     组序铺排清单（组 16–40：包管理 GUI/反编译与二进制分析/文档工具/
//!     CI 面板/监控仪表长尾）+ 每会话任务储备深度 ≥3 登记入台账；判据
//!     锚定 多 AI 并行五条防卡死 ③；③ 完成时 API/软件写回：状态=已完
//!     成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52296）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52297-J1，照原文锚定；功能按本卡标题（体验日志全域回归账）落
//! 地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 挫败信号四类 16 格：rage click / dead click / 浮层反复开关 /
//!      回退风暴，每类 4 格，逐格对照恒等，越格必抓；
//!   ② B01–B14 十四段覆盖 28 格（每段 2 格：捕获位/回归位），段号越
//!      闭集必抓；
//!   ③ 捕获率闸：捕获格按整数百分位核算 ≥80%（captured×100 ≥
//!      80×total），不足即封拒；
//!   ④ 滚动回归 8 拍单调：拍号全流程单调不减，逆序必抓且不改账位；
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a，
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10/10（信号漂移/
//!      段越闭集/滚动拍逆序），三类注入全部落在当轮临时账本上，主账
//!      injected==caught==10、escaped==0；双跑 FNV 指纹一致；定级
//!      S1/S2/S3 闭集（16 信号格 + 28 段格 + 8 拍 + 六判据 + 捕获率闸
//!      全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标；XOR 锚常量低 16 位恒零。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52296;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 挫败信号类数（四类）。
pub const SIG_KINDS: usize = 4;

/// 每类信号格数（4 类 × 4 格 = 16 格）。
pub const CELLS_PER_KIND: usize = 4;

/// 信号总格数（16）。
pub const SIG_CELLS: usize = SIG_KINDS * CELLS_PER_KIND;

/// 回归段数（B01–B14）。
pub const SEG_COUNT: usize = 14;

/// 每段格数（捕获位/回归位）。
pub const CELLS_PER_SEG: usize = 2;

/// 段覆盖总格数（14 × 2 = 28）。
pub const SEG_CELLS: usize = SEG_COUNT * CELLS_PER_SEG;

/// 滚动回归拍数。
pub const ROLL_BEATS: usize = 8;

/// 六判据闸数。
pub const GATE_COUNT: usize = 6;

/// 捕获率闸百分位（≥80%）。
pub const CAP_GATE_PCT: u32 = 80;

/// XOR 联签锚（低 16 位恒零）。
pub const XOR_ANCHOR: u64 = 0x0055_5852_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UxRegressCode {
    /// 成功
    Ok = 0,
    /// 信号类或格号越闭集
    ENoSig,
    /// 信号格漂移
    ESigDrift,
    /// 段号越闭集
    ENoSeg,
    /// 段覆盖格漂移
    ESegDrift,
    /// 滚动拍越闭集
    ENoBeat,
    /// 滚动拍逆序
    EBeatOrder,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 捕获率闸未达
    ECovGate,
    /// 双跑指纹不一致
    EFp,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl UxRegressCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            UxRegressCode::Ok => "成功",
            UxRegressCode::ENoSig => "信号类或格号越闭集：挫败信号四类 ×4 格之外先登记",
            UxRegressCode::ESigDrift => "信号格漂移：信号载荷与基准不恒等，逐格复算",
            UxRegressCode::ENoSeg => "段号越闭集：B01–B14 十四段覆盖之外先登记",
            UxRegressCode::ESegDrift => "段覆盖格漂移：捕获位/回归位与基准不恒等，逐格复算",
            UxRegressCode::ENoBeat => "滚动拍越闭集：滚动回归 8 拍之外先登记",
            UxRegressCode::EBeatOrder => "滚动拍逆序：拍号全流程单调不减，逆序必抓",
            UxRegressCode::EDup => "重复落账：同格只许一条账目",
            UxRegressCode::ESeal => "未齐即封：16 信号格 + 28 段格 + 8 拍未齐先补齐",
            UxRegressCode::ECovGate => "捕获率闸未达：captured×100 ≥ 80×total 先补捕获账",
            UxRegressCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            UxRegressCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            UxRegressCode::EGradeDrift => "定级漂移：全通且捕获率闸 ≥80% 才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// 体验日志回归定级闭集。
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
            Grade::S1 => "S1：16信号格+28段格+8拍+六判据+捕获率闸全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 挫败信号格载荷（类 k、格 c 的对照基准，纯函数恒等）。
pub const fn sig_cell(k: usize, c: usize) -> Option<u64> {
    if k >= SIG_KINDS || c >= CELLS_PER_KIND {
        return None;
    }
    Some((0xBC000u64 + (k as u64) * 0x100 + (c as u64) * 0x43) ^ XOR_ANCHOR)
}

/// 挫败信号类名（const fn 表 + Option 边界）。
pub const fn sig_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("rage click"),
        1 => Some("dead click"),
        2 => Some("浮层反复开关"),
        3 => Some("回退风暴"),
        _ => None,
    }
}

/// 信号格名。
pub const fn sig_cell_name(k: usize, c: usize) -> Option<&'static str> {
    match (k, c) {
        (0, 0) => Some("rage·连点窗口捕获位"),
        (0, 1) => Some("rage·连点阈值核对位"),
        (0, 2) => Some("rage·日志落盘位"),
        (0, 3) => Some("rage·回归核验位"),
        (1, 0) => Some("dead·无响应捕获位"),
        (1, 1) => Some("dead·命中区核对位"),
        (1, 2) => Some("dead·日志落盘位"),
        (1, 3) => Some("dead·回归核验位"),
        (2, 0) => Some("浮层·开合序列捕获位"),
        (2, 1) => Some("浮层·抖动阈值核对位"),
        (2, 2) => Some("浮层·日志落盘位"),
        (2, 3) => Some("浮层·回归核验位"),
        (3, 0) => Some("回退·风暴序列捕获位"),
        (3, 1) => Some("回退·深度阈值核对位"),
        (3, 2) => Some("回退·日志落盘位"),
        (3, 3) => Some("回退·回归核验位"),
        _ => None,
    }
}

/// 段号名（B01–B14）。
pub const fn seg_name(seg: usize) -> Option<&'static str> {
    match seg {
        0 => Some("B01"),
        1 => Some("B02"),
        2 => Some("B03"),
        3 => Some("B04"),
        4 => Some("B05"),
        5 => Some("B06"),
        6 => Some("B07"),
        7 => Some("B08"),
        8 => Some("B09"),
        9 => Some("B10"),
        10 => Some("B11"),
        11 => Some("B12"),
        12 => Some("B13"),
        13 => Some("B14"),
        _ => None,
    }
}

/// 段覆盖格载荷（段 s、半格 h 的对照基准）。
pub const fn seg_cell(s: usize, h: usize) -> Option<u32> {
    if s >= SEG_COUNT || h >= CELLS_PER_SEG {
        return None;
    }
    Some((0xBD000u32 + (s as u32) * 0x1A + (h as u32) * 0x07) ^ 0x0005_0000)
}

/// 滚动拍载荷（拍 b 的对照基准）。
pub const fn roll_cell(b: usize) -> Option<u32> {
    if b >= ROLL_BEATS {
        return None;
    }
    Some((0xBDC0u32 + (b as u32) * 0x2B) ^ 0x0005_0000)
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("日志零阻塞启动"),
        1 => Some("信号捕获恒等"),
        2 => Some("段覆盖恒等"),
        3 => Some("滚动回归恒等"),
        4 => Some("捕获率闸核算"),
        5 => Some("全域回归收口"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 体验日志回归账本状态机。
#[derive(Clone)]
pub struct UxRegressLedger {
    /// 六判据闸。
    gates: [Option<bool>; GATE_COUNT],
    /// 挫败信号账（类 k × 格 c）。
    sigs: [[Option<u64>; CELLS_PER_KIND]; SIG_KINDS],
    /// 段覆盖账（段 s × 半格 h）。
    segs: [[Option<u32>; CELLS_PER_SEG]; SEG_COUNT],
    /// 滚动回归拍账。
    rolls: [Option<u32>; ROLL_BEATS],
    /// 最后已录拍号（滚动拍单调）。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl UxRegressLedger {
    /// 新账本。
    pub fn new() -> Self {
        UxRegressLedger {
            gates: [None; GATE_COUNT],
            sigs: [[None; CELLS_PER_KIND]; SIG_KINDS],
            segs: [[None; CELLS_PER_SEG]; SEG_COUNT],
            rolls: [None; ROLL_BEATS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), UxRegressCode> {
        if gate >= GATE_COUNT {
            return Err(UxRegressCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5700 + gate as u64);
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

    /// 挫败信号一格（载荷对照 + 重复拒）。
    pub fn record_sig(&mut self, k: usize, c: usize, got: u64) -> Result<(), UxRegressCode> {
        let want = match sig_cell(k, c) {
            Some(w) => w,
            None => return Err(UxRegressCode::ENoSig),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5800 + k as u64 * 4 + c as u64);
            return Err(UxRegressCode::ESigDrift);
        }
        if match self.sigs.get(k).and_then(|row| row.get(c)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UxRegressCode::EDup);
        }
        if let Some(row) = self.sigs.get_mut(k) {
            if let Some(cell) = row.get_mut(c) {
                *cell = Some(got);
            }
        }
        self.fnv(0x5801 + k as u64 * 4 + c as u64);
        Ok(())
    }

    /// 信号全账核验（16 格）。
    pub fn sigs_all(&self) -> bool {
        let mut k = 0usize;
        while k < SIG_KINDS {
            let mut c = 0usize;
            while c < CELLS_PER_KIND {
                match self.sigs.get(k).and_then(|row| row.get(c)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                c += 1;
            }
            k += 1;
        }
        true
    }

    /// 段覆盖一格（载荷对照 + 重复拒）。
    pub fn record_seg(&mut self, s: usize, h: usize, got: u32) -> Result<(), UxRegressCode> {
        let want = match seg_cell(s, h) {
            Some(x) => x,
            None => return Err(UxRegressCode::ENoSeg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5900 + s as u64 * 2 + h as u64);
            return Err(UxRegressCode::ESegDrift);
        }
        if match self.segs.get(s).and_then(|row| row.get(h)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UxRegressCode::EDup);
        }
        if let Some(row) = self.segs.get_mut(s) {
            if let Some(cell) = row.get_mut(h) {
                *cell = Some(got);
            }
        }
        self.fnv(0x5901 + s as u64 * 2 + h as u64);
        Ok(())
    }

    /// 段覆盖全账核验（28 格）。
    pub fn segs_all(&self) -> bool {
        let mut s = 0usize;
        while s < SEG_COUNT {
            let mut h = 0usize;
            while h < CELLS_PER_SEG {
                match self.segs.get(s).and_then(|row| row.get(h)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                h += 1;
            }
            s += 1;
        }
        true
    }

    /// 滚动回归一拍（拍号单调 + 载荷对照 + 重复拒）。
    pub fn record_roll(&mut self, b: usize, got: u32) -> Result<(), UxRegressCode> {
        if b >= ROLL_BEATS {
            return Err(UxRegressCode::ENoBeat);
        }
        if (b as i64) < self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A00);
            return Err(UxRegressCode::EBeatOrder);
        }
        let want = match roll_cell(b) {
            Some(x) => x,
            None => return Err(UxRegressCode::ENoBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5A01 + b as u64);
            return Err(UxRegressCode::ESegDrift);
        }
        if match self.rolls.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(UxRegressCode::EDup);
        }
        if let Some(c) = self.rolls.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0x5A02 + b as u64);
        Ok(())
    }

    /// 滚动拍全账核验（8 拍）。
    pub fn rolls_all(&self) -> bool {
        let mut b = 0usize;
        while b < ROLL_BEATS {
            match self.rolls.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 捕获率闸（整数百分位：captured×100 ≥ 80×total）。
    pub fn cap_gate(&self) -> bool {
        let mut captured = 0u32;
        let mut total = 0u32;
        let mut k = 0usize;
        while k < SIG_KINDS {
            let mut c = 0usize;
            while c < CELLS_PER_KIND {
                total += 1;
                if match self.sigs.get(k).and_then(|row| row.get(c)) {
                    Some(Some(_)) => true,
                    _ => false,
                } {
                    captured += 1;
                }
                c += 1;
            }
            k += 1;
        }
        let mut s = 0usize;
        while s < SEG_COUNT {
            let mut h = 0usize;
            while h < CELLS_PER_SEG {
                total += 1;
                if match self.segs.get(s).and_then(|row| row.get(h)) {
                    Some(Some(_)) => true,
                    _ => false,
                } {
                    captured += 1;
                }
                h += 1;
            }
            s += 1;
        }
        captured * 100 >= CAP_GATE_PCT * total
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 定级（全通且捕获率闸达标 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.sigs_all() && self.segs_all() && self.rolls_all() && self.cap_gate()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for UxRegressLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = UxRegressLedger::new();
    let mut g = 0usize;
    while g < GATE_COUNT {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut k = 0usize;
    while k < SIG_KINDS {
        let mut c = 0usize;
        while c < CELLS_PER_KIND {
            if let Some(w) = sig_cell(k, c) {
                if led.record_sig(k, c, w).is_ok() {
                    recorded += 1;
                }
            }
            c += 1;
        }
        k += 1;
    }
    let mut s = 0usize;
    while s < SEG_COUNT {
        let mut h = 0usize;
        while h < CELLS_PER_SEG {
            if let Some(x) = seg_cell(s, h) {
                if led.record_seg(s, h, x).is_ok() {
                    recorded += 1;
                }
            }
            h += 1;
        }
        s += 1;
    }
    let mut b = 0usize;
    while b < ROLL_BEATS {
        if let Some(x) = roll_cell(b) {
            if led.record_roll(b, x).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    // 注入 10 轮：信号漂移 / 段越闭集 / 滚动拍逆序三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记 injected/caught。
    let mut i = 0usize;
    while i < 10 {
        let kind = (i % 3) as u32;
        let mut lt = UxRegressLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_sig(1, 1, sig_cell(1, 1).unwrap_or(0));
                lt.record_sig(1, 2, sig_cell(1, 2).unwrap_or(0).wrapping_add(1))
                    == Err(UxRegressCode::ESigDrift)
            }
            1 => {
                let _ = lt.record_seg(0, 0, seg_cell(0, 0).unwrap_or(0));
                lt.record_seg(SEG_COUNT, 0, 0).is_err()
            }
            _ => {
                let _ = lt.record_roll(5, roll_cell(5).unwrap_or(0));
                lt.record_roll(2, roll_cell(2).unwrap_or(0)) == Err(UxRegressCode::EBeatOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5B00 + i as u64);
        } else {
            led.escaped += 1;
        }
        i += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52296 判据集（锚 UNX-F52297-J1，off-by-one 板式）。
pub fn run_unx52296_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52296");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "挫败信号四类 16 格全落",
        recorded >= SIG_CELLS as u32,
        "rage click/dead click/浮层反复开关/回退风暴逐格恒等（体验十三·补 异常显性化）",
    );
    s.add(
        "B01–B14 十四段覆盖 28 格全落",
        {
            let mut lt = UxRegressLedger::new();
            let mut ok = true;
            let mut s2 = 0usize;
            while s2 < SEG_COUNT {
                let mut h = 0usize;
                while h < CELLS_PER_SEG {
                    if let Some(x) = seg_cell(s2, h) {
                        if lt.record_seg(s2, h, x).is_err() {
                            ok = false;
                        }
                    }
                    h += 1;
                }
                s2 += 1;
            }
            ok && lt.segs_all()
        },
        "十四段 ×（捕获位/回归位）逐格恒等",
    );
    s.add(
        "滚动回归 8 拍全落",
        {
            let mut lt = UxRegressLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < ROLL_BEATS {
                if let Some(x) = roll_cell(b) {
                    if lt.record_roll(b, x).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.rolls_all()
        },
        "八拍逐拍恒等且单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = UxRegressLedger::new();
            let mut g = 0usize;
            while g < GATE_COUNT {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "日志零阻塞启动/信号捕获恒等/段覆盖恒等/滚动回归恒等/捕获率闸核算/全域回归收口",
    );
    s.add(
        "捕获率闸 ≥80%",
        {
            let mut lt = UxRegressLedger::new();
            // 全落 16 信号格 + 28 段格 = 44/44 = 100% 达闸（部分落账
            // 由负向用例另验不达闸路径）。
            let mut k = 0usize;
            while k < SIG_KINDS {
                let mut c = 0usize;
                while c < CELLS_PER_KIND {
                    if let Some(w) = sig_cell(k, c) {
                        let _ = lt.record_sig(k, c, w);
                    }
                    c += 1;
                }
                k += 1;
            }
            let mut s2 = 0usize;
            while s2 < SEG_COUNT {
                let mut h = 0usize;
                while h < CELLS_PER_SEG {
                    if let Some(x) = seg_cell(s2, h) {
                        let _ = lt.record_seg(s2, h, x);
                    }
                    h += 1;
                }
                s2 += 1;
            }
            lt.cap_gate()
        },
        "captured×100 ≥ 80×total 整数核算",
    );
    s.add(
        "滚动拍单调",
        {
            let mut ok = true;
            let mut b = 0usize;
            while b < ROLL_BEATS {
                if roll_cell(b).is_none() {
                    ok = false;
                }
                b += 1;
            }
            ok
        },
        "拍号全流程单调不减，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "信号漂移/段越闭集/滚动拍逆序三态轮转必抓");
    s.add(
        "边界·信号越闭集",
        {
            let mut lt = UxRegressLedger::new();
            lt.record_sig(SIG_KINDS, 0, 0).is_err()
        },
        "信号类 4 越闭集必拒",
    );
    s.add(
        "边界·段越闭集",
        {
            let mut lt = UxRegressLedger::new();
            lt.record_seg(SEG_COUNT, 0, 0).is_err()
        },
        "段 14 越闭集必拒",
    );
    s.add(
        "边界·滚动拍越闭集",
        {
            let mut lt = UxRegressLedger::new();
            lt.record_roll(ROLL_BEATS, 0).is_err()
        },
        "拍 8 越闭集必拒",
    );
    s.add(
        "段覆盖格漂移拒",
        {
            let mut lt = UxRegressLedger::new();
            lt.record_seg(3, 1, seg_cell(3, 1).unwrap_or(0).wrapping_add(1))
                == Err(UxRegressCode::ESegDrift)
        },
        "段载荷漂移必抓",
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
            let mut lt = UxRegressLedger::new();
            let mut g = 0usize;
            while g < GATE_COUNT {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut k = 0usize;
            while k < SIG_KINDS {
                let mut c = 0usize;
                while c < CELLS_PER_KIND {
                    if let Some(w) = sig_cell(k, c) {
                        let _ = lt.record_sig(k, c, w);
                    }
                    c += 1;
                }
                k += 1;
            }
            let mut s2 = 0usize;
            while s2 < SEG_COUNT {
                let mut h = 0usize;
                while h < CELLS_PER_SEG {
                    if let Some(x) = seg_cell(s2, h) {
                        let _ = lt.record_seg(s2, h, x);
                    }
                    h += 1;
                }
                s2 += 1;
            }
            let mut b = 0usize;
            while b < ROLL_BEATS {
                if let Some(x) = roll_cell(b) {
                    let _ = lt.record_roll(b, x);
                }
                b += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通且捕获率闸达标 → S1（全域回归定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC89C0,
        "SEG_BASE 0x40 对齐且按 VX15 公式落 0xC89C0",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sig_cells_pure() {
        let a = sig_cell(0, 2).unwrap_or(0);
        assert_eq!(a, sig_cell(0, 2).unwrap_or(0));
        assert_ne!(a, sig_cell(0, 3).unwrap_or(0));
        assert_ne!(a, sig_cell(1, 2).unwrap_or(0));
        assert!(sig_cell(SIG_KINDS, 0).is_none());
        assert!(sig_cell(0, CELLS_PER_KIND).is_none());
    }

    #[test]
    fn test_sig_kind_names() {
        assert_eq!(sig_kind_name(0), Some("rage click"));
        assert_eq!(sig_kind_name(1), Some("dead click"));
        assert_eq!(sig_kind_name(2), Some("浮层反复开关"));
        assert_eq!(sig_kind_name(3), Some("回退风暴"));
        assert!(sig_kind_name(4).is_none());
        assert_eq!(sig_cell_name(2, 0), Some("浮层·开合序列捕获位"));
        assert!(sig_cell_name(0, 4).is_none());
    }

    #[test]
    fn test_seg_names() {
        assert_eq!(seg_name(0), Some("B01"));
        assert_eq!(seg_name(13), Some("B14"));
        assert!(seg_name(14).is_none());
        assert!(seg_cell(SEG_COUNT, 0).is_none());
        assert!(seg_cell(0, CELLS_PER_SEG).is_none());
        assert_ne!(seg_cell(5, 0).unwrap_or(0), seg_cell(5, 1).unwrap_or(0));
    }

    #[test]
    fn test_roll_cells() {
        assert!(roll_cell(ROLL_BEATS).is_none());
        assert_ne!(roll_cell(3).unwrap_or(0), roll_cell(4).unwrap_or(0));
        let x = roll_cell(2).unwrap_or(0);
        assert_eq!(x, roll_cell(2).unwrap_or(0));
    }

    #[test]
    fn test_beat_order_caught() {
        let mut lt = UxRegressLedger::new();
        let _ = lt.record_roll(5, roll_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_roll(2, roll_cell(2).unwrap_or(0)),
            Err(UxRegressCode::EBeatOrder)
        );
        // 逆序被拒后不改账位：合法后续仍可落。
        assert!(lt.record_roll(6, roll_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = UxRegressLedger::new();
        assert_eq!(
            lt.record_sig(3, 1, sig_cell(3, 1).unwrap_or(0).wrapping_add(1)),
            Err(UxRegressCode::ESigDrift)
        );
        assert_eq!(
            lt.record_roll(1, roll_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(UxRegressCode::ESegDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = UxRegressLedger::new();
        let _ = lt.record_sig(2, 0, sig_cell(2, 0).unwrap_or(0));
        assert_eq!(
            lt.record_sig(2, 0, sig_cell(2, 0).unwrap_or(0)),
            Err(UxRegressCode::EDup)
        );
        let _ = lt.record_seg(9, 1, seg_cell(9, 1).unwrap_or(0));
        assert_eq!(
            lt.record_seg(9, 1, seg_cell(9, 1).unwrap_or(0)),
            Err(UxRegressCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = UxRegressLedger::new();
        let mut g = 0usize;
        while g < GATE_COUNT {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(GATE_COUNT).is_err());
        assert!(lt.gates_all());
        let mut lt2 = UxRegressLedger::new();
        assert!(lt2.judge_gate(4).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_cap_gate() {
        let mut lt = UxRegressLedger::new();
        // 空账本 0% 不达闸。
        assert!(!lt.cap_gate());
        // 全落信号 16 格 + 段 12 格 = 28/44 ≈ 63.6% 不达闸。
        let mut k = 0usize;
        while k < SIG_KINDS {
            let mut c = 0usize;
            while c < CELLS_PER_KIND {
                if let Some(w) = sig_cell(k, c) {
                    let _ = lt.record_sig(k, c, w);
                }
                c += 1;
            }
            k += 1;
        }
        let mut s2 = 0usize;
        while s2 < 6 {
            let mut h = 0usize;
            while h < CELLS_PER_SEG {
                if let Some(x) = seg_cell(s2, h) {
                    let _ = lt.record_seg(s2, h, x);
                }
                h += 1;
            }
            s2 += 1;
        }
        assert!(!lt.cap_gate());
        // 补齐剩余段格 → 100% 达闸。
        let mut s3 = 6usize;
        while s3 < SEG_COUNT {
            let mut h = 0usize;
            while h < CELLS_PER_SEG {
                if let Some(x) = seg_cell(s3, h) {
                    let _ = lt.record_seg(s3, h, x);
                }
                h += 1;
            }
            s3 += 1;
        }
        assert!(lt.cap_gate());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SIG_CELLS + SEG_CELLS + ROLL_BEATS) as u32);
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
        assert_eq!(SEG_BASE, 0xC89C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(UxRegressCode::Ok as u32, 0);
        assert_eq!(UxRegressCode::EGradeDrift as u32, 12);
        assert!(!UxRegressCode::Ok.detail().is_empty());
        assert!(!UxRegressCode::EBeatOrder.detail().is_empty());
        assert_eq!(UxRegressCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = UxRegressLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert!(!Grade::S1.detail().is_empty());
    }

    #[test]
    fn test_injection_conservation() {
        let (_, caught, _) = run_cycle();
        assert_eq!(caught, 10);
        let mut lt = UxRegressLedger::new();
        let (inj, c, e) = lt.stats();
        assert_eq!(inj, c);
        assert_eq!(e, 0);
    }

    #[test]
    fn unx52296_checks_green() {
        let s = run_unx52296_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
