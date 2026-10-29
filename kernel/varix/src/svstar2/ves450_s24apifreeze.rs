//! ves450_s24apifreeze — VE-F4216 · 一致性 API 冻结
//! U 域对外 API 面冻结（注册/查询/验证/度量四族 API+参数域钳制+错误码三件套）；
//! 冻结哈希留痕；冻结后直改→回退+立案；越域参数→钳制+告警；错误码缺→补齐；
//! API 含无障碍判据查询族（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1520;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AfCode {
    Ok = 0,
    Clamped = SEG_BASE + 0x01,   // 0x1521 越域钳制告警
    ErrCodeGap = SEG_BASE + 0x02, // 0x1522
    DirectChange = SEG_BASE + 0x03, // 0x1523
}

/// 四族 API（含无障碍判据查询族——域本色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily { Register = 0, Query = 1, Validate = 2, Metric = 3, A11yQuery = 4 }

impl ApiFamily {
    pub const ALL: [ApiFamily; 5] = [
        ApiFamily::Register, ApiFamily::Query, ApiFamily::Validate, ApiFamily::Metric, ApiFamily::A11yQuery];
}

/// API 条目（签名 id + 参数域上下限 + 三件套标记）。
pub struct ApiEntry { pub family: ApiFamily, pub sig: u32, pub pmin: u32, pub pmax: u32, pub triple: [bool; 3] }

/// API 冻结册。
pub struct ApiFreezeBook {
    pub entries: Vec<ApiEntry>,
    pub frozen_hash: u32,
    pub filed: u32,
    pub alerts: u32,
}

impl ApiFreezeBook {
    pub fn new() -> Self { ApiFreezeBook { entries: Vec::new(), frozen_hash: 0, filed: 0, alerts: 0 } }
    pub fn put(&mut self, e: ApiEntry) {
        self.entries.retain(|x| x.family != e.family);
        self.entries.push(e);
    }
    fn hash(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for e in &self.entries {
            h ^= e.sig; h = h.wrapping_mul(0x0100_0193);
            h ^= e.pmin; h = h.wrapping_mul(0x0100_0193);
            h ^= e.pmax; h = h.wrapping_mul(0x0100_0193);
        }
        h
    }
    /// 错误码三件套齐备核验（入参错误/越域/内部错）。
    pub fn triple_full(&self) -> Result<(), AfCode> {
        for e in &self.entries {
            if e.triple.iter().any(|x| !x) { return Err(AfCode::ErrCodeGap); }
        }
        Ok(())
    }
    /// 参数域钳制：越域值钳到边界+告警。
    pub fn clamp(&mut self, family: ApiFamily, v: u32) -> u32 {
        if let Some(e) = self.entries.iter().find(|e| e.family == family) {
            if v < e.pmin { self.alerts += 1; return e.pmin; }
            if v > e.pmax { self.alerts += 1; return e.pmax; }
        }
        v
    }
    /// 冻结：五族齐+三件套齐→哈希留痕。
    pub fn freeze(&mut self) -> Result<(), AfCode> {
        for f in ApiFamily::ALL {
            if !self.entries.iter().any(|e| e.family == f) { return Err(AfCode::ErrCodeGap); }
        }
        self.triple_full()?;
        self.frozen_hash = self.hash();
        Ok(())
    }
    /// 冻结后直改→回退+立案。
    pub fn direct_change(&mut self, family: ApiFamily, pmax: u32) -> Result<(), AfCode> {
        let old = self.entries.iter().find(|e| e.family == family).map(|e| (e.sig, e.pmin, e.pmax));
        if let Some((sig, pmin, _)) = old {
            for e in self.entries.iter_mut() {
                if e.family == family { e.pmax = pmax; }
            }
            if self.hash() != self.frozen_hash {
                if let Some((sig, pmin, pm)) = old {
                    for e in self.entries.iter_mut() {
                        if e.family == family { e.sig = sig; e.pmin = pmin; e.pmax = pm; }
                    }
                }
                self.filed += 1;
                return Err(AfCode::DirectChange);
            }
        }
        Ok(())
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves450_checks() -> CheckSet {
    let mut set = CheckSet::new("ves450");
    let mut bk = ApiFreezeBook::new();
    for (f, pmax) in [(ApiFamily::Register, 100u32), (ApiFamily::Query, 200), (ApiFamily::Validate, 300),
                      (ApiFamily::Metric, 400), (ApiFamily::A11yQuery, 500)] {
        bk.put(ApiEntry { family: f, sig: f as u32 * 10, pmin: 1, pmax, triple: [true; 3] });
    }

    // C450-01 五族齐+三件套齐→冻结+哈希留痕。
    chk(&mut set, "C450-01", bk.freeze().is_ok() && bk.frozen_hash != 0, "冻结留痕");

    // C450-02 参数域钳制：越上界→钳到边界+告警。
    chk(&mut set, "C450-02", bk.clamp(ApiFamily::Query, 999) == 200 && bk.alerts == 1, "上界钳制");

    // C450-03 越下界钳制。
    chk(&mut set, "C450-03", bk.clamp(ApiFamily::Query, 0) == 1 && bk.alerts == 2, "下界钳制");

    // C450-04 域内值直通。
    chk(&mut set, "C450-04", bk.clamp(ApiFamily::Query, 100) == 100, "域内直通");

    // C450-05 错误码缺→补齐码。
    bk.entries[0].triple[1] = false;
    chk(&mut set, "C450-05", bk.triple_full() == Err(AfCode::ErrCodeGap), "三件套缺");

    // C450-06 补齐后过。
    bk.entries[0].triple[1] = true;
    chk(&mut set, "C450-06", bk.triple_full().is_ok(), "三件套齐");

    // C450-07 冻结后直改→回退+立案。
    chk(&mut set, "C450-07", bk.direct_change(ApiFamily::Query, 999) == Err(AfCode::DirectChange)
        && bk.entries.iter().find(|e| e.family == ApiFamily::Query).unwrap().pmax == 200
        && bk.filed == 1, "直改回退立案");

    // C450-08 冻结后无扰动→直通（哈希一致）。
    chk(&mut set, "C450-08", bk.direct_change(ApiFamily::Query, 200).is_ok(), "无扰动直通");

    // C450-09 五族单源（a11y 查询族在册——域本色）。
    chk(&mut set, "C450-09", ApiFamily::ALL.len() == 5
        && ApiFamily::ALL[4] == ApiFamily::A11yQuery, "a11y 查询族");

    // C450-10 冻结未含族→拒。
    let mut bk2 = ApiFreezeBook::new();
    for f in [ApiFamily::Register, ApiFamily::Query, ApiFamily::Validate, ApiFamily::Metric] {
        bk2.put(ApiEntry { family: f, sig: 1, pmin: 1, pmax: 9, triple: [true; 3] });
    }
    chk(&mut set, "C450-10", bk2.freeze() == Err(AfCode::ErrCodeGap), "缺 a11y 族拒冻");

    // C450-11 钳制告警计数累计。
    bk.clamp(ApiFamily::Metric, 9999);
    chk(&mut set, "C450-11", bk.alerts == 3, "告警累计");

    // C450-12 哈希可复现。
    let h = bk.hash();
    chk(&mut set, "C450-12", h == bk.hash() && h == bk.frozen_hash, "哈希复现");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves450_smoke() {
        let set = run_ves450_checks();
        assert!(set.failed_count() == 0, "ves450 red: {}", set.red_items_text());
    }
}
