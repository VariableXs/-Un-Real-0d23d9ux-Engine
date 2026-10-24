//! ves427_s23obsfinal — VE-F4187 · T 域体验日志与观测终审
//! 终审：T 域观测终审——体验日志埋点口径冻结（国际化事件四类埋点：语种切换/翻译缺失回退/地区格式变更/方向切换）
//! +观测面板项终版+留痕采样率复核；
//! 口径漂移→对版修正；面板缺项→补齐；采样失真→重校；
//! 埋点不含可识别用户内容（隐私红线复述）；采样脱敏。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x11A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObCode {
    Ok = 0,
    CaliberDrift = SEG_BASE + 0x01, // 0x11A1
    PanelMissing = SEG_BASE + 0x02, // 0x11A2
    SampleSkew = SEG_BASE + 0x03,   // 0x11A3
    UserContent = SEG_BASE + 0x04,  // 0x11A4 隐私红线
}

/// 四类埋点（域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind { LangSwitch = 0, MtFallback = 1, RegionFormat = 2, DirSwitch = 3 }

impl EventKind {
    pub const ALL: [EventKind; 4] = [
        EventKind::LangSwitch, EventKind::MtFallback, EventKind::RegionFormat, EventKind::DirSwitch];
    pub fn name(self) -> &'static str {
        match self {
            EventKind::LangSwitch => "lang-switch", EventKind::MtFallback => "mt-fallback",
            EventKind::RegionFormat => "region-format", EventKind::DirSwitch => "dir-switch",
        }
    }
}

/// 埋点口径条目：事件类 + 口径版本 + 是否含用户内容（必须否）+ 采样率（bp）。
pub struct CaliberEntry { pub kind: EventKind, pub caliber_ver: u32, pub has_user_content: bool, pub sample_bp: u32 }

/// 观测终审册。
pub struct ObsFinalBook {
    pub calibers: Vec<CaliberEntry>,
    pub frozen_ver: u32,
    pub panel: Vec<EventKind>, // 面板项终版
}

impl ObsFinalBook {
    pub fn new() -> Self { ObsFinalBook { calibers: Vec::new(), frozen_ver: 0, panel: Vec::new() } }
    pub fn put(&mut self, kind: EventKind, ver: u32, uc: bool, bp: u32) {
        self.calibers.retain(|c| c.kind != kind);
        self.calibers.push(CaliberEntry { kind, caliber_ver: ver, has_user_content: uc, sample_bp: bp });
    }
    pub fn caliber_ver(&self, k: EventKind) -> u32 {
        self.calibers.iter().find(|c| c.kind == k).map(|c| c.caliber_ver).unwrap_or(0)
    }
    /// 口径冻结：四类齐+全部不含用户内容+口径版本一致才可冻。
    pub fn freeze(&mut self, ver: u32) -> Result<(), ObCode> {
        for k in EventKind::ALL {
            let c = match self.calibers.iter().find(|c| c.kind == k) {
                Some(c) => c, None => return Err(ObCode::CaliberDrift),
            };
            if c.has_user_content { return Err(ObCode::UserContent); }
            if c.caliber_ver != ver { return Err(ObCode::CaliberDrift); }
        }
        self.frozen_ver = ver;
        Ok(())
    }
    /// 面板缺项检查。
    pub fn panel_gaps(&self) -> Vec<EventKind> {
        EventKind::ALL.iter().filter(|k| !self.panel.contains(k)).map(|k| *k).collect()
    }
    /// 采样率复核：每类须在 [MIN_BP, MAX_BP] 内，失真→重校码。
    pub fn sample_recheck(&self) -> Result<(), ObCode> {
        for c in &self.calibers {
            if c.sample_bp < 100 || c.sample_bp > 10_000 { return Err(ObCode::SampleSkew); }
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves427_checks() -> CheckSet {
    let mut set = CheckSet::new("ves427");
    let mut bk = ObsFinalBook::new();

    // C427-01 四类齐前冻结拒（口径漂移码）。
    bk.put(EventKind::LangSwitch, 3, false, 1_000);
    chk(&mut set, "C427-01", bk.freeze(3) == Err(ObCode::CaliberDrift), "口径不齐拒冻");

    // C427-02 埋点含用户内容→隐私红线码（域本色红线复述）。
    bk.put(EventKind::MtFallback, 3, true, 1_000);
    chk(&mut set, "C427-02", bk.freeze(3) == Err(ObCode::UserContent), "用户内容红线");

    // C427-03 修正为不含用户内容。
    bk.put(EventKind::MtFallback, 3, false, 1_000);

    // C427-04 口径版本漂移拒。
    bk.put(EventKind::RegionFormat, 2, false, 500);
    chk(&mut set, "C427-04", bk.freeze(3) == Err(ObCode::CaliberDrift), "版本漂移拒");

    // C427-05 对版修正后四类齐冻。
    bk.put(EventKind::RegionFormat, 3, false, 500);
    bk.put(EventKind::DirSwitch, 3, false, 2_000);
    chk(&mut set, "C427-05", bk.freeze(3).is_ok() && bk.frozen_ver == 3, "四类齐冻结");

    // C427-06 面板缺项定位。
    bk.panel.push(EventKind::LangSwitch);
    let gaps = bk.panel_gaps();
    chk(&mut set, "C427-06", gaps.len() == 3 && gaps.contains(&EventKind::DirSwitch), "面板缺项定位");

    // C427-07 补齐面板项终版。
    bk.panel.push(EventKind::MtFallback);
    bk.panel.push(EventKind::RegionFormat);
    bk.panel.push(EventKind::DirSwitch);
    chk(&mut set, "C427-07", bk.panel_gaps().is_empty() && bk.panel.len() == 4, "面板终版齐");

    // C427-08 采样率失真（过低）→重校。
    bk.put(EventKind::LangSwitch, 3, false, 50);
    chk(&mut set, "C427-08", bk.sample_recheck() == Err(ObCode::SampleSkew), "采样过低重校");

    // C427-09 采样率失真（过高超 100%）→重校。
    bk.put(EventKind::LangSwitch, 3, false, 10_500);
    chk(&mut set, "C427-09", bk.sample_recheck() == Err(ObCode::SampleSkew), "采样超界重校");

    // C427-10 复核后采样带内通过。
    bk.put(EventKind::LangSwitch, 3, false, 1_000);
    chk(&mut set, "C427-10", bk.sample_recheck().is_ok(), "采样带内通过");

    // C427-11 四类埋点名单单源。
    chk(&mut set, "C427-11", EventKind::ALL.len() == 4
        && EventKind::ALL[0].name() == "lang-switch"
        && EventKind::ALL[3].name() == "dir-switch", "四类单源");

    // C427-12 埋点口径覆盖幂等（同事件替换）。
    chk(&mut set, "C427-12", bk.calibers.len() == 4, "口径覆盖幂等");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves427_smoke() {
        let set = run_ves427_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves427 red: {:?}", red);
    }
}
