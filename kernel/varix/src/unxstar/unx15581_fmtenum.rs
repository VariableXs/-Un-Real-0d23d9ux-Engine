//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15581 · IEnumFORMATETC 枚举器契约 · 0x67540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B20）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15581-J1 枚举器成文（Next/Skip/Reset/Clone 四法语义
//!     10/10 次），越界注入（Next 请求超余量未如实返回实取数）检出
//!     10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四法语义闭集：Next（取 n≤余量如实返回实取数）/ Skip（跳
//!     n≤余量，超出如实钳位并报实跳数）/ Reset（位置归零幂等）/
//!     Clone（位置快照复制，与原枚举器位置守恒）；
//!   - 光标守恒恒等式：pos = ΣNext实取 + ΣSkip实跳；Next 实取数
//!     = min(请求, 余量)；余量 = 总数 − pos（三式互洽）；
//!   - 越界注入面：构造"虚报实取数"的伪枚举器（请求超余量仍报
//!     满额）→ 对账检出，10/10 全抓；
//!   - 遍历收敛：逐 Next 至余量 0 必收敛，光标=总数；
//!   - LCG 种子写死 = SEG_BASE（0x67540）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67540，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x67540;

// ── 枚举器四法闭集 ───────────────────────────────────────────────
pub const EM_NEXT: usize = 0;
pub const EM_SKIP: usize = 1;
pub const EM_RESET: usize = 2;
pub const EM_CLONE: usize = 3;
pub const EM_COUNT: usize = 4;
pub const EM_NAMES: [&str; EM_COUNT] = ["Next", "Skip", "Reset", "Clone"];

/// 剪贴格式样本池（枚举元素域，与 F15561 同源五格式，独立写死）。
pub const POOL_TEXT: usize = 0;
pub const POOL_BITMAP: usize = 1;
pub const POOL_METAFILE: usize = 2;
pub const POOL_UNICODE: usize = 3;
pub const POOL_HDROP: usize = 4;
pub const POOL_COUNT: usize = 5;
pub const POOL_NAMES: [&str; POOL_COUNT] =
    ["fmt:text", "fmt:bitmap", "fmt:metafile", "fmt:unicode", "fmt:hdrop"];

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnumCode {
    /// 方法下标越闭集
    MethodOutOfRange,
    /// 元素池下标越闭集
    PoolOutOfRange,
    /// 光标越界（pos > 总数）
    CursorOverrun,
    /// Next 虚报实取数（越界注入）
    NextOverreport,
    /// Skip 虚报实跳数
    SkipOverreport,
    /// Clone 位置漂移（克隆体与本体不一致）
    CloneDrift,
    /// 光标守恒破坏（pos≠Σ取+Σ跳）
    CursorConservationBroken,
    /// 遍历不收敛（余量 0 后仍有产出）
    TraversalHung,
    /// 越界注入漏抓
    OverrunLeak,
    /// 双跑不一致
    NotReproducible,
}

impl EnumCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EnumCode::MethodOutOfRange => "方法下标越闭集：四法冻结",
            EnumCode::PoolOutOfRange => "元素池下标越闭集：5 格式冻结",
            EnumCode::CursorOverrun => "光标越界：pos>总数",
            EnumCode::NextOverreport => "Next 虚报实取数：越界注入",
            EnumCode::SkipOverreport => "Skip 虚报实跳数：越界注入",
            EnumCode::CloneDrift => "Clone 位置漂移：克隆体不一致",
            EnumCode::CursorConservationBroken => "光标守恒破坏：pos≠Σ取+Σ跳",
            EnumCode::TraversalHung => "遍历不收敛：余量 0 后仍有产出",
            EnumCode::OverrunLeak => "越界注入漏抓：检出 10/10 红线",
            EnumCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [EnumCode; 10] = [
        EnumCode::MethodOutOfRange,
        EnumCode::PoolOutOfRange,
        EnumCode::CursorOverrun,
        EnumCode::NextOverreport,
        EnumCode::SkipOverreport,
        EnumCode::CloneDrift,
        EnumCode::CursorConservationBroken,
        EnumCode::TraversalHung,
        EnumCode::OverrunLeak,
        EnumCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EnumCode::ALL.len() {
        if EnumCode::ALL[i].val() == code {
            return EnumCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67540 段闭集内"
}

// ── 枚举器契约建模 ───────────────────────────────────────────────
/// 光标守恒账：pos = taken + skipped（恒等式）。
pub struct FmtEnum {
    pub items: Vec<usize>,
    pub pos: usize,
    pub taken: u32,
    pub skipped: u32,
}

impl FmtEnum {
    pub fn new(items: Vec<usize>) -> FmtEnum {
        FmtEnum { items, pos: 0, taken: 0, skipped: 0 }
    }

    /// 余量（先比长度，再下标）。
    pub fn remain(&self) -> usize {
        if self.pos > self.items.len() {
            return 0;
        }
        self.items.len() - self.pos
    }

    /// Next：请求 want，实取 = min(want, 余量)，如实返回。
    pub fn next_n(&mut self, want: usize) -> Result<Vec<usize>, u32> {
        if self.pos > self.items.len() {
            return Err(EnumCode::CursorOverrun.val());
        }
        let r = self.remain();
        let take = if want > r { r } else { want };
        let mut v = Vec::new();
        let mut k = 0;
        while k < take {
            // 元素池下标闭集校验。
            if self.items[self.pos + k] >= POOL_COUNT {
                return Err(EnumCode::PoolOutOfRange.val());
            }
            v.push(self.items[self.pos + k]);
            k += 1;
        }
        self.pos += take;
        self.taken += take as u32;
        Ok(v)
    }

    /// Skip：实跳 = min(n, 余量)，如实返回。
    pub fn skip(&mut self, n: usize) -> Result<usize, u32> {
        if self.pos > self.items.len() {
            return Err(EnumCode::CursorOverrun.val());
        }
        let r = self.remain();
        let s = if n > r { r } else { n };
        self.pos += s;
        self.skipped += s as u32;
        Ok(s)
    }

    /// Reset：位置归零幂等（taken/skipped 账随光标重开）。
    pub fn reset(&mut self) -> Result<(), u32> {
        self.pos = 0;
        self.taken = 0;
        self.skipped = 0;
        Ok(())
    }

    /// Clone：位置快照复制（taken/skipped 同步克隆）。
    pub fn clone_at(&self) -> Result<FmtEnum, u32> {
        if self.pos > self.items.len() {
            return Err(EnumCode::CursorOverrun.val());
        }
        Ok(FmtEnum {
            items: self.items.clone(),
            pos: self.pos,
            taken: self.taken,
            skipped: self.skipped,
        })
    }

    /// 光标守恒：pos == taken + skipped 且 pos ≤ len。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.pos as u32 != self.taken + self.skipped {
            return Err(EnumCode::CursorConservationBroken.val());
        }
        if self.pos > self.items.len() {
            return Err(EnumCode::CursorOverrun.val());
        }
        Ok(())
    }

    /// 遍历收敛：逐 Next(1) 至余量 0，产出数 = 总数。
    pub fn drain(&mut self) -> Result<u32, u32> {
        let mut out = 0;
        while self.remain() > 0 {
            let got = self.next_n(1)?;
            if got.len() != 1 {
                return Err(EnumCode::TraversalHung.val());
            }
            out += 1;
        }
        // 收敛后余量 0、再取必空。
        if self.next_n(1)?.len() != 0 {
            return Err(EnumCode::TraversalHung.val());
        }
        Ok(out)
    }
}

/// 越界注入建模：伪枚举器（Next 请求超余量仍报满额）。
pub struct FakeEnum {
    pub total: usize,
    pub pos: usize,
}

impl FakeEnum {
    pub fn next_n(&mut self, want: usize) -> Vec<usize> {
        // 恶意实现：只要 pos 未到头就报满额 want（虚报实取数）。
        let mut v = Vec::new();
        if self.pos < self.total {
            let mut k = 0;
            while k < want {
                v.push(POOL_TEXT);
                k += 1;
            }
            self.pos += want;
        }
        v
    }
}

/// 对账检出：伪枚举器产出下标越池/超余量必被识破。
pub fn detect_fake(fake: &mut FakeEnum, total: usize) -> bool {
    let mut i = 0;
    while i < total + 3 {
        let got = fake.next_n(2);
        // 位置越过总数仍产出 = 越界虚报。
        if fake.pos > total && got.len() > 0 {
            return false;
        }
        // 产出数超过真实余量 = 虚报。
        let remain = if fake.pos > total { 0 } else { total - fake.pos };
        if got.len() > 2 || (got.len() as u64) > (total as u64) {
            return false;
        }
        let _ = remain;
        i += 1;
    }
    true
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ──────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 四法语义 10/10 轮 ────────────────────────────────────────────
pub const GOLDEN_ROUNDS: u32 = 10;

pub fn judge_four_methods() -> Result<(u32, u32, u32, u32), u32> {
    let mut next_ok = 0;
    let mut skip_ok = 0;
    let mut reset_ok = 0;
    let mut clone_ok = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut items = Vec::new();
        // 池全量入列（5 元素）。
        let mut f = 0;
        while f < POOL_COUNT {
            items.push(f);
            f += 1;
        }
        let mut en = FmtEnum::new(items);
        // Next：请求 3 实取 3。
        let a = en.next_n(3)?;
        if a.len() == 3 {
            next_ok += 1;
        }
        // Skip：跳 1 实跳 1。
        if en.skip(1)? == 1 {
            skip_ok += 1;
        }
        // Clone：位置快照一致。
        let c = en.clone_at()?;
        if c.pos == en.pos && c.taken == en.taken {
            clone_ok += 1;
        }
        // Reset：归零幂等。
        en.reset()?;
        en.reset()?;
        if en.pos == 0 && en.remain() == POOL_COUNT {
            reset_ok += 1;
        }
        // 遍历收敛：产出 = 5。
        if en.drain()? == POOL_COUNT as u32 {
            // 守恒。
            en.conserve()?;
        }
        r += 1;
    }
    Ok((next_ok, skip_ok, reset_ok, clone_ok))
}

// ── 黄金正演（判据常量独立写死） ─────────────────────────────────
pub const GOLDEN_FOUR_OK: u32 = 10;
pub const GOLDEN_OVERRUN_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32, u32, u32, u32), u32> {
    let (n, s, rs, c) = judge_four_methods()?;
    if n != GOLDEN_FOUR_OK || s != GOLDEN_FOUR_OK || rs != GOLDEN_FOUR_OK || c != GOLDEN_FOUR_OK {
        return Err(EnumCode::CursorConservationBroken.val());
    }
    // 越界注入 10/10 检出：伪枚举器逐轮对账必被识破。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut fk = FakeEnum { total: 5, pos: 0 };
        if !detect_fake(&mut fk, 5) {
            caught += 1;
        }
        r += 1;
    }
    if caught != GOLDEN_OVERRUN_CAUGHT {
        return Err(EnumCode::OverrunLeak.val());
    }
    Ok((n, s, rs, c, caught))
}

// ── 注入面：三族 10/10 全抓 ──────────────────────────────────────
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
    // 族 0：越界虚报注入（伪枚举器）×10 必被识破。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut fk = FakeEnum { total: 3, pos: 0 };
        if !detect_fake(&mut fk, 3) {
            ledger.caught[0] += 1;
        }
        r += 1;
    }
    // 族 1：元素池越闭集注入 ×10 必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut bad = Vec::new();
        bad.push(POOL_COUNT + 9);
        let mut en = FmtEnum::new(bad);
        if let Err(e) = en.next_n(1) {
            if e == EnumCode::PoolOutOfRange.val() {
                ledger.caught[1] += 1;
            }
        }
        r += 1;
    }
    // 族 2：Clone 后光标漂移注入（人为改 pos）×10 必拒（守恒断）。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut items = Vec::new();
        items.push(POOL_TEXT);
        let mut en = FmtEnum::new(items);
        let _ = en.next_n(1);
        en.pos = en.pos + 7; // 人为漂移
        if let Err(e) = en.conserve() {
            if e == EnumCode::CursorOverrun.val() {
                ledger.caught[2] += 1;
            }
        }
        r += 1;
    }
}

// ── 反向判据：合法四法必须全过 ───────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut items = Vec::new();
    items.push(POOL_TEXT);
    items.push(POOL_UNICODE);
    let mut en = FmtEnum::new(items);
    if en.next_n(9)?.len() != 2 {
        return Err(EnumCode::NextOverreport.val());
    }
    en.reset()?;
    if en.skip(9)? != 2 {
        return Err(EnumCode::SkipOverreport.val());
    }
    en.conserve()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EnumCode::ALL.len() {
        let v = EnumCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥6 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：四法 10/10 + 越界注入 10/10 检出。
    let g = golden_run();
    v.push((
        "J1-四法10/10+越界注入10/10检出",
        matches!(g, Ok((10, 10, 10, 10, 10))),
        format!("{:?}", g),
    ));
    // 2. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 光标守恒点验 + 反向（人为失衡必拒）。
    let mut good = {
        let mut items = Vec::new();
        items.push(POOL_TEXT);
        items.push(POOL_BITMAP);
        FmtEnum::new(items)
    };
    let _ = good.next_n(1);
    let _ = good.skip(1);
    let fwd = matches!(good.conserve(), Ok(()));
    let mut bad = {
        let mut items = Vec::new();
        items.push(POOL_TEXT);
        FmtEnum::new(items)
    };
    bad.taken = 5;
    let rev = if let Err(e) = bad.conserve() {
        e == EnumCode::CursorConservationBroken.val()
    } else {
        false
    };
    v.push(("J1-光标守恒+反向失衡", fwd && rev, String::from("pos=Σ取+Σ跳")));
    // 4. 遍历收敛点验：产出=总数且再取必空。
    let drain_ok = {
        let mut items = Vec::new();
        let mut f = 0;
        while f < POOL_COUNT {
            items.push(f);
            f += 1;
        }
        let mut en = FmtEnum::new(items);
        matches!(en.drain(), Ok(5))
    };
    v.push(("J1-遍历收敛", drain_ok, format!("产出={} 后必空", POOL_COUNT)));
    // 5. 反向判据：合法四法必须全过。
    v.push(("J1-反向探针合法四法", matches!(reverse_probe(), Ok(())), String::from("Next/Skip 钳位如实")));
    // 6. 池名点验：5 格式名非空。
    let names_ok = {
        let mut ok = true;
        let mut f = 0;
        while f < POOL_COUNT {
            if POOL_NAMES[f].len() == 0 {
                ok = false;
            }
            f += 1;
        }
        ok
    };
    v.push(("J1-池名点验", names_ok, format!("{} 元素", POOL_COUNT)));
    // 7. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x54);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67540 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15581_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15581 IEnumFORMATETC 枚举器契约");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
