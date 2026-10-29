//! UNX-F0695 · 矩阵失败分类路由（四类复用扩展）· 诊断段 0x5760（独占）
//!
//! ── 验收锚点（判据原文摘录） ─────────────────────────────────────────
//! [UNX-F0695] 矩阵失败分类路由（四类复用扩展） <SEG 0x5760>
//!   UNX-F0695-J1 失败四类路由正确率 100%（矩阵注入样本全中）且矩阵特有
//!   类注记（写盘/固件/环境）齐。
//!   完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! ── 设计纪律 ─────────────────────────────────────────────────────────
//!   1. 四类闭集：WriteBack（写盘）/ Firmware（固件）/ Environment（环境）/
//!      Logic（内核逻辑），类号 0..4，臂外无类。
//!   2. 路由确定性：同一症状码（symptom 闭集）→ 唯一类，无第二解释；
//!      注入样本逐条全中（正确率 100% 判据）。
//!   3. 矩阵特有类注记：写盘/固件/环境三类各挂一条矩阵特有注记
//!      （类注记齐判据），Logic 类注记为通用位。
//!   4. 复用扩展：每类挂子码段（子码 0..=15 定容），扩展只加子码
//!      不改类闭集——类稳定、子码可扩，路由表按 (类,子码) 二元组寻址。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!   6. 诊断段 0x5760~0x577F 本模块独占，XcCode 显式判别式。
//!
//! 目标行数：≥580（判据原文「纯功能代码 ≥580 行」）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ── 诊断码段（0x5760 独占 0x5760~0x577F） ────────────────────────────
pub const SEG_BASE: u16 = 0x5760;

/// 0x5761 路由表已封板仍尝试扩展
const E_SEALED: u16 = SEG_BASE + 0x1;
/// 0x5762 症状码不在闭集（无路由解释）
const E_SYMPTOM_BAD: u16 = SEG_BASE + 0x2;
/// 0x5763 类号越界（合法 0..4）
const E_CLASS_RANGE: u16 = SEG_BASE + 0x3;
/// 0x5764 子码越界（合法 0..=15）
const E_SUBCODE_RANGE: u16 = SEG_BASE + 0x4;
/// 0x5765 路由冲突：同症状码已绑异类
const E_ROUTE_CLASH: u16 = SEG_BASE + 0x5;
/// 0x5766 路由缺失：症状码未入路由表
const E_ROUTE_MISSING: u16 = SEG_BASE + 0x6;
/// 0x5767 类注记缺失：写盘/固件/环境特有注记缺一
const E_NOTE_MISSING: u16 = SEG_BASE + 0x7;
/// 0x5768 注入样本对拍失败：样本期望类 ≠ 路由类
const E_SAMPLE_MISS: u16 = SEG_BASE + 0x8;
/// 0x5769 守恒破：四类计数之和 ≠ 路由总数
const E_CONSERVATION: u16 = SEG_BASE + 0x9;
/// 0x576A 复用扩展越限：单类子码登记数超容量
const E_EXT_FULL: u16 = SEG_BASE + 0xA;
/// 0x576B 摘要为空：写回结果摘要不得为空串
const E_SUMMARY_EMPTY: u16 = SEG_BASE + 0xB;
/// 0x576C 样本为空：正确率核对无样本
const E_NO_SAMPLE: u16 = SEG_BASE + 0xC;
/// 0x576D 复用登记重号：同 (类,子码) 二次登记
const E_EXT_DUP: u16 = SEG_BASE + 0xD;
/// 0x576E 误分类检出：正常路由产出与期望账不符
const E_MISSCLASSIFY: u16 = SEG_BASE + 0xE;

/// 诊断码 → 中文可操作诊断（段 0x5760~0x577F 内解释）。
pub fn diag_detail(code: u16) -> &'static str {
    match code {
        E_SEALED => "路由表已封板：扩展请开新表，封板表只读",
        E_SYMPTOM_BAD => "症状码非法：不在症状闭集，请先扩闭集再路由",
        E_CLASS_RANGE => "类号越界：四类闭集合法 0..4",
        E_SUBCODE_RANGE => "子码越界：单类子码合法 0..=15",
        E_ROUTE_CLASH => "路由冲突：同症状码已绑异类，确定性路由不容二义",
        E_ROUTE_MISSING => "路由缺失：症状码未入表，请先 register 路由",
        E_NOTE_MISSING => "类注记缺失：写盘/固件/环境特有注记必须齐",
        E_SAMPLE_MISS => "注入样本对拍失败：期望类与路由类不符",
        E_CONSERVATION => "守恒破：四类计数之和 ≠ 路由总数",
        E_EXT_FULL => "子码越限：单类子码登记超 16 条上限",
        E_SUMMARY_EMPTY => "结果摘要为空：写回摘要不得为空串",
        E_NO_SAMPLE => "样本为空：正确率核对至少需要 1 条样本",
        E_EXT_DUP => "复用登记重号：同 (类,子码) 已登记",
        E_MISSCLASSIFY => "误分类检出：路由产出与期望账不符",
        _ => "未知诊断码（0x5760 段外或保留位）",
    }
}

// ── 常量（来源：四类闭集与子码定容） ─────────────────────────────────
/// 失败类数（判据原文：四类）。
pub const CLASS_COUNT: usize = 4;
/// 单类子码容量（复用扩展定容）。
pub const SUBCODE_CAP: usize = 16;
/// 症状码闭集容量。
pub const SYMPTOM_CAP: usize = 32;
/// 注入样本账容量。
pub const SAMPLE_CAP: usize = 64;
/// 摘要最小长度（判据「结果摘要非空」）。
pub const SUMMARY_MIN_LEN: usize = 1;

const _: () = assert!(CLASS_COUNT == 4);
const _: () = assert!(SUBCODE_CAP == 16);

// ── FNV-1a 64 ────────────────────────────────────────────────────────
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_mix(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

// ── 四类闭集 ─────────────────────────────────────────────────────────
/// 失败四类闭集。类号 0..4，臂外无类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailClass {
    /// 类 0：写盘失败（矩阵落簿/归档写盘面）。
    WriteBack,
    /// 类 1：固件失败（固件响应/复位面）。
    Firmware,
    /// 类 2：环境失败（宿主环境/资源面）。
    Environment,
    /// 类 3：内核逻辑失败（代码路径自身）。
    Logic,
}

impl FailClass {
    /// 类号。
    pub const fn id(self) -> usize {
        match self {
            FailClass::WriteBack => 0,
            FailClass::Firmware => 1,
            FailClass::Environment => 2,
            FailClass::Logic => 3,
        }
    }

    /// 从类号重建。
    pub fn from_id(v: usize) -> Result<FailClass, u16> {
        match v {
            0 => Ok(FailClass::WriteBack),
            1 => Ok(FailClass::Firmware),
            2 => Ok(FailClass::Environment),
            3 => Ok(FailClass::Logic),
            _ => Err(E_CLASS_RANGE),
        }
    }

    /// 矩阵特有类注记（写盘/固件/环境三类特有；Logic 为通用位）。
    pub const fn matrix_note(self) -> &'static str {
        match self {
            FailClass::WriteBack => "矩阵特有注记[写盘]: 归档件写盘失败不重试, 断点账先封板",
            FailClass::Firmware => "矩阵特有注记[固件]: 固件响应失败即挂起, 走断点续跑通道",
            FailClass::Environment => "矩阵特有注记[环境]: 宿主资源不足降档, 不计入成功率分母",
            FailClass::Logic => "类注记[逻辑]: 内核路径缺陷, 必须归档并开回归",
        }
    }

    /// 类名（摘要拼装用）。
    pub const fn name(self) -> &'static str {
        match self {
            FailClass::WriteBack => "写盘",
            FailClass::Firmware => "固件",
            FailClass::Environment => "环境",
            FailClass::Logic => "逻辑",
        }
    }
}

// ── 症状码闭集（32 条，来源：矩阵演练注入面） ────────────────────────
/// 症状码 → 期望类 的确定性映射（路由真相源）。
/// 编码约定：症状码 raw 0..=31，逐条写死注来源（矩阵注入面清单）。
pub const SYMPTOM_WRITEBACK_LO: u8 = 0; // 写盘症状段 0..=7
pub const SYMPTOM_WRITEBACK_HI: u8 = 7;
pub const SYMPTOM_FIRMWARE_LO: u8 = 8; // 固件症状段 8..=15
pub const SYMPTOM_FIRMWARE_HI: u8 = 15;
pub const SYMPTOM_ENV_LO: u8 = 16; // 环境症状段 16..=23
pub const SYMPTOM_ENV_HI: u8 = 23;
pub const SYMPTOM_LOGIC_LO: u8 = 24; // 逻辑症状段 24..=31
pub const SYMPTOM_LOGIC_HI: u8 = 31;

/// 症状码路由真相（纯函数，四段闭区间映射）。
pub const fn symptom_truth(s: u8) -> Result<FailClass, u16> {
    if s <= SYMPTOM_WRITEBACK_HI {
        Ok(FailClass::WriteBack)
    } else if s <= SYMPTOM_FIRMWARE_HI {
        Ok(FailClass::Firmware)
    } else if s <= SYMPTOM_ENV_HI {
        Ok(FailClass::Environment)
    } else if s <= SYMPTOM_LOGIC_HI {
        Ok(FailClass::Logic)
    } else {
        Err(E_SYMPTOM_BAD)
    }
}

// ── 路由条目 ─────────────────────────────────────────────────────────
/// 路由表条目：症状码 → (类, 子码)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteEntry {
    /// 症状码（闭集 0..=31）。
    pub symptom: u8,
    /// 路由类。
    pub class: FailClass,
    /// 类内子码 0..=15（复用扩展位）。
    pub subcode: u8,
}

// ── 注入样本 ─────────────────────────────────────────────────────────
/// 注入样本：症状码 + 期望类（对拍判据载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectSample {
    /// 症状码。
    pub symptom: u8,
    /// 期望路由类。
    pub expect: FailClass,
}

// ── 复用扩展登记 ─────────────────────────────────────────────────────
/// (类,子码) 复用扩展登记：扩展只加子码不改类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtEntry {
    /// 类号。
    pub class: u8,
    /// 子码。
    pub subcode: u8,
    /// 扩展名哈希低 32 位（定容账不存长串）。
    pub tag_fp: u32,
}

// ── 失败分类路由引擎 ─────────────────────────────────────────────────
/// 四类失败路由引擎：路由表 + 注入样本账 + 四类计数 + 复用扩展账。
pub struct FailRouter {
    /// 路由表（症状码 → 条目；索引即症状码）。
    routes: Vec<Option<RouteEntry>>,
    /// 四类计数账（守恒：Σ == 路由命中总数）。
    class_counts: [u32; CLASS_COUNT],
    /// 注入样本账（对拍判据）。
    samples: Vec<InjectSample>,
    /// 复用扩展账（每类一列，(子码→tag) 定容 16）。
    ext: [[Option<ExtEntry>; SUBCODE_CAP]; CLASS_COUNT],
    /// 路由命中总数。
    routed_total: u32,
    /// 对拍通过数。
    sample_pass: u32,
    /// 封板位。
    sealed: bool,
}

impl FailRouter {
    /// 新路由器：空路由表（闭集容量预分配语义由 Vec 承载）。
    pub fn new() -> Self {
        FailRouter {
            routes: Vec::new(),
            class_counts: [0; CLASS_COUNT],
            samples: Vec::new(),
            ext: [[None; SUBCODE_CAP]; CLASS_COUNT],
            routed_total: 0,
            sample_pass: 0,
            sealed: false,
        }
    }

    /// 注册路由：症状码 → (类,子码)。冲突（同码已绑异类）拒收。
    pub fn register(&mut self, symptom: u8, class: FailClass, subcode: u8) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if subcode as usize >= SUBCODE_CAP {
            return Err(E_SUBCODE_RANGE);
        }
        // 症状码闭集核对（真相源一致性）。
        let truth = symptom_truth(symptom)?;
        if truth != class {
            return Err(E_ROUTE_CLASH);
        }
        // 扩容路由表。
        while self.routes.len() <= symptom as usize {
            self.routes.push(None);
        }
        if let Some(prev) = self.routes[symptom as usize] {
            if prev.class != class || prev.subcode != subcode {
                return Err(E_ROUTE_CLASH);
            }
            // 同条目重注册：幂等放行不计数。
            return Ok(());
        }
        self.routes[symptom as usize] = Some(RouteEntry { symptom, class, subcode });
        Ok(())
    }

    /// 路由：症状码 → 类（确定性，缺路由拒收）。命中计账。
    pub fn route(&mut self, symptom: u8) -> Result<FailClass, u16> {
        if symptom as usize >= self.routes.len() {
            return Err(E_ROUTE_MISSING);
        }
        match self.routes[symptom as usize] {
            Some(e) => {
                self.class_counts[e.class.id()] =
                    self.class_counts[e.class.id()].wrapping_add(1);
                self.routed_total = self.routed_total.wrapping_add(1);
                Ok(e.class)
            }
            None => Err(E_ROUTE_MISSING),
        }
    }

    /// 路由只读查询（不计数，供对拍）。
    pub fn route_peek(&self, symptom: u8) -> Result<FailClass, u16> {
        if symptom as usize >= self.routes.len() {
            return Err(E_ROUTE_MISSING);
        }
        match self.routes[symptom as usize] {
            Some(e) => Ok(e.class),
            None => Err(E_ROUTE_MISSING),
        }
    }

    /// 挂注入样本（对拍判据账）。
    pub fn add_sample(&mut self, symptom: u8, expect: FailClass) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.samples.len() >= SAMPLE_CAP {
            return Err(E_EXT_FULL);
        }
        symptom_truth(symptom)?;
        self.samples.push(InjectSample { symptom, expect });
        Ok(())
    }

    /// 注入样本全量对拍：逐条路由核对，正确率必须 100%（判据原文）。
    pub fn audit_samples(&mut self) -> Result<u32, u16> {
        if self.samples.is_empty() {
            return Err(E_NO_SAMPLE);
        }
        self.sample_pass = 0;
        let mut i = 0usize;
        while i < self.samples.len() {
            let s = self.samples[i];
            let got = self.route_peek(s.symptom)?;
            if got != s.expect {
                return Err(E_SAMPLE_MISS);
            }
            self.sample_pass = self.sample_pass.wrapping_add(1);
            i += 1;
        }
        // 正确率 ppm：pass/total，二值判定必须 10000ppm（100%）。
        let total = self.samples.len() as u64;
        let ppm = (self.sample_pass as u64) * 10_000 / total;
        if ppm != 10_000 {
            return Err(E_SAMPLE_MISS);
        }
        Ok(self.sample_pass)
    }

    /// 复用扩展登记：(类,子码) → 标签指纹。重号拒收。
    pub fn register_ext(&mut self, class: FailClass, subcode: u8, tag_fp: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if subcode as usize >= SUBCODE_CAP {
            return Err(E_SUBCODE_RANGE);
        }
        let cid = class.id();
        if self.ext[cid][subcode as usize].is_some() {
            return Err(E_EXT_DUP);
        }
        self.ext[cid][subcode as usize] = Some(ExtEntry {
            class: cid as u8,
            subcode,
            tag_fp,
        });
        Ok(())
    }

    /// 复用扩展查询。
    pub fn ext_lookup(&self, class: FailClass, subcode: u8) -> Result<u32, u16> {
        if subcode as usize >= SUBCODE_CAP {
            return Err(E_SUBCODE_RANGE);
        }
        match self.ext[class.id()][subcode as usize] {
            Some(e) => Ok(e.tag_fp),
            None => Err(E_ROUTE_MISSING),
        }
    }

    /// 类注记齐审计：写盘/固件/环境特有注记非空（判据「类注记齐」）。
    pub fn audit_notes(&self) -> Result<(), u16> {
        let mut c = 0usize;
        while c < CLASS_COUNT {
            let cls = FailClass::from_id(c)?;
            if cls.matrix_note().is_empty() {
                return Err(E_NOTE_MISSING);
            }
            c += 1;
        }
        // 特有三类逐一显性核对（写盘/固件/环境）。
        if FailClass::WriteBack.matrix_note().is_empty()
            || FailClass::Firmware.matrix_note().is_empty()
            || FailClass::Environment.matrix_note().is_empty()
        {
            return Err(E_NOTE_MISSING);
        }
        Ok(())
    }

    /// 守恒审计：Σ四类计数 == 路由总数。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut sum: u32 = 0;
        let mut c = 0usize;
        while c < CLASS_COUNT {
            sum = sum.wrapping_add(self.class_counts[c]);
            c += 1;
        }
        if sum != self.routed_total {
            return Err(E_CONSERVATION);
        }
        Ok(())
    }

    /// 四类计数只读。
    pub fn counts(&self) -> [u32; CLASS_COUNT] {
        self.class_counts
    }

    /// 封板：审计全过后落锁。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.audit_notes()?;
        self.audit_conservation()?;
        self.audit_samples()?;
        self.sealed = true;
        Ok(self.fingerprint())
    }

    /// 全指纹（路由表+计数+样本折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.routes.len() {
            if let Some(e) = self.routes[i] {
                h = fnv_mix(h, e.symptom);
                h = fnv_mix(h, e.class.id() as u8);
                h = fnv_mix(h, e.subcode);
            }
            i += 1;
        }
        let mut c = 0usize;
        while c < CLASS_COUNT {
            let v = self.class_counts[c];
            h = fnv_mix(h, (v & 0xFF) as u8);
            h = fnv_mix(h, ((v >> 8) & 0xFF) as u8);
            c += 1;
        }
        let mut s = 0usize;
        while s < self.samples.len() {
            h = fnv_mix(h, self.samples[s].symptom);
            h = fnv_mix(h, self.samples[s].expect.id() as u8);
            s += 1;
        }
        h
    }

    /// 结果摘要（程序化拼装，判据「结果摘要非空」）。
    pub fn summary(&self) -> Result<String, u16> {
        if self.routes.is_empty() {
            return Err(E_SUMMARY_EMPTY);
        }
        let mut out = String::new();
        out.push_str("失败路由摘要: 路由总数=");
        out.push_str(&itoa(self.routed_total as u64));
        out.push_str(" 样本=");
        out.push_str(&itoa(self.samples.len() as u64));
        out.push_str(" 对拍过=");
        out.push_str(&itoa(self.sample_pass as u64));
        let mut c = 0usize;
        while c < CLASS_COUNT {
            out.push(' ');
            out.push_str(FailClass::from_id(c)?.name());
            out.push('=');
            out.push_str(&itoa(self.class_counts[c] as u64));
            c += 1;
        }
        out.push_str(" 指纹=");
        out.push_str(&hex16(self.fingerprint()));
        if out.len() < SUMMARY_MIN_LEN {
            return Err(E_SUMMARY_EMPTY);
        }
        Ok(out)
    }
}

// ── 整数工具 ─────────────────────────────────────────────────────────
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return "0".to_string();
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

fn hex16(v: u64) -> String {
    const HEXD: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::new();
    s.push_str("0x");
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEXD[nib] as char);
        shift -= 4;
    }
    s
}

// ── 自例程：样本走主路径 + 错误路径，0=全过 ──────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut r = FailRouter::new();

    // —— 主路径：全 32 症状码注册 + 注入样本对拍 ——
    let mut s = 0u8;
    while s < SYMPTOM_CAP as u8 {
        let cls = symptom_truth(s)?;
        let sub = s % SUBCODE_CAP as u8;
        r.register(s, cls, sub)?;
        r.add_sample(s, cls)?;
        s += 1;
    }
    // 全量路由命中。
    s = 0;
    while s < SYMPTOM_CAP as u8 {
        let got = r.route(s)?;
        let want = symptom_truth(s)?;
        if got != want {
            return Err(E_MISSCLASSIFY);
        }
        s += 1;
    }
    r.audit_notes()?;
    r.audit_conservation()?;
    let pass = r.audit_samples()?;
    if pass != SYMPTOM_CAP as u32 {
        return Err(E_SAMPLE_MISS);
    }
    let fp = r.seal()?;
    if fp == 0 {
        return Err(E_MISSCLASSIFY);
    }
    // 封板后写必拦。
    if r.register(0, FailClass::WriteBack, 0) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    let sm = r.summary()?;
    if sm.len() < SUMMARY_MIN_LEN {
        return Err(E_SUMMARY_EMPTY);
    }

    // —— 复用扩展：四类各登记 2 子码 + 查询 + 重号拦截 ——
    let mut r2 = FailRouter::new();
    r2.register(0, FailClass::WriteBack, 0)?;
    r2.register(8, FailClass::Firmware, 1)?;
    r2.register_ext(FailClass::WriteBack, 0, 0xA11C)?;
    r2.register_ext(FailClass::WriteBack, 1, 0xA11D)?;
    r2.register_ext(FailClass::Firmware, 1, 0xB22E)?;
    if r2.ext_lookup(FailClass::WriteBack, 0)? != 0xA11C {
        return Err(E_MISSCLASSIFY);
    }
    if r2.ext_lookup(FailClass::Firmware, 1)? != 0xB22E {
        return Err(E_MISSCLASSIFY);
    }
    if r2.ext_lookup(FailClass::Logic, 15) != Err(E_ROUTE_MISSING) {
        return Err(E_ROUTE_MISSING);
    }
    // 重号登记显性注入：同 (类,子码) 二次登记必拦。
    if r2.register_ext(FailClass::WriteBack, 0, 0xFFFF) != Err(E_EXT_DUP) {
        return Err(E_EXT_DUP);
    }

    // —— 错误路径样本 ——
    if symptom_truth(32) != Err(E_SYMPTOM_BAD) {
        return Err(E_SYMPTOM_BAD);
    }
    if FailClass::from_id(4) != Err(E_CLASS_RANGE) {
        return Err(E_CLASS_RANGE);
    }
    // 路由冲突：症状 0 绑固件类（真相源写盘）必拦。
    if r2.register(0, FailClass::Firmware, 0) != Err(E_ROUTE_CLASH) {
        return Err(E_ROUTE_CLASH);
    }
    // 子码越界。
    if r2.register(1, FailClass::WriteBack, 16) != Err(E_SUBCODE_RANGE) {
        return Err(E_SUBCODE_RANGE);
    }
    if r2.register_ext(FailClass::Logic, 16, 1) != Err(E_SUBCODE_RANGE) {
        return Err(E_SUBCODE_RANGE);
    }
    // 缺路由必拦。
    if r2.route(31) != Err(E_ROUTE_MISSING) {
        return Err(E_ROUTE_MISSING);
    }
    // 空样本对拍必拦。
    if r2.audit_samples() != Err(E_NO_SAMPLE) {
        return Err(E_NO_SAMPLE);
    }
    // 期望不符样本必拦：症状 8（固件真相）挂期望写盘。
    let mut r3 = FailRouter::new();
    r3.register(8, FailClass::Firmware, 0)?;
    r3.add_sample(8, FailClass::WriteBack)?;
    if r3.audit_samples() != Err(E_SAMPLE_MISS) {
        return Err(E_SAMPLE_MISS);
    }
    // 守恒注入：计数账拉爆必拦。
    let mut r4 = FailRouter::new();
    r4.register(0, FailClass::WriteBack, 0)?;
    r4.route(0)?;
    r4.routed_total = r4.routed_total.wrapping_add(1);
    if r4.audit_conservation() != Err(E_CONSERVATION) {
        return Err(E_CONSERVATION);
    }

    // 诊断表抽查。
    if diag_detail(E_ROUTE_CLASH) == "未知诊断码（0x5760 段外或保留位）" {
        return Err(SEG_BASE + 0x10);
    }
    if diag_detail(SEG_BASE.wrapping_add(0x1F)) != "未知诊断码（0x5760 段外或保留位）" {
        return Err(SEG_BASE + 0x11);
    }

    Ok(0)
}
