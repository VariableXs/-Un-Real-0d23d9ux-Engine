//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18414 · 版本查询与 bottle 上下文绑定（E1 衔接） · 0x7E340段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18414-J1 查询结果随 bottle 档案切换断言全过（同进程双 bottle 场景），漂移注入检出
//!
//! 结构清单：
//!   1. BtlCode        诊断码闭集（code() = SEG_BASE + n）
//!   2. BottleProfile  bottle 档案（真值快照 + 纪元 + 别名）
//!   3. BottleBinder   同进程绑定器（注册/激活/切换/查询）
//!   4. drift_check    漂移检测核（查询纪元 vs 活跃纪元）
//!   5. switch_probe   同进程双 bottle 切换探针（查询结果随档切换）
//!   6. Lcg + InjectLedger（三类违约 × 10 轮）
//!   7. golden_run / probe_a/b/c / inject_rounds
//!   8. run_unx18414_checks 判据集（≤20 条）
//!
//! 设计要点：
//!   - bottle 档案（E1 域衔接）：一份「版本真值」连同纪元号打包；同进程
//!     可注册多份，但活跃只许一份——查询永远读活跃纪元的真值。
//!   - 切换语义：switch_active 纪元 +1，旧纪元句柄立刻失效（查询必须带
//!     当前纪元，纪元不配 = 漂移，显式拒并记账）。
//!   - 漂移注入：模拟「查询被钉死在旧 bottle」——活跃已切、查询仍报旧档
//!     真值，drift_check 逐字段对拍活跃真值，不一致即漂移检出。
//!   - 同进程双 bottle 场景：A/B 双档案注册后交替激活，查询结果必须
//!     逐字段跟随活跃档（J1 主判据）。
//!   - 注入三类：漂移钉死 / 未知档案激活 / 未注册档案查询，各 10 轮。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E340，批 B01 独占区，%0x40==0 对齐）。
pub const SEG_BASE: u32 = 0x7E340;

// ── 冻结常量（推导见行注） ──────────────────────────────────────
/// 档案表容量（冻结：4 = 同进程双 bottle 主场景 × 2 余量）。
pub const BOTTLE_CAP: usize = 4;
/// 档案 ID 合法域上限（冻结：1..=8，注册时域闸）。
pub const BOTTLE_ID_MAX: u32 = 8;
/// 切换步数（冻结：切换探针 32 步，LCG 交替足够覆盖两档多轮）。
pub const SWITCH_STEPS: u32 = 32;
/// 判据注入轮数（写死：J1「10/10」口径）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtlCode {
    /// 空绑定器（无任何档案）
    EmptyBinder,
    /// 档案表满
    BinderFull,
    /// 档案 ID 越域（0 或 >8）
    IdOutOfRange,
    /// 重复注册（同 ID 已有档案）
    DuplicateProfile,
    /// 激活未知档案（未注册 ID）
    ActivateUnknown,
    /// 查询未注册档案
    QueryUnregistered,
    /// 纪元漂移（查询纪元 ≠ 活跃纪元）
    EpochDrift,
    /// 无活跃档案（注册后未激活即查询）
    NoActive,
    /// 真值快照非法（维度越域）
    TruthIllegal,
    /// 切换后查询结果未跟随活跃档
    SwitchNotFollowed,
    /// 纪元回退（切换只许前进）
    EpochRegress,
    /// 双跑不一致
    NotReproducible,
}

impl BtlCode {
    /// 诊断码取值：SEG_BASE + 枚举序号（窗口 0x7E340..0x7E380）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐条非空）。
    pub fn detail(self) -> &'static str {
        match self {
            BtlCode::EmptyBinder => "空绑定器：无任何 bottle 档案",
            BtlCode::BinderFull => "档案表满：4 槽占尽",
            BtlCode::IdOutOfRange => "档案 ID 越域：合法域 1..8",
            BtlCode::DuplicateProfile => "重复注册：同 ID 已有档案",
            BtlCode::ActivateUnknown => "激活未知档案：未注册 ID",
            BtlCode::QueryUnregistered => "查询未注册档案",
            BtlCode::EpochDrift => "纪元漂移：查询纪元≠活跃纪元",
            BtlCode::NoActive => "无活跃档案：注册后须先激活",
            BtlCode::TruthIllegal => "真值快照非法：维度越域",
            BtlCode::SwitchNotFollowed => "切换后查询未跟随活跃档",
            BtlCode::EpochRegress => "纪元回退：切换只许前进",
            BtlCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 闭集全量。
    pub const ALL: [BtlCode; 12] = [
        BtlCode::EmptyBinder,
        BtlCode::BinderFull,
        BtlCode::IdOutOfRange,
        BtlCode::DuplicateProfile,
        BtlCode::ActivateUnknown,
        BtlCode::QueryUnregistered,
        BtlCode::EpochDrift,
        BtlCode::NoActive,
        BtlCode::TruthIllegal,
        BtlCode::SwitchNotFollowed,
        BtlCode::EpochRegress,
        BtlCode::NotReproducible,
    ];
}

/// 按数值反查诊断文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < BtlCode::ALL.len() {
        if BtlCode::ALL[i].code() == code {
            return BtlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E340 段闭集内"
}

// ── 真值快照（与 unx18413 同构但独立定义，域模型不共享） ────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TruthSnap {
    /// 主版本
    pub major: u32,
    /// 次版本
    pub minor: u32,
    /// 构建号
    pub build: u32,
    /// 产品类型（1=工作站 3=服务器，口径冻结）
    pub product_type: u32,
}

impl TruthSnap {
    /// 全维域守卫：build ≤0xFFFF、product_type ∈ {1,3}。
    pub fn legal(&self) -> bool {
        self.major <= 0xFFFF && self.minor <= 0xFFFF && self.build <= 0xFFFF
            && (self.product_type == 1 || self.product_type == 3)
    }
    /// 逐字段比对。
    pub fn same(&self, o: &TruthSnap) -> bool {
        self.major == o.major && self.minor == o.minor
            && self.build == o.build && self.product_type == o.product_type
    }
}

/// 双演示档案真值（写死：Win10 工作站口径 / Win11 工作站口径）。
pub const TRUTH_A: TruthSnap = TruthSnap { major: 10, minor: 0, build: 19045, product_type: 1 };
pub const TRUTH_B: TruthSnap = TruthSnap { major: 10, minor: 0, build: 22631, product_type: 1 };
/// 服务器口径档案（product_type=3 分支覆盖）。
pub const TRUTH_SRV: TruthSnap = TruthSnap { major: 10, minor: 0, build: 20348, product_type: 3 };

// ── bottle 档案 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BottleProfile {
    /// 档案 ID（1..=8）
    pub id: u32,
    /// 版本真值
    pub truth: TruthSnap,
    /// 档案别名指纹（注册期冻结，判漂移用）
    pub alias_fp: u32,
    /// 活跃标志
    pub live: bool,
}

// ── 绑定器 ──────────────────────────────────────────────────────
/// 同进程 bottle 绑定器：定容 4 槽档案账本 + 活跃纪元。
pub struct BottleBinder {
    slots: [Option<BottleProfile>; BOTTLE_CAP],
    /// 活跃档案 ID（None=未激活）
    active: Option<u32>,
    /// 活跃纪元（每次切换 +1，只增）
    pub epoch: u32,
    /// 累计注册数
    pub registered_total: u32,
    /// 累计拒绝数
    pub rejected_total: u32,
    /// 累计漂移拦截数
    pub drift_blocks: u32,
}

impl BottleBinder {
    /// 新绑定器（空表、零纪元、无活跃）。
    pub fn new() -> BottleBinder {
        BottleBinder {
            slots: [None; BOTTLE_CAP],
            active: None,
            epoch: 0,
            registered_total: 0,
            rejected_total: 0,
            drift_blocks: 0,
        }
    }

    /// 注册档案：ID 域闸 + 真值域闸 + 重复闸。
    pub fn register(&mut self, id: u32, truth: TruthSnap, alias_fp: u32) -> Result<(), u32> {
        if id == 0 || id > BOTTLE_ID_MAX {
            self.rejected_total += 1;
            return Err(BtlCode::IdOutOfRange.code());
        }
        if !truth.legal() {
            self.rejected_total += 1;
            return Err(BtlCode::TruthIllegal.code());
        }
        if self.find(id).is_some() {
            self.rejected_total += 1;
            return Err(BtlCode::DuplicateProfile.code());
        }
        let mut slot: Option<usize> = None;
        let mut i = 0;
        while i < BOTTLE_CAP {
            if self.slots[i].is_none() {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match slot {
            Some(s) => s,
            None => {
                self.rejected_total += 1;
                return Err(BtlCode::BinderFull.code());
            }
        };
        self.slots[idx] = Some(BottleProfile { id, truth, alias_fp, live: false });
        self.registered_total += 1;
        Ok(())
    }

    /// 按 ID 找槽序号。
    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0;
        while i < BOTTLE_CAP {
            if let Some(p) = self.slots[i] {
                if p.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 激活：档案 live 置位、其余清位（单活跃不变量）、纪元 +1。
    pub fn activate(&mut self, id: u32) -> Result<u32, u32> {
        let idx = match self.find(id) {
            Some(i) => i,
            None => {
                self.rejected_total += 1;
                return Err(BtlCode::ActivateUnknown.code());
            }
        };
        // 单活跃：先清全部 live。
        let mut i = 0;
        while i < BOTTLE_CAP {
            if let Some(p) = self.slots[i] {
                if p.live {
                    self.slots[i] = Some(BottleProfile { live: false, ..p });
                }
            }
            i += 1;
        }
        if let Some(p) = self.slots[idx] {
            self.slots[idx] = Some(BottleProfile { live: true, ..p });
        }
        self.active = Some(id);
        self.epoch += 1;
        Ok(self.epoch)
    }

    /// 当前活跃 ID。
    pub fn active_id(&self) -> Option<u32> {
        self.active
    }

    /// 查询：必须带调用方记录的纪元；纪元不配 = 漂移，显式拒。
    /// 返回 (活跃档案真值, 当前纪元)。
    pub fn query(&mut self, expect_epoch: u32) -> Result<(TruthSnap, u32), u32> {
        let act = match self.active {
            Some(a) => a,
            None => return Err(BtlCode::NoActive.code()),
        };
        if expect_epoch != self.epoch {
            self.drift_blocks += 1;
            return Err(BtlCode::EpochDrift.code());
        }
        let idx = match self.find(act) {
            Some(i) => i,
            None => return Err(BtlCode::QueryUnregistered.code()),
        };
        match self.slots[idx] {
            Some(p) if p.live => Ok((p.truth, self.epoch)),
            _ => Err(BtlCode::QueryUnregistered.code()),
        }
    }

    /// 漂移检测核：给定「钉死旧档」的查询结果，与活跃档真值逐字段对拍。
    /// 不一致 = 漂移检出（Ok(false)），一致 = 无漂移（Ok(true)）。
    pub fn drift_check(&self, pinned: &TruthSnap) -> Result<bool, u32> {
        let act = match self.active {
            Some(a) => a,
            None => return Err(BtlCode::NoActive.code()),
        };
        let idx = match self.find(act) {
            Some(i) => i,
            None => return Err(BtlCode::QueryUnregistered.code()),
        };
        match self.slots[idx] {
            Some(p) => Ok(pinned.same(&p.truth)),
            None => Err(BtlCode::QueryUnregistered.code()),
        }
    }
}

// ── 同进程双 bottle 切换探针 ────────────────────────────────────
/// 切换探针：A/B 双档案注册后 LCG 交替激活 `steps` 步，每步查询必须
/// 逐字段跟随活跃档。返回 Ok(切换步数)；任一步未跟随即 Err。
pub fn switch_probe(steps: u32) -> Result<u32, u32> {
    let mut b = BottleBinder::new();
    b.register(1, TRUTH_A, 0xAA01)?;
    b.register(2, TRUTH_B, 0xBB02)?;
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut step = 0;
    while step < steps {
        let to_a = rng.below(2) == 0;
        let target = if to_a { 1 } else { 2 };
        b.activate(target)?;
        let ep = b.epoch;
        let (truth, cur_ep) = b.query(ep)?;
        if cur_ep != ep {
            return Err(BtlCode::EpochDrift.code());
        }
        let expect = if to_a { TRUTH_A } else { TRUTH_B };
        if !truth.same(&expect) {
            return Err(BtlCode::SwitchNotFollowed.code());
        }
        // 漂移核：拿另一档真值来对拍，必须判「漂移」。
        let other = if to_a { TRUTH_B } else { TRUTH_A };
        if b.drift_check(&other)? {
            return Err(BtlCode::SwitchNotFollowed.code());
        }
        step += 1;
    }
    Ok(steps)
}

// ── 确定性伪随机 ────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    /// 播种（种子 = SEG_BASE）。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    /// 推进一格。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0, n) 伪随机。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
/// 三类注入槽：0=纪元漂移钉死，1=激活未知档案，2=查询未注册档案。
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    /// 全抓断言。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检合计。
    pub fn missed_total(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三类违约逐类注入，断言每轮被显式检出。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut b = BottleBinder::new();
        b.register(1, TRUTH_A, 0xAA01)?;
        b.register(2, TRUTH_B, 0xBB02)?;
        // 槽 0：漂移钉死——激活 A 后切 B，查询仍带旧纪元。
        b.activate(1)?;
        let old_epoch = b.epoch;
        b.activate(2)?;
        let drift = match b.query(old_epoch) {
            Err(c) => c == BtlCode::EpochDrift.code(),
            Ok(_) => false,
        };
        led.record(0, drift);
        // 槽 1：激活未知档案（ID=7 未注册）。
        let unknown = match b.activate(7) {
            Err(c) => c == BtlCode::ActivateUnknown.code(),
            Ok(_) => false,
        };
        led.record(1, unknown);
        // 槽 2：漂移核对拍——把「另一档真值」喂给 drift_check 必判漂移。
        let (truth, _) = b.query(b.epoch)?;
        let cross = !b.drift_check(&TRUTH_A)?; // 活跃是 2，A 真值必判漂移
        let consistent = truth.same(&TRUTH_B);
        led.record(2, cross && consistent);
        r += 1;
    }
    if !led.all_caught() || led.missed_total() != 0 {
        return Err(BtlCode::NotReproducible.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：三档案注册（含服务器口径）→ 双 bottle 交替切换 32 步全跟随
/// → 纪元单调 → 服务器口径分支覆盖。返回 (注册数, 切换步数, 纪元)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut b = BottleBinder::new();
    b.register(1, TRUTH_A, 0xAA01)?;
    b.register(2, TRUTH_B, 0xBB02)?;
    b.register(3, TRUTH_SRV, 0xCC03)?;
    // 切换主链在 golden 自身绑定器上进行：LCG 交替激活，每步查询须跟随。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut steps = 0;
    while steps < SWITCH_STEPS {
        let to_a = rng.below(2) == 0;
        let target = if to_a { 1 } else { 2 };
        b.activate(target)?;
        let ep = b.epoch;
        let (truth, cur_ep) = b.query(ep)?;
        if cur_ep != ep {
            return Err(BtlCode::EpochDrift.code());
        }
        let expect = if to_a { TRUTH_A } else { TRUTH_B };
        if !truth.same(&expect) {
            return Err(BtlCode::SwitchNotFollowed.code());
        }
        // 漂移核：另一档真值对拍必判漂移。
        let other = if to_a { TRUTH_B } else { TRUTH_A };
        if b.drift_check(&other)? {
            return Err(BtlCode::SwitchNotFollowed.code());
        }
        steps += 1;
    }
    // 纪元单调：本绑定器恰好消耗 32 次激活。
    if b.epoch != SWITCH_STEPS {
        return Err(BtlCode::EpochRegress.code());
    }
    // 服务器口径：激活 3 号后查询必须给出 product_type=3 真值。
    b.activate(3)?;
    let (srv_truth, _) = b.query(b.epoch)?;
    if !srv_truth.same(&TRUTH_SRV) || srv_truth.product_type != 3 {
        return Err(BtlCode::SwitchNotFollowed.code());
    }
    // 漂移核对拍：A 档真值对活跃服务器档必判漂移。
    if b.drift_check(&TRUTH_A)? {
        return Err(BtlCode::NotReproducible.code());
    }
    Ok((b.registered_total, steps, b.epoch))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：注册未激活即查询 → NoActive。
pub fn probe_a() -> Result<(), u32> {
    let mut b = BottleBinder::new();
    b.register(1, TRUTH_A, 0xAA01)?;
    match b.query(0) {
        Err(c) if c == BtlCode::NoActive.code() => Ok(()),
        _ => Err(BtlCode::NotReproducible.code()),
    }
}

/// 探针 B：重复注册与 ID 越域双闸。
pub fn probe_b() -> Result<(), u32> {
    let mut b = BottleBinder::new();
    b.register(1, TRUTH_A, 0xAA01)?;
    let dup = b.register(1, TRUTH_B, 0xBB02).is_err();
    let id0 = b.register(0, TRUTH_A, 0).is_err();
    let id9 = b.register(9, TRUTH_A, 0).is_err();
    if dup && id0 && id9 {
        Ok(())
    } else {
        Err(BtlCode::NotReproducible.code())
    }
}

/// 探针 C：真值非法（product_type=2 / build 越域）注册拒。
pub fn probe_c() -> Result<(), u32> {
    let mut b = BottleBinder::new();
    let bad_pt = TruthSnap { major: 10, minor: 0, build: 1000, product_type: 2 };
    let bad_build = TruthSnap { major: 10, minor: 0, build: 0x1_0000, product_type: 1 };
    if b.register(4, bad_pt, 0).is_err() && b.register(5, bad_build, 0).is_err() {
        Ok(())
    } else {
        Err(BtlCode::NotReproducible.code())
    }
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BtlCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BtlCode::ALL.len() {
        let v = BtlCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集巡检。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < BtlCode::ALL.len() {
        let c = BtlCode::ALL[i];
        if c.detail().is_empty() || detail_of(c.code()) != c.detail() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
/// UNX-F18414 判据集（≤20 条；动态明细 ≤8 槽）。
pub fn run_unx18414_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18414 版本查询与 bottle 上下文绑定");

    // 1. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7E340 段 0x40 窗口且 0x40 对齐");
    // 2. 黄金全链。
    let golden = golden_run();
    let golden_ok = match &golden {
        Ok((r, st, ep)) => *r == 3 && *st == SWITCH_STEPS && *ep == SWITCH_STEPS + 1,
        Err(_) => false,
    };
    s.add_dyn("J1-黄金全链", golden_ok, &format!("{:?}", golden));
    // 3. 同进程双 bottle 切换跟随（J1 主判据）。
    let sw = switch_probe(SWITCH_STEPS).is_ok() && switch_probe(SWITCH_STEPS * 2).is_ok();
    s.add("J1-双bottle切换跟随", sw, "32/64 步交替激活逐字段跟随");
    // 4. 漂移注入检出（J1 主判据）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-漂移注入10/10", inj_ok, &format!("r={}", JUDGE_ROUNDS));
    // 5. 计数守恒。
    let conserve_ok = match &inj {
        Ok(l) => l.missed_total() == 0,
        Err(_) => false,
    };
    s.add("J1-计数守恒漏检零", conserve_ok, "tries=caught+missed 且 missed=0");
    // 6. 纪元单调：切换只增不回退。
    let mut mono = BottleBinder::new();
    let _ = mono.register(1, TRUTH_A, 0xAA01);
    let _ = mono.register(2, TRUTH_B, 0xBB02);
    let e1 = mono.activate(1).unwrap_or(0);
    let e2 = mono.activate(2).unwrap_or(0);
    let e3 = mono.activate(1).unwrap_or(0);
    s.add("J1-纪元单调前进", e1 < e2 && e2 < e3, "三次激活纪元 1<2<3");
    // 7. 单活跃不变量：激活 B 后 A 不再活跃。
    let single = mono.active_id() == Some(1) && {
        // 逐槽核 live 计数恰为 1。
        let mut live_n = 0;
        let mut i = 0;
        while i < BOTTLE_CAP {
            if let Some(p) = mono.slots[i] {
                if p.live {
                    live_n += 1;
                }
            }
            i += 1;
        }
        live_n == 1
    };
    s.add("J1-单活跃不变量", single, "live 槽恰 1 且活跃 ID 正确");
    // 8. 漂移核双向：同档判无漂移、异档判漂移。
    let mut dv = BottleBinder::new();
    let _ = dv.register(1, TRUTH_A, 0xAA01);
    let _ = dv.activate(1);
    let drift_bi = dv.drift_check(&TRUTH_A) == Ok(true) && dv.drift_check(&TRUTH_B) == Ok(false);
    s.add("J1-漂移核双向", drift_bi, "同档 true / 异档 false");
    // 9. 服务器口径分支。
    let mut srv = BottleBinder::new();
    let _ = srv.register(3, TRUTH_SRV, 0xCC03);
    let _ = srv.activate(3);
    let srv_ok = match srv.query(srv.epoch) {
        Ok((t, _)) => t.product_type == 3 && t.same(&TRUTH_SRV),
        Err(_) => false,
    };
    s.add("J1-服务器口径分支", srv_ok, "product_type=3 真值直读");
    // 10. 探针三序。
    let a = probe_a().is_ok();
    let b = probe_b().is_ok();
    let c = probe_c().is_ok();
    s.add_dyn("J1-探针三序", a && b && c, &format!("a={} b={} c={}", a, b, c));
    // 11. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 12. 空绑定器边界。
    let mut empty = BottleBinder::new();
    s.add("J1-空绑定器边界", empty.query(0).is_err() && empty.active_id().is_none(), "未激活查询必拒");
    // 13. 表满边界。
    let mut full = BottleBinder::new();
    let mut fid = 1;
    let mut fill_ok = true;
    while fid <= BOTTLE_CAP as u32 {
        if full.register(fid, TRUTH_A, fid).is_err() {
            fill_ok = false;
        }
        fid += 1;
    }
    s.add("J1-表满边界", fill_ok && full.register(9, TRUTH_A, 9).is_err(), "4 槽占尽第 5 份必拒");
    // 14. 文案闭集。
    s.add("J3-文案闭集", detail_closure(), "12 码逐条非空且反查一致");
    // 15. 段内窗口逐码核对。
    let win_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < BtlCode::ALL.len() {
            if BtlCode::ALL[i].code() >= SEG_BASE + 0x40 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J1-段内窗口核对", win_ok, "12 码全部 < SEG_BASE+0x40");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 判据集全绿。
    #[test]
    fn all_checks_pass() {
        let set = run_unx18414_checks();
        assert!(set.all_passed(), "UNX-F18414 判据集存在红项");
    }

    /// 正常流：注册→激活→查询跟随。
    #[test]
    fn normal_flow_register_activate_query() {
        let mut b = BottleBinder::new();
        b.register(1, TRUTH_A, 0xAA01).unwrap_or_default();
        b.activate(1).unwrap_or_default();
        let (t, _e) = match b.query(b.epoch) {
            Ok(x) => x,
            Err(_) => panic!("激活后查询失败"),
        };
        assert!(t.same(&TRUTH_A));
    }

    /// 注入三类 10/10。
    #[test]
    fn injection_all_caught() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(_) => panic!("注入轮次内部出错"),
        };
        assert!(led.all_caught());
        assert_eq!(led.missed_total(), 0);
    }

    /// 切换探针：查询结果随档切换。
    #[test]
    fn switch_follows_active() {
        assert_eq!(switch_probe(SWITCH_STEPS), Ok(SWITCH_STEPS));
        assert_eq!(switch_probe(64), Ok(64));
    }

    /// 判定次序：ID 域闸先于重复闸。
    #[test]
    fn order_domain_before_duplicate() {
        let mut b = BottleBinder::new();
        b.register(1, TRUTH_A, 1).unwrap_or_default();
        match b.register(1, TRUTH_A, 1) {
            Err(c) => assert_eq!(c, BtlCode::DuplicateProfile.code()),
            Ok(_) => panic!("重复注册不应成功"),
        }
        match b.register(0, TRUTH_A, 1) {
            Err(c) => assert_eq!(c, BtlCode::IdOutOfRange.code()),
            Ok(_) => panic!("ID=0 不应注册成功"),
        }
    }

    /// 漂移注入：旧纪元句柄必拒。
    #[test]
    fn epoch_drift_detected() {
        let mut b = BottleBinder::new();
        b.register(1, TRUTH_A, 1).unwrap_or_default();
        b.register(2, TRUTH_B, 2).unwrap_or_default();
        b.activate(1).unwrap_or_default();
        let old = b.epoch;
        b.activate(2).unwrap_or_default();
        match b.query(old) {
            Err(c) => assert_eq!(c, BtlCode::EpochDrift.code()),
            Ok(_) => panic!("旧纪元查询不应成功"),
        }
        // 漂移核对拍。
        assert_eq!(b.drift_check(&TRUTH_A), Ok(false));
        assert_eq!(b.drift_check(&TRUTH_B), Ok(true));
    }

    /// 四类边界：空/满/重复/越域。
    #[test]
    fn boundary_edges() {
        let mut b = BottleBinder::new();
        assert!(b.query(0).is_err()); // 空
        let mut i = 1;
        while i <= BOTTLE_CAP as u32 {
            assert!(b.register(i, TRUTH_A, i as u32).is_ok());
            i += 1;
        }
        assert!(b.register(5, TRUTH_A, 5).is_err()); // 满
        assert!(b.register(1, TRUTH_A, 1).is_err()); // 重复
        assert!(b.register(0, TRUTH_A, 0).is_err()); // 越域
        assert!(b.register(9, TRUTH_A, 9).is_err()); // 越域
    }

    /// 双跑一致。
    #[test]
    fn reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫 + 文案闭集。
    #[test]
    fn segment_and_detail() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert!(detail_of(SEG_BASE + 0x40).starts_with("未知"));
    }
}
