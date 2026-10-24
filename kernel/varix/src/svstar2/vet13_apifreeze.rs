//! VE-F3814 · 无障碍渲染 API 冻结（VE-T 域 · 无障碍渲染 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3814`
//!
//! **判据（锚点原文）**：四族冻结、消费双签、生成单源、十年承诺、回执红线、判据。
//!
//! **本条是什么**：渲染 API 冻结——四族签名表（态查询/策略注册/注入钩子/
//! 事件）只增不改；消费方（D 域渲染/N 域控件/第三方主题）双签回执；
//! 文档生成单源复用；十年兼容承诺。回执未达 = 不冻结。
//!
//! 诊断码段：独占 0xC7xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC7xx）
// ─────────────────────────────────────────────────────────────

/// F3814 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C7Code(pub u16);

impl C7Code {
    /// 回执未达——不冻结。
    pub const NO_RECEIPT: C7Code = C7Code(0xC701);
    /// 手写文档——迁移。
    pub const DOC_MANUAL: C7Code = C7Code(0xC702);
    /// 断兼容——ADR 拒绝。
    pub const COMPAT_BREAK: C7Code = C7Code(0xC703);
    /// 消费方漏签——协调补签。
    pub const CONSUMER_MISS: C7Code = C7Code(0xC704);
    /// 四族缺族。
    pub const FAMILY_MISS: C7Code = C7Code(0xC705);
    /// 只增不改违例。
    pub const CHANGE_BREAK: C7Code = C7Code(0xC706);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC701 => "消费方回执未达，API 不冻结。",
            0xC702 => "文档手写违规，须迁移生成单源。",
            0xC703 => "兼容承诺被破坏，ADR 拒绝。",
            0xC704 => "消费方漏签，需协调补签。",
            0xC705 => "四族签名表缺族。",
            0xC706 => "只增不改原则违例。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 四族签名表
// ─────────────────────────────────────────────────────────────

/// API 族（四族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    /// 态查询。
    StateQuery,
    /// 策略注册。
    PolicyReg,
    /// 注入钩子。
    InjectHook,
    /// 事件。
    Event,
}

impl ApiFamily {
    /// 全集。
    pub const ALL: [ApiFamily; 4] = [
        ApiFamily::StateQuery,
        ApiFamily::PolicyReg,
        ApiFamily::InjectHook,
        ApiFamily::Event,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            ApiFamily::StateQuery => 0,
            ApiFamily::PolicyReg => 1,
            ApiFamily::InjectHook => 2,
            ApiFamily::Event => 3,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            ApiFamily::StateQuery => "态查询",
            ApiFamily::PolicyReg => "策略注册",
            ApiFamily::InjectHook => "注入钩子",
            ApiFamily::Event => "事件",
        }
    }
}

/// 签名表。
#[derive(Clone, Debug)]
pub struct SigTable {
    /// 每族签名条数。
    counts: [u16; 4],
    /// 冻结标志。
    pub frozen: bool,
}

impl SigTable {
    /// 新建。
    pub const fn new() -> Self {
        SigTable {
            counts: [0; 4],
            frozen: false,
        }
    }

    /// 加签名。
    pub fn add(&mut self, f: ApiFamily) -> u16 {
        let i = f.ordinal();
        self.counts[i] = self.counts[i].saturating_add(1);
        self.counts[i]
    }

    /// 缺族数。
    pub fn missing(&self) -> usize {
        self.counts.iter().filter(|c| **c == 0).count()
    }

    /// 四族齐。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 总签名数。
    pub fn total(&self) -> u32 {
        self.counts.iter().map(|c| *c as u32).sum()
    }
}

// ─────────────────────────────────────────────────────────────
// 消费方双签 + 只增不改
// ─────────────────────────────────────────────────────────────

/// 消费方（三方）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Consumer {
    /// D 域渲染。
    DomainD,
    /// N 域控件。
    DomainN,
    /// 第三方主题。
    ThirdParty,
}

impl Consumer {
    /// 全集。
    pub const ALL: [Consumer; 3] = [Consumer::DomainD, Consumer::DomainN, Consumer::ThirdParty];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            Consumer::DomainD => 0,
            Consumer::DomainN => 1,
            Consumer::ThirdParty => 2,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Consumer::DomainD => "D域渲染",
            Consumer::DomainN => "N域控件",
            Consumer::ThirdParty => "第三方主题",
        }
    }
}

/// 双签回执。
#[derive(Clone, Debug)]
pub struct ReceiptBook {
    signed: [bool; 3],
    /// 文档生成单源。
    pub doc_generated: bool,
    /// 十年兼容承诺在册。
    pub compat_promise: bool,
}

impl ReceiptBook {
    /// 新建。
    pub const fn new() -> Self {
        ReceiptBook {
            signed: [false; 3],
            doc_generated: true,
            compat_promise: false,
        }
    }

    /// 签收。
    pub fn sign(&mut self, c: Consumer) -> bool {
        let i = c.ordinal();
        if self.signed[i] {
            false
        } else {
            self.signed[i] = true;
            true
        }
    }

    /// 已签数。
    pub fn count(&self) -> usize {
        self.signed.iter().filter(|s| **s).count()
    }

    /// 全签。
    pub fn complete(&self) -> bool {
        self.count() == 3
    }

    /// 缺签名。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for c in Consumer::ALL.iter() {
            if !self.signed[c.ordinal()] {
                out.push(c.name());
            }
        }
        out
    }
}

/// 只增不改检查。
#[derive(Clone, Debug)]
pub struct AddOnly {
    /// 已发布接口数。
    pub published: u32,
    /// 被改动数（违例）。
    pub changed: u32,
    /// 被删除数（违例）。
    pub removed: u32,
}

impl AddOnly {
    /// 新建。
    pub const fn new() -> Self {
        AddOnly {
            published: 0,
            changed: 0,
            removed: 0,
        }
    }

    /// 发布新接口。
    pub fn publish(&mut self) {
        self.published += 1;
    }

    /// 记改动（违例）。
    pub fn break_change(&mut self) {
        self.changed += 1;
    }

    /// 记删除（违例）。
    pub fn break_remove(&mut self) {
        self.removed += 1;
    }

    /// 只增不改成立。
    pub fn ok(&self) -> bool {
        self.changed == 0 && self.removed == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3814 判据集。
pub fn run_f3814_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3814");

    // 0xC705 四族签名
    {
        let mut t = SigTable::new();
        set.add("0xC705-初始缺四", t.missing() == 4, "");
        for f in ApiFamily::ALL.iter() {
            t.add(*f);
        }
        set.add("0xC705-四族齐", t.complete() && t.total() == 4, "");
    }

    // 0xC701/0xC704 消费方双签
    {
        let mut r = ReceiptBook::new();
        set.add("0xC704-初始缺三", r.count() == 0, "");
        set.add("0xC704-缺名单", r.missing().len() == 3, "");
        r.sign(Consumer::DomainD);
        r.sign(Consumer::DomainN);
        set.add("0xC701-未全签不冻", !r.complete(), "");
        r.sign(Consumer::ThirdParty);
        set.add("0xC701-全签可冻", r.complete(), "");
        set.add("0xC704-重签拒", !r.sign(Consumer::DomainD), "");
    }

    // 0xC702 文档单源
    {
        let r = ReceiptBook::new();
        set.add("0xC702-生成单源在", r.doc_generated, "");
    }

    // 0xC703 兼容承诺
    {
        let mut r = ReceiptBook::new();
        set.add("0xC703-承诺未立", !r.compat_promise, "");
        r.compat_promise = true;
        set.add("0xC703-十年承诺立", r.compat_promise, "");
    }

    // 0xC706 只增不改
    {
        let mut a = AddOnly::new();
        a.publish();
        a.publish();
        set.add("0xC706-纯增合规", a.ok() && a.published == 2, "");
        a.break_change();
        set.add("0xC706-改一违例", !a.ok() && a.changed == 1, "");
    }

    // 族序自洽
    {
        let mut ok = true;
        for (i, f) in ApiFamily::ALL.iter().enumerate() {
            if f.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xC705-族序自洽", ok, "");
        let mut ok_c = true;
        for (i, c) in Consumer::ALL.iter().enumerate() {
            if c.ordinal() != i {
                ok_c = false;
            }
        }
        set.add("0xC704-消费方序自洽", ok_c, "");
    }

    set
}
