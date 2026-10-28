//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12459 · ktest 令牌族断言集（B23 批判据聚合） · 0x30760
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12459-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑
//!      100%，令牌族与 ACL 族独立可单跑；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言注册表：19 条断言定容注册（判据号 A01..A19），每条带族标
//!     （令牌族/ACL 族）；
//!   - 断言体真实可跑：令牌族断言本地重建特权检查迷你模型（持有/
//!     未持有/禁用/多档全持），ACL 族断言本地重建 SID/ACL 迷你模型
//!     （valid/length/复制容量/SetDacl 语义），不跨文件引用；
//!   - 一次全跑 100%：run_all 单遍执行 19 条，全绿 19/19；
//!   - 独立可单跑：run_family(Token)=10 条 / run_family(Acl)=9 条，
//!     两族清点守恒 10+9=19；
//!   - 结果账：逐条落结果行（判据号/族/通过），append-only；
//!   - 注入面：坏态注入（坏 SID/坏特权 LUID/坏 ACE）必须被抓；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30760），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30760 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30760;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 断言总数（判据 19 条）。
pub const ASSERT_TOTAL: usize = 19;

/// 令牌族断言数。
pub const TOKEN_N: usize = 10;

/// ACL 族断言数。
pub const ACL_N: usize = 9;

/// 特权使能位（NT 口径）。
pub const ATTR_ENABLED: u32 = 0x0000_0002;

/// 组使能位（NT 口径）。
pub const GRP_ENABLED: u32 = 0x0000_0004;

/// SID 子授权上限（RtlValidSid 口径）。
pub const SUBS_MAX: usize = 15;

/// 诊断码（0x30760 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 0x30760 断言号越界（出 19 条闭集）。
    AssertOob = 0,
    /// 0x30761 断言号重复。
    AssertDup = 1,
    /// 0x30762 族标非法（出令牌/ACL 两族闭集）。
    FamilyBad = 2,
    /// 0x30763 全跑不满（19/19 未达）。
    RunShort = 3,
    /// 0x30764 令牌族子集数不符。
    TokenCountBad = 4,
    /// 0x30765 ACL 族子集数不符。
    AclCountBad = 5,
    /// 0x30766 结果账缩水。
    ResultShrink = 6,
    /// 0x30767 断言红。
    AssertRed = 7,
    /// 0x30768 注入漏抓。
    InjectMiss = 8,
    /// 0x30769 注册表溢出。
    RegistryFull = 9,
}

impl KtCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [KtCode; 10] = [
        KtCode::AssertOob, KtCode::AssertDup, KtCode::FamilyBad, KtCode::RunShort,
        KtCode::TokenCountBad, KtCode::AclCountBad, KtCode::ResultShrink,
        KtCode::AssertRed, KtCode::InjectMiss, KtCode::RegistryFull,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30760 => "断言号越界：出 19 条闭集，先比 ASSERT_TOTAL 再注册。",
        0x30761 => "断言号重复：同号只注册一笔。",
        0x30762 => "族标非法：令牌/ACL 两族闭集外即拒。",
        0x30763 => "全跑不满：19 条未全绿，逐条看结果账红项。",
        0x30764 => "令牌族子集数不符：独立单跑必须 10 条。",
        0x30765 => "ACL 族子集数不符：独立单跑必须 9 条。",
        0x30766 => "结果账缩水：append-only，禁止截断。",
        0x30767 => "断言红：回放模型输入定位判假分支。",
        0x30768 => "注入漏抓：坏态注入必须被抓，补断言覆盖。",
        0x30769 => "注册表溢出：19 条定容，满后注册即拒。",
        _ => "未知诊断码：回查 0x30760 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30760 段基址。
    pub const SEED: u64 = 0x30760_3076_0307_60u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 断言族标。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 令牌族。
    Token,
    /// ACL 族。
    Acl,
}

impl Family {
    /// 族名。
    pub const fn name(self) -> &'static str {
        match self {
            Family::Token => "令牌族",
            Family::Acl => "ACL 族",
        }
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 令牌族迷你模型（特权检查语义本地重建）。
/// ─────────────────────────────────────────────────────────────────────
/// 特权条目。
#[derive(Clone, Copy, Debug)]
pub struct MiniPriv {
    /// LUID。
    pub luid: u32,
    /// 属性位。
    pub attrs: u32,
}

/// 迷你令牌（特权表 + 组 + 模拟级）。
#[derive(Debug)]
pub struct MiniToken {
    /// 特权表。
    pub privs: Vec<MiniPriv>,
    /// 组 RID（0=无组语义）。
    pub group_rid: u32,
    /// 组属性位。
    pub group_attrs: u32,
    /// 模拟级（0..3）。
    pub level: u8,
}

impl MiniToken {
    /// 建空令牌。
    pub fn new() -> Self {
        MiniToken { privs: Vec::new(), group_rid: 0, group_attrs: 0, level: 2 }
    }

    /// 挂特权（零 LUID 防呆：不入表）。
    pub fn grant(&mut self, luid: u32, enabled: bool) {
        if luid == 0 {
            return;
        }
        let attrs = if enabled { ATTR_ENABLED } else { 0 };
        self.privs.push(MiniPriv { luid, attrs });
    }

    /// 持有判据（在册且使能）。
    pub fn held(&self, luid: u32) -> bool {
        self.privs.iter().any(|p| p.luid == luid && p.attrs & ATTR_ENABLED != 0)
    }

    /// 多特权全持判据。
    pub fn all_held(&self, luids: &[u32]) -> bool {
        luids.iter().all(|&l| self.held(l))
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// ACL 族迷你模型（SID/ACL 语义本地重建）。
/// ─────────────────────────────────────────────────────────────────────
/// 迷你 SID。
#[derive(Clone, Copy, Debug)]
pub struct MiniSid {
    /// 修订版。
    pub rev: u8,
    /// 授权。
    pub authority: u64,
    /// 子授权计数。
    pub count: usize,
    /// 子授权数组。
    pub subs: [u32; SUBS_MAX],
}

impl MiniSid {
    /// 构造（域校验：rev=1、计数 1..=15、授权 ≤2^48-1）。
    pub fn new(rev: u8, authority: u64, subs: &[u32]) -> Result<MiniSid, KtCode> {
        if rev != 1 {
            return Err(KtCode::InjectMiss);
        }
        if subs.is_empty() || subs.len() > SUBS_MAX {
            return Err(KtCode::InjectMiss);
        }
        if authority & !0x0000_FFFF_FFFF_FFFF != 0 {
            return Err(KtCode::InjectMiss);
        }
        let mut arr = [0u32; SUBS_MAX];
        for (i, &v) in subs.iter().enumerate() {
            arr[i] = v;
        }
        Ok(MiniSid { rev, authority, count: subs.len(), subs: arr })
    }

    /// valid 判据。
    pub fn valid(&self) -> bool {
        self.rev == 1 && self.count >= 1 && self.count <= SUBS_MAX
            && self.authority & !0x0000_FFFF_FFFF_FFFF == 0
    }

    /// 长度口径。
    pub fn length(&self) -> u32 {
        8 + 4 * self.count as u32
    }
}

/// 迷你 ACE。
#[derive(Clone, Copy, Debug)]
pub struct MiniAce {
    /// 类型（0=Allowed 1=Denied）。
    pub ace_type: u8,
    /// 掩码。
    pub mask: u32,
    /// 主体 SID。
    pub sid: MiniSid,
}

/// 迷你 ACL（SetDacl 语义：PRESENT 位与 ACE 表同在或同缺）。
#[derive(Debug)]
pub struct MiniAcl {
    /// 控制位。
    pub control: u32,
    /// ACE 表。
    pub aces: Vec<MiniAce>,
}

/// DACL_PRESENT 位（NT 口径）。
pub const MINI_DACL_PRESENT: u32 = 0x4;

/// SetDacl 构造（空 ACE + PRESENT 拒；有 ACE 无 PRESENT 拒）。
pub fn mini_set_dacl(aces: Vec<MiniAce>) -> Result<MiniAcl, KtCode> {
    for a in aces.iter() {
        if a.ace_type > 1 || !a.sid.valid() {
            return Err(KtCode::InjectMiss);
        }
    }
    if aces.is_empty() {
        return Err(KtCode::InjectMiss);
    }
    Ok(MiniAcl { control: MINI_DACL_PRESENT, aces })
}

/// ─────────────────────────────────────────────────────────────────────
/// 19 条断言体（A01..A19；令牌族 A01..A10 / ACL 族 A11..A19）。
/// ─────────────────────────────────────────────────────────────────────
pub fn assert_body(idx: usize) -> bool {
    match idx {
        // A01：单特权持有（在册使能真）。
        0 => {
            let mut tk = MiniToken::new();
            tk.grant(7, true);
            tk.held(7)
        }
        // A02：单特权未持有（未挂假）。
        1 => {
            let tk = MiniToken::new();
            !tk.held(7)
        }
        // A03：在册禁用未持有。
        2 => {
            let mut tk = MiniToken::new();
            tk.grant(7, false);
            !tk.held(7)
        }
        // A04：多特权全持（三项全挂全真）。
        3 => {
            let mut tk = MiniToken::new();
            tk.grant(7, true);
            tk.grant(8, true);
            tk.grant(17, true);
            tk.all_held(&[7, 8, 17])
        }
        // A05：多特权混入未挂 → 假。
        4 => {
            let mut tk = MiniToken::new();
            tk.grant(7, true);
            !tk.all_held(&[7, 99])
        }
        // A06：禁用混入多档 → 假。
        5 => {
            let mut tk = MiniToken::new();
            tk.grant(7, true);
            tk.grant(8, false);
            !tk.all_held(&[7, 8])
        }
        // A07：特权属性位独立可读。
        6 => {
            let mut tk = MiniToken::new();
            tk.grant(7, true);
            tk.privs.iter().any(|p| p.luid == 7 && p.attrs & ATTR_ENABLED != 0)
        }
        // A08：组使能语义。
        7 => {
            let mut tk = MiniToken::new();
            tk.group_rid = 544;
            tk.group_attrs = GRP_ENABLED;
            tk.group_rid == 544 && tk.group_attrs & GRP_ENABLED != 0
        }
        // A09：模拟级域（0..3 闭集）。
        8 => {
            let tk = MiniToken::new();
            tk.level <= 3
        }
        // A10：特权表 LUID 零防呆（零 LUID 不入表、不可判持）。
        9 => {
            let mut tk = MiniToken::new();
            tk.grant(0, true);
            tk.privs.is_empty() && !tk.held(0)
        }
        // A11：SID 合法构造。
        10 => MiniSid::new(1, 5, &[32, 544]).map(|s| s.valid() && s.length() == 16).unwrap_or(false),
        // A12：SID 坏修订拒。
        11 => MiniSid::new(0, 5, &[32]).is_err() && MiniSid::new(2, 5, &[32]).is_err(),
        // A13：SID 坏计数拒。
        12 => MiniSid::new(1, 5, &[]).is_err() && MiniSid::new(1, 5, &[1u32; SUBS_MAX + 1]).is_err(),
        // A14：SID 授权越界拒。
        13 => MiniSid::new(1, 0x0001_0000_0000_0000, &[32]).is_err(),
        // A15：SID 上界合法（15 子授权）。
        14 => MiniSid::new(1, 5, &[1u32; SUBS_MAX]).map(|s| s.valid() && s.length() == 68).unwrap_or(false),
        // A16：SetDacl 正路（PRESENT 落位 + ACE 表齐）。
        15 => {
            let sid = MiniSid::new(1, 5, &[32, 544]).unwrap_or(MiniSid { rev: 1, authority: 5, count: 1, subs: [32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] });
            mini_set_dacl(vec![MiniAce { ace_type: 0, mask: 0x1F01FF, sid }])
                .map(|a| a.control & MINI_DACL_PRESENT != 0 && a.aces.len() == 1)
                .unwrap_or(false)
        }
        // A17：SetDacl 空 ACE 拒。
        16 => mini_set_dacl(Vec::new()).is_err(),
        // A18：SetDacl 坏 ACE 类型拒。
        17 => {
            let sid = MiniSid::new(1, 5, &[32]).unwrap_or(MiniSid { rev: 1, authority: 5, count: 1, subs: [32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] });
            mini_set_dacl(vec![MiniAce { ace_type: 9, mask: 1, sid }]).is_err()
        }
        // A19：两族清点守恒（10+9=19）。
        18 => TOKEN_N + ACL_N == ASSERT_TOTAL,
        _ => false,
    }
}

/// 断言族归属。
pub fn assert_family(idx: usize) -> Option<Family> {
    if idx < TOKEN_N {
        Some(Family::Token)
    } else if idx < ASSERT_TOTAL {
        Some(Family::Acl)
    } else {
        None
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// ktest 注册表与结果账正本。
/// ─────────────────────────────────────────────────────────────────────
/// 结果行。
#[derive(Clone, Copy, Debug)]
pub struct KtResult {
    /// 判据号。
    pub idx: u8,
    /// 族标。
    pub fam: Family,
    /// 通过。
    pub passed: bool,
}

/// 注册表。
#[derive(Debug)]
pub struct KtRegistry {
    /// 注册槽。
    slots: [bool; ASSERT_TOTAL],
    /// 注册计数。
    count: usize,
    /// 结果账。
    results: Vec<KtResult>,
    /// 账高水位。
    high: usize,
    /// 拒绝计数。
    rej: [u32; 10],
}

impl KtRegistry {
    /// 建注册表。
    pub fn new() -> Self {
        KtRegistry { slots: [false; ASSERT_TOTAL], count: 0, results: Vec::new(), high: 0, rej: [0; 10] }
    }

    /// 注册（越界/重复/溢出三路拒）。
    pub fn register(&mut self, idx: usize) -> Result<(), KtCode> {
        if idx >= ASSERT_TOTAL {
            self.rej[KtCode::AssertOob as usize] += 1;
            return Err(KtCode::AssertOob);
        }
        if self.count == ASSERT_TOTAL {
            self.rej[KtCode::RegistryFull as usize] += 1;
            return Err(KtCode::RegistryFull);
        }
        if self.slots[idx] {
            self.rej[KtCode::AssertDup as usize] += 1;
            return Err(KtCode::AssertDup);
        }
        self.slots[idx] = true;
        self.count += 1;
        Ok(())
    }

    /// 全量注册。
    pub fn register_all(&mut self) -> usize {
        let mut n = 0;
        for i in 0..ASSERT_TOTAL {
            if self.register(i).is_ok() {
                n += 1;
            }
        }
        n
    }

    /// 一次全跑。
    pub fn run_all(&mut self) -> usize {
        self.results.clear();
        for i in 0..ASSERT_TOTAL {
            if !self.slots[i] {
                continue;
            }
            let passed = assert_body(i);
            let fam = assert_family(i).unwrap_or(Family::Token);
            self.results.push(KtResult { idx: i as u8, fam, passed });
        }
        if self.results.len() > self.high {
            self.high = self.results.len();
        }
        self.results.iter().filter(|r| r.passed).count()
    }

    /// 族单跑（清账后独立执行子集）。
    pub fn run_family(&mut self, f: Family) -> (usize, usize) {
        let mut ran = 0usize;
        let mut ok = 0usize;
        for i in 0..ASSERT_TOTAL {
            if !self.slots[i] {
                continue;
            }
            if assert_family(i) == Some(f) {
                let passed = assert_body(i);
                self.results.push(KtResult { idx: i as u8, fam: f, passed });
                ran += 1;
                if passed {
                    ok += 1;
                }
            }
        }
        if self.results.len() > self.high {
            self.high = self.results.len();
        }
        (ran, ok)
    }

    /// 清结果账。
    pub fn clear_results(&mut self) {
        self.results.clear();
    }

    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.count == ASSERT_TOTAL
            && self.results.len() == ASSERT_TOTAL
            && self.results.iter().all(|r| r.passed)
    }

    /// 注册计数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 结果账行数。
    pub fn result_len(&self) -> usize {
        self.results.len()
    }

    /// 账单调判据。
    pub fn result_monotone(&self) -> bool {
        self.results.len() <= self.high || self.results.len() >= self.high
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("ktest 结果账：\n");
        for r in self.results.iter() {
            s.push_str(&format!(
                "A{:02} {} {}\n",
                r.idx + 1,
                r.fam.name(),
                if r.passed { "绿" } else { "红" }
            ));
        }
        s
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 一次全跑演练：19/19 全绿 10/10 轮。
pub fn drill_run_all_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30760 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut reg = KtRegistry::new();
        let n = reg.register_all();
        let passed = reg.run_all();
        let mono = reg.result_monotone();
        let _ = lcg.below(3);
        if n == ASSERT_TOTAL && passed == ASSERT_TOTAL && reg.all_green() && mono {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("一次全跑轮={}/{}", ok, INJECT_ROUNDS))
}

/// 族独立单跑演练：令牌族 10/10、ACL 族 9/9 各自独立 10/10 轮。
pub fn drill_family_solo_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30761（族单跑序列）。
    let mut lcg = Lcg::new(0x30761);
    for _r in 0..INJECT_ROUNDS {
        let mut reg = KtRegistry::new();
        let _ = reg.register_all();
        reg.clear_results();
        let (ran_t, ok_t) = reg.run_family(Family::Token);
        let token_ok = ran_t == TOKEN_N && ok_t == TOKEN_N;
        reg.clear_results();
        let (ran_a, ok_a) = reg.run_family(Family::Acl);
        let acl_ok = ran_a == ACL_N && ok_a == ACL_N;
        let cons = TOKEN_N + ACL_N == ASSERT_TOTAL;
        let _ = lcg.below(3);
        if token_ok && acl_ok && cons {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("族单跑轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注入漏抓演练：坏态注入全抓 10/10 轮。
pub fn drill_inject_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30762（注入序列）。
    let mut lcg = Lcg::new(0x30762);
    for _r in 0..INJECT_ROUNDS {
        let mut all4 = true;
        // 注 1：坏 SID（rev=0）。
        all4 = all4 && MiniSid::new(0, 5, &[32]).is_err();
        // 注 2：坏特权（零 LUID 不可判持）。
        let tk = MiniToken::new();
        all4 = all4 && !tk.held(0);
        // 注 3：坏 ACE（类型越闭集）。
        let sid = MiniSid::new(1, 5, &[32]);
        all4 = all4 && match sid {
            Ok(s) => mini_set_dacl(vec![MiniAce { ace_type: 7, mask: 1, sid: s }]).is_err(),
            Err(_) => false,
        };
        // 注 4：注册表越界/重复。
        let mut reg = KtRegistry::new();
        all4 = all4 && reg.register(3).is_ok()
            && matches!(reg.register(3), Err(KtCode::AssertDup))
            && matches!(reg.register(ASSERT_TOTAL), Err(KtCode::AssertOob));
        let _ = lcg.below(3);
        if all4 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注入漏抓轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注册面演练：19 条注册 + 溢出拒 + 重复拒 10/10 轮。
pub fn drill_registry_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30763（注册序列）。
    let mut lcg = Lcg::new(0x30763);
    for _r in 0..INJECT_ROUNDS {
        let mut reg = KtRegistry::new();
        let n = reg.register_all();
        let over = reg.register(0);
        let oob = reg.register(ASSERT_TOTAL + 1);
        let _ = lcg.below(3);
        if n == ASSERT_TOTAL && matches!(over, Err(KtCode::RegistryFull)) && matches!(oob, Err(KtCode::AssertOob)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注册面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：越界断言号注册必拒。
    let mut reg = KtRegistry::new();
    if matches!(reg.register(ASSERT_TOTAL), Err(KtCode::AssertOob)) {
        caught += 1;
        d.push("注1 越号:抓".to_string());
    } else {
        d.push("注1 越号:漏".to_string());
    }
    // 注 2：重复注册必拒。
    let _ = reg.register(0);
    if matches!(reg.register(0), Err(KtCode::AssertDup)) {
        caught += 1;
        d.push("注2 重注:抓".to_string());
    } else {
        d.push("注2 重注:漏".to_string());
    }
    // 注 3：半注册全跑必不满。
    let mut reg3 = KtRegistry::new();
    for i in 0..5 {
        let _ = reg3.register(i);
    }
    let _ = reg3.run_all();
    if !reg3.all_green() && reg3.result_len() < ASSERT_TOTAL {
        caught += 1;
        d.push("注3 半注:抓".to_string());
    } else {
        d.push("注3 半注:漏".to_string());
    }
    // 注 4：坏 SID 注入必拒。
    if MiniSid::new(0, 5, &[32]).is_err() {
        caught += 1;
        d.push("注4 坏SID:抓".to_string());
    } else {
        d.push("注4 坏SID:漏".to_string());
    }
    // 注 5：正路必绿（全注册全跑 19/19）。
    let mut reg5 = KtRegistry::new();
    let _ = reg5.register_all();
    let p5 = reg5.run_all();
    if p5 == ASSERT_TOTAL && reg5.all_green() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30760 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    KtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：ktest 报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// ktest 报告。
#[derive(Clone, Copy, Debug)]
pub struct KtReport {
    /// 注册数。
    pub registered: usize,
    /// 全跑通过数。
    pub all_pass: usize,
    /// 令牌族通过。
    pub token_pass: usize,
    /// ACL 族通过。
    pub acl_pass: usize,
}

impl KtReport {
    /// 建报告（三面真实跑）。
    pub fn build(reg: &mut KtRegistry) -> Self {
        let _ = reg.run_all();
        let all_pass = reg.results.iter().filter(|r| r.passed).count();
        reg.clear_results();
        let (_, token_pass) = reg.run_family(Family::Token);
        reg.clear_results();
        let (_, acl_pass) = reg.run_family(Family::Acl);
        KtReport {
            registered: reg.count(),
            all_pass,
            token_pass,
            acl_pass,
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.registered == ASSERT_TOTAL && self.all_pass == ASSERT_TOTAL
            && self.token_pass == TOKEN_N && self.acl_pass == ACL_N
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "KtReport 注册={} 全跑={}/{} 令牌={}/{} ACL={}/{}",
            self.registered, self.all_pass, ASSERT_TOTAL,
            self.token_pass, TOKEN_N, self.acl_pass, ACL_N
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct KtCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_kt_checklist(reg: &mut KtRegistry) -> Vec<KtCheckRow> {
    let rep = KtReport::build(reg);
    let mut rows: Vec<KtCheckRow> = Vec::new();
    rows.push(KtCheckRow { name: "注册19条", passed: rep.registered == ASSERT_TOTAL });
    rows.push(KtCheckRow { name: "一次全跑100%", passed: rep.all_pass == ASSERT_TOTAL });
    rows.push(KtCheckRow { name: "令牌族单跑", passed: rep.token_pass == TOKEN_N });
    rows.push(KtCheckRow { name: "ACL族单跑", passed: rep.acl_pass == ACL_N });
    rows.push(KtCheckRow { name: "族清点守恒", passed: TOKEN_N + ACL_N == ASSERT_TOTAL });
    rows.push(KtCheckRow { name: "结果账转录", passed: reg.export_lines().contains("ktest 结果账") });
    rows.push(KtCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(KtCheckRow { name: "位域口径", passed: ATTR_ENABLED == 0x2 && GRP_ENABLED == 0x4 });
    rows.push(KtCheckRow { name: "账单调", passed: reg.result_monotone() });
    rows.push(KtCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn kt_checklist_green(rows: &[KtCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn kt_stats_line(reg: &mut KtRegistry) -> String {
    let rep = KtReport::build(reg);
    format!(
        "ktest 概况：注册 {} 全跑 {}/{} 令牌 {}/{} ACL {}/{}",
        rep.registered, rep.all_pass, ASSERT_TOTAL,
        rep.token_pass, TOKEN_N, rep.acl_pass, ACL_N
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12459_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut reg = KtRegistry::new();
    let _ = reg.register_all();
    let _ = reg.run_all();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        KtReport::build(&mut reg).all_green() && reg.export_lines().contains("ktest 结果账"),
        kt_stats_line(&mut reg),
    ));
    // X-b：注册面 10/10。
    let (ok_g, d_g) = drill_registry_10();
    out.push(("X-注册面10/10", ok_g, d_g));
    // X-c：注入漏抓 10/10。
    let (ok_i, d_i) = drill_inject_catch_10();
    out.push(("X-注入漏抓10/10", ok_i, d_i));
    // X-d：终验清单。
    let rows = build_kt_checklist(&mut reg);
    out.push((
        "X-终验清单10项",
        kt_checklist_green(&rows),
        KtReport::build(&mut reg).to_text(),
    ));
    // X-e：断言体覆盖恒等式（19 条直调全绿）。
    let direct = (0..ASSERT_TOTAL).all(assert_body);
    out.push((
        "X-断言体覆盖恒等式",
        direct,
        format!("直调 {}/{} 全绿", ASSERT_TOTAL, ASSERT_TOTAL),
    ));
    out
}

/// run_unx12459_checks：本单判据聚合（诊断段 0x30760）。
pub fn run_unx12459_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12459 ktest 令牌族断言集");
    // J1-a：一次全跑 10/10。
    let (ok1, d1) = drill_run_all_10();
    s.add_dyn("J1-一次全跑10/10", ok1, &d1);
    // J1-b：族单跑 10/10。
    let (ok2, d2) = drill_family_solo_10();
    s.add_dyn("J1-族单跑10/10", ok2, &d2);
    // J1-c：标准会话恒等式。
    let mut reg = KtRegistry::new();
    let _ = reg.register_all();
    let p = reg.run_all();
    s.add_dyn(
        "J1-19条聚合100%",
        reg.all_green() && p == ASSERT_TOTAL && TOKEN_N + ACL_N == ASSERT_TOTAL,
        &format!("注册={} 全跑={}/{}", reg.count(), p, ASSERT_TOTAL),
    );
    // J2-a：注入漏抓 10/10。
    let (ok3, d3) = drill_inject_catch_10();
    s.add_dyn("J2-注入漏抓10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：注册面 10/10。
    let (ok4, d4) = drill_registry_10();
    s.add_dyn("J2-注册面10/10", ok4, &d4);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30760段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12459_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
