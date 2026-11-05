//! UNX-F1575 消费域联签汇总口径统一（口径一册） · 0x78EC0段
//!
//! 判据原文（UNX-F1575-J1 摘录）：「联签口径统一册生效（联签状态/结论/保留
//! 三口径逐域统一），口径漂移 10/10 拦截，口径册落档」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三口径闭集：联签状态（绿/黄/红三态）、结论（通过/有保留/否决三态）、
//!    保留（保留项必须挂整改刻）——口径定义全文冻结为常量，域内只许引用；
//! 2. 逐域统一：消费域逐域登记三口径引用，任一口径与册面定义不一致即记
//!    漂移，漂移域点名入报告；
//! 3. 口径漂移 10/10 拦截：十路漂移样本（三口径各路逐字改动）必须全数
//!    被统一门拦下，防口径册退化成「见册就盖通过章」；
//! 4. 口径册落档：逐域一行 + 漂移清单 + 折叠根，漂移未清禁止封册，封册后
//!    册面冻结重放逐字一致；
//! 5. 状态机 拟册→已统一→已封册 单向，倒退显性拒；诊断码闭集
//!    0x78EC1..0x78ECC。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B07 批窗口 0x78E00..=0x78FC0）。
pub const SEG_BASE: u32 = 0x78EC0;

/// 消费域数（六个域逐域统一）。
pub const DOMAIN_COUNT: usize = 6;

/// 漂移注入路数（10/10 拦截）。
pub const DRIFT_ROUTES: usize = 10;

/// 口径一面名：联签状态。
pub const ASPECT_STATUS: &str = "联签状态";
/// 口径二面名：结论。
pub const ASPECT_VERDICT: &str = "结论";
/// 口径三面名：保留。
pub const ASPECT_RESERVE: &str = "保留";

/// 冻结口径定义：联签状态口径（与 F1507 三态账同源）。
pub const CANON_STATUS: &str = "绿/黄/红三态闭合";
/// 冻结口径定义：结论口径。
pub const CANON_VERDICT: &str = "通过/有保留/否决三态闭合";
/// 冻结口径定义：保留口径。
pub const CANON_RESERVE: &str = "保留项挂整改刻入册";

/// 消费域联签口径诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 域名非法（空名或重复）。
    EDomain,
    /// 口径面取值非法。
    EAspect,
    /// 口径漂移：状态口径不一致。
    EDriftStatus,
    /// 口径漂移：结论口径不一致。
    EDriftVerdict,
    /// 口径漂移：保留口径不一致。
    EDriftReserve,
    /// 漂移未清禁止封册。
    ERedBlock,
    /// 册已封。
    ESealed,
    /// 重复发起统一。
    EDupUnify,
    /// 空册样本。
    EEmpty,
    /// 口径册缺失。
    ENoBook,
    /// 册重放不符。
    EReplay,
    /// 相位倒退。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EDomain => 0x01,
                Diag::EAspect => 0x02,
                Diag::EDriftStatus => 0x03,
                Diag::EDriftVerdict => 0x04,
                Diag::EDriftReserve => 0x05,
                Diag::ERedBlock => 0x06,
                Diag::ESealed => 0x07,
                Diag::EDupUnify => 0x08,
                Diag::EEmpty => 0x09,
                Diag::ENoBook => 0x0a,
                Diag::EReplay => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EDomain => "域名非法：消费域按册面名单逐域登记，空名重名拒收",
            Diag::EAspect => "口径面取值非法：只认联签状态/结论/保留三面",
            Diag::EDriftStatus => "状态口径漂移：改回「绿/黄/红三态闭合」册面定义",
            Diag::EDriftVerdict => "结论口径漂移：改回「通过/有保留/否决三态闭合」册面定义",
            Diag::EDriftReserve => "保留口径漂移：改回「保留项挂整改刻入册」册面定义",
            Diag::ERedBlock => "漂移未清：存在漂移域禁止封册，先逐域改齐",
            Diag::ESealed => "册已封：封册后写入拒绝，改动走下一批口径册",
            Diag::EDupUnify => "统一已发起：重复发起拒绝，先取回上一轮册面",
            Diag::EEmpty => "空册样本：无域不构成口径册，先逐域登记",
            Diag::ENoBook => "口径册缺失：封册前必须生成册面",
            Diag::EReplay => "册重放不符：封册后册面被改动，须复核",
            Diag::EPhase => "相位倒退：拟册→已统一→已封册 单向，倒退即拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EDomain,
    Diag::EAspect,
    Diag::EDriftStatus,
    Diag::EDriftVerdict,
    Diag::EDriftReserve,
    Diag::ERedBlock,
    Diag::ESealed,
    Diag::EDupUnify,
    Diag::EEmpty,
    Diag::ENoBook,
    Diag::EReplay,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78EC1..0x78ECC 闭集内"
}

/// 口径三面闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aspect {
    /// 联签状态口径。
    Status,
    /// 结论口径。
    Verdict,
    /// 保留口径。
    Reserve,
}

impl Aspect {
    /// 面名。
    pub fn name(self) -> &'static str {
        match self {
            Aspect::Status => ASPECT_STATUS,
            Aspect::Verdict => ASPECT_VERDICT,
            Aspect::Reserve => ASPECT_RESERVE,
        }
    }

    /// 册面冻结定义。
    pub fn canon(self) -> &'static str {
        match self {
            Aspect::Status => CANON_STATUS,
            Aspect::Verdict => CANON_VERDICT,
            Aspect::Reserve => CANON_RESERVE,
        }
    }

    /// 对应漂移诊断码。
    pub fn drift_diag(self) -> Diag {
        match self {
            Aspect::Status => Diag::EDriftStatus,
            Aspect::Verdict => Diag::EDriftVerdict,
            Aspect::Reserve => Diag::EDriftReserve,
        }
    }
}

/// 三面闭集数组。
pub const ASPECT_ALL: &[Aspect] = &[Aspect::Status, Aspect::Verdict, Aspect::Reserve];

/// 漂移面名拼接（"面1+面2"），不用切片 join（no_std 下避免依赖 prelude）。
fn join_aspect_names(aspects: &[Aspect]) -> String {
    let mut out = String::new();
    let mut k = 0usize;
    for a in aspects {
        if k > 0 {
            out.push('+');
        }
        out.push_str(a.name());
        k += 1;
    }
    out
}

/// 单域口径登记：三面各自引用册面定义。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlossEntry {
    /// 域名。
    pub domain: &'static str,
    /// 状态口径引用。
    pub status_def: &'static str,
    /// 结论口径引用。
    pub verdict_def: &'static str,
    /// 保留口径引用。
    pub reserve_def: &'static str,
}

impl GlossEntry {
    /// 该域三口径是否与册面定义逐字一致。
    pub fn unified(&self) -> bool {
        self.status_def == CANON_STATUS
            && self.verdict_def == CANON_VERDICT
            && self.reserve_def == CANON_RESERVE
    }

    /// 漂移面清单（统一则空）。
    pub fn drift_aspects(&self) -> Vec<Aspect> {
        let mut out = Vec::new();
        if self.status_def != CANON_STATUS {
            out.push(Aspect::Status);
        }
        if self.verdict_def != CANON_VERDICT {
            out.push(Aspect::Verdict);
        }
        if self.reserve_def != CANON_RESERVE {
            out.push(Aspect::Reserve);
        }
        out
    }
}

/// 册相位（拟册→已统一→已封册 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 拟册：逐域登记中。
    Draft,
    /// 已统一：统一已发起且册面已生成。
    Unified,
    /// 已封册：册面冻结、写入拒绝。
    Sealed,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Draft => 0,
            Phase::Unified => 1,
            Phase::Sealed => 2,
        }
    }
}

/// 口径册：逐域登记 + 统一 + 封册。
#[derive(Clone, Debug)]
pub struct GlossBook {
    phase: Phase,
    entries: Vec<GlossEntry>,
    /// 逐域册面行。
    lines: Vec<String>,
    /// 漂移域清单。
    drifts: Vec<&'static str>,
    /// 折叠根。
    root: u64,
    rejects: u32,
}

impl GlossBook {
    /// 新册。
    pub fn new() -> GlossBook {
        GlossBook {
            phase: Phase::Draft,
            entries: Vec::new(),
            lines: Vec::new(),
            drifts: Vec::new(),
            root: 0,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已登记域数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 登记一域（域名非空且不重复）。
    pub fn put(&mut self, e: GlossEntry) -> Result<(), u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if e.domain.is_empty() {
            self.rejects += 1;
            return Err(Diag::EDomain.val());
        }
        for x in &self.entries {
            if x.domain == e.domain {
                self.rejects += 1;
                return Err(Diag::EDomain.val());
            }
        }
        self.entries.push(e);
        Ok(())
    }

    /// 按域名取登记。
    pub fn entry_of(&self, domain: &str) -> Option<&GlossEntry> {
        self.entries.iter().find(|x| x.domain == domain)
    }

    /// 册面行只读。
    pub fn line_at(&self, k: usize) -> Option<&str> {
        self.lines.get(k).map(|s| s.as_str())
    }

    /// 漂移域清单。
    pub fn drifts(&self) -> &[&'static str] {
        &self.drifts
    }

    /// 折叠根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 发起统一：逐域对册面定义对平，生成册面行与漂移清单。
    pub fn unify(&mut self) -> Result<usize, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.phase == Phase::Unified {
            return Err(Diag::EDupUnify.val());
        }
        if self.entries.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        let mut lines = Vec::new();
        let mut drifts = Vec::new();
        let mut h: u64 = 0xCBF29CE484222325;
        for e in &self.entries {
            let line = if e.unified() {
                format!("域{} 三口径统一", e.domain)
            } else {
                drifts.push(e.domain);
                format!("域{} 漂移{}", e.domain, join_aspect_names(&e.drift_aspects()))
            };
            for b in line.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001B3);
            }
            lines.push(line);
        }
        self.lines = lines;
        self.drifts = drifts;
        self.root = h;
        self.phase = Phase::Unified;
        Ok(self.drifts.len())
    }

    /// 封册：漂移必须已清零，册面必须已生成。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.phase != Phase::Unified {
            return Err(Diag::EPhase.val());
        }
        if !self.drifts.is_empty() {
            return Err(Diag::ERedBlock.val());
        }
        if self.lines.is_empty() || self.root == 0 {
            return Err(Diag::ENoBook.val());
        }
        self.phase = Phase::Sealed;
        Ok(self.root)
    }

    /// 封册后重放：逐域复对平 + 根复算逐字一致。
    pub fn replay(&self) -> Result<u32, u32> {
        if self.phase != Phase::Sealed {
            return Err(Diag::EPhase.val());
        }
        let mut h: u64 = 0xCBF29CE484222325;
        for e in &self.entries {
            let line = if e.unified() {
                format!("域{} 三口径统一", e.domain)
            } else {
                format!("域{} 漂移{}", e.domain, join_aspect_names(&e.drift_aspects()))
            };
            for b in line.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001B3);
            }
        }
        if h != self.root {
            return Err(Diag::EReplay.val());
        }
        Ok(self.lines.len() as u32)
    }

    /// 册面全文（落档面）。
    pub fn text(&self) -> String {
        let mut out = String::new();
        for l in &self.lines {
            out.push_str(l);
            out.push('\n');
        }
        out
    }
}

/// 消费域名册面名单（六域，判据面钦定）。
pub const DOMAIN_NAMES: [&str; DOMAIN_COUNT] = ["计费", "退款", "对账", "开票", "积分", "客服"];

/// 标准册：六域全部引用册面冻结定义（逐域统一的基线批）。
pub fn build_book() -> GlossBook {
    let mut b = GlossBook::new();
    let mut i = 0usize;
    while i < DOMAIN_COUNT {
        let name = match DOMAIN_NAMES.get(i) {
            Some(x) => *x,
            None => break,
        };
        let _ = b.put(GlossEntry {
            domain: name,
            status_def: CANON_STATUS,
            verdict_def: CANON_VERDICT,
            reserve_def: CANON_RESERVE,
        });
        i += 1;
    }
    b
}

/// 口径漂移注入 10/10：十路漂移样本（三面逐字改动轮转）必须全数拦截。
/// 返回十布尔（true=该路漂移被统一门拦下且封册被拒）。
pub fn drift_matrix() -> [bool; DRIFT_ROUTES] {
    let mut out = [false; DRIFT_ROUTES];
    let mut i = 0usize;
    while i < DRIFT_ROUTES {
        // 路序决定被篡改的域与面：域 = i % 6，面 = i % 3。
        let dom_idx = i % DOMAIN_COUNT;
        let aspect_idx = i % ASPECT_ALL.len();
        let dom = match DOMAIN_NAMES.get(dom_idx) {
            Some(x) => *x,
            None => break,
        };
        let mut probe = GlossBook::new();
        let mut k = 0usize;
        while k < DOMAIN_COUNT {
            let name = match DOMAIN_NAMES.get(k) {
                Some(x) => *x,
                None => break,
            };
            let (sd, vd, rd) = if k == dom_idx {
                // 只篡改一个面，其余两面照抄册面。
                let a = match ASPECT_ALL.get(aspect_idx) {
                    Some(x) => *x,
                    None => break,
                };
                let bad = "本地口径自制";
                match a {
                    Aspect::Status => (bad, CANON_VERDICT, CANON_RESERVE),
                    Aspect::Verdict => (CANON_STATUS, bad, CANON_RESERVE),
                    Aspect::Reserve => (CANON_STATUS, CANON_VERDICT, bad),
                }
            } else {
                (CANON_STATUS, CANON_VERDICT, CANON_RESERVE)
            };
            let _ = probe.put(GlossEntry { domain: name, status_def: sd, verdict_def: vd, reserve_def: rd });
            k += 1;
        }
        // 统一门必须点出恰好一个漂移域。
        let caught = match probe.unify() {
            Ok(n) => n == 1,
            Err(_) => false,
        };
        // 封册门：漂移未清必须拒绝（第二道闸）。
        let seal_blocked = probe.seal() == Err(Diag::ERedBlock.val());
        let cell = match out.get_mut(i) {
            Some(x) => x,
            None => break,
        };
        *cell = caught && seal_blocked && probe.entry_of(dom).map(|e| !e.unified()).unwrap_or(false);
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B07 派工卡 0x78E00..=0x78FC0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x78E00 || SEG_BASE > 0x78FC0 {
        return false;
    }
    SEG_BASE == 0x78EC0
}

/// 三面闭集（互异且册面定义互异）。
fn aspect_closed_ok() -> bool {
    if ASPECT_ALL.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < ASPECT_ALL.len() {
        let a = match ASPECT_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < ASPECT_ALL.len() {
            let b = match ASPECT_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b || a.canon() == b.canon() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 基线册六域逐域统一。
fn baseline_ok(b: &GlossBook) -> bool {
    if b.entry_count() != DOMAIN_COUNT {
        return false;
    }
    for name in DOMAIN_NAMES {
        match b.entry_of(name) {
            Some(e) if e.unified() => {}
            _ => return false,
        }
    }
    true
}

/// 口径册落档（封册 + 重放一致 + 册面非空）。
fn book_archived_ok(b: &GlossBook) -> bool {
    b.phase() == Phase::Sealed
        && b.replay() == Ok(DOMAIN_COUNT as u32)
        && b.text().contains("三口径统一")
}

/// 漂移 10/10 拦截。
fn drift_ok() -> bool {
    drift_matrix() == [true; DRIFT_ROUTES]
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_book();
    let b = build_book();
    let mut x = a;
    let mut y = b;
    match (x.unify(), y.unify()) {
        (Ok(n1), Ok(n2)) => n1 == n2 && n1 == 0 && x.root() == y.root() && x.root() != 0,
        _ => false,
    }
}

/// 十三项判据：UNX-F1575 口径一册。
pub fn run_unx1575_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1575");
    let mut b = build_book();

    s.add_dyn(
        "F1575-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1575-02 三口径面闭集",
        aspect_closed_ok(),
        &format!(
            "n={} faces={}/{}/{}",
            ASPECT_ALL.len(),
            Aspect::Status.name(),
            Aspect::Verdict.name(),
            Aspect::Reserve.name()
        ),
    );

    s.add_dyn(
        "F1575-03 基线册六域齐备",
        baseline_ok(&b) && b.entry_count() == DOMAIN_COUNT,
        &format!("domains={}", b.entry_count()),
    );

    // 结构拒绝：空域名、重名域、空册统一。
    let mut r = GlossBook::new();
    let empty_name = r.put(GlossEntry { domain: "", status_def: CANON_STATUS, verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE }) == Err(Diag::EDomain.val());
    let _ = r.put(GlossEntry { domain: "计费", status_def: CANON_STATUS, verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE });
    let dup = r.put(GlossEntry { domain: "计费", status_def: CANON_STATUS, verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE }) == Err(Diag::EDomain.val());
    let empty_book = GlossBook::new().unify() == Err(Diag::EEmpty.val());
    s.add_dyn(
        "F1575-04 结构拒绝三门",
        empty_name && dup && empty_book,
        &format!("rejects={}", r.rejects()),
    );

    // 统一发起：基线零漂移、重复发起拒绝、相位推进。
    let n = b.unify();
    let n_ok = match n {
        Ok(x) => x == 0,
        Err(_) => false,
    };
    let dup = b.unify() == Err(Diag::EDupUnify.val());
    s.add_dyn(
        "F1575-05 统一发起单向零漂移",
        n_ok && dup && b.phase() == Phase::Unified,
        &format!("phase_ord={} drifts={}", b.phase().ord(), b.drifts().len()),
    );

    // 口径漂移 10/10 拦截。
    s.add_dyn(
        "F1575-06 口径漂移十路全拦截",
        drift_ok(),
        &format!("matrix={:?}", drift_matrix()),
    );

    // 漂移册面点名域与面。
    let mut q = GlossBook::new();
    let _ = q.put(GlossEntry { domain: "计费", status_def: "本地口径自制", verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE });
    let _ = q.put(GlossEntry { domain: "退款", status_def: CANON_STATUS, verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE });
    let qn = q.unify();
    let q_ok = match &qn {
        Ok(x) => {
            *x == 1
                && q.line_at(0).map(|t| t.contains("计费") && t.contains(ASPECT_STATUS)).unwrap_or(false)
                && q.drifts() == ["计费"]
        }
        Err(_) => false,
    };
    s.add_dyn(
        "F1575-07 漂移册面点名域与面",
        q_ok,
        &match &qn {
            Ok(_) => q.line_at(0).map(|t| t.to_string()).unwrap_or_default(),
            Err(c) => detail(*c).to_string(),
        },
    );

    // 封册闸门：漂移未清拒绝；清零放行；封册后写入拒绝。
    let q_seal = q.seal() == Err(Diag::ERedBlock.val());
    let seal = b.seal();
    let seal_ok = match seal {
        Ok(root) => root != 0,
        Err(_) => false,
    };
    let write_after = b.put(GlossEntry { domain: "补域", status_def: CANON_STATUS, verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE }) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1575-08 封册闸门三向",
        q_seal && seal_ok && write_after && b.phase() == Phase::Sealed,
        &format!("q_seal={} seal_ok={}", q_seal, seal_ok),
    );

    // 封册重放逐字一致。
    s.add_dyn(
        "F1575-09 封册重放逐字一致",
        book_archived_ok(&b),
        &format!("replay={:?}", b.replay()),
    );

    // 口径册落档面：册面含逐域行。
    let text = b.text();
    s.add_dyn(
        "F1575-10 口径册落档非空",
        text.contains("域计费") && text.contains("域客服") && text.contains("三口径统一"),
        &format!("bytes={}", text.len()),
    );

    // 册面冻结定义逐字守卫（防常量被改）。
    let canon_ok = CANON_STATUS == "绿/黄/红三态闭合"
        && CANON_VERDICT == "通过/有保留/否决三态闭合"
        && CANON_RESERVE == "保留项挂整改刻入册";
    s.add_dyn(
        "F1575-11 册面冻结定义守卫",
        canon_ok,
        &format!("{}/{}{}", CANON_STATUS, CANON_VERDICT, CANON_RESERVE),
    );

    // 相位倒退显性拒。
    let back = b.unify() == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1575-12 相位倒退显性拒",
        back && b.phase().ord() == 2,
        &format!("phase_ord={}", b.phase().ord()),
    );

    s.add_dyn(
        "F1575-13 双跑一致与闭集守卫",
        dual_run_ok() && ALL.len() == 12,
        &format!("all={} drifts_total={}", ALL.len(), DRIFT_ROUTES),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_closed() {
        assert_eq!(ASPECT_ALL.len(), 3);
        assert_ne!(Aspect::Status, Aspect::Verdict);
        assert_ne!(Aspect::Verdict, Aspect::Reserve);
        assert_ne!(Aspect::Status.canon(), Aspect::Verdict.canon());
    }

    #[test]
    fn entry_unified_and_drift_aspects() {
        let ok = GlossEntry { domain: "计费", status_def: CANON_STATUS, verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE };
        assert!(ok.unified());
        assert!(ok.drift_aspects().is_empty());
        let bad = GlossEntry { domain: "退款", status_def: "自制", verdict_def: CANON_VERDICT, reserve_def: "另册" };
        assert!(!bad.unified());
        let d = bad.drift_aspects();
        assert_eq!(d.len(), 2);
        assert_eq!(d.get(0), Some(&Aspect::Status));
        assert_eq!(d.get(1), Some(&Aspect::Reserve));
    }

    #[test]
    fn book_gates() {
        let mut b = GlossBook::new();
        assert_eq!(b.unify(), Err(Diag::EEmpty.val()));
        let e = GlossEntry { domain: "", status_def: CANON_STATUS, verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE };
        assert_eq!(b.put(e), Err(Diag::EDomain.val()));
        let _ = b.put(GlossEntry { domain: "计费", status_def: CANON_STATUS, verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE });
        assert_eq!(b.put(GlossEntry { domain: "计费", status_def: CANON_STATUS, verdict_def: CANON_VERDICT, reserve_def: CANON_RESERVE }), Err(Diag::EDomain.val()));
        assert_eq!(b.rejects(), 2);
    }

    #[test]
    fn baseline_unify_zero_drift() {
        let mut b = build_book();
        assert_eq!(b.entry_count(), DOMAIN_COUNT);
        let n = b.unify().expect("基线统一");
        assert_eq!(n, 0);
        assert_eq!(b.unify(), Err(Diag::EDupUnify.val()));
        let root = b.seal().expect("零漂移封册");
        assert_ne!(root, 0);
        assert_eq!(b.replay(), Ok(DOMAIN_COUNT as u32));
    }

    #[test]
    fn drift_named_in_line() {
        let mut q = GlossBook::new();
        let _ = q.put(GlossEntry { domain: "开票", status_def: CANON_STATUS, verdict_def: "自定", reserve_def: CANON_RESERVE });
        let n = q.unify().expect("统一可发起");
        assert_eq!(n, 1);
        assert_eq!(q.drifts(), ["开票"]);
        let line = q.line_at(0).expect("行存在");
        assert!(line.contains("开票") && line.contains(ASPECT_VERDICT));
        assert_eq!(q.seal(), Err(Diag::ERedBlock.val()));
    }

    #[test]
    fn drift_ten_of_ten() {
        let m = drift_matrix();
        assert_eq!(m, [true; DRIFT_ROUTES]);
        assert!(drift_ok());
    }

    #[test]
    fn canon_strings_frozen() {
        assert_eq!(CANON_STATUS, "绿/黄/红三态闭合");
        assert_eq!(CANON_VERDICT, "通过/有保留/否决三态闭合");
        assert_eq!(CANON_RESERVE, "保留项挂整改刻入册");
        assert_eq!(DOMAIN_NAMES.len(), DOMAIN_COUNT);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EDomain.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x50), "未知诊断码：不在 0x78EC1..0x78ECC 闭集内");
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1575_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
