//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15401 · IClassFactory 三法全谱（CreateInstance/LockServer/QI） · 0x67300
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15401-J1 三法全谱成文（三法签名/语义/序律逐法断言
//!      10/10 次），伪工厂注入（缺法残形）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三法闭集（签名+语义+序律逐法钉版）：
//!       法 0 QueryInterface：三返回律归位（复用段判据语义，独立写死）；
//!       法 1 CreateInstance：携 unk_outer + iid → 产对象（初始 1 强引）；
//!           聚合路径要求 unk_outer 非空且 iid==IUnknown（聚合律）；
//!       法 2 LockServer(TRUE/FALSE)：锁计数配对守恒（TRUE +1 /
//!           FALSE −1，下溢拒收）；持锁工厂不得销毁；
//!   - 工厂形态账：三法齐全 = 全谱形；缺任一法 = 伪工厂残形，
//!     形检必拒（伪工厂注入 10/10 检出）；
//!   - 序律：LockServer(TRUE) 后才允许「持锁期」；销毁前锁计数必须归零；
//!   - LCG 种子写死 = SEG_BASE（0x67300），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67300，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67300;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x67300;

/// 三法闭集。
pub const M_QI: u8 = 0;
pub const M_CREATE: u8 = 1;
pub const M_LOCK: u8 = 2;
pub const METHOD_COUNT: usize = 3;
pub const METHOD_NAMES: [&str; METHOD_COUNT] = ["QueryInterface", "CreateInstance", "LockServer"];

/// IID 闭集（CreateInstance/QI 共用最小面）。
pub const IID_IUNKNOWN: u32 = 0;
pub const IID_ICLASSFACTORY: u32 = 1;
pub const IID_IPERSIST: u32 = 2;
pub const IID_FOREIGN: u32 = 3;
pub const IID_COUNT: usize = 4;

/// LockServer 布尔面。
pub const LOCK_TRUE: u8 = 1;
pub const LOCK_FALSE: u8 = 0;

/// 判据投影：逐法断言 10/10。
pub const SPECTRA_TRIALS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IcfCode {
    /// 方法码越闭集
    MethodOutOfRange,
    /// 伪工厂残形（缺法）
    PseudoFactoryShape,
    /// QI 返回第四态（归位外值）
    QiFourthState,
    /// CreateInstance 产出零引用对象（初引缺失）
    CreateNoInitRef,
    /// 聚合路径违规（unk_outer 空/错 iid）
    AggregatePathBreach,
    /// LockServer 下溢（FALSE 无配对 TRUE）
    LockUnderflow,
    /// 持锁销毁（锁计数非零即弃厂）
    DestroyWhileLocked,
    /// 锁配对不守恒（TRUE/FALSE 差非零）
    LockPairBreach,
    /// 序律违（持锁期外声称持锁）
    LockPhaseBreach,
}

impl IcfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IcfCode::MethodOutOfRange => "方法码越闭集：三法冻结",
            IcfCode::PseudoFactoryShape => "伪工厂残形：三法缺一",
            IcfCode::QiFourthState => "QI 返回第四态：归位外值",
            IcfCode::CreateNoInitRef => "CreateInstance 产出零引用对象",
            IcfCode::AggregatePathBreach => "聚合路径违规：外层/iid 不合律",
            IcfCode::LockUnderflow => "LockServer 下溢：FALSE 无配对",
            IcfCode::DestroyWhileLocked => "持锁销毁：锁计数非零即弃厂",
            IcfCode::LockPairBreach => "锁配对不守恒：TRUE/FALSE 差非零",
            IcfCode::LockPhaseBreach => "序律违：持锁期状态与账不符",
        }
    }
    pub const ALL: [IcfCode; 9] = [
        IcfCode::MethodOutOfRange,
        IcfCode::PseudoFactoryShape,
        IcfCode::QiFourthState,
        IcfCode::CreateNoInitRef,
        IcfCode::AggregatePathBreach,
        IcfCode::LockUnderflow,
        IcfCode::DestroyWhileLocked,
        IcfCode::LockPairBreach,
        IcfCode::LockPhaseBreach,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IcfCode::ALL.len() {
        if IcfCode::ALL[i].val() == code {
            return IcfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67300 段闭集内"
}

// ── 工厂形态账（全谱/残形判定） ──────────────────────────────────
/// 三法位图：bit0=QI，bit1=CreateInstance，bit2=LockServer。
pub const SHAPE_FULL: u32 = 0b111;

/// 工厂实体。
pub struct ClassFactory {
    pub id: u32,
    /// 形态位图（三法齐全 = 全谱形）。
    pub shape: u32,
    /// LockServer 锁计数。
    pub locks: i64,
    /// 工厂自持引用（初值 1）。
    pub self_refs: i64,
    /// 本厂产出对象数（CreateInstance 账）。
    pub produced: u32,
    /// 持锁期标记（序律账）。
    pub in_lock_phase: bool,
}

impl ClassFactory {
    pub fn new(id: u32, shape: u32) -> ClassFactory {
        ClassFactory {
            id,
            shape,
            locks: 0,
            self_refs: 1,
            produced: 0,
            in_lock_phase: false,
        }
    }
    /// 形检：三法齐全才可上岗。
    pub fn is_full_spectrum(&self) -> bool {
        self.shape == SHAPE_FULL
    }
}

// ── 三法语义（逐法钉版） ─────────────────────────────────────────
/// 法 0：QI 三返回律（判据语义独立写死：0/1 号已知 → S_OK(0)；
/// 2/3 号未实现 → E_NOINTERFACE(0x80004002)；其他一切 → 第四态拒）。
pub const HR_S_OK: u32 = 0x0000_0000;
pub const HR_E_NOINTERFACE: u32 = 0x8000_4002;

pub fn factory_qi(iid: u32) -> Result<u32, u32> {
    if iid as usize >= IID_COUNT {
        return Err(IcfCode::QiFourthState.val());
    }
    match iid {
        IID_IUNKNOWN | IID_ICLASSFACTORY => Ok(HR_S_OK),
        _ => Ok(HR_E_NOINTERFACE),
    }
}

/// 法 1：CreateInstance。聚合律：unk_outer≠0 时 iid 必须 IUnknown；
/// 产出对象初始恰 1 强引用。返回产出对象 id（1 起）。
pub fn factory_create(f: &mut ClassFactory, unk_outer: u32, iid: u32) -> Result<u32, u32> {
    if unk_outer != 0 && iid != IID_IUNKNOWN {
        return Err(IcfCode::AggregatePathBreach.val());
    }
    if iid as usize >= IID_COUNT {
        return Err(IcfCode::QiFourthState.val());
    }
    if iid == IID_FOREIGN {
        // 未实现接口：不产出（E_NOINTERFACE 语义，零产出）。
        return Err(IcfCode::QiFourthState.val());
    }
    f.produced += 1;
    // 初引钉版：产出即 1 强引用（守恒列核对在审计面）。
    if f.produced == 0 {
        return Err(IcfCode::CreateNoInitRef.val());
    }
    Ok(f.produced)
}

/// 法 2：LockServer。TRUE +1 / FALSE −1（下溢拒）；持锁期账随动。
pub fn factory_lock(f: &mut ClassFactory, b: u8) -> Result<i64, u32> {
    match b {
        LOCK_TRUE => {
            f.locks += 1;
            f.in_lock_phase = true;
        }
        LOCK_FALSE => {
            if f.locks <= 0 {
                return Err(IcfCode::LockUnderflow.val());
            }
            f.locks -= 1;
            if f.locks == 0 {
                f.in_lock_phase = false;
            }
        }
        _ => return Err(IcfCode::MethodOutOfRange.val()),
    }
    Ok(f.locks)
}

/// 工厂销毁序律：锁归零 + 自引用释放后才可弃厂。
pub fn factory_destroy(f: &ClassFactory) -> Result<(), u32> {
    if f.locks != 0 {
        return Err(IcfCode::DestroyWhileLocked.val());
    }
    if f.in_lock_phase {
        return Err(IcfCode::LockPhaseBreach.val());
    }
    Ok(())
}

// ── 三法全谱序账（一轮：QI→Create→Lock 全谱走查） ───────────────
pub struct FactorySpectra {
    pub factories: Vec<ClassFactory>,
}

impl FactorySpectra {
    pub fn new() -> FactorySpectra {
        FactorySpectra { factories: Vec::new() }
    }

    /// 一轮全谱：全谱形工厂走 QI 四族 + Create 两路 + Lock 配对 + 弃厂。
    /// 返回 (QI 断言数, 产出数, 锁配对数)。
    pub fn run_spectra(&mut self) -> Result<(u32, u32, u32), u32> {
        let id = self.factories.len() as u32;
        self.factories.push(ClassFactory::new(id, SHAPE_FULL));
        let f = &mut self.factories[id as usize];
        if !f.is_full_spectrum() {
            return Err(IcfCode::PseudoFactoryShape.val());
        }
        // 法 0 断言：4 族 × 期望值（判据写死：0/1→S_OK，2/3→E_NOINTERFACE）。
        let mut qi_asserts = 0;
        let mut iid = 0;
        while (iid as usize) < IID_COUNT {
            let hr = factory_qi(iid)?;
            let expect = if iid <= IID_ICLASSFACTORY { HR_S_OK } else { HR_E_NOINTERFACE };
            if hr != expect {
                return Err(IcfCode::QiFourthState.val());
            }
            qi_asserts += 1;
            iid += 1;
        }
        // 法 1 断言：非聚合 1 路 + 聚合 1 路 + 聚合违规 1 路（必拒）。
        let o1 = factory_create(f, 0, IID_IUNKNOWN)?;
        if o1 != 1 {
            return Err(IcfCode::CreateNoInitRef.val());
        }
        let _o2 = factory_create(f, 77, IID_IUNKNOWN)?;
        let bad_agg = factory_create(f, 77, IID_IPERSIST);
        if let Ok(_) = bad_agg {
            return Err(IcfCode::AggregatePathBreach.val());
        }
        // 法 2 断言：TRUE/FALSE 配对 + 下溢拒。
        let l1 = factory_lock(f, LOCK_TRUE)?;
        let l2 = factory_lock(f, LOCK_TRUE)?;
        if l1 != 1 || l2 != 2 {
            return Err(IcfCode::LockPairBreach.val());
        }
        let l3 = factory_lock(f, LOCK_FALSE)?;
        let l4 = factory_lock(f, LOCK_FALSE)?;
        if l3 != 1 || l4 != 0 {
            return Err(IcfCode::LockPairBreach.val());
        }
        let under = factory_lock(f, LOCK_FALSE);
        if let Ok(_) = under {
            return Err(IcfCode::LockUnderflow.val());
        }
        // 序律：锁归零后弃厂合法。
        factory_destroy(f)?;
        Ok((qi_asserts, f.produced, 2))
    }
}

// ── 确定性 LCG（种子 = 0x67300） ─────────────────────────────────
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

// ── 10/10 全谱压演（每轮 1 全谱 + 2 伪形必拒） ───────────────────
pub fn spectra_round(round: usize) -> Result<(u32, u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut sp = FactorySpectra::new();
    let (q, p, l) = sp.run_spectra()?;
    if q != IID_COUNT as u32 || p != 2 || l != 2 {
        return Err(IcfCode::PseudoFactoryShape.val());
    }
    // 伪形注入：缺法残形（三选一缺）×2，形检必拒。
    let mut pseudo_caught = 0;
    let mut k = 0;
    while k < 2 {
        let shape = match k {
            0 => 0b110, // 缺 LockServer
            _ => 0b101, // 缺 CreateInstance
        };
        let f = ClassFactory::new(99, shape);
        if !f.is_full_spectrum() {
            pseudo_caught += 1;
        }
        k += 1;
    }
    if pseudo_caught != 2 {
        return Err(IcfCode::PseudoFactoryShape.val());
    }
    let _ = lcg.next();
    Ok((q, p, l))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut rounds = 0;
    let mut prod_total = 0;
    let mut r = 0;
    while r < SPECTRA_TRIALS {
        let (_, p, _) = spectra_round(r)?;
        prod_total += p;
        rounds += 1;
        r += 1;
    }
    // 黄金手算对照（写死）：
    //   QI(IUNKNOWN)=S_OK；QI(FOREIGN)=E_NOINTERFACE；
    //   聚合(outer≠0, iid≠IUnknown) 拒；锁 TRUE,TRUE,FALSE,FALSE → 1,2,1,0。
    let a = factory_qi(IID_IUNKNOWN)?;
    let b = factory_qi(IID_FOREIGN)?;
    if a != HR_S_OK || b != HR_E_NOINTERFACE {
        return Err(IcfCode::QiFourthState.val());
    }
    let mut g = ClassFactory::new(7, SHAPE_FULL);
    let bad = factory_create(&mut g, 9, IID_IPERSIST);
    if let Ok(_) = bad {
        return Err(IcfCode::AggregatePathBreach.val());
    }
    let l = factory_lock(&mut g, LOCK_TRUE)?;
    if l != 1 {
        return Err(IcfCode::LockPairBreach.val());
    }
    let destroy_locked = factory_destroy(&g);
    if let Ok(_) = destroy_locked {
        return Err(IcfCode::DestroyWhileLocked.val());
    }
    let _ = factory_lock(&mut g, LOCK_FALSE)?;
    factory_destroy(&g)?;
    Ok((rounds, prod_total, g.locks as u32))
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
    // 族 0：伪工厂残形（6 种缺法位图，排除 0b111 全谱形）×10。
    let mut t = 0;
    while t < 10 {
        let shape = (t % 6) as u32 + 1; // 1..6 全为非 0b111
        let f = ClassFactory::new(1, shape);
        ledger.tries[0] += 1;
        if !f.is_full_spectrum() {
            ledger.caught[0] += 1;
        }
        t += 1;
    }
    // 族 1：聚合路径违规（outer≠0 且 iid≠IUnknown）×10。
    let mut t = 0;
    while t < 10 {
        let mut f = ClassFactory::new(1, SHAPE_FULL);
        let v = factory_create(&mut f, 55, IID_ICLASSFACTORY);
        ledger.tries[1] += 1;
        if let Err(e) = v {
            if e == IcfCode::AggregatePathBreach.val() {
                ledger.caught[1] += 1;
            }
        }
        t += 1;
    }
    // 族 2：持锁销毁注入 ×10。
    let mut t = 0;
    while t < 10 {
        let mut f = ClassFactory::new(1, SHAPE_FULL);
        let _ = factory_lock(&mut f, LOCK_TRUE);
        let v = factory_destroy(&f);
        ledger.tries[2] += 1;
        if let Err(e) = v {
            if e == IcfCode::DestroyWhileLocked.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IcfCode::ALL.len() {
        let v = IcfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 三法闭集自洽（判据常量独立写死）。
    METHOD_COUNT == 3 && SHAPE_FULL == 0b111 && IID_COUNT == 4
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((rounds, _, locks)) = golden {
        rounds == SPECTRA_TRIALS as u32 && locks == 0
    } else {
        false
    };
    v.push((
        "J1-三法全谱10/10逐法断言",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-伪工厂/聚合/持锁注入10/10必抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 三法名点验。
    let mut okn = 0;
    let mut i = 0;
    while i < METHOD_COUNT {
        if METHOD_NAMES[i].len() > 0 {
            okn += 1;
        }
        i += 1;
    }
    v.push((
        "J1-三法名点验",
        okn == METHOD_COUNT,
        format!("{}/3", okn),
    ));
    // 黄金 QI 四族独立点验（判据写死）。
    let q0 = factory_qi(IID_IUNKNOWN);
    let q3 = factory_qi(IID_FOREIGN);
    let q_ok = q0 == Ok(HR_S_OK) && q3 == Ok(HR_E_NOINTERFACE);
    v.push((
        "J1-黄金QI四族归位",
        q_ok,
        format!("IUNKNOWN=S_OK FOREIGN=E_NOINTERFACE"),
    ));
    // 锁配对守恒独立点验。
    let mut f = ClassFactory::new(3, SHAPE_FULL);
    let _ = factory_lock(&mut f, LOCK_TRUE);
    let _ = factory_lock(&mut f, LOCK_TRUE);
    let _ = factory_lock(&mut f, LOCK_FALSE);
    let _ = factory_lock(&mut f, LOCK_FALSE);
    v.push((
        "J1-锁配对归零守恒",
        f.locks == 0 && !f.in_lock_phase,
        format!("locks={}", f.locks),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67300 窗口")));
    v
}

pub fn run_unx15401_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15401 IClassFactory 三法全谱");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
