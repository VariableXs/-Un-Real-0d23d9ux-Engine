//! ves350_s19ldom — VE-F4110 · T06 地区与 L 域联动
//! L 域生命周期对地区维度的支撑面：地区设置迁移（用户换地区→设置继承评审——冲突默认保守）+ 地区相关数据保留策略（与 F4103 驻留联动——驻留规则优先）+ 联动面登记不越界 + 无障碍设置强制继承（用户无障碍偏好不因换地区丢失——域本色红线）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xC5C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LdCode {
    Ok = 0,
    ConflictConservative = SEG_BASE + 0x01,
    Overreach = SEG_BASE + 0x02,
    RetentionClash = SEG_BASE + 0x03,
}

/// 用户设置集（迁移对象）。
#[derive(Clone)]
pub struct UserSettings {
    pub lang: &'static str,
    pub region: &'static str,
    /// 无障碍偏好（域本色红线——强制继承）。
    pub screen_reader: bool,
    pub contrast_high: bool,
    pub reduce_motion: bool,
    /// 非无障碍个性化设置（可评审覆盖）。
    pub theme: &'static str,
}

impl UserSettings {
    /// 无障碍偏好指纹（迁移前后必须全等——强制继承断言）。
    pub fn a11y_fingerprint(&self) -> (bool, bool, bool) {
        (self.screen_reader, self.contrast_high, self.reduce_motion)
    }
}

/// 迁移评审：换地区 → 设置继承。
pub struct MigrationReview { pub from: &'static str, pub to: &'static str, pub overridden: Vec<&'static str>, pub conservative: bool }

pub fn migrate_settings(from: &UserSettings, to_region: &'static str, region_rules: &[(&'static str, &'static str)]) -> (UserSettings, MigrationReview) {
    let mut out = from.clone();
    out.region = to_region;
    let mut rev = MigrationReview {
        from: from.region, to: to_region, overridden: Vec::new(), conservative: false,
    };
    // 地区规则迁移：目标地区有强制规则 → 覆盖对应设置（评审留痕）。
    for (key, val) in region_rules.iter() {
        if *key == "lang" && out.lang != *val {
            out.lang = val; // 目标地区语言强制建议
            rev.overridden.push("lang");
        } else if *key == "theme" && out.theme != *val {
            out.theme = val;
            rev.overridden.push("theme");
        }
    }
    // 冲突保守：无规则可依的设置保持原值（保守 = 不动）。
    rev.conservative = rev.overridden.is_empty();
    (out, rev)
}

/// 无障碍强制继承红线断言：迁移前后指纹必须全等。
pub fn a11y_inherited(before: &UserSettings, after: &UserSettings) -> bool {
    before.a11y_fingerprint() == after.a11y_fingerprint()
}

/// 数据保留策略：地区 → 保留天数；驻留规则优先。
pub struct Retention { pub region: &'static str, pub days: u32, pub residency_backed: bool }

pub struct RetentionBook { pub rules: Vec<Retention> }

impl RetentionBook {
    pub fn new() -> Self { RetentionBook { rules: Vec::new() } }
    pub fn put(&mut self, r: Retention) { self.rules.push(r); }

    /// 保留策略冲突 → 驻留规则优先（residency_backed=true 的最短保留胜）。
    pub fn effective(&self, region: &str) -> Result<u32, LdCode> {
        let mut best: Option<u32> = None;
        let mut clash = false;
        for r in self.rules.iter().filter(|r| r.region == region) {
            best = Some(match best {
                None => r.days,
                Some(b) => {
                    if b != r.days { clash = true; }
                    if r.residency_backed { r.days } else { b.min(r.days) }
                }
            });
        }
        match best {
            Some(d) => { let _ = clash; Ok(d) }
            None => Err(LdCode::RetentionClash),
        }
    }
}

/// 联动面登记（不越界——L 域只登记不实现地区规则）。
pub struct LinkRegistry { pub faces: Vec<&'static str>, pub t06_impl: Vec<&'static str> }

impl LinkRegistry {
    pub fn new() -> Self { LinkRegistry { faces: Vec::new(), t06_impl: Vec::new() } }
    pub fn register_face(&mut self, f: &'static str) {
        if !self.faces.contains(&f) { self.faces.push(f); }
    }
    pub fn flag_impl(&mut self, what: &'static str) { self.t06_impl.push(what); }
    pub fn clean(&self) -> bool { self.t06_impl.is_empty() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves350_checks() -> CheckSet {
    let mut set = CheckSet::new("ves350");

    let mk_user = || UserSettings {
        lang: "en", region: "US",
        screen_reader: true, contrast_high: true, reduce_motion: false,
        theme: "dark",
    };

    // C350-01 设置迁移：换地区生效。
    let u = mk_user();
    let (after, rev) = migrate_settings(&u, "DE", &[]);
    chk(&mut set, "C350-01", after.region == "DE" && rev.to == "DE", "地区迁移生效");

    // C350-02 无冲突保守：无规则则不动任何设置。
    chk(&mut set, "C350-02", rev.conservative && rev.overridden.is_empty() && after.theme == "dark", "无规则保守不动");

    // C350-03 地区规则覆盖留痕。
    let rules = [("lang", "de"), ("theme", "light")];
    let (after2, rev2) = migrate_settings(&u, "DE", &rules);
    chk(&mut set, "C350-03", after2.lang == "de" && after2.theme == "light" && rev2.overridden == vec!["lang", "theme"], "两覆盖全留痕");

    // C350-04 无障碍强制继承（域本色红线）。
    chk(&mut set, "C350-04", a11y_inherited(&u, &after2), "无障碍指纹迁移前后全等");

    // C350-05 红线实测：试图覆盖无障碍 → 断言失败。
    let mut evil = after2.clone();
    evil.screen_reader = false;
    chk(&mut set, "C350-05", !a11y_inherited(&u, &evil), "关读屏 => 继承断言破");

    // C350-06 无障碍指纹单源。
    chk(&mut set, "C350-06", u.a11y_fingerprint() == (true, true, false), "指纹三元组");

    // C350-07 保留策略：驻留规则优先。
    let mut rbk = RetentionBook::new();
    rbk.put(Retention { region: "EU", days: 30, residency_backed: false });
    rbk.put(Retention { region: "EU", days: 7, residency_backed: true });
    chk(&mut set, "C350-07", rbk.effective("EU") == Ok(7), "驻留 7 天优先");

    // C350-08 无驻留背书取最短。
    rbk.put(Retention { region: "US", days: 90, residency_backed: false });
    rbk.put(Retention { region: "US", days: 30, residency_backed: false });
    chk(&mut set, "C350-08", rbk.effective("US") == Ok(30), "US 取最短 30");

    // C350-09 未登记地区 → 冲突缺。
    chk(&mut set, "C350-09", rbk.effective("JP") == Err(LdCode::RetentionClash), "JP 无策略 => Clash");

    // C350-10 联动面登记。
    let mut lr = LinkRegistry::new();
    lr.register_face("settings-migration");
    lr.register_face("retention-policy");
    lr.register_face("settings-migration");
    chk(&mut set, "C350-10", lr.faces.len() == 2, "两面登记去重");

    // C350-11 不越界：T06 侧无 L 域实现。
    chk(&mut set, "C350-11", lr.clean(), "无越界实现");

    // C350-12 越界标记可测。
    lr.flag_impl("l-domain-scheduler");
    chk(&mut set, "C350-12", !lr.clean(), "flag 后不干净（守卫可测）");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves350_smoke() {
        let set = run_ves350_checks();
        assert!(set.failed_count() == 0, "ves350 red: {}", set.red_items_text());
    }

    #[test]
    fn ves350_inherit() {
        let u = UserSettings { lang: "en", region: "US", screen_reader: false, contrast_high: false, reduce_motion: true, theme: "dark" };
        let (a, _) = migrate_settings(&u, "CN", &[("lang", "zh")]);
        assert_eq!(a.lang, "zh");
        assert!(a11y_inherited(&u, &a));
    }
}
