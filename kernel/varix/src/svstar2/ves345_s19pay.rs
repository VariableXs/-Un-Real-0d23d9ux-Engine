//! ves345_s19pay — VE-F4105 · T06 地区支付与商业
//! 支付方式地区矩阵（卡/钱包/运营商代扣×地区可用表）+ 价格展示货币格式联动（F4106 格式单源）+ 商业合规（发票/税率地区规则登记）+ 不可用次选显性提示 + 税率缺阻断交易 + 支付数据不落日志（红线）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xBDC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PyCode {
    Ok = 0,
    TaxMissing = SEG_BASE + 0x01,
    Drift = SEG_BASE + 0x02,
    NoLog = SEG_BASE + 0x03,
}

/// 支付方式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PayMethod { Card = 0, Wallet = 1, Carrier = 2 }

impl PayMethod {
    pub const ALL: [PayMethod; 3] = [PayMethod::Card, PayMethod::Wallet, PayMethod::Carrier];
    pub fn name(self) -> &'static str {
        match self { PayMethod::Card => "card", PayMethod::Wallet => "wallet", PayMethod::Carrier => "carrier" }
    }
}

/// 支付矩阵：地区 × 可用方式（有序——首选在前）。
#[derive(Clone)]
pub struct PayRow { pub region: &'static str, pub methods: Vec<PayMethod> }

pub struct PayMatrix { pub rows: Vec<PayRow> }

impl PayMatrix {
    pub fn new() -> Self { PayMatrix { rows: Vec::new() } }

    pub fn put(&mut self, r: PayRow) -> Result<(), PyCode> {
        if r.methods.is_empty() { return Err(PyCode::Drift); }
        if self.rows.iter().any(|x| x.region == r.region) { return Err(PyCode::Drift); }
        self.rows.push(r);
        Ok(())
    }

    pub fn row_of(&self, region: &str) -> Option<&PayRow> {
        self.rows.iter().find(|r| r.region == region)
    }

    /// 首选支付方式（矩阵序第一位）。
    pub fn primary(&self, region: &str) -> Option<PayMethod> {
        self.row_of(region).and_then(|r| r.methods.first().copied())
    }

    /// 不可用次选：请求方式不在矩阵 → 次选（矩阵内下一可用）+ 显性标记。
    pub fn with_fallback(&self, region: &str, want: PayMethod) -> Result<(PayMethod, bool), PyCode> {
        match self.row_of(region) {
            None => Err(PyCode::Drift),
            Some(r) => {
                if r.methods.contains(&want) {
                    Ok((want, false))
                } else {
                    r.methods.first().copied()
                        .map(|m| (m, true))
                        .ok_or(PyCode::Drift)
                }
            }
        }
    }

    /// 矩阵对版：行数与方式集合与基准一致。
    pub fn agrees(&self, base: &PayMatrix) -> bool {
        base.rows.iter().all(|b| {
            self.row_of(b.region).map_or(false, |r| r.methods == b.methods)
        }) && self.rows.len() == base.rows.len()
    }
}

/// 货币格式联动（F4106 单源消费面）：地区 → 货币码。
pub fn currency_of(region: &str) -> &'static str {
    match region { "US" => "USD", "CN" => "CNY", "JP" => "JPY", "EU" | "DE" | "FR" => "EUR", _ => "USD" }
}

/// 商业规则册：税率/发票。
#[derive(Clone)]
pub struct BizRule { pub region: &'static str, pub tax_bp: u32, pub invoice_required: bool }

pub struct BizBook { pub rules: Vec<BizRule> }

impl BizBook {
    pub fn new() -> Self { BizBook { rules: Vec::new() } }
    pub fn put(&mut self, r: BizRule) { self.rules.push(r); }
    pub fn of(&self, region: &str) -> Option<&BizRule> {
        self.rules.iter().find(|r| r.region == region)
    }

    /// 税率缺 → 阻断交易 + 登记。
    pub fn tax_gate(&self, region: &str) -> Result<u32, PyCode> {
        self.of(region).map(|r| r.tax_bp).ok_or(PyCode::TaxMissing)
    }
}

/// 支付日志脱敏红线：日志条目不得含 PAN/卡号。
pub struct PayLog { pub entries: Vec<String> }

impl PayLog {
    pub fn new() -> Self { PayLog { entries: Vec::new() } }

    /// 记录：卡号字段必须脱敏（只留尾 4 位）。
    pub fn record(&mut self, region: &str, method: PayMethod, pan: &str) {
        let masked: String = if pan.len() > 4 {
            let tail: String = pan[pan.len() - 4..].chars().collect();
            let mut s = String::from("****-****-****-");
            s.push_str(&tail);
            s
        } else {
            String::from("****")
        };
        let mut e = String::from(region);
        e.push('|');
        e.push_str(method.name());
        e.push('|');
        e.push_str(&masked);
        self.entries.push(e);
    }

    /// 红线自检：日志不含完整卡号。
    pub fn no_pan_leak(&self, pan: &str) -> bool {
        pan.len() <= 4 || !self.entries.iter().any(|e| e.contains(pan))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves345_checks() -> CheckSet {
    let mut set = CheckSet::new("ves345");
    let mut pm = PayMatrix::new();

    // C345-01 支付矩阵登记：三地区三方式。
    let _ = pm.put(PayRow { region: "US", methods: vec![PayMethod::Card, PayMethod::Wallet] });
    let _ = pm.put(PayRow { region: "CN", methods: vec![PayMethod::Wallet, PayMethod::Card, PayMethod::Carrier] });
    let _ = pm.put(PayRow { region: "DE", methods: vec![PayMethod::Card] });
    chk(&mut set, "C345-01", pm.rows.len() == 3, "三地区登记");

    // C345-02 重复地区登记 → 漂移拒。
    chk(&mut set, "C345-02",
        pm.put(PayRow { region: "US", methods: vec![PayMethod::Card] }) == Err(PyCode::Drift), "重复 => Drift");

    // C345-03 首选方式单源。
    chk(&mut set, "C345-03", pm.primary("CN") == Some(PayMethod::Wallet) && pm.primary("US") == Some(PayMethod::Card), "CN 首选钱包 US 首选卡");

    // C345-04 不可用次选 + 显性标记。
    let fb = pm.with_fallback("DE", PayMethod::Carrier);
    chk(&mut set, "C345-04", fb == Ok((PayMethod::Card, true)), "DE 无代扣 → 次选卡显性");

    // C345-05 可用不降级。
    chk(&mut set, "C345-05", pm.with_fallback("CN", PayMethod::Carrier) == Ok((PayMethod::Carrier, false)), "CN 代扣可用直用");

    // C345-06 货币格式联动单源。
    chk(&mut set, "C345-06",
        currency_of("CN") == "CNY" && currency_of("DE") == "EUR" && currency_of("JP") == "JPY", "三地区货币单源");

    // C345-07 税率门：已登记放行。
    let mut bb = BizBook::new();
    bb.put(BizRule { region: "DE", tax_bp: 1900, invoice_required: true });
    bb.put(BizRule { region: "US", tax_bp: 0, invoice_required: false });
    chk(&mut set, "C345-07", bb.tax_gate("DE") == Ok(1900), "DE 税率 19%");

    // C345-08 税率缺 → 阻断交易。
    chk(&mut set, "C345-08", bb.tax_gate("JP") == Err(PyCode::TaxMissing), "JP 未登记 => 阻断");

    // C345-09 发票要求列。
    chk(&mut set, "C345-09", bb.of("DE").map_or(false, |r| r.invoice_required) && !bb.of("US").map_or(false, |r| r.invoice_required), "发票列地区差异");

    // C345-10 支付数据不落日志（红线）：卡号脱敏。
    let mut log = PayLog::new();
    log.record("DE", PayMethod::Card, "4111111111111111");
    chk(&mut set, "C345-10", log.entries[0].ends_with("1111") && log.entries[0].starts_with("DE|card|****"), "日志只留尾 4 位");

    // C345-11 红线自检：完整卡号不泄漏。
    chk(&mut set, "C345-11", log.no_pan_leak("4111111111111111"), "完整 PAN 不在日志");

    // C345-12 矩阵对版。
    let mut base = PayMatrix::new();
    let _ = base.put(PayRow { region: "US", methods: vec![PayMethod::Card, PayMethod::Wallet] });
    let _ = base.put(PayRow { region: "CN", methods: vec![PayMethod::Wallet, PayMethod::Card, PayMethod::Carrier] });
    let _ = base.put(PayRow { region: "DE", methods: vec![PayMethod::Card] });
    chk(&mut set, "C345-12", pm.agrees(&base), "与基准矩阵对版一致");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves345_smoke() {
        let set = run_ves345_checks();
        assert!(set.failed_count() == 0, "ves345 red: {}", set.red_items_text());
    }

    #[test]
    fn ves345_fallback() {
        let mut pm = PayMatrix::new();
        let _ = pm.put(PayRow { region: "JP", methods: vec![PayMethod::Carrier] });
        assert_eq!(pm.with_fallback("JP", PayMethod::Card), Ok((PayMethod::Carrier, true)));
        assert_eq!(pm.primary("JP"), Some(PayMethod::Carrier));
    }
}
