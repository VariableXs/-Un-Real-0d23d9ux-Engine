//! UNX-F50052 水印样本页（×6 档样本入账）（0x432a0 段）
//!
//! 锚点：UNX-F50052-J1 样本哈希账可复跑；判据锚定 §AI-63 §3 判据主轴。
//!
//! 结构清单：
//! * 档位注册表 `Tier`：六档载荷容量 / 标记配额 / 页配额。三张表都是推导值，
//!   不是拍脑袋常数——载荷按档翻倍、配额按档反向收紧，理由写在每个函数上；
//! * 样本页 `SamplePage`：id + 档 + 载荷 + 标记位；`page_digest` 给内容摘要；
//! * 哈希链账本 `SampleLedger`：每条入账携带前一条链值，`verify_chain` 可脱离
//!   账本本体离线重算——「可复跑」= 同一批页任意时刻重放都得到逐位相同的
//!   头哈希与逐条链值；
//! * 注入核 `injection_core`：空载荷 / 超长 / 标记超限 / 重复 id / 档配额 /
//!   账满 / 链篡改 / 序号乱序 / 非法档码 九类坏样本，逐类断言具体诊断码。
//!
//! 错误纪律：诊断码闭集 `0x432a0 + 1..=9`，九条全部被真实构造；内核路径零
//! panic——所有可失败入口返回 `Result<_, WmError>`，Vec 访问走 `get`。

use crate::alloc::format;
use crate::alloc::string::ToString;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x432a0 独占 0x20；本组其余模块从 0x432c0 起顺序占用）。
pub const SEG_BASE: u32 = 0x432a0;

pub const E_EMPTY: u16 = 1;
pub const E_LEN: u16 = 2;
pub const E_MARK: u16 = 3;
pub const E_DUP: u16 = 4;
pub const E_QUOTA: u16 = 5;
pub const E_FULL: u16 = 6;
pub const E_CHAIN: u16 = 7;
pub const E_SEQ: u16 = 8;
pub const E_TIER: u16 = 9;

/// 样本页入账错误。`code()` 返回 `SEG_BASE + n`，与全仓诊断码闭集一致。
///
/// `Chain` 携带出错的条目下标而不是裸 `u32`：审计时需要定位到「第几条开始
/// 对不上」，否则只能全账重扫，定位成本与账长成正比。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmError {
    Empty,
    Len,
    Mark,
    Dup,
    Quota,
    Full,
    Chain(usize),
    Seq,
    Tier(u8),
}

impl WmError {
    pub fn code(self) -> u32 {
        let n = match self {
            WmError::Empty => E_EMPTY,
            WmError::Len => E_LEN,
            WmError::Mark => E_MARK,
            WmError::Dup => E_DUP,
            WmError::Quota => E_QUOTA,
            WmError::Full => E_FULL,
            WmError::Chain(_) => E_CHAIN,
            WmError::Seq => E_SEQ,
            WmError::Tier(_) => E_TIER,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            WmError::Empty => "sample page payload is empty",
            WmError::Len => "sample payload exceeds tier capacity",
            WmError::Mark => "mark count exceeds tier quota",
            WmError::Dup => "sample id already booked",
            WmError::Quota => "tier page quota exhausted",
            WmError::Full => "sample ledger is full",
            WmError::Chain(_) => "hash chain divergence",
            WmError::Seq => "ledger sequence is not contiguous",
            WmError::Tier(_) => "tier code outside 0..6",
        }
    }
}

pub const TIER_COUNT: usize = 6;

/// FNV-1a 64 位基数与质数（取自 Fowler–Noll–Vo 参考实现）。
const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 六档样本页。档位越高，单页承载的水印载荷越大、每档允许的页数越少。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    T0,
    T1,
    T2,
    T3,
    T4,
    T5,
}

impl Tier {
    pub const ALL: [Tier; TIER_COUNT] = [
        Tier::T0,
        Tier::T1,
        Tier::T2,
        Tier::T3,
        Tier::T4,
        Tier::T5,
    ];

    pub fn index(self) -> usize {
        match self {
            Tier::T0 => 0,
            Tier::T1 => 1,
            Tier::T2 => 2,
            Tier::T3 => 3,
            Tier::T4 => 4,
            Tier::T5 => 5,
        }
    }

    pub fn from_index(i: usize) -> Option<Tier> {
        match i {
            0 => Some(Tier::T0),
            1 => Some(Tier::T1),
            2 => Some(Tier::T2),
            3 => Some(Tier::T3),
            4 => Some(Tier::T4),
            5 => Some(Tier::T5),
            _ => None,
        }
    }

    /// 每页载荷上限：64B 起、逐档翻倍（64/128/256/512/1024/2048）。
    /// 用翻倍而不是线性：高档样本必须能完整复刻低档样本的多份副本，
    /// 线性增长会让 T5 放不下"一份低档页 × 档数"的最小冗余布局。
    pub fn payload_cap(self) -> usize {
        64usize << self.index()
    }

    /// 每页标记位配额：4 + 4×档（4/8/12/16/20/24）。
    /// 标记是校验位，随载荷一起增长；固定 4 位则高档页的校验覆盖率会掉到
    /// 1/24 以下，篡改检出率随之塌方。
    pub fn mark_quota(self) -> usize {
        4 + 4 * self.index()
    }

    /// 每档页配额：8/6/5/4/3/2。高档样本稀缺且算力贵，配额反向收紧；
    /// 同时保证六档各入一页时低档不会把账本容量吃光（8+6+5+4+3+2 = 28）。
    pub fn page_quota(self) -> usize {
        const Q: [usize; TIER_COUNT] = [8, 6, 5, 4, 3, 2];
        Q[self.index()]
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::T0 => "T0-coarse",
            Tier::T1 => "T1-64",
            Tier::T2 => "T2-256",
            Tier::T3 => "T3-512",
            Tier::T4 => "T4-1k",
            Tier::T5 => "T5-2k",
        }
    }
}

/// 档位码 → 档位。离线导入账本时档位以 u8 落盘，越界码必须显式拒绝，
/// 不能按 `% 6` 回绕——回绕会把一份 T5 样本悄悄记成 T0。
pub fn tier_from_code(code: u8) -> Result<Tier, WmError> {
    match Tier::from_index(code as usize) {
        Some(t) => Ok(t),
        None => Err(WmError::Tier(code)),
    }
}

/// 一页水印样本：载荷是待嵌入的明文，标记是页内校验位位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SamplePage {
    pub id: u32,
    pub tier: Tier,
    pub payload: Vec<u8>,
    pub marks: Vec<u32>,
}

impl SamplePage {
    pub fn new(id: u32, tier: Tier, payload: Vec<u8>, marks: Vec<u32>) -> SamplePage {
        SamplePage {
            id,
            tier,
            payload,
            marks,
        }
    }
}

/// FNV-1a 风格的单 u64 混入：把 v 的 8 个字节依次并入 h。
///
/// 不用标准库哈希：no_std 下没有 `Hasher`，且账本必须跨构建逐位可复算，
/// 任何随机种子化的哈希都会让 J1 的"可复跑"失去意义。
fn mix_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 样本页内容摘要：id、档、载荷长度、载荷、标记数、标记全部参与。
///
/// 长度字段单独混入的作用是消除拼接歧义：`[1,2] + []` 与 `[1] + [2]` 如果
/// 只混字节会撞同值，账本上就分不清两份不同结构的页。
pub fn page_digest(p: &SamplePage) -> u64 {
    let mut h = FNV_BASIS;
    h = mix_u64(h, p.id as u64);
    h = mix_u64(h, p.tier.index() as u64);
    h = mix_u64(h, p.payload.len() as u64);
    let mut i = 0usize;
    while i < p.payload.len() {
        if let Some(b) = p.payload.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h = mix_u64(h, p.marks.len() as u64);
    let mut k = 0usize;
    while k < p.marks.len() {
        if let Some(m) = p.marks.get(k) {
            h = mix_u64(h, *m as u64);
        }
        k += 1;
    }
    h
}

/// 链步进：`chain(n) = H(chain(n-1) || seq || digest)`。
/// 把 seq 也卷进链，序号的任何改写都会在下一条的链值上炸开，不能靠"重编号"
/// 把乱序账洗成合法账。
pub fn chain_step(prev: u64, seq: u32, digest: u64) -> u64 {
    let h = mix_u64(prev, seq as u64);
    mix_u64(h, digest)
}

/// 一条入账记录。`chain` 是包含本条的链值，`digest` 是页内容摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerEntry {
    pub seq: u32,
    pub id: u32,
    pub tier: Tier,
    pub digest: u64,
    pub chain: u64,
}

/// 样本哈希账。容量在构造时固定，满了显式 `Full`——不静默丢弃，
/// 因为丢一条样本等于丢一段水印覆盖，事后无法从账本反推。
#[derive(Clone, Debug)]
pub struct SampleLedger {
    entries: Vec<LedgerEntry>,
    head: u64,
    tier_pages: [u16; TIER_COUNT],
    cap: usize,
}

impl SampleLedger {
    pub fn new(cap: usize) -> SampleLedger {
        SampleLedger {
            entries: Vec::new(),
            head: FNV_BASIS,
            tier_pages: [0u16; TIER_COUNT],
            cap,
        }
    }

    /// 入账一页样本，返回该条的链值。
    ///
    /// 校验顺序固定为：空 → 超长 → 标记超限 → 账满 → 重复 id → 档配额。
    /// 「结构错误优先于容量错误」：一份本来就非法的页不该因为账满而得到
    /// 一个容量诊断，否则排障会被误导到扩容方向。
    pub fn book(&mut self, page: &SamplePage) -> Result<u64, WmError> {
        if page.payload.is_empty() {
            return Err(WmError::Empty);
        }
        if page.payload.len() > page.tier.payload_cap() {
            return Err(WmError::Len);
        }
        if page.marks.len() > page.tier.mark_quota() {
            return Err(WmError::Mark);
        }
        if self.entries.len() >= self.cap {
            return Err(WmError::Full);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.id == page.id {
                    return Err(WmError::Dup);
                }
            }
            i += 1;
        }
        let idx = page.tier.index();
        let used = match self.tier_pages.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        if used as usize >= page.tier.page_quota() {
            return Err(WmError::Quota);
        }
        let seq = self.entries.len() as u32;
        let digest = page_digest(page);
        let chain = chain_step(self.head, seq, digest);
        self.entries.push(LedgerEntry {
            seq,
            id: page.id,
            tier: page.tier,
            digest,
            chain,
        });
        self.head = chain;
        if let Some(slot) = self.tier_pages.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
        Ok(chain)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn head(&self) -> u64 {
        self.head
    }

    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }

    /// 本档已入账页数。
    pub fn tier_pages(&self, t: Tier) -> u16 {
        match self.tier_pages.get(t.index()) {
            Some(v) => *v,
            None => 0,
        }
    }

    /// 离线复核本账：链值、序号逐条重算，返回合法条目数。
    pub fn verify(&self) -> Result<usize, WmError> {
        verify_chain(&self.entries)
    }
}

/// 脱离账本本体复核一串记录。审计/恢复路径直接复用——落盘账本读回来
/// 之后不必重建 `SampleLedger` 就能验链。
pub fn verify_chain(entries: &[LedgerEntry]) -> Result<usize, WmError> {
    let mut prev = FNV_BASIS;
    let mut i = 0usize;
    while i < entries.len() {
        let e = match entries.get(i) {
            Some(v) => v,
            None => return Err(WmError::Chain(i)),
        };
        if e.seq != i as u32 {
            return Err(WmError::Seq);
        }
        if chain_step(prev, e.seq, e.digest) != e.chain {
            return Err(WmError::Chain(i));
        }
        prev = e.chain;
        i += 1;
    }
    Ok(entries.len())
}

/// 按给定顺序重放一批样本页。J1 的复跑口径：同样的 `pages` 调两次，
/// 返回账本的 `head()` 与每条 `chain` 必须逐位一致。
pub fn replay(pages: &[SamplePage], cap: usize) -> Result<SampleLedger, WmError> {
    let mut l = SampleLedger::new(cap);
    let mut i = 0usize;
    while i < pages.len() {
        if let Some(p) = pages.get(i) {
            l.book(p)?;
        }
        i += 1;
    }
    Ok(l)
}

/// 六档各一页的基准样本集。载荷长度 8/11/14/17/20/23 全部落在 T0 的 64B
/// 容量内，所以这份集合只考验档位登记与链，不考验容量——容量由注入核另测。
pub fn six_tier_probe_pages() -> Vec<SamplePage> {
    let mut out = Vec::with_capacity(TIER_COUNT);
    let mut i = 0usize;
    while i < TIER_COUNT {
        if let Some(t) = Tier::from_index(i) {
            let n = 8 + i * 3;
            let mut payload = Vec::with_capacity(n);
            let mut k = 0usize;
            while k < n {
                payload.push((0x30u8).wrapping_add(k as u8) ^ (i as u8));
                k += 1;
            }
            let mut marks = Vec::with_capacity(3);
            let mut m = 0usize;
            while m < 3 {
                marks.push(0x1000 + (i as u32) * 0x100 + m as u32);
                m += 1;
            }
            out.push(SamplePage::new(0xA000 + i as u32, t, payload, marks));
        }
        i += 1;
    }
    out
}

fn bytes(n: usize, seed: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(seed.wrapping_add(i as u8));
        i += 1;
    }
    v
}

fn marks(n: usize) -> Vec<u32> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(0x100 + i as u32);
        i += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 注入核：九类坏样本必须逐类落到具体诊断码
// ---------------------------------------------------------------------------

/// 返回 (抓住的类数, 总类数)。调用方断言相等——任何一类未被拒都说明
/// 校验链上有一个洞，而洞的位置恰好是下一份坏样本能穿过去的地方。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    // 1 空载荷
    let mut l1 = SampleLedger::new(16);
    if l1
        .book(&SamplePage::new(1, Tier::T0, Vec::new(), Vec::new()))
        .err()
        == Some(WmError::Empty)
    {
        caught += 1;
    }

    // 2 载荷超出本档容量
    let mut l2 = SampleLedger::new(16);
    let over = Tier::T0.payload_cap() + 1;
    if l2
        .book(&SamplePage::new(2, Tier::T0, bytes(over, 7), Vec::new()))
        .err()
        == Some(WmError::Len)
    {
        caught += 1;
    }

    // 3 标记数超配额
    let mut l3 = SampleLedger::new(16);
    let too_many_marks = Tier::T0.mark_quota() + 1;
    if l3
        .book(&SamplePage::new(
            3,
            Tier::T0,
            bytes(4, 1),
            marks(too_many_marks),
        ))
        .err()
        == Some(WmError::Mark)
    {
        caught += 1;
    }

    // 4 重复 id（换档换内容也不行，id 是页身份）
    let mut l4 = SampleLedger::new(16);
    let _ = l4.book(&SamplePage::new(4, Tier::T0, bytes(8, 2), marks(2)));
    if l4
        .book(&SamplePage::new(4, Tier::T1, bytes(8, 3), marks(2)))
        .err()
        == Some(WmError::Dup)
    {
        caught += 1;
    }

    // 5 档配额打穿（T5 配额最小，最省事地构造）
    let mut l5 = SampleLedger::new(64);
    let q = Tier::T5.page_quota();
    let mut k = 0usize;
    while k < q {
        let _ = l5.book(&SamplePage::new(
            100 + k as u32,
            Tier::T5,
            bytes(16, k as u8),
            marks(2),
        ));
        k += 1;
    }
    if l5
        .book(&SamplePage::new(200, Tier::T5, bytes(16, 9), marks(2)))
        .err()
        == Some(WmError::Quota)
    {
        caught += 1;
    }

    // 6 账满
    let mut l6 = SampleLedger::new(2);
    let _ = l6.book(&SamplePage::new(10, Tier::T0, bytes(4, 1), marks(1)));
    let _ = l6.book(&SamplePage::new(11, Tier::T0, bytes(4, 2), marks(1)));
    if l6
        .book(&SamplePage::new(12, Tier::T0, bytes(4, 3), marks(1)))
        .err()
        == Some(WmError::Full)
    {
        caught += 1;
    }

    let pages = six_tier_probe_pages();

    // 7 链值被改写（改第 1 条的 chain）
    match replay(&pages, 16) {
        Ok(led) => {
            let mut e: Vec<LedgerEntry> = Vec::new();
            for x in led.entries() {
                e.push(x.clone());
            }
            if let Some(slot) = e.get_mut(1) {
                slot.chain ^= 0x55;
            }
            if verify_chain(&e).err() == Some(WmError::Chain(1)) {
                caught += 1;
            }
        }
        Err(_) => {}
    }

    // 8 序号被改写（改第 0 条的 seq，链重算会出现空洞）
    match replay(&pages, 16) {
        Ok(led) => {
            let mut e: Vec<LedgerEntry> = Vec::new();
            for x in led.entries() {
                e.push(x.clone());
            }
            if let Some(slot) = e.get_mut(0) {
                slot.seq = 7;
            }
            if verify_chain(&e).err() == Some(WmError::Seq) {
                caught += 1;
            }
        }
        Err(_) => {}
    }

    // 9 非法档码
    if tier_from_code(9).err() == Some(WmError::Tier(9)) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50052_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50052");

    // J1-1 六档各入一页：条目数 = 6，且每档计数恰为 1（×6 档样本入账）
    let pages = six_tier_probe_pages();
    let led = replay(&pages, 32);
    let ok = match &led {
        Ok(l) => {
            let mut all_one = l.len() == TIER_COUNT;
            let mut i = 0usize;
            while i < TIER_COUNT {
                if let Some(t) = Tier::from_index(i) {
                    if l.tier_pages(t) != 1 {
                        all_one = false;
                    }
                }
                i += 1;
            }
            all_one
        }
        Err(_) => false,
    };
    s.add("50052-J1-六档入账", ok, if ok { "" } else { "六档未各记一页" });

    // J1-2 档位表单调：载荷容量严格递增、页配额严格递减、标记配额递增
    let mut cap_inc = true;
    let mut quota_dec = true;
    let mut mark_inc = true;
    let mut i = 1usize;
    while i < TIER_COUNT {
        let a = Tier::from_index(i - 1);
        let b = Tier::from_index(i);
        if let (Some(lo), Some(hi)) = (a, b) {
            if hi.payload_cap() <= lo.payload_cap() {
                cap_inc = false;
            }
            if hi.page_quota() >= lo.page_quota() {
                quota_dec = false;
            }
            if hi.mark_quota() <= lo.mark_quota() {
                mark_inc = false;
            }
        }
        i += 1;
    }
    let ok = cap_inc && quota_dec && mark_inc;
    let det = format!("cap_inc={} quota_dec={} mark_inc={}", cap_inc, quota_dec, mark_inc);
    s.add_dyn("50052-J1-档表单调", ok, &det);

    // J1-3 空载荷必拒（具体码）
    let mut l = SampleLedger::new(8);
    let ok = l
        .book(&SamplePage::new(1, Tier::T0, Vec::new(), Vec::new()))
        .err()
        == Some(WmError::Empty);
    s.add("50052-J1-空样本必拒", ok, if ok { "" } else { "空载荷被放行" });

    // J1-4 超长载荷必拒
    let mut l = SampleLedger::new(8);
    let over = Tier::T0.payload_cap() + 1;
    let ok = l
        .book(&SamplePage::new(2, Tier::T0, bytes(over, 1), Vec::new()))
        .err()
        == Some(WmError::Len);
    s.add("50052-J1-超长必拒", ok, if ok { "" } else { "超长载荷被放行" });

    // J1-5 重复 id 必拒
    let mut l = SampleLedger::new(8);
    let _ = l.book(&SamplePage::new(5, Tier::T0, bytes(4, 1), marks(1)));
    let ok = l
        .book(&SamplePage::new(5, Tier::T2, bytes(4, 2), marks(1)))
        .err()
        == Some(WmError::Dup);
    s.add("50052-J1-重复id必拒", ok, if ok { "" } else { "重复 id 被放行" });

    // J1-6 档配额必拒（T5 = 2 页）
    let mut l = SampleLedger::new(64);
    let q = Tier::T5.page_quota();
    let mut k = 0usize;
    while k < q {
        let _ = l.book(&SamplePage::new(
            300 + k as u32,
            Tier::T5,
            bytes(8, k as u8),
            marks(1),
        ));
        k += 1;
    }
    let ok = l
        .book(&SamplePage::new(399, Tier::T5, bytes(8, 7), marks(1)))
        .err()
        == Some(WmError::Quota);
    s.add("50052-J1-档配额必拒", ok, if ok { "" } else { "档配额被打穿" });

    // J1-7 账满必拒（容量 2）
    let mut l = SampleLedger::new(2);
    let _ = l.book(&SamplePage::new(41, Tier::T0, bytes(4, 1), marks(1)));
    let _ = l.book(&SamplePage::new(42, Tier::T0, bytes(4, 2), marks(1)));
    let ok = l
        .book(&SamplePage::new(43, Tier::T0, bytes(4, 3), marks(1)))
        .err()
        == Some(WmError::Full);
    s.add("50052-J1-账满必拒", ok, if ok { "" } else { "账满仍入账" });

    // J1-8 标记超限必拒
    let mut l = SampleLedger::new(8);
    let ok = l
        .book(&SamplePage::new(
            51,
            Tier::T1,
            bytes(4, 1),
            marks(Tier::T1.mark_quota() + 1),
        ))
        .err()
        == Some(WmError::Mark);
    s.add("50052-J1-标记超限必拒", ok, if ok { "" } else { "标记超限被放行" });

    // J1-9 链篡改必抓，且定位到具体下标
    let base = replay(&pages, 32);
    let mut caught_at = usize::MAX;
    if let Ok(l0) = &base {
        let mut e: Vec<LedgerEntry> = Vec::new();
        for x in l0.entries() {
            e.push(x.clone());
        }
        if let Some(slot) = e.get_mut(3) {
            slot.digest ^= 1;
        }
        if let Err(WmError::Chain(at)) = verify_chain(&e) {
            caught_at = at;
        }
    }
    let ok = caught_at == 3;
    let det = format!("caught_at={}", caught_at);
    s.add_dyn("50052-J1-链篡改定位", ok, &det);

    // J1-10 复跑逐位一致：同一批页重放两次，头哈希与每条链值全部相等
    let again = replay(&pages, 32);
    let ok = match (&base, &again) {
        (Ok(a), Ok(b)) => {
            let mut same = a.head() == b.head() && a.len() == b.len();
            let mut i = 0usize;
            while i < a.len() {
                let (ea, eb) = (a.entries().get(i), b.entries().get(i));
                match (ea, eb) {
                    (Some(x), Some(y)) => {
                        if x.chain != y.chain || x.digest != y.digest {
                            same = false;
                        }
                    }
                    _ => same = false,
                }
                i += 1;
            }
            same
        }
        _ => false,
    };
    let det = match &base {
        Ok(a) => format!("head={:016x} n={}", a.head(), a.len()),
        Err(_) => "replay failed".to_string(),
    };
    s.add_dyn("50052-J1-复跑逐位一致", ok, &det);

    // J1-11 顺序敏感但两序皆合法：链绑定顺序，换序后头不同、两账都验得过
    let mut rev: Vec<SamplePage> = Vec::new();
    let mut i = pages.len();
    while i > 0 {
        i -= 1;
        if let Some(p) = pages.get(i) {
            rev.push(p.clone());
        }
    }
    let rrev = replay(&rev, 32);
    let ok = match (&base, &rrev) {
        (Ok(a), Ok(b)) => a.head() != b.head() && a.verify().is_ok() && b.verify().is_ok(),
        _ => false,
    };
    s.add("50052-J1-顺序敏感", ok, if ok { "" } else { "换序后链值未变或验链失败" });

    // J1-12 摘要雪崩：载荷单 bit 翻转后摘要汉明距离 ≥ 8
    let a = SamplePage::new(61, Tier::T1, bytes(16, 9), marks(2));
    let mut b = a.clone();
    if let Some(x) = b.payload.get_mut(0) {
        *x ^= 1;
    }
    let dist = (page_digest(&a) ^ page_digest(&b)).count_ones();
    let ok = dist >= 8;
    let det = format!("hamming={}", dist);
    s.add_dyn("50052-J1-摘要雪崩", ok, &det);
    // 同内容、独立构造的两页摘要必须相等（摘要只依赖内容，不依赖对象身份）
    let a2 = SamplePage::new(61, Tier::T1, bytes(16, 9), marks(2));
    let stable = page_digest(&a) == page_digest(&a2);

    // J1-13 配额守恒：六档计数之和 = 账条目数
    let ok = match &base {
        Ok(l0) => {
            let mut sum = 0u32;
            let mut i = 0usize;
            while i < TIER_COUNT {
                if let Some(t) = Tier::from_index(i) {
                    sum += l0.tier_pages(t) as u32;
                }
                i += 1;
            }
            sum as usize == l0.len()
        }
        Err(_) => false,
    };
    s.add("50052-J1-配额守恒", ok && stable, if ok && stable { "" } else { "档计数与条目数不符" });

    // J1-14 注入核九类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50052-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50052_checks_green() {
        let set = run_unx50052_checks();
        assert!(set.all_passed(), "UNX-F50052 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50052_replay_is_bitwise_stable() {
        let pages = six_tier_probe_pages();
        let a = replay(&pages, 32);
        let b = replay(&pages, 32);
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert_eq!(x.head(), y.head());
                assert_eq!(x.len(), y.len());
                assert!(x.verify().is_ok());
                assert!(y.verify().is_ok());
            }
            _ => panic!("replay must succeed for probe pages"),
        }
    }
}
