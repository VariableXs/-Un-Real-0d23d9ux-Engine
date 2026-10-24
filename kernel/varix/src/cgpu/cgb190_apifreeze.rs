//! CGPU-F0190 · 帧图 API 冻结（判据段 0x0580）
//! 帧图对外 API 冻结 v1：图构建 API（节点/边/子图/模板——渲染域
//! 构建帧图的接口）、图查询 API（节点状态/预算/耗时——诊断面板
//! 数据源）、图控制 API（暂停/恢复/取消节点——调试与降级）、图
//! 事件 API（完成/失败/预算事件——订阅接口）。判据：四族 API 冻结、
//! 语义测试、十年承诺、文档、示例。
//!
//! 诊断码段 0x0580（本模块独占：0x0580~0x058F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0580;

/// API 族（判据「四族 API 冻结」闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    /// 图构建 API。
    Build,
    /// 图查询 API。
    Query,
    /// 图控制 API。
    Control,
    /// 图事件 API。
    Event,
}

impl ApiFamily {
    /// 序号。
    pub fn rank(self) -> u8 {
        match self {
            ApiFamily::Build => 0,
            ApiFamily::Query => 1,
            ApiFamily::Control => 2,
            ApiFamily::Event => 3,
        }
    }
}

/// API 签名（判据「四族 API 冻结」原子）：族 + 名指纹 + 参数布局
/// 指纹 + 返回语义码。冻结 = 签名指纹不可再变。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApiSig {
    /// 族。
    pub family: ApiFamily,
    /// 名字指纹。
    pub name_fp: u64,
    /// 参数布局指纹。
    pub param_fp: u64,
    /// 返回语义码（0=值 1=Result 2=订阅流）。
    pub ret: u8,
}

impl ApiSig {
    /// 签名校验：名/参指纹非零、返回码在闭集。
    pub fn validate(&self) -> Result<(), u16> {
        if self.name_fp == 0 || self.param_fp == 0 {
            return Err(SEG_BASE);
        }
        if self.ret > 2 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }

    /// 签名指纹（冻结对账键）。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = h.wrapping_mul(0x100000001b3) ^ self.family.rank() as u64;
        h = h.wrapping_mul(0x100000001b3) ^ self.name_fp;
        h = h.wrapping_mul(0x100000001b3) ^ self.param_fp;
        h.wrapping_mul(0x100000001b3) ^ self.ret as u64
    }
}

/// API 冻结册（判据「四族 API 冻结」载体）：签名集 + 冻结态 +
/// 十年承诺（判据「十年承诺」：冻结后签名只增不改——改签名 =
/// 主版本事件）。
pub struct ApiFreezeBook {
    /// 签名集。
    pub sigs: Vec<ApiSig>,
    /// 冻结版本号（v1）。
    pub version: u32,
    /// 已冻结。
    pub frozen: bool,
    /// 冻结后的漂移立案（判据「十年承诺」检测面）。
    pub drifts: Vec<u64>,
}

impl ApiFreezeBook {
    /// 空册（v1）。
    pub fn new() -> Self {
        ApiFreezeBook {
            sigs: Vec::new(),
            version: 1,
            frozen: false,
            drifts: Vec::new(),
        }
    }

    /// 挂签名（冻结后拒——加签名也要升版本，v1 内冻结即封口）。
    pub fn attach(&mut self, s: ApiSig) -> Result<(), u16> {
        s.validate()?;
        if self.frozen {
            return Err(SEG_BASE + 2);
        }
        if self.sigs.iter().any(|x| x.fp() == s.fp()) {
            return Err(SEG_BASE + 3);
        }
        self.sigs.push(s);
        Ok(())
    }

    /// 四族齐判（判据「四族 API 冻结」门槛）：四族各至少一签名。
    pub fn families_complete(&self) -> Result<(), u16> {
        for f in [
            ApiFamily::Build,
            ApiFamily::Query,
            ApiFamily::Control,
            ApiFamily::Event,
        ] {
            if !self.sigs.iter().any(|s| s.family == f) {
                return Err(SEG_BASE + 4);
            }
        }
        Ok(())
    }

    /// 冻结（判据核心：族齐才可冻结）。
    pub fn freeze(&mut self) -> Result<u64, u16> {
        self.families_complete()?;
        if self.frozen {
            return Err(SEG_BASE + 5);
        }
        self.frozen = true;
        let mut h: u64 = 0xcbf29ce484222325;
        for s in &self.sigs {
            h = h.wrapping_mul(0x100000001b3) ^ s.fp();
        }
        if h == 0 {
            return Err(SEG_BASE + 6);
        }
        Ok(h)
    }

    /// 漂移检测（判据「十年承诺」核心面）：冻结后同族同名指纹的
    /// 参数布局变化即漂移立案——返回旧签名指纹。
    pub fn drift_check(&mut self, current: &ApiSig) -> Result<Option<u64>, u16> {
        current.validate()?;
        if !self.frozen {
            return Err(SEG_BASE + 7);
        }
        let m = self
            .sigs
            .iter()
            .find(|s| s.family == current.family && s.name_fp == current.name_fp);
        match m {
            None => Err(SEG_BASE + 8), // 冻结后新签名 = 未承诺 API
            Some(old) => {
                if old.param_fp != current.param_fp {
                    self.drifts.push(old.fp());
                    Ok(Some(old.fp()))
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// 漂移零判（判据汇总）：冻结册零漂移。
    pub fn drift_free(&self) -> Result<(), u16> {
        if !self.drifts.is_empty() {
            return Err(SEG_BASE + 9);
        }
        Ok(())
    }
}

/// 语义测试记录（判据「语义测试」载体）：签名 → 用例数 → 通过数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SemCase {
    /// 对应签名的指纹。
    pub sig_fp: u64,
    /// 用例数。
    pub cases: u32,
    /// 通过数。
    pub passed: u32,
}

impl SemCase {
    /// 校验：通过 ≤ 用例数、用例 ≥ 2（单用例不算语义测试）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.sig_fp == 0 || self.cases < 2 || self.passed > self.cases {
            return Err(SEG_BASE + 10);
        }
        Ok(())
    }
}

/// 语义测试全通过判（判据「语义测试」批量面）：每条冻结签名
/// 都有语义记录且全通过。
pub fn semantic_green(sigs: &[ApiSig], cases: &[SemCase]) -> Result<(), u16> {
    if sigs.is_empty() || cases.is_empty() {
        return Err(SEG_BASE + 11);
    }
    for s in sigs {
        let c = cases
            .iter()
            .find(|c| c.sig_fp == s.fp())
            .ok_or(SEG_BASE + 12)?;
        c.validate()?;
        if c.passed != c.cases {
            return Err(SEG_BASE + 13);
        }
    }
    Ok(())
}

/// 文档与示例齐判（判据「文档」「示例」面）：每族至少一条文档
/// 锚 + 一条示例锚（指纹非零）。
pub fn doc_example_green(anchors: &[(u8, u64, u64)]) -> Result<(), u16> {
    for fam in 0..4u8 {
        let hit = anchors.iter().find(|(f, _, _)| *f == fam);
        match hit {
            Some((_, d, e)) if *d != 0 && *e != 0 => {}
            _ => return Err(SEG_BASE + 14),
        }
    }
    Ok(())
}

/// API 冻结一站式审计（总闸）：族齐 + 冻结 + 零漂移 + 语义绿 +
/// 文档示例绿。
pub fn api_freeze_audit(
    book: &mut ApiFreezeBook,
    cases: &[SemCase],
    anchors: &[(u8, u64, u64)],
) -> Result<u64, u16> {
    let fp = book.freeze()?;
    book.drift_free()?;
    semantic_green(&book.sigs, cases)?;
    doc_example_green(anchors)?;
    Ok(fp)
}

/// API 冻结册深度扩展面。
impl ApiFreezeBook {
    /// 册规模。
    pub fn count(&self) -> usize {
        self.sigs.len()
    }

    /// 册指纹（冻结对账总键）。
    pub fn book_fp(&self) -> Result<u64, u16> {
        if self.sigs.is_empty() {
            return Err(SEG_BASE + 15);
        }
        let mut h: u64 = 0xcbf29ce484222325;
        for s in &self.sigs {
            h = h.wrapping_mul(0x100000001b3) ^ s.fp();
        }
        h = h.wrapping_mul(0x100000001b3) ^ self.version as u64;
        Ok(h)
    }

    /// 族普查（判据「四族 API 冻结」可见性）。
    pub fn family_census(&self) -> Result<[usize; 4], u16> {
        let mut out = [0usize; 4];
        for s in &self.sigs {
            out[s.family.rank() as usize] += 1;
        }
        Ok(out)
    }

    /// 版本事件审计（判据「十年承诺」强化面）：漂移立案非空时
    /// 必须升主版本（v1→v2）——漂移与版本联动是承诺的执行机制。
    pub fn version_event_required(&self) -> Result<bool, u16> {
        if !self.frozen {
            return Err(SEG_BASE + 16);
        }
        Ok(!self.drifts.is_empty())
    }
}

/// 语义测试台账指纹（独立函数面）。
pub fn semantic_fp(cases: &[SemCase]) -> Result<u64, u16> {
    if cases.is_empty() {
        return Err(SEG_BASE + 17);
    }
    let mut h: u64 = 0xcbf29ce484222325;
    for c in cases {
        c.validate()?;
        h = h.wrapping_mul(0x100000001b3) ^ c.sig_fp;
        h = h.wrapping_mul(0x100000001b3) ^ c.cases as u64;
    }
    Ok(h)
}

/// 事件 API 语义闭集（独立函数面）：图事件三事件（完成/失败/预算）
/// 必须全部有订阅签名（判据「图事件 API」的事件闭集）。
pub fn event_api_closed(book: &ApiFreezeBook) -> Result<(), u16> {
    if !book.frozen {
        return Err(SEG_BASE + 18);
    }
    let events = book
        .sigs
        .iter()
        .filter(|s| s.family == ApiFamily::Event)
        .count();
    if events < 3 {
        return Err(SEG_BASE + 19);
    }
    Ok(())
}
