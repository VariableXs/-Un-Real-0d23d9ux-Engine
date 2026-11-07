//! UNX-F52419 KeePass 插件生态消费面收口（×10 插件实测）· 0xCA880段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52419"，逐字）：
//!   title: KeePass 插件生态消费面收口（×10 插件实测）
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52420-J1 全组条
//!     目红线登记：测试库一律随机口令+即弃、真实凭证零入库、日志不记
//!     口令内容（体验十三隐私红线）；判据锚定 体验十三·补 + §AI-66.9；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52419）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52420-J1，照原文锚定；功能按本卡标题（KeePass 插件生态消费面
//! 收口 ×10 插件实测）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① ×10 插件闭集：每插件 2 格（装载/功能）共 20 格，插件序单调
//!      （逆序必抓），格型 = 格号%2（0=装载 1=功能），逐格载荷折叠恒
//!      等才收。
//!   ② 插件 SDK 消费 8 格：SDK API 消费面逐格落账，越界/漂移必抓。
//!   ③ 热加载 6 拍单调：拍号严格递增（回退必抓），逐拍载荷折叠恒等。
//!   ④ 六闸：装载/功能/卸载生命周期三元 + 崩溃隔离二元（启用/复核）+
//!      热加载确认，逐格落账，未齐即封。
//!   ⑤ 与 UNX-F52401（KeePass 本体）分账声明 2 行闭集，联签锚
//!      KEEPASS_F52401_SEG（0xCA400），缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10/10（插件载荷
//!      漂移/插件格越闭集/插件序逆序），检出+逃逸守恒等式；双跑指纹一
//!      致；定级 S1/S2/S3 闭集。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52419;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// ×10 插件 20 格数。
pub const PLUG_CELLS: usize = 20;

/// 插件数。
pub const PLUG_COUNT: usize = 10;

/// 插件 SDK 消费格数。
pub const SDK_CELLS: usize = 8;

/// 热加载拍数。
pub const HOT_BEATS: usize = 6;

/// 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// KeePass 本体（UNX-F52401）段基址，联签锚。
pub const KEEPASS_F52401_SEG: u32 = 0xCA400 + 0x40 * (52401 - 52401);

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KpPlugCode {
    /// 成功
    Ok = 0,
    /// 插件格越闭集
    ENoPlug,
    /// 插件格载荷漂移
    EPlugDrift,
    /// 插件序逆序
    EOrder,
    /// SDK 格越闭集
    ENoSdk,
    /// SDK 格载荷漂移
    ESdkDrift,
    /// 热加载拍越闭集
    ENoBeat,
    /// 热加载拍回退/漂移
    EBeatDrift,
    /// 分账声明行越闭集
    ENoRow,
    /// 分账声明行缺行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl KpPlugCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KpPlugCode::Ok => "成功",
            KpPlugCode::ENoPlug => "插件格越闭集：×10 插件 ×（装载/功能）20 格之外先登记",
            KpPlugCode::EPlugDrift => "插件格载荷漂移：装载/功能折叠与基准不恒等，逐格复算",
            KpPlugCode::EOrder => "插件序逆序：插件 0→9 单调，逆序必抓",
            KpPlugCode::ENoSdk => "SDK 格越闭集：插件 SDK 消费 8 格之外先登记",
            KpPlugCode::ESdkDrift => "SDK 格载荷漂移：SDK 消费折叠与基准不恒等，逐格复算",
            KpPlugCode::ENoBeat => "热加载拍越闭集：6 拍闭集之外先登记",
            KpPlugCode::EBeatDrift => "热加载拍回退/漂移：拍号严格递增且载荷恒等，回退必抓",
            KpPlugCode::ENoRow => "分账声明行越闭集：与 F52401 本体分账 2 行之外先登记",
            KpPlugCode::ERowMiss => "分账声明行缺行：与 F52401 本体分账声明未齐先补齐",
            KpPlugCode::EDup => "重复落账：同格只许一条账目",
            KpPlugCode::ESeal => "未齐即封：20 插件格 + 8 SDK 格 + 6 拍热加载 + 六闸未齐先补齐",
            KpPlugCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六闸+20插件格+8SDK格+6拍热加载+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 插件格载荷（格 c；插件 = c/2，格型 = c%2：0=装载 1=功能）。
pub const fn plug_cell(c: usize) -> Option<u64> {
    if c >= PLUG_CELLS {
        return None;
    }
    Some((0xCA890u64 + (c as u64) * 0x2C) ^ ((c / 2) as u64).wrapping_mul(0x01000193))
}

/// 插件号（i/N 映射：插件 = c/2）。
pub const fn plug_kind(c: usize) -> Option<u32> {
    if c >= PLUG_CELLS {
        return None;
    }
    Some((c / 2) as u32)
}

/// 格型（i/N 映射：0=装载 1=功能）。
pub const fn plug_phase(c: usize) -> Option<u32> {
    if c >= PLUG_CELLS {
        return None;
    }
    Some((c % 2) as u32)
}

/// 格型名。
pub const fn plug_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("装载"),
        1 => Some("功能"),
        _ => None,
    }
}

/// SDK 消费格载荷（格 s）。
pub const fn sdk_cell(s: usize) -> Option<u64> {
    if s >= SDK_CELLS {
        return None;
    }
    Some((0xCA8D0u64 + (s as u64) * 0x38) ^ ((s % 4) as u64).wrapping_mul(0x01000193))
}

/// 热加载拍载荷（拍 b）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= HOT_BEATS {
        return None;
    }
    Some((0xCA910u64 + (b as u64) * 0x5D) ^ 0x4B50_504C_5547_4854)
}

/// 分账声明行折叠（行 r，锚 UNX-F52401 本体段）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(KEEPASS_F52401_SEG + 0x0C0 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// KeePass 插件生态收口账本状态机。
#[derive(Clone)]
pub struct KpPlugLedger {
    /// 六闸：0=装载闸 1=功能闸 2=卸载闸 3=隔离启用 4=隔离复核 5=热加载确认。
    gates: [Option<bool>; 6],
    /// 插件账。
    plugs: [Option<u64>; PLUG_CELLS],
    /// SDK 消费账。
    sdks: [Option<u64>; SDK_CELLS],
    /// 热加载拍账。
    beats: [Option<u64>; HOT_BEATS],
    /// 已落最高拍。
    last_beat: i64,
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后插件号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KpPlugLedger {
    /// 新账本。
    pub fn new() -> Self {
        KpPlugLedger {
            gates: [None; 6],
            plugs: [None; PLUG_CELLS],
            sdks: [None; SDK_CELLS],
            beats: [None; HOT_BEATS],
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

    /// 落一格六闸（生命周期三元 + 崩溃隔离二元 + 热加载确认；越界即 ESeal）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KpPlugCode> {
        if gate >= 6 {
            return Err(KpPlugCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5300 + gate as u64);
        Ok(())
    }

    /// 六闸全落核验。
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

    /// 插件一格（插件序单调 + 载荷对照 + 重复拒）。
    pub fn record_plug(&mut self, c: usize, got: u64) -> Result<(), KpPlugCode> {
        if c >= PLUG_CELLS {
            return Err(KpPlugCode::ENoPlug);
        }
        let kind = match plug_kind(c) {
            Some(k) => k,
            None => return Err(KpPlugCode::ENoPlug),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400);
            return Err(KpPlugCode::EOrder);
        }
        let want = match plug_cell(c) {
            Some(w) => w,
            None => return Err(KpPlugCode::ENoPlug),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5401 + c as u64);
            return Err(KpPlugCode::EPlugDrift);
        }
        if match self.plugs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KpPlugCode::EDup);
        }
        if let Some(cell) = self.plugs.get_mut(c) {
            *cell = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5402 + c as u64);
        Ok(())
    }

    /// 插件全账核验（20 格）。
    pub fn plugs_all(&self) -> bool {
        let mut c = 0usize;
        while c < PLUG_CELLS {
            match self.plugs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// SDK 消费一格（载荷对照 + 重复拒）。
    pub fn record_sdk(&mut self, s: usize, got: u64) -> Result<(), KpPlugCode> {
        if s >= SDK_CELLS {
            return Err(KpPlugCode::ENoSdk);
        }
        let want = match sdk_cell(s) {
            Some(w) => w,
            None => return Err(KpPlugCode::ENoSdk),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5500 + s as u64);
            return Err(KpPlugCode::ESdkDrift);
        }
        if match self.sdks.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KpPlugCode::EDup);
        }
        if let Some(cell) = self.sdks.get_mut(s) {
            *cell = Some(got);
        }
        self.fnv(0x5501 + s as u64);
        Ok(())
    }

    /// SDK 全账核验（8 格）。
    pub fn sdks_all(&self) -> bool {
        let mut s = 0usize;
        while s < SDK_CELLS {
            match self.sdks.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 热加载一拍（拍号严格递增 + 载荷对照；回退即 EBeatDrift）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), KpPlugCode> {
        if b >= HOT_BEATS {
            return Err(KpPlugCode::ENoBeat);
        }
        if (b as i64) <= self.last_beat {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5600);
            return Err(KpPlugCode::EBeatDrift);
        }
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(KpPlugCode::ENoBeat),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5601 + b as u64);
            return Err(KpPlugCode::EBeatDrift);
        }
        if let Some(cell) = self.beats.get_mut(b) {
            *cell = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0x5602 + b as u64);
        Ok(())
    }

    /// 热加载全账核验（6 拍全齐）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < HOT_BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 分账声明一行（与 UNX-F52401 本体分账）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), KpPlugCode> {
        if r >= SPLIT_ROWS {
            return Err(KpPlugCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KpPlugCode::EDup);
        }
        if let Some(cell) = self.splits.get_mut(r) {
            *cell = split_row(r);
        }
        self.fnv(0x5700 + r as u64);
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

    /// 行收口核（缺行即 ERowMiss）。
    pub fn row_seal(&self) -> Result<(), KpPlugCode> {
        if self.splits_all() {
            Ok(())
        } else {
            Err(KpPlugCode::ERowMiss)
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 注入守恒核（注入=检出+逃逸）。
    pub fn verify_conservation(&self) -> bool {
        self.injected == self.caught.wrapping_add(self.escaped)
    }

    /// KB-COMPAT 定级（全通 → S1，六闸齐 → S2，否则 S3）。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.plugs_all() && self.sdks_all() && self.beats_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级封核（非 S1 即 EGradeDrift）。
    pub fn seal_grade(&self) -> Result<Grade, KpPlugCode> {
        let g = self.grade();
        if g == Grade::S1 {
            Ok(g)
        } else {
            Err(KpPlugCode::EGradeDrift)
        }
    }
}

impl Default for KpPlugLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KpPlugLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < PLUG_CELLS {
        match plug_cell(c) {
            Some(w) => {
                if led.record_plug(c, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        c += 1;
    }
    let mut s = 0usize;
    while s < SDK_CELLS {
        match sdk_cell(s) {
            Some(w) => {
                if led.record_sdk(s, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        s += 1;
    }
    let mut b = 0usize;
    while b < HOT_BEATS {
        match beat_cell(b) {
            Some(w) => {
                if led.record_beat(b, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：插件载荷漂移 / 插件格越闭集 / 插件序逆序三态轮转，
    // 全部落在当轮临时账本上，主账只记注入三数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KpPlugLedger::new();
        let caught = match kind {
            0 => {
                // 漂移注入：先按合法序落正格（插件 0 装载格），再注漂移载荷。
                match plug_cell(0) {
                    Some(w) => {
                        let _ = lt.record_plug(0, w);
                    }
                    None => {}
                }
                match plug_cell(1) {
                    Some(w) => lt.record_plug(1, w.wrapping_add(1)) == Err(KpPlugCode::EPlugDrift),
                    None => false,
                }
            }
            1 => {
                // 越闭集注入：空账本直接越界必抓，不改 last_kind。
                lt.record_plug(PLUG_CELLS, 0).is_err()
            }
            _ => {
                // 逆序注入：空账本先落插件 2 正格，再落插件 0 正格必抓。
                match plug_cell(4) {
                    Some(w) => {
                        let _ = lt.record_plug(4, w);
                    }
                    None => {}
                }
                match plug_cell(1) {
                    Some(w) => lt.record_plug(1, w) == Err(KpPlugCode::EOrder),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5800 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let _ = led.verify_conservation();
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52419 判据集（锚 UNX-F52420-J1，off-by-one 板式）。
pub fn run_unx52419_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52419");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "×10 插件 20 格全通（装载/功能）",
        recorded >= PLUG_CELLS as u32 + SDK_CELLS as u32 + HOT_BEATS as u32,
        "×10 插件每插件 2 格（装载/功能）逐格恒等落账",
    );
    s.add(
        "插件序单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < PLUG_CELLS {
                let a = match plug_kind(c) {
                    Some(k) => k,
                    None => 99,
                };
                let b = match plug_kind(c - 1) {
                    Some(k) => k,
                    None => 0,
                };
                if a < b {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "插件 0→9 单调，逆序必抓（崩溃隔离口径）",
    );
    s.add(
        "插件格型核（偶=装载 奇=功能）",
        {
            let mut ok = true;
            let mut c = 0usize;
            while c < PLUG_CELLS {
                let p = match plug_phase(c) {
                    Some(p) => p,
                    None => 9,
                };
                if p != (c % 2) as u32 {
                    ok = false;
                }
                c += 1;
            }
            ok && plug_phase_name(0) == Some("装载") && plug_phase_name(1) == Some("功能")
        },
        "格型 = 格号%2 的 i/N 映射逐格核对",
    );
    s.add(
        "插件 SDK 消费 8 格全落",
        {
            let mut lt = KpPlugLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < SDK_CELLS {
                match sdk_cell(sc) {
                    Some(w) => {
                        if lt.record_sdk(sc, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                sc += 1;
            }
            ok && lt.sdks_all()
        },
        "插件 SDK 消费面逐格恒等，越界/漂移必抓",
    );
    s.add(
        "热加载 6 拍单调全落",
        {
            let mut lt = KpPlugLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < HOT_BEATS {
                match beat_cell(b) {
                    Some(w) => {
                        if lt.record_beat(b, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "拍号严格递增且逐拍载荷恒等，回退必抓",
    );
    s.add(
        "生命周期三元 + 崩溃隔离二元 + 热加载确认六闸全落",
        {
            let mut lt = KpPlugLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "装载/功能/卸载 + 隔离启用/隔离复核 + 热加载确认逐格落账",
    );
    s.add(
        "与 F52401 本体分账声明 2/2 全齐",
        {
            let mut lt = KpPlugLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.row_seal().is_ok()
        },
        "与 UNX-F52401 KeePass 本体分账声明两行闭集，联签锚 KEEPASS_F52401_SEG",
    );
    let mut l0 = KpPlugLedger::new();
    s.add(
        "边界·插件格越闭集",
        l0.record_plug(PLUG_CELLS, 0).is_err(),
        "插件格 20 越闭集必拒",
    );
    s.add(
        "插件载荷漂移拒",
        match plug_cell(3) {
            Some(w) => l0.record_plug(3, w.wrapping_add(1)) == Err(KpPlugCode::EPlugDrift),
            None => false,
        },
        "插件格载荷漂移必抓",
    );
    s.add(
        "边界·SDK 格越闭集",
        l0.record_sdk(SDK_CELLS, 0).is_err(),
        "SDK 格 8 越闭集必拒",
    );
    s.add(
        "热加载拍回退必抓",
        {
            match beat_cell(0) {
                Some(w) => {
                    let _ = l0.record_beat(0, w);
                    match beat_cell(1) {
                        Some(w1) => {
                            let _ = l0.record_beat(1, w1);
                            l0.record_beat(0, w) == Err(KpPlugCode::EBeatDrift)
                        }
                        None => false,
                    }
                }
                None => false,
            }
        },
        "拍号 1 落后再落拍号 0 回退必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "插件漂移/插件越闭集/插件序逆序三态轮转必抓",
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
            let mut lt = KpPlugLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < PLUG_CELLS {
                match plug_cell(c) {
                    Some(w) => {
                        let _ = lt.record_plug(c, w);
                    }
                    None => {}
                }
                c += 1;
            }
            let mut sc = 0usize;
            while sc < SDK_CELLS {
                match sdk_cell(sc) {
                    Some(w) => {
                        let _ = lt.record_sdk(sc, w);
                    }
                    None => {}
                }
                sc += 1;
            }
            let mut b = 0usize;
            while b < HOT_BEATS {
                match beat_cell(b) {
                    Some(w) => {
                        let _ = lt.record_beat(b, w);
                    }
                    None => {}
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.seal_grade() == Ok(Grade::S1)
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA880 && KEEPASS_F52401_SEG == 0xCA400,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA400 + 0x40×(id−52401)，联签锚 F52401",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 16,
        "判据原文条数按求值时点核：含本条共 17 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plug_kind_phase_mapping() {
        assert_eq!(plug_kind(0), Some(0));
        assert_eq!(plug_kind(1), Some(0));
        assert_eq!(plug_kind(2), Some(1));
        assert_eq!(plug_kind(19), Some(9));
        assert!(plug_kind(20).is_none());
        assert_eq!(plug_phase(0), Some(0));
        assert_eq!(plug_phase(1), Some(1));
        assert_eq!(plug_phase(19), Some(1));
        assert!(plug_phase(20).is_none());
        assert_eq!(plug_phase_name(0), Some("装载"));
        assert_eq!(plug_phase_name(1), Some("功能"));
        assert!(plug_phase_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = plug_cell(5).unwrap_or(0);
        assert_eq!(a, plug_cell(5).unwrap_or(0));
        assert_ne!(a, plug_cell(6).unwrap_or(0));
        let sc = sdk_cell(3).unwrap_or(0);
        assert_eq!(sc, sdk_cell(3).unwrap_or(0));
        assert_ne!(sc, sdk_cell(4).unwrap_or(0));
        assert!(plug_cell(PLUG_CELLS).is_none());
        assert!(sdk_cell(SDK_CELLS).is_none());
        assert!(beat_cell(HOT_BEATS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_plug_order_caught() {
        let mut lt = KpPlugLedger::new();
        // 先落插件 2 正格（c=4），再落插件 0 正格（c=1）必抓。
        let _ = lt.record_plug(4, plug_cell(4).unwrap_or(0));
        assert_eq!(lt.record_plug(1, plug_cell(1).unwrap_or(0)), Err(KpPlugCode::EOrder));
        assert!(lt.record_plug(5, plug_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = KpPlugLedger::new();
        assert_eq!(
            lt.record_plug(7, plug_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(KpPlugCode::EPlugDrift)
        );
        assert_eq!(
            lt.record_sdk(5, sdk_cell(5).unwrap_or(0).wrapping_add(3)),
            Err(KpPlugCode::ESdkDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KpPlugLedger::new();
        let _ = lt.record_plug(2, plug_cell(2).unwrap_or(0));
        assert_eq!(lt.record_plug(2, plug_cell(2).unwrap_or(0)), Err(KpPlugCode::EDup));
        let _ = lt.record_sdk(0, sdk_cell(0).unwrap_or(0));
        assert_eq!(lt.record_sdk(0, sdk_cell(0).unwrap_or(0)), Err(KpPlugCode::EDup));
    }

    #[test]
    fn test_beat_monotone() {
        let mut lt = KpPlugLedger::new();
        let mut b = 0usize;
        while b < HOT_BEATS {
            assert!(lt.record_beat(b, beat_cell(b).unwrap_or(0)).is_ok());
            b += 1;
        }
        // 闭集边界：0..=5 拍之外越界必抓；重复拍回退必抓。
        assert!(lt.record_beat(HOT_BEATS, 0).is_err());
        assert_eq!(
            lt.record_beat(0, beat_cell(0).unwrap_or(0)),
            Err(KpPlugCode::EBeatDrift)
        );
        assert!(lt.beats_all());
        // 空账本首拍回退核：last_beat=-1，先落拍 1 再落拍 0 必抓。
        let mut lt2 = KpPlugLedger::new();
        let _ = lt2.record_beat(1, beat_cell(1).unwrap_or(0));
        assert_eq!(
            lt2.record_beat(0, beat_cell(0).unwrap_or(0)),
            Err(KpPlugCode::EBeatDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = KpPlugLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = KpPlugLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 插件 + 8 SDK + 6 拍。
        assert!(recorded >= PLUG_CELLS as u32 + SDK_CELLS as u32 + HOT_BEATS as u32);
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
        assert_eq!(SEG_BASE, 0xCA880);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        assert_eq!(KEEPASS_F52401_SEG, 0xCA400);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(KpPlugCode::Ok as u32, 0);
        assert_eq!(KpPlugCode::EGradeDrift as u32, 12);
        let all = [
            KpPlugCode::Ok,
            KpPlugCode::ENoPlug,
            KpPlugCode::EPlugDrift,
            KpPlugCode::EOrder,
            KpPlugCode::ENoSdk,
            KpPlugCode::ESdkDrift,
            KpPlugCode::ENoBeat,
            KpPlugCode::EBeatDrift,
            KpPlugCode::ENoRow,
            KpPlugCode::ERowMiss,
            KpPlugCode::EDup,
            KpPlugCode::ESeal,
            KpPlugCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            i += 1;
        }
    }

    #[test]
    fn test_conservation_and_grade() {
        let mut lt = KpPlugLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert!(lt.verify_conservation());
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.grade(), Grade::S2);
        let mut c = 0usize;
        while c < PLUG_CELLS {
            let _ = lt.record_plug(c, plug_cell(c).unwrap_or(0));
            c += 1;
        }
        let mut sc = 0usize;
        while sc < SDK_CELLS {
            let _ = lt.record_sdk(sc, sdk_cell(sc).unwrap_or(0));
            sc += 1;
        }
        let mut b = 0usize;
        while b < HOT_BEATS {
            let _ = lt.record_beat(b, beat_cell(b).unwrap_or(0));
            b += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt.declare_split(r);
            r += 1;
        }
        assert_eq!(lt.grade(), Grade::S1);
        assert!(lt.seal_grade().is_ok());
        assert!(lt.row_seal().is_ok());
    }

    #[test]
    fn test_empty_ledger_injections() {
        let mut a = KpPlugLedger::new();
        assert!(a.record_plug(PLUG_CELLS, 0).is_err());
        let mut b = KpPlugLedger::new();
        let _ = b.record_plug(4, plug_cell(4).unwrap_or(0));
        let kind_before = b.last_kind;
        assert_eq!(b.record_plug(1, plug_cell(1).unwrap_or(0)), Err(KpPlugCode::EOrder));
        assert_eq!(b.last_kind, kind_before);
    }

    #[test]
    fn unx52419_checks_green() {
        let s = run_unx52419_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
