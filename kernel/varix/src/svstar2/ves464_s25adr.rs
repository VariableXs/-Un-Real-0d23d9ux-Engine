//! ves464_s25adr — VE-F4230 · 契约与 ADR 联动
//! 架构决策记录双向链接（契约←→ADR 互挂指针：契约引用决策依据/ADR 引用落地契约）+
//! 决策变更→契约影响预警+链接完整性审计；单向挂→补挂+催办；决策变更未预警→立案；链接断→修复登记；
//! ADR 含无障碍决策标注位（涉无障碍决策必挂——域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1820;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdCode {
    Ok = 0,
    OneWay = SEG_BASE + 0x01,     // 0x1821 单向挂
    NoWarn = SEG_BASE + 0x02,     // 0x1822 决策变更未预警
    LinkBroken = SEG_BASE + 0x03, // 0x1823
    A11yUntagged = SEG_BASE + 0x04, // 0x1824 标注位缺（域本色）
}

/// 互挂指针（契约 id ←→ ADR id + a11y 标注位）。
#[derive(Clone, Copy)]
pub struct Link { pub contract: u32, pub adr: u32, pub a11y: bool, pub c2a: bool, pub a2c: bool }

/// 联动册。
pub struct AdrBook {
    pub links: Vec<Link>,
    pub warned: Vec<u32>, // 决策变更预警记录（ADR id）
    pub filed: u32,
    pub repaired: u32,
}

impl AdrBook {
    pub fn new() -> Self { AdrBook { links: Vec::new(), warned: Vec::new(), filed: 0, repaired: 0 } }
    pub fn put(&mut self, l: Link) {
        self.links.retain(|x| x.contract != l.contract);
        self.links.push(l);
    }
    /// 链接完整性审计：双向必须齐+a11y 决策必标注（域本色）。
    pub fn audit(&mut self) -> Result<(), AdCode> {
        for l in &self.links {
            if !l.c2a || !l.a2c { self.filed += 1; return Err(AdCode::OneWay); }
            if l.a11y { return Err(AdCode::A11yUntagged); } // 占位：a11y=true 表示「涉无障碍但未标注」
        }
        Ok(())
    }
    /// 决策变更预警：ADR 变更→影响契约预警（未预警→立案）。
    pub fn change_adr(&mut self, adr: u32, warn: bool) -> Result<(), AdCode> {
        if !self.links.iter().any(|l| l.adr == adr) { return Err(AdCode::LinkBroken); }
        if !warn { self.filed += 1; return Err(AdCode::NoWarn); }
        if !self.warned.contains(&adr) { self.warned.push(adr); }
        Ok(())
    }
    /// 补挂（修复单向）。
    pub fn fix_link(&mut self, contract: u32, rev_c2a: bool, rev_a2c: bool) -> bool {
        for l in self.links.iter_mut() {
            if l.contract == contract {
                if rev_c2a { l.c2a = true; }
                if rev_a2c { l.a2c = true; }
                self.repaired += 1;
                return true;
            }
        }
        false
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves464_checks() -> CheckSet {
    let mut set = CheckSet::new("ves464");
    let mut bk = AdrBook::new();

    // C464-01 双向互挂齐→审计过。
    bk.put(Link { contract: 7001, adr: 100, a11y: false, c2a: true, a2c: true });
    bk.put(Link { contract: 7002, adr: 101, a11y: false, c2a: true, a2c: true });
    chk(&mut set, "C464-01", bk.audit().is_ok(), "双向齐过");

    // C464-02 单向挂→补挂码+立案。
    bk.put(Link { contract: 7003, adr: 102, a11y: false, c2a: true, a2c: false });
    chk(&mut set, "C464-02", bk.audit() == Err(AdCode::OneWay) && bk.filed == 1, "单向挂检出");

    // C464-03 补挂修复→复检过。
    chk(&mut set, "C464-03", bk.fix_link(7003, false, true) && bk.repaired == 1
        && bk.audit().is_ok(), "补挂复检");

    // C464-04 决策变更预警在册。
    chk(&mut set, "C464-04", bk.change_adr(100, true).is_ok() && bk.warned.contains(&100), "变更预警");

    // C464-05 决策变更未预警→立案。
    chk(&mut set, "C464-05", bk.change_adr(101, false) == Err(AdCode::NoWarn) && bk.filed == 2, "未预警立案");

    // C464-06 链接断（ADR 无挂）→断链码。
    chk(&mut set, "C464-06", bk.change_adr(999, true) == Err(AdCode::LinkBroken), "断链检出");

    // C464-07 域本色：涉无障碍决策未标注→拒。
    bk.put(Link { contract: 7004, adr: 103, a11y: true, c2a: true, a2c: true });
    chk(&mut set, "C464-07", bk.audit() == Err(AdCode::A11yUntagged), "a11y 标注位缺拒");

    // C464-08 标注齐→复检过。
    bk.links[3].a11y = false; // 修正为已标注态
    chk(&mut set, "C464-08", bk.audit().is_ok(), "标注复检");

    // C464-09 预警幂等。
    bk.change_adr(100, true).unwrap();
    chk(&mut set, "C464-09", bk.warned.len() == 1, "预警幂等");

    // C464-10 互挂覆盖幂等（同契约重挂替换）。
    bk.put(Link { contract: 7001, adr: 105, a11y: false, c2a: true, a2c: true });
    chk(&mut set, "C464-10", bk.links.len() == 4, "重挂替换");

    // C464-11 立案计数累计。
    chk(&mut set, "C464-11", bk.filed == 2, "立案计数");

    // C464-12 错误码段位单源。
    chk(&mut set, "C464-12", AdCode::A11yUntagged as u16 == SEG_BASE + 4, "段位单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves464_smoke() {
        let set = run_ves464_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves464 red: {:?}", red);
    }
}
