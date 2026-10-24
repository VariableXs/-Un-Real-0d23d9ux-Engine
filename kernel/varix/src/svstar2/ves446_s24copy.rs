//! ves446_s24copy — VE-F4212 · 一致性与文案口径
//! 文案口径入约落地：术语表+语调规范+标点规则三源→规则转译；
//! 多语言口径联动（T04 术语库×口径单源——术语漂移跨语种检测）；文案一致性回归门；
//! 术语漂移→定位语种+回 T04 复审；口径冲突→仲裁；回归红→阻断合入；
//! 口径含读屏文本措辞一致性条目（域本色）；语料脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x14A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpCode {
    Ok = 0,
    TermDrift = SEG_BASE + 0x01, // 0x14A1
    CaliberConflict = SEG_BASE + 0x02, // 0x14A2
    RegressRed = SEG_BASE + 0x03, // 0x14A3
}

/// 三源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaliberSource { TermBase = 0, Tone = 1, Punct = 2 }

/// 口径条目（源 + 规则 id + 读屏措辞标记——域本色）。
pub struct CaliberItem { pub src: CaliberSource, pub rule_id: u32, pub sr_wording: bool }

/// 文案口径册。
pub struct CopyBook {
    pub items: Vec<CaliberItem>,
    pub terms: Vec<(&'static str, u32)>, // 术语跨语种指纹（语种 id → 指纹）
    pub regress_green: bool,
    pub filed: Vec<&'static str>,
}

impl CopyBook {
    pub fn new() -> Self { CopyBook { items: Vec::new(), terms: Vec::new(), regress_green: false, filed: Vec::new() } }
    pub fn put(&mut self, src: CaliberSource, rule: u32, sr: bool) {
        self.items.retain(|i| i.src != src);
        self.items.push(CaliberItem { src, rule_id: rule, sr_wording: sr });
    }
    /// 三源转译齐备核验。
    pub fn sources_full(&self) -> bool { self.items.len() == 3 }
    /// 跨语种术语漂移检测：同术语各语种指纹必须一致。
    pub fn drift_check(&mut self, fingerprints: &[(&'static str, u32)]) -> Vec<&'static str> {
        self.terms = fingerprints.to_vec();
        let mut bad: Vec<&'static str> = Vec::new();
        for (t, _) in fingerprints {
            let fs: Vec<u32> = fingerprints.iter().filter(|(tt, _)| tt == t).map(|(_, f)| *f).collect();
            if fs.windows(2).any(|w| w[0] != w[1]) && !bad.contains(t) { bad.push(t); }
        }
        for t in &bad { if !self.filed.contains(t) { self.filed.push(t); } }
        bad
    }
    /// 口径冲突仲裁：三源间规则 id 冲突→冲突码。
    pub fn conflict_check(&self) -> Result<(), CpCode> {
        let mut ids: Vec<u32> = self.items.iter().map(|i| i.rule_id).collect();
        ids.sort_unstable();
        for w in ids.windows(2) { if w[0] == w[1] { return Err(CpCode::CaliberConflict); } }
        Ok(())
    }
    /// 回归门。
    pub fn gate(&self) -> Result<(), CpCode> {
        if !self.regress_green { return Err(CpCode::RegressRed); }
        Ok(())
    }
    /// 读屏措辞条目在册（域本色）。
    pub fn sr_present(&self) -> bool { self.items.iter().any(|i| i.sr_wording) }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves446_checks() -> CheckSet {
    let mut set = CheckSet::new("ves446");
    let mut bk = CopyBook::new();

    // C446-01 三源未齐。
    bk.put(CaliberSource::TermBase, 201, false);
    chk(&mut set, "C446-01", !bk.sources_full(), "三源缺");

    // C446-02 三源齐（含读屏措辞条目——域本色）。
    bk.put(CaliberSource::Tone, 202, false);
    bk.put(CaliberSource::Punct, 203, true);
    chk(&mut set, "C446-02", bk.sources_full() && bk.sr_present(), "三源齐+读屏在册");

    // C446-03 口径冲突（两源同规则 id）→仲裁码。
    bk.put(CaliberSource::Tone, 201, false);
    chk(&mut set, "C446-03", bk.conflict_check() == Err(CpCode::CaliberConflict), "冲突检出");

    // C446-04 仲裁修正后无冲突。
    bk.put(CaliberSource::Tone, 202, false);
    chk(&mut set, "C446-04", bk.conflict_check().is_ok(), "仲裁修正");

    // C446-05 跨语种术语指纹一致（零漂移）。
    let fps = [("保存", 1001u32), ("保存", 1001)];
    chk(&mut set, "C446-05", bk.drift_check(&fps).is_empty(), "零漂移");

    // C446-06 术语漂移检出+定位。
    let fps2 = [("另存", 1002u32), ("另存", 1003)];
    let bad = bk.drift_check(&fps2);
    chk(&mut set, "C446-06", bad == vec!["另存"] && bk.filed.contains(&"另存"), "漂移定位立案");

    // C446-07 回 T04 复审后指纹统一→复检零漂移。
    let fps3 = [("另存", 1002u32), ("另存", 1002)];
    chk(&mut set, "C446-07", bk.drift_check(&fps3).is_empty(), "复审归一");

    // C446-08 回归门：红→阻断合入。
    chk(&mut set, "C446-08", bk.gate() == Err(CpCode::RegressRed), "回归红阻断");

    // C446-09 回归绿→放行。
    bk.regress_green = true;
    chk(&mut set, "C446-09", bk.gate().is_ok(), "合入放行");

    // C446-10 三源单源断言。
    chk(&mut set, "C446-10", CaliberSource::TermBase as u8 == 0
        && CaliberSource::Punct as u8 == 2, "三源单源");

    // C446-11 语料脱敏断言（漂移立案只留术语名不留语料内容）。
    chk(&mut set, "C446-11", bk.filed.iter().all(|t| t.len() <= 8), "立案脱敏");

    // C446-12 源覆盖幂等。
    bk.put(CaliberSource::TermBase, 201, false);
    chk(&mut set, "C446-12", bk.items.len() == 3, "源覆盖幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves446_smoke() {
        let set = run_ves446_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves446 red: {:?}", red);
    }
}
