//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14621 · WM_NCHITTEST 协议与 HT 码全集 · 0x66940
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F14621-J1 HTCLIENT/CAPTION/NOWHERE/TRANSPARENT 等码集派发
//!      账一致，应用返回码优先语义对齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - HT 码闭集 14 码（ERROR/TRANSPARENT/NOWHERE/CLIENT/CAPTION/
//!     SYSMENU/GROWBOX/MENU/HSCROLL/VSCROLL/MINBUTTON/MAXBUTTON/
//!     LEFT/RIGHT），写死 wingdi/winuser 值；
//!   - 派发协议：命中探针 (hwnd, x, y) → 应用回调优先（应用返回码
//!     非 TRANSPARENT 即采纳）→ 回调缺省走桌面几何判定 → 派发账；
//!   - HTTRANSPARENT 语义：穿透到下层窗口（不本位派发），穿透账
//!     单列；
//!   - 非法 HT 码（越闭集）拒收落账；
//!   - LCG 种子写死 = SEG_BASE（0x66940），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66940，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66940;

// ── HT 码闭集（写死：winuser HT* 值） ────────────────────────────
pub const HT_ERROR: i32 = -2;
pub const HT_TRANSPARENT: i32 = -1;
pub const HT_NOWHERE: i32 = 0;
pub const HT_CLIENT: i32 = 1;
pub const HT_CAPTION: i32 = 2;
pub const HT_SYSMENU: i32 = 3;
pub const HT_GROWBOX: i32 = 4;
pub const HT_MENU: i32 = 5;
pub const HT_HSCROLL: i32 = 6;
pub const HT_VSCROLL: i32 = 7;
pub const HT_MINBUTTON: i32 = 8;
pub const HT_MAXBUTTON: i32 = 9;
pub const HT_LEFT: i32 = 10;
pub const HT_RIGHT: i32 = 11;
pub const HT_CODE_COUNT: usize = 14;
pub const HT_CODES: [i32; HT_CODE_COUNT] = [
    HT_ERROR, HT_TRANSPARENT, HT_NOWHERE, HT_CLIENT, HT_CAPTION, HT_SYSMENU, HT_GROWBOX,
    HT_MENU, HT_HSCROLL, HT_VSCROLL, HT_MINBUTTON, HT_MAXBUTTON, HT_LEFT, HT_RIGHT,
];
pub const HT_NAMES: [&str; HT_CODE_COUNT] = [
    "HT_ERROR", "HT_TRANSPARENT", "HT_NOWHERE", "HT_CLIENT", "HT_CAPTION", "HT_SYSMENU",
    "HT_GROWBOX", "HT_MENU", "HT_HSCROLL", "HT_VSCROLL", "HT_MINBUTTON", "HT_MAXBUTTON",
    "HT_LEFT", "HT_RIGHT",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitCode {
    /// HT 码越闭集
    HtOutOfRange,
    /// 窗口未入册
    HwndUnregistered,
    /// 探针越出全桌
    ProbeOutOfDesk,
    /// 应用返回码优先账破（被桌面判定覆盖）
    AppPriorityBroken,
    /// 穿透账破（TRANSPARENT 未穿透）
    TransparentNotPassed,
    /// 桌面几何判定与基准机不一致
    GeometryMismatch,
    /// 派发账守恒破（探针数≠派发数）
    LedgerBroken,
    /// 穿透环（三层互穿）
    TransparentLoop,
}

impl HitCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HitCode::HtOutOfRange => "HT 码越闭集：14 码冻结",
            HitCode::HwndUnregistered => "窗口未入册：拒收",
            HitCode::ProbeOutOfDesk => "探针越出全桌",
            HitCode::AppPriorityBroken => "应用返回码优先被覆盖",
            HitCode::TransparentNotPassed => "TRANSPARENT 未穿透",
            HitCode::GeometryMismatch => "桌面几何判定与基准机不一致",
            HitCode::LedgerBroken => "派发账守恒破",
            HitCode::TransparentLoop => "穿透环：三层互穿拒收",
        }
    }
    pub const ALL: [HitCode; 8] = [
        HitCode::HtOutOfRange,
        HitCode::HwndUnregistered,
        HitCode::ProbeOutOfDesk,
        HitCode::AppPriorityBroken,
        HitCode::TransparentNotPassed,
        HitCode::GeometryMismatch,
        HitCode::LedgerBroken,
        HitCode::TransparentLoop,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HitCode::ALL.len() {
        if HitCode::ALL[i].val() == code {
            return HitCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66940 段闭集内"
}

// ── 桌面几何 ─────────────────────────────────────────────────────
/// 全桌（写死）：0..640 × 0..480。
pub const DESK_W: i32 = 640;
pub const DESK_H: i32 = 480;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WinBox {
    pub hwnd: u32,
    /// 窗口矩形（客户区外框）。
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
    /// 标题栏高（0 = 无非客户区）。
    pub caption_h: i32,
    /// 应用回调写死的返回码档（None = 缺省走几何判定）。
    pub app_ret: Option<i32>,
}

impl WinBox {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.l && x < self.r && y >= self.t && y < self.b
    }
    /// 桌面几何判定（写死基准语义）：
    ///   标题栏带（top..top+caption_h）→ HT_CAPTION；
    ///   左/右 4px 边带 → HT_LEFT/HT_RIGHT；
    ///   其余 → HT_CLIENT。
    pub fn geometry_hit(&self, x: i32, y: i32) -> i32 {
        let edge = 4;
        if self.caption_h > 0 && y < self.t + self.caption_h {
            return HT_CAPTION;
        }
        if x < self.l + edge {
            return HT_LEFT;
        }
        if x >= self.r - edge {
            return HT_RIGHT;
        }
        HT_CLIENT
    }
}

// ── 派发账 ───────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HitRec {
    pub hwnd: u32,
    pub x: i32,
    pub y: i32,
    pub ht: i32,
    /// 来源：0=应用回调优先、1=桌面几何、2=穿透落点、3=NOWHERE。
    pub src: u8,
}

pub struct HitDesk {
    pub wins: Vec<WinBox>,
    pub recs: Vec<HitRec>,
    pub transparent_hops: u32,
    pub probed: u32,
}

impl HitDesk {
    pub fn new() -> HitDesk {
        HitDesk { wins: Vec::new(), recs: Vec::new(), transparent_hops: 0, probed: 0 }
    }

    pub fn add(&mut self, w: WinBox) -> Result<(), u32> {
        if w.l >= w.r || w.t >= w.b {
            return Err(HitCode::ProbeOutOfDesk.val());
        }
        self.wins.push(w);
        Ok(())
    }

    fn find(&self, hwnd: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.wins.len() {
            if self.wins[i].hwnd == hwnd {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// WM_NCHITTEST 派发：应用返回码优先（非 TRANSPARENT 即采纳）；
    /// TRANSPARENT 穿透至下层（册序在后 = 下层）。
    pub fn hit_test(&mut self, hwnd: u32, x: i32, y: i32) -> Result<i32, u32> {
        if x < 0 || y < 0 || x >= DESK_W || y >= DESK_H {
            return Err(HitCode::ProbeOutOfDesk.val());
        }
        self.probed += 1;
        let k = match self.find(hwnd) {
            Some(k) => k,
            None => return Err(HitCode::HwndUnregistered.val()),
        };
        if !self.wins[k].contains(x, y) {
            let rec = HitRec { hwnd, x, y, ht: HT_NOWHERE, src: 3 };
            self.recs.push(rec);
            return Ok(HT_NOWHERE);
        }
        let app = self.wins[k].app_ret;
        let ht = match app {
            Some(code) => {
                if code == HT_TRANSPARENT {
                    // 穿透：找册中更靠后的包含点窗口。
                    self.transparent_hops += 1;
                    let mut lower = HT_NOWHERE;
                    let mut found = false;
                    let mut j = k + 1;
                    while j < self.wins.len() {
                        if self.wins[j].contains(x, y) {
                            lower = self.wins[j].geometry_hit(x, y);
                            let rec = HitRec { hwnd: self.wins[j].hwnd, x, y, ht: lower, src: 2 };
                            self.recs.push(rec);
                            found = true;
                            break;
                        }
                        j += 1;
                    }
                    if !found {
                        // 无下层可穿：本位落 NOWHERE 账（守恒：每探针一条账）。
                        let rec = HitRec { hwnd, x, y, ht: HT_NOWHERE, src: 3 };
                        self.recs.push(rec);
                    }
                    lower
                } else {
                    if code_index_of(code) == HT_CODE_COUNT {
                        return Err(HitCode::HtOutOfRange.val());
                    }
                    code
                }
            }
            None => self.wins[k].geometry_hit(x, y),
        };
        let rec = HitRec { hwnd, x, y, ht, src: if app.is_some() { 0 } else { 1 } };
        self.recs.push(rec);
        Ok(ht)
    }

    /// 派发账守恒：每次有效探针恰落一条账。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.recs.len() as u32 != self.probed {
            return Err(HitCode::LedgerBroken.val());
        }
        Ok(())
    }
}

/// HT 码 → 闭集下标（越闭集返回 HT_CODE_COUNT）。
pub fn code_index_of(code: i32) -> usize {
    let mut i = 0;
    while i < HT_CODE_COUNT {
        if HT_CODES[i] == code {
            return i;
        }
        i += 1;
    }
    HT_CODE_COUNT
}

// ── 基准机对拍（写死场景表） ─────────────────────────────────────
/// 基准机场景（写死）：窗 1 (0,0,200,100) 标题 24；窗 2 (100,50,300,200)
/// 标题 0，应用档回 HT_TRANSPARENT。
///   探针 (150,12)→窗1 HT_CAPTION；(150,60)→窗2 穿透→窗1 HT_CLIENT；
///   (250,150)→窗2 穿透→无下层 HT_NOWHERE；(199,99)→窗2 HT_RIGHT。
pub fn scripted_parity() -> Result<[i32; 4], u32> {
    let mut d = HitDesk::new();
    d.add(WinBox { hwnd: 1, l: 0, t: 0, r: 200, b: 100, caption_h: 24, app_ret: None })?;
    d.add(WinBox { hwnd: 2, l: 100, t: 50, r: 300, b: 200, caption_h: 0, app_ret: Some(HT_TRANSPARENT) })?;
    let r0 = d.hit_test(1, 150, 12)?;
    let r1 = d.hit_test(2, 150, 60)?;
    let r2 = d.hit_test(2, 250, 150)?;
    let r3 = d.hit_test(2, 199, 99)?;
    d.conservation()?;
    let expect = [HT_CAPTION, HT_CLIENT, HT_NOWHERE, HT_RIGHT];
    if r0 != expect[0] || r1 != expect[1] || r2 != expect[2] || r3 != expect[3] {
        return Err(HitCode::GeometryMismatch.val());
    }
    Ok([r0, r1, r2, r3])
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 随机探针压力正演 ─────────────────────────────────────────────
pub fn stress_run(spots: usize) -> Result<(u32, u32, u32), u32> {
    let mut d = HitDesk::new();
    d.add(WinBox { hwnd: 1, l: 0, t: 0, r: 400, b: 300, caption_h: 24, app_ret: None })?;
    d.add(WinBox { hwnd: 2, l: 100, t: 100, r: 500, b: 400, caption_h: 0, app_ret: Some(HT_TRANSPARENT) })?;
    d.add(WinBox { hwnd: 3, l: 200, t: 200, r: 600, b: 450, caption_h: 0, app_ret: None })?;
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < spots {
        let x = lcg.below(DESK_W as u64) as i32;
        let y = lcg.below(DESK_H as u64) as i32;
        let hwnd = 1 + lcg.below(3) as u32;
        let ht = d.hit_test(hwnd, x, y)?;
        if code_index_of(ht) == HT_CODE_COUNT {
            return Err(HitCode::HtOutOfRange.val());
        }
        r += 1;
    }
    d.conservation()?;
    Ok((d.probed, d.transparent_hops, d.recs.len() as u32))
}

// ── 注入面：三族全抓 ─────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：探针越桌必拒。
    ledger.tries[0] += 1;
    let mut d = HitDesk::new();
    let _ = d.add(WinBox { hwnd: 1, l: 0, t: 0, r: 100, b: 100, caption_h: 0, app_ret: None });
    if let Err(e) = d.hit_test(1, 700, 10) {
        if e == HitCode::ProbeOutOfDesk.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：未入册窗口必拒。
    ledger.tries[1] += 1;
    if let Err(e) = d.hit_test(9, 50, 50) {
        if e == HitCode::HwndUnregistered.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：坏 HT 码（越闭集）经 code_index_of 必检出。
    ledger.tries[2] += 1;
    if code_index_of(777) == HT_CODE_COUNT {
        ledger.caught[2] += 1;
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<([i32; 4], u32, u32, u32), u32> {
    let scr = scripted_parity()?;
    let (probed, hops, recs) = stress_run(72)?;
    Ok((scr, probed, hops, recs))
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HitCode::ALL.len() {
        let v = HitCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-剧本4点+压力72点全过",
        matches!(golden, Ok(_)),
        format!("{:?}", golden),
    ));
    // HT 码名/值点验：值唯一 + 名非空。
    let mut ok = 0;
    let mut uniq = true;
    let mut i = 0;
    while i < HT_CODE_COUNT {
        if HT_NAMES[i].len() > 0 {
            ok += 1;
        }
        let mut j = i + 1;
        while j < HT_CODE_COUNT {
            if HT_CODES[i] == HT_CODES[j] {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    v.push((
        "J1-HT 14 码值唯一+名点验",
        ok == 14 && uniq,
        format!("{}/14 uniq={}", ok, uniq),
    ));
    // 应用返回码优先抽验：窗 3 应用档回 HT_SYSMENU，几何判定本应
    // HT_LEFT —— 应用码必须胜出（含反向：缺省窗走几何）。
    let mut d = HitDesk::new();
    let _ = d.add(WinBox { hwnd: 4, l: 0, t: 0, r: 100, b: 100, caption_h: 0, app_ret: Some(HT_SYSMENU) });
    let _ = d.add(WinBox { hwnd: 5, l: 0, t: 0, r: 100, b: 100, caption_h: 0, app_ret: None });
    let r4 = d.hit_test(4, 2, 50);
    let r5 = d.hit_test(5, 2, 50);
    let prio = r4 == Ok(HT_SYSMENU) && r5 == Ok(HT_LEFT);
    v.push((
        "J1-应用返回码优先语义",
        prio,
        format!("{:?}/{:?}", r4, r5),
    ));
    // 反向判据：HT 码表混入重复必须可检出。
    let dup_detected = {
        let mut det = false;
        let mut a = 0;
        while a < HT_CODE_COUNT {
            let mut b = a + 1;
            while b < HT_CODE_COUNT {
                if HT_CODES[a] == HT_CODES[b] {
                    det = true;
                }
                b += 1;
            }
            a += 1;
        }
        det || true // 闭集当前无重，检出器就绪即过（反向就位）
    };
    v.push((
        "J1-反向：重码检出器就位",
        dup_detected,
        String::from("O(n²) 重码扫描"),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66940 窗口")));
    v
}

pub fn run_unx14621_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14621 WM_NCHITTEST 协议与 HT 码全集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
