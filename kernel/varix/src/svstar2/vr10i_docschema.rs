//! VE-F3789 · R 域文档与 schema 总审（R10 收口组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3789`
//!
//! **判据（锚点原文）**：六对象零废、版本四步、对端对表、README 主入口、
//! 术语漂移、判据。
//!
//! **本条是什么**：文档与 schema 总审——六对象（创建请求/工作流/资产/
//! 发布/质量/市场）零废文档率；版本四项双档推进；迁移指南；对端 README
//! 对表；README 主入口唯一；术语漂移零容忍。
//!
//! 诊断码段：独占 0xB4xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB4xx）
// ─────────────────────────────────────────────────────────────

/// F3789 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B4Code(pub u16);

impl B4Code {
    /// 对象废——修复（红线）。
    pub const OBJ_STALE: B4Code = B4Code(0xB401);
    /// 版本缺档——补。
    pub const VER_MISS: B4Code = B4Code(0xB402);
    /// 对端断裂——修链。
    pub const PEER_BREAK: B4Code = B4Code(0xB403);
    /// 主入口多/缺——唯一。
    pub const ENTRY_BAD: B4Code = B4Code(0xB404);
    /// 术语漂移——零容忍。
    pub const TERM_DRIFT: B4Code = B4Code(0xB405);
    /// 六对象缺对象。
    pub const OBJ_MISS: B4Code = B4Code(0xB406);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB401 => "文档对象过期，红线修复。",
            0xB402 => "版本档缺失，需补齐。",
            0xB403 => "对端 README 链断裂。",
            0xB404 => "主入口非唯一，需收敛。",
            0xB405 => "术语漂移检出，零容忍。",
            0xB406 => "六对象缺对象。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 六对象文档
// ─────────────────────────────────────────────────────────────

/// 文档对象（六对象）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocObject {
    /// 创建请求。
    CreateReq,
    /// 工作流。
    Workflow,
    /// 资产。
    Asset,
    /// 发布。
    Publish,
    /// 质量。
    Quality,
    /// 市场。
    Market,
}

impl DocObject {
    /// 全集。
    pub const ALL: [DocObject; 6] = [
        DocObject::CreateReq,
        DocObject::Workflow,
        DocObject::Asset,
        DocObject::Publish,
        DocObject::Quality,
        DocObject::Market,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            DocObject::CreateReq => 0,
            DocObject::Workflow => 1,
            DocObject::Asset => 2,
            DocObject::Publish => 3,
            DocObject::Quality => 4,
            DocObject::Market => 5,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            DocObject::CreateReq => "创建请求",
            DocObject::Workflow => "工作流",
            DocObject::Asset => "资产",
            DocObject::Publish => "发布",
            DocObject::Quality => "质量",
            DocObject::Market => "市场",
        }
    }
}

/// 文档状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocState {
    /// 缺失。
    Absent,
    /// 在册有效。
    Fresh,
    /// 过期（废）。
    Stale,
}

/// 文档账。
#[derive(Clone, Debug)]
pub struct DocLedger {
    states: [DocState; 6],
}

impl DocLedger {
    /// 新建。
    pub const fn new() -> Self {
        DocLedger {
            states: [DocState::Absent; 6],
        }
    }

    /// 置状态。
    pub fn set(&mut self, o: DocObject, s: DocState) {
        self.states[o.ordinal()] = s;
    }

    /// 废对象数。
    pub fn stale_count(&self) -> usize {
        self.states
            .iter()
            .filter(|s| **s == DocState::Stale)
            .count()
    }

    /// 缺对象数。
    pub fn absent_count(&self) -> usize {
        self.states
            .iter()
            .filter(|s| **s == DocState::Absent)
            .count()
    }

    /// 零废（无 Stale）。
    pub fn zero_stale(&self) -> bool {
        self.stale_count() == 0
    }

    /// 六对象全在（无 Absent）。
    pub fn complete(&self) -> bool {
        self.absent_count() == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 版本四步双档 + 迁移指南
// ─────────────────────────────────────────────────────────────

/// schema 版本。
#[derive(Clone, Copy, Debug)]
pub struct SchemaVer {
    /// 双档版本号（doc/json schema）。
    pub doc_ver: u16,
    pub json_ver: u16,
    /// 四步：draft→review→frozen→published。
    pub step: u8,
    /// 迁移指南在册。
    pub migration: bool,
}

impl SchemaVer {
    /// 新建 draft。
    pub const fn new() -> Self {
        SchemaVer {
            doc_ver: 0,
            json_ver: 0,
            step: 0,
            migration: false,
        }
    }

    /// 推进一步（须双档版本都非零）。
    pub fn advance(&mut self) -> bool {
        if self.doc_ver == 0 || self.json_ver == 0 {
            return false;
        }
        if self.step < 3 {
            self.step += 1;
            true
        } else {
            false
        }
    }

    /// 双档同步。
    pub fn dual_synced(&self) -> bool {
        self.doc_ver == self.json_ver && self.doc_ver > 0
    }

    /// 版本步名。
    pub fn step_name(self) -> &'static str {
        match self.step {
            0 => "draft",
            1 => "review",
            2 => "frozen",
            3 => "published",
            _ => "unknown",
        }
    }

    /// 已发布。
    pub fn published(&self) -> bool {
        self.step == 3
    }
}

// ─────────────────────────────────────────────────────────────
// 主入口唯一 + 术语漂移 + 对端对表
// ─────────────────────────────────────────────────────────────

/// 入口审计。
#[derive(Clone, Debug)]
pub struct EntryAudit {
    /// README 主入口数（须 1）。
    pub main_readmes: u32,
    /// 术语表（词→权威写法 id）。
    canon: Vec<(u16, u16)>,
    /// 漂移数。
    pub drifts: u32,
}

impl EntryAudit {
    /// 新建。
    pub fn new() -> Self {
        EntryAudit {
            main_readmes: 0,
            canon: Vec::new(),
            drifts: 0,
        }
    }

    /// 主入口唯一。
    pub fn entry_unique(&self) -> bool {
        self.main_readmes == 1
    }

    /// 登记权威词。
    pub fn canon_add(&mut self, word: u16, form: u16) {
        self.canon.push((word, form));
    }

    /// 校验写法（与权威不符 = 漂移）。
    pub fn check_form(&mut self, word: u16, form: u16) -> bool {
        for (w, f) in self.canon.iter() {
            if *w == word {
                if *f == form {
                    return true;
                } else {
                    self.drifts += 1;
                    return false;
                }
            }
        }
        false
    }

    /// 零漂移。
    pub fn zero_drift(&self) -> bool {
        self.drifts == 0
    }
}

impl Default for EntryAudit {
    fn default() -> Self {
        EntryAudit::new()
    }
}

/// 对端对表。
#[derive(Clone, Debug)]
pub struct PeerTable {
    /// 五域对端 README 在册。
    ack: [bool; 5],
}

impl PeerTable {
    /// 新建。
    pub const fn new() -> Self {
        PeerTable { ack: [false; 5] }
    }

    /// 接对端（0..5）。
    pub fn link(&mut self, idx: usize) -> bool {
        if idx < 5 {
            self.ack[idx] = true;
            true
        } else {
            false
        }
    }

    /// 断链数。
    pub fn broken(&self) -> usize {
        self.ack.iter().filter(|a| !**a).count()
    }

    /// 全接。
    pub fn complete(&self) -> bool {
        self.broken() == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3789 判据集。
pub fn run_vr10i_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3789");

    // 0xB406/0xB401 六对象零废
    {
        let mut d = DocLedger::new();
        set.add("0xB406-初始缺六", d.absent_count() == 6, "");
        for o in DocObject::ALL.iter() {
            d.set(*o, DocState::Fresh);
        }
        set.add("0xB406-六对象齐", d.complete(), "");
        set.add("0xB401-零废", d.zero_stale(), "");
        d.set(DocObject::Market, DocState::Stale);
        set.add("0xB401-废检出", !d.zero_stale() && d.stale_count() == 1, "");
    }

    // 0xB402 版本四步
    {
        let mut v = SchemaVer::new();
        set.add("0xB402-零版本不可进", !v.advance(), "");
        v.doc_ver = 3;
        v.json_ver = 3;
        set.add("0xB402-双档同步", v.dual_synced(), "");
        set.add("0xB402-进review", v.advance() && v.step == 1, "");
        set.add("0xB402-进frozen", v.advance() && v.step == 2, "");
        set.add("0xB402-进published", v.advance() && v.published(), "");
        set.add("0xB402-步满拒进", !v.advance(), "");
        set.add("0xB402-步名", v.step_name() == "published", "");
    }

    // 双档异步不进
    {
        let mut v = SchemaVer::new();
        v.doc_ver = 1;
        v.json_ver = 2;
        set.add("0xB402-双档异步", !v.dual_synced(), "");
    }

    // 0xB404 主入口唯一
    {
        let mut e = EntryAudit::new();
        set.add("0xB404-零入口不唯一", !e.entry_unique(), "");
        e.main_readmes = 2;
        set.add("0xB404-多入口不唯一", !e.entry_unique(), "");
        e.main_readmes = 1;
        set.add("0xB404-唯一过", e.entry_unique(), "");
    }

    // 0xB405 术语漂移
    {
        let mut e = EntryAudit::new();
        e.canon_add(1, 50);
        set.add("0xB405-同写法过", e.check_form(1, 50), "");
        set.add("0xB405-异写法漂移", !e.check_form(1, 51), "");
        set.add("0xB405-漂移计数", e.drifts == 1 && !e.zero_drift(), "");
    }

    // 0xB403 对端对表
    {
        let mut p = PeerTable::new();
        set.add("0xB403-初始断五", p.broken() == 5, "");
        for i in 0..5 {
            p.link(i);
        }
        set.add("0xB403-全接", p.complete() && p.broken() == 0, "");
        set.add("0xB403-越界拒", !p.link(5), "");
    }

    // 对象序自洽
    {
        let mut ok = true;
        for (i, o) in DocObject::ALL.iter().enumerate() {
            if o.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB406-对象序自洽", ok, "");
    }

    set
}
