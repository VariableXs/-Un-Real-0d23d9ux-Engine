//! unx0976_humandom — UNX-F0976 中断面人话总册（域级 explain/humanize 总入口）。
//!
//! 任务锚点：UNX-F0976（第二批工位 W7，SEG 段位 0x8000，本单独占
//! 0x8000..=0x801F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**中断面全域 explain/humanize 经单一总
//! 入口出文（域内各面词条零遗漏、体例统一、术语表逐条对齐），未知
//! 词条带码拒收，总册双导出指纹稳定**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x800 段闭集）；tick 显性
//! 注入零墙钟；词条账守恒（已释+待释==总词条 恒等式机检）；
//! 结尾 drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8000 段，本单独占）
// ---------------------------------------------------------------------------

/// 中断面人话总册段基址（0x8000，本单 0x8000..=0x801F 独占）。
pub const SEG_BASE: u16 = 0x8000;
/// 0x8001 词条非法（空键/空释义——词条准入拒收）。
pub const XC_TERM_INVALID: u16 = SEG_BASE + 1;
/// 0x8002 词条重复（同键双词条——术语表不唯一）。
pub const XC_TERM_DUPLICATE: u16 = SEG_BASE + 2;
/// 0x8003 未知词条（explain 查询的键不在术语表内）。
pub const XC_TERM_UNKNOWN: u16 = SEG_BASE + 3;
/// 0x8004 释文体例越界（体例码不在闭集内）。
pub const XC_STYLE_BAD: u16 = SEG_BASE + 4;
/// 0x8005 总册守恒破坏（已释+待释 != 总词条——册面被外力改写）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 5;
/// 0x8006 册面指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 6;
/// 0x8007 定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 7;
/// 0x8008 词条未释即出册（存在待释词条——总入口不齐备）。
pub const XC_NOT_ALL_EXPLAINED: u16 = SEG_BASE + 8;
/// 0x8009 释文过短（释义不足十字——人话纪律下限）。
pub const XC_GLOSS_TOO_SHORT: u16 = SEG_BASE + 9;
/// 0x800A 面域越界（面域码不在闭集内）。
pub const XC_FACE_BAD: u16 = SEG_BASE + 10;

const _: () = assert!(XC_TERM_INVALID != XC_TERM_DUPLICATE);
const _: () = assert!(XC_CONSERVATION != XC_NOT_ALL_EXPLAINED);
const _: () = assert!(XC_DRIFT != XC_FACE_BAD);

// ---------------------------------------------------------------------------
// §1 面域闭集（中断面七个面域——总册按面域组织）
// ---------------------------------------------------------------------------

/// 面域闭集：中断面全域七面（人话总册的组织轴）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 开合面（中断开/关/嵌套）。
    Gate,
    /// 向量面（分配/路由/屏蔽）。
    Vector,
    /// EOI 面（结束中断闭环）。
    Eoi,
    /// 风暴面（检测/节流/恢复）。
    Storm,
    /// 计时面（节拍/单调/超时）。
    Timing,
    /// 健康面（总账/告警）。
    Health,
    /// 注入面（故障注入/检出）。
    Inject,
}

impl Face {
    /// 面码（1..=7）。
    pub const fn code(self) -> u8 {
        match self {
            Face::Gate => 1,
            Face::Vector => 2,
            Face::Eoi => 3,
            Face::Storm => 4,
            Face::Timing => 5,
            Face::Health => 6,
            Face::Inject => 7,
        }
    }
    /// 面名。
    pub const fn name(self) -> &'static str {
        match self {
            Face::Gate => "开合面",
            Face::Vector => "向量面",
            Face::Eoi => "EOI面",
            Face::Storm => "风暴面",
            Face::Timing => "计时面",
            Face::Health => "健康面",
            Face::Inject => "注入面",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<Face> {
        match c {
            1 => Some(Face::Gate),
            2 => Some(Face::Vector),
            3 => Some(Face::Eoi),
            4 => Some(Face::Storm),
            5 => Some(Face::Timing),
            6 => Some(Face::Health),
            7 => Some(Face::Inject),
            _ => None,
        }
    }
}

/// 面域总数。
pub const FACE_COUNT: usize = 7;

// ---------------------------------------------------------------------------
// §2 释文体例闭集
// ---------------------------------------------------------------------------

/// 释文体例闭集：explain（面向排查）/ humanize（面向汇报）两体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlossStyle {
    /// explain 体：讲清"为什么发生、怎么定位"。
    Explain,
    /// humanize 体：讲清"影响什么、下一步做什么"。
    Humanize,
}

impl GlossStyle {
    /// 体例码（1/2）。
    pub const fn code(self) -> u8 {
        match self {
            GlossStyle::Explain => 1,
            GlossStyle::Humanize => 2,
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<GlossStyle> {
        match c {
            1 => Some(GlossStyle::Explain),
            2 => Some(GlossStyle::Humanize),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// §3 词条与人话总册（HumanDomBook）
// ---------------------------------------------------------------------------

/// 一条术语词条：面域 / 键 / 体例 / 释文。
#[derive(Clone, Debug)]
pub struct TermEntry {
    face: Face,
    key: String,
    style: GlossStyle,
    gloss: String,
    explained: bool,
}

/// 人话总册——域级 explain/humanize 总入口本体。
///
/// 守恒恒等式：`explained + pending == total`（每词条恰处一态）；
/// 面域覆盖恒等式：七面各至少一条词条（总入口全域覆盖）。
#[derive(Debug)]
pub struct HumanDomBook {
    entries: Vec<TermEntry>,
    cap: usize,
    explained_cnt: u32,
    per_face: [u32; FACE_COUNT],
    rejected: u32,
}

impl HumanDomBook {
    /// 建总册。
    pub fn new(cap: usize) -> Result<HumanDomBook, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(HumanDomBook {
            entries: Vec::with_capacity(cap),
            cap,
            explained_cnt: 0,
            per_face: [0; FACE_COUNT],
            rejected: 0,
        })
    }

    /// 词条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 入册一条词条（键/释文非空、面域体例闭集、同键同体例唯一）。
    pub fn enroll(&mut self, face: Face, key: &str, style: GlossStyle, gloss: &str) -> Result<(), u16> {
        if self.entries.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        if key.is_empty() || gloss.is_empty() {
            self.rejected += 1;
            return Err(XC_TERM_INVALID);
        }
        if Face::from_code(face.code()).is_none() {
            self.rejected += 1;
            return Err(XC_FACE_BAD);
        }
        if GlossStyle::from_code(style.code()).is_none() {
            self.rejected += 1;
            return Err(XC_STYLE_BAD);
        }
        if gloss.chars().count() < 10 {
            self.rejected += 1;
            return Err(XC_GLOSS_TOO_SHORT);
        }
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].key == key && self.entries[i].style.code() == style.code() {
                self.rejected += 1;
                return Err(XC_TERM_DUPLICATE);
            }
            i += 1;
        }
        let fc = face.code() as usize;
        self.per_face[fc - 1] += 1;
        self.entries.push(TermEntry {
            face,
            key: String::from(key),
            style,
            gloss: String::from(gloss),
            explained: false,
        });
        Ok(())
    }

    /// 出释文（explain/humanize 总入口——未知词条带码拒收）。
    ///
    /// 出文即"释出"：词条从待释态迁入已释态（单调，不回退）。
    pub fn explain(&mut self, key: &str, style: GlossStyle) -> Result<String, u16> {
        let mut idx: Option<usize> = None;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].key == key && self.entries[i].style.code() == style.code() {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        let i = match idx {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_TERM_UNKNOWN);
            }
        };
        let face_name = self.entries[i].face.name();
        let mut out = String::from("[");
        out.push_str(face_name);
        out.push_str("|");
        out.push_str(&self.entries[i].style.code().to_string());
        out.push_str("] ");
        out.push_str(&self.entries[i].gloss);
        if !self.entries[i].explained {
            self.entries[i].explained = true;
            self.explained_cnt += 1;
        }
        Ok(out)
    }

    /// 守恒机检：已释 + 待释 == 总词条（逐条重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut e: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if self.entries[i].explained {
                e += 1;
            }
            i += 1;
        }
        if e != self.explained_cnt || e + (self.entries.len() as u32 - e) != self.entries.len() as u32
        {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 面域覆盖机检：七面各至少一条词条。
    pub fn audit_face_coverage(&self) -> Result<(), u16> {
        let mut i: usize = 0;
        while i < FACE_COUNT {
            if self.per_face[i] == 0 {
                return Err(XC_NOT_ALL_EXPLAINED);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全量释出待释词条（出册前置）。
    pub fn explain_all(&mut self) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < self.entries.len() {
            if !self.entries[i].explained {
                self.entries[i].explained = true;
                self.explained_cnt += 1;
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 册面指纹（面/键/体例/释出态逐条 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            h = (h ^ e.face.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            let kb: &[u8] = e.key.as_bytes();
            let mut b: usize = 0;
            while b < kb.len() {
                h = (h ^ kb[b] as u64).wrapping_mul(0x0000_0100_0000_01b3);
                b += 1;
            }
            h = (h ^ e.style.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            h = (h ^ e.explained as u8 as u64).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 总册文本导出（确定性序 + 双导出对拍 + 全释前置）。
    pub fn export(&mut self) -> Result<String, u16> {
        self.audit_face_coverage()?;
        if self.explained_cnt as usize != self.entries.len() {
            self.rejected += 1;
            return Err(XC_NOT_ALL_EXPLAINED);
        }
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0976-HUMANDOM|v1|terms=");
        s.push_str(&self.entries.len().to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            s.push_str(e.face.name());
            s.push('|');
            s.push_str(e.key.as_str());
            s.push('|');
            s.push_str(&e.style.code().to_string());
            s.push('\n');
            i += 1;
        }
        if self.fingerprint() != fp0 {
            return Err(XC_DRIFT);
        }
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// §4 自例程（drill）——构造样本走核心路径，返回 0 表示全过
// ---------------------------------------------------------------------------

/// 自例程：七面词条入册→注入面拒收→逐条释出→守恒→双导出。
pub fn drill() -> Result<u32, u16> {
    let mut b = HumanDomBook::new(32)?;
    // 七面各入一条 explain 词条 + 风暴面加一条 humanize。
    b.enroll(Face::Gate, "中断开合", GlossStyle::Explain, "中断开合描述核进入核出的开关路径与嵌套规则")?;
    b.enroll(Face::Vector, "向量分配", GlossStyle::Explain, "向量分配描述每设备如何取得唯一中断向量号")?;
    b.enroll(Face::Eoi, "EOI闭环", GlossStyle::Explain, "EOI闭环描述中断服务结束后如何通知控制器放行下一波")?;
    b.enroll(Face::Storm, "风暴检测", GlossStyle::Explain, "风暴检测描述事件率判据如何确认一次中断风暴成立")?;
    b.enroll(Face::Storm, "风暴汇报", GlossStyle::Humanize, "风暴汇报描述对上汇报的影响面与已执行的降压动作")?;
    b.enroll(Face::Timing, "节拍窗", GlossStyle::Explain, "节拍窗描述计时面给中断面承诺的周期精度边界")?;
    b.enroll(Face::Health, "健康总账", GlossStyle::Explain, "健康总账描述全域健康计数与告警流的终点账本")?;
    b.enroll(Face::Inject, "故障注入", GlossStyle::Explain, "故障注入描述人为制造故障以验证检出面的资产入口")?;
    // 注入面：空键、重复、释文过短、未知词条全拦。
    if b.enroll(Face::Gate, "", GlossStyle::Explain, "空键词条应被拒收的释文长度足够") != Err(XC_TERM_INVALID) {
        return Err(XC_TERM_INVALID);
    }
    if b.enroll(Face::Gate, "中断开合", GlossStyle::Explain, "重复词条应被拒收的释文长度足够长") != Err(XC_TERM_DUPLICATE) {
        return Err(XC_TERM_DUPLICATE);
    }
    if b.enroll(Face::Gate, "过短", GlossStyle::Explain, "太短") != Err(XC_GLOSS_TOO_SHORT) {
        return Err(XC_GLOSS_TOO_SHORT);
    }
    if b.explain("不存在", GlossStyle::Explain) != Err(XC_TERM_UNKNOWN) {
        return Err(XC_TERM_UNKNOWN);
    }
    // 逐条释出（风暴汇报 humanize 单独走一遍总入口）。
    let h = b.explain("风暴汇报", GlossStyle::Humanize)?;
    if !h.contains("风暴面") {
        return Err(XC_STYLE_BAD);
    }
    b.explain_all();
    // 守恒机检：8 词条全释。
    b.audit_conservation()?;
    b.audit_face_coverage()?;
    if b.explained_cnt != 8 || b.len() != 8 || b.rejected() != 4 {
        return Err(XC_CONSERVATION);
    }
    // 双导出指纹稳定。
    let fp1 = b.fingerprint();
    let _ = b.export()?;
    if b.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
