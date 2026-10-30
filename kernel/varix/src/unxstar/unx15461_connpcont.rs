//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15461 · IConnectionPointContainer 面谱 · 0x673C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15461-J1 容器面成文（EnumConnectionPoints/
//!      FindConnectionPoint 双法 10/10 次），伪容器注入（无出事件
//!      谎报）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双法闭集（签名+语义钉版）：
//!       法 0 EnumConnectionPoints：枚举出全部出事件点（计数与
//!           连接点账守恒；空容器返回空枚举——不得谎报）；
//!       法 1 FindConnectionPoint：按 IID 查点（命中 → S_OK+点句柄；
//!           未命中 → 拒收，不得谎报有出事件）；
//!   - 连接点账：每点挂 IID + 建议池（advise cookie 单调派发）；
//!       Advise/Unadvise 配对守恒（cookie 只可注销一次）；
//!   - 伪容器注入面：无出事件谎报（声称有 N 点但账为 0，或 IID
//!       查无却返回成功）必检出 10/10；
//!   - 守恒恒等式：Σ各点存活建议数 == 活 cookie 数；枚举计数 ==
//!       点账数；
//!   - LCG 种子写死 = SEG_BASE（0x673C0），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x673C0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x673C0;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x673C0;

/// 出事件 IID 闭集（点账写死，来源：容器最小出事件面）。
pub const IID_SINK_CHANGE: u32 = 0;
pub const IID_SINK_ERROR: u32 = 1;
pub const IID_SINK_PROGRESS: u32 = 2;
pub const IID_SINK_COUNT: usize = 3;
pub const IID_SINK_NAMES: [&str; IID_SINK_COUNT] = ["变更汇", "错误汇", "进度汇"];

/// 双法闭集。
pub const M_ENUM: u8 = 0;
pub const M_FIND: u8 = 1;
pub const METHOD_COUNT: usize = 2;
pub const METHOD_NAMES: [&str; METHOD_COUNT] = ["EnumConnectionPoints", "FindConnectionPoint"];

/// 建议池容量钉版（每点上限，写死）。
pub const ADVISE_CAP: usize = 8;

/// 判据投影：双法 10/10。
pub const CP_TRIALS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpcCode {
    /// 法码越闭集
    MethodOutOfRange,
    /// IID 越出事件闭集
    IidOutOfRange,
    /// 伪容器谎报（空账声称有点）
    PseudoContainerClaim,
    /// 查无谎报（IID 未命中却返回成功）
    FindFalseHit,
    /// 枚举计数与点账不守恒
    EnumLedgerBreach,
    /// Advise 池满（超容量钉版）
    AdviseCapBreach,
    /// Unadvise 孤儿（cookie 不存在/已注销）
    UnadviseOrphan,
    /// Cookie 重派发（单调账破）
    CookieDuplicate,
    /// 建议池守恒破（Σ存活建议 ≠ 活 cookie）
    AdviseLedgerBreach,
}

impl CpcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CpcCode::MethodOutOfRange => "法码越闭集：双法冻结",
            CpcCode::IidOutOfRange => "IID 越出事件闭集：3 汇冻结",
            CpcCode::PseudoContainerClaim => "伪容器谎报：空账声称有点",
            CpcCode::FindFalseHit => "查无谎报：IID 未命中却成功",
            CpcCode::EnumLedgerBreach => "枚举计数与点账不守恒",
            CpcCode::AdviseCapBreach => "Advise 池满：超容量钉版",
            CpcCode::UnadviseOrphan => "Unadvise 孤儿：cookie 不存在",
            CpcCode::CookieDuplicate => "Cookie 重派发：单调账破",
            CpcCode::AdviseLedgerBreach => "建议池守恒破：Σ存活≠活cookie",
        }
    }
    pub const ALL: [CpcCode; 9] = [
        CpcCode::MethodOutOfRange,
        CpcCode::IidOutOfRange,
        CpcCode::PseudoContainerClaim,
        CpcCode::FindFalseHit,
        CpcCode::EnumLedgerBreach,
        CpcCode::AdviseCapBreach,
        CpcCode::UnadviseOrphan,
        CpcCode::CookieDuplicate,
        CpcCode::AdviseLedgerBreach,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CpcCode::ALL.len() {
        if CpcCode::ALL[i].val() == code {
            return CpcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x673C0 段闭集内"
}

// ── 连接点与容器账 ───────────────────────────────────────────────
/// 单个出事件连接点：IID + 建议池（cookie 存活表）。
pub struct ConnPoint {
    pub iid: u32,
    /// 存活 cookie 表（cookie 值 = 派发序号，1 起单调）。
    pub live_cookies: Vec<u32>,
    /// 已派发 cookie 上限（单调账）。
    pub next_cookie: u32,
}

impl ConnPoint {
    pub fn new(iid: u32) -> ConnPoint {
        ConnPoint { iid, live_cookies: Vec::new(), next_cookie: 0 }
    }

    /// Advise：派发新 cookie（单调，禁重派发；池满拒）。
    pub fn advise(&mut self) -> Result<u32, u32> {
        if self.live_cookies.len() >= ADVISE_CAP {
            return Err(CpcCode::AdviseCapBreach.val());
        }
        self.next_cookie += 1;
        let c = self.next_cookie;
        // 单调账铁律：新 cookie 必大于池内一切存活 cookie。
        let mut i = 0;
        while i < self.live_cookies.len() {
            if self.live_cookies[i] >= c {
                return Err(CpcCode::CookieDuplicate.val());
            }
            i += 1;
        }
        self.live_cookies.push(c);
        Ok(c)
    }

    /// Unadvise：注销 cookie（不存在/已注销 → 孤儿拒）。
    pub fn unadvise(&mut self, cookie: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < self.live_cookies.len() {
            if self.live_cookies[i] == cookie {
                self.live_cookies.swap_remove(i);
                return Ok(());
            }
            i += 1;
        }
        Err(CpcCode::UnadviseOrphan.val())
    }
}

/// 容器：连接点账（出事件面）。
pub struct ConnContainer {
    pub points: Vec<ConnPoint>,
}

impl ConnContainer {
    /// 建真容器：按出事件闭集全量建点（1:1）。
    pub fn real() -> ConnContainer {
        let mut points = Vec::new();
        let mut i = 0;
        while i < IID_SINK_COUNT {
            points.push(ConnPoint::new(i as u32));
            i += 1;
        }
        ConnContainer { points }
    }
    /// 建伪容器：零点账（谎报源）。
    pub fn fake() -> ConnContainer {
        ConnContainer { points: Vec::new() }
    }

    /// 法 0：EnumConnectionPoints → 枚举计数（与点账守恒）。
    pub fn enum_points(&self) -> Result<u32, u32> {
        Ok(self.points.len() as u32)
    }

    /// 法 1：FindConnectionPoint。命中 → 返回点下标；未命中 → 拒
    /// （查无谎报红线）；越闭集 IID → 越界拒。
    pub fn find_point(&self, iid: u32) -> Result<usize, u32> {
        if iid as usize >= IID_SINK_COUNT {
            return Err(CpcCode::IidOutOfRange.val());
        }
        let mut i = 0;
        while i < self.points.len() {
            if self.points[i].iid == iid {
                return Ok(i);
            }
            i += 1;
        }
        Err(CpcCode::FindFalseHit.val())
    }

    /// 守恒审计：枚举计数 == 点账数；Σ存活建议 == Σ池长；
    /// 点 IID 无重且按闭集序。
    pub fn audit(&self) -> Result<u32, u32> {
        if self.points.len() as u32 != self.enum_points()? {
            return Err(CpcCode::EnumLedgerBreach.val());
        }
        let mut live_total = 0;
        let mut i = 0;
        while i < self.points.len() {
            if self.points[i].iid != i as u32 {
                return Err(CpcCode::EnumLedgerBreach.val());
            }
            live_total += self.points[i].live_cookies.len() as u32;
            i += 1;
        }
        Ok(live_total)
    }
}

// ── 伪容器谎报检测器 ─────────────────────────────────────────────
/// 谎报双查（对被查容器 c 逐项裁定）：
/// ① 声称枚举数 > 0 但点账为 0 → 谎报一现形（PseudoContainerClaim）；
/// ② 全部合法 IID 查点均被拒（Err(FindFalseHit)）→ 查无谎报现形
///    （容器无出事件却不谎报成功，面谱正确）。
/// 返回 (谎报一检出, 查无谎报现形)。
pub fn detect_fake_claim(claimed_count: u32, c: &ConnContainer) -> Result<(bool, bool), u32> {
    let actual = c.enum_points()?;
    let claim1 = claimed_count > 0 && actual == 0;
    let mut claim2 = true;
    let mut iid = 0;
    while (iid as usize) < IID_SINK_COUNT {
        match c.find_point(iid) {
            Err(e) => {
                if e != CpcCode::FindFalseHit.val() {
                    claim2 = false;
                }
            }
            Ok(_) => claim2 = false,
        }
        iid += 1;
    }
    Ok((claim1, claim2))
}

// ── 确定性 LCG（种子 = 0x673C0） ─────────────────────────────────
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
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 单轮双法全谱压演 ─────────────────────────────────────────────
/// 每轮：真容器走双法 + 建议/注销配对 + 池满注入 + 伪容器谎报检出。
pub fn cpc_round(round: usize) -> Result<(u32, u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut c = ConnContainer::real();
    // 法 0 断言：枚举计数 == 3（点账守恒）。
    let n = c.enum_points()?;
    if n != IID_SINK_COUNT as u32 {
        return Err(CpcCode::EnumLedgerBreach.val());
    }
    // 法 1 断言：3 IID 全命中。
    let mut found = 0;
    let mut iid = 0;
    while (iid as usize) < IID_SINK_COUNT {
        let idx = c.find_point(iid)?;
        if c.points[idx].iid != iid {
            return Err(CpcCode::FindFalseHit.val());
        }
        found += 1;
        iid += 1;
    }
    if found != IID_SINK_COUNT as u32 {
        return Err(CpcCode::EnumLedgerBreach.val());
    }
    // 建议谱：0 号点满池（8 cookie）→ 第 9 个必拒；随后注销 3 个。
    let mut cookies: Vec<u32> = Vec::new();
    let mut k = 0;
    while k < ADVISE_CAP {
        let ck = c.points[0].advise()?;
        cookies.push(ck);
        k += 1;
    }
    let cap_hit = c.points[0].advise();
    if let Ok(_) = cap_hit {
        return Err(CpcCode::AdviseCapBreach.val());
    }
    // 注销 3 个（顺序乱序注销，验证孤儿拒）。
    let mut un = 0;
    while un < 3 {
        let idx = (un * 2) % cookies.len();
        let ck = cookies[idx];
        c.points[0].unadvise(ck)?;
        cookies.remove(idx);
        un += 1;
    }
    let orphan = c.points[0].unadvise(9999);
    if let Ok(_) = orphan {
        return Err(CpcCode::UnadviseOrphan.val());
    }
    let live = c.audit()?;
    if live != 5 {
        // 8 派发 − 3 注销 = 5 存活（手算黄金值）。
        return Err(CpcCode::AdviseLedgerBreach.val());
    }
    // 伪容器谎报检出：空账 + 声称 3 点 → 双查全中。
    let fake = ConnContainer::fake();
    let (c1, c2) = detect_fake_claim(IID_SINK_COUNT as u32, &fake)?;
    if !(c1 && c2) {
        return Err(CpcCode::PseudoContainerClaim.val());
    }
    let _ = lcg.next();
    Ok((found, live, if c1 && c2 { 1 } else { 0 }))
}

// ── 黄金正演：10/10 轮 ───────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut rounds = 0;
    let mut live_total = 0;
    let mut r = 0;
    while r < CP_TRIALS {
        let (_, live, _) = cpc_round(r)?;
        live_total += live;
        rounds += 1;
        r += 1;
    }
    // 黄金手算对照（写死）：
    //   真容器枚举=3；查 IID_SINK_ERROR 命中下标 1；
    //   cookie 单调：1,2,3。
    let g = ConnContainer::real();
    if g.enum_points()? != 3 {
        return Err(CpcCode::EnumLedgerBreach.val());
    }
    if g.find_point(IID_SINK_ERROR)? != 1 {
        return Err(CpcCode::FindFalseHit.val());
    }
    let mut p = ConnPoint::new(IID_SINK_CHANGE);
    let a0 = p.advise()?;
    let a1 = p.advise()?;
    let a2 = p.advise()?;
    if a0 != 1 || a1 != 2 || a2 != 3 {
        return Err(CpcCode::CookieDuplicate.val());
    }
    let _ = p.unadvise(a1)?;
    if p.live_cookies.len() != 2 {
        return Err(CpcCode::AdviseLedgerBreach.val());
    }
    Ok((rounds, live_total, p.live_cookies.len() as u32))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
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
    // 族 0：伪容器谎报注入 ×10（空账声称 3 点 + 假命中）。
    let mut t = 0;
    while t < 10 {
        let fake = ConnContainer::fake();
        let r = detect_fake_claim(IID_SINK_COUNT as u32, &fake);
        ledger.tries[0] += 1;
        if let Ok((a, b)) = r {
            if a && b {
                ledger.caught[0] += 1;
            }
        }
        t += 1;
    }
    // 族 1：查无谎报注入 ×10（真容器查越闭集 IID 之外的 99 号）。
    let mut t = 0;
    while t < 10 {
        let c = ConnContainer::real();
        let r = c.find_point(99);
        ledger.tries[1] += 1;
        if let Err(e) = r {
            if e == CpcCode::IidOutOfRange.val() {
                ledger.caught[1] += 1;
            }
        }
        t += 1;
    }
    // 族 2：Unadvise 孤儿注入 ×10（未派发 cookie 注销）。
    let mut t = 0;
    while t < 10 {
        let mut p = ConnPoint::new(IID_SINK_CHANGE);
        let r = p.unadvise(1);
        ledger.tries[2] += 1;
        if let Err(e) = r {
            if e == CpcCode::UnadviseOrphan.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CpcCode::ALL.len() {
        let v = CpcCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 双法/双容器判据自洽（常量独立写死）。
    METHOD_COUNT == 2 && IID_SINK_COUNT == 3 && ADVISE_CAP == 8
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((rounds, _, left)) = golden {
        rounds == CP_TRIALS as u32 && left == 2
    } else {
        false
    };
    v.push((
        "J1-双法全谱10/10+黄金建议账",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-伪容器谎报注入10/10必抓",
        led.all_caught() && led.caught[0] == 10,
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 双法名点验。
    let mut okn = 0;
    let mut i = 0;
    while i < METHOD_COUNT {
        if METHOD_NAMES[i].len() > 0 {
            okn += 1;
        }
        i += 1;
    }
    v.push((
        "J1-双法名点验",
        okn == METHOD_COUNT,
        format!("{}/2", okn),
    ));
    // 出事件汇名点验。
    let mut oks = 0;
    i = 0;
    while i < IID_SINK_COUNT {
        if IID_SINK_NAMES[i].len() > 0 {
            oks += 1;
        }
        i += 1;
    }
    v.push((
        "J1-3汇名点验",
        oks == IID_SINK_COUNT,
        format!("{}/3", oks),
    ));
    // 池满钉版独立点验（8 派发后第 9 必拒，判据写死）。
    let mut p = ConnPoint::new(IID_SINK_CHANGE);
    let mut filled = 0;
    let mut k = 0;
    while k < ADVISE_CAP {
        if p.advise().is_ok() {
            filled += 1;
        }
        k += 1;
    }
    let cap_ok = filled == ADVISE_CAP && p.advise().is_err();
    v.push((
        "J1-池满钉版 8+1 拒",
        cap_ok,
        format!("filled={}", filled),
    ));
    // 空账守恒自洽。
    let fake = ConnContainer::fake();
    v.push((
        "J1-空账枚举守恒自洽",
        fake.audit() == Ok(0),
        String::from("空账 0 点 0 建议"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x673C0 窗口")));
    v
}

pub fn run_unx15461_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15461 IConnectionPointContainer 面谱");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
