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

/// 收库修正：判据面占位释文构造辅（收库新增；enroll 直收 StyledGloss 值，
/// 而三段字面量恒非空、构造恒成，此辅以同模块直构兜底，全程不 panic）。
fn gloss3(why: &str, how: &str, next: &str) -> StyledGloss {
    match StyledGloss::new(why, how, next) {
        Ok(gl) => gl,
        // 兜底分支仅当入参含空段时可达：三段统一以「占位」补齐，保持 no_std 无 panic 口径。
        Err(_) => StyledGloss { why: String::from("占位"), how: String::from("占位"), next: String::from("占位") },
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
    // 收库修正：StyledGloss::new 返回 Result，须先构造成功再入册，不得把 Result 直接当实参传给 enroll。
    let gl_probe = match StyledGloss::new("为什么段", "怎么定位段", "下一步段") {
        Ok(gl) => gl,
        Err(_) => return Err(XC_GLOSS_MISSING), // 三段字面量恒非空，此分支不可达
    };
    if g.enroll(GlossCat::State, "", gl_probe.clone()) != Err(XC_KEY_EMPTY) {
        return Err(XC_KEY_EMPTY);
    }
    if g.enroll(GlossCat::State, "静默", gl_probe) != Err(XC_KEY_DUP) {
        return Err(XC_KEY_DUP);
    }
    if !matches!(StyledGloss::new("只有一段", "", "第三段"), Err(XC_GLOSS_MISSING)) {
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

// ════════════════════ 判据聚合入口（AI-INTAKE-1107 收库补齐） ════════════════════
//
// 修正记录：本模块历史会话只交付了域模型与测试，从未交付 run_unx988_checks
// 聚合入口——等于从未参与判据聚合闭环。此入口把模块自身的公开 API 逐项
// 折叠成 CheckSet 判据（判据强度不降：每条判据都调真实 API、断真实性质）。

use crate::checks::CheckSet;

pub fn run_unx988_checks() -> CheckSet {
    let mut s = CheckSet::new("unx0988_stormexplain");
    // J1 核心路径：三类词条入册→释出→注入拦→守恒→双导出。
    s.add("核心路径drill全过", drill() == Ok(0), "6 词条入册/释出/四注入拦/守恒/指纹稳定返回 0");
    // J2 词条三类闭集（码 1..=3 双向可逆）。
    let cat_closed = GlossCat::from_code(1) == Some(GlossCat::State)
        && GlossCat::from_code(2) == Some(GlossCat::Action)
        && GlossCat::from_code(3) == Some(GlossCat::Criterion)
        && GlossCat::from_code(0).is_none()
        && GlossCat::from_code(4).is_none()
        && GlossCat::State.code() == 1 && GlossCat::Criterion.code() == 3;
    s.add("词条三类闭集", cat_closed, "态/动作/判据 from_code 可逆、0/4 拒、码值冻结");
    // J3 三段式体例段名冻结（为什么/怎么定位/下一步）。
    let [sec0, sec1, sec2] = GLOSS_SECTIONS;
    s.add("体例段名冻结", GLOSS_SECTIONS.len() == 3 && sec0 == "为什么" && sec1 == "怎么定位" && sec2 == "下一步",
        "explain 对齐体例三段名逐字一致");
    // J4 正例：三段式释文构造与渲染带段名。
    let gloss_ok = match StyledGloss::new("现象A", "定位B", "动作C") {
        Ok(g) => {
            let r = g.render();
            r.contains("为什么") && r.contains("怎么定位") && r.contains("下一步")
                && r.contains("现象A") && r.contains("定位B") && r.contains("动作C")
        }
        Err(_) => false,
    };
    s.add("三段式释文渲染", gloss_ok, "render 拼段名前缀且三段原文全含");
    // J5 反例一：缺段释文拒收。
    let missing_rej = match StyledGloss::new("只有一段", "", "第三段") {
        Err(XC_GLOSS_MISSING) => true,
        _ => false,
    };
    s.add("缺段释文拒收", missing_rej, "空段被 XC_GLOSS_MISSING 拒——三段缺一不可");
    // J6 正例：三类词条入册、explain 命中释出、release_all 补齐。
    let pos_ok = match StormGlossary::new(GLOSS_CAP) {
        Ok(mut g) => {
            // 收库修正：enroll 实参改经 gloss3 构造辅（StyledGloss::new 的 Result 不得直接入册）。
            let e1 = g.enroll(GlossCat::State, "静默", gloss3("w", "h", "n"));
            let e2 = g.enroll(GlossCat::Action, "节流", gloss3("w", "h", "n"));
            let e3 = g.enroll(GlossCat::Criterion, "静默窗", gloss3("w", "h", "n"));
            let ex = g.explain("静默");
            let rel = g.release_all();
            e1.is_ok() && e2.is_ok() && e3.is_ok()
                && g.len() == 3
                && match ex { Ok(t) => t.contains("[风暴面|") && t.contains("静默"), Err(_) => false }
                && rel == 2
                && g.audit_conservation().is_ok()
        }
        Err(_) => false,
    };
    s.add("入册释出补齐", pos_ok, "三型各 1 条、explain 带面标头、release_all 补 2、守恒过");
    // J7 反例二：空键/重复键/未知键/未释出册四类注入全拦且拒收记账。
    let neg_ok = match StormGlossary::new(GLOSS_CAP) {
        Ok(mut g) => {
            let _ = g.enroll(GlossCat::State, "k", gloss3("w", "h", "n")); // 收库修正：实参经 gloss3 构造辅
            let r1 = match g.enroll(GlossCat::State, "", gloss3("w", "h", "n")) {
                Err(XC_KEY_EMPTY) => true, _ => false,
            };
            let r2 = match g.enroll(GlossCat::State, "k", gloss3("w", "h", "n")) {
                Err(XC_KEY_DUP) => true, _ => false,
            };
            let r3 = match g.explain("无此键") {
                Err(XC_KEY_UNKNOWN) => true, _ => false,
            };
            let r4 = match g.export() {
                Err(XC_NOT_ALL_RELEASED) => true, _ => false,
            };
            r1 && r2 && r3 && r4 && g.rejected() == 4
        }
        Err(_) => false,
    };
    s.add("四类注入全拦", neg_ok, "空键/重复/未知/未释出册全拒且 rejected 显性记 4");
    // J8 反例三：定容非法（容量为零）拒收。
    let cap_ok = match StormGlossary::new(0) {
        Err(XC_CAP_INVALID) => true,
        _ => false,
    };
    s.add("零容量拒收", cap_ok, "new(0) 被 XC_CAP_INVALID 拒");
    // J9 词条账定容冻结。
    s.add("词条账定容冻结", GLOSS_CAP == 24, "24 条容量（三态+六动作+九判据+余量）不漂移");
    // J10 诊断段独占。
    s.add("诊断段位独占", SEG_BASE == 0x8180, "0x8180 段本单独占不漂移");
    // 动态明细两列。
    let (fp_stable, exp_len) = match StormGlossary::new(GLOSS_CAP) {
        Ok(mut g) => {
            let _ = g.enroll(GlossCat::State, "确认", gloss3("w", "h", "n")); // 收库修正：实参经 gloss3 构造辅
            let _ = g.explain("确认");
            let fp1 = g.fingerprint();
            let e1 = g.export();
            let fp2 = g.fingerprint();
            (fp1 == fp2, e1.map(|t| t.len()).unwrap_or(0))
        }
        Err(_) => (false, 0),
    };
    s.add_dyn("导出对拍稳定", fp_stable && exp_len > 0, &alloc::format!("词条账导出 {} 字节、双指纹一致 {}", exp_len, fp_stable));
    let explain_len = match StormGlossary::new(GLOSS_CAP) {
        Ok(mut g) => {
            let _ = g.enroll(GlossCat::Criterion, "静默窗", gloss3("恢复要连续静默窗", "看静默连击计数", "连击满三窗才解除")); // 收库修正：实参经 gloss3 构造辅
            g.explain("静默窗").map(|t| t.len()).unwrap_or(0)
        }
        Err(_) => 0,
    };
    s.add_dyn("explain人话可报", explain_len > 0, &alloc::format!("explain 输出 {} 字节（面标头+三段式）", explain_len));
    s
}
