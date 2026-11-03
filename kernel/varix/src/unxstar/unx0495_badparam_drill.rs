//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0495 坏参数注入演练（渲染面鲁棒） · 0x75F80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批6 M组）：
//!   三类坏参注入（坐标烂/格式表烂/字库索引烂）检出 3/3，好路径
//!   误伤 0，演练账只追加。
//! 逐卡要点：
//!   三类坏参闭集（坐标烂=越界/负数/回绕、格式表烂=未注册格式、
//!   字库索引烂=索引≥字库容量）检出 3/3，好路径 12 语料误伤 0，
//!   演练账只追加不抹账。
//! 设计要点：
//!   ① 坏参三类闭集：坐标烂（越界/负数/回绕三态）、格式表烂（未
//!     注册格式）、字库索引烂（索引≥容量），其余参数一概放行；
//!   ② 渲染面闸序固定：回绕→负数→越界→格式→字库，先闸回绕负
//!     数再闸边界，诊断码按闸位闭集返回；
//!   ③ 好路径 12 语料手写（覆盖三格式与字库首尾界），全过即误伤
//!     0，任一好语料被拒即 GoodHitGood 红线；
//!   ④ 演练账只追加：演练行带类/码/命中位，抹账与删行显性拒，
//!     行数守恒可复算；
//!   ⑤ 演练状态机 待命→开演→封账 单调，封账后追加显性拒；
//!   ⑥ 常量冻结：文本面 40x12、注册格式 3、字库容量 96、好语料
//!     12、坏参 12（6 坐标 + 3 格式 + 3 字库）、判据轮数 10（来
//!     源：本卡判据与显示面文本面约定）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75F80，AI-VX7-1104 批6 M组窗口第三格）。
pub const SEG_BASE: u32 = 0x75F80;

/// 文本面宽（冻结 40）。
pub const FACE_W: i64 = 40;
/// 文本面高（冻结 12）。
pub const FACE_H: i64 = 12;
/// 注册格式闭集（冻结 3：0=文本 1=图案 2=网格）。
pub const FMT_TABLE: [u32; 3] = [0, 1, 2];
/// 字库容量（冻结 96，可打印 ASCII 字形数）。
pub const GLYPH_CAP: u32 = 96;
/// 好语料数（冻结 12）。
pub const GOOD_CORPUS: usize = 12;
/// 坏参语料数（冻结 12：6 坐标 + 3 格式 + 3 字库）。
pub const BAD_CORPUS: usize = 12;
/// 判据轮数（写死 10：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 回绕探针上界（冻结 i64::MAX：整型回绕哨兵）。
pub const WRAP_HI: i64 = i64::MAX;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadParamCode {
    /// 坐标越界：x≥面宽或 y≥面高
    CoordOob,
    /// 坐标负数：x<0 或 y<0
    CoordNeg,
    /// 坐标回绕：坐标为整型上界哨兵
    CoordWrap,
    /// 格式表烂：格式号未注册
    FmtUnknown,
    /// 字库索引烂：索引≥字库容量
    GlyphOver,
    /// 好参误伤：好语料被拒（鲁棒红线）
    GoodHitGood,
    /// 演练账抹账被拒：只追加红线
    DrillResetDenied,
    /// 封账后追加被拒：状态机红线
    DrillSealed,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl BadParamCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BadParamCode::CoordOob => "坐标越界：请把坐标钳到 [0,40)x[0,12) 文本面内",
            BadParamCode::CoordNeg => "坐标负数：渲染面坐标不可为负，请先钳位再下发",
            BadParamCode::CoordWrap => "坐标回绕：坐标为整型上界哨兵，疑似上游溢出，请查调用方",
            BadParamCode::FmtUnknown => "格式表烂：格式号未注册，请改用 0/1/2 或先注册新格式",
            BadParamCode::GlyphOver => "字库索引烂：索引≥96，请查字库装载或钳到容量内",
            BadParamCode::GoodHitGood => "好参误伤：好语料被拒即鲁棒红线，请复查闸门边界",
            BadParamCode::DrillResetDenied => "演练账抹账被拒：演练账只追加，更正请追加冲正行",
            BadParamCode::DrillSealed => "封账后追加被拒：请重开新演练期再注入",
            BadParamCode::NotReproducible => "双跑不一致：复现红线，请固定种子与输入重跑",
        }
    }
    pub const ALL: [BadParamCode; 9] = [
        BadParamCode::CoordOob,
        BadParamCode::CoordNeg,
        BadParamCode::CoordWrap,
        BadParamCode::FmtUnknown,
        BadParamCode::GlyphOver,
        BadParamCode::GoodHitGood,
        BadParamCode::DrillResetDenied,
        BadParamCode::DrillSealed,
        BadParamCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码给中文提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BadParamCode::ALL.len() {
        if let Some(c) = BadParamCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75F80 段闭集内"
}

// ── 坏参三类闭集 ────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadClass {
    /// 坐标烂（越界/负数/回绕）
    Coord,
    /// 格式表烂（未注册格式）
    Format,
    /// 字库索引烂（索引≥容量）
    Glyph,
}

/// 类 → 槽位（0..3 闭集映射）。
pub fn class_slot(c: BadClass) -> usize {
    match c {
        BadClass::Coord => 0,
        BadClass::Format => 1,
        BadClass::Glyph => 2,
    }
}

// ── 渲染面探针（闸序：回绕→负数→越界→格式→字库） ────────────────
#[derive(Debug)]
pub struct RenderProbe {
    pub good_draws: u32,
    /// 拒收计数（槽序：越界/负数/回绕/格式/字库）。
    pub rejects: [u32; 5],
    pub state: DrillState,
}

impl RenderProbe {
    pub fn new() -> RenderProbe {
        RenderProbe { good_draws: 0, rejects: [0; 5], state: DrillState::Idle }
    }

    fn bump(&mut self, slot: usize) {
        if let Some(r) = self.rejects.get_mut(slot) {
            *r = r.saturating_add(1);
        }
    }

    /// 下发一笔绘制：五闸依次落闸，全过记好路径。
    pub fn draw(&mut self, x: i64, y: i64, fmt: u32, glyph: u32) -> Result<(), u32> {
        // 闸 1：回绕（整型上界哨兵，负下界由闸 2 兜住）。
        if x == WRAP_HI || y == WRAP_HI {
            self.bump(2);
            return Err(BadParamCode::CoordWrap.val());
        }
        // 闸 2：负数。
        if x < 0 || y < 0 {
            self.bump(1);
            return Err(BadParamCode::CoordNeg.val());
        }
        // 闸 3：越界。
        if x >= FACE_W || y >= FACE_H {
            self.bump(0);
            return Err(BadParamCode::CoordOob.val());
        }
        // 闸 4：格式表烂（未注册格式）。
        if !FMT_TABLE.iter().any(|f| *f == fmt) {
            self.bump(3);
            return Err(BadParamCode::FmtUnknown.val());
        }
        // 闸 5：字库索引烂（索引≥容量）。
        if glyph >= GLYPH_CAP {
            self.bump(4);
            return Err(BadParamCode::GlyphOver.val());
        }
        self.good_draws = self.good_draws.saturating_add(1);
        Ok(())
    }
}

// ── 演练状态机（待命→开演→封账，单调） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrillState {
    /// 待命：可注入
    Idle,
    /// 开演：演练进行中
    Running,
    /// 封账：演练账冻结只读
    Sealed,
}

// ── 好语料 12（手写：覆盖三格式 + 字库首尾界 + 面四角） ─────────
pub fn good_corpus() -> [(i64, i64, u32, u32); GOOD_CORPUS] {
    [
        (0, 0, 0, 65),
        (39, 11, 1, 65),
        (0, 11, 2, 0),
        (39, 0, 0, 95),
        (7, 3, 1, 42),
        (13, 5, 2, 33),
        (20, 6, 0, 96 - 1),
        (1, 1, 1, 1),
        (38, 10, 2, 90),
        (5, 2, 0, 10),
        (25, 8, 1, 55),
        (30, 4, 2, 66),
    ]
}

// ── 坏参语料 12（手写闭集：6 坐标 + 3 格式 + 3 字库） ───────────
pub fn bad_corpus() -> [(i64, i64, u32, u32, u32); BAD_CORPUS] {
    [
        (-1, 0, 0, 65, 1),          // 坐标负数（x）
        (0, -1, 0, 65, 1),          // 坐标负数（y）
        (40, 0, 0, 65, 0),          // 坐标越界（x=面宽）
        (0, 12, 0, 65, 0),          // 坐标越界（y=面高）
        (i64::MAX, 0, 0, 65, 2),    // 坐标回绕（x 上界哨兵）
        (0, i64::MAX, 0, 65, 2),    // 坐标回绕（y 上界哨兵）
        (3, 3, 3, 65, 3),           // 格式未注册（3）
        (3, 3, 7, 65, 3),           // 格式未注册（7）
        (3, 3, 9999, 65, 3),        // 格式未注册（9999）
        (3, 3, 0, 96, 4),           // 字库索引=容量
        (3, 3, 0, 200, 4),          // 字库索引=200
        (3, 3, 0, u32::MAX, 4),     // 字库索引=上界
    ]
}

/// 坏参语料 → 期望诊断码 + 类。
pub fn bad_expect(
    e: &(i64, i64, u32, u32, u32),
) -> (BadClass, u32) {
    let code = e.4;
    let code_val = match code {
        0 => BadParamCode::CoordOob.val(),
        1 => BadParamCode::CoordNeg.val(),
        2 => BadParamCode::CoordWrap.val(),
        3 => BadParamCode::FmtUnknown.val(),
        _ => BadParamCode::GlyphOver.val(),
    };
    let cls = if code <= 2 { BadClass::Coord } else if code == 3 { BadClass::Format } else { BadClass::Glyph };
    (cls, code_val)
}

// ── 演练账（只追加） ────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillRow {
    pub case_no: u32,
    pub class: BadClass,
    pub code: u32,
    pub hit: bool,
}

#[derive(Debug)]
pub struct DrillLedger {
    pub rows: Vec<DrillRow>,
    /// 类命中账（槽序：坐标/格式/字库）。
    pub caught: [u32; 3],
    pub tries: [u32; 3],
    pub state: DrillState,
}

impl DrillLedger {
    pub fn new() -> DrillLedger {
        DrillLedger { rows: Vec::new(), caught: [0; 3], tries: [0; 3], state: DrillState::Idle }
    }

    /// 追加演练行（待命/开演态可写，封账拒）。
    pub fn record(&mut self, class: BadClass, code: u32, hit: bool) -> Result<usize, u32> {
        if self.state == DrillState::Sealed {
            return Err(BadParamCode::DrillSealed.val());
        }
        let slot = class_slot(class);
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
        let no = self.rows.len() as u32;
        self.rows.push(DrillRow { case_no: no, class, code, hit });
        Ok(self.rows.len().saturating_sub(1))
    }

    /// 抹账显性拒（只追加红线）。
    pub fn clear(&mut self) -> Result<(), u32> {
        Err(BadParamCode::DrillResetDenied.val())
    }

    /// 删行显性拒（只追加红线）。
    pub fn remove_row(&mut self, case_no: u32) -> Result<(), u32> {
        let _ = case_no;
        Err(BadParamCode::DrillResetDenied.val())
    }

    /// 开演（待命→开演）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state != DrillState::Idle {
            return Err(BadParamCode::DrillSealed.val());
        }
        self.state = DrillState::Running;
        Ok(())
    }

    /// 封账（开演→封账，倒退拒）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state != DrillState::Running {
            return Err(BadParamCode::DrillSealed.val());
        }
        self.state = DrillState::Sealed;
        Ok(())
    }

    /// 三类全检出（每类 try>0 且 catch==try）。
    pub fn all_classes_caught(&self) -> bool {
        let mut slot = 0;
        while slot < 3 {
            let t = *self.tries.get(slot).unwrap_or(&0);
            let c = *self.caught.get(slot).unwrap_or(&0);
            if t == 0 || c != t {
                return false;
            }
            slot += 1;
        }
        true
    }
}

// ── LCG（固定种子，整数口径） ───────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 12 坏参全扫，单账全程只追加） ──────
pub fn inject_rounds(rounds: u32) -> Result<DrillLedger, u32> {
    let mut led = DrillLedger::new();
    let _ = led.begin();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        let mut probe = RenderProbe::new();
        for e in bad_corpus().iter() {
            let (cls, expect) = bad_expect(e);
            let got = probe.draw(e.0, e.1, e.2, e.3);
            let hit = match got {
                Err(c) => c == expect,
                Ok(_) => false,
            };
            led.record(cls, expect, hit)?;
        }
        // 好路径 12 语料误伤 0。
        for g in good_corpus().iter() {
            if probe.draw(g.0, g.1, g.2, g.3).is_err() {
                led.record(BadClass::Coord, BadParamCode::GoodHitGood.val(), false)?;
            }
        }
        r += 1;
    }
    let _ = led.seal();
    if !led.all_classes_caught()
        || led
            .rows
            .iter()
            .any(|row| !row.hit && row.code != BadParamCode::GoodHitGood.val())
    {
        return Err(BadParamCode::GoodHitGood.val());
    }
    Ok(led)
}

// ── 黄金全链：好 12 全过 + 坏 12 全抓 + 账只追加 ────────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut probe = RenderProbe::new();
    let mut good_ok = 0u32;
    for g in good_corpus().iter() {
        if probe.draw(g.0, g.1, g.2, g.3).is_ok() {
            good_ok = good_ok.saturating_add(1);
        }
    }
    let mut bad_ok = 0u32;
    for e in bad_corpus().iter() {
        let (cls, expect) = bad_expect(e);
        let got = probe.draw(e.0, e.1, e.2, e.3);
        let hit = got == Err(expect);
        if hit {
            bad_ok = bad_ok.saturating_add(1);
        }
        let _ = cls;
    }
    // 演练账只追加：全账 12 坏 + 12 好抽检行。
    let mut led = DrillLedger::new();
    let _ = led.begin();
    for e in bad_corpus().iter() {
        let (cls, expect) = bad_expect(e);
        led.record(cls, expect, true)?;
    }
    let len_after = led.rows.len();
    let clear_deny = led.clear().is_err();
    let rm_deny = led.remove_row(0).is_err();
    let kept = led.rows.len() == len_after;
    let _ = led.seal();
    let sealed_deny = led.record(BadClass::Coord, 0, true).is_err();
    Ok((
        (good_ok == GOOD_CORPUS as u32 && bad_ok == BAD_CORPUS as u32) as u32,
        (clear_deny && rm_deny && kept && sealed_deny && len_after == BAD_CORPUS) as u32,
    ))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链 2/2。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 1)) => Ok(()),
        _ => Err(BadParamCode::NotReproducible.val()),
    }
}

/// 序 B：单点三闸抽检（越界/格式/字库各一点，码须精确匹配）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = RenderProbe::new();
    let c1 = p.draw(FACE_W, 0, 0, 0);
    let c2 = p.draw(0, 0, 9, 0);
    let c3 = p.draw(0, 0, 0, GLYPH_CAP);
    match (c1, c2, c3) {
        (Err(a), Err(b), Err(c)) => {
            if a == BadParamCode::CoordOob.val()
                && b == BadParamCode::FmtUnknown.val()
                && c == BadParamCode::GlyphOver.val()
            {
                Ok(())
            } else {
                Err(BadParamCode::NotReproducible.val())
            }
        }
        _ => Err(BadParamCode::NotReproducible.val()),
    }
}

/// 序 C：常量冻结（面 40x12 / 格式 3 / 字库 96 / 语料 12+12）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        FACE_W == 40,
        FACE_H == 12,
        FMT_TABLE.len() == 3,
        GLYPH_CAP == 96,
        GOOD_CORPUS == 12,
        BAD_CORPUS == 12,
    ) {
        (true, true, true, true, true, true) => Ok(()),
        _ => Err(BadParamCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BadParamCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < BadParamCode::ALL.len() {
        if let Some(c) = BadParamCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // ① 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((a, b)) => *a == 1 && *b == 1,
            Err(_) => false,
        },
        match &g {
            Ok((a, b)) => format!("语料检出={} 账红线={}", a, b),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ② 好路径 12 语料误伤 0。
    let mut p2 = RenderProbe::new();
    let mut good_pass = 0u32;
    for gg in good_corpus().iter() {
        if p2.draw(gg.0, gg.1, gg.2, gg.3).is_ok() {
            good_pass = good_pass.saturating_add(1);
        }
    }
    v.push((
        "J1-好路径误伤0",
        good_pass == GOOD_CORPUS as u32,
        format!("好语料通过 {}/12", good_pass),
    ));

    // ③ 三类检出 3/3（类账全命中）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-三类检出3/3",
        match &inj {
            Ok(l) => l.all_classes_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!(
                "坐标{}/{} 格式{}/{} 字库{}/{}",
                *l.caught.get(0).unwrap_or(&0),
                *l.tries.get(0).unwrap_or(&0),
                *l.caught.get(1).unwrap_or(&0),
                *l.tries.get(1).unwrap_or(&0),
                *l.caught.get(2).unwrap_or(&0),
                *l.tries.get(2).unwrap_or(&0)
            ),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // ④ 演练 10/10 全命中（三类总数守恒 12×10）。
    let rounds_ok = match &inj {
        Ok(l) => {
            let total = l.tries.iter().fold(0u32, |a, t| a.saturating_add(*t));
            total == BAD_CORPUS as u32 * JUDGE_ROUNDS as u32
        }
        Err(_) => false,
    };
    v.push((
        "J1-演练10/10",
        rounds_ok && inj.is_ok(),
        String::from("10 轮 × 12 坏参全扫全命中"),
    ));

    // ⑤ 坏参闭集码精确匹配（12/12）。
    let mut p5 = RenderProbe::new();
    let mut exact = 0u32;
    for e in bad_corpus().iter() {
        let (_, expect) = bad_expect(e);
        if p5.draw(e.0, e.1, e.2, e.3) == Err(expect) {
            exact = exact.saturating_add(1);
        }
    }
    v.push((
        "J1-坏参码精确匹配",
        exact == BAD_CORPUS as u32,
        format!("12 坏参诊断码全对 {}/12", exact),
    ));

    // ⑥ 演练账只追加（抹账/删行拒 + 行数守恒）。
    let mut led6 = DrillLedger::new();
    let _ = led6.begin();
    let _ = led6.record(BadClass::Coord, BadParamCode::CoordOob.val(), true);
    let len6 = led6.rows.len();
    let d6 = led6.clear().is_err() && led6.remove_row(0).is_err() && led6.rows.len() == len6;
    v.push((
        "J1-演练账只追加",
        d6,
        String::from("抹账/删行全拒且行数守恒"),
    ));

    // ⑦ 演练状态机单调（封账后追加拒）。
    let mut led7 = DrillLedger::new();
    let _ = led7.begin();
    let _ = led7.seal();
    let sealed_deny = led7.record(BadClass::Format, 0, true).is_err();
    let idle_reject = DrillLedger::new().seal().is_err();
    v.push((
        "J1-状态机单调",
        sealed_deny && idle_reject,
        String::from("待命→开演→封账 单调，越态全拒"),
    ));

    // ⑧ 闸序边界抽检（面内边角放行，出界/满界拒）。
    let mut p8 = RenderProbe::new();
    let inside_edge = p8.draw(FACE_W - 1, FACE_H - 1, 2, 0).is_ok();
    let just_out = p8.draw(FACE_W, 0, 0, 0).is_err();
    v.push((
        "J1-闸序边界抽检",
        inside_edge && just_out,
        String::from("(39,11) 放行 / (40,0) 拒收"),
    ));

    // ⑨ 回绕哨兵单点（x=i64::MAX 必报回绕码）。
    let mut p9 = RenderProbe::new();
    let wrap_hit = p9.draw(WRAP_HI, 0, 0, 0) == Err(BadParamCode::CoordWrap.val());
    v.push((
        "J1-回绕哨兵必抓",
        wrap_hit,
        String::from("i64::MAX 坐标报 CoordWrap"),
    ));

    // ⑩ 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push(("J1-探针三序", a && b && c, format!("a={} b=检出{} c=检出{}", a, b, c)));

    // ⑪ 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // ⑫ 诊断码闭集查表。
    let mut codes_ok = true;
    let mut ci = 0;
    while ci < BadParamCode::ALL.len() {
        if let Some(c) = BadParamCode::ALL.get(ci) {
            if detail(c.val()) != c.detail() {
                codes_ok = false;
            }
        }
        ci += 1;
    }
    v.push((
        "J3-诊断码闭集",
        codes_ok && detail(0xDEAD_0000) == "未知诊断码：不在 0x75F80 段闭集内",
        String::from("ALL 查表一致 + 未知码兜底"),
    ));

    // ⑬ 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75F80 窗口 + 0x40 对齐"),
    ));
    v
}

pub fn run_unx0495_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0495 坏参数注入演练");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_golden_chain() {
        assert_eq!(golden_run(), Ok((1, 1)));
    }

    #[test]
    fn t02_good_corpus_zero_false_positive() {
        let mut p = RenderProbe::new();
        for g in good_corpus().iter() {
            assert!(p.draw(g.0, g.1, g.2, g.3).is_ok(), "好语料被误伤 {:?}", g);
        }
        assert_eq!(p.good_draws, GOOD_CORPUS as u32);
        assert!(p.rejects.iter().all(|r| *r == 0));
    }

    #[test]
    fn t03_bad_corpus_exact_codes() {
        let mut p = RenderProbe::new();
        for e in bad_corpus().iter() {
            let (cls, expect) = bad_expect(e);
            assert_eq!(p.draw(e.0, e.1, e.2, e.3), Err(expect), "类 {:?} 码错", cls);
        }
        assert_eq!(*p.rejects.get(0).unwrap_or(&0), 2);
        assert_eq!(*p.rejects.get(1).unwrap_or(&0), 2);
        assert_eq!(*p.rejects.get(2).unwrap_or(&0), 2);
        assert_eq!(*p.rejects.get(3).unwrap_or(&0), 3);
        assert_eq!(*p.rejects.get(4).unwrap_or(&0), 3);
    }

    #[test]
    fn t04_inject_rounds_full() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(x) => x,
            Err(_) => DrillLedger::new(),
        };
        assert!(led.all_classes_caught());
        assert_eq!(*led.tries.get(0).unwrap_or(&0), 6 * JUDGE_ROUNDS as u32);
        assert_eq!(*led.tries.get(1).unwrap_or(&0), 3 * JUDGE_ROUNDS as u32);
        assert_eq!(*led.tries.get(2).unwrap_or(&0), 3 * JUDGE_ROUNDS as u32);
        assert_eq!(*led.caught.get(0).unwrap_or(&0), *led.tries.get(0).unwrap_or(&0));
        assert_eq!(*led.caught.get(1).unwrap_or(&0), *led.tries.get(1).unwrap_or(&0));
        assert_eq!(*led.caught.get(2).unwrap_or(&0), *led.tries.get(2).unwrap_or(&0));
    }

    #[test]
    fn t05_ledger_append_only() {
        let mut led = DrillLedger::new();
        let _ = led.begin();
        assert!(led.record(BadClass::Coord, 0, true).is_ok());
        let len = led.rows.len();
        assert!(led.clear().is_err());
        assert!(led.remove_row(0).is_err());
        assert_eq!(led.rows.len(), len);
    }

    #[test]
    fn t06_drill_state_machine() {
        let mut led = DrillLedger::new();
        assert!(DrillLedger::new().seal().is_err());
        assert!(led.begin().is_ok());
        assert!(led.begin().is_err());
        assert!(led.seal().is_ok());
        assert!(led.record(BadClass::Glyph, 0, true).is_err());
        assert!(led.seal().is_err());
    }

    #[test]
    fn t07_wrap_sentinel_and_neg() {
        let mut p = RenderProbe::new();
        assert_eq!(p.draw(WRAP_HI, 0, 0, 0), Err(BadParamCode::CoordWrap.val()));
        assert_eq!(p.draw(0, WRAP_HI, 0, 0), Err(BadParamCode::CoordWrap.val()));
        assert_eq!(p.draw(i64::MIN, 0, 0, 0), Err(BadParamCode::CoordNeg.val()));
        assert_eq!(p.draw(-1, 0, 0, 0), Err(BadParamCode::CoordNeg.val()));
    }

    #[test]
    fn t08_boundary_rows() {
        let mut p = RenderProbe::new();
        assert!(p.draw(FACE_W - 1, FACE_H - 1, 2, GLYPH_CAP - 1).is_ok());
        assert_eq!(p.draw(FACE_W, 0, 0, 0), Err(BadParamCode::CoordOob.val()));
        assert_eq!(p.draw(0, FACE_H, 0, 0), Err(BadParamCode::CoordOob.val()));
        assert_eq!(p.draw(0, 0, 3, 0), Err(BadParamCode::FmtUnknown.val()));
        assert_eq!(p.draw(0, 0, 0, GLYPH_CAP), Err(BadParamCode::GlyphOver.val()));
    }

    #[test]
    fn t09_repro_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn t10_code_closed_set() {
        assert_eq!(BadParamCode::ALL.len(), 9);
        let mut i = 0;
        while i < BadParamCode::ALL.len() {
            if let Some(c) = BadParamCode::ALL.get(i) {
                assert!(c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40);
                assert!(!detail(c.val()).is_empty());
            }
            i += 1;
        }
        assert_eq!(detail(0xDEAD_0000), "未知诊断码：不在 0x75F80 段闭集内");
    }

    #[test]
    fn t11_probes_abc() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn t12_class_slot_mapping() {
        assert_eq!(class_slot(BadClass::Coord), 0);
        assert_eq!(class_slot(BadClass::Format), 1);
        assert_eq!(class_slot(BadClass::Glyph), 2);
        assert_eq!(good_corpus().len(), GOOD_CORPUS);
        assert_eq!(bad_corpus().len(), BAD_CORPUS);
    }
}
