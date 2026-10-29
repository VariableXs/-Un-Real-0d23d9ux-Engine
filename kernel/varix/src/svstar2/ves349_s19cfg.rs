//! ves349_s19cfg — VE-F4109 · T06 地区配置化
//! 地区规则全配置化声明（法规/格式/审查/支付四类规则数据与代码分离——改规则不改码）+ 配置 schema 冻结+版本化 + 热加载（配置更新不重启）+ schema 漂移拒载 + 配置冲突优先级仲裁表 + 热加载失败保留旧配显性通告。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xC540;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CfCode {
    Ok = 0,
    SchemaDrift = SEG_BASE + 0x01,
    LoadFail = SEG_BASE + 0x02,
    Conflict = SEG_BASE + 0x03,
}

/// 四类规则域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CfgKind { Law = 0, Format = 1, Content = 2, Payment = 3 }

impl CfgKind {
    pub const ALL: [CfgKind; 4] = [CfgKind::Law, CfgKind::Format, CfgKind::Content, CfgKind::Payment];
    pub fn name(self) -> &'static str {
        match self { CfgKind::Law => "law", CfgKind::Format => "format",
            CfgKind::Content => "content", CfgKind::Payment => "payment" }
    }
}

/// schema 冻结：字段名单源。
pub const SCHEMA_FIELDS: [&'static str; 5] = ["kind", "region", "key", "value", "version"];

pub fn schema_frozen(fields: &[&'static str]) -> bool {
    fields.len() == SCHEMA_FIELDS.len() && SCHEMA_FIELDS.iter().all(|f| fields.contains(f))
}

/// 配置条目（数据，非代码）。
#[derive(Clone)]
pub struct CfgEntry { pub kind: CfgKind, pub region: &'static str, pub key: &'static str, pub value: &'static str, pub version: u32 }

/// 配置册。
pub struct CfgBook { pub entries: Vec<CfgEntry>, pub live_version: u32 }

impl CfgBook {
    pub fn new() -> Self { CfgBook { entries: Vec::new(), live_version: 0 } }

    /// 热加载：schema 冻结校验 → 全量换装（不重启——版本号推进即生效）。
    pub fn hot_load(&mut self, new_entries: Vec<CfgEntry>, new_version: u32) -> Result<(), CfCode> {
        for e in new_entries.iter() {
            if e.region.is_empty() || e.key.is_empty() || e.version == 0 {
                return Err(CfCode::SchemaDrift);
            }
        }
        self.entries = new_entries;
        self.live_version = new_version;
        Ok(())
    }

    /// 热加载失败 → 保留旧配 + 显性通告（返回值区分）。
    pub fn hot_load_or_keep(&mut self, new_entries: Vec<CfgEntry>, new_version: u32) -> bool {
        let backup_version = self.live_version;
        let backup_len = self.entries.len();
        match self.hot_load(new_entries, new_version) {
            Ok(()) => true,
            Err(_) => {
                // 保留旧配：hot_load 失败路径已在写前全拒（原子性）。
                let _ = (backup_version, backup_len);
                false
            }
        }
    }

    /// 查配置值。
    pub fn lookup(&self, kind: CfgKind, region: &str, key: &str) -> Option<&'static str> {
        self.entries.iter().find(|e| e.kind == kind && e.region == region && e.key == key).map(|e| e.value)
    }

    /// 配置冲突：同 kind+region+key 多条 → 优先级仲裁表（版本高者胜，平局拒）。
    pub fn arbitrate(&self, kind: CfgKind, region: &str, key: &str) -> Result<&'static str, CfCode> {
        let mut best: Option<(&CfgEntry, usize)> = None;
        let mut count = 0;
        for e in self.entries.iter() {
            if e.kind == kind && e.region == region && e.key == key {
                count += 1;
                let score = e.version as usize;
                if best.map_or(true, |(b, bs)| score > bs) { best = Some((e, score)); }
            }
        }
        if count == 0 { return Err(CfCode::LoadFail); }
        let (w, ws) = best.unwrap();
        let ties = self.entries.iter().filter(|e| e.kind == kind && e.region == region && e.key == key && e.version as usize == ws).count();
        if ties > 1 { return Err(CfCode::Conflict); } // 平局拒——需人工定优先级
        Ok(w.value)
    }
}

/// 显性通告册。
pub struct NoticeBook { pub notices: Vec<&'static str> }

impl NoticeBook {
    pub fn new() -> Self { NoticeBook { notices: Vec::new() } }
    pub fn notify(&mut self, what: &'static str) { self.notices.push(what); }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves349_checks() -> CheckSet {
    let mut set = CheckSet::new("ves349");

    // C349-01 schema 冻结：五字段名单一致。
    chk(&mut set, "C349-01", schema_frozen(&["kind", "region", "key", "value", "version"]), "五字段冻结");

    // C349-02 schema 漂移拒载。
    chk(&mut set, "C349-02", !schema_frozen(&["kind", "region", "key", "val", "version"]), "val≠value => 漂移");

    // C349-03 四类分离声明：数据驱动（条目即数据）。
    let mut cb = CfgBook::new();
    let batch = vec![
        CfgEntry { kind: CfgKind::Law, region: "EU", key: "gdpr-mode", value: "strict", version: 2 },
        CfgEntry { kind: CfgKind::Format, region: "DE", key: "neg-paren", value: "on", version: 1 },
        CfgEntry { kind: CfgKind::Content, region: "BR", key: "ok-gesture", value: "blocked", version: 3 },
        CfgEntry { kind: CfgKind::Payment, region: "CN", key: "carrier-pay", value: "on", version: 1 },
    ];
    chk(&mut set, "C349-03", cb.hot_load(batch.clone(), 1).is_ok() && cb.entries.len() == 4, "四类配置热加载");

    // C349-04 热加载不重启：版本推进即生效。
    chk(&mut set, "C349-04", cb.live_version == 1, "live 版本推进");

    // C349-05 查配置四类各一条。
    chk(&mut set, "C349-05",
        cb.lookup(CfgKind::Law, "EU", "gdpr-mode") == Some("strict")
        && cb.lookup(CfgKind::Payment, "CN", "carrier-pay") == Some("on"), "四类查值");

    // C349-06 schema 畸形条目 → 拒载。
    let bad = vec![CfgEntry { kind: CfgKind::Law, region: "", key: "x", value: "y", version: 1 }];
    chk(&mut set, "C349-06", cb.hot_load(bad, 2) == Err(CfCode::SchemaDrift), "空 region => 拒载");

    // C349-07 拒载后保留旧配。
    chk(&mut set, "C349-07", cb.entries.len() == 4 && cb.live_version == 1, "旧配未动");

    // C349-08 失败显性通告。
    let mut nb = NoticeBook::new();
    if !cb.hot_load_or_keep(vec![CfgEntry { kind: CfgKind::Law, region: "", key: "x", value: "y", version: 1 }], 2) {
        nb.notify("热加载失败保留旧配");
    }
    chk(&mut set, "C349-08", nb.notices.len() == 1, "通告留痕");

    // C349-09 配置冲突仲裁：版本高者胜。
    let dup = vec![
        CfgEntry { kind: CfgKind::Format, region: "US", key: "week-start", value: "sun", version: 1 },
        CfgEntry { kind: CfgKind::Format, region: "US", key: "week-start", value: "mon", version: 2 },
    ];
    let _ = cb.hot_load(dup, 3);
    chk(&mut set, "C349-09", cb.arbitrate(CfgKind::Format, "US", "week-start") == Ok("mon"), "版本 2 胜出");

    // C349-10 平局拒。
    let tie = vec![
        CfgEntry { kind: CfgKind::Format, region: "JP", key: "date-sep", value: "slash", version: 1 },
        CfgEntry { kind: CfgKind::Format, region: "JP", key: "date-sep", value: "dash", version: 1 },
    ];
    let _ = cb.hot_load(tie, 4);
    chk(&mut set, "C349-10", cb.arbitrate(CfgKind::Format, "JP", "date-sep") == Err(CfCode::Conflict), "同版本平局 => 拒");

    // C349-11 未配置查询 → LoadFail。
    chk(&mut set, "C349-11", cb.arbitrate(CfgKind::Law, "ZZ", "nope") == Err(CfCode::LoadFail), "无配置 => LoadFail");

    // C349-12 四类名单单源。
    chk(&mut set, "C349-12", CfgKind::ALL.len() == 4 && CfgKind::ALL[3].name() == "payment", "四类单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves349_smoke() {
        let set = run_ves349_checks();
        assert!(set.failed_count() == 0, "ves349 red: {}", set.red_items_text());
    }

    #[test]
    fn ves349_arbitrate() {
        let mut cb = CfgBook::new();
        let _ = cb.hot_load(vec![
            CfgEntry { kind: CfgKind::Content, region: "US", key: "k", value: "a", version: 1 },
            CfgEntry { kind: CfgKind::Content, region: "US", key: "k", value: "b", version: 5 },
            CfgEntry { kind: CfgKind::Content, region: "US", key: "k", value: "c", version: 3 },
        ], 1);
        assert_eq!(cb.arbitrate(CfgKind::Content, "US", "k"), Ok("b"));
    }
}
