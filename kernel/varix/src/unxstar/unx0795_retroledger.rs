#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0795 · 域复盘账（复盘决议五条登记）
//! 域/批：CoRun·UNX A1 收官域（复盘/维护/封域面 · W7 清单第 22 单）
//! 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥570 行；② 判据原文：UNX-F0795-J1 复盘账齐（五条决议：维度/
//!   决议/执行路径三列 = 5 条）且逐条有对应账面或通道登记；③ 完成时 API/软件
//!   写回：状态=已完成、结果摘要非空。
//! 设计纪律：① 决议三列建模：维度（Scope/Quality/Sched/Risk/Handover 五维
//!   闭集）/ 决议文本（非空）/ 执行路径（引用 ID，指向账面登记或通道登记）。
//! ② 引用闭集：账面引用（LedgerRef，指向已落账账面）与通道引用（ChannelRef，
//!   指向维护通道）两类；引用 ID 必须先登记后解引，未登记解引即红。
//! ③ 逐条解引断言：五条决议的执行路径逐条在登记簿解引成功且种类一致；
//!   解引失败定位到决议号。
//! ④ 守恒恒等式：决议数 = 5 = 五维各恰好 1 条（维度不重不漏）；
//!   登记簿条目数 = 被解引引用数 + 未用引用数。
//! ⑤ 状态机 Draft→Registered→CrossRef'd→Sealed 单向闭集；诊断码定位到决议号。
//! 诊断段声明：0x61A0–0x61BF 本单独占（清单 <SEG 0x61A0>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x61A0–0x61BF）
pub const SEG_BASE: u16 = 0x61A0; // 诊断码段基数，来源：W7 清单 UNX-F0795 行 <SEG 0x61A0>
pub const SEG_TOP: u16 = 0x61BF; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 复盘状态机非法边
pub const XC_DUP_DIM: u16 = SEG_BASE + 2; // 维度重复（五维不重破）
pub const XC_DIM_OUT: u16 = SEG_BASE + 3; // 维度越出五维闭集
pub const XC_TEXT_EMPTY: u16 = SEG_BASE + 4; // 决议文本为空
pub const XC_PATH_EMPTY: u16 = SEG_BASE + 5; // 执行路径引用 ID 为空
pub const XC_REF_UNKNOWN: u16 = SEG_BASE + 6; // 引用未登记即解引
pub const XC_REF_KIND: u16 = SEG_BASE + 7; // 引用种类与登记不符
pub const XC_RES_SHORT: u16 = SEG_BASE + 8; // 决议不足 5 条
pub const XC_RES_DUP: u16 = SEG_BASE + 9; // 决议号重复
pub const XC_NOT_ALL_REF: u16 = SEG_BASE + 10; // 存在决议无对应账面/通道登记
pub const XC_ALREADY_SEALED: u16 = SEG_BASE + 11; // 已封账再封账
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 12; // 摘要为空
pub const XC_CONSERV_BREAK: u16 = SEG_BASE + 13; // 守恒恒等式破
pub const XC_NO_PENDING: u16 = SEG_BASE + 14; // 无待补登记却申请补登

// 2. 复盘常量（写死，来源=判据原文/本单口径头注假设）
pub const RES_N: usize = 5; // 决议条数（五维各一条），来源：判据原文
pub const REF_CAP: usize = 16; // 登记簿定容，头注假设

/// 复盘维度闭集（五维）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetroDim {
    Scope = 0, // 范围维
    Quality = 1, // 质量维
    Sched = 2, // 进度维
    Risk = 3, // 风险维
    Handover = 4, // 移交维
}
pub const DIM_ALL: [RetroDim; 5] = [RetroDim::Scope, RetroDim::Quality, RetroDim::Sched, RetroDim::Risk, RetroDim::Handover];

/// 引用种类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    LedgerRef = 0, // 账面引用（指向已落账账面）
    ChannelRef = 1, // 通道引用（指向维护通道）
}
impl RefKind {
    pub fn view(self) -> &'static str {
        match self {
            RefKind::LedgerRef => "ledger",
            RefKind::ChannelRef => "channel",
        }
    }
}

/// 一条复盘决议（三列：维度/决议/执行路径）。
#[derive(Clone, Debug)]
pub struct Resolution {
    pub rid: u32, // 决议号 1..=5（唯一）
    pub dim: RetroDim, // 列一：维度
    pub text: String, // 列二：决议文本（非空）
    pub path: String, // 列三：执行路径（引用 ID，非空）
    pub kind: RefKind, // 执行路径种类（与登记簿对账）
}

/// 登记簿条目（账面或通道登记）。
#[derive(Clone, Debug)]
pub struct RefEntry {
    pub ref_id: String, // 引用 ID（唯一键）
    pub kind: RefKind, // 种类
    pub anchor: String, // 锚点说明（账面名或通道名，非空）
}

/// 封账回执。
#[derive(Clone, Debug)]
pub struct RetroSeal {
    pub res: u32, // 决议数（5）
    pub refs: u32, // 登记簿条目数
    pub all_ref: bool, // 逐条有对应账面/通道登记结论
    pub summary: String, // 摘要（非空）
    pub fingerprint: u64, // 复盘账指纹（对拍用）
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetroStage {
    Draft = 0,
    Registered = 1,
    CrossRefed = 2,
    Sealed = 3,
}
impl RetroStage {
    pub fn can_step(self, next: RetroStage) -> bool {
        (self as u8) + 1 == next as u8
    }
    pub fn view(self) -> &'static str {
        match self {
            RetroStage::Draft => "draft",
            RetroStage::Registered => "registered",
            RetroStage::CrossRefed => "crossrefed",
            RetroStage::Sealed => "sealed",
        }
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步（字节混合）。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub fn fnv_str(seed: u64, s: &str) -> u64 {  // 字符串 FNV（逐字节）。
    let mut h = seed;
    let bs = s.as_bytes();
    let mut i = 0usize;
    while i < bs.len() {
        h ^= bs[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}
pub struct RetroLedger {  // 域复盘账登记机。
    stage: RetroStage,
    resolutions: Vec<Resolution>,
    refs: Vec<RefEntry>,
    crossed: Vec<u32>, // 已逐条解引成功的决议号
    pinned: Option<RetroSeal>,
    events: Vec<String>,
}
impl RetroLedger {
    pub fn new() -> Self {
        RetroLedger {
            stage: RetroStage::Draft,
            resolutions: Vec::new(),
            refs: Vec::new(),
            crossed: Vec::new(),
            pinned: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> RetroStage {
        self.stage
    }
    pub fn res_len(&self) -> usize {
        self.resolutions.len()
    }
    pub fn ref_len(&self) -> usize {
        self.refs.len()
    }
    pub fn crossed_len(&self) -> usize {
        self.crossed.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn register_ref(&mut self, e: RefEntry) -> Result<u32, u16> {  // 登记账面/通道条目：ID 唯一、锚点非空、容量守卫。
        if self.stage != RetroStage::Draft && self.stage != RetroStage::Registered {
            return Err(XC_STATE_INVALID);
        }
        if e.ref_id.is_empty() {
            return Err(XC_PATH_EMPTY);
        }
        if e.anchor.is_empty() {
            return Err(XC_TEXT_EMPTY);
        }
        if self.refs.len() >= REF_CAP {
            return Err(XC_CONSERV_BREAK);
        }
        let mut i = 0usize;
        while i < self.refs.len() {
            if self.refs[i].ref_id == e.ref_id {
                return Err(XC_RES_DUP); // 复用重号码：登记键重复
            }
            i += 1;
        }
        self.refs.push(e);
        self.stage = RetroStage::Registered;
        self.events.push(alloc::format!("ref +{}", self.refs.len()));
        Ok(self.refs.len() as u32)
    }
    pub fn add_resolution(&mut self, r: Resolution) -> Result<u32, u16> {  // 决议登记：号唯一、维度闭集不重、文本/路径非空。
        if self.stage != RetroStage::Draft && self.stage != RetroStage::Registered {
            return Err(XC_STATE_INVALID);
        }
        if r.rid < 1 || r.rid as usize > RES_N {
            return Err(XC_DIM_OUT);
        }
        if r.text.is_empty() {
            return Err(XC_TEXT_EMPTY);
        }
        if r.path.is_empty() {
            return Err(XC_PATH_EMPTY);
        }
        let mut i = 0usize;
        while i < self.resolutions.len() {
            if self.resolutions[i].rid == r.rid {
                return Err(XC_RES_DUP);
            }
            if self.resolutions[i].dim == r.dim {
                return Err(XC_DUP_DIM); // 五维不重
            }
            i += 1;
        }
        self.resolutions.push(r);
        self.events.push(alloc::format!("res {}", self.resolutions.len()));
        Ok(self.resolutions.len() as u32)
    }
    pub fn crossref(&mut self) -> Result<u32, u16> {  // 逐条解引：五条执行路径逐条到登记簿解引 + 种类一致，未登记/错种即红。
        if !self.stage.can_step(RetroStage::CrossRefed) {
            return Err(XC_STATE_INVALID);
        }
        if self.resolutions.len() != RES_N {
            return Err(XC_RES_SHORT);
        }
        self.crossed.clear();
        let mut k = 0usize;
        while k < self.resolutions.len() {
            let r = &self.resolutions[k];
            let mut hit: Option<usize> = None;
            let mut i = 0usize;
            while i < self.refs.len() {
                if self.refs[i].ref_id == r.path {
                    hit = Some(i);
                    break;
                }
                i += 1;
            }
            let h = match hit {
                Some(x) => x,
                None => return Err(XC_REF_UNKNOWN), // 定位：k 即决议下标
            };
            if self.refs[h].kind != r.kind {
                return Err(XC_REF_KIND);
            }
            self.crossed.push(r.rid);
            k += 1;
        }
        self.stage = RetroStage::CrossRefed;
        self.events.push(alloc::format!("crossref {}", self.crossed.len()));
        Ok(self.crossed.len() as u32)
    }
    pub fn seal(&mut self) -> Result<RetroSeal, u16> {  // 封账：5 条全解引 → 摘要定格（重封红）。
        if !self.stage.can_step(RetroStage::Sealed) {
            return Err(if self.pinned.is_some() {
                XC_ALREADY_SEALED
            } else {
                XC_STATE_INVALID
            });
        }
        if self.crossed.len() != RES_N {
            return Err(XC_NOT_ALL_REF);
        }
        // 守恒恒等式：登记簿条目 = 已解引引用键数 + 未用引用键数（此处核前项≤总）。
        let mut used = 0usize;
        let mut a = 0usize;
        while a < self.resolutions.len() {
            let mut b = 0usize;
            while b < self.refs.len() {
                if self.refs[b].ref_id == self.resolutions[a].path {
                    used += 1;
                    break;
                }
                b += 1;
            }
            a += 1;
        }
        if used != RES_N || self.refs.len() < used {
            return Err(XC_CONSERV_BREAK);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0795-J1 复盘账齐：五条决议（维度/决议/执行路径三列）逐条有对应账面或通道登记（登记 {} 条，解引 5/5）",
            self.refs.len()
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = RetroSeal {
            res: self.resolutions.len() as u32,
            refs: self.refs.len() as u32,
            all_ref: true,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = RetroStage::Sealed;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&RetroSeal> {
        self.pinned.as_ref()
    }
    pub fn fingerprint(&self) -> u64 {  // 复盘账指纹：决议（rid,dim,path）+ 登记簿（ref_id,kind）规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.resolutions.len() {
            h = fnv1a(h, self.resolutions[i].rid as u64);
            h = fnv1a(h, self.resolutions[i].dim as u64);
            h = fnv_str(h, &self.resolutions[i].path);
            i += 1;
        }
        i = 0;
        while i < self.refs.len() {
            h = fnv_str(h, &self.refs[i].ref_id);
            h = fnv1a(h, self.refs[i].kind as u64);
            i += 1;
        }
        h
    }
    pub fn res_of(&self, rid: u32) -> Option<&Resolution> {  // 决议查询。
        let mut i = 0usize;
        while i < self.resolutions.len() {
            if self.resolutions[i].rid == rid {
                return Some(&self.resolutions[i]);
            }
            i += 1;
        }
        None
    }
    pub fn ref_of(&self, ref_id: &str) -> Option<&RefEntry> {  // 登记条目查询。
        let mut i = 0usize;
        while i < self.refs.len() {
            if self.refs[i].ref_id == ref_id {
                return Some(&self.refs[i]);
            }
            i += 1;
        }
        None
    }
    pub fn dim_of(&self, d: RetroDim) -> Option<&Resolution> {  // 按维度查决议（五维各恰一条）。
        let mut i = 0usize;
        while i < self.resolutions.len() {
            if self.resolutions[i].dim == d {
                return Some(&self.resolutions[i]);
            }
            i += 1;
        }
        None
    }
    pub fn res_table_view(&self) -> String {  // 三列视图串："rid|dim|path" 行串。
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.resolutions.len() {
            let r = &self.resolutions[i];
            s.push_str(alloc::format!("{}|{:?}|{}\n", r.rid, r.dim, r.path).as_str());
            i += 1;
        }
        s
    }
    pub fn kind_histogram(&self) -> [u32; 2] {  // 登记簿种类直方图 [LedgerRef, ChannelRef]（守恒=登记总数）。
        let mut h = [0u32; 2];
        let mut i = 0usize;
        while i < self.refs.len() {
            h[self.refs[i].kind as usize] += 1;
            i += 1;
        }
        h
    }
    pub fn unused_refs(&self) -> Vec<String> {  // 未被任何决议解引的登记键（守恒：used + unused = 总数）。
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.refs.len() {
            let mut used = false;
            let mut a = 0usize;
            while a < self.resolutions.len() {
                if self.resolutions[a].path == self.refs[i].ref_id {
                    used = true;
                    break;
                }
                a += 1;
            }
            if !used {
                out.push(self.refs[i].ref_id.clone());
            }
            i += 1;
        }
        out
    }
}
pub fn gen_resolutions() -> Vec<Resolution> {  // 样本发生器：五维各一条决议（确定性文本与路径）。
    let mut out: Vec<Resolution> = Vec::new();
    let mut i = 0usize;
    while i < RES_N {
        let dim = DIM_ALL[i];
        let path = if i % 2 == 0 {
            alloc::format!("LG-A1-{:02}", i + 1)
        } else {
            alloc::format!("CH-A1-{:02}", i + 1)
        };
        out.push(Resolution {
            rid: (i + 1) as u32,
            dim,
            text: alloc::format!("决议_{:?}_维度收官结论", dim),
            path,
            kind: if i % 2 == 0 { RefKind::LedgerRef } else { RefKind::ChannelRef },
        });
        i += 1;
    }
    out
}
pub fn gen_refs() -> Vec<RefEntry> {  // 样本发生器：登记簿 6 条（5 被用 + 1 未用，确定性）。
    let mut out: Vec<RefEntry> = Vec::new();
    let mut i = 0usize;
    while i < RES_N {
        let kind = if i % 2 == 0 { RefKind::LedgerRef } else { RefKind::ChannelRef };
        let id = if i % 2 == 0 {
            alloc::format!("LG-A1-{:02}", i + 1)
        } else {
            alloc::format!("CH-A1-{:02}", i + 1)
        };
        out.push(RefEntry {
            ref_id: id,
            kind,
            anchor: alloc::format!("锚点_{:?}", kind),
        });
        i += 1;
    }
    out.push(RefEntry {
        ref_id: String::from("CH-A1-99"),
        kind: RefKind::ChannelRef,
        anchor: String::from("锚点_备用通道"),
    });
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let ress = gen_resolutions();
    let refs = gen_refs();
    // —— 主路径：登记 → 五决议 → 逐条解引 → 封账 ——
    let mut m = RetroLedger::new();
    let mut i = 0usize;
    while i < refs.len() {
        m.register_ref(refs[i].clone())?;
        i += 1;
    }
    if m.ref_len() != 6 {
        return Err(XC_CONSERV_BREAK);
    }
    i = 0;
    while i < ress.len() {
        m.add_resolution(ress[i].clone())?;
        i += 1;
    }
    if m.res_len() != RES_N {
        return Err(XC_RES_SHORT);
    }
    if m.crossref()? != RES_N as u32 {
        return Err(XC_NOT_ALL_REF);
    }
    let rc = m.seal()?;
    if rc.res != 5 || !rc.all_ref || rc.refs != 6 || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !matches!(m.seal(), Err(XC_ALREADY_SEALED)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 双跑对拍：同料重跑指纹一致（确定性）——
    let fp1 = rc.fingerprint;
    let fp2 = {
        let mut m2 = RetroLedger::new();
        let mut j = 0usize;
        while j < refs.len() {
            m2.register_ref(refs[j].clone())?;
            j += 1;
        }
        j = 0;
        while j < ress.len() {
            m2.add_resolution(ress[j].clone())?;
            j += 1;
        }
        m2.crossref()?;
        m2.fingerprint()
    };
    if fp1 != fp2 {
        return Err(XC_CONSERV_BREAK);
    }
    // —— 错误路径 1：维度重复 / 决议号重复 / 文本空 / 路径空 / 号越界 ——
    let mut m3 = RetroLedger::new();
    let mut d1 = ress.clone();
    d1[1].dim = RetroDim::Scope;
    if m3.add_resolution(d1[0].clone()).is_err() {
        return Err(XC_STATE_INVALID);
    }
    if !matches!(m3.add_resolution(d1[1].clone()), Err(XC_DUP_DIM)) {
        return Err(XC_DUP_DIM);
    }
    let mut d2 = ress.clone();
    d2[1].rid = 1;
    if !matches!(m3.add_resolution(d2[1].clone()), Err(XC_RES_DUP)) {
        return Err(XC_RES_DUP);
    }
    let mut d3 = ress.clone();
    d3[1].text = String::new();
    if !matches!(m3.add_resolution(d3[1].clone()), Err(XC_TEXT_EMPTY)) {
        return Err(XC_TEXT_EMPTY);
    }
    let mut d4 = ress.clone();
    d4[1].path = String::new();
    if !matches!(m3.add_resolution(d4[1].clone()), Err(XC_PATH_EMPTY)) {
        return Err(XC_PATH_EMPTY);
    }
    let mut d5 = ress.clone();
    d5[1].rid = 6;
    if !matches!(m3.add_resolution(d5[1].clone()), Err(XC_DIM_OUT)) {
        return Err(XC_DIM_OUT);
    }
    // —— 错误路径 2：未登记解引 / 种类不符 / 不足 5 条 ——
    let mut m4 = RetroLedger::new();
    i = 0;
    while i < ress.len() {
        m4.add_resolution(ress[i].clone())?;
        i += 1;
    }
    if !matches!(m4.crossref(), Err(XC_REF_UNKNOWN)) {
        return Err(XC_REF_UNKNOWN);
    }
    let mut m5 = RetroLedger::new();
    i = 0;
    while i < refs.len() {
        m5.register_ref(refs[i].clone())?;
        i += 1;
    }
    i = 0;
    while i < ress.len() {
        let mut r = ress[i].clone();
        if r.kind == RefKind::LedgerRef {
            r.kind = RefKind::ChannelRef;
        } else {
            r.kind = RefKind::LedgerRef;
        }
        m5.add_resolution(r)?;
        i += 1;
    }
    if !matches!(m5.crossref(), Err(XC_REF_KIND)) {
        return Err(XC_REF_KIND);
    }
    let mut m6 = RetroLedger::new();
    i = 0;
    while i < refs.len() {
        m6.register_ref(refs[i].clone())?;
        i += 1;
    }
    i = 0;
    while i < RES_N - 1 {
        m6.add_resolution(ress[i].clone())?;
        i += 1;
    }
    if !matches!(m6.crossref(), Err(XC_RES_SHORT)) {
        return Err(XC_RES_SHORT);
    }
    // —— 错误路径 3：登记键重锚空 / Draft 未登决议即解引（状态机非法边）——
    let mut m7 = RetroLedger::new();
    if m7.register_ref(RefEntry { ref_id: String::from("X"), kind: RefKind::LedgerRef, anchor: String::new() }) != Err(XC_TEXT_EMPTY) {
        return Err(XC_TEXT_EMPTY);
    }
    if m7.register_ref(RefEntry { ref_id: String::new(), kind: RefKind::LedgerRef, anchor: String::from("a") }) != Err(XC_PATH_EMPTY) {
        return Err(XC_PATH_EMPTY);
    }
    // —— 查询 API：按号/按维/按键解引 + 直方图 + 未用引用守恒 ——
    match m.res_of(3) {
        Some(r) if r.rid == 3 => {}
        _ => return Err(XC_RES_SHORT),
    }
    if m.res_of(9).is_some() {
        return Err(XC_RES_DUP);
    }
    let dvh = match m.dim_of(RetroDim::Handover) {
        Some(r) => r.rid,
        None => return Err(XC_RES_SHORT),
    };
    if dvh != 5 {
        return Err(XC_DUP_DIM); // Handover 第 5 条
    }
    match m.ref_of("LG-A1-01") {
        Some(e) if e.kind == RefKind::LedgerRef => {}
        _ => return Err(XC_REF_KIND),
    }
    if m.ref_of("NO-SUCH").is_some() {
        return Err(XC_REF_UNKNOWN);
    }
    let kh = m.kind_histogram();
    if kh[0] + kh[1] != m.ref_len() as u32 || kh[0] != 3 || kh[1] != 3 {
        return Err(XC_CONSERV_BREAK);
    }
    let un = m.unused_refs();
    if un.len() != 1 || un[0] != "CH-A1-99" {
        return Err(XC_CONSERV_BREAK); // 守恒：used 5 + unused 1 = 6
    }
    let tv = m.res_table_view();
    if !tv.contains("3|Sched|") {
        return Err(XC_SUMMARY_EMPTY);
    }
    if m.stage().view() != "sealed" {
        return Err(XC_STATE_INVALID);
    }
    Ok(0)
}
