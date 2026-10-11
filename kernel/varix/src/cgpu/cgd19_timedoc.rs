//! cgd19_timedoc — CGPU-F0493 帧计时文档（合同条款+方法论+查询手册——无歧义契约）。
//!
//! 判据原文（施工书锚点 CGPU-F0493）：文档：合同条款文档（80 帧合同的每条
//! 定义/测量方法/判定规则——无歧义）；计时方法论（四段定义/校准方法）；账
//! 本查询手册。判据：合同文档、方法论、手册、判据。
//!
//! 职责定位：合同条款文档（80 帧合同逐条登记，每条必须三要素齐备：定义
//! （这条约束什么）/测量方法（怎么量出来）/判定规则（怎么判达标）——缺一
//! 即歧义 0x7802，条款数不足 0x7801）+计时方法论（四段定义闭集+校准方法
//! 锚 cgd09 口径——段缺 0x7803）+账本查询手册（查询条目表：键名+返回形状
//! ——条目缺 0x7804）+新鲜度（实现指纹推进未重对拍 0x7805）。诊断码段
//! 0x7801~0x7805。

use alloc::vec::Vec;

/// 模块版本。
pub const TIME_DOC_VERSION: &str = "D19-timedoc-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x78 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7801 合同条款缺（80 帧合同条目不全）。
pub const XC_CONTRACT_SHORT: u32 = 0x7801;
/// 0x7802 条款歧义（定义/测量方法/判定规则三要素缺一）。
pub const XC_CLAUSE_AMBIGUOUS: u32 = 0x7802;
/// 0x7803 方法论缺（四段定义不全）。
pub const XC_METHODOLOGY_MISS: u32 = 0x7803;
/// 0x7804 手册缺（账本查询手册条目缺）。
pub const XC_MANUAL_MISS: u32 = 0x7804;
/// 0x7805 文档过期（指纹未随实现更新）。
pub const XC_DOC_OUTDATED: u32 = 0x7805;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 合同条款数（80 帧合同——条款闭集）。
pub const CONTRACT_CLAUSES: usize = 8;
/// 计时方法论段数（四段闭集）。
pub const METHODOLOGY_SECTIONS: usize = 4;
/// 账本手册条目数。
pub const MANUAL_ENTRIES: usize = 4;
/// 文档指纹种子。
pub const TIME_DOC_FP_SEED: u64 = 0x71D0_C000_0000_1943;

// ---------------------------------------------------------------------------
// 一、合同条款文档（三要素齐备——无歧义）
// ---------------------------------------------------------------------------

/// 合同条款（定义/测量方法/判定规则三要素）。
#[derive(Clone, Copy, Debug)]
pub struct ContractClause {
    /// 条款号。
    pub no: usize,
    /// 定义锚（语义指纹——非零）。
    pub definition: u32,
    /// 测量方法锚（口径单号——非零）。
    pub measurement: u32,
    /// 判定规则锚（阈值单号——非零）。
    pub rule: u32,
}

impl ContractClause {
    /// 三要素齐备校验（缺一即歧义）。
    pub const fn unambiguous(&self) -> bool {
        self.definition != 0 && self.measurement != 0 && self.rule != 0
    }
}

/// 合同条款文档（判据"80 帧合同的每条定义/测量方法/判定规则"）。
#[derive(Debug)]
pub struct ContractDoc {
    /// 条款表。
    pub clauses: Vec<ContractClause>,
    /// 条款缺事件数（0x7801）。
    pub short_events: u32,
    /// 歧义事件数（0x7802）。
    pub ambiguous_events: u32,
}

impl ContractDoc {
    /// 新文档（空）。
    pub const fn new() -> ContractDoc {
        ContractDoc {
            clauses: Vec::new(),
            short_events: 0,
            ambiguous_events: 0,
        }
    }

    /// 加条款（歧义条款拒收 0x7802）。
    pub fn add(&mut self, c: ContractClause) -> Result<(), u32> {
        if !c.unambiguous() {
            self.ambiguous_events += 1;
            return Err(XC_CLAUSE_AMBIGUOUS);
        }
        self.clauses.push(c);
        Ok(())
    }

    /// 完备裁决：条款数达标+逐条无歧义（0x7801/0x7802）。
    pub fn complete(&mut self) -> Result<(), u32> {
        if self.clauses.len() < CONTRACT_CLAUSES {
            self.short_events += 1;
            return Err(XC_CONTRACT_SHORT);
        }
        let mut i = 0;
        while i < self.clauses.len() {
            if !self.clauses[i].unambiguous() {
                self.ambiguous_events += 1;
                return Err(XC_CLAUSE_AMBIGUOUS);
            }
            i += 1;
        }
        Ok(())
    }

    /// 条款指纹（合成——过期检测用）。
    pub const fn fingerprint(&self) -> u64 {
        // 简化指纹：条款数+首条锚合成（u64 稳定口径）。
        (self.clauses.len() as u64) << 32 | (TIME_DOC_FP_SEED & 0xFFFF_FFFF)
    }
}

// ---------------------------------------------------------------------------
// 二、计时方法论（四段定义+校准方法）
// ---------------------------------------------------------------------------

/// 方法论四段（闭集：间隔定义/全延迟定义/达标定义/卡顿定义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MethodSection {
    Interval,
    EndToEnd,
    Compliance,
    Jank,
}

impl MethodSection {
    /// 四段闭集。
    pub const ALL: [MethodSection; METHODOLOGY_SECTIONS] = [
        MethodSection::Interval,
        MethodSection::EndToEnd,
        MethodSection::Compliance,
        MethodSection::Jank,
    ];

    /// 序号。
    pub const fn idx(self) -> usize {
        match self {
            MethodSection::Interval => 0,
            MethodSection::EndToEnd => 1,
            MethodSection::Compliance => 2,
            MethodSection::Jank => 3,
        }
    }
}

/// 方法论文档（判据"计时方法论（四段定义/校准方法"）。
#[derive(Debug)]
pub struct MethodologyDoc {
    /// 已登记段位图。
    pub sections: u32,
    /// 校准方法锚（cgd09 单号——非零=校准口径同源）。
    pub calibration_anchor: u32,
    /// 段缺事件数（0x7803）。
    pub miss_events: u32,
}

impl MethodologyDoc {
    /// 新文档（校准锚未挂）。
    pub const fn new() -> MethodologyDoc {
        MethodologyDoc {
            sections: 0,
            calibration_anchor: 0,
            miss_events: 0,
        }
    }

    /// 登记段。
    pub fn register(&mut self, s: MethodSection) {
        self.sections |= 1 << s.idx();
    }

    /// 完备裁决：四段全登记+校准锚非零（0x7803）。
    pub fn complete(&mut self) -> Result<(), u32> {
        let want = (1 << METHODOLOGY_SECTIONS) - 1;
        if self.sections != want || self.calibration_anchor == 0 {
            self.miss_events += 1;
            return Err(XC_METHODOLOGY_MISS);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、账本查询手册
// ---------------------------------------------------------------------------

/// 手册条目（查询键+返回形状码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ManualEntry {
    /// 查询键（小写下划线命名——与 cgd12 命名一致性同源）。
    pub key: [u8; 12],
    /// 键长。
    pub key_len: usize,
    /// 返回形状码。
    pub ret_shape: u32,
}

/// 账本查询手册（判据"账本查询手册"）。
#[derive(Debug)]
pub struct LedgerManual {
    /// 条目表。
    pub entries: Vec<ManualEntry>,
    /// 条目缺事件数（0x7804）。
    pub miss_events: u32,
}

impl LedgerManual {
    /// 新手册（空）。
    pub const fn new() -> LedgerManual {
        LedgerManual {
            entries: Vec::new(),
            miss_events: 0,
        }
    }

    /// 加条目（键空/形状空拒 0x7804）。
    pub fn add(&mut self, key: &[u8], ret_shape: u32) -> Result<(), u32> {
        if key.is_empty() || ret_shape == 0 {
            self.miss_events += 1;
            return Err(XC_MANUAL_MISS);
        }
        // 重复键拒（手册一键一义）。
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.key_len == key.len().min(12) {
                let mut same = true;
                let mut j = 0;
                while j < key.len().min(12) {
                    if e.key[j] != key[j] {
                        same = false;
                        j = 12;
                    }
                    j += 1;
                }
                if same {
                    self.miss_events += 1;
                    return Err(XC_MANUAL_MISS);
                }
            }
            i += 1;
        }
        let mut k = [0u8; 12];
        let mut n = 0;
        while n < key.len().min(12) {
            k[n] = key[n];
            n += 1;
        }
        self.entries.push(ManualEntry {
            key: k,
            key_len: key.len().min(12),
            ret_shape,
        });
        Ok(())
    }

    /// 完备裁决：条目数达标（0x7804）。
    pub fn complete(&mut self) -> Result<(), u32> {
        if self.entries.len() < MANUAL_ENTRIES {
            self.miss_events += 1;
            return Err(XC_MANUAL_MISS);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、新鲜度（实现指纹对拍）
// ---------------------------------------------------------------------------

/// 文档新鲜度账（判据"判据"的机检面——过期 0x7805）。
#[derive(Debug)]
pub struct DocFreshness {
    /// 最近对拍通过时的实现指纹。
    pub synced_fp: u64,
    /// 过期事件数（0x7805）。
    pub stale_events: u32,
}

impl DocFreshness {
    /// 新账（未对拍态）。
    pub const fn new() -> DocFreshness {
        DocFreshness {
            synced_fp: 0,
            stale_events: 0,
        }
    }

    /// 对拍通过（记录实现指纹）。
    pub fn sync(&mut self, impl_fp: u64) {
        self.synced_fp = impl_fp;
    }

    /// 新鲜裁决（0x7805）。
    pub fn fresh(&mut self, current: u64) -> Result<(), u32> {
        if self.synced_fp == 0 || self.synced_fp != current {
            self.stale_events += 1;
            return Err(XC_DOC_OUTDATED);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 自检（合同/方法论/手册/新鲜/判据五组+段对账）
// ---------------------------------------------------------------------------

/// 帧计时文档总引擎。
#[derive(Debug)]
pub struct TimeDocEngine {
    /// 合同条款文档。
    pub contract: ContractDoc,
    /// 方法论文档。
    pub method: MethodologyDoc,
    /// 账本手册。
    pub manual: LedgerManual,
    /// 新鲜度账。
    pub fresh: DocFreshness,
}

impl TimeDocEngine {
    /// 出厂。
    pub const fn new() -> TimeDocEngine {
        TimeDocEngine {
            contract: ContractDoc::new(),
            method: MethodologyDoc::new(),
            manual: LedgerManual::new(),
            fresh: DocFreshness::new(),
        }
    }

    /// 组一：合同——歧义条款拒；条款数达标；全量齐备过。
    pub fn selfcheck_contract(&mut self) -> Result<(), u32> {
        let mut e = TimeDocEngine::new();
        // 歧义条款（缺判定规则）→0x7802。
        if e.contract.add(ContractClause { no: 0, definition: 1, measurement: 2, rule: 0 })
            != Err(XC_CLAUSE_AMBIGUOUS)
        {
            return Err(XC_CLAUSE_AMBIGUOUS);
        }
        if e.contract.ambiguous_events != 1 {
            return Err(XC_CLAUSE_AMBIGUOUS);
        }
        // 条款不足→0x7801。
        if e.contract.complete() != Err(XC_CONTRACT_SHORT) {
            return Err(XC_CONTRACT_SHORT);
        }
        // 八条款全加（三要素齐备）→过。
        let mut i = 0;
        while i < CONTRACT_CLAUSES {
            e.contract.add(ContractClause {
                no: i,
                definition: 0x1000 + i as u32,
                measurement: 0x2000 + i as u32,
                rule: 0x3000 + i as u32,
            })?;
            i += 1;
        }
        e.contract.complete()?;
        Ok(())
    }

    /// 组二：方法论——四段登记；校准锚挂接；缺一 0x7803。
    pub fn selfcheck_method(&mut self) -> Result<(), u32> {
        let mut e = TimeDocEngine::new();
        // 只登记两段→0x7803。
        e.method.register(MethodSection::Interval);
        e.method.register(MethodSection::EndToEnd);
        if e.method.complete() != Err(XC_METHODOLOGY_MISS) {
            return Err(XC_METHODOLOGY_MISS);
        }
        // 四段齐但校准锚空→0x7803。
        e.method.register(MethodSection::Compliance);
        e.method.register(MethodSection::Jank);
        if e.method.complete() != Err(XC_METHODOLOGY_MISS) {
            return Err(XC_METHODOLOGY_MISS);
        }
        // 挂校准锚（cgd09 单号）→过。
        e.method.calibration_anchor = 0x0489;
        e.method.complete()?;
        Ok(())
    }

    /// 组三：手册——键空拒；重复键拒；四条目齐过。
    pub fn selfcheck_manual(&mut self) -> Result<(), u32> {
        let mut e = TimeDocEngine::new();
        // 键空→0x7804。
        if e.manual.add(b"", 0x5250) != Err(XC_MANUAL_MISS) {
            return Err(XC_MANUAL_MISS);
        }
        // 形状空→0x7804。
        if e.manual.add(b"ledger_query", 0) != Err(XC_MANUAL_MISS) {
            return Err(XC_MANUAL_MISS);
        }
        // 四条目入册。
        e.manual.add(b"frame_cost", 0x5330)?;
        e.manual.add(b"ledger_span", 0x5331)?;
        e.manual.add(b"jank_count", 0x5332)?;
        e.manual.add(b"compliance", 0x5333)?;
        // 重复键→0x7804。
        if e.manual.add(b"frame_cost", 0x5330) != Err(XC_MANUAL_MISS) {
            return Err(XC_MANUAL_MISS);
        }
        if e.manual.miss_events != 3 {
            return Err(XC_MANUAL_MISS);
        }
        e.manual.complete()?;
        Ok(())
    }

    /// 组四：新鲜——未对拍过期；指纹推进过期；重对拍新鲜。
    pub fn selfcheck_freshness(&mut self) -> Result<(), u32> {
        let mut e = TimeDocEngine::new();
        // 未对拍→0x7805。
        if e.fresh.fresh(0xF00D) != Err(XC_DOC_OUTDATED) {
            return Err(XC_DOC_OUTDATED);
        }
        // 对拍 0xF00D→新鲜。
        e.fresh.sync(0xF00D);
        e.fresh.fresh(0xF00D)?;
        // 实现推进 0xF00E 未重对拍→过期。
        if e.fresh.fresh(0xF00E) != Err(XC_DOC_OUTDATED) {
            return Err(XC_DOC_OUTDATED);
        }
        if e.fresh.stale_events != 1 {
            return Err(XC_DOC_OUTDATED);
        }
        // 重对拍→新鲜。
        e.fresh.sync(0xF00E);
        e.fresh.fresh(0xF00E)?;
        Ok(())
    }

    /// 组五：判据——端到端：合同+方法论+手册+新鲜四件全链绿。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = TimeDocEngine::new();
        let mut i = 0;
        while i < CONTRACT_CLAUSES {
            e.contract.add(ContractClause {
                no: i,
                definition: 0x10 + i as u32,
                measurement: 0x20 + i as u32,
                rule: 0x30 + i as u32,
            })?;
            i += 1;
        }
        e.contract.complete()?;
        let mut s = 0;
        while s < METHODOLOGY_SECTIONS {
            e.method.register(MethodSection::ALL[s]);
            s += 1;
        }
        e.method.calibration_anchor = 0x0489;
        e.method.complete()?;
        e.manual.add(b"frame_cost", 0x5330)?;
        e.manual.add(b"ledger_span", 0x5331)?;
        e.manual.add(b"jank_count", 0x5332)?;
        e.manual.add(b"compliance", 0x5333)?;
        e.manual.complete()?;
        e.fresh.sync(0xCAFE);
        e.fresh.fresh(0xCAFE)?;
        // 全链账面零事件。
        if e.contract.short_events != 0
            || e.contract.ambiguous_events != 0
            || e.method.miss_events != 0
            || e.manual.miss_events != 0
            || e.fresh.stale_events != 0
        {
            return Err(XC_CONTRACT_SHORT);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_contract()?;
        self.selfcheck_method()?;
        self.selfcheck_manual()?;
        self.selfcheck_freshness()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7800..0x7900 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_CONTRACT_SHORT,
            XC_CLAUSE_AMBIGUOUS,
            XC_METHODOLOGY_MISS,
            XC_MANUAL_MISS,
            XC_DOC_OUTDATED,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7800 || codes[i] >= 0x7900 {
                return Err(XC_CONTRACT_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_CONTRACT_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
