//! unx0988_stormexplain — UNX-F0988 风暴面人话化（explain 对齐体例）。
//!
//! 任务锚点：UNX-F0988（第二批工位 W7，SEG 段位 0x8180，本单独占
//! 0x8180..=0x819F；诊断码自 SEG_BASE+1 起闭集分派）。
//! 判据摘录（卡面验收行）：**风暴面全部状态与动作有人话词条（态/
//! 动作/判据三类词条零遗漏），释文对齐 explain 体例（为什么-怎么定
//! 位-下一步三段式），未知态查询带码拒收，词条账守恒**。
//! 设计纪律：no_std+alloc；零 unwrap/expect/panic!/浮点/std；
//! 全可失败路径 `Result<T, u16>`（错误码 0x818 段闭集）；tick 显性
//! 注入零墙钟；词条账定容守恒（已释+待释==总词条 恒等式机检）；
//! 结尾 drill() 自例程走核心路径返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// §0 诊断码（0x8180 段，本单独占）
// ---------------------------------------------------------------------------

/// 风暴面人话化段基址（0x8180，本单 0x8180..=0x819F 独占）。
pub const SEG_BASE: u16 = 0x8180;
/// 0x8181 词条键非法（空键——准入拒收）。
pub const XC_KEY_EMPTY: u16 = SEG_BASE + 1;
/// 0x8182 词条重复（同键双词条——术语表不唯一）。
pub const XC_KEY_DUP: u16 = SEG_BASE + 2;
/// 0x8183 词条未知（explain 查询的键不在表内）。
pub const XC_KEY_UNKNOWN: u16 = SEG_BASE + 3;
/// 0x8184 释文缺段（explain 体例三段式任一段缺失）。
pub const XC_GLOSS_MISSING: u16 = SEG_BASE + 4;
/// 0x8185 词条类越界（类码不在三类闭集内）。
pub const XC_CAT_BAD: u16 = SEG_BASE + 5;
/// 0x8186 词条账守恒破坏（已释+待释 != 总词条）。
pub const XC_CONSERVATION: u16 = SEG_BASE + 6;
/// 0x8187 词条账指纹漂移（双导出指纹失配）。
pub const XC_DRIFT: u16 = SEG_BASE + 7;
/// 0x8188 词条账定容非法（容量为零）。
pub const XC_CAP_INVALID: u16 = SEG_BASE + 8;
/// 0x8189 未释词条出册（存在待释词条即出册——齐备性破坏）。
pub const XC_NOT_ALL_RELEASED: u16 = SEG_BASE + 9;

const _: () = assert!(XC_KEY_EMPTY != XC_KEY_DUP);
const _: () = assert!(XC_GLOSS_MISSING != XC_CAT_BAD);
const _: () = assert!(XC_DRIFT != XC_NOT_ALL_RELEASED);

// ---------------------------------------------------------------------------
// §1 词条类闭集（态 / 动作 / 判据三类）
// ---------------------------------------------------------------------------

/// 词条类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlossCat {
    /// 态词条（检测态人话）。
    State,
    /// 动作词条（处置动作人话）。
    Action,
    /// 判据词条（判据条件人话）。
    Criterion,
}

impl GlossCat {
    /// 类码（1..=3）。
    pub const fn code(self) -> u8 {
        match self {
            GlossCat::State => 1,
            GlossCat::Action => 2,
            GlossCat::Criterion => 3,
        }
    }
    /// 类名。
    pub const fn name(self) -> &'static str {
        match self {
            GlossCat::State => "态",
            GlossCat::Action => "动作",
            GlossCat::Criterion => "判据",
        }
    }
    /// 由码还原。
    pub const fn from_code(c: u8) -> Option<GlossCat> {
        match c {
            1 => Some(GlossCat::State),
            2 => Some(GlossCat::Action),
            3 => Some(GlossCat::Criterion),
            _ => None,
        }
    }
}

/// 三段式体例段名（explain 对齐体例——为什么/怎么定位/下一步）。
pub const GLOSS_SECTIONS: [&str; 3] = ["为什么", "怎么定位", "下一步"];

// ---------------------------------------------------------------------------
// §2 三段式释文（StyledGloss）与词条
// ---------------------------------------------------------------------------

/// 三段式释文：为什么 / 怎么定位 / 下一步（explain 体例对齐本体）。
#[derive(Clone, Debug)]
pub struct StyledGloss {
    why: String,
    how: String,
    next: String,
}

impl StyledGloss {
    /// 构造（三段任一空带码拒收）。
    pub fn new(why: &str, how: &str, next: &str) -> Result<StyledGloss, u16> {
        if why.is_empty() || how.is_empty() || next.is_empty() {
            return Err(XC_GLOSS_MISSING);
        }
        Ok(StyledGloss {
            why: String::from(why),
            how: String::from(how),
            next: String::from(next),
        })
    }

    /// 拼为一段人话输出（段名前缀对齐体例）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(GLOSS_SECTIONS[0]);
        s.push_str("：");
        s.push_str(&self.why);
        s.push('；');
        s.push_str(GLOSS_SECTIONS[1]);
        s.push_str("：");
        s.push_str(&self.how);
        s.push('；');
        s.push_str(GLOSS_SECTIONS[2]);
        s.push_str("：");
        s.push_str(&self.next);
        s
    }
}

/// 一条词条：类 / 键 / 释文 / 释出态。
#[derive(Clone, Debug)]
pub struct StormTerm {
    cat: GlossCat,
    key: String,
    gloss: StyledGloss,
    released: bool,
}

// ---------------------------------------------------------------------------
// §3 风暴面词条账（StormGlossary）
// ---------------------------------------------------------------------------

/// 词条账容量定值：24 条（三态+六动作+九判据 + 余量）。
pub const GLOSS_CAP: usize = 24;

/// 风暴面词条账——风暴面人话化总入口本体。
///
/// 守恒恒等式：`已释 + 待释 == 总词条`（每词条恰处一态）。
#[derive(Debug)]
pub struct StormGlossary {
    terms: Vec<StormTerm>,
    cap: usize,
    released_cnt: u32,
    rejected: u32,
}

impl StormGlossary {
    /// 建词条账。
    pub fn new(cap: usize) -> Result<StormGlossary, u16> {
        if cap == 0 {
            return Err(XC_CAP_INVALID);
        }
        Ok(StormGlossary {
            terms: Vec::with_capacity(cap),
            cap,
            released_cnt: 0,
            rejected: 0,
        })
    }

    /// 词条数。
    pub fn len(&self) -> usize {
        self.terms.len()
    }
    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
    /// 拒收次数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    fn find(&self, key: &str) -> Option<usize> {
        let mut i: usize = 0;
        while i < self.terms.len() {
            if self.terms[i].key == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 入册一条词条（键非空/类闭集/同键唯一/释文三段齐备）。
    pub fn enroll(&mut self, cat: GlossCat, key: &str, gloss: StyledGloss) -> Result<(), u16> {
        if self.terms.len() >= self.cap {
            self.rejected += 1;
            return Err(XC_CAP_INVALID);
        }
        if key.is_empty() {
            self.rejected += 1;
            return Err(XC_KEY_EMPTY);
        }
        if GlossCat::from_code(cat.code()).is_none() {
            self.rejected += 1;
            return Err(XC_CAT_BAD);
        }
        if self.find(key).is_some() {
            self.rejected += 1;
            return Err(XC_KEY_DUP);
        }
        self.terms.push(StormTerm {
            cat,
            key: String::from(key),
            gloss,
            released: false,
        });
        Ok(())
    }

    /// explain 查询（未知键拒收；命中即释出——待释迁已释）。
    pub fn explain(&mut self, key: &str) -> Result<String, u16> {
        let idx = match self.find(key) {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(XC_KEY_UNKNOWN);
            }
        };
        let mut out = String::from("[风暴面|");
        out.push_str(self.terms[idx].cat.name());
        out.push_str("|");
        out.push_str(key);
        out.push_str("] ");
        out.push_str(&self.terms[idx].gloss.render());
        if !self.terms[idx].released {
            self.terms[idx].released = true;
            self.released_cnt += 1;
        }
        Ok(out)
    }

    /// 全量释出待释词条。
    pub fn release_all(&mut self) -> u32 {
        let mut n: u32 = 0;
        let mut i: usize = 0;
        while i < self.terms.len() {
            if !self.terms[i].released {
                self.terms[i].released = true;
                self.released_cnt += 1;
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 守恒机检：已释 + 待释 == 总词条（逐条重扫）。
    pub fn audit_conservation(&self) -> Result<(), u16> {
        let mut r: u32 = 0;
        let mut i: usize = 0;
        while i < self.terms.len() {
            if self.terms[i].released {
                r += 1;
            }
            i += 1;
        }
        if r != self.released_cnt {
            return Err(XC_CONSERVATION);
        }
        if r + (self.terms.len() as u32 - r) != self.terms.len() as u32 {
            return Err(XC_CONSERVATION);
        }
        Ok(())
    }

    /// 词条账指纹（键字节/类/释出态逐条 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i: usize = 0;
        while i < self.terms.len() {
            let t = &self.terms[i];
            h = (h ^ t.cat.code() as u64).wrapping_mul(0x0000_0100_0000_01b3);
            let kb: &[u8] = t.key.as_bytes();
            let mut b: usize = 0;
            while b < kb.len() {
                h = (h ^ kb[b] as u64).wrapping_mul(0x0000_0100_0000_01b3);
                b += 1;
            }
            h = (h ^ t.released as u8 as u64).wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    /// 词条账文本导出（确定性序 + 全释前置 + 双导出对拍）。
    pub fn export(&mut self) -> Result<String, u16> {
        if self.released_cnt as usize != self.terms.len() {
            self.rejected += 1;
            return Err(XC_NOT_ALL_RELEASED);
        }
        self.audit_conservation()?;
        let fp0 = self.fingerprint();
        let mut s = String::from("UNX-F0988-STORMGLOSS|v1|terms=");
        s.push_str(&self.terms.len().to_string());
        s.push('\n');
        let mut i: usize = 0;
        while i < self.terms.len() {
            s.push_str(self.terms[i].cat.name());
            s.push('|');
            s.push_str(self.terms[i].key.as_str());
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

/// 自例程：三类词条入册→explain 释出→注入面全拦→守恒→双导出。
pub fn drill() -> Result<u32, u16> {
    let mut g = StormGlossary::new(GLOSS_CAP)?;
    // 三态词条。
    g.enroll(GlossCat::State, "静默", StyledGloss::new("事件率低于静默阈，无风暴迹象", "看窗口计数与静默连击", "继续观察，无需动作")?)?;
    g.enroll(GlossCat::State, "疑点", StyledGloss::new("率越疑点阈但未满确认持续窗", "看高阈连击计数", "盯紧下一窗，越确认阈即升档")?)?;
    g.enroll(GlossCat::State, "确认", StyledGloss::new("确认判据成立，风暴已发生", "看风暴确认账与触发时刻", "按分级账进入对应处置档")?)?;
    // 动作词条。
    g.enroll(GlossCat::Action, "节流", StyledGloss::new("事件率超出业务承受度", "看节流档位与协商回执", "按档位降压并记录动作账")?)?;
    g.enroll(GlossCat::Action, "隔离", StyledGloss::new("单向量持续致风暴", "看向量态与屏蔽通知账", "屏蔽该向量并通知设备")?)?;
    // 判据词条。
    g.enroll(GlossCat::Criterion, "静默窗", StyledGloss::new("恢复评估需要连续静默窗", "看静默连击计数", "连击满三窗才可申请解除")?)?;
    // 注入面：空键、重复、缺段释文、未知键全拦。
    if g.enroll(GlossCat::State, "", StyledGloss::new("为什么段", "怎么定位段", "下一步段")) != Err(XC_KEY_EMPTY) {
        return Err(XC_KEY_EMPTY);
    }
    if g.enroll(GlossCat::State, "静默", StyledGloss::new("为什么段", "怎么定位段", "下一步段")) != Err(XC_KEY_DUP) {
        return Err(XC_KEY_DUP);
    }
    if StyledGloss::new("只有一段", "", "第三段") != Err(XC_GLOSS_MISSING) {
        return Err(XC_GLOSS_MISSING);
    }
    if g.explain("不存在") != Err(XC_KEY_UNKNOWN) {
        return Err(XC_KEY_UNKNOWN);
    }
    // explain 释出（三段式对齐体例——输出带段名）。
    let s1 = g.explain("确认")?;
    if !s1.contains("为什么") || !s1.contains("怎么定位") || !s1.contains("下一步") {
        return Err(XC_GLOSS_MISSING);
    }
    g.release_all();
    // 守恒机检：6 词条全释。
    g.audit_conservation()?;
    if g.released_cnt != 6 || g.len() != 6 || g.rejected() != 4 {
        return Err(XC_CONSERVATION);
    }
    // 双导出指纹稳定。
    let fp1 = g.fingerprint();
    let _ = g.export()?;
    if g.fingerprint() != fp1 {
        return Err(XC_DRIFT);
    }
    Ok(0)
}
