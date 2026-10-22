//! cga139_kmbase — CGPU-F0139 A 域知识库沉淀。
//!
//! 判据原文（施工书锚点 CGPU-F0139）：A 域方法论沉淀（F0398 范式）：
//! SIMD 开发方法论（跨指令集派发/精度纪律/确定性工程）、并行栅格化
//! 方法论、能效工程方法论、fuzz 工程方法论——四份方法论带八批实战
//! 案例，供 CGPU-B/C/G 域直接消费。判据：四份方法论、案例、消费验证、
//! 索引、更新流程。
//!
//! **设计定盘**：知识库账面化：四份方法论闭集（SIMD 开发/并行栅格化/
//! 能效工程/fuzz 工程），每份登记（指纹 + 章节数 + 版本）。案例账 =
//! 八批实战案例逐条登记（案例 → 归属方法论 + 案例指纹——无归属
//! 方法论的案例是孤魂，拒收）。消费验证 = 下游域（B/C/G）对方法论
//! 的引用登记（引用指纹必须与方法论指纹一致——引用了旧版即无效）。
//! 索引 = 方法论 → 案例的反查表投影。更新流程 = 方法论版本演进
//! （演进后旧版指纹保留为历史档——知识不抹史）。
//!
//! 诊断码段 0x02A0~0x02AF（0x02 段第 11 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA139_VERSION: &str = "cga139-kmbase-v1";

/// 诊断子区基值（0x02 段第 11 子区）。
pub const SEG_BASE: u16 = 0x02A0;

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 四份方法论闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Methodology {
    /// SIMD 开发方法论。
    SimdDev,
    /// 并行栅格化方法论。
    ParallelRaster,
    /// 能效工程方法论。
    PowerEng,
    /// fuzz 工程方法论。
    FuzzEng,
}

impl Methodology {
    /// 全集稳定序。
    pub const ALL: [Methodology; 4] = [
        Methodology::SimdDev,
        Methodology::ParallelRaster,
        Methodology::PowerEng,
        Methodology::FuzzEng,
    ];

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            Methodology::SimdDev => 0,
            Methodology::ParallelRaster => 1,
            Methodology::PowerEng => 2,
            Methodology::FuzzEng => 3,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 方法论登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MethodEntry {
    /// 方法论。
    pub m: Methodology,
    /// 当前版本指纹。
    pub fp: u64,
    /// 章节数。
    pub sections: u32,
    /// 演进代数（每次更新 +1）。
    pub revisions: u32,
}

/// 一条实战案例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseEntry {
    /// 案例号。
    pub id: u32,
    /// 归属方法论。
    pub m: Methodology,
    /// 案例指纹。
    pub fp: u64,
}

/// 消费验证条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConsumeEntry {
    /// 消费域（0=B 1=C 2=G）。
    pub domain: u8,
    /// 消费的方法论。
    pub m: Methodology,
    /// 引用指纹（必须与当前版一致）。
    pub cited_fp: u64,
}

/// 知识库账。
pub struct KmLedger {
    /// 方法论册。
    methods: Vec<MethodEntry>,
    /// 历史版本档（方法论, 旧指纹）。
    history: Vec<(Methodology, u64)>,
    /// 案例册。
    cases: Vec<CaseEntry>,
    /// 消费验证册。
    consumes: Vec<ConsumeEntry>,
    /// 封库。
    sealed: Option<u64>,
}

impl KmLedger {
    /// 空库。
    pub fn new() -> Self {
        KmLedger {
            methods: Vec::new(),
            history: Vec::new(),
            cases: Vec::new(),
            consumes: Vec::new(),
            sealed: None,
        }
    }

    /// 方法论入库（重复即拒；章节数下限 3——撑不起方法论的正文
    /// 不叫方法论）。
    pub fn register(
        &mut self,
        m: Methodology,
        body: &[u8],
        sections: u32,
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if self.methods.iter().any(|e| e.m == m) {
            return Err(SEG_BASE + 6);
        }
        if sections < 3 || body.is_empty() {
            return Err(SEG_BASE + 7);
        }
        self.methods.push(MethodEntry {
            m,
            fp: fnv1a64(body),
            sections,
            revisions: 0,
        });
        Ok(())
    }

    /// 方法论更新（判据"更新流程"：旧指纹进历史档，代数 +1；
    /// 未入库即拒）。
    pub fn revise(&mut self, m: Methodology, body: &[u8]) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if body.is_empty() {
            return Err(SEG_BASE + 8);
        }
        let new_fp = fnv1a64(body);
        let idx = match self.methods.iter().position(|e| e.m == m) {
            Some(i) => i,
            None => return Err(SEG_BASE + 9),
        };
        let old = self.methods[idx].fp;
        if old == new_fp {
            return Err(SEG_BASE + 10);
        }
        self.history.push((m, old));
        self.methods[idx].fp = new_fp;
        self.methods[idx].revisions += 1;
        Ok(new_fp)
    }

    /// 案例入册（判据"案例"：归属方法论必须在库；重复案例号即拒）。
    pub fn add_case(&mut self, id: u32, m: Methodology, body: &[u8]) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if id == 0 || self.cases.iter().any(|c| c.id == id) {
            return Err(SEG_BASE + 11);
        }
        if !self.methods.iter().any(|e| e.m == m) {
            return Err(m.code());
        }
        if body.is_empty() {
            return Err(SEG_BASE + 12);
        }
        self.cases.push(CaseEntry {
            id,
            m,
            fp: fnv1a64(body),
        });
        Ok(())
    }

    /// 消费验证登记（判据"消费验证"：引用指纹与方法论当前版一致
    /// 才算有效消费；旧版引用拒绝——下游必须升到当前版）。
    pub fn consume(&mut self, domain: u8, m: Methodology, cited_fp: u64) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 5);
        }
        if domain > 2 {
            return Err(SEG_BASE + 13);
        }
        let cur = self
            .methods
            .iter()
            .find(|e| e.m == m)
            .ok_or(m.code())?
            .fp;
        if cited_fp != cur {
            return Err(SEG_BASE + 14);
        }
        if self
            .consumes
            .iter()
            .any(|c| c.domain == domain && c.m == m)
        {
            return Err(SEG_BASE + 15);
        }
        self.consumes.push(ConsumeEntry {
            domain,
            m,
            cited_fp,
        });
        Ok(())
    }

    /// 索引投影（判据"索引"：方法论 → 案例数的反查表）。
    pub fn case_index(&self) -> Vec<(Methodology, usize)> {
        let mut out: Vec<(Methodology, usize)> = Vec::new();
        for m in Methodology::ALL {
            let n = self.cases.iter().filter(|c| c.m == m).count();
            out.push((m, n));
        }
        out
    }

    /// 四份方法论齐核验（判据"四份方法论"）。
    pub fn methods_complete(&self) -> Result<(), u16> {
        for m in Methodology::ALL {
            if !self.methods.iter().any(|e| e.m == m) {
                return Err(m.code());
            }
        }
        Ok(())
    }

    /// 案例数（判据"八批实战案例"）。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 历史档数。
    pub fn history_count(&self) -> usize {
        self.history.len()
    }

    /// 消费验证数。
    pub fn consume_count(&self) -> usize {
        self.consumes.len()
    }

    /// 封库（判据"四份齐 + 案例非空"才能封）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 16);
        }
        self.methods_complete()?;
        if self.cases.is_empty() {
            return Err(SEG_BASE + 17);
        }
        let mut fp = FNV_OFFSET;
        for e in &self.methods {
            fp = fp.wrapping_mul(FNV_PRIME) ^ e.fp;
        }
        for c in &self.cases {
            fp = fp.wrapping_mul(FNV_PRIME) ^ c.fp;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for KmLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl KmLedger {
    /// 方法论指纹查询（判据"消费验证"的引用面：下游取当前版指纹
    /// 用此入口——拿到什么引什么，引用即对齐）。
    pub fn current_fp(&self, m: Methodology) -> Option<u64> {
        self.methods.iter().find(|e| e.m == m).map(|e| e.fp)
    }

    /// 方法论演进代数投影。
    pub fn revision_census(&self) -> Vec<(Methodology, u32)> {
        self.methods.iter().map(|e| (e.m, e.revisions)).collect()
    }

    /// 消费覆盖核验（判据"消费验证"的完整性面：B/C/G 三域全部消费
    /// 过——缺一域即知识库无人用）。
    pub fn consume_domains_covered(&self) -> Result<(), u16> {
        for d in 0u8..3 {
            if !self.consumes.iter().any(|c| c.domain == d) {
                return Err(SEG_BASE + 18);
            }
        }
        Ok(())
    }

    /// 方法论数。
    pub fn method_count(&self) -> usize {
        self.methods.len()
    }

    /// 封库指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }
}
