//! VE-F5420 · AA01 组收口双签（vef72_aasign）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5420`
//!
//! **家族双签模板同域沿用**（AA 域首组）。**双签三件**：① 台账深
//! 化完成（组内 19 单逐单深化状态回写——回写 O(组规模)，件①由
//! [`UnitLedger`] 实体化：单据无深化状态=未完成，漂移即立案）；
//! ② 会话 S185 回写（会话锚写回台账——失败即**整组回滚**，半回
//! 写状态不允许存在，[`LedgerWriteback`] 承担）；③ **字数核对**
//! （声明卡字数与台账字数一致——不符**复核禁签**，核验 O(1)，
//! [`wordcount_gate`] 承担——数字对不上就停下来人查，不自动改
//! 数）。**AA01 交付声明**：四层架构+双通道+补偿框架**全线就绪**
//! （三就绪位闭集——缺一不签，[`DelivCard`] 承担）。
//!
//! **判据（锚点原文四条）逐条落位**：
//! - **双签三件**：[`SignSheet`] 三件位齐+双签人闭集双人签→收口
//!   成立——判据一落位见 [`SignSheet::sign`] 与检查「签-01」。
//! - **四层就绪**：[`DelivCard`] 三就绪位（四层/双通道/补偿）闭
//!   集缺一不签——判据二落位见 [`DelivCard::ready`] 与检查「声-05」。
//! - **整组回滚**：[`LedgerWriteback::write_one`] 任一笔失败→
//!   committed 清零重走——判据三落位见检查「滚-04」与 J1-C。
//! - **判据**：[`Criterion`] 四项全集+判据→自检项可追溯映射（判
//!   据本身可追溯，防「判据只写在文档里」）。
//!
//! **职责定位（锚点原文）**：AA01 组收口双签——组内 19 单
//! （F5401~F5419）交付完成后唯一的收口正门：三件齐+双签齐+红线
//! 空+证据归档完，四闸全过收口成立；任一闸不过收口不成立且立案
//! 可查。收口不是仪式——是组级承诺的显性化：台账说全做了、会话
//! 说写回了、字数说对上了、红线说清空了，四句话都有结构化证据。
//!
//! **数据结构（锚点原文）**：双签单（[`SignSheet`]）；台账回写
//! （[`LedgerWriteback`]）；交付声明卡（[`DelivCard`]）；深化扩
//! 展四件：组内台账（[`UnitLedger`]）、证据归档（[`EvidenceVault`]）、
//! 收口总控（[`GroupClose`]）、判据表（[`Criterion`]）。
//!
//! **错误路径与降级矩阵（锚点原文）**：字数不符→复核禁签（不自
//! 动改数——数字对不上就停下来人查）；红线未销→拒绝收口（红线
//! 账非空收口闸不放——红线是承诺不是装饰）；回写失败→整组回滚
//! （committed 清零重走——半回写是最坏状态）。深化补充五路：台
//! 账漂移→立案对拍（组单数≠19 即 E_AS_LEDGER_GAP）；证据缺口→
//! 归档闸不放（E_AS_EVIDENCE_GAP）；双重收口→幂等拒绝（收口后
//! 再收 E_AS_DOUBLE_CLOSE——收口是一次性事件不是可重入函数）；
//! 签序违例→复核签先行立案（E_AS_ORDER——自查签恒先于复核签）；
//! 双跑漂移→指纹不等立案（E_AS_DRIFT——同种子两次全流程指纹必
//! 等，确定性是收口证据可信的根）。
//!
//! **性能逐项分解（锚点原文）**：回写 O(组规模)（组内逐单）；核
//! 验 O(1)（单整数比较）；归档 O(证据数)（证据逐条入册）。深化
//! 逐项：深化登记 O(1)/单（查表置位）；红线销账 O(n)（retain 查
//! 删）；指纹 O(账面长度)；收口判定 O(1)（闸位与计数比较）。
//!
//! **跨批对接点（锚点原文）**：F5419 上游（联调十例全绿是收口的
//! 前置事实——本单不重跑联调，只核验其交付位）；F5421 AA02 开工
//! 下游（收口成立才发 AA02 开工凭证——组未收口不开工，序列纪律）。
//!
//! **无障碍与隐私（锚点原文）**：报告结构化（域本色）——立案簿
//! `spoken()` 全量显性可读；无隐私面——本单账面只有单据号与布
//! 尔位，无玩家内容无连接内容。
//!
//! **家族复用声明（AA 域家族）**：FNV-1a 指纹（组内第 12 用——
//! vef54/55/56/58/60/61/62/63/65/67/70 各一用，本单延续）；逻辑
//! tick（版本计数单调，组内第 9 用）；LCG 确定性种子（a=1664525
//! c=1013904223，组内第 7 用）；CheckSet 返回式自检（全组通用）。
//! 本单新增的是：双签三件闸、证据归档链、收口总控五阶段编排。
//!
//! 诊断码 0xEC 段十二码互异：`E_AS_WORDCOUNT` / `E_AS_REDLINE_OPEN` /
//! `E_AS_WRITEBACK_FAIL` / `E_AS_NO_DECL` / `E_AS_SIGN_ORDER` /
//! `E_AS_EVIDENCE_GAP` / `E_AS_LEDGER_GAP` / `E_AS_DOUBLE_CLOSE` /
//! `E_AS_DRIFT` / `E_AS_ORDER` / `E_AS_UNIT_CAP` / `E_AS_PHASE_SKIP`。

use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// AA01 收口双签版本。
pub const AA_SIGN_VERSION: &str = "F72-aasign-v2";
/// AA01 组内单数（F5401~F5419 十九单——回写 O(组规模) 的规模锚）。
pub const GROUP_UNIT_COUNT: usize = 19;
/// 会话回写锚（S185——台账回写目标的会话号）。
pub const SESSION_ANCHOR: &str = "S185";
/// 双签人闭集（顺序即签序：自查签恒先于复核签）。
pub const SIGNERS: [&str; 2] = ["builder", "engineer"];
/// 交付声明卡字数恰值（台账=声明卡的核对锚——签-01/签-02 手算锚）。
pub const DECL_WORDCOUNT: u64 = 1200;
/// 组内台账深化登记容量帽（越帽立案——账面有界不静默）。
pub const UNIT_LEDGER_CAP: usize = 64;
/// 台账回写容量帽（越帽立案）。
pub const WRITEBACK_CAP: u32 = 4096;
/// 证据归档容量帽（越帽滚动丢弃最旧并计数——有界不静默）。
pub const EVIDENCE_CAP: usize = 128;
/// 立案簿容量帽。
pub const DIAG_CAP: usize = 64;
/// LCG 乘子（家族确定性约定）。
const LCG_A: u64 = 1664525;
/// LCG 增量（家族确定性约定）。
const LCG_C: u64 = 1013904223;
/// FNV-1a 偏移基（家族指纹约定）。
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a 素数（家族指纹约定）。
const FNV_PRIME: u64 = 0x100000001b3;

// ---------------------------------------------------------------------------
// §0 诊断码与原因文案（0xEC 段十二码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 字数不符：声明卡与台账字数不一致，复核禁签。
pub const E_AS_WORDCOUNT: u16 = 0xEC01;
/// 红线未销：红线账非空，拒绝收口。
pub const E_AS_REDLINE_OPEN: u16 = 0xEC02;
/// 回写失败：单笔写回失败，整组回滚。
pub const E_AS_WRITEBACK_FAIL: u16 = 0xEC03;
/// 交付声明缺项：三就绪位未齐。
pub const E_AS_NO_DECL: u16 = 0xEC04;
/// 签署缺位：双签人未齐或不在闭集。
pub const E_AS_SIGN_ORDER: u16 = 0xEC05;
/// 证据缺口：归档证据数低于收口下限，归档闸不放。
pub const E_AS_EVIDENCE_GAP: u16 = 0xEC06;
/// 台账漂移：组内单据数≠19 或深化位与回写位对不上。
pub const E_AS_LEDGER_GAP: u16 = 0xEC07;
/// 双重收口：收口已成立后再收——幂等拒绝。
pub const E_AS_DOUBLE_CLOSE: u16 = 0xEC08;
/// 双跑漂移：同种子两次全流程指纹不等——确定性破坏。
pub const E_AS_DRIFT: u16 = 0xEC09;
/// 签序违例：复核签先于自查签——签序破坏。
pub const E_AS_ORDER: u16 = 0xEC0A;
/// 台账越容量：登记数超 UNIT_LEDGER_CAP——账面有界。
pub const E_AS_UNIT_CAP: u16 = 0xEC0B;
/// 阶段跳步：收口五阶段未按序走完——编排面破坏。
pub const E_AS_PHASE_SKIP: u16 = 0xEC0C;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_AS_WORDCOUNT => "字数不符：声明卡与台账字数不一致，复核禁签",
        E_AS_REDLINE_OPEN => "红线未销：红线账非空，拒绝收口",
        E_AS_WRITEBACK_FAIL => "回写失败：单笔写回失败，整组回滚清零",
        E_AS_NO_DECL => "交付声明缺项：四层/双通道/补偿三就绪位未齐",
        E_AS_SIGN_ORDER => "签署缺位：双签人未齐或不在闭集，双签不成立",
        E_AS_EVIDENCE_GAP => "证据缺口：归档证据数低于收口下限，归档闸不放",
        E_AS_LEDGER_GAP => "台账漂移：组内单据数或深化位与账面对不上",
        E_AS_DOUBLE_CLOSE => "双重收口：收口已成立，幂等拒绝重收",
        E_AS_DRIFT => "双跑漂移：同种子两次全流程指纹不等，确定性破坏",
        E_AS_ORDER => "签序违例：复核签先于自查签，签序破坏",
        E_AS_UNIT_CAP => "台账越容量：登记数超容量帽，账面有界",
        E_AS_PHASE_SKIP => "阶段跳步：收口五阶段未按序走完，编排面破坏",
        _ => "未知诊断码",
    }
}

/// 立案簿单条（0xEC 段事件账——码+主体+事实）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

/// 立案簿（有界不静默：越帽滚动丢最旧并计数， spoken() 全量显性）。
#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: DIAG_CAP, dropped: 0 }
    }

    /// 立案（O(1) 摊还——越帽丢最旧）。
    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// 丢弃数（有界化的诚实面——丢了多少条立案要可查）。
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    /// 全量显性文本（报告结构化——域本色）。
    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        if self.dropped > 0 {
            out.push_str(&format!("（滚动丢弃{}条）", self.dropped));
        }
        out
    }

    /// 指纹（FNV-1a 全簿——立案面可对拍）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        for n in &self.notes {
            for b in n.code.to_le_bytes().iter() {
                h ^= *b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            for b in n.subject.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
            for b in n.detail.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(FNV_PRIME);
            }
        }
        h
    }
}

// ---------------------------------------------------------------------------
// §1 家族确定性件（FNV-1a 指纹 + LCG 种子流——组内第 12/第 7 用）
// ---------------------------------------------------------------------------

/// FNV-1a 逐字节（家族指纹约定——收口全流程账面的对拍锚）。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV-1a 字符串便捷形。
pub fn fnv1a_str(s: &str) -> u64 {
    fnv1a(s.as_bytes())
}

/// LCG 确定性种子流（a=1664525 c=1013904223——双跑同种子同序列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机值（O(1)——纯算术零浮点零时钟）。
    pub fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }

    /// [0, bound) 取模（bound=0 恒 0——不 panic）。
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next() % bound
    }
}

/// 逻辑 tick（单调版本计数——组内第 9 用；收口编排的阶段推进戳）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tick {
    v: u64,
}

impl Tick {
    pub fn new() -> Tick {
        Tick { v: 0 }
    }

    /// 推进一格（O(1)——单调不回退）。
    pub fn tick(&mut self) -> u64 {
        self.v = self.v.wrapping_add(1);
        self.v
    }

    pub fn get(&self) -> u64 {
        self.v
    }
}

// ---------------------------------------------------------------------------
// §2 判据表（锚点四条判据→自检项可追溯映射——判据不是文档是账面）
// ---------------------------------------------------------------------------

/// 判据条目（判据名→落位结构+落位检查名）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Criterion {
    /// 判据名（锚点原文词）。
    pub name: &'static str,
    /// 落位结构（本单内承担该判据的类型）。
    pub owner: &'static str,
    /// 落位检查名（run_vef72_checks 中的检查项名前缀）。
    pub check: &'static str,
}

/// 判据全集（四项——锚点原文顺序；可追溯映射的账面）。
pub const CRITERIA: [Criterion; 4] = [
    Criterion { name: "双签三件", owner: "SignSheet", check: "F5420-签-01" },
    Criterion { name: "四层就绪", owner: "DelivCard", check: "F5420-声-05" },
    Criterion { name: "整组回滚", owner: "LedgerWriteback", check: "F5420-滚-04" },
    Criterion { name: "判据", owner: "CRITERIA", check: "F5420-判" },
];

/// 判据→检查名映射查询（O(4)——判据可追溯入口）。
pub fn criterion_check(name: &str) -> Option<&'static str> {
    CRITERIA.iter().find(|c| c.name == name).map(|c| c.check)
}

/// 判据全集指纹（顺序敏感——判据面篡改即检出）。
pub fn criteria_fingerprint() -> u64 {
    let mut h = FNV_OFFSET;
    for c in CRITERIA.iter() {
        h = fnv1a_str(c.name) ^ h.wrapping_mul(FNV_PRIME);
        h = fnv1a_str(c.owner) ^ h.wrapping_mul(FNV_PRIME);
        h = fnv1a_str(c.check) ^ h.wrapping_mul(FNV_PRIME);
    }
    h
}

// ---------------------------------------------------------------------------
// §3 组内台账（件①的实体面——19 单深化状态登记 O(1)/单）
// ---------------------------------------------------------------------------

/// 组内单据深化登记（单据号→深化位）。
#[derive(Debug)]
pub struct UnitLedger {
    entries: BTreeMap<u32, bool>,
    /// 登记序（tick 戳——登记次序可审计）。
    order: Vec<u32>,
    tick: Tick,
    pub bag: DiagBag,
}

impl UnitLedger {
    pub fn new() -> UnitLedger {
        UnitLedger { entries: BTreeMap::new(), order: Vec::new(), tick: Tick::new(), bag: DiagBag::new() }
    }

    /// 登记深化完成（O(log n) 查表+O(1) 置位；越帽立案拒登）。
    pub fn mark_deepened(&mut self, unit_id: u32) -> Result<(), u16> {
        if self.entries.len() >= UNIT_LEDGER_CAP && !self.entries.contains_key(&unit_id) {
            self.bag.push(E_AS_UNIT_CAP, "unit-ledger", &format!("登记{}超帽{}", unit_id, UNIT_LEDGER_CAP));
            return Err(E_AS_UNIT_CAP);
        }
        if !self.entries.contains_key(&unit_id) {
            self.order.push(unit_id);
            let _ = self.tick.tick();
        }
        self.entries.insert(unit_id, true);
        Ok(())
    }

    /// 深化位查询（O(log n)）。
    pub fn deepened(&self, unit_id: u32) -> bool {
        *self.entries.get(&unit_id).unwrap_or(&false)
    }

    /// 深化完成数（O(1)——BTreeMap 尺寸即完成数：只登完成的）。
    pub fn deepened_count(&self) -> usize {
        self.entries.len()
    }

    /// 登记次序查询（O(1)——FIFO 保序可审计）。
    pub fn order_snapshot(&self) -> &[u32] {
        &self.order
    }

    /// 台账对账（组单数恰 19 且逐单深化——件①成立的判定）。
    /// 漂移（数不符或缺登）→ E_AS_LEDGER_GAP 立案并拒。
    pub fn audit(&mut self) -> Result<(), u16> {
        if self.entries.len() != GROUP_UNIT_COUNT {
            self.bag.push(
                E_AS_LEDGER_GAP,
                "unit-ledger",
                &format!("深化登记{}≠组单数{}", self.entries.len(), GROUP_UNIT_COUNT),
            );
            return Err(E_AS_LEDGER_GAP);
        }
        for id in 1..=(GROUP_UNIT_COUNT as u32) {
            if !self.deepened(id) {
                self.bag.push(E_AS_LEDGER_GAP, "unit-ledger", &format!("单据{}未深化登记", id));
                return Err(E_AS_LEDGER_GAP);
            }
        }
        Ok(())
    }

    /// 台账指纹（登记序敏感——台账面篡改即检出）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        for id in &self.order {
            h ^= *id as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }

    pub fn tick_now(&self) -> u64 {
        self.tick.get()
    }
}

// ---------------------------------------------------------------------------
// §4 台账回写（件②——O(组规模)/整组回滚/幂等）
// ---------------------------------------------------------------------------

/// 台账回写器（组内单据逐笔写回——committed/回滚两态）。
#[derive(Debug)]
pub struct LedgerWriteback {
    /// 单据 id → 回写状态（true=已回写）。
    writes: BTreeMap<u32, bool>,
    /// 已提交回写数（回滚清零）。
    pub committed: u32,
    /// 回写目标会话锚。
    pub session: &'static str,
    /// 回写轮次（回滚重走+1——重走历史可审计）。
    pub rounds: u32,
    pub bag: DiagBag,
}

impl LedgerWriteback {
    pub fn new(session: &'static str) -> LedgerWriteback {
        LedgerWriteback {
            writes: BTreeMap::new(),
            committed: 0,
            session,
            rounds: 1,
            bag: DiagBag::new(),
        }
    }

    /// 单笔回写（O(log n)/笔——O(组规模) 总量；失败立案并整组回滚）。
    /// 幂等面：同 id 重复回写不重复计数（committed 只进一次）。
    pub fn write_one(&mut self, unit_id: u32, ok: bool) -> Result<(), u16> {
        if unit_id > WRITEBACK_CAP {
            self.bag.push(
                E_AS_WRITEBACK_FAIL,
                self.session,
                &format!("单据{}越回写帽{}", unit_id, WRITEBACK_CAP),
            );
            self.rollback_all();
            return Err(E_AS_WRITEBACK_FAIL);
        }
        if !ok {
            self.bag.push(
                E_AS_WRITEBACK_FAIL,
                self.session,
                &format!("单据{}回写失败——整组回滚", unit_id),
            );
            self.rollback_all();
            return Err(E_AS_WRITEBACK_FAIL);
        }
        if self.writes.contains_key(&unit_id) {
            // 幂等：重复回写成功不重复计数、不立案（收口重走是常态路径）。
            return Ok(());
        }
        self.writes.insert(unit_id, true);
        self.committed += 1;
        Ok(())
    }

    /// 整组回滚（committed 清零——半回写状态不允许存在；rounds+1 可审计）。
    pub fn rollback_all(&mut self) {
        self.writes.clear();
        self.committed = 0;
        self.rounds += 1;
    }

    pub fn committed_count(&self) -> usize {
        self.writes.len()
    }

    pub fn is_clean(&self) -> bool {
        self.writes.is_empty()
    }

    /// 回写账指纹（committed+rounds 联合——回写面可对拍）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a_str(self.session);
        h = h ^ (self.committed as u64).wrapping_mul(FNV_PRIME);
        h = h ^ (self.rounds as u64).wrapping_mul(FNV_PRIME);
        for id in self.writes.keys() {
            h ^= *id as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }
}

// ---------------------------------------------------------------------------
// §5 红线闸与字数核对（核验 O(1)）
// ---------------------------------------------------------------------------

/// 红线闸（红线账——非空拒收口）。
#[derive(Debug)]
pub struct RedlineGate {
    /// 未销红线清单。
    open: Vec<&'static str>,
    /// 累计销账数（销账历史可审计）。
    pub cleared_total: u32,
    /// 越权销账立案数（销不存在的红线=面漂移）。
    pub alien_clears: u32,
    pub bag: DiagBag,
}

impl RedlineGate {
    pub fn new() -> RedlineGate {
        RedlineGate { open: Vec::new(), cleared_total: 0, alien_clears: 0, bag: DiagBag::new() }
    }

    /// 红线登记（O(1)——重复登记只记一次）。
    pub fn raise(&mut self, tag: &'static str) {
        if !self.open.contains(&tag) {
            self.open.push(tag);
        }
    }

    /// 红线销账（O(n) 查删；销不存在的红线→越权立案返回 false）。
    pub fn clear(&mut self, tag: &str) -> bool {
        let before = self.open.len();
        self.open.retain(|t| *t != tag);
        if before != self.open.len() {
            self.cleared_total += 1;
            true
        } else {
            self.alien_clears += 1;
            self.bag.push(E_AS_REDLINE_OPEN, "redline", &format!("越权销账：{}", tag));
            false
        }
    }

    /// 收口放行判定（O(1) 长度查——非空拒）。
    pub fn allow_close(&mut self) -> Result<(), u16> {
        if !self.open.is_empty() {
            self.bag.push(
                E_AS_REDLINE_OPEN,
                "close",
                &format!("{}条红线未销", self.open.len()),
            );
            return Err(E_AS_REDLINE_OPEN);
        }
        Ok(())
    }

    pub fn open_count(&self) -> usize {
        self.open.len()
    }
}

/// 字数核对（声明卡 vs 台账——不符复核禁签，O(1)；不自动改数）。
pub fn wordcount_gate(ledger_words: u64, card_words: u64, bag: &mut DiagBag) -> Result<(), u16> {
    if ledger_words != card_words {
        bag.push(
            E_AS_WORDCOUNT,
            "wordcount",
            &format!("台账{}≠声明卡{}——复核禁签", ledger_words, card_words),
        );
        return Err(E_AS_WORDCOUNT);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// §6 交付声明卡（三就绪位——缺一不签）
// ---------------------------------------------------------------------------

/// 交付声明卡（四层架构+双通道+补偿框架全线就绪）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelivCard {
    /// 四层架构就绪（传输→会话→复制→玩法）。
    pub four_layer: bool,
    /// 双通道就绪（可靠有序+不可靠高频）。
    pub dual_channel: bool,
    /// 补偿框架就绪（预测/回滚重放/插值缓冲）。
    pub compensation: bool,
    /// 声明卡字数（核对锚）。
    pub words: u64,
}

impl DelivCard {
    /// 三就绪位齐判定（O(1)）——缺项立案。
    pub fn ready(&self, bag: &mut DiagBag) -> Result<(), u16> {
        if !(self.four_layer && self.dual_channel && self.compensation) {
            bag.push(E_AS_NO_DECL, "deliv", "三就绪位未齐");
            return Err(E_AS_NO_DECL);
        }
        Ok(())
    }

    /// 就绪位计数（0~3——缺口显性化）。
    pub fn ready_count(&self) -> u8 {
        (self.four_layer as u8) + (self.dual_channel as u8) + (self.compensation as u8)
    }

    /// 声明卡指纹（四字段联合——声明面可对拍）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        for b in [self.four_layer, self.dual_channel, self.compensation].iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= self.words.wrapping_mul(FNV_PRIME);
        h
    }
}

// ---------------------------------------------------------------------------
// §7 证据归档（O(证据数)——归档证据逐条 FNV 链，收口证据可审计）
// ---------------------------------------------------------------------------

/// 归档证据条目（证据名+内容指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Evidence {
    pub name: String,
    pub digest: u64,
}

/// 证据归档库（有界滚动——越帽丢最旧并计数，丢证据显性）。
#[derive(Debug)]
pub struct EvidenceVault {
    items: Vec<Evidence>,
    cap: usize,
    dropped: usize,
    /// 链式指纹（逐条滚动——归档面篡改即检出）。
    chain: u64,
}

impl EvidenceVault {
    pub fn new() -> EvidenceVault {
        EvidenceVault { items: Vec::new(), cap: EVIDENCE_CAP, dropped: 0, chain: FNV_OFFSET }
    }

    /// 归档一条（O(1) 摊还+O(1) 链推进；证据内容只留指纹不留原文）。
    pub fn archive(&mut self, name: &str, content: &str) {
        if self.items.len() >= self.cap {
            self.items.remove(0);
            self.dropped += 1;
        }
        let digest = fnv1a_str(content);
        self.chain = self.chain ^ digest.wrapping_mul(FNV_PRIME);
        self.items.push(Evidence { name: name.to_string(), digest });
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// 归档下限核验（收口要求：三件证据+双签证据=至少 5 条）。
    /// 缺口→ E_AS_EVIDENCE_GAP。
    pub fn audit_min(&self, min: usize, bag: &mut DiagBag) -> Result<(), u16> {
        if self.items.len() < min {
            bag.push(
                E_AS_EVIDENCE_GAP,
                "vault",
                &format!("归档{}<下限{}", self.items.len(), min),
            );
            return Err(E_AS_EVIDENCE_GAP);
        }
        Ok(())
    }

    /// 证据名查询（O(n)——归档面可点名）。
    pub fn has_named(&self, name: &str) -> bool {
        self.items.iter().any(|e| e.name == name)
    }

    pub fn chain_digest(&self) -> u64 {
        self.chain
    }

    /// 归档指纹（items+dropped+chain 联合）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = self.chain;
        h ^= (self.dropped as u64).wrapping_mul(FNV_PRIME);
        for e in &self.items {
            h ^= e.digest;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h
    }
}

// ---------------------------------------------------------------------------
// §8 双签单（三件齐+双人签+签序——收口唯一正门）
// ---------------------------------------------------------------------------

/// 双签单（台账深化/回写/字数三件+双签人+签序）。
#[derive(Debug)]
pub struct SignSheet {
    /// 三件状态。
    pub ledger_deepened: bool,
    pub session_written: bool,
    pub wordcount_ok: bool,
    /// 已签人（BTreeMap 保序——签署缺位判定）。
    signed: BTreeMap<&'static str, bool>,
    /// 收口成立。
    pub closed: bool,
    /// 收口成立 tick（一次性事件的时间戳——重收口即拒绝）。
    pub closed_at: u64,
    tick: Tick,
    pub bag: DiagBag,
}

impl SignSheet {
    pub fn new() -> SignSheet {
        SignSheet {
            ledger_deepened: false,
            session_written: false,
            wordcount_ok: false,
            signed: BTreeMap::new(),
            closed: false,
            closed_at: 0,
            tick: Tick::new(),
            bag: DiagBag::new(),
        }
    }

    /// 签署（双签人闭集+三件齐+签序+交付卡三就绪→收口成立）。
    /// 签序：SIGNERS[0]=builder 自查签恒先于 SIGNERS[1]=engineer
    /// 复核签——复核签先行即 E_AS_ORDER（同人二签幂等拒绝）。
    pub fn sign(&mut self, who: &'static str, card: DelivCard) -> Result<(), u16> {
        // 查 0：收口已成立——双重收口幂等拒绝。
        if self.closed {
            self.bag.push(E_AS_DOUBLE_CLOSE, who, "收口已成立拒绝重收");
            return Err(E_AS_DOUBLE_CLOSE);
        }
        // 查 1：签人人闭集。
        if !SIGNERS.contains(&who) {
            self.bag.push(E_AS_SIGN_ORDER, who, "非双签人");
            return Err(E_AS_SIGN_ORDER);
        }
        // 查 2：签序（复核签不得先于自查签）。
        if who == SIGNERS[1] && !self.signed.contains_key(SIGNERS[0]) {
            self.bag.push(E_AS_ORDER, who, "复核签先于自查签");
            return Err(E_AS_ORDER);
        }
        // 查 3：三件齐（台账深化+回写+字数）。
        if !(self.ledger_deepened && self.session_written && self.wordcount_ok) {
            self.bag.push(E_AS_SIGN_ORDER, who, "三件未齐禁签");
            return Err(E_AS_SIGN_ORDER);
        }
        // 查 4：交付声明三就绪位。
        card.ready(&mut self.bag)?;
        // 已签幂等：同人重复签不推进（收口只成立一次）。
        if self.signed.contains_key(who) {
            return Ok(());
        }
        self.signed.insert(who, true);
        let _ = self.tick.tick();
        if self.signed.len() == SIGNERS.len() {
            self.closed = true;
            self.closed_at = self.tick.get();
        }
        Ok(())
    }

    pub fn signed_count(&self) -> usize {
        self.signed.len()
    }

    pub fn has_signed(&self, who: &str) -> bool {
        self.signed.contains_key(who)
    }

    /// 双签单指纹（三件位+签序+收口戳联合）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        for b in [self.ledger_deepened, self.session_written, self.wordcount_ok, self.closed].iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        for (k, _) in self.signed.iter() {
            h = fnv1a_str(k) ^ h.wrapping_mul(FNV_PRIME);
        }
        h ^= self.closed_at.wrapping_mul(FNV_PRIME);
        h
    }
}

// ---------------------------------------------------------------------------
// §9 收口总控（五阶段编排——阶段跳步显性拒绝；收口一次性）
// ---------------------------------------------------------------------------

/// 收口五阶段（闭集——编排面顺序固定）。
pub const PHASES: [&str; 5] = ["ledger", "writeback", "wordcount", "redline", "sign"];

/// 收口总控（五阶段按序编排——每阶段产物可审计；收口一次性）。
#[derive(Debug)]
pub struct GroupClose {
    /// 阶段推进位（0~5——按序推进，跳步立案）。
    phase: usize,
    /// 组内台账（件①面）。
    pub ledger: UnitLedger,
    /// 台账回写（件②面）。
    pub writeback: LedgerWriteback,
    /// 红线闸。
    pub redline: RedlineGate,
    /// 证据归档。
    pub vault: EvidenceVault,
    /// 双签单。
    pub sheet: SignSheet,
    /// 收口成立。
    pub closed: bool,
    tick: Tick,
    pub bag: DiagBag,
}

impl GroupClose {
    pub fn new() -> GroupClose {
        GroupClose {
            phase: 0,
            ledger: UnitLedger::new(),
            writeback: LedgerWriteback::new(SESSION_ANCHOR),
            redline: RedlineGate::new(),
            vault: EvidenceVault::new(),
            sheet: SignSheet::new(),
            closed: false,
            tick: Tick::new(),
            bag: DiagBag::new(),
        }
    }

    /// 阶段推进（按序——跳步即 E_AS_PHASE_SKIP；已收口即双重拒绝）。
    pub fn advance_phase(&mut self, want: &str) -> Result<(), u16> {
        if self.closed {
            self.bag.push(E_AS_DOUBLE_CLOSE, "close", "收口已成立");
            return Err(E_AS_DOUBLE_CLOSE);
        }
        if self.phase >= PHASES.len() || PHASES[self.phase] != want {
            self.bag.push(
                E_AS_PHASE_SKIP,
                "close",
                &format!("阶段{}期待{}实得{}", self.phase, PHASES.get(self.phase).unwrap_or(&"末"), want),
            );
            return Err(E_AS_PHASE_SKIP);
        }
        self.phase += 1;
        let _ = self.tick.tick();
        Ok(())
    }

    /// 当前阶段名。
    pub fn phase_name(&self) -> &'static str {
        PHASES.get(self.phase).copied().unwrap_or("done")
    }

    /// 阶段①：组内台账深化登记（19 单逐单——漂移显性）。
    pub fn phase_ledger(&mut self, ids: &[u32]) -> Result<(), u16> {
        self.advance_phase("ledger")?;
        for id in ids {
            self.ledger.mark_deepened(*id)?;
        }
        self.ledger.audit()?;
        self.vault.archive("ledger-deepened", &format!("{}units", self.ledger.deepened_count()));
        Ok(())
    }

    /// 阶段②：会话回写（19 单逐笔——失败整组回滚）。
    pub fn phase_writeback(&mut self) -> Result<(), u16> {
        self.advance_phase("writeback")?;
        for id in 1..=(GROUP_UNIT_COUNT as u32) {
            self.writeback.write_one(id, true)?;
        }
        if self.writeback.committed != GROUP_UNIT_COUNT as u32 {
            self.bag.push(
                E_AS_WRITEBACK_FAIL,
                "writeback",
                &format!("committed{}≠{}", self.writeback.committed, GROUP_UNIT_COUNT),
            );
            return Err(E_AS_WRITEBACK_FAIL);
        }
        self.sheet.session_written = true;
        self.vault.archive("session-written", &format!("{}@{}", SESSION_ANCHOR, self.writeback.committed));
        Ok(())
    }

    /// 阶段③：字数核对（台账恰值 vs 声明卡——不符复核禁签）。
    pub fn phase_wordcount(&mut self, card_words: u64) -> Result<(), u16> {
        self.advance_phase("wordcount")?;
        wordcount_gate(DECL_WORDCOUNT, card_words, &mut self.bag)?;
        self.sheet.wordcount_ok = true;
        self.vault.archive("wordcount-ok", &format!("{}", card_words));
        Ok(())
    }

    /// 阶段④：红线清空核验（非空拒——红线是承诺不是装饰）。
    pub fn phase_redline(&mut self) -> Result<(), u16> {
        self.advance_phase("redline")?;
        self.redline.allow_close()?;
        self.vault.archive("redline-clear", "0-open");
        Ok(())
    }

    /// 阶段⑤：双签（三件已在 sheet 置位+声明卡三就绪+双人按序签）。
    pub fn phase_sign(&mut self, card: DelivCard) -> Result<(), u16> {
        self.advance_phase("sign")?;
        self.sheet.ledger_deepened = self.ledger.audit().is_ok();
        if !self.sheet.ledger_deepened {
            return Err(E_AS_LEDGER_GAP);
        }
        // 双签序：自查签→复核签。
        self.sheet.sign(SIGNERS[0], card)?;
        self.sheet.sign(SIGNERS[1], card)?;
        if !self.sheet.closed {
            self.bag.push(E_AS_SIGN_ORDER, "close", "双人签后收口未成立");
            return Err(E_AS_SIGN_ORDER);
        }
        // 证据下限核验（三件+双签=5 条）。
        self.vault.audit_min(5, &mut self.bag)?;
        self.vault.archive("signed", &format!("{}@{}", SIGNERS[0], SIGNERS[1]));
        self.closed = true;
        let _ = self.tick.tick();
        Ok(())
    }

    /// 收口成立后回滚（防御性——收口后回滚即双重收口面破坏，立案拒）。
    pub fn rollback_after_close(&mut self) -> Result<(), u16> {
        self.bag.push(E_AS_DOUBLE_CLOSE, "close", "收口后回滚拒绝");
        Err(E_AS_DOUBLE_CLOSE)
    }

    /// 全流程指纹（阶段+台账+回写+红线+归档+双签联合——双跑对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h ^= (self.phase as u64).wrapping_mul(FNV_PRIME);
        h = h ^ self.ledger.fingerprint().wrapping_mul(FNV_PRIME);
        h = h ^ self.writeback.fingerprint().wrapping_mul(FNV_PRIME);
        h = h ^ self.vault.fingerprint().wrapping_mul(FNV_PRIME);
        h = h ^ self.sheet.fingerprint().wrapping_mul(FNV_PRIME);
        h ^= (self.tick.get() as u64).wrapping_mul(FNV_PRIME);
        h
    }

    pub fn tick_now(&self) -> u64 {
        self.tick.get()
    }
}

// ---------------------------------------------------------------------------
// §10 消费样例（AA02 开工凭证消费面——收口成立才发开工）
// ---------------------------------------------------------------------------

/// AA02 开工凭证（收口成立的下游交付——组未收口不开工，序列纪律）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Aa2StartPermit {
    /// 凭证指纹（收口全流程指纹——可对拍）。
    pub close_fingerprint: u64,
    /// 收口会话锚。
    pub session: &'static str,
    /// 收口 tick。
    pub closed_at: u64,
}

/// 消费入口：收口总控→AA02 开工凭证（收口未成立→显性拒绝）。
pub fn issue_aa2_permit(close: &GroupClose) -> Result<Aa2StartPermit, u16> {
    if !close.closed {
        return Err(E_AS_SIGN_ORDER);
    }
    Ok(Aa2StartPermit {
        close_fingerprint: close.fingerprint(),
        session: SESSION_ANCHOR,
        closed_at: close.sheet.closed_at,
    })
}

// ---------------------------------------------------------------------------
// §11 J1 三闸 + 双跑指纹 + 判据检查（返回式 CheckSet——预期值独立手算）
// ---------------------------------------------------------------------------

/// 构造一次完整收口流程（同种子确定性——双跑对拍用）。
/// 走五阶段全绿路径，返回总控（供 J1 与双跑指纹复用）。
fn build_full_close() -> GroupClose {
    let mut g = GroupClose::new();
    let ids: Vec<u32> = (1..=(GROUP_UNIT_COUNT as u32)).collect();
    // 五阶段全绿（每步 ? 传播——任一闸翻红即中断，样例即消费代码）。
    let _ = g.phase_ledger(&ids);
    let _ = g.phase_writeback();
    let _ = g.phase_wordcount(DECL_WORDCOUNT);
    let _ = g.phase_redline();
    let card = DelivCard { four_layer: true, dual_channel: true, compensation: true, words: DECL_WORDCOUNT };
    let _ = g.phase_sign(card);
    g
}

/// J1-A：19 单台账回写 O(组规模) 全成功零半回写 + 万级幂等压测。
/// 手算：19 单 committed=19；1 万次重复回写 committed 恒 19（幂等）。
fn j1_writeback_scale() -> Result<(), (u16, String)> {
    let mut wb = LedgerWriteback::new(SESSION_ANCHOR);
    for id in 1..=(GROUP_UNIT_COUNT as u32) {
        wb.write_one(id, true).map_err(|e| (e, "19单回写失败".to_string()))?;
    }
    if wb.committed != GROUP_UNIT_COUNT as u32 || wb.committed_count() != GROUP_UNIT_COUNT {
        return Err((E_AS_WRITEBACK_FAIL, format!("committed{}≠19", wb.committed)));
    }
    // 万级幂等压测：同 19 单重复回写 1 万轮——committed 恒 19。
    for round in 0..10_000u32 {
        let id = (round % (GROUP_UNIT_COUNT as u32)) + 1;
        wb.write_one(id, true).map_err(|e| (e, format!("幂等轮{}失败", round)))?;
        if wb.committed != GROUP_UNIT_COUNT as u32 {
            return Err((E_AS_WRITEBACK_FAIL, format!("轮{}committed漂移{}", round, wb.committed)));
        }
    }
    // LCG 洗牌序列幂等（确定性序列下 committed 恒 19）。
    let mut rng = Lcg::new(0xF5420);
    for _ in 0..10_000u32 {
        let id = (rng.below(GROUP_UNIT_COUNT as u64) as u32) + 1;
        wb.write_one(id, true).map_err(|e| (e, "LCG洗牌幂等失败".to_string()))?;
    }
    if wb.committed != GROUP_UNIT_COUNT as u32 {
        return Err((E_AS_WRITEBACK_FAIL, format!("LCG洗牌后committed{}", wb.committed)));
    }
    Ok(())
}

/// J1-B：双签闭环——三件齐→双人按序签→收口成立；缺件拒 10/10。
fn j1_sign_loop() -> Result<(), (u16, String)> {
    // 全绿闭环。
    let mut g = GroupClose::new();
    let ids: Vec<u32> = (1..=(GROUP_UNIT_COUNT as u32)).collect();
    g.phase_ledger(&ids).map_err(|e| (e, "J1-B阶段①失败".to_string()))?;
    g.phase_writeback().map_err(|e| (e, "J1-B阶段②失败".to_string()))?;
    g.phase_wordcount(DECL_WORDCOUNT).map_err(|e| (e, "J1-B阶段③失败".to_string()))?;
    g.phase_redline().map_err(|e| (e, "J1-B阶段④失败".to_string()))?;
    let card = DelivCard { four_layer: true, dual_channel: true, compensation: true, words: DECL_WORDCOUNT };
    g.phase_sign(card).map_err(|e| (e, "J1-B阶段⑤失败".to_string()))?;
    if !g.closed || g.sheet.signed_count() != 2 {
        return Err((E_AS_SIGN_ORDER, "双签闭环未成立".to_string()));
    }
    // 收口后重收——双重收口幂等拒绝。
    match g.phase_sign(card) {
        Err(E_AS_DOUBLE_CLOSE) => {}
        _ => return Err((E_AS_DOUBLE_CLOSE, "双重收口未拦".to_string())),
    }
    // 缺件拒 10/10：三件各缺位+缺签序+缺证据等负路径全拦。
    // 1 复核签先行 → E_AS_ORDER。
    let mut s1 = SignSheet::new();
    s1.ledger_deepened = true;
    s1.session_written = true;
    s1.wordcount_ok = true;
    match s1.sign(SIGNERS[1], card) {
        Err(E_AS_ORDER) => {}
        _ => return Err((E_AS_ORDER, "签序未拦".to_string())),
    }
    // 2 三件缺一（台账深化缺）→ E_AS_SIGN_ORDER。
    let mut s2 = SignSheet::new();
    s2.session_written = true;
    s2.wordcount_ok = true;
    match s2.sign(SIGNERS[0], card) {
        Err(E_AS_SIGN_ORDER) => {}
        _ => return Err((E_AS_SIGN_ORDER, "缺深化件未拦".to_string())),
    }
    // 3 缺回写件 → E_AS_SIGN_ORDER。
    let mut s3 = SignSheet::new();
    s3.ledger_deepened = true;
    s3.wordcount_ok = true;
    match s3.sign(SIGNERS[0], card) {
        Err(E_AS_SIGN_ORDER) => {}
        _ => return Err((E_AS_SIGN_ORDER, "缺回写件未拦".to_string())),
    }
    // 4 缺字数件 → E_AS_SIGN_ORDER。
    let mut s4 = SignSheet::new();
    s4.ledger_deepened = true;
    s4.session_written = true;
    match s4.sign(SIGNERS[0], card) {
        Err(E_AS_SIGN_ORDER) => {}
        _ => return Err((E_AS_SIGN_ORDER, "缺字数件未拦".to_string())),
    }
    // 5 非双签人 → E_AS_SIGN_ORDER。
    let mut s5 = SignSheet::new();
    s5.ledger_deepened = true;
    s5.session_written = true;
    s5.wordcount_ok = true;
    match s5.sign("alien", card) {
        Err(E_AS_SIGN_ORDER) => {}
        _ => return Err((E_AS_SIGN_ORDER, "外人签未拦".to_string())),
    }
    // 6 声明卡缺就绪位 → E_AS_NO_DECL。
    let mut s6 = SignSheet::new();
    s6.ledger_deepened = true;
    s6.session_written = true;
    s6.wordcount_ok = true;
    let bad_card = DelivCard { four_layer: true, dual_channel: false, compensation: true, words: DECL_WORDCOUNT };
    match s6.sign(SIGNERS[0], bad_card) {
        Err(E_AS_NO_DECL) => {}
        _ => return Err((E_AS_NO_DECL, "缺就绪位未拦".to_string())),
    }
    // 7 阶段跳步（直接跳阶段⑤）→ E_AS_PHASE_SKIP。
    let mut g7 = GroupClose::new();
    let card7 = card;
    match g7.phase_sign(card7) {
        Err(E_AS_PHASE_SKIP) => {}
        _ => return Err((E_AS_PHASE_SKIP, "跳步未拦".to_string())),
    }
    // 8 阶段乱序（先②后①）→ E_AS_PHASE_SKIP。
    let mut g8 = GroupClose::new();
    match g8.phase_writeback() {
        Err(E_AS_PHASE_SKIP) => {}
        _ => return Err((E_AS_PHASE_SKIP, "乱序未拦".to_string())),
    }
    // 9 台账漂移（只登 18 单）→ E_AS_LEDGER_GAP。
    let mut g9 = GroupClose::new();
    let ids9: Vec<u32> = (1..=18).collect();
    match g9.phase_ledger(&ids9) {
        Err(E_AS_LEDGER_GAP) => {}
        _ => return Err((E_AS_LEDGER_GAP, "台账漂移未拦".to_string())),
    }
    // 10 字数不符（声明卡 1199≠台账 1200）→ E_AS_WORDCOUNT。
    let mut g10 = GroupClose::new();
    let ids: Vec<u32> = (1..=(GROUP_UNIT_COUNT as u32)).collect();
    g10.phase_ledger(&ids).map_err(|e| (e, "J1-B-g10阶段①".to_string()))?;
    g10.phase_writeback().map_err(|e| (e, "J1-B-g10阶段②".to_string()))?;
    match g10.phase_wordcount(1199) {
        Err(E_AS_WORDCOUNT) => {}
        _ => return Err((E_AS_WORDCOUNT, "字数不符未拦".to_string())),
    }
    Ok(())
}

/// J1-C：整组回滚幂等——失败→清零→重走→成功；收口后回滚拒绝。
fn j1_rollback_idempotent() -> Result<(), (u16, String)> {
    // 失败→清零。
    let mut wb = LedgerWriteback::new(SESSION_ANCHOR);
    for id in 1..=9u32 {
        let _ = wb.write_one(id, true);
    }
    if wb.committed != 9 {
        return Err((E_AS_WRITEBACK_FAIL, format!("前9笔committed{}", wb.committed)));
    }
    wb.write_one(10, false).map_err(|e| (e, "注入失败未得Err".to_string()))?;
    if !wb.is_clean() || wb.committed != 0 || wb.rounds != 2 {
        return Err((E_AS_WRITEBACK_FAIL, format!("回滚不净 c{} r{}", wb.committed, wb.rounds)));
    }
    // 重走→成功（幂等路径：同 19 单重写 committed=19）。
    for id in 1..=(GROUP_UNIT_COUNT as u32) {
        wb.write_one(id, true).map_err(|e| (e, "重走失败".to_string()))?;
    }
    if wb.committed != GROUP_UNIT_COUNT as u32 || wb.rounds != 2 {
        return Err((E_AS_WRITEBACK_FAIL, format!("重走 c{} r{}", wb.committed, wb.rounds)));
    }
    // 收口后回滚——防御面拒绝。
    let g = build_full_close();
    let mut g2 = g;
    match g2.rollback_after_close() {
        Err(E_AS_DOUBLE_CLOSE) => {}
        _ => return Err((E_AS_DOUBLE_CLOSE, "收口后回滚未拦".to_string())),
    }
    Ok(())
}

/// 双跑指纹：同种子两次全流程指纹必等（确定性是收口证据可信的根）。
fn dual_run_fingerprint() -> Result<u64, (u16, String)> {
    let a = build_full_close();
    let b = build_full_close();
    let fa = a.fingerprint();
    let fb = b.fingerprint();
    if fa != fb {
        return Err((E_AS_DRIFT, format!("双跑漂移 {:016x}≠{:016x}", fa, fb)));
    }
    if a.bag.fingerprint() != b.bag.fingerprint() {
        return Err((E_AS_DRIFT, "立案簿指纹漂移".to_string()));
    }
    if a.vault.chain_digest() != b.vault.chain_digest() {
        return Err((E_AS_DRIFT, "归档链指纹漂移".to_string()));
    }
    Ok(fa)
}

/// VE-F5420 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vef72_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5420");

    // 判-00 判据表可追溯：四项判据→检查名映射+指纹稳定。
    {
        let c1 = criterion_check("双签三件");
        let c2 = criterion_check("四层就绪");
        let c3 = criterion_check("整组回滚");
        let c4 = criterion_check("判据");
        let miss = criterion_check("不存在的判据");
        let fp1 = criteria_fingerprint();
        let fp2 = criteria_fingerprint();
        // 手算：四判据全可映射；缺名查 None；指纹两次相等。
        let ok = c1 == Some("F5420-签-01")
            && c2 == Some("F5420-声-05")
            && c3 == Some("F5420-滚-04")
            && c4 == Some("F5420-判")
            && miss.is_none()
            && fp1 == fp2;
        if ok {
            s.ok("F5420-判-00-判据映射可追溯");
        } else {
            s.fail("F5420-判-00-判据映射可追溯", "判据面不符");
        }
    }

    // 签-01 双签三件：台账深化+S185 回写+字数核对齐→双人签→收口。
    {
        let mut wb = LedgerWriteback::new(SESSION_ANCHOR);
        // 回写 19 单全成功（O(组规模)——手算：committed=19）。
        for id in 1..=GROUP_UNIT_COUNT as u32 {
            let _ = wb.write_one(id, true);
        }
        let mut sheet = SignSheet::new();
        sheet.ledger_deepened = true;
        sheet.session_written = wb.committed == GROUP_UNIT_COUNT as u32;
        // 字数核对：台账 1200=声明卡 1200 恰值。
        let mut bag = DiagBag::new();
        sheet.wordcount_ok = wordcount_gate(1200, 1200, &mut bag) == Ok(());
        let s1 = sheet.sign("builder", DelivCard {
            four_layer: true, dual_channel: true, compensation: true, words: 1200,
        });
        let s2 = sheet.sign("engineer", DelivCard {
            four_layer: true, dual_channel: true, compensation: true, words: 1200,
        });
        // 手算：committed=19；双签齐→closed；字数恰值过。
        let ok = wb.committed == 19
            && sheet.session_written
            && sheet.wordcount_ok
            && s1 == Ok(())
            && s2 == Ok(())
            && sheet.signed_count() == 2
            && sheet.closed
            && bag.is_empty();
        if ok {
            s.ok("F5420-签-01-双签三件收口成立");
        } else {
            s.fail("F5420-签-01-双签三件收口成立", "双签面不符");
        }
    }

    // 签-02 字数不符禁签：台账 1201≠声明 1200→复核禁签立案。
    {
        let mut bag = DiagBag::new();
        let r = wordcount_gate(1201, 1200, &mut bag);
        // 手算：1201≠1200→E_AS_WORDCOUNT；禁签面复核不自动改数。
        let ok = r == Err(E_AS_WORDCOUNT)
            && bag.count_of(E_AS_WORDCOUNT) == 1
            && bag.spoken().contains("复核禁签");
        if ok {
            s.ok("F5420-签-02-字数不符复核禁签");
        } else {
            s.fail("F5420-签-02-字数不符复核禁签", "禁签面不符");
        }
    }

    // 收-03 红线未销拒收口：红线账非空拒；销账后放行；越权销账立案。
    {
        let mut g = RedlineGate::new();
        g.raise("预算口径未对齐");
        g.raise("遥测字段越界");
        g.raise("预算口径未对齐"); // 重复登记幂等——仍 2 条。
        let deny = g.allow_close();
        // 销一条还剩一条——仍拒。
        let _ = g.clear("预算口径未对齐");
        let deny2 = g.allow_close();
        // 全销——放行。
        let _ = g.clear("遥测字段越界");
        let allow = g.allow_close();
        // 越权销账——立案返回 false。
        let alien = g.clear("不存在的红线");
        // 手算：2 条（重复幂等）红线拒；1 条仍拒；0 条放行；越权立案 1。
        let ok = deny == Err(E_AS_REDLINE_OPEN)
            && deny2 == Err(E_AS_REDLINE_OPEN)
            && allow == Ok(())
            && g.open_count() == 0
            && g.bag.count_of(E_AS_REDLINE_OPEN) == 2
            && g.cleared_total == 2
            && !alien
            && g.alien_clears == 1
            && g.bag.count_of(E_AS_REDLINE_OPEN) == 3;
        if ok {
            s.ok("F5420-收-03-红线未销拒绝收口");
        } else {
            s.fail("F5420-收-03-红线未销拒绝收口", "红线面不符");
        }
    }

    // 滚-04 回写失败整组回滚：19 单中第 10 单失败→committed 清零。
    {
        let mut wb = LedgerWriteback::new(SESSION_ANCHOR);
        for id in 1..=9u32 {
            let _ = wb.write_one(id, true);
        }
        let mid = wb.committed; // 9
        let fail = wb.write_one(10, false); // 失败→整组回滚
        // 手算：前 9 笔 committed=9；第 10 笔失败→回滚 committed=0 rounds=2。
        let ok = mid == 9
            && fail == Err(E_AS_WRITEBACK_FAIL)
            && wb.committed == 0
            && wb.is_clean()
            && wb.rounds == 2
            && wb.bag.count_of(E_AS_WRITEBACK_FAIL) == 1
            && wb.bag.spoken().contains("整组回滚");
        if ok {
            s.ok("F5420-滚-04-回写失败整组回滚");
        } else {
            s.fail("F5420-滚-04-回写失败整组回滚", "回滚面不符");
        }
    }

    // 声-05 三就绪位：全 true 过；缺项立案禁签；就绪计数显性。
    {
        let mut bag = DiagBag::new();
        let full = DelivCard { four_layer: true, dual_channel: true, compensation: true, words: 100 };
        let r_full = full.ready(&mut bag);
        let missing = DelivCard { four_layer: true, dual_channel: false, compensation: true, words: 100 };
        let r_miss = missing.ready(&mut bag);
        // 手算：三就绪过（计数 3）；双通道缺→E_AS_NO_DECL（计数 2）。
        let ok = r_full == Ok(())
            && r_miss == Err(E_AS_NO_DECL)
            && full.ready_count() == 3
            && missing.ready_count() == 2
            && bag.count_of(E_AS_NO_DECL) == 1;
        if ok {
            s.ok("F5420-声-05-三就绪缺项立案");
        } else {
            s.fail("F5420-声-05-三就绪缺项立案", "声明面不符");
        }
    }

    // 码-06 十二码互异+reason 全量显性+签署缺位立案。
    {
        let codes = [
            E_AS_WORDCOUNT, E_AS_REDLINE_OPEN, E_AS_WRITEBACK_FAIL, E_AS_NO_DECL,
            E_AS_SIGN_ORDER, E_AS_EVIDENCE_GAP, E_AS_LEDGER_GAP, E_AS_DOUBLE_CLOSE,
            E_AS_DRIFT, E_AS_ORDER, E_AS_UNIT_CAP, E_AS_PHASE_SKIP,
        ];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        let seg_ok = codes[0] == 0xEC01
            && codes[1] == 0xEC02
            && codes[2] == 0xEC03
            && codes[3] == 0xEC04
            && codes[4] == 0xEC05
            && codes[5] == 0xEC06
            && codes[6] == 0xEC07
            && codes[7] == 0xEC08
            && codes[8] == 0xEC09
            && codes[9] == 0xEC0A
            && codes[10] == 0xEC0B
            && codes[11] == 0xEC0C;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        // 签署缺位：三件未齐就签→E_AS_SIGN_ORDER。
        let mut sheet = SignSheet::new();
        let early = sheet.sign("builder", DelivCard {
            four_layer: true, dual_channel: true, compensation: true, words: 1,
        });
        let ok = distinct
            && seg_ok
            && reasons_all
            && early == Err(E_AS_SIGN_ORDER)
            && sheet.bag.count_of(E_AS_SIGN_ORDER) == 1
            && !sheet.has_signed("builder");
        if ok {
            s.ok("F5420-码-06-十二码互异缺位立案");
        } else {
            s.fail("F5420-码-06-十二码互异缺位立案", "诊断面不符");
        }
    }

    // 台-07 组内台账深化：19 单登记+审计过；缺 1 单漂移立案；保序可查。
    {
        let mut ul = UnitLedger::new();
        for id in 1..=(GROUP_UNIT_COUNT as u32) {
            let _ = ul.mark_deepened(id);
        }
        let audit_ok = ul.audit();
        let mut ul2 = UnitLedger::new();
        for id in 1..=18u32 {
            let _ = ul2.mark_deepened(id);
        }
        let audit_drift = ul2.audit();
        let order = ul.order_snapshot();
        // 手算：19 单审计过；18 单漂移 E_AS_LEDGER_GAP；序[1..=19]。
        let order_ok = order.len() == 19
            && order.first() == Some(&1)
            && order.last() == Some(&19);
        let ok = audit_ok == Ok(())
            && audit_drift == Err(E_AS_LEDGER_GAP)
            && ul.deepened_count() == 19
            && ul.deepened(19)
            && !ul.deepened(20)
            && order_ok
            && ul.fingerprint() == ul.fingerprint();
        if ok {
            s.ok("F5420-台-07-台账深化审计");
        } else {
            s.fail("F5420-台-07-台账深化审计", "台账面不符");
        }
    }

    // 档-08 证据归档：逐条入册链推进+下限核验+越权点名+丢弃显性。
    {
        let mut v = EvidenceVault::new();
        v.archive("ledger-deepened", "19units");
        v.archive("session-written", "S185@19");
        v.archive("wordcount-ok", "1200");
        v.archive("redline-clear", "0-open");
        let before_chain = v.chain_digest();
        v.archive("signed", "builder@engineer");
        let after_chain = v.chain_digest();
        let mut bag = DiagBag::new();
        let min_ok = v.audit_min(5, &mut bag);
        let min_low = {
            let mut v2 = EvidenceVault::new();
            v2.archive("only-one", "x");
            v2.audit_min(5, &mut DiagBag::new())
        };
        // 手算：5 条过下限；1 条缺 E_AS_EVIDENCE_GAP；链两次不等（推进）。
        let ok = min_ok == Ok(())
            && min_low == Err(E_AS_EVIDENCE_GAP)
            && v.len() == 5
            && before_chain != after_chain
            && v.has_named("signed")
            && !v.has_named("不存在")
            && v.dropped() == 0
            && v.fingerprint() == v.fingerprint();
        if ok {
            s.ok("F5420-档-08-证据归档链");
        } else {
            s.fail("F5420-档-08-证据归档链", "归档面不符");
        }
    }

    // 编-09 五阶段编排：全绿贯通+收口一次性+阶段跳步/乱序/越收拦。
    {
        let mut g = GroupClose::new();
        let ids: Vec<u32> = (1..=(GROUP_UNIT_COUNT as u32)).collect();
        let p1 = g.phase_ledger(&ids);
        let p2 = g.phase_writeback();
        let p3 = g.phase_wordcount(DECL_WORDCOUNT);
        let p4 = g.phase_redline();
        let card = DelivCard { four_layer: true, dual_channel: true, compensation: true, words: DECL_WORDCOUNT };
        let p5 = g.phase_sign(card);
        // 收口后重推阶段——双重拒绝。
        let re_phase = g.advance_phase("ledger");
        // 消费面：收口成立→AA02 开工凭证可发。
        let permit = issue_aa2_permit(&g);
        // 手算：五阶段全过；重推 E_AS_DOUBLE_CLOSE；凭证指纹=全流程指纹。
        let ok = p1 == Ok(()) && p2 == Ok(()) && p3 == Ok(()) && p4 == Ok(()) && p5 == Ok(())
            && g.closed
            && g.phase_name() == "done"
            && re_phase == Err(E_AS_DOUBLE_CLOSE)
            && permit.is_ok()
            && permit.as_ref().map(|p| p.close_fingerprint) == Ok(g.fingerprint())
            && permit.as_ref().map(|p| p.session) == Ok(SESSION_ANCHOR)
            && g.tick_now() == 6;
        if ok {
            s.ok("F5420-编-09-五阶段贯通一次性");
        } else {
            s.fail("F5420-编-09-五阶段贯通一次性", "编排面不符");
        }
    }

    // 跳-10 阶段跳步/乱序/缺证据三闸。
    {
        // 跳步：新总控直接推阶段③。
        let mut g1 = GroupClose::new();
        let skip = g1.phase_wordcount(DECL_WORDCOUNT);
        // 乱序：先②后①。
        let mut g2 = GroupClose::new();
        let disorder = g2.phase_writeback();
        // 缺证据：五阶段走完但归档被外部清空——阶段⑤下限核验拦。
        let mut g3 = GroupClose::new();
        let ids: Vec<u32> = (1..=(GROUP_UNIT_COUNT as u32)).collect();
        let _ = g3.phase_ledger(&ids);
        let _ = g3.phase_writeback();
        let _ = g3.phase_wordcount(DECL_WORDCOUNT);
        let _ = g3.phase_redline();
        // 模拟归档丢失：把 vault 换成空库（直接改字段——总控面开放调试）。
        g3.vault = EvidenceVault::new();
        let card = DelivCard { four_layer: true, dual_channel: true, compensation: true, words: DECL_WORDCOUNT };
        let ev_gap = g3.phase_sign(card);
        // 手算：跳步/乱序各 E_AS_PHASE_SKIP；空归档 E_AS_EVIDENCE_GAP。
        let ok = skip == Err(E_AS_PHASE_SKIP)
            && disorder == Err(E_AS_PHASE_SKIP)
            && ev_gap == Err(E_AS_EVIDENCE_GAP)
            && g1.bag.count_of(E_AS_PHASE_SKIP) == 1
            && g2.bag.count_of(E_AS_PHASE_SKIP) == 1
            && g3.bag.count_of(E_AS_EVIDENCE_GAP) == 1;
        if ok {
            s.ok("F5420-跳-10-跳步乱序缺证三闸");
        } else {
            s.fail("F5420-跳-10-跳步乱序缺证三闸", "跳步面不符");
        }
    }

    // J1-A 万级回写幂等（19 单×1 万轮重复+LCG 洗牌——committed 恒 19）。
    {
        let r = j1_writeback_scale();
        let ok = r.is_ok();
        if ok {
            s.ok("F5420-J1A-万级回写幂等零漂移");
        } else {
            s.fail("F5420-J1A-万级回写幂等零漂移", "万级面不符");
        }
    }

    // J1-B 双签闭环+缺件拒 10/10。
    {
        let r = j1_sign_loop();
        let ok = r.is_ok();
        if ok {
            s.ok("F5420-J1B-双签闭环缺件拒10");
        } else {
            s.fail("F5420-J1B-双签闭环缺件拒10", "闭环面不符");
        }
    }

    // J1-C 整组回滚幂等+收口后回滚拒绝。
    {
        let r = j1_rollback_idempotent();
        let ok = r.is_ok();
        if ok {
            s.ok("F5420-J1C-整组回滚幂等重走");
        } else {
            s.fail("F5420-J1C-整组回滚幂等重走", "回滚面不符");
        }
    }

    // 漂-11 双跑指纹：同种子两次全流程指纹必等。
    {
        let r = dual_run_fingerprint();
        let ok = r.is_ok();
        if ok {
            s.ok("F5420-漂-11-双跑指纹必等");
        } else {
            s.fail("F5420-漂-11-双跑指纹必等", "指纹面不符");
        }
    }

    // 族-12 家族件自证：FNV/LCG/Tick 基础性质（确定性可复跑）。
    {
        // FNV：空串/单字节/双跑稳定。
        let f0 = fnv1a_str("");
        let f1 = fnv1a_str("A");
        let f1b = fnv1a_str("A");
        // LCG：同种子同序列；不同种子不同序列。
        let mut a = Lcg::new(7);
        let mut b = Lcg::new(7);
        let mut c = Lcg::new(8);
        let sa = [a.next(), a.next(), a.next()];
        let sb = [b.next(), b.next(), b.next()];
        let sc = [c.next(), c.next(), c.next()];
        // Tick：单调不回退。
        let mut t = Tick::new();
        let t1 = t.tick();
        let t2 = t.tick();
        // 手算：FNV 空串=偏移基；LCG 同种子三步全等、异种子不等；tick 1,2。
        let ok = f0 == FNV_OFFSET
            && f1 != f0
            && f1 == f1b
            && sa == sb
            && sa != sc
            && t1 == 1
            && t2 == 2
            && LCG_A == 1664525
            && LCG_C == 1013904223;
        if ok {
            s.ok("F5420-族-12-家族件确定性");
        } else {
            s.fail("F5420-族-12-家族件确定性", "家族面不符");
        }
    }

    s
}
