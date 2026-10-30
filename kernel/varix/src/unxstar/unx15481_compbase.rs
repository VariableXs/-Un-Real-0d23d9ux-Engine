//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15481 · IStorage/IStream 复合文件基座 · 0x67400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B15）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15481-J1 基座成文（存储/流双元素创建/打开/删除 10/10 次），
//!     悬空元素注入（删除后打开）如实拒断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 元素双类闭集：Storage（子存储容器）/ Stream（字节流），二值
//!     封闭，拒收第三类；
//!   - 元素生命周期状态机：Void → Created → Bound(引用计数) →
//!     Deleted（终态），Deleted 为吸收态，复活非法；
//!   - 三操作闭集：Create/Open/Delete；Create 重名拒、Open 缺名拒、
//!     Delete 缺名拒；Deleted 后 Open = 悬空注入，如实拒；
//!   - 守恒恒等式：存活数 = 创建数 − 删除数；总账数 = 存活 + 删除；
//!   - 悬空注入面 10/10 全抓（双联账 tries/caught）；
//!   - LCG 种子写死 = SEG_BASE（0x67400），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67400，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x67400;

// ── 元素类闭集 ───────────────────────────────────────────────────
pub const EK_STORAGE: usize = 0;
pub const EK_STREAM: usize = 1;
pub const EK_COUNT: usize = 2;
pub const EK_NAMES: [&str; EK_COUNT] = ["存储 Storage", "流 Stream"];

// ── 操作闭集 ─────────────────────────────────────────────────────
pub const OP_CREATE: usize = 0;
pub const OP_OPEN: usize = 1;
pub const OP_DELETE: usize = 2;
pub const OP_COUNT: usize = 3;
pub const OP_NAMES: [&str; OP_COUNT] = ["创建", "打开", "删除"];

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseCode {
    /// 元素类越闭集
    ElementKindOutOfRange,
    /// 操作越闭集
    OperationOutOfRange,
    /// 创建重名（同名元素已存活）
    DuplicateName,
    /// 打开缺名（元素从未创建）
    OpenMissing,
    /// 删除缺名（元素从未创建）
    DeleteMissing,
    /// 悬空元素：删除后打开（如实拒）
    DanglingOpen,
    /// 非法生命周期迁移（复活/双删）
    IllegalTransition,
    /// 守恒恒等式破坏（存活≠创建−删除）
    ConservationBroken,
    /// 悬空注入漏抓
    DanglingLeak,
    /// 元素句柄越界
    HandleOutOfRange,
}

impl BaseCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BaseCode::ElementKindOutOfRange => "元素类越闭集：仅存储/流两类",
            BaseCode::OperationOutOfRange => "操作越闭集：仅创建/打开/删除",
            BaseCode::DuplicateName => "创建重名：同名元素已存活",
            BaseCode::OpenMissing => "打开缺名：元素从未创建",
            BaseCode::DeleteMissing => "删除缺名：元素从未创建",
            BaseCode::DanglingOpen => "悬空元素：删除后打开，如实拒",
            BaseCode::IllegalTransition => "非法生命周期迁移：复活/双删",
            BaseCode::ConservationBroken => "守恒恒等式破坏：存活≠创建−删除",
            BaseCode::DanglingLeak => "悬空注入漏抓：判据红线",
            BaseCode::HandleOutOfRange => "元素句柄越界：先比长度再下标",
        }
    }
    pub const ALL: [BaseCode; 10] = [
        BaseCode::ElementKindOutOfRange,
        BaseCode::OperationOutOfRange,
        BaseCode::DuplicateName,
        BaseCode::OpenMissing,
        BaseCode::DeleteMissing,
        BaseCode::DanglingOpen,
        BaseCode::IllegalTransition,
        BaseCode::ConservationBroken,
        BaseCode::DanglingLeak,
        BaseCode::HandleOutOfRange,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BaseCode::ALL.len() {
        if BaseCode::ALL[i].val() == code {
            return BaseCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67400 段闭集内"
}

// ── 元素生命周期状态机 ───────────────────────────────────────────
/// 元素四态：Void（未建）→ Created → Bound → Deleted（吸收终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElemState {
    Void,
    Created,
    Bound(u32),
    Deleted,
}

/// 合法边闭集：Void→Created（创建）；Created→Bound（打开+1）；
/// Bound(n)→Bound(n+1)（再打开+1）；Bound(1)→Deleted（删除）；
/// Created→Deleted（删除）；其余边（Deleted→任何态、Void→Bound、
/// Void→Deleted）全部非法。
pub fn can_transition(from: ElemState, to: ElemState) -> bool {
    match (from, to) {
        (ElemState::Void, ElemState::Created) => true,
        (ElemState::Created, ElemState::Bound(1)) => true,
        (ElemState::Created, ElemState::Deleted) => true,
        (ElemState::Bound(n), ElemState::Bound(m)) => m == n + 1,
        (ElemState::Bound(1), ElemState::Deleted) => true,
        _ => false,
    }
}

/// 元素记录。
#[derive(Clone, Debug)]
pub struct Elem {
    pub name: String,
    pub kind: usize,
    pub state: ElemState,
    /// 创建序号（单调账）
    pub birth_seq: u64,
    /// 删除序号（0 = 未删）
    pub death_seq: u64,
}

// ── 复合文件基座账 ───────────────────────────────────────────────
pub struct CompBase {
    pub elems: Vec<Elem>,
    pub stat_created: u32,
    pub stat_deleted: u32,
    pub stat_opened: u32,
    pub seq: u64,
}

impl CompBase {
    pub fn new() -> CompBase {
        CompBase {
            elems: Vec::new(),
            stat_created: 0,
            stat_deleted: 0,
            stat_opened: 0,
            seq: 0,
        }
    }

    /// 按名找存活元素下标（先比长度，越界安全）。
    pub fn find_alive(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.elems.len() {
            if self.elems[i].state != ElemState::Deleted
                && self.elems[i].state != ElemState::Void
                && self.elems[i].name == name
            {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 创建：重名（存活）拒；kind 越闭集拒。
    pub fn create(&mut self, name: &str, kind: usize) -> Result<usize, u32> {
        if kind >= EK_COUNT {
            return Err(BaseCode::ElementKindOutOfRange.val());
        }
        if self.find_alive(name).is_some() {
            return Err(BaseCode::DuplicateName.val());
        }
        self.seq += 1;
        let e = Elem {
            name: String::from(name),
            kind,
            state: ElemState::Created,
            birth_seq: self.seq,
            death_seq: 0,
        };
        self.elems.push(e);
        self.stat_created += 1;
        Ok(self.elems.len() - 1)
    }

    /// 打开：缺名拒；Deleted 拒（悬空元素如实拒）；合法迁移 +1。
    pub fn open(&mut self, name: &str) -> Result<usize, u32> {
        match self.find_alive(name) {
            Some(i) => {
                let st = self.elems[i].state;
                let nxt = match st {
                    ElemState::Created => ElemState::Bound(1),
                    ElemState::Bound(n) => ElemState::Bound(n + 1),
                    _ => ElemState::Void,
                };
                if !can_transition(st, nxt) {
                    return Err(BaseCode::IllegalTransition.val());
                }
                self.elems[i].state = nxt;
                self.stat_opened += 1;
                Ok(i)
            }
            None => {
                // Deleted（悬空）与从未创建（缺名）分账：
                // 悬空 = 死亡序号>0；缺名 = 记录不存在。
                let mut dead = false;
                let mut k = 0;
                while k < self.elems.len() {
                    if self.elems[k].name == name && self.elems[k].death_seq > 0 {
                        dead = true;
                    }
                    k += 1;
                }
                if dead {
                    Err(BaseCode::DanglingOpen.val())
                } else {
                    Err(BaseCode::OpenMissing.val())
                }
            }
        }
    }

    /// 删除：缺名拒；Bound(n>1)（仍有打开引用）拒；合法则落 Deleted。
    pub fn delete(&mut self, name: &str) -> Result<(), u32> {
        match self.find_alive(name) {
            Some(i) => {
                let st = self.elems[i].state;
                let allow = match st {
                    ElemState::Created => true,
                    ElemState::Bound(1) => true,
                    _ => false,
                };
                if !allow {
                    return Err(BaseCode::IllegalTransition.val());
                }
                self.seq += 1;
                self.elems[i].state = ElemState::Deleted;
                self.elems[i].death_seq = self.seq;
                self.stat_deleted += 1;
                Ok(())
            }
            None => Err(BaseCode::DeleteMissing.val()),
        }
    }

    /// 守恒对账：存活数 = 创建数 − 删除数；账面记录 = 存活 + 已删。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut alive = 0;
        let mut dead = 0;
        let mut i = 0;
        while i < self.elems.len() {
            match self.elems[i].state {
                ElemState::Deleted => dead += 1,
                ElemState::Void => {}
                _ => alive += 1,
            }
            i += 1;
        }
        if alive as u32 != self.stat_created - self.stat_deleted {
            return Err(BaseCode::ConservationBroken.val());
        }
        if (alive + dead) as u32 != self.stat_created {
            return Err(BaseCode::ConservationBroken.val());
        }
        Ok(())
    }

    /// 判据正演：创建/打开/删除各 10/10 次（存储+流双元素各半）。
    pub fn judge_run(&mut self) -> Result<(u32, u32, u32), u32> {
        let mut ok_create = 0;
        let mut ok_open = 0;
        let mut ok_delete = 0;
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let kind = if r % 2 == 0 { EK_STORAGE } else { EK_STREAM };
            let name = format!("elem{}", r);
            let h = self.create(&name, kind)?;
            if self.elems[h].state == ElemState::Created {
                ok_create += 1;
            }
            self.open(&name)?;
            ok_open += 1;
            self.delete(&name)?;
            ok_delete += 1;
            r += 1;
        }
        Ok((ok_create, ok_open, ok_delete))
    }
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

// ── 黄金正演（判据常量独立写死，不复用被测值） ────────────────────
/// 黄金期望：10 轮全过 → 创建 10、打开 10、删除 10；悬空 10 全拒。
pub const GOLDEN_CREATE: u32 = 10;
pub const GOLDEN_OPEN: u32 = 10;
pub const GOLDEN_DELETE: u32 = 10;
pub const GOLDEN_DANGLING_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut base = CompBase::new();
    let (c, o, d) = base.judge_run()?;
    if c != GOLDEN_CREATE || o != GOLDEN_OPEN || d != GOLDEN_DELETE {
        return Err(BaseCode::ConservationBroken.val());
    }
    base.conserve()?;
    // 悬空注入 10/10：删除后打开必拒。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let name = format!("elem{}", r);
        if let Err(e) = base.open(&name) {
            if e == BaseCode::DanglingOpen.val() {
                caught += 1;
            }
        }
        r += 1;
    }
    if caught != GOLDEN_DANGLING_CAUGHT {
        return Err(BaseCode::DanglingLeak.val());
    }
    Ok((c, o, d, caught))
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
    // 族 0：悬空元素注入（删除后打开）×10，必拒且码=悬空。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut b0 = CompBase::new();
    let _ = b0.create("dangle", EK_STORAGE);
    let _ = b0.delete("dangle");
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if let Err(e) = b0.open("dangle") {
            if e == BaseCode::DanglingOpen.val() {
                ledger.caught[0] += 1;
            }
        }
        r += 1;
    }
    // 族 1：重名注入 ×10，必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut b1 = CompBase::new();
    let _ = b1.create("dup", EK_STREAM);
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if b1.create("dup", EK_STREAM).is_err() {
            ledger.caught[1] += 1;
        }
        r += 1;
    }
    // 族 2：缺名打开注入 ×10，必拒。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut b2 = CompBase::new();
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if let Err(e) = b2.open("ghost") {
            if e == BaseCode::OpenMissing.val() {
                ledger.caught[2] += 1;
            }
        }
        r += 1;
    }
}

// ── 反向判据：合法基座必须全过（防判据恒真） ─────────────────────
/// 若基座连合法三连（建/开/删）都拒，则判据失真——反向探针。
pub fn reverse_probe() -> Result<(), u32> {
    let mut b = CompBase::new();
    let h = b.create("legal", EK_STORAGE)?;
    if b.elems[h].state != ElemState::Created {
        return Err(BaseCode::IllegalTransition.val());
    }
    b.open("legal")?;
    b.delete("legal")?;
    b.conserve()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BaseCode::ALL.len() {
        let v = BaseCode::ALL[i].val();
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
    // 1. 黄金正演：10/10 三连 + 悬空 10/10 拒。
    let g = golden_run();
    v.push((
        "J1-基座成文10/10+悬空10/10拒",
        matches!(g, Ok((10, 10, 10, 10))),
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
    // 3. 生命周期边闭集：非法边（Deleted→Created 复活）必拒。
    let revive_rejected = !can_transition(ElemState::Deleted, ElemState::Created)
        && !can_transition(ElemState::Void, ElemState::Deleted)
        && !can_transition(ElemState::Bound(2), ElemState::Deleted);
    v.push((
        "J1-非法迁移边闭集",
        revive_rejected,
        String::from("复活/虚空删/多引用删 恒拒"),
    ));
    // 4. 守恒恒等式点验：跑一轮后 conserve 必过。
    let conserve_ok = {
        let mut b = CompBase::new();
        let _ = b.judge_run();
        matches!(b.conserve(), Ok(()))
    };
    v.push((
        "J1-守恒恒等式",
        conserve_ok,
        String::from("存活=创建−删除；总账=存活+已删"),
    ));
    // 5. 反向判据：合法三连必须全过（防判据恒真）。
    v.push(("J1-反向探针合法三连", matches!(reverse_probe(), Ok(())), String::from("建/开/删全过")));
    // 6. 元素类/操作名点验。
    v.push((
        "J1-双类三操作名点验",
        EK_NAMES[0].len() > 0 && EK_NAMES[1].len() > 0 && OP_COUNT == 3,
        format!("{}类/{}操作", EK_COUNT, OP_COUNT),
    ));
    // 7. LCG 确定性：同种子双跑一致（反向：异种子不同）。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let det = l1.next() == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 1);
    let nondet = l3.next() != l2.next();
    v.push((
        "J1-LCG确定性+反向",
        det && nondet,
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67400 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15481_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15481 IStorage/IStream 复合文件基座");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
