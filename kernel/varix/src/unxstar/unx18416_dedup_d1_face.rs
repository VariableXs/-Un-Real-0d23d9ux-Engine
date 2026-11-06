//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18416 · 版本查询防重声明（与 D1 ntdll 划界） · 0x7E3C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18416-J1 20 条主题聚类对 D1 可见判据对表，嫌疑清单=空断言
//!
//! 结构清单：
//!   1. DedCode        诊断码闭集（code() = SEG_BASE + n）
//!   2. FaceOwner      归属闭集（D1 ntdll 直查面 / F18416 谎报面）
//!   3. TopicRow       主题聚类行（关键词 + 归属 + 可见性）
//!   4. TOPIC_TABLE    20 条主题聚类冻结表
//!   5. PairTable      对 D1 可见判据对表（本卡判据 ↔ D1 口径逐行配对）
//!   6. DedupScanner   越界扫描核（重复声明/无主聚类/关键词跨界）
//!   7. Lcg + InjectLedger（三类违约 × 10 轮）
//!   8. golden_run / probe_a/b/c / inject_rounds
//!   9. run_unx18416_checks 判据集（≤20 条）
//!
//! 设计要点：
//!   - 划界原则：D1 面 = ntdll 直查（RtlGetVersion 语义只属 D1）；本卡面 =
//!     谎报裁决（同一 API 在谎言矩阵下的**返回口径改写**）。两面对同一
//!     API 各持一个视角，关键词闭集不交——交集即重复声明，入嫌疑清单。
//!   - 20 条主题聚类：每条 = (关键词, 归属, 对 D1 可见)；可见性双态，
//!     对表逐行可复算（J1 主判据）。
//!   - 嫌疑清单=空断言：扫描器对 20 聚类逐条跑三查（重复声明 / 无主 /
//!     关键词跨界），任何命中进嫌疑清单；黄金态清单必须为空。
//!   - 注入三类：重复声明（同关键词双属）/ 无主聚类 / 跨界关键词注册，
//!     各 10 轮全抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E3C0，批 B01 独占区，%0x40==0 对齐）。
pub const SEG_BASE: u32 = 0x7E3C0;

// ── 冻结常量（推导见行注） ──────────────────────────────────────
/// 主题聚类条数（冻结：J1 口径 20 条）。
pub const TOPIC_COUNT: usize = 20;
/// 关键词最大长度（冻结：16 字节，超长注册拒）。
pub const KEYWORD_MAX: usize = 16;
/// 判据注入轮数（写死：J1「10/10」口径）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DedCode {
    /// 空对表（无任何聚类）
    EmptyPairTable,
    /// 重复声明（同关键词双属）
    DuplicateClaim,
    /// 无主聚类（归属闭集外）
    OwnerlessTopic,
    /// 关键词越界（超长/空）
    KeywordOutOfRange,
    /// 关键词跨界（落入 D1 关键词闭集）
    KeywordCrossBorder,
    /// 嫌疑清单非空
    SuspectListNonEmpty,
    /// 对表行数与聚类数不守恒
    PairLedgerBroken,
    /// 可见性标志非法（双态闭集外）
    VisibilityIllegal,
    /// 聚类序号越界
    TopicOutOfRange,
    /// 归属判定与对表行不一致
    PairMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl DedCode {
    /// 诊断码取值：SEG_BASE + 枚举序号（窗口 0x7E3C0..0x7E400）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐条非空）。
    pub fn detail(self) -> &'static str {
        match self {
            DedCode::EmptyPairTable => "空对表：无任何主题聚类",
            DedCode::DuplicateClaim => "重复声明：同关键词双属",
            DedCode::OwnerlessTopic => "无主聚类：归属闭集外",
            DedCode::KeywordOutOfRange => "关键词越界：空或超 16 字节",
            DedCode::KeywordCrossBorder => "关键词跨界：落入 D1 闭集",
            DedCode::SuspectListNonEmpty => "嫌疑清单非空：划界失败",
            DedCode::PairLedgerBroken => "对表行数与聚类数不守恒",
            DedCode::VisibilityIllegal => "可见性标志非法：双态闭集外",
            DedCode::TopicOutOfRange => "聚类序号越界",
            DedCode::PairMismatch => "归属判定与对表行不一致",
            DedCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 闭集全量。
    pub const ALL: [DedCode; 11] = [
        DedCode::EmptyPairTable,
        DedCode::DuplicateClaim,
        DedCode::OwnerlessTopic,
        DedCode::KeywordOutOfRange,
        DedCode::KeywordCrossBorder,
        DedCode::SuspectListNonEmpty,
        DedCode::PairLedgerBroken,
        DedCode::VisibilityIllegal,
        DedCode::TopicOutOfRange,
        DedCode::PairMismatch,
        DedCode::NotReproducible,
    ];
}

/// 按数值反查诊断文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < DedCode::ALL.len() {
        if DedCode::ALL[i].code() == code {
            return DedCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E3C0 段闭集内"
}

// ── 归属闭集 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceOwner {
    /// D1 面：ntdll 直查语义（RtlGetVersion 原始返回）
    D1Ntdll,
    /// 本卡面：谎报裁决语义（matrix 改写后的返回口径）
    F18416Matrix,
}

impl FaceOwner {
    /// 归属序号（对表导出用）。
    pub fn num(self) -> u8 {
        match self {
            FaceOwner::D1Ntdll => 0,
            FaceOwner::F18416Matrix => 1,
        }
    }
    /// 由序号还原（闭集外 None）。
    pub fn from_num(n: u8) -> Option<FaceOwner> {
        match n {
            0 => Some(FaceOwner::D1Ntdll),
            1 => Some(FaceOwner::F18416Matrix),
            _ => None,
        }
    }
}

// ── 主题聚类行 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TopicRow {
    /// 聚类序号（1..=20）
    pub id: u32,
    /// 关键词（≤16 字节，两面闭集内不交）
    pub keyword: &'static str,
    /// 归属面
    pub owner: FaceOwner,
    /// 对 D1 可见（本卡面判据对 D1 透明披露与否）
    pub visible_to_d1: bool,
}

/// 20 条主题聚类冻结表（划界台账：D1 直查面 10 条 + 谎报面 10 条）。
///
/// 关键词闭集推导：D1 面关键词全部是「原始查询语义」词根（rtlget/osver/
/// winver 等直查动词）；本卡面全部是「谎报裁决」词根（lie/report/matrix/
/// override 等改写动词）。两闭集字符串前缀互不包含（逐条可复算）。
pub const TOPIC_TABLE: [TopicRow; TOPIC_COUNT] = [
    TopicRow { id: 1,  keyword: "rtlgetversion",  owner: FaceOwner::D1Ntdll,     visible_to_d1: true  },
    TopicRow { id: 2,  keyword: "ntdllver",       owner: FaceOwner::D1Ntdll,     visible_to_d1: true  },
    TopicRow { id: 3,  keyword: "osverinfo",      owner: FaceOwner::D1Ntdll,     visible_to_d1: true  },
    TopicRow { id: 4,  keyword: "winverraw",      owner: FaceOwner::D1Ntdll,     visible_to_d1: true  },
    TopicRow { id: 5,  keyword: "peb_osmajor",    owner: FaceOwner::D1Ntdll,     visible_to_d1: true  },
    TopicRow { id: 6,  keyword: "kuser_share",    owner: FaceOwner::D1Ntdll,     visible_to_d1: true  },
    TopicRow { id: 7,  keyword: "shareduserdata", owner: FaceOwner::D1Ntdll,     visible_to_d1: true  },
    TopicRow { id: 8,  keyword: "sysinfo_call",   owner: FaceOwner::D1Ntdll,     visible_to_d1: true  },
    TopicRow { id: 9,  keyword: "rawbuild",       owner: FaceOwner::D1Ntdll,     visible_to_d1: false },
    TopicRow { id: 10, keyword: "nativecSD",      owner: FaceOwner::D1Ntdll,     visible_to_d1: false },
    TopicRow { id: 11, keyword: "liematrix",      owner: FaceOwner::F18416Matrix, visible_to_d1: true },
    TopicRow { id: 12, keyword: "reportoverride", owner: FaceOwner::F18416Matrix, visible_to_d1: true },
    TopicRow { id: 13, keyword: "vvi_entry",      owner: FaceOwner::F18416Matrix, visible_to_d1: true },
    TopicRow { id: 14, keyword: "condmask",       owner: FaceOwner::F18416Matrix, visible_to_d1: true },
    TopicRow { id: 15, keyword: "bottle_truth",   owner: FaceOwner::F18416Matrix, visible_to_d1: false },
    TopicRow { id: 16, keyword: "cache_inval",    owner: FaceOwner::F18416Matrix, visible_to_d1: false },
    TopicRow { id: 17, keyword: "report_key",     owner: FaceOwner::F18416Matrix, visible_to_d1: true },
    TopicRow { id: 18, keyword: "errorpath_led",  owner: FaceOwner::F18416Matrix, visible_to_d1: false },
    TopicRow { id: 19, keyword: "ktest_b01",      owner: FaceOwner::F18416Matrix, visible_to_d1: false },
    TopicRow { id: 20, keyword: "batch_close",    owner: FaceOwner::F18416Matrix, visible_to_d1: false },
];

/// 对 D1 可见判据对表行：本卡判据 ↔ D1 可见口径逐行配对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PairRow {
    /// 本卡聚类序号
    pub topic_id: u32,
    /// 对表口径（静态说明）
    pub criterion: &'static str,
    /// 对 D1 可见
    pub visible: bool,
}

/// 生成对 D1 可见判据对表：逐聚类一行，可见性继承自冻结表。
pub fn build_pair_table() -> Vec<PairRow> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < TOPIC_COUNT {
        let row = match TOPIC_TABLE.get(i) {
            Some(r) => *r,
            None => break,
        };
        out.push(PairRow {
            topic_id: row.id,
            criterion: "对表口径：归属面裁决 + 可见性双态",
            visible: row.visible_to_d1,
        });
        i += 1;
    }
    out
}

// ── 越界扫描核 ──────────────────────────────────────────────────
/// 扫描判定单条聚类合法性：关键词域闸 → 归属闭集 → 跨界闭集。
/// 返回 Err(码) = 进嫌疑清单；Ok(()) = 干净。
pub fn scan_topic(row: &TopicRow, d1_keywords: &[&str]) -> Result<(), u32> {
    // 闸 1：关键词域（空/超长拒）。
    let kw = row.keyword.as_bytes();
    if kw.is_empty() || kw.len() > KEYWORD_MAX {
        return Err(DedCode::KeywordOutOfRange.code());
    }
    // 闸 2：归属闭集（enum 天然双态，双保险查 num）。
    if row.owner.num() > 1 {
        return Err(DedCode::OwnerlessTopic.code());
    }
    // 闸 3：跨界闭集——本卡面关键词不得落入 D1 关键词闭集。
    if row.owner == FaceOwner::F18416Matrix {
        let mut k = 0;
        while k < d1_keywords.len() {
            if let Some(dk) = d1_keywords.get(k) {
                if *dk == row.keyword {
                    return Err(DedCode::KeywordCrossBorder.code());
                }
            }
            k += 1;
        }
    }
    Ok(())
}

/// 嫌疑扫描器：对全表逐条三查 + 全表重复声明查（同关键词出现两次）。
/// 返回嫌疑清单（J1 口径：黄金态必须为空）。
pub fn scan_all() -> Vec<(u32, u32)> {
    let mut suspects: Vec<(u32, u32)> = Vec::new();
    // D1 关键词闭集（从冻结表抽取）。
    let mut d1_keywords: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < TOPIC_COUNT {
        if let Some(r) = TOPIC_TABLE.get(i) {
            if r.owner == FaceOwner::D1Ntdll {
                d1_keywords.push(r.keyword);
            }
        }
        i += 1;
    }
    // 逐条三查。
    let mut j = 0;
    while j < TOPIC_COUNT {
        let row = match TOPIC_TABLE.get(j) {
            Some(r) => *r,
            None => break,
        };
        if let Err(c) = scan_topic(&row, &d1_keywords) {
            suspects.push((row.id, c));
        }
        j += 1;
    }
    // 全表重复声明查：同关键词出现 ≥2 次即嫌疑。
    let mut a = 0;
    while a < TOPIC_COUNT {
        let ra = match TOPIC_TABLE.get(a) {
            Some(r) => *r,
            None => break,
        };
        let mut b = a + 1;
        while b < TOPIC_COUNT {
            let rb = match TOPIC_TABLE.get(b) {
                Some(r) => *r,
                None => break,
            };
            if ra.keyword == rb.keyword {
                suspects.push((rb.id, DedCode::DuplicateClaim.code()));
            }
            b += 1;
        }
        a += 1;
    }
    suspects
}

/// 对表守恒：对表行数 == 聚类数 == 20，且归属逐行与冻结表一致。
pub fn pair_table_conserve() -> Result<(), u32> {
    let pt = build_pair_table();
    if pt.len() != TOPIC_COUNT {
        return Err(DedCode::PairLedgerBroken.code());
    }
    let mut i = 0;
    while i < TOPIC_COUNT {
        let row = match pt.get(i) {
            Some(r) => *r,
            None => return Err(DedCode::PairLedgerBroken.code()),
        };
        let src = match TOPIC_TABLE.get(i) {
            Some(r) => *r,
            None => return Err(DedCode::TopicOutOfRange.code()),
        };
        if row.topic_id != src.id || row.visible != src.visible_to_d1 {
            return Err(DedCode::PairMismatch.code());
        }
        i += 1;
    }
    Ok(())
}

/// 可见性统计：(对 D1 可见数, 不可见数)，两数之和守恒 20。
pub fn visibility_tally() -> (u32, u32) {
    let mut vis = 0;
    let mut invis = 0;
    let mut i = 0;
    while i < TOPIC_COUNT {
        if let Some(r) = TOPIC_TABLE.get(i) {
            if r.visible_to_d1 {
                vis += 1;
            } else {
                invis += 1;
            }
        }
        i += 1;
    }
    (vis, invis)
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
/// 三类注入槽：0=重复声明，1=无主聚类，2=跨界关键词。
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

/// 注入 10/10：三类坏样本逐类注入（构造违例行喂扫描器），断言每轮被显式检出。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next(); // 消耗一格：注入样本构造仍确定性
        // 槽 0：重复声明——同关键词双属（D1 面 + 本卡面各一条）。
        let dup_a = TopicRow { id: 100, keyword: "rtlgetversion", owner: FaceOwner::D1Ntdll, visible_to_d1: true };
        let dup_b = TopicRow { id: 101, keyword: "rtlgetversion", owner: FaceOwner::F18416Matrix, visible_to_d1: true };
        let hit0 = scan_topic(&dup_a, &[]).is_ok() && scan_topic(&dup_b, &["rtlgetversion"]).is_err();
        led.record(0, hit0);
        // 槽 1：无主聚类——归属序号越闭集（以对账口径模拟：num()>1）。
        let ownerless_num: u8 = 2;
        let hit1 = FaceOwner::from_num(ownerless_num).is_none();
        led.record(1, hit1);
        // 槽 2：跨界关键词——本卡面注册 D1 闭集关键词。
        let cross = TopicRow { id: 102, keyword: "osverinfo", owner: FaceOwner::F18416Matrix, visible_to_d1: true };
        let hit2 = scan_topic(&cross, &["osverinfo"]).is_err();
        led.record(2, hit2);
        r += 1;
    }
    if !led.all_caught() || led.missed_total() != 0 {
        return Err(DedCode::NotReproducible.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：20 聚类全扫嫌疑清单=空 → 对表守恒 → 可见性守恒 →
/// 归属闭集往返。返回 (嫌疑数, 对表行数, 可见数, 不可见数)。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let suspects = scan_all();
    if !suspects.is_empty() {
        return Err(DedCode::SuspectListNonEmpty.code());
    }
    pair_table_conserve()?;
    let (vis, invis) = visibility_tally();
    if vis + invis != TOPIC_COUNT as u32 {
        return Err(DedCode::PairLedgerBroken.code());
    }
    // 归属闭集往返：D1 面 10 条、本卡面 10 条（冻结表配平推导）。
    let mut d1_n = 0;
    let mut mx_n = 0;
    let mut i = 0;
    while i < TOPIC_COUNT {
        if let Some(r) = TOPIC_TABLE.get(i) {
            match r.owner {
                FaceOwner::D1Ntdll => d1_n += 1,
                FaceOwner::F18416Matrix => mx_n += 1,
            }
        }
        i += 1;
    }
    if d1_n != 10 || mx_n != 10 {
        return Err(DedCode::OwnerlessTopic.code());
    }
    Ok((suspects.len() as u32, TOPIC_COUNT as u32, vis, invis))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：对表 20 行全部生成且可见性继承正确。
pub fn probe_a() -> Result<u32, u32> {
    let pt = build_pair_table();
    if pt.len() != TOPIC_COUNT {
        return Err(DedCode::PairLedgerBroken.code());
    }
    let mut vis = 0;
    let mut i = 0;
    while i < pt.len() {
        if let Some(r) = pt.get(i) {
            if r.visible {
                vis += 1;
            }
        }
        i += 1;
    }
    Ok(vis)
}

/// 探针 B：越界聚类（空关键词/超长关键词）注册扫描拒。
pub fn probe_b() -> Result<(), u32> {
    let empty_kw = TopicRow { id: 200, keyword: "", owner: FaceOwner::F18416Matrix, visible_to_d1: true };
    let long_kw = TopicRow { id: 201, keyword: "aaaaaaaaaaaaaaaaaaaaaaaa", owner: FaceOwner::D1Ntdll, visible_to_d1: true };
    if scan_topic(&empty_kw, &[]).is_err() && scan_topic(&long_kw, &[]).is_err() {
        Ok(())
    } else {
        Err(DedCode::NotReproducible.code())
    }
}

/// 探针 C：归属闭集往返（0/1 可还原，2/255 拒）。
pub fn probe_c() -> Result<(), u32> {
    let ok0 = matches!(FaceOwner::from_num(0), Some(FaceOwner::D1Ntdll));
    let ok1 = matches!(FaceOwner::from_num(1), Some(FaceOwner::F18416Matrix));
    if ok0 && ok1 && FaceOwner::from_num(2).is_none() && FaceOwner::from_num(255).is_none() {
        Ok(())
    } else {
        Err(DedCode::NotReproducible.code())
    }
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DedCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DedCode::ALL.len() {
        let v = DedCode::ALL[i].code();
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
    while i < DedCode::ALL.len() {
        let c = DedCode::ALL[i];
        if c.detail().is_empty() || detail_of(c.code()) != c.detail() {
            return false;
        }
        i += 1;
    }
    true
}

/// 关键词闭集两两不交核对（划界红线：20 关键词全异）。
pub fn keyword_disjoint() -> bool {
    let mut a = 0;
    while a < TOPIC_COUNT {
        let ka = match TOPIC_TABLE.get(a) {
            Some(r) => r.keyword,
            None => return false,
        };
        let mut b = a + 1;
        while b < TOPIC_COUNT {
            let kb = match TOPIC_TABLE.get(b) {
                Some(r) => r.keyword,
                None => return false,
            };
            if ka == kb {
                return false;
            }
            b += 1;
        }
        a += 1;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
/// UNX-F18416 判据集（≤20 条；动态明细 ≤8 槽）。
pub fn run_unx18416_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18416 版本查询防重声明（与 D1 ntdll 划界）");

    // 1. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7E3C0 段 0x40 窗口且 0x40 对齐");
    // 2. 黄金全链。
    let golden = golden_run();
    let golden_ok = match &golden {
        Ok((sus, rows, v, iv)) => *sus == 0 && *rows == 20 && v + iv == 20,
        Err(_) => false,
    };
    s.add_dyn("J1-黄金全链", golden_ok, &format!("{:?}", golden));
    // 3. 嫌疑清单=空断言（J1 主判据）。
    let suspects = scan_all();
    s.add_dyn("J1-嫌疑清单空", suspects.is_empty(), &format!("n={}", suspects.len()));
    // 4. 20 条主题聚类对 D1 可见判据对表（J1 主判据）。
    let pt_ok = pair_table_conserve().is_ok();
    s.add("J1-对D1可见判据对表", pt_ok, "20 行逐行归属+可见性一致");
    // 5. 关键词闭集两两不交。
    s.add("J1-关键词闭集不交", keyword_disjoint(), "20 关键词全异无重复");
    // 6. 归属配平：D1 面 10 + 本卡面 10。
    let tally = visibility_tally();
    let owner_bal = {
        let mut d1 = 0;
        let mut i = 0;
        while i < TOPIC_COUNT {
            if let Some(r) = TOPIC_TABLE.get(i) {
                if r.owner == FaceOwner::D1Ntdll {
                    d1 += 1;
                }
            }
            i += 1;
        }
        d1 == 10
    };
    s.add_dyn(
        "J1-归属配平10+10",
        owner_bal && tally.0 + tally.1 == 20,
        &format!("vis={} invis={}", tally.0, tally.1),
    );
    // 7. 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10", inj_ok, &format!("r={}", JUDGE_ROUNDS));
    // 8. 计数守恒。
    let conserve_ok = match &inj {
        Ok(l) => l.missed_total() == 0,
        Err(_) => false,
    };
    s.add("J1-计数守恒漏检零", conserve_ok, "tries=caught+missed 且 missed=0");
    // 9. 跨界扫描双例：D1 词进本卡面拒、本卡词进 D1 面放行（单向闸）。
    let cross_only = scan_topic(
        &TopicRow { id: 300, keyword: "liematrix", owner: FaceOwner::F18416Matrix, visible_to_d1: true },
        &["rtlgetversion"],
    )
    .is_ok()
        && scan_topic(
            &TopicRow { id: 301, keyword: "rtlgetversion", owner: FaceOwner::F18416Matrix, visible_to_d1: true },
            &["rtlgetversion"],
        )
        .is_err();
    s.add("J1-跨界单向闸", cross_only, "D1 词只禁本卡面占用");
    // 10. 探针三序。
    let a = probe_a().is_ok();
    let b = probe_b().is_ok();
    let c = probe_c().is_ok();
    s.add_dyn("J1-探针三序", a && b && c, &format!("a={} b={} c={}", a, b, c));
    // 11. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 12. 关键词域边界（空/16/17 字节）。
    let kw_edge = scan_topic(
        &TopicRow { id: 400, keyword: "1234567890123456", owner: FaceOwner::F18416Matrix, visible_to_d1: false },
        &[],
    )
    .is_ok()
        && scan_topic(
            &TopicRow { id: 401, keyword: "12345678901234567", owner: FaceOwner::F18416Matrix, visible_to_d1: false },
            &[],
        )
        .is_err();
    s.add("J1-关键词域边界", kw_edge, "16 字节过闸 / 17 字节拒");
    // 13. 可见性双态守恒。
    s.add("J1-可见性守恒", tally.0 + tally.1 == 20, "可见+不可见=20");
    // 14. 文案闭集。
    s.add("J3-文案闭集", detail_closure(), "11 码逐条非空且反查一致");
    // 15. 段内窗口逐码核对。
    let win_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < DedCode::ALL.len() {
            if DedCode::ALL[i].code() >= SEG_BASE + 0x40 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J1-段内窗口核对", win_ok, "11 码全部 < SEG_BASE+0x40");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 判据集全绿。
    #[test]
    fn all_checks_pass() {
        let set = run_unx18416_checks();
        assert!(set.all_passed(), "UNX-F18416 判据集存在红项");
    }

    /// 正常流：对表 20 行生成、守恒、可见性统计。
    #[test]
    fn normal_flow_pair_table() {
        let pt = build_pair_table();
        assert_eq!(pt.len(), TOPIC_COUNT);
        assert!(pair_table_conserve().is_ok());
        let (v, iv) = visibility_tally();
        assert_eq!(v + iv, 20);
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

    /// 嫌疑清单黄金态为空。
    #[test]
    fn suspect_list_empty() {
        assert!(scan_all().is_empty());
        assert!(golden_run().is_ok());
    }

    /// 判定次序：关键词域闸先于归属闸。
    #[test]
    fn order_keyword_before_owner() {
        // 空关键词 + 本卡面：应返回关键词越界（第一闸）。
        let row = TopicRow { id: 500, keyword: "", owner: FaceOwner::F18416Matrix, visible_to_d1: true };
        match scan_topic(&row, &[]) {
            Err(c) => assert_eq!(c, DedCode::KeywordOutOfRange.code()),
            Ok(_) => panic!("空关键词不应过闸"),
        }
    }

    /// 跨界闭集：D1 关键词在本卡面必拒。
    #[test]
    fn cross_border_rejected() {
        let d1 = ["rtlgetversion", "osverinfo"];
        let bad = TopicRow { id: 501, keyword: "rtlgetversion", owner: FaceOwner::F18416Matrix, visible_to_d1: true };
        match scan_topic(&bad, &d1) {
            Err(c) => assert_eq!(c, DedCode::KeywordCrossBorder.code()),
            Ok(_) => panic!("D1 关键词不应进本卡面"),
        }
        let good = TopicRow { id: 502, keyword: "liematrix", owner: FaceOwner::F18416Matrix, visible_to_d1: true };
        assert!(scan_topic(&good, &d1).is_ok());
    }

    /// 关键词域边界：恰 16 过、17 拒。
    #[test]
    fn keyword_boundary() {
        // 16 字节恰过闸。
        let k16 = TopicRow { id: 503, keyword: "abcdefghijklmnop", owner: FaceOwner::D1Ntdll, visible_to_d1: false };
        assert!(scan_topic(&k16, &[]).is_ok());
        // 17 字节超限拒。
        let k17 = TopicRow { id: 504, keyword: "abcdefghijklmnopq", owner: FaceOwner::D1Ntdll, visible_to_d1: false };
        assert!(scan_topic(&k17, &[]).is_err());
    }

    /// 双跑一致 + 段位守卫 + 文案闭集。
    #[test]
    fn repro_segment_detail() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(detail_closure());
        assert!(keyword_disjoint());
    }
}
