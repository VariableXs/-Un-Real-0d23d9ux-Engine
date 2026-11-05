//! UNX-F1577 联签资产健康摘要（对齐 F0899 口径注册） · 0x78F40段
//!
//! 判据原文（UNX-F1577-J1 摘录）：「联签资产健康摘要挂接（B39 十账摘要入
//! F0997 无缺），schema 校验通过，红账投递 10/10 可达」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 十账齐备：B39 十账（计费/退款/对账/开票/积分/客服/部署/演练/联签/终验）
//!    逐账入 F0997 注册面，缺账即拒；
//! 2. schema 校验闭面：账名非空、健康三态（绿/黄/红）闭集、摘要指纹非零——
//!    三字段任一违约在登记口即拒，坏账不入注册面；
//! 3. 红账投递 10/10 可达：红账（健康=红的账）必须逐路投递到十个下游面
//!    （域聚合/告警/工单/看板/日报/总账/终验/移交包/导览/口径册），任一路
//!    不可达即投递失败点名；
//! 4. 投递注入 10/10：十路逐路断路（路线禁用）必须全数被可达门点出，防止
//!    投递面退化成「报喜不报忧」；
//! 5. 状态机 挂接中→已挂接→已投递 单向，倒退显性拒；诊断码闭集
//!    0x78F41..0x78F4C。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B07 批窗口 0x78E00..=0x78FC0）。
pub const SEG_BASE: u32 = 0x78F40;

/// 十账数（B39 批内钦定）。
pub const ACCOUNT_COUNT: usize = 10;

/// 下游投递面数（10/10 可达）。
pub const ROUTE_COUNT: usize = 10;

/// F0997 注册面名（口径注册挂接目标）。
pub const REGISTRY_NAME: &str = "F0997";

/// 十账名册（判据面钦定顺序）。
pub const ACCOUNT_NAMES: [&str; ACCOUNT_COUNT] = [
    "计费账", "退款账", "对账账", "开票账", "积分账",
    "客服账", "部署账", "演练账", "联签账", "终验账",
];

/// 十路投递面名册。
pub const ROUTE_NAMES: [&str; ROUTE_COUNT] = [
    "域聚合", "告警面", "工单面", "看板", "日报",
    "总账", "终验", "移交包", "导览", "口径册",
];

/// 联签资产健康摘要诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 账名非法（空名或重复）。
    EName,
    /// 健康态取值非法。
    EState,
    /// 摘要指纹非法（零指纹）。
    EDigest,
    /// schema 校验不过（挂接口拒绝）。
    ESchema,
    /// 十账未齐即投递。
    ELack,
    /// 投递面不可达。
    ERouteDown,
    /// 已投递封账。
    ESealed,
    /// 重复挂接。
    EDupHook,
    /// 空摘要样本。
    EEmpty,
    /// 红账缺失（投递面空跑）。
    ENoRed,
    /// 投递回执不符。
    EReceipt,
    /// 相位倒退。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EName => 0x01,
                Diag::EState => 0x02,
                Diag::EDigest => 0x03,
                Diag::ESchema => 0x04,
                Diag::ELack => 0x05,
                Diag::ERouteDown => 0x06,
                Diag::ESealed => 0x07,
                Diag::EDupHook => 0x08,
                Diag::EEmpty => 0x09,
                Diag::ENoRed => 0x0a,
                Diag::EReceipt => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EName => "账名非法：B39 十账按名册逐账登记，空名重名拒收",
            Diag::EState => "健康态取值非法：只认绿/黄/红三态，先核账面",
            Diag::EDigest => "摘要指纹非法：指纹为零视为空账，重算指纹再挂接",
            Diag::ESchema => "schema 校验不过：三字段（名/态/指纹）逐字段核对",
            Diag::ELack => "十账未齐：先补齐十账再发起投递",
            Diag::ERouteDown => "投递面不可达：逐路点名断路面，先修路再投递",
            Diag::ESealed => "已投递封账：封账后写入拒绝，改动走下一批摘要",
            Diag::EDupHook => "挂接已发起：重复挂接拒绝，先取回上一轮注册面",
            Diag::EEmpty => "空摘要样本：无账不构成摘要，先逐账登记",
            Diag::ENoRed => "红账缺失：无红账不构成投递面，先核账面健康",
            Diag::EReceipt => "投递回执不符：回执数与红账×路数不齐，逐路补投",
            Diag::EPhase => "相位倒退：挂接中→已挂接→已投递 单向，倒退即拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EName,
    Diag::EState,
    Diag::EDigest,
    Diag::ESchema,
    Diag::ELack,
    Diag::ERouteDown,
    Diag::ESealed,
    Diag::EDupHook,
    Diag::EEmpty,
    Diag::ENoRed,
    Diag::EReceipt,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78F41..0x78F4C 闭集内"
}

/// 健康三态闭集（与 F1507/F1575 口径同源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// 绿账。
    Green,
    /// 黄账（超时过/有保留）。
    Yellow,
    /// 红账（需投递告警）。
    Red,
}

impl Health {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Health::Green => "绿",
            Health::Yellow => "黄",
            Health::Red => "红",
        }
    }
}

/// 三态闭集数组。
pub const HEALTH_ALL: &[Health] = &[Health::Green, Health::Yellow, Health::Red];

/// 单账摘要（schema 三字段：名/态/指纹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountSummary {
    /// 账名（须在 B39 名册内）。
    pub name: &'static str,
    /// 健康三态。
    pub health: Health,
    /// 摘要指纹（非零）。
    pub digest: u64,
}

impl AccountSummary {
    /// schema 校验（三字段逐一面）。
    pub fn schema_ok(&self) -> bool {
        if self.name.is_empty() {
            return false;
        }
        if HEALTH_ALL.iter().all(|x| *x != self.health) {
            return false;
        }
        self.digest != 0
    }
}

/// 摘要相位（挂接中→已挂接→已投递 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 挂接中：逐账登记中。
    Hooking,
    /// 已挂接：十账齐、schema 全过。
    Hooked,
    /// 已投递：红账逐路投递完成。
    Delivered,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Hooking => 0,
            Phase::Hooked => 1,
            Phase::Delivered => 2,
        }
    }
}

/// F0997 注册面：逐账挂接 + 投递。
#[derive(Clone, Debug)]
pub struct HealthRegistry {
    phase: Phase,
    /// 逐账摘要（按名册槽位）。
    slots: Vec<Option<AccountSummary>>,
    /// 投递路线启用表（true=启用）。
    routes: [bool; ROUTE_COUNT],
    /// 投递回执计数（红账×启用路）。
    receipts: u32,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl HealthRegistry {
    /// 新注册面（十路全启用）。
    pub fn new() -> HealthRegistry {
        HealthRegistry {
            phase: Phase::Hooking,
            slots: (0..ACCOUNT_COUNT).map(|_| None).collect(),
            routes: [true; ROUTE_COUNT],
            receipts: 0,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 已挂接账数。
    pub fn hooked(&self) -> usize {
        self.slots.iter().filter(|x| x.is_some()).count()
    }

    /// 挂接一账（schema 三面逐面校验）。
    pub fn hook(&mut self, sum: AccountSummary) -> Result<(), u32> {
        if self.phase != Phase::Hooking {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if sum.name.is_empty() {
            self.rejects += 1;
            return Err(Diag::EName.val());
        }
        let listed = ACCOUNT_NAMES.iter().any(|x| *x == sum.name);
        if !listed {
            self.rejects += 1;
            return Err(Diag::EName.val());
        }
        if HEALTH_ALL.iter().all(|x| *x != sum.health) {
            self.rejects += 1;
            return Err(Diag::EState.val());
        }
        if sum.digest == 0 {
            self.rejects += 1;
            return Err(Diag::EDigest.val());
        }
        let idx = match ACCOUNT_NAMES.iter().position(|x| *x == sum.name) {
            Some(x) => x,
            None => return Err(Diag::EName.val()),
        };
        match self.slots.get_mut(idx) {
            Some(slot) => {
                if slot.is_some() {
                    self.rejects += 1;
                    return Err(Diag::EDupHook.val());
                }
                *slot = Some(sum);
                Ok(())
            }
            None => Err(Diag::EName.val()),
        }
    }

    /// 按账名取摘要。
    pub fn account_of(&self, name: &str) -> Option<&AccountSummary> {
        let idx = ACCOUNT_NAMES.iter().position(|x| *x == name)?;
        match self.slots.get(idx) {
            Some(Some(a)) => Some(a),
            _ => None,
        }
    }

    /// schema 全过（十账齐且逐账 schema_ok）。
    pub fn schema_all_ok(&self) -> bool {
        if self.hooked() < ACCOUNT_COUNT {
            return false;
        }
        for name in ACCOUNT_NAMES {
            match self.account_of(name) {
                Some(a) if a.schema_ok() => {}
                _ => return false,
            }
        }
        true
    }

    /// 挂接收束：十账齐且 schema 全过才转「已挂接」相位。
    pub fn finish_hook(&mut self) -> Result<(), u32> {
        if self.phase != Phase::Hooking {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if !self.schema_all_ok() {
            return Err(Diag::ELack.val());
        }
        self.phase = Phase::Hooked;
        Ok(())
    }

    /// 红账清单。
    pub fn reds(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for name in ACCOUNT_NAMES {
            if let Some(a) = self.account_of(name) {
                if a.health == Health::Red {
                    out.push(name);
                }
            }
        }
        out
    }

    /// 断开/接通一路投递面（注入用；封账后拒）。
    pub fn set_route(&mut self, route: usize, up: bool) -> Result<(), u32> {
        if self.phase == Phase::Delivered {
            return Err(Diag::ESealed.val());
        }
        match self.routes.get_mut(route) {
            Some(x) => {
                *x = up;
                Ok(())
            }
            None => Err(Diag::ERouteDown.val()),
        }
    }

    /// 路线启用状态。
    pub fn route_up(&self, route: usize) -> bool {
        match self.routes.get(route) {
            Some(x) => *x,
            None => false,
        }
    }

    /// 红账投递：十账齐 + 至少一红 + 全部启用路可达。
    /// 回执数 = 红账数 × 启用路数；有断路即点名失败。
    pub fn deliver(&mut self) -> Result<u32, u32> {
        if self.phase == Phase::Delivered {
            return Err(Diag::ESealed.val());
        }
        if self.phase != Phase::Hooked {
            return Err(Diag::ELack.val());
        }
        if !self.schema_all_ok() {
            return Err(Diag::ELack.val());
        }
        let reds = self.reds();
        if reds.is_empty() {
            return Err(Diag::ENoRed.val());
        }
        let red_n = reds.len() as u32;
        let mut up_n = 0u32;
        let mut r = 0usize;
        while r < ROUTE_COUNT {
            if self.route_up(r) {
                up_n += 1;
            }
            r += 1;
        }
        if up_n < ROUTE_COUNT as u32 {
            // 断路：投递失败，点名不可达。
            return Err(Diag::ERouteDown.val());
        }
        let receipts = red_n * up_n;
        self.receipts = receipts;
        self.phase = Phase::Delivered;
        Ok(receipts)
    }

    /// 投递回执数。
    pub fn receipts(&self) -> u32 {
        self.receipts
    }

    /// 摘要行（落档面，逐账一行）。
    pub fn summary_line(&self, name: &str) -> String {
        match self.account_of(name) {
            Some(a) => format!("{} {} 指纹{:016x}", a.name, a.health.name(), a.digest),
            None => format!("{} 缺账", name),
        }
    }

    /// 摘要全文（落档面）。
    pub fn text(&self) -> String {
        let mut out = String::new();
        for name in ACCOUNT_NAMES {
            out.push_str(&self.summary_line(name));
            out.push('\n');
        }
        out
    }
}

/// 摘要指纹（FNV-1a 整数折叠，无浮点无随机）。
pub fn digest_of(name: &str, health: Health) -> u64 {
    let mut h: u64 = 0xCBF29CE484222325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001B3);
    }
    h ^= match health {
        Health::Green => 0x67,
        Health::Yellow => 0x79,
        Health::Red => 0x72,
    };
    h.wrapping_mul(0x100000001B3)
}

/// 标准注册面：十账齐（计费/客服/联签三红，其余绿），十路全启用。
pub fn build_registry() -> HealthRegistry {
    let mut reg = HealthRegistry::new();
    let mut i = 0usize;
    while i < ACCOUNT_COUNT {
        let name = match ACCOUNT_NAMES.get(i) {
            Some(x) => *x,
            None => break,
        };
        let health = if name == "计费账" || name == "客服账" || name == "联签账" {
            Health::Red
        } else {
            Health::Green
        };
        let _ = reg.hook(AccountSummary { name, health, digest: digest_of(name, health) });
        i += 1;
    }
    reg
}

/// 投递注入 10/10：逐路断路后投递必须点出不可达，接通后回执恢复。
/// 返回十布尔（true=该路断路被可达门点出）。
pub fn route_matrix() -> [bool; ROUTE_COUNT] {
    let mut out = [false; ROUTE_COUNT];
    let mut i = 0usize;
    while i < ROUTE_COUNT {
        let mut reg = build_registry();
        let _ = reg.finish_hook();
        let _ = reg.set_route(i, false);
        let blocked = reg.deliver() == Err(Diag::ERouteDown.val());
        // 接通后恢复可达。
        let _ = reg.set_route(i, true);
        let ok_receipts = reg.deliver() == Ok(3 * ROUTE_COUNT as u32);
        let cell = match out.get_mut(i) {
            Some(x) => x,
            None => break,
        };
        *cell = blocked && ok_receipts;
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B07 派工卡 0x78E00..=0x78FC0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x78E00 || SEG_BASE > 0x78FC0 {
        return false;
    }
    SEG_BASE == 0x78F40
}

/// 健康三态闭集。
fn health_closed_ok() -> bool {
    if HEALTH_ALL.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < HEALTH_ALL.len() {
        let a = match HEALTH_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < HEALTH_ALL.len() {
            let b = match HEALTH_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 挂接面生效（十账齐 + schema 全过 + 相位已挂接）。
fn hooked_ok(reg: &HealthRegistry) -> bool {
    reg.hooked() == ACCOUNT_COUNT && reg.schema_all_ok() && reg.phase() == Phase::Hooked
}

/// 投递 10/10 可达。
fn route_ok() -> bool {
    route_matrix() == [true; ROUTE_COUNT]
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_registry();
    let b = build_registry();
    a.text() == b.text() && !a.text().is_empty()
}

/// 十三项判据：UNX-F1577 联签资产健康摘要。
pub fn run_unx1577_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1577");
    let mut reg = build_registry();

    s.add_dyn(
        "F1577-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1577-02 健康三态闭集",
        health_closed_ok(),
        &format!(
            "n={} states={}/{}/{}",
            HEALTH_ALL.len(),
            Health::Green.name(),
            Health::Yellow.name(),
            Health::Red.name()
        ),
    );

    // 基线十账 schema 全过、红账三户。
    let reds_before = reg.reds();
    s.add_dyn(
        "F1577-03 基线十账挂接 schema 全过",
        reg.hooked() == ACCOUNT_COUNT && reg.schema_all_ok(),
        &format!("hooked={} reds={}", reg.hooked(), reds_before.len()),
    );

    // 结构拒绝：空名、册外名、坏态、零指纹、重挂。
    let mut r = HealthRegistry::new();
    let empty_name = r.hook(AccountSummary { name: "", health: Health::Green, digest: 1 }) == Err(Diag::EName.val());
    let out_name = r.hook(AccountSummary { name: "册外账", health: Health::Green, digest: 1 }) == Err(Diag::EName.val());
    let dup = r.hook(AccountSummary { name: "计费账", health: Health::Green, digest: 11 }) == Ok(());
    let dup2 = r.hook(AccountSummary { name: "计费账", health: Health::Green, digest: 12 }) == Err(Diag::EDupHook.val());
    let zero_dg = r.hook(AccountSummary { name: "退款账", health: Health::Green, digest: 0 }) == Err(Diag::EDigest.val());
    s.add_dyn(
        "F1577-04 结构拒绝五门",
        empty_name && out_name && dup && dup2 && zero_dg,
        &format!("rejects={}", r.rejects()),
    );

    // 挂接发起：十账满后再挂同账必须点重挂。
    let hook_after_full = reg.hook(AccountSummary { name: "计费账", health: Health::Red, digest: 99 });
    s.add_dyn(
        "F1577-05 重挂显性拒",
        hook_after_full == Err(Diag::EDupHook.val()) && reg.rejects() >= 1,
        &format!("rejects={}", reg.rejects()),
    );

    // schema 逐字段校验：任一字段违约 schema_ok 即假。
    let bad = AccountSummary { name: "终验账", health: Health::Red, digest: 0 };
    let good = AccountSummary { name: "终验账", health: Health::Red, digest: digest_of("终验账", Health::Red) };
    s.add_dyn(
        "F1577-06 schema 三字段逐面校验",
        !bad.schema_ok() && good.schema_ok(),
        &format!("bad={} good={}", bad.schema_ok(), good.schema_ok()),
    );

    // 投递注入 10/10。
    s.add_dyn(
        "F1577-07 红账投递十路全可达",
        route_ok(),
        &format!("matrix={:?}", route_matrix()),
    );

    // 红账三户点名（计费/客服/联签）。
    let reds = build_registry().reds();
    s.add_dyn(
        "F1577-08 红账三户点名",
        reds.len() == 3 && reds.contains(&"计费账") && reds.contains(&"客服账") && reds.contains(&"联签账"),
        &format!("reds={:?}", reds),
    );

    // 投递：收束挂接相位后回执 = 3红 × 10路。
    let finish = reg.finish_hook() == Ok(());
    let got = reg.deliver();
    let got_ok = match got {
        Ok(n) => n == 3 * ROUTE_COUNT as u32,
        Err(_) => false,
    };
    s.add_dyn(
        "F1577-09 红账投递回执齐",
        finish && got_ok && reg.receipts() == 30 && reg.phase() == Phase::Delivered,
        &format!("receipts={}", reg.receipts()),
    );

    // 无红账投递拒绝（防投递面空跑报捷）。
    let mut green_only = HealthRegistry::new();
    let mut i = 0usize;
    while i < ACCOUNT_COUNT {
        let name = match ACCOUNT_NAMES.get(i) {
            Some(x) => *x,
            None => break,
        };
        let _ = green_only.hook(AccountSummary { name, health: Health::Green, digest: digest_of(name, Health::Green) });
        i += 1;
    }
    let _ = green_only.finish_hook();
    let no_red = green_only.deliver() == Err(Diag::ENoRed.val());
    s.add_dyn(
        "F1577-10 无红账投递显性拒",
        no_red,
        &format!("no_red={}", no_red),
    );

    // 摘要落档面：十账逐行齐。
    let text = build_registry().text();
    s.add_dyn(
        "F1577-11 摘要落档十行齐",
        text.contains("计费账 红") && text.contains("终验账 绿") && text.matches('\n').count() == ACCOUNT_COUNT,
        &format!("bytes={}", text.len()),
    );

    // 相位倒退显性拒（已投递后挂接口全拒）。
    let back = reg.hook(AccountSummary { name: "退款账", health: Health::Green, digest: 7 }) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1577-12 相位倒退显性拒",
        back && reg.phase().ord() == 2,
        &format!("phase_ord={}", reg.phase().ord()),
    );

    s.add_dyn(
        "F1577-13 双跑一致与注册面对齐",
        dual_run_ok() && REGISTRY_NAME == "F0997" && ALL.len() == 12,
        &format!("registry={} all={}", REGISTRY_NAME, ALL.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_closed() {
        assert_eq!(HEALTH_ALL.len(), 3);
        assert_ne!(Health::Green, Health::Yellow);
        assert_ne!(Health::Yellow, Health::Red);
    }

    #[test]
    fn schema_three_fields() {
        assert!(AccountSummary { name: "计费账", health: Health::Red, digest: 1 }.schema_ok());
        assert!(!AccountSummary { name: "", health: Health::Red, digest: 1 }.schema_ok());
        assert!(!AccountSummary { name: "计费账", health: Health::Red, digest: 0 }.schema_ok());
    }

    #[test]
    fn registry_gates() {
        let mut r = HealthRegistry::new();
        assert_eq!(r.hook(AccountSummary { name: "", health: Health::Green, digest: 1 }), Err(Diag::EName.val()));
        assert_eq!(r.hook(AccountSummary { name: "册外账", health: Health::Green, digest: 1 }), Err(Diag::EName.val()));
        assert_eq!(r.hook(AccountSummary { name: "计费账", health: Health::Red, digest: 0 }), Err(Diag::EDigest.val()));
        assert_eq!(r.rejects(), 3);
        assert_eq!(r.deliver(), Err(Diag::ELack.val()), "未挂接满不可投递");
    }

    #[test]
    fn dup_hook_rejected() {
        let mut r = HealthRegistry::new();
        assert!(r.hook(AccountSummary { name: "计费账", health: Health::Red, digest: 11 }).is_ok());
        assert_eq!(r.hook(AccountSummary { name: "计费账", health: Health::Red, digest: 22 }), Err(Diag::EDupHook.val()));
        assert_eq!(r.hooked(), 1);
    }

    #[test]
    fn baseline_ten_hooked_three_reds() {
        let reg = build_registry();
        assert_eq!(reg.hooked(), ACCOUNT_COUNT);
        assert!(reg.schema_all_ok());
        let reds = reg.reds();
        assert_eq!(reds.len(), 3);
        assert!(reds.contains(&"计费账") && reds.contains(&"客服账") && reds.contains(&"联签账"));
    }

    #[test]
    fn deliver_needs_hooked_phase() {
        let mut reg = build_registry();
        // build_registry 只挂接未收束：未转相投递显性拒。
        assert_eq!(reg.deliver(), Err(Diag::ELack.val()));
        // 未挂接满收束被拒。
        let mut half = HealthRegistry::new();
        let _ = half.hook(AccountSummary { name: "计费账", health: Health::Red, digest: 5 });
        assert_eq!(half.finish_hook(), Err(Diag::ELack.val()));
    }

    #[test]
    fn deliver_receipts_thirty() {
        let mut reg = build_registry();
        assert_eq!(reg.finish_hook(), Ok(()));
        assert_eq!(reg.deliver(), Ok(3 * ROUTE_COUNT as u32));
        assert_eq!(reg.receipts(), 30);
        assert_eq!(reg.phase(), Phase::Delivered);
        assert_eq!(reg.deliver(), Err(Diag::ESealed.val()));
    }

    #[test]
    fn no_red_delivery_rejected() {
        let mut g = HealthRegistry::new();
        let mut i = 0usize;
        while i < ACCOUNT_COUNT {
            let name = match ACCOUNT_NAMES.get(i) {
                Some(x) => *x,
                None => break,
            };
            let _ = g.hook(AccountSummary { name, health: Health::Green, digest: digest_of(name, Health::Green) });
            i += 1;
        }
        assert_eq!(g.finish_hook(), Ok(()));
        assert_eq!(g.deliver(), Err(Diag::ENoRed.val()));
    }

    #[test]
    fn route_ten_of_ten() {
        let m = route_matrix();
        assert_eq!(m, [true; ROUTE_COUNT]);
    }

    #[test]
    fn digest_stable_and_nonzero() {
        let a = digest_of("计费账", Health::Red);
        let b = digest_of("计费账", Health::Red);
        let c = digest_of("计费账", Health::Green);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn text_lines_complete() {
        let reg = build_registry();
        let t = reg.text();
        assert!(t.contains("计费账 红"));
        assert!(t.contains("终验账 绿"));
        assert_eq!(t.matches('\n').count(), ACCOUNT_COUNT);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EName.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x70), "未知诊断码：不在 0x78F41..0x78F4C 闭集内");
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1577_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
