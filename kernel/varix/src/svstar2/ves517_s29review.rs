//! VE-F4283 · 一致性×代码审查（U05 工具集成组）
//! 职责：审查助手提交时自动附一致性画像三卡（违例预判/历史热点/相似判例）
//! + 评论代拟纪律（机器草拟人改发——不代发不代评）+ 发送前脱敏扫描
//! + 画像误判置信度标注 + 负担分档防过载（过载建议拆分提交）。
//! 段位：0x29E0（续表 idx517）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x29E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RvCode {
    /// 代拟泄漏敏感串（发送前脱敏拦截）。
    LeakHit = SEG_BASE,
    /// 负担过载（建议拆分提交）。
    Overload = SEG_BASE + 0x01,
    /// 画像误判无置信度。
    NoConfidence = SEG_BASE + 0x02,
    /// 代发违规（纪律红线）。
    AutoSend = SEG_BASE + 0x03,
}

/// 画像三卡。
#[derive(Clone, Debug)]
pub struct Profile {
    pub predicts: Vec<u16>,      // 违例预判
    pub hotspots: Vec<u16>,      // 历史热点
    pub similar: Vec<u32>,       // 相似判例
    pub confidence_bp: u16,
}

impl Profile {
    pub fn card_count(&self) -> usize {
        let mut n = 0;
        if !self.predicts.is_empty() { n += 1; }
        if !self.hotspots.is_empty() { n += 1; }
        if !self.similar.is_empty() { n += 1; }
        n
    }
    pub fn burden(&self) -> u32 {
        (self.predicts.len() + self.hotspots.len() + self.similar.len()) as u32
    }
}

/// 审查助手。
pub struct ReviewAssist {
    /// 草拟评论池（人改发）。
    pub drafts: Vec<String>,
    /// 敏感串黑名单。
    pub sensitive: Vec<String>,
    /// 负担分档阈值（信息量上限）。
    pub burden_cap: u32,
    pub sent_by_human: u32,
    pub auto_send_blocked: u32,
}

impl ReviewAssist {
    pub fn new(burden_cap: u32) -> Self {
        ReviewAssist { drafts: Vec::new(), sensitive: Vec::new(), burden_cap, sent_by_human: 0, auto_send_blocked: 0 }
    }

    /// 脱敏扫描：命中敏感串→拦截。
    pub fn desensitize_scan(&mut self, text: &str) -> bool {
        let hit = self.sensitive.iter().any(|s| text.contains(s.as_str()));
        hit
    }

    /// 代拟：草拟入库（纪律：人不改不发）。
    pub fn draft(&mut self, text: &str) -> Result<(), RvCode> {
        if self.desensitize_scan(text) {
            return Err(RvCode::LeakHit);
        }
        self.drafts.push(String::from(text));
        Ok(())
    }

    /// 发送：只允许人改发（auto=true 拒绝）。
    pub fn send(&mut self, auto: bool) -> Result<(), RvCode> {
        if auto {
            self.auto_send_blocked += 1;
            return Err(RvCode::AutoSend); // 不代发不代评
        }
        if self.drafts.is_empty() {
            return Ok(());
        }
        self.drafts.remove(0);
        self.sent_by_human += 1;
        Ok(())
    }

    /// 负担分档：0 轻 1 中 2 过载（建议拆分提交）。
    pub fn burden_tier(&self, p: &Profile) -> u8 {
        let b = p.burden();
        if b > self.burden_cap {
            2
        } else if b * 2 > self.burden_cap {
            1
        } else {
            0
        }
    }

    /// 画像完整性：非空画像必须有置信度。
    pub fn profile_ok(&self, p: &Profile) -> Result<(), RvCode> {
        if p.card_count() > 0 && p.confidence_bp == 0 {
            return Err(RvCode::NoConfidence);
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C517-01..12。
pub fn run_ves517_checks() -> CheckSet {
    let mut set = CheckSet::new("ves517");

    let mut ra = ReviewAssist::new(6);
    ra.sensitive.push(String::from("SECRET"));

    // 画像三卡
    let p = Profile { predicts: alloc::vec![10, 20], hotspots: alloc::vec![30], similar: alloc::vec![99], confidence_bp: 8_000 };
    chk(&mut set, "C517-01", p.card_count() == 3, "画像三卡齐（预判/热点/判例）");
    chk(&mut set, "C517-02", ra.profile_ok(&p).is_ok(), "置信度标注在册");
    let p0 = Profile { predicts: alloc::vec![1], hotspots: Vec::new(), similar: Vec::new(), confidence_bp: 0 };
    chk(&mut set, "C517-03", ra.profile_ok(&p0) == Err(RvCode::NoConfidence), "有卡无置信度拒绝");

    // 负担分档
    chk(&mut set, "C517-04", ra.burden_tier(&p) == 1, "负担 4 → 中档（4*2>6? 否 → 轻档）");
    let heavy = Profile { predicts: alloc::vec![1, 2, 3, 4], hotspots: alloc::vec![5, 6], similar: alloc::vec![7, 8], confidence_bp: 9_000 };
    chk(&mut set, "C517-05", ra.burden_tier(&heavy) == 2, "负担 8 → 过载（建议拆分提交）");
    let light = Profile { predicts: alloc::vec![1], hotspots: Vec::new(), similar: Vec::new(), confidence_bp: 1 };
    chk(&mut set, "C517-06", ra.burden_tier(&light) == 0, "负担 1 → 轻档");

    // 代拟纪律
    chk(&mut set, "C517-07", ra.draft("建议对齐令牌 bp-4").is_ok(), "草拟入库");
    chk(&mut set, "C517-08", ra.draft("key=SECRET") == Err(RvCode::LeakHit), "敏感串发送前拦截");
    chk(&mut set, "C517-09", ra.drafts.len() == 1, "拦截不入草稿池");

    chk(&mut set, "C517-10", ra.send(true) == Err(RvCode::AutoSend), "机器代发拒绝（不代发不代评）");
    chk(&mut set, "C517-11", ra.send(false).is_ok() && ra.sent_by_human == 1, "人改发成功");
    chk(&mut set, "C517-12", ra.auto_send_blocked == 1 && ra.drafts.is_empty(), "代发拦截计数与池清空自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_assist() {
        let mut ra = ReviewAssist::new(10);
        ra.sensitive.push(String::from("pwd"));
        assert!(ra.draft("ok").is_ok());
        assert_eq!(ra.draft("pwd"), Err(RvCode::LeakHit));
        assert_eq!(ra.send(true), Err(RvCode::AutoSend));
        assert!(ra.send(false).is_ok());
    }
}
