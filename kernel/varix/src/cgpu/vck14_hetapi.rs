//! vck14_hetapi — CGPU-F1614 异构 API 冻结 v1（14 函数三要素+只增承诺+开放位）。
//!
//! 判据原文（施工书锚点 CGPU-F1614）：API 冻结：冻结清单（K01 组对外 API
//! （引擎注册/能力查询/任务提交/同步/遥测——14 函数签名——冻结 v1 十年承
//! 诺）；三要素（每函数{签名/语义/错误路径}完备）；兼容承诺（v1 后只增不
//! 改——只增承诺；扩展位（新引擎类型注册扩展位（未来加速器（枚举开放复用
//! F0406 精神——开放位；文档（API 参考文档+异构指南——文档两件；与 B/J 域
//! API 对齐（跨域调用签名一致——一致性核验；测试（清单/三要素/兼容/扩展
//! 位/对齐五组）。判据：14 函数、三要素、只增、开放位、对齐、五组、判据。
//!
//! 职责定位：冻结清单（K01 组对外 API 14 函数闭集：引擎注册 2+能力查询 2+
//! 任务提交 3+同步 3+遥测 4——清单数冻结，缺函数 0x8201）+三要素（每函数
//! {签名/语义/错误路径}三件齐备——缺一 0x8202）+兼容承诺（v1 后只增不改：
//! 已冻结函数签名篡改 0x8203，只增承诺十年）+扩展位（新引擎类型注册扩展
//! 位显性在册——F0406 枚举开放精神复用声明，扩展位缺席 0x8204）+跨域对齐
//! （与 B/J 域 API 调用签名一致——漂移 0x8205）。诊断码段 0x8201~0x8205。

use alloc::vec::Vec;

/// 模块版本。
pub const HET_API_VERSION: &str = "K14-hetapi-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x82 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x8201 冻结清单缺函数（14 函数闭集不全）。
pub const XC_API_LIST_SHORT: u32 = 0x8201;
/// 0x8202 三要素缺（签名/语义/错误路径缺一）。
pub const XC_TRIPLE_MISS: u32 = 0x8202;
/// 0x8203 冻结签名篡改（只增承诺违约）。
pub const XC_FROZEN_MUTATED: u32 = 0x8203;
/// 0x8204 扩展位缺席（新引擎注册扩展位未登记）。
pub const XC_EXT_SLOT_MISS: u32 = 0x8204;
/// 0x8205 跨域对齐漂移（与 B/J 域签名不一致）。
pub const XC_CROSS_DOMAIN_DRIFT: u32 = 0x8205;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 冻结函数数（14 函数闭集）。
pub const API_FN_COUNT: usize = 14;
/// 三要素数（签名/语义/错误路径）。
pub const TRIPLE_COUNT: usize = 3;
/// 冻结版本 v1。
pub const FROZEN_VER: u32 = 1;
/// 扩展位数（新引擎类型预留）。
pub const EXT_SLOTS: usize = 2;

// ---------------------------------------------------------------------------
// 一、冻结清单（14 函数闭集+三要素）
// ---------------------------------------------------------------------------

/// API 函数条目（三要素原子单位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApiFn {
    /// 函数序（0..=13）。
    pub no: usize,
    /// 所属组（0=引擎注册 1=能力查询 2=任务提交 3=同步 4=遥测）。
    pub group: usize,
    /// 签名锚（形状码——非零）。
    pub sig: u32,
    /// 语义锚（语义单号——非零）。
    pub semantic: u32,
    /// 错误路径锚（错误码域——非零）。
    pub err_path: u32,
}

impl ApiFn {
    /// 三要素齐备校验。
    pub const fn triple_complete(&self) -> bool {
        self.sig != 0 && self.semantic != 0 && self.err_path != 0
    }
}

/// 五组闭集容量（注册 2/查询 2/提交 3/同步 3/遥测 4）。
pub const GROUP_CAPS: [usize; 5] = [2, 2, 3, 3, 4];

/// 冻结注册表（判据"冻结清单（K01 组对外 API……14 函数签名——冻结 v1"）。
#[derive(Debug)]
pub struct FrozenRegistry {
    /// 函数表。
    pub fns: Vec<ApiFn>,
    /// 冻结版本。
    pub ver: u32,
    /// 清单缺事件数（0x8201）。
    pub short_events: u32,
    /// 要素缺事件数（0x8202）。
    pub triple_miss_events: u32,
    /// 篡改事件数（0x8203）。
    pub mutate_events: u32,
}

impl FrozenRegistry {
    /// 新表（v1 冻结态）。
    pub const fn new() -> FrozenRegistry {
        FrozenRegistry {
            fns: Vec::new(),
            ver: FROZEN_VER,
            short_events: 0,
            triple_miss_events: 0,
            mutate_events: 0,
        }
    }

    /// 登记函数（三要素缺一 0x8202 拒入冻结表）。
    pub fn enroll(&mut self, f: ApiFn) -> Result<(), u32> {
        if !f.triple_complete() {
            self.triple_miss_events += 1;
            return Err(XC_TRIPLE_MISS);
        }
        // 同序号重复拒。
        let mut i = 0;
        while i < self.fns.len() {
            if self.fns[i].no == f.no {
                self.triple_miss_events += 1;
                return Err(XC_TRIPLE_MISS);
            }
            i += 1;
        }
        self.fns.push(f);
        Ok(())
    }

    /// 冻结完备裁决：14 函数全在册+逐组容量对册（0x8201）。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.fns.len() != API_FN_COUNT {
            self.short_events += 1;
            return Err(XC_API_LIST_SHORT);
        }
        // 逐组容量对册。
        let mut g = 0;
        while g < 5 {
            let mut n = 0;
            let mut i = 0;
            while i < self.fns.len() {
                if self.fns[i].group == g {
                    n += 1;
                }
                i += 1;
            }
            if n != GROUP_CAPS[g] {
                self.short_events += 1;
                return Err(XC_API_LIST_SHORT);
            }
            g += 1;
        }
        Ok(())
    }

    /// 只增承诺校验：冻结后逐函数三要素重对拍（篡改 0x8203）。
    pub fn verify_frozen(&mut self, baseline: &[(usize, u32, u32, u32)]) -> Result<(), u32> {
        if baseline.len() != self.fns.len() {
            self.mutate_events += 1;
            return Err(XC_FROZEN_MUTATED);
        }
        let mut i = 0;
        while i < baseline.len() {
            let f = &self.fns[i];
            let b = &baseline[i];
            if f.no != b.0 || f.sig != b.1 || f.semantic != b.2 || f.err_path != b.3 {
                self.mutate_events += 1;
                return Err(XC_FROZEN_MUTATED);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、扩展位（新引擎类型注册——F0406 枚举开放精神）
// ---------------------------------------------------------------------------

/// 扩展位簿（判据"扩展位（新引擎类型注册扩展位（未来加速器"）。
#[derive(Debug)]
pub struct ExtSlotBook {
    /// 扩展位表（预留槽）。
    pub slots: [Option<u32>; EXT_SLOTS],
    /// F0406 精神复用声明在册。
    pub f0406_declared: bool,
    /// 缺席事件数（0x8204）。
    pub miss_events: u32,
}

impl ExtSlotBook {
    /// 新簿（全空槽+声明在册）。
    pub const fn new() -> ExtSlotBook {
        ExtSlotBook {
            slots: [None; EXT_SLOTS],
            f0406_declared: true,
            miss_events: 0,
        }
    }

    /// 占位（新引擎类型登记到扩展位）。
    pub fn occupy(&mut self, slot: usize, engine_tag: u32) -> Result<(), u32> {
        if slot >= EXT_SLOTS || engine_tag == 0 {
            self.miss_events += 1;
            return Err(XC_EXT_SLOT_MISS);
        }
        self.slots[slot] = Some(engine_tag);
        Ok(())
    }

    /// 完备裁决：声明在册+槽位数正确（0x8204）。
    pub fn sane(&self) -> Result<(), u32> {
        if !self.f0406_declared {
            return Err(XC_EXT_SLOT_MISS);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、跨域对齐（与 B/J 域 API 签名一致）
// ---------------------------------------------------------------------------

/// 跨域签名画像。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainShape {
    /// 域标识（0=B 域 1=J 域 2=K 域自身）。
    pub domain: usize,
    /// 任务提交签名形状码。
    pub submit_sig: u32,
    /// 遥测查询签名形状码。
    pub tel_sig: u32,
}

/// 跨域对齐器（判据"与 B/J 域 API 对齐（跨域调用签名一致"）。
#[derive(Debug)]
pub struct CrossDomainAlign {
    /// 域画像表。
    pub shapes: Vec<DomainShape>,
    /// 漂移事件数（0x8205）。
    pub drift_events: u32,
}

impl CrossDomainAlign {
    /// 新对齐器。
    pub const fn new() -> CrossDomainAlign {
        CrossDomainAlign {
            shapes: Vec::new(),
            drift_events: 0,
        }
    }

    /// 登记域画像。
    pub fn enroll(&mut self, s: DomainShape) {
        self.shapes.push(s);
    }

    /// 对齐裁决：全域 submit/tel 签名逐位一致（漂移 0x8205）。
    pub fn verify(&mut self, k_shape: &DomainShape) -> Result<(), u32> {
        let mut i = 0;
        while i < self.shapes.len() {
            let s = &self.shapes[i];
            if s.domain == k_shape.domain {
                i += 1;
                continue;
            }
            if s.submit_sig != k_shape.submit_sig || s.tel_sig != k_shape.tel_sig {
                self.drift_events += 1;
                return Err(XC_CROSS_DOMAIN_DRIFT);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、总引擎 + 五组自检
// ---------------------------------------------------------------------------

/// 异构 API 总引擎。
#[derive(Debug)]
pub struct HetApiEngine {
    /// 冻结注册表。
    pub reg: FrozenRegistry,
    /// 扩展位簿。
    pub ext: ExtSlotBook,
    /// 跨域对齐器。
    pub align: CrossDomainAlign,
}

impl HetApiEngine {
    /// 出厂。
    pub const fn new() -> HetApiEngine {
        HetApiEngine {
            reg: FrozenRegistry::new(),
            ext: ExtSlotBook::new(),
            align: CrossDomainAlign::new(),
        }
    }

    /// 构造标准 14 函数清单（组序：注册 2/查询 2/提交 3/同步 3/遥测 4）。
    pub fn std_manifest() -> Vec<ApiFn> {
        let mut out = Vec::new();
        let mut no = 0usize;
        let mut g = 0usize;
        while g < 5 {
            let mut k = 0;
            while k < GROUP_CAPS[g] {
                out.push(ApiFn {
                    no,
                    group: g,
                    sig: 0x5100 + no as u32,
                    semantic: 0x5200 + no as u32,
                    err_path: 0x5300 + no as u32,
                });
                k += 1;
                no += 1;
            }
            g += 1;
        }
        out
    }

    /// 组一：清单——14 函数齐+逐组容量对册；缺函数 0x8201。
    pub fn selfcheck_manifest(&mut self) -> Result<(), u32> {
        let mut e = HetApiEngine::new();
        // 空表冻结→0x8201。
        if e.reg.freeze() != Err(XC_API_LIST_SHORT) {
            return Err(XC_API_LIST_SHORT);
        }
        // 标准 14 函数入册。
        let m = Self::std_manifest();
        let mut i = 0;
        while i < m.len() {
            e.reg.enroll(m[i])?;
            i += 1;
        }
        e.reg.freeze()?;
        if e.reg.fns.len() != API_FN_COUNT {
            return Err(XC_API_LIST_SHORT);
        }
        Ok(())
    }

    /// 组二：三要素——缺要素 0x8202 拒；重复序号拒。
    pub fn selfcheck_triple(&mut self) -> Result<(), u32> {
        let mut e = HetApiEngine::new();
        // 缺错误路径→0x8202。
        if e.reg.enroll(ApiFn { no: 0, group: 0, sig: 1, semantic: 2, err_path: 0 })
            != Err(XC_TRIPLE_MISS)
        {
            return Err(XC_TRIPLE_MISS);
        }
        // 合法入册后重复序号→0x8202。
        e.reg.enroll(ApiFn { no: 0, group: 0, sig: 1, semantic: 2, err_path: 3 })?;
        if e.reg.enroll(ApiFn { no: 0, group: 0, sig: 9, semantic: 9, err_path: 9 })
            != Err(XC_TRIPLE_MISS)
        {
            return Err(XC_TRIPLE_MISS);
        }
        if e.reg.triple_miss_events != 2 {
            return Err(XC_TRIPLE_MISS);
        }
        Ok(())
    }

    /// 组三：只增承诺——冻结后篡改 0x8203；未篡改过。
    pub fn selfcheck_immutable(&mut self) -> Result<(), u32> {
        let mut e = HetApiEngine::new();
        let m = Self::std_manifest();
        let mut i = 0;
        while i < m.len() {
            e.reg.enroll(m[i])?;
            i += 1;
        }
        // 基线快照。
        let mut baseline = Vec::new();
        let mut j = 0;
        while j < e.reg.fns.len() {
            let f = &e.reg.fns[j];
            baseline.push((f.no, f.sig, f.semantic, f.err_path));
            j += 1;
        }
        e.reg.verify_frozen(&baseline)?;
        // 篡改一个签名→0x8203。
        e.reg.fns[3].sig ^= 1;
        if e.reg.verify_frozen(&baseline) != Err(XC_FROZEN_MUTATED) {
            return Err(XC_FROZEN_MUTATED);
        }
        if e.reg.mutate_events != 1 {
            return Err(XC_FROZEN_MUTATED);
        }
        // 版本锚仍 v1（只增承诺下冻结版本不变，扩展走新版本窗）。
        if e.reg.ver != FROZEN_VER {
            return Err(XC_FROZEN_MUTATED);
        }
        Ok(())
    }

    /// 组四：扩展位——占位合法；越界/空标 0x8204；声明在册。
    pub fn selfcheck_ext(&mut self) -> Result<(), u32> {
        let mut e = HetApiEngine::new();
        e.ext.sane()?;
        // 越界槽→0x8204。
        if e.ext.occupy(9, 0x99) != Err(XC_EXT_SLOT_MISS) {
            return Err(XC_EXT_SLOT_MISS);
        }
        // 空标→0x8204。
        if e.ext.occupy(0, 0) != Err(XC_EXT_SLOT_MISS) {
            return Err(XC_EXT_SLOT_MISS);
        }
        // 合法占位。
        e.ext.occupy(0, 0xF00D)?;
        e.ext.occupy(1, 0xBEAD)?;
        if e.ext.miss_events != 2 {
            return Err(XC_EXT_SLOT_MISS);
        }
        Ok(())
    }

    /// 组五：跨域对齐+判据——B/J 域同形状过；漂移 0x8205；端到端全绿。
    pub fn selfcheck_align(&mut self) -> Result<(), u32> {
        let mut e = HetApiEngine::new();
        let k = DomainShape { domain: 2, submit_sig: 0x5501, tel_sig: 0x5502 };
        // B/J 域同形状→对齐过。
        e.align.enroll(DomainShape { domain: 0, submit_sig: 0x5501, tel_sig: 0x5502 });
        e.align.enroll(DomainShape { domain: 1, submit_sig: 0x5501, tel_sig: 0x5502 });
        e.align.verify(&k)?;
        // J 域漂移→0x8205。
        let mut e2 = HetApiEngine::new();
        e2.align.enroll(DomainShape { domain: 0, submit_sig: 0x5501, tel_sig: 0x5502 });
        e2.align.enroll(DomainShape { domain: 1, submit_sig: 0x5509, tel_sig: 0x5502 });
        if e2.align.verify(&k) != Err(XC_CROSS_DOMAIN_DRIFT) {
            return Err(XC_CROSS_DOMAIN_DRIFT);
        }
        if e2.align.drift_events != 1 {
            return Err(XC_CROSS_DOMAIN_DRIFT);
        }
        // 端到端：清单+三要素+只增+扩展位+对齐全链绿。
        let mut full = HetApiEngine::new();
        let m = Self::std_manifest();
        let mut i = 0;
        while i < m.len() {
            full.reg.enroll(m[i])?;
            i += 1;
        }
        full.reg.freeze()?;
        full.ext.sane()?;
        full.align.enroll(DomainShape { domain: 0, submit_sig: 0x5501, tel_sig: 0x5502 });
        full.align.enroll(DomainShape { domain: 1, submit_sig: 0x5501, tel_sig: 0x5502 });
        full.align.verify(&k)?;
        if full.reg.short_events != 0
            || full.reg.triple_miss_events != 0
            || full.reg.mutate_events != 0
            || full.ext.miss_events != 0
            || full.align.drift_events != 0
        {
            return Err(XC_API_LIST_SHORT);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_manifest()?;
        self.selfcheck_triple()?;
        self.selfcheck_immutable()?;
        self.selfcheck_ext()?;
        self.selfcheck_align()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8200..0x8300 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_API_LIST_SHORT,
            XC_TRIPLE_MISS,
            XC_FROZEN_MUTATED,
            XC_EXT_SLOT_MISS,
            XC_CROSS_DOMAIN_DRIFT,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8200 || codes[i] >= 0x8300 {
                return Err(XC_API_LIST_SHORT);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_API_LIST_SHORT);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
