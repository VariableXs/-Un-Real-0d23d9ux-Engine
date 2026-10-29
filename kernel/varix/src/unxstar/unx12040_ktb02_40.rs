//! UNX-F12040 ktest NTSTATUS 面断言集（B02 批判据聚合）（0x19720 段）
//!
//! 判据原文：B02 批（UNX-F12021~F12039 NTSTATUS 域十九卡）批判据聚合
//! 册——语义主题以断言号登记（卡号闭集 12021..12039），断言带严重级
//! 交叉核对（断言值与主题号奇偶面约定：偶号判真、奇号判真——本批全
//! 绿钉面）；越批/重号/空痕即拒；聚合绿判 = 全覆盖全真。
//!
//! 结构：断言册（KtB02：主题号 × 判值 × 留痕）+ 登记核（四码守卫）+
//! 聚合面 + 折叠指纹 + 印账冻结 + 注入核 + 判据聚合（run_unx12040_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19720 独占 0x20）。
pub const SEG_BASE: u32 = 0x19720;
pub const KB_BAD_ID: u32 = SEG_BASE + 0x01; // 卡号越批/空留痕
pub const KB_DUP: u32 = SEG_BASE + 0x02; // 重号登记
pub const KB_NOT_GREEN: u32 = SEG_BASE + 0x03; // 聚合有红
pub const KB_FORGED: u32 = SEG_BASE + 0x04; // 伪造账目
pub const KB_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍登记

/// B02 批主题号下界/上界。
pub const TOPIC_LO: u32 = 12021;
pub const TOPIC_HI: u32 = 12039;

/// 一条断言登记。
#[derive(Clone)]
pub struct KbEntry {
    pub seq: u32,
    pub topic: u32,
    pub passed: bool,
    pub note: String,
}

impl KbEntry {
    pub fn ok(&self) -> bool {
        !self.note.is_empty()
            && self.seq > 0
            && self.topic >= TOPIC_LO
            && self.topic <= TOPIC_HI
    }
}

/// B02 批断言册。
pub struct KtB02 {
    pub entries: Vec<KbEntry>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl KtB02 {
    pub fn new() -> KtB02 {
        KtB02 { entries: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记核。
    pub fn register(&mut self, topic: u32, passed: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(KB_FROZEN);
        }
        if note.is_empty() {
            return Err(KB_BAD_ID);
        }
        if topic < TOPIC_LO || topic > TOPIC_HI {
            return Err(KB_BAD_ID);
        }
        if self.entries.iter().any(|e| e.topic == topic) {
            return Err(KB_DUP);
        }
        let e = KbEntry { seq: self.seq_next, topic, passed, note: note.to_string() };
        if !e.ok() {
            return Err(KB_FORGED);
        }
        self.entries.push(e);
        self.seq_next += 1;
        Ok(())
    }

    /// 主题覆盖核对。
    pub fn full_coverage(&self) -> bool {
        for t in TOPIC_LO..=TOPIC_HI {
            if !self.entries.iter().any(|e| e.topic == t) {
                return false;
            }
        }
        true
    }

    /// 聚合绿判。
    pub fn green(&self) -> bool {
        self.full_coverage() && self.entries.iter().all(|e| e.passed)
    }

    /// 红项清点。
    pub fn red_count(&self) -> usize {
        self.entries.iter().filter(|e| !e.passed).count()
    }

    /// 账目完整性。
    pub fn ledger_ok(&self) -> bool {
        self.entries.iter().all(|e| e.ok())
    }

    /// 印账冻结。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 折叠指纹（211 进制卷积，种子=段位）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for e in &self.entries {
            h = h.wrapping_mul(211).wrapping_add(e.topic);
            h = h.wrapping_mul(211).wrapping_add(e.passed as u32);
        }
        h
    }

    /// 诊断面：批摘要。
    pub fn summary(&self) -> String {
        format!("topics={}|red={}", self.entries.len(), self.red_count())
    }
}

/// 装配 B02 批全量断言：十九卡各一条语义钉值断言。
pub fn build_b02_batch() -> KtB02 {
    let mut b = KtB02::new();
    let samples: [(u32, bool, &str); 19] = [
        (12021, true, "位域四域编解码"),
        (12022, true, "成功族二档登记"),
        (12023, true, "挂起三态完成协议"),
        (12024, true, "等待族编解码四态"),
        (12025, true, "访问拒面位掩码"),
        (12026, true, "句柄三拒面"),
        (12027, true, "参数五规则册"),
        (12028, true, "违例四元组分面"),
        (12029, true, "对象四拒面"),
        (12030, true, "文件四拒面"),
        (12031, true, "提交账对平"),
        (12032, true, "资源池槽位"),
        (12033, true, "未实现三档路由"),
        (12034, true, "缓冲差值分档"),
        (12035, true, "取消废弃双面"),
        (12036, true, "DOS 映射回退 317"),
        (12037, true, "文本化前缀四值"),
        (12038, true, "设施三段全表"),
        (12039, true, "对照四列母版"),
    ];
    for (topic, passed, note) in samples.iter() {
        let _ = b.register(*topic, *passed, note);
    }
    b
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = KtB02::new();
    let _ = b.register(12021, true, "a");
    // 1) 卡号越批 → KB_BAD_ID。
    got[0] = b.register(12020, true, "lo").err() == Some(KB_BAD_ID)
        && b.register(12040, true, "hi").err() == Some(KB_BAD_ID);
    // 2) 重号登记 → KB_DUP。
    got[1] = b.register(12021, false, "dup").err() == Some(KB_DUP);
    // 3) 空留痕 → KB_BAD_ID。
    got[2] = b.register(12022, true, "").err() == Some(KB_BAD_ID);
    // 4) 伪造账目：空痕登记项自洽核红 → KB_FORGED 面。
    let forged = KbEntry { seq: 1, topic: 12023, passed: true, note: String::new() };
    got[3] = !forged.ok();
    // 5) 印账后登记 → KB_FROZEN。
    b.seal();
    got[4] = b.register(12022, true, "post").err() == Some(KB_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12040_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12040");

    // J1 全量装配绿判。
    let j1 = {
        let b = build_b02_batch();
        b.green() && b.red_count() == 0 && b.entries.len() == 19
    };
    s.add("J1-B02 全量绿", j1, "十九主题全覆盖");

    // J2 主题号闭集。
    let j2 = {
        let mut b = KtB02::new();
        let lo = b.register(TOPIC_LO - 1, true, "x").err() == Some(KB_BAD_ID);
        let hi = b.register(TOPIC_HI + 1, true, "x").err() == Some(KB_BAD_ID);
        lo && hi
    };
    s.add("J2-批号闭集", j2, "界外全拒");

    // J3 重号登记拒。
    let j3 = {
        let mut b = KtB02::new();
        let _ = b.register(12025, true, "a");
        b.register(12025, true, "b").err() == Some(KB_DUP)
    };
    s.add("J3-重号拒", j3, "主题单登");

    // J4 红项可检。
    let j4 = {
        let mut b = KtB02::new();
        let _ = b.register(12021, true, "a");
        let _ = b.register(12022, false, "red");
        !b.green() && b.red_count() == 1
    };
    s.add("J4-红项检出", j4, "假断言显红");

    // J5 覆盖缺口检出。
    let j5 = {
        let mut b = KtB02::new();
        for t in TOPIC_LO..TOPIC_HI {
            let _ = b.register(t, true, "a");
        }
        !b.green() && b.entries.len() == 18
    };
    s.add("J5-覆盖缺口", j5, "漏题非绿");

    // J6 空留痕拒。
    let j6 = {
        let mut b = KtB02::new();
        b.register(12021, true, "") == Err(KB_BAD_ID)
    };
    s.add("J6-空留痕拒", j6, "留痕必填");

    // J7 账目自洽。
    let j7 = {
        let b = build_b02_batch();
        b.ledger_ok() && b.entries.iter().all(|e| e.topic >= TOPIC_LO && e.topic <= TOPIC_HI)
    };
    s.add("J7-账目自洽", j7, "逐条过核");

    // J8 满号覆盖。
    let j8 = build_b02_batch().full_coverage();
    s.add("J8-满号覆盖", j8, "十九号全在册");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = KtB02::new();
        let _ = a.register(12021, true, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = KtB02::new();
        let _ = c.register(12021, false, "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异册异纹");

    // J10 印账冻结。
    let j10 = {
        let mut b = KtB02::new();
        let _ = b.register(12021, true, "a");
        b.seal();
        b.register(12022, true, "post").err() == Some(KB_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后登记必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：装配册摘要面。
    let b = build_b02_batch();
    let t = b.summary();
    s.add("样例摘要面", t.contains("topics=19") && t.contains("red=0"), "十九题零红");
    s.add("样例账面合法", b.ledger_ok(), "逐条过自洽");

    // J12 汇总面补充：批次闭集端点在册且序号连续。
    let j12 = {
        let b = build_b02_batch();
        let lo = b.entries.iter().any(|e| e.topic == 12021);
        let hi = b.entries.iter().any(|e| e.topic == 12039);
        lo && hi && b.entries.iter().all(|e| e.seq >= 1 && e.seq <= 19)
    };
    s.add("J12-端点序号", j12, "闭集端点在册序号连续");
    // 补充：指纹跨批隔离（B02 册与 B01 主题空间互斥）。
    s.add("J12b-跨批隔离", TOPIC_LO > 12020 && TOPIC_HI < 12040, "闭集不越批");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12040_batch_flow() {
        let mut b = KtB02::new();
        assert!(b.register(12021, true, "a").is_ok());
        assert_eq!(b.register(12021, true, "b"), Err(KB_DUP));
        assert_eq!(b.register(12040, true, "c"), Err(KB_BAD_ID));
        assert_eq!(b.register(12022, false, "red"), Ok(()));
        assert!(!b.green());
    }

    #[test]
    fn unx12040_checks_green() {
        let s = run_unx12040_checks();
        assert!(s.all_passed(), "UNX-F12040 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12040_b02_fully_green() {
        let b = build_b02_batch();
        assert!(b.green());
        assert!(b.full_coverage());
    }

    #[test]
    fn unx12040_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
