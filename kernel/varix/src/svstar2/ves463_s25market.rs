//! ves463_s25market — VE-F4229 · 契约市场与复用
//! 契约复用库（通用契约模板：分页/排序/过滤/上传四类起步）+模板实例化流程（实例化→定制→六检验证→注册）+
//! 复用率度量；模板缺陷→模板即缺陷立案；实例化失真→六检拦截；度量失真→口径对拍；
//! 模板内建无障碍判据骨架（实例化不可删除——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1800;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MkCode {
    Ok = 0,
    TemplateDefect = SEG_BASE + 0x01, // 0x1801
    InstanceDistort = SEG_BASE + 0x02, // 0x1802
    MetricSkew = SEG_BASE + 0x03,     // 0x1803
}

/// 四类模板。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TemplateKind { Pagination = 0, Sort = 1, Filter = 2, Upload = 3 }

impl TemplateKind {
    pub const ALL: [TemplateKind; 4] = [TemplateKind::Pagination, TemplateKind::Sort, TemplateKind::Filter, TemplateKind::Upload];
}

/// 模板（内建 a11y 判据骨架——域本色）。
#[derive(Clone, Copy)]
pub struct Template { pub kind: TemplateKind, pub a11y_skeleton: bool, pub body: u32 }

/// 实例化器。
pub struct MarketBook {
    pub templates: Vec<Template>,
    pub instances: Vec<(TemplateKind, u32)>, // (类, 实例 id)
    pub reused: u32,
    pub filed: u32,
}

impl MarketBook {
    pub fn new() -> Self { MarketBook { templates: Vec::new(), instances: Vec::new(), reused: 0, filed: 0 } }
    pub fn put_template(&mut self, k: TemplateKind, a11y: bool, body: u32) {
        self.templates.retain(|t| t.kind != k);
        self.templates.push(Template { kind: k, a11y_skeleton: a11y, body });
    }
    /// 实例化：模板齐+a11y 骨架在→实例；骨架缺→模板即缺陷立案。
    pub fn instantiate(&mut self, k: TemplateKind, custom: u32) -> Result<u32, MkCode> {
        let t = match self.templates.iter().find(|t| t.kind == k) {
            Some(t) => t.clone(), None => { self.filed += 1; return Err(MkCode::TemplateDefect); }
        };
        if !t.a11y_skeleton { self.filed += 1; return Err(MkCode::TemplateDefect); }
        // 实例化失真：定制体丢失内建骨架（custom 尾号 9 代）→六检拦截。
        if custom % 10 == 9 { return Err(MkCode::InstanceDistort); }
        let id = 8000 + self.instances.len() as u32;
        self.instances.push((k, id));
        self.reused += 1;
        Ok(id)
    }
    /// 复用率度量：实例数/模板数（口径对拍）。
    pub fn reuse_rate_bp(&self) -> Option<u32> {
        if self.templates.is_empty() { return None; }
        Some(self.instances.len() as u32 * 10_000 / self.templates.len() as u32)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves463_checks() -> CheckSet {
    let mut set = CheckSet::new("ves463");
    let mut bk = MarketBook::new();

    // C463-01 四类模板入库（内建 a11y 骨架——域本色）。
    for k in TemplateKind::ALL { bk.put_template(k, true, 100); }
    chk(&mut set, "C463-01", bk.templates.len() == 4
        && bk.templates.iter().all(|t| t.a11y_skeleton), "四类模板+骨架");

    // C463-02 实例化→定制→注册（复用计数）。
    let id1 = bk.instantiate(TemplateKind::Pagination, 100).unwrap();
    let id2 = bk.instantiate(TemplateKind::Sort, 200).unwrap();
    chk(&mut set, "C463-02", id1 == 8000 && id2 == 8001 && bk.reused == 2, "实例化注册");

    // C463-03 实例化失真→六检拦截。
    chk(&mut set, "C463-03", bk.instantiate(TemplateKind::Filter, 119) == Err(MkCode::InstanceDistort), "失真拦截");

    // C463-04 模板缺骨架→模板即缺陷立案。
    bk.put_template(TemplateKind::Upload, false, 100);
    chk(&mut set, "C463-04", bk.instantiate(TemplateKind::Upload, 100) == Err(MkCode::TemplateDefect)
        && bk.filed == 1, "缺陷立案");

    // C463-05 修复骨架→可实例化。
    bk.put_template(TemplateKind::Upload, true, 100);
    chk(&mut set, "C463-05", bk.instantiate(TemplateKind::Upload, 100).is_ok(), "修复可用");

    // C463-06 复用率度量。
    let rate = bk.reuse_rate_bp().unwrap();
    chk(&mut set, "C463-06", rate == 10_000, "复用率 bp");

    // C463-07 度量失真（零模板）→空态。
    let bk2 = MarketBook::new();
    chk(&mut set, "C463-07", bk2.reuse_rate_bp().is_none(), "零模板空态");

    // C463-08 四类单源断言。
    chk(&mut set, "C463-08", TemplateKind::ALL.len() == 4
        && TemplateKind::ALL[3] == TemplateKind::Upload, "四类单源");

    // C463-09 实例 id 递增唯一。
    chk(&mut set, "C463-09", bk.instances.iter().map(|(_, i)| *i).collect::<Vec<u32>>()
        == vec![8000, 8001, 8002], "实例唯一");

    // C463-10 立案计数累计。
    chk(&mut set, "C463-10", bk.filed == 1, "立案计数");

    // C463-11 模板覆盖幂等。
    bk.put_template(TemplateKind::Sort, true, 100);
    chk(&mut set, "C463-11", bk.templates.len() == 4, "模板幂等");

    // C463-12 错误码段位单源。
    chk(&mut set, "C463-12", MkCode::InstanceDistort as u16 == SEG_BASE + 2, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves463_smoke() {
        let set = run_ves463_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves463 red: {:?}", red);
    }
}
