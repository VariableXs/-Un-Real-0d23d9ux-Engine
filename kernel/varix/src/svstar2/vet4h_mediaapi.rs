#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xD980）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD980;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CD9BCode(pub u16);

impl CD9BCode {
    pub const FREEZE_NOACK: Self = Self(SEG_BASE);     // 回执未达
    pub const DOC_MANUAL: Self = Self(SEG_BASE + 1);   // 手写文档
    pub const COMPAT_BREAK: Self = Self(SEG_BASE + 2); // 断兼容
    pub const CONSUMER_MISS: Self = Self(SEG_BASE + 3); // 消费漏
    pub const SIGN_MISMATCH: Self = Self(SEG_BASE + 4); // 签名不一致

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::FREEZE_NOACK => "回执未达",
            Self::DOC_MANUAL => "手写文档",
            Self::COMPAT_BREAK => "断兼容",
            Self::CONSUMER_MISS => "消费漏",
            Self::SIGN_MISMATCH => "签名不一致",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::FREEZE_NOACK, Self::DOC_MANUAL, Self::COMPAT_BREAK,
        Self::CONSUMER_MISS, Self::SIGN_MISMATCH,
    ];
}

// ── API 四族签名 ────────────────────────────────────────────────────

/// API 族分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiFamily {
    /// 字幕注册。
    SubtitleRegister,
    /// 描述注册。
    DescriptionRegister,
    /// 控制代理。
    ControlProxy,
    /// 状态查询。
    StatusQuery,
}

impl ApiFamily {
    pub const ALL: [Self; 4] = [Self::SubtitleRegister, Self::DescriptionRegister, Self::ControlProxy, Self::StatusQuery];

    pub fn name(self) -> &'static str {
        match self {
            Self::SubtitleRegister => "字幕注册",
            Self::DescriptionRegister => "描述注册",
            Self::ControlProxy => "控制代理",
            Self::StatusQuery => "状态查询",
        }
    }
}

/// API 签名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiSignature {
    pub family: ApiFamily,
    pub name: &'static str,
    pub params: Vec<&'static str>,
    pub frozen: bool,
}

impl ApiSignature {
    pub fn new(family: ApiFamily, name: &'static str, params: Vec<&'static str>) -> Self {
        Self { family, name, params, frozen: false }
    }

    /// 冻结签名。
    pub fn freeze(&mut self) { self.frozen = true; }

    /// 签名一致（版本兼容断言）。
    pub fn compatible_with(&self, other: &ApiSignature) -> bool {
        self.name == other.name && self.params == other.params
    }
}

// ── 消费双签 ────────────────────────────────────────────────────────

/// 消费方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumer {
    pub id: u32,
    pub name: &'static str,
    pub signed: bool,
}

/// 双签协议：API 提供方 + 消费方双方签字。
#[derive(Debug, Clone)]
pub struct DualSign {
    pub provider_signed: bool,
    pub consumers: Vec<Consumer>,
}

impl DualSign {
    pub fn new() -> Self { Self { provider_signed: false, consumers: Vec::new() } }

    /// 双签完成：提供方签 + 至少一消费方签。
    pub fn complete(&self) -> bool {
        self.provider_signed && self.consumers.iter().any(|c| c.signed)
    }

    /// 消费漏审计：未签消费方清单。
    pub fn unsigned(&self) -> Vec<&Consumer> {
        self.consumers.iter().filter(|c| !c.signed).collect()
    }
}

// ── 文档生成 ────────────────────────────────────────────────────────

/// 文档来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocSource { Generated, Manual }

impl DocSource {
    /// 硬门：文档必须由接口生成（手写禁止）。
    pub fn hardgate_ok(self) -> bool { self == Self::Generated }
}

/// API 文档。
#[derive(Debug, Clone)]
pub struct ApiDoc {
    pub source: DocSource,
    pub families_covered: Vec<ApiFamily>,
}

impl ApiDoc {
    /// 覆盖全部四族。
    pub fn covers_all(&self) -> bool {
        ApiFamily::ALL.iter().all(|f| self.families_covered.contains(f))
    }
}

// ── 冻结状态机 ──────────────────────────────────────────────────────

/// API 冻结状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeState { Draft, Pending, Frozen }

/// API 冻结流程。
#[derive(Debug, Clone)]
pub struct ApiFreezeProcess {
    pub state: FreezeState,
    pub signatures: Vec<ApiSignature>,
    pub dual_sign: DualSign,
    pub doc: ApiDoc,
}

impl ApiFreezeProcess {
    pub fn new() -> Self {
        Self {
            state: FreezeState::Draft,
            signatures: Vec::new(),
            dual_sign: DualSign::new(),
            doc: ApiDoc { source: DocSource::Generated, families_covered: Vec::new() },
        }
    }

    /// 提交冻结申请（须全部签名 frozen + 双签完成 + 文档生成）。
    pub fn freeze(&mut self) -> bool {
        if self.state != FreezeState::Draft {
            return false;
        }
        let sigs_frozen = !self.signatures.is_empty() && self.signatures.iter().all(|s| s.frozen);
        let ds_ok = self.dual_sign.complete();
        let doc_ok = self.doc.source.hardgate_ok() && self.doc.covers_all();
        if sigs_frozen && ds_ok && doc_ok {
            self.state = FreezeState::Frozen;
            true
        } else {
            self.state = FreezeState::Pending;
            false
        }
    }

    /// 回执红线：未达回执不冻结。
    pub fn freeze_with_ack(&mut self, ack_received: bool) -> bool {
        if !ack_received { return false; }
        self.freeze()
    }
}

// ── 兼容承诺 ────────────────────────────────────────────────────────

/// 十年兼容承诺。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompatPledge {
    pub years: u8,
    pub adr_required_on_break: bool,
}

impl CompatPledge {
    pub const STANDARD: Self = Self { years: 10, adr_required_on_break: true };

    /// 断兼容必须 ADR。
    pub fn break_ok_with_adr(&self, has_adr: bool) -> bool {
        self.adr_required_on_break && has_adr
    }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3870_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4h_mediaapi");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 四族齐全
    {
        chk(&mut set, "C3870-01", ApiFamily::ALL.len() == 4, "err");
    }
    // 2 族名
    {
        chk(&mut set, "C3870-02", ApiFamily::ControlProxy.name() == "控制代理", "err");
    }
    // 3 签名冻结
    {
        let mut s = ApiSignature::new(ApiFamily::SubtitleRegister, "register_sub", vec!["track", "lang"]);
        s.freeze();
        chk(&mut set, "C3870-03", s.frozen, "err");
    }
    // 4 签名兼容
    {
        let a = ApiSignature::new(ApiFamily::StatusQuery, "get_status", vec!["id"]);
        let b = ApiSignature::new(ApiFamily::StatusQuery, "get_status", vec!["id"]);
        chk(&mut set, "C3870-04", a.compatible_with(&b), "err");
    }
    // 5 签名不兼容
    {
        let a = ApiSignature::new(ApiFamily::StatusQuery, "get_status", vec!["id"]);
        let b = ApiSignature::new(ApiFamily::StatusQuery, "get_status", vec!["id", "verbose"]);
        chk(&mut set, "C3870-05", !a.compatible_with(&b), "err");
    }
    // 6 双签完成
    {
        let mut ds = DualSign::new();
        ds.provider_signed = true;
        ds.consumers.push(Consumer { id: 1, name: "player", signed: true });
        chk(&mut set, "C3870-06", ds.complete(), "err");
    }
    // 7 双签不完整
    {
        let mut ds = DualSign::new();
        ds.provider_signed = true;
        ds.consumers.push(Consumer { id: 1, name: "player", signed: false });
        chk(&mut set, "C3870-07", !ds.complete(), "err");
    }
    // 8 消费漏审计
    {
        let mut ds = DualSign::new();
        ds.provider_signed = true;
        ds.consumers.push(Consumer { id: 1, name: "a", signed: true });
        ds.consumers.push(Consumer { id: 2, name: "b", signed: false });
        chk(&mut set, "C3870-08", ds.unsigned().len() == 1, "err");
    }
    // 9 文档硬门
    {
        chk(&mut set, "C3870-09", DocSource::Generated.hardgate_ok(), "err");
    }
    // 10 手写文档不合格
    {
        chk(&mut set, "C3870-10", !DocSource::Manual.hardgate_ok(), "err");
    }
    // 11 文档覆盖四族
    {
        let d = ApiDoc { source: DocSource::Generated, families_covered: ApiFamily::ALL.to_vec() };
        chk(&mut set, "C3870-11", d.covers_all(), "err");
    }
    // 12 文档缺族
    {
        let d = ApiDoc { source: DocSource::Generated, families_covered: vec![ApiFamily::SubtitleRegister] };
        chk(&mut set, "C3870-12", !d.covers_all(), "err");
    }
    // 13 冻结失败（无签名）
    {
        let mut p = ApiFreezeProcess::new();
        chk(&mut set, "C3870-13", !p.freeze(), "err");
    }
    // 14 冻结成功
    {
        let mut p = ApiFreezeProcess::new();
        for f in ApiFamily::ALL {
            let mut s = ApiSignature::new(f, "api", vec!["x"]);
            s.freeze();
            p.signatures.push(s);
        }
        p.dual_sign.provider_signed = true;
        p.dual_sign.consumers.push(Consumer { id: 1, name: "c", signed: true });
        p.doc.families_covered = ApiFamily::ALL.to_vec();
        chk(&mut set, "C3870-14", p.freeze(), "err");
    }
    // 15 回执红线
    {
        let mut p = ApiFreezeProcess::new();
        for f in ApiFamily::ALL {
            let mut s = ApiSignature::new(f, "api", vec!["x"]);
            s.freeze();
            p.signatures.push(s);
        }
        p.dual_sign.provider_signed = true;
        p.dual_sign.consumers.push(Consumer { id: 1, name: "c", signed: true });
        p.doc.families_covered = ApiFamily::ALL.to_vec();
        chk(&mut set, "C3870-15", !p.freeze_with_ack(false), "err");
    }
    // 16 兼容承诺十年
    {
        chk(&mut set, "C3870-16", CompatPledge::STANDARD.years == 10, "err");
    }
    // 17 断兼容须 ADR
    {
        chk(&mut set, "C3870-17", CompatPledge::STANDARD.break_ok_with_adr(true), "err");
    }
    // 18 断兼容无 ADR 拒绝
    {
        chk(&mut set, "C3870-18", !CompatPledge::STANDARD.break_ok_with_adr(false), "err");
    }

    set
}
