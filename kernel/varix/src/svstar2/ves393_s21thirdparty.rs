//! ves393_s21thirdparty — VE-F4153 · 第三方 i18n 工具接入
//! 第三方工具接入面三类适配规范（CAT 工具/机翻引擎/审查服务）+ 接入四步流程（申报→沙箱验证→契约签→上架）
//! + 接入黑名单机制（违规工具可撤）；沙箱验证失败→拒绝+列因；越权行为→拉黑+通告；接口漂移→对版+限期；
//! 出站白名单审（用户内容永不外发——红线实测）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xECC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TpCode {
    Ok = 0,
    SandboxFail = SEG_BASE + 0x01,
    Blacklisted = SEG_BASE + 0x02,
    ApiDrift = SEG_BASE + 0x03,
    OutboundBreach = SEG_BASE + 0x04,
}

/// 三类适配规范。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolKind { Cat = 0, MtEngine = 1, ReviewService = 2 }

impl ToolKind {
    pub const ALL: [ToolKind; 3] = [ToolKind::Cat, ToolKind::MtEngine, ToolKind::ReviewService];
    pub fn name(self) -> &'static str {
        match self {
            ToolKind::Cat => "cat", ToolKind::MtEngine => "mt-engine", ToolKind::ReviewService => "review-svc",
        }
    }
}

/// 接入四步。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Step { Declare = 0, SandboxVerify = 1, ContractSign = 2, Listed = 3 }

impl Step {
    pub fn name(self) -> &'static str {
        match self {
            Step::Declare => "declare", Step::SandboxVerify => "sandbox-verify",
            Step::ContractSign => "contract-sign", Step::Listed => "listed",
        }
    }
}

/// 接入工具：类 + 当前步 + 接口版本 + 出站白名单审。
pub struct Tool {
    pub name: &'static str,
    pub kind: ToolKind,
    pub step: Step,
    pub api_version: u32,
    pub outbound_whitelisted: bool, // 用户内容永不外发（红线）
}

impl Tool {
    /// 出站红线实测：未过白名单审 = 违规。
    pub fn redline_ok(&self) -> bool { self.outbound_whitelisted }

    /// 沙箱验证：过线则进契约步，失败拒绝+列因。
    pub fn sandbox_pass(&mut self, ok: bool) -> Result<&'static str, TpCode> {
        if self.step != Step::Declare { return Err(TpCode::SandboxFail); }
        if !ok { return Err(TpCode::SandboxFail); }
        self.step = Step::ContractSign;
        Ok("sandbox-clean")
    }

    pub fn sign(&mut self, version: u32) -> bool {
        if self.step == Step::ContractSign && self.redline_ok() && version > 0 {
            self.api_version = version;
            self.step = Step::Listed;
            true
        } else { false }
    }
}

/// 接入册：在册工具 + 黑名单 + 通告。
pub struct OnboardBook {
    pub tools: Vec<Tool>,
    pub blacklist: Vec<&'static str>,
    pub notices: Vec<&'static str>,
    pub deadlines: Vec<(&'static str, u32)>, // 对版限期（工具→期限版本）
}

impl OnboardBook {
    pub fn new() -> Self { OnboardBook { tools: Vec::new(), blacklist: Vec::new(), notices: Vec::new(), deadlines: Vec::new() } }

    pub fn declare(&mut self, name: &'static str, kind: ToolKind) -> bool {
        if self.blacklist.contains(&name) || self.tools.iter().any(|t| t.name == name) { return false; }
        self.tools.push(Tool { name, kind, step: Step::Declare, api_version: 0, outbound_whitelisted: false });
        true
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Tool> {
        self.tools.iter_mut().find(|t| t.name == name)
    }
    pub fn get(&self, name: &str) -> Option<&Tool> { self.tools.iter().find(|t| t.name == name) }

    /// 上架四步齐备核验。
    pub fn listed(&self, name: &str) -> bool {
        self.get(name).map_or(false, |t| t.step == Step::Listed)
    }

    /// 拉黑+通告：违规工具可撤。
    pub fn blacklist_tool(&mut self, name: &'static str, why: &'static str) -> bool {
        if !self.tools.iter().any(|t| t.name == name) { return false; }
        if !self.blacklist.contains(&name) { self.blacklist.push(name); }
        self.tools.retain(|t| t.name != name);
        if !self.notices.contains(&why) { self.notices.push(why); }
        true
    }

    /// 黑名单拒申报。
    pub fn declare_blocked(&self, name: &str) -> bool { self.blacklist.contains(&name) }

    /// 接口漂移：上架工具版本低于当前契约版 → 对版+限期。
    pub fn audit_drift(&mut self, current: u32) -> Vec<&'static str> {
        let mut drifted: Vec<&'static str> = Vec::new();
        for t in self.tools.iter() {
            if t.step == Step::Listed && t.api_version < current {
                drifted.push(t.name);
                if !self.deadlines.iter().any(|(n, _)| *n == t.name) {
                    self.deadlines.push((t.name, current));
                }
            }
        }
        drifted
    }

    /// 三类覆盖。
    pub fn kinds_covered(&self) -> bool {
        ToolKind::ALL.iter().all(|k| self.tools.iter().any(|t| t.kind == *k))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves393_checks() -> CheckSet {
    let mut set = CheckSet::new("ves393");
    let mut book = OnboardBook::new();

    // C393-01 三类申报。
    book.declare("cat-a", ToolKind::Cat);
    book.declare("mt-b", ToolKind::MtEngine);
    book.declare("rv-c", ToolKind::ReviewService);
    chk(&mut set, "C393-01", book.tools.len() == 3 && book.kinds_covered(), "三类齐申报");

    // C393-02 四步流程：申报→沙箱→契约→上架。
    book.get_mut("cat-a").unwrap().outbound_whitelisted = true;
    book.get_mut("cat-a").unwrap().sandbox_pass(true).unwrap();
    chk(&mut set, "C393-02", book.get("cat-a").unwrap().step == Step::ContractSign, "沙箱过进契约");

    // C393-03 契约签+上架（白名单审先决）。
    chk(&mut set, "C393-03", book.get_mut("cat-a").unwrap().sign(2) && book.listed("cat-a"), "四步上架");

    // C393-04 沙箱失败 → 拒绝+列因。
    let r = book.get_mut("mt-b").unwrap().sandbox_pass(false);
    chk(&mut set, "C393-04", r == Err(TpCode::SandboxFail)
        && book.get("mt-b").unwrap().step == Step::Declare, "沙箱拒列因");

    // C393-05 出站红线：未过白名单审不得签。
    book.get_mut("rv-c").unwrap().sandbox_pass(true).unwrap();
    chk(&mut set, "C393-05", !book.get_mut("rv-c").unwrap().sign(1)
        && book.get("rv-c").unwrap().step == Step::ContractSign, "出站红线拦签");

    // C393-06 补过白名单审后可签。
    book.get_mut("rv-c").unwrap().outbound_whitelisted = true;
    chk(&mut set, "C393-06", book.get_mut("rv-c").unwrap().sign(1) && book.listed("rv-c"), "白名单后上架");

    // C393-07 越权 → 拉黑+通告（违规工具可撤）。
    chk(&mut set, "C393-07", book.blacklist_tool("mt-b", "越权写用户库")
        && book.tools.iter().all(|t| t.name != "mt-b")
        && book.notices == vec!["越权写用户库"], "拉黑+通告");

    // C393-08 黑名单拒再申报。
    chk(&mut set, "C393-08", book.declare_blocked("mt-b") && !book.declare("mt-b", ToolKind::MtEngine), "黑名单拒申报");

    // C393-09 接口漂移：契约升 3，v2 工具限期对版。
    let drifted = book.audit_drift(3);
    chk(&mut set, "C393-09", drifted == vec!["cat-a", "rv-c"]
        && book.deadlines.len() == 2, "漂移二限期");

    // C393-10 对版复验：升级后限期清单清。
    book.get_mut("cat-a").unwrap().api_version = 3;
    book.deadlines.retain(|(n, _)| *n != "cat-a");
    chk(&mut set, "C393-10", book.get("cat-a").unwrap().api_version == 3
        && book.deadlines.len() == 1, "对版一撤限期");

    // C393-11 三类+四步名单单源。
    chk(&mut set, "C393-11", ToolKind::ALL.len() == 3 && ToolKind::ALL[0].name() == "cat"
        && Step::Declare < Step::Listed && Step::Listed.name() == "listed", "类型+步骤单源");

    // C393-12 重复申报拒。
    chk(&mut set, "C393-12", !book.declare("cat-a", ToolKind::Cat), "重名拒申报");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves393_smoke() {
        let set = run_ves393_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves393 red: {:?}", red);
    }

    #[test]
    fn ves393_redline_direct() {
        let t = Tool { name: "x", kind: ToolKind::Cat, step: Step::Declare, api_version: 0, outbound_whitelisted: false };
        assert!(!t.redline_ok());
    }
}
