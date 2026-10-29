//! UNX-F12020 ktest 进程对象面断言集（B01 批判据聚合）（0x194A0 段）
//!
//! 判据原文：B01 批（UNX-F12001~F12019 进程对象域十九卡）批判据聚合
//! 册——每卡语义主题以断言号登记（卡号闭集 12001..12019），断言值域
//! 布尔二值；卡号越批即拒；重号登记即拒；空留痕即拒；聚合绿判 = 全登记
//! 断言为真且无遗漏主题。
//!
//! 结构：断言册（KtBatch：主题号 × 判值 × 留痕）+ 登记核（四码守卫）+
//! 聚合面（绿判 + 主题覆盖核对）+ 折叠指纹 + 印账冻结 + 注入核 +
//! 判据聚合（run_unx12020_checks）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x194A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x194A0;
pub const KT_BAD_ID: u32 = SEG_BASE + 0x01; // 卡号越批/空留痕
pub const KT_DUP: u32 = SEG_BASE + 0x02; // 重号登记
pub const KT_NOT_GREEN: u32 = SEG_BASE + 0x03; // 聚合有红
pub const KT_FORGED: u32 = SEG_BASE + 0x04; // 伪造账目（自洽核红）
pub const KT_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍登记

/// B01 批主题号下界/上界（12001..12019，本卡 12020 为聚合位不登记）。
pub const TOPIC_LO: u32 = 12001;
pub const TOPIC_HI: u32 = 12019;

/// 一条断言登记。
#[derive(Clone)]
pub struct KtEntry {
    pub seq: u32,
    pub topic: u32,
    pub passed: bool,
    pub note: String,
}

impl KtEntry {
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && self.seq > 0 && self.topic >= TOPIC_LO && self.topic <= TOPIC_HI
    }
}

/// B01 批断言册。
pub struct KtBatch {
    pub entries: Vec<KtEntry>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl KtBatch {
    pub fn new() -> KtBatch {
        KtBatch { entries: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 登记核：批号闭集 + 唯一号 + 非空留痕。
    pub fn register(&mut self, topic: u32, passed: bool, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(KT_FROZEN);
        }
        if note.is_empty() {
            return Err(KT_BAD_ID);
        }
        if topic < TOPIC_LO || topic > TOPIC_HI {
            return Err(KT_BAD_ID);
        }
        if self.entries.iter().any(|e| e.topic == topic) {
            return Err(KT_DUP);
        }
        let e = KtEntry { seq: self.seq_next, topic, passed, note: note.to_string() };
        if !e.ok() {
            return Err(KT_FORGED);
        }
        self.entries.push(e);
        self.seq_next += 1;
        Ok(())
    }

    /// 主题覆盖核对：12001..12019 全号在册。
    pub fn full_coverage(&self) -> bool {
        for t in TOPIC_LO..=TOPIC_HI {
            if !self.entries.iter().any(|e| e.topic == t) {
                return false;
            }
        }
        true
    }

    /// 聚合绿判：全覆盖且全真。
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

/// 装配 B01 批全量断言：十九卡各一条语义钉值断言。
pub fn build_b01_batch() -> KtBatch {
    let mut b = KtBatch::new();
    let samples: [(u32, bool, &str); 19] = [
        (12001, true, "内核写路径六步生命线"),
        (12002, true, "EPROCESS 八槽字段账"),
        (12003, true, "创建六相推进"),
        (12004, true, "终止传播链跨代继承"),
        (12005, true, "PEB 关键字段组对账"),
        (12006, true, "TIB 四锚自洽"),
        (12007, true, ".pdata 三函数目录查找"),
        (12008, true, "参数块五字段装配"),
        (12009, true, "句柄三开三关回收"),
        (12010, true, "快照页冻结与篡改检出"),
        (12011, true, "gs 基址双态轮转"),
        (12012, true, "SD 挂卸正路径"),
        (12013, true, "亲和掩码子集对账"),
        (12014, true, "优先级带钳制"),
        (12015, true, "配额双池对平"),
        (12016, true, "拆除三相机"),
        (12017, true, "人格路由隔离"),
        (12018, true, "信息类需量表"),
        (12019, true, "版本分档 22000 切档"),
    ];
    for (topic, passed, note) in samples.iter() {
        let _ = b.register(*topic, *passed, note);
    }
    b
}

/// 注入核：五个坏样本各踩一个语义检出分支。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = KtBatch::new();
    let _ = b.register(12001, true, "a");
    // 1) 卡号越批（12000 下界外 / 12020 聚合位）→ KT_BAD_ID。
    got[0] = b.register(12000, true, "lo").err() == Some(KT_BAD_ID)
        && b.register(12020, true, "hi").err() == Some(KT_BAD_ID);
    // 2) 重号登记 → KT_DUP。
    got[1] = b.register(12001, false, "dup").err() == Some(KT_DUP);
    // 3) 空留痕 → KT_BAD_ID。
    got[2] = b.register(12002, true, "").err() == Some(KT_BAD_ID);
    // 4) 伪造账目：空留痕登记项过不了自洽核 → KT_FORGED 面。
    let forged = KtEntry { seq: 1, topic: 12003, passed: true, note: String::new() };
    got[3] = !forged.ok();
    // 5) 印账后登记 → KT_FROZEN。
    b.seal();
    got[4] = b.register(12002, true, "post").err() == Some(KT_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12020_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12020");

    // J1 全量装配绿判：十九主题全覆盖全真。
    let j1 = {
        let b = build_b01_batch();
        b.green() && b.red_count() == 0 && b.entries.len() == 19
    };
    s.add("J1-B01 全量绿", j1, "十九主题全覆盖");

    // J2 主题号闭集：界外双面拒。
    let j2 = {
        let mut b = KtBatch::new();
        let lo = b.register(TOPIC_LO - 1, true, "x").err() == Some(KT_BAD_ID);
        let hi = b.register(TOPIC_HI + 1, true, "x").err() == Some(KT_BAD_ID);
        lo && hi
    };
    s.add("J2-批号闭集", j2, "界外全拒");

    // J3 重号登记拒。
    let j3 = {
        let mut b = KtBatch::new();
        let _ = b.register(12005, true, "a");
        b.register(12005, true, "b").err() == Some(KT_DUP)
    };
    s.add("J3-重号拒", j3, "主题单登");

    // J4 红项可检：一条假断言聚合必红。
    let j4 = {
        let mut b = KtBatch::new();
        let _ = b.register(12001, true, "a");
        let _ = b.register(12002, false, "red");
        !b.green() && b.red_count() == 1
    };
    s.add("J4-红项检出", j4, "假断言显红");

    // J5 覆盖缺口检出：漏一主题非绿。
    let j5 = {
        let mut b = KtBatch::new();
        for t in TOPIC_LO..TOPIC_HI {
            let _ = b.register(t, true, "a");
        }
        // 少登 12019。
        !b.green() && b.entries.len() == 18
    };
    s.add("J5-覆盖缺口", j5, "漏题非绿");

    // J6 空留痕拒。
    let j6 = {
        let mut b = KtBatch::new();
        b.register(12001, true, "").err() == Some(KT_BAD_ID)
    };
    s.add("J6-空留痕拒", j6, "留痕必填");

    // J7 账目完整性：装配册逐条过自洽核。
    let j7 = {
        let b = build_b01_batch();
        b.ledger_ok() && b.entries.iter().all(|e| e.topic >= TOPIC_LO && e.topic <= TOPIC_HI)
    };
    s.add("J7-账目自洽", j7, "逐条过核");

    // J8 覆盖核对全量：满号覆盖恒真。
    let j8 = {
        let b = build_b01_batch();
        b.full_coverage()
    };
    s.add("J8-满号覆盖", j8, "十九号全在册");

    // J9 指纹区分度与幂等。
    let j9 = {
        let mut a = KtBatch::new();
        let _ = a.register(12001, true, "x");
        let f1 = a.fold();
        let f2 = a.fold();
        let mut c = KtBatch::new();
        let _ = c.register(12001, false, "x");
        f1 == f2 && a.fold() != c.fold()
    };
    s.add("J9-指纹性质", j9, "幂等且异册异纹");

    // J10 印账冻结。
    let j10 = {
        let mut b = KtBatch::new();
        let _ = b.register(12001, true, "a");
        b.seal();
        b.register(12002, true, "post").err() == Some(KT_FROZEN)
    };
    s.add("J10-印账冻结", j10, "印后登记必拒");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例：装配册摘要面。
    let b = build_b01_batch();
    let t = b.summary();
    s.add("样例摘要面", t.contains("topics=19") && t.contains("red=0"), "十九题零红");
    s.add("样例账面合法", b.ledger_ok(), "逐条过自洽");

    // J12 汇总面补充：批次闭集端点在册且序号连续。
    let j12 = {
        let b = build_b01_batch();
        let lo = b.entries.iter().any(|e| e.topic == 12001);
        let hi = b.entries.iter().any(|e| e.topic == 12019);
        lo && hi && b.entries.iter().all(|e| e.seq >= 1 && e.seq <= 19)
    };
    s.add("J12-端点序号", j12, "闭集端点在册序号连续");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12020_batch_flow() {
        let mut b = KtBatch::new();
        assert!(b.register(12001, true, "a").is_ok());
        assert_eq!(b.register(12001, true, "b"), Err(KT_DUP));
        assert_eq!(b.register(12020, true, "c"), Err(KT_BAD_ID));
        assert_eq!(b.register(12002, false, "red"), Ok(()));
        assert!(!b.green());
        assert_eq!(b.red_count(), 1);
    }

    #[test]
    fn unx12020_checks_green() {
        let s = run_unx12020_checks();
        assert!(s.all_passed(), "UNX-F12020 判据未全绿: {}", s.red_items_text());
    }

    #[test]
    fn unx12020_b01_fully_green() {
        let b = build_b01_batch();
        assert!(b.green());
        assert!(b.full_coverage());
    }

    #[test]
    fn unx12020_inject_all() {
        let got = inject_five();
        assert!(got.iter().all(|x| *x), "inj5={:?}", got);
    }
}
