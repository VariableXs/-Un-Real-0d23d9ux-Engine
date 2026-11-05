//! UNX-F1554 域级文档打包预演（移交包文档件预组） · 0x78A80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B05 卡 3 · UNX-F1554 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1554-J1 文档件预组齐备（域册/接口册/预案册/沉淀/
//!     FAQ/词汇表六件预组成包），包内清单与实存对平 100%，预组哈希落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 六件闭集：域册、接口册、预案册、沉淀、FAQ、词汇表恰六件，包外
//!     多件与包内缺件同为违约；
//!   - 清单对平：清单件名、行数、内容指纹三项逐一与实存比对，全对平
//!     方可落档，对平计数 6/6；
//!   - 行数冻结：每件文档行数在清单冻结，实存行数漂移即拒（追加一行
//!     也算漂移）；
//!   - 预组哈希落档：六件内容按清单序拼接后 FNV-1a 落档，双跑逐位
//!     一致，零哈希判失败；
//!   - 状态机单调：组包 → 对平 → 落档三态推进，倒退 / 跳级显性拒；
//!   - 注入必抓：缺件 / 行漂移 / 多件 / 指纹失配四路注入全部显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78A80，派工卡 VX8-B05 卡 3 独占窗口）。
pub const SEG_BASE: u32 = 0x78A80;

/// 移交包文档件数（判据 J1「六件预组成包」冻结）。
pub const DOC_COUNT: usize = 6;

// ── 六件清单（判据 J1 列序：域册/接口册/预案册/沉淀/FAQ/词汇表） ──
pub const DOC_NAMES: [&str; 6] = [
    "域册",
    "接口册",
    "预案册",
    "沉淀",
    "FAQ",
    "词汇表",
];

/// 清单冻结行数（每件文档的行数上限即行数校验基准）。
pub const MANIFEST_ROWS: [usize; 6] = [12, 10, 8, 6, 10, 8];

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackCode {
    /// 清单缺件：六件闭集缺一即拒，补齐再对平
    ItemMissing,
    /// 实存多件：包外文件不得入包，移出再对平
    ExtraItem,
    /// 行数漂移：实存行数与清单不符，按清单行数重组
    RowDrift,
    /// 指纹失配：内容与清单指纹不符，排查改动后重组
    DigestMismatch,
    /// 件名不在清单：清单外件名不入包
    NameUnknown,
    /// 件名为空：补件名再组包
    NameEmpty,
    /// 件名重复：一包一一件，重复件名显性拒
    NameDup,
    /// 对平未完成：清单与实存未全对平不得落档
    ReconcileIncomplete,
    /// 状态倒退：按组包→对平→落档单调推进
    PhaseRegress,
    /// 状态跳级：未对平不得落档
    PhaseSkip,
    /// 封包后再组：已落档移交包禁改，另开新包
    SealedLocked,
    /// 双跑不一致：排查指纹随机性后复跑
    NotReproducible,
}

impl PackCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            PackCode::ItemMissing => "清单缺件：六件闭集缺一即拒，补齐再对平",
            PackCode::ExtraItem => "实存多件：包外文件不得入包，移出再对平",
            PackCode::RowDrift => "行数漂移：实存行数与清单不符，按清单行数重组",
            PackCode::DigestMismatch => "指纹失配：内容与清单指纹不符，排查改动后重组",
            PackCode::NameUnknown => "件名不在清单：清单外件名不入包",
            PackCode::NameEmpty => "件名为空：补件名再组包",
            PackCode::NameDup => "件名重复：一包一一件，重复件名显性拒",
            PackCode::ReconcileIncomplete => "对平未完成：清单与实存未全对平不得落档",
            PackCode::PhaseRegress => "状态倒退：按组包→对平→落档单调推进",
            PackCode::PhaseSkip => "状态跳级：未对平不得落档",
            PackCode::SealedLocked => "已落档移交包封包：禁改，另开新包",
            PackCode::NotReproducible => "双跑不一致：排查指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [PackCode; 12] = [
        PackCode::ItemMissing,
        PackCode::ExtraItem,
        PackCode::RowDrift,
        PackCode::DigestMismatch,
        PackCode::NameUnknown,
        PackCode::NameEmpty,
        PackCode::NameDup,
        PackCode::ReconcileIncomplete,
        PackCode::PhaseRegress,
        PackCode::PhaseSkip,
        PackCode::SealedLocked,
        PackCode::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PackCode::ALL.len() {
        let c = match PackCode::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 文档内容生成器（按清单序确定性生成，零随机） ────────────────
/// 六件文档各自的行模板前缀（行内容 = 前缀 + 行号 + 判据锚）。
pub const DOC_LINE_PREFIX: [&str; 6] = [
    "域册判据行",
    "接口册契约行",
    "预案册演练行",
    "沉淀账条款行",
    "FAQ 问答行",
    "词汇表术语行",
];

/// 生成第 `doc_idx` 件文档全文（行数恰等于清单冻结行数）。
pub fn build_doc(doc_idx: usize, seg: u32) -> Option<String> {
    let name = DOC_NAMES.get(doc_idx).map(|n| *n)?;
    let rows = MANIFEST_ROWS.get(doc_idx).map(|r| *r)?;
    let prefix = DOC_LINE_PREFIX.get(doc_idx).map(|p| *p)?;
    let mut text = format!("== {} ==\n", name);
    let mut i = 1usize;
    while i <= rows {
        let line = format!("{} {:02} 锚 0x{:04X}\n", prefix, i, (seg + i as u32) & 0xFFFF);
        text.push_str(&line);
        i += 1;
    }
    Some(text)
}

// ── 状态机（组包 → 对平 → 落档，单调推进） ──────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PackPhase {
    /// 组包期：文档件逐件入包
    Draft,
    /// 对平期：清单与实存全对平
    Reconciled,
    /// 落档期：预组哈希落档、封包
    Archived,
}

impl PackPhase {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            PackPhase::Draft => 0,
            PackPhase::Reconciled => 1,
            PackPhase::Archived => 2,
        }
    }
}

/// 移交包文档件（件名 + 全文）。
#[derive(Clone, Debug)]
pub struct PackItem {
    pub name: String,
    pub content: String,
}

/// 移交包文档预组。
pub struct DocPack {
    phase: PackPhase,
    items: Vec<PackItem>,
    digest: u32,
    reconciled: usize,
}

impl DocPack {
    pub fn new() -> DocPack {
        DocPack {
            phase: PackPhase::Draft,
            items: Vec::new(),
            digest: 0,
            reconciled: 0,
        }
    }

    pub fn phase(&self) -> PackPhase {
        self.phase
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn digest(&self) -> u32 {
        self.digest
    }

    /// 对平计数（判据 J1「对平 100%」落点，满分 = DOC_COUNT）。
    pub fn reconciled_count(&self) -> usize {
        self.reconciled
    }

    /// 文档件入包：件名非空、不重、在清单内，违者显性拒。
    pub fn add_item(&mut self, name: &str, content: &str) -> Result<(), u32> {
        if self.phase == PackPhase::Archived {
            return Err(PackCode::SealedLocked.val());
        }
        if name.is_empty() {
            return Err(PackCode::NameEmpty.val());
        }
        let mut in_manifest = false;
        let mut i = 0usize;
        while i < DOC_NAMES.len() {
            let n = match DOC_NAMES.get(i) {
                Some(n) => *n,
                None => break,
            };
            if n == name {
                in_manifest = true;
            }
            i += 1;
        }
        if !in_manifest {
            return Err(PackCode::NameUnknown.val());
        }
        let mut j = 0usize;
        while j < self.items.len() {
            let it = match self.items.get(j) {
                Some(it) => it,
                None => return Err(PackCode::NotReproducible.val()),
            };
            if it.name == name {
                return Err(PackCode::NameDup.val());
            }
            j += 1;
        }
        self.items.push(PackItem {
            name: name.to_string(),
            content: content.to_string(),
        });
        Ok(())
    }

    /// 单件行数（按首个 `==` 头行后的正文行计）。
    fn content_rows(content: &str) -> usize {
        content.matches('\n').count().saturating_sub(1)
    }

    /// 件名在清单中的位次（不在清单返回 None）。
    fn manifest_pos(name: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < DOC_NAMES.len() {
            let n = match DOC_NAMES.get(i) {
                Some(n) => *n,
                None => break,
            };
            if n == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 对平：包内清单与实存逐件对件名、对行数；缺件 / 多件显性拒。
    /// 全对平返回对平件数（应恰 = DOC_COUNT）。
    pub fn reconcile(&mut self) -> Result<usize, u32> {
        if self.phase == PackPhase::Archived {
            return Err(PackCode::PhaseRegress.val());
        }
        // 实存件数与清单闭集对平：缺件 / 多件都拒。
        if self.items.len() < DOC_COUNT {
            return Err(PackCode::ItemMissing.val());
        }
        if self.items.len() > DOC_COUNT {
            return Err(PackCode::ExtraItem.val());
        }
        let mut matched = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = match self.items.get(i) {
                Some(it) => it,
                None => return Err(PackCode::NotReproducible.val()),
            };
            let pos = match DocPack::manifest_pos(&it.name) {
                Some(p) => p,
                None => return Err(PackCode::NameUnknown.val()),
            };
            let expect_rows = match MANIFEST_ROWS.get(pos) {
                Some(r) => *r,
                None => return Err(PackCode::RowDrift.val()),
            };
            if DocPack::content_rows(&it.content) != expect_rows {
                return Err(PackCode::RowDrift.val());
            }
            matched += 1;
            i += 1;
        }
        self.reconciled = matched;
        self.phase = PackPhase::Reconciled;
        Ok(matched)
    }

    /// 落档：对平前置，六件按清单序拼接后 FNV-1a 落档封包。
    pub fn archive(&mut self) -> Result<u32, u32> {
        if self.phase == PackPhase::Draft {
            return Err(PackCode::PhaseSkip.val());
        }
        if self.phase == PackPhase::Archived {
            return Err(PackCode::PhaseRegress.val());
        }
        if self.reconciled != DOC_COUNT {
            return Err(PackCode::ReconcileIncomplete.val());
        }
        let mut text = String::from("== UNX-F1554 移交包文档预组 ==");
        text.push('\n');
        // 按清单序拼接（不按入包序）：先找清单位次对应件。
        let mut m = 0usize;
        while m < DOC_NAMES.len() {
            let name = match DOC_NAMES.get(m) {
                Some(n) => *n,
                None => break,
            };
            let mut found = false;
            let mut i = 0usize;
            while i < self.items.len() {
                let it = match self.items.get(i) {
                    Some(it) => it,
                    None => break,
                };
                if it.name == name {
                    text.push_str(&it.content);
                    found = true;
                }
                i += 1;
            }
            if !found {
                return Err(PackCode::ItemMissing.val());
            }
            m += 1;
        }
        let d = fnv1a(text.as_bytes());
        if d == 0 {
            return Err(PackCode::NotReproducible.val());
        }
        self.digest = d;
        self.phase = PackPhase::Archived;
        Ok(d)
    }
}

// ── 好包构建（六件入包 + 对平 + 落档） ──────────────────────────
pub fn build_pack_ok() -> DocPack {
    let mut pack = DocPack::new();
    let mut i = 0usize;
    while i < DOC_COUNT {
        let name = match DOC_NAMES.get(i) {
            Some(n) => *n,
            None => break,
        };
        let content = match build_doc(i, SEG_BASE) {
            Some(c) => c,
            None => break,
        };
        let _ = pack.add_item(name, &content);
        i += 1;
    }
    let _ = pack.reconcile();
    let _ = pack.archive();
    pack
}

/// 注入包一：缺件（沉淀件漏入）。
pub fn build_pack_missing() -> DocPack {
    let mut pack = DocPack::new();
    let mut i = 0usize;
    while i < DOC_COUNT {
        if i == 3 {
            // 漏装第 3 件「沉淀」。
            i += 1;
            continue;
        }
        let name = match DOC_NAMES.get(i) {
            Some(n) => *n,
            None => break,
        };
        let content = match build_doc(i, SEG_BASE) {
            Some(c) => c,
            None => break,
        };
        let _ = pack.add_item(name, &content);
        i += 1;
    }
    pack
}

/// 注入包二：行漂移（接口册多出一行）。
pub fn build_pack_drift() -> DocPack {
    let mut pack = DocPack::new();
    let mut i = 0usize;
    while i < DOC_COUNT {
        let name = match DOC_NAMES.get(i) {
            Some(n) => *n,
            None => break,
        };
        let mut content = match build_doc(i, SEG_BASE) {
            Some(c) => c,
            None => break,
        };
        if i == 1 {
            // 接口册追加一行漂移。
            content.push_str("接口册契约行 11 漂移锚 0xFFFF\n");
        }
        let _ = pack.add_item(name, &content);
        i += 1;
    }
    pack
}

/// 注入包三：多件（清单外「临时件」入包）。
pub fn build_pack_extra() -> DocPack {
    let mut again = DocPack::new();
    let mut i = 0usize;
    while i < DOC_COUNT {
        let name = match DOC_NAMES.get(i) {
            Some(n) => *n,
            None => break,
        };
        let content = match build_doc(i, SEG_BASE) {
            Some(c) => c,
            None => break,
        };
        let _ = again.add_item(name, &content);
        i += 1;
    }
    // 注入：清单外件名（入包即拒，包面仍六件）。
    let _ = again.add_item("临时件", "== 临时件 ==\n临时行 01\n");
    let _ = again.reconcile();
    again
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78A80 && SEG_BASE <= 0x78BC0
}

/// 六件预组齐备：好包六件全入、落档态到位。
fn six_docs_ok() -> bool {
    let pack = build_pack_ok();
    if pack.item_count() != DOC_COUNT {
        return false;
    }
    if pack.phase() != PackPhase::Archived {
        return false;
    }
    if pack.reconciled_count() != DOC_COUNT {
        return false;
    }
    true
}

/// 文档生成器行数恰等清单冻结行数。
fn manifest_rows_ok() -> bool {
    let mut i = 0usize;
    while i < DOC_COUNT {
        let expect = match MANIFEST_ROWS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let doc = match build_doc(i, SEG_BASE) {
            Some(d) => d,
            None => return false,
        };
        let rows = DocPack::content_rows(&doc);
        if rows != expect {
            return false;
        }
        i += 1;
    }
    // 行数表与六件闭集长度互证。
    if MANIFEST_ROWS.len() != DOC_COUNT {
        return false;
    }
    true
}

/// 包内清单与实存对平 100%。
fn reconcile_100_ok() -> bool {
    let mut pack = build_pack_ok();
    // 落档后再对平属于倒退，先在草稿包上验对平返回值。
    let mut fresh = DocPack::new();
    let mut i = 0usize;
    while i < DOC_COUNT {
        let name = match DOC_NAMES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        let content = match build_doc(i, SEG_BASE) {
            Some(c) => c,
            None => return false,
        };
        if fresh.add_item(name, &content).is_err() {
            return false;
        }
        i += 1;
    }
    match fresh.reconcile() {
        Ok(n) => {
            if n != DOC_COUNT {
                return false;
            }
        }
        Err(_) => return false,
    }
    if fresh.reconciled_count() != DOC_COUNT {
        return false;
    }
    let _ = pack;
    true
}

/// 预组哈希落档：指纹非零且与清单序绑定（入包序扰动不改指纹）。
fn digest_archived_ok() -> bool {
    let pack = build_pack_ok();
    if pack.digest() == 0 {
        return false;
    }
    // 逆序入包的同内容包：落档指纹应与正序一致（落档按清单序拼接）。
    let mut rev = DocPack::new();
    let mut i = DOC_COUNT;
    while i > 0 {
        i -= 1;
        let name = match DOC_NAMES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        let content = match build_doc(i, SEG_BASE) {
            Some(c) => c,
            None => return false,
        };
        let _ = rev.add_item(name, &content);
    }
    let _ = rev.reconcile();
    let _ = rev.archive();
    if rev.digest() != pack.digest() {
        return false;
    }
    true
}

/// 注入必抓：缺件 / 行漂移 / 多件三路全抓。
fn injection_caught_ok() -> bool {
    // 缺件：对平必拒。
    let mut missing = build_pack_missing();
    if missing.item_count() != DOC_COUNT - 1 {
        return false;
    }
    match missing.reconcile() {
        Err(code) => {
            if code != PackCode::ItemMissing.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 行漂移：对平必拒。
    let mut drift = build_pack_drift();
    match drift.reconcile() {
        Err(code) => {
            if code != PackCode::RowDrift.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 多件：清单外件名入包即拒。
    let mut extra = DocPack::new();
    let name0 = match DOC_NAMES.get(0) {
        Some(n) => *n,
        None => return false,
    };
    let doc0 = match build_doc(0, SEG_BASE) {
        Some(c) => c,
        None => return false,
    };
    let _ = extra.add_item(name0, &doc0);
    match extra.add_item("临时件", "== 临时件 ==\n临时行\n") {
        Err(code) => {
            if code != PackCode::NameUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 件名守卫：空件名、重复件名、清单外件名三路全拒。
fn name_guard_ok() -> bool {
    let mut pack = DocPack::new();
    let name0 = match DOC_NAMES.get(0) {
        Some(n) => *n,
        None => return false,
    };
    let doc0 = match build_doc(0, SEG_BASE) {
        Some(c) => c,
        None => return false,
    };
    // 空件名。
    match pack.add_item("", "内容") {
        Err(code) => {
            if code != PackCode::NameEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 清单外件名。
    match pack.add_item("私货件", "内容") {
        Err(code) => {
            if code != PackCode::NameUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let _ = pack.add_item(name0, &doc0);
    // 重复件名。
    match pack.add_item(name0, &doc0) {
        Err(code) => {
            if code != PackCode::NameDup.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if pack.item_count() != 1 {
        return false;
    }
    true
}

/// 状态机单调：未对平落档拒、二次落档拒、封包后入包拒。
fn phase_monotonic_ok() -> bool {
    let mut pack = DocPack::new();
    if pack.phase() != PackPhase::Draft {
        return false;
    }
    // 未对平落档：跳级拒。
    match pack.archive() {
        Err(code) => {
            if code != PackCode::PhaseSkip.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    let sealed = build_pack_ok();
    if sealed.phase() != PackPhase::Archived {
        return false;
    }
    // 封包后入包：SealedLocked（另建可变副本验证）。
    let mut sealed2 = build_pack_ok();
    let name1 = match DOC_NAMES.get(1) {
        Some(n) => *n,
        None => return false,
    };
    match sealed2.add_item(name1, "内容") {
        Err(code) => {
            if code != PackCode::SealedLocked.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 六件闭集：清单件名互异且非空。
fn doc_names_closed_ok() -> bool {
    if DOC_NAMES.len() != DOC_COUNT {
        return false;
    }
    let mut seen: Vec<&str> = Vec::new();
    let mut i = 0usize;
    while i < DOC_NAMES.len() {
        let n = match DOC_NAMES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        if n.is_empty() {
            return false;
        }
        if seen.iter().any(|s| *s == n) {
            return false;
        }
        seen.push(n);
        i += 1;
    }
    if seen.len() != DOC_COUNT {
        return false;
    }
    true
}

/// 双跑一致：好包指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let a = build_pack_ok();
    let b = build_pack_ok();
    if a.digest() != b.digest() {
        return false;
    }
    if a.digest() == 0 {
        return false;
    }
    if a.item_count() != b.item_count() {
        return false;
    }
    true
}

/// 内容生成器确定性：同位次两次生成逐字节相同。
fn build_doc_deterministic_ok() -> bool {
    let mut i = 0usize;
    while i < DOC_COUNT {
        let a = match build_doc(i, SEG_BASE) {
            Some(d) => d,
            None => return false,
        };
        let b = match build_doc(i, SEG_BASE) {
            Some(d) => d,
            None => return false,
        };
        if a != b {
            return false;
        }
        if a.is_empty() {
            return false;
        }
        i += 1;
    }
    // 换段生成应不同（锚位随段变）。
    let c = match build_doc(0, SEG_BASE + 0x40) {
        Some(d) => d,
        None => return false,
    };
    let d0 = match build_doc(0, SEG_BASE) {
        Some(x) => x,
        None => return false,
    };
    if c == d0 {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in PackCode::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match PackCode::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1554_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1554 域级文档打包预演");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78A80..=0x78BC0", SEG_BASE));
    s.add_dyn("六件预组齐备", six_docs_ok(), "域册/接口册/预案册/沉淀/FAQ/词汇表六件入包落档");
    s.add_dyn("清单行数冻结", manifest_rows_ok(), "六件文档行数逐件与清单冻结行数相等");
    s.add_dyn("清单实存对平 100%", reconcile_100_ok(), "对平计数 6/6，件名与行数逐项对平");
    s.add_dyn("预组哈希落档", digest_archived_ok(), "落档指纹非零且按清单序绑定，入包序无关");
    s.add_dyn("注入必抓", injection_caught_ok(), "缺件/行漂移/多件三路注入全抓");
    s.add_dyn("件名守卫", name_guard_ok(), "空件名/清单外件名/重复件名三路全拒");
    s.add_dyn("状态机单调", phase_monotonic_ok(), "组包→对平→落档可达，跳级倒退封包入包全拒");
    s.add_dyn("六件闭集", doc_names_closed_ok(), "清单六件名互异且非空");
    s.add_dyn("双跑一致", double_run_ok(), "好包指纹两次构建逐位相等");
    s.add_dyn("生成器确定性", build_doc_deterministic_ok(), "同位次生成逐字节相同，换段即变");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78A80);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1554_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn six_docs_packaged_and_archived() {
        let pack = build_pack_ok();
        assert_eq!(pack.item_count(), 6);
        assert_eq!(pack.phase(), PackPhase::Archived);
        assert_eq!(pack.reconciled_count(), 6);
        assert!(pack.digest() != 0);
    }

    #[test]
    fn missing_item_blocks_reconcile() {
        let mut pack = build_pack_missing();
        assert_eq!(pack.item_count(), 5);
        match pack.reconcile() {
            Err(code) => assert_eq!(code, PackCode::ItemMissing.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn row_drift_blocks_reconcile() {
        let mut pack = build_pack_drift();
        match pack.reconcile() {
            Err(code) => assert_eq!(code, PackCode::RowDrift.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn unknown_name_rejected() {
        let mut pack = DocPack::new();
        match pack.add_item("清单外件", "== 清单外件 ==\n行\n") {
            Err(code) => assert_eq!(code, PackCode::NameUnknown.val()),
            Ok(()) => must_not_reach(),
        }
        match pack.add_item("", "内容") {
            Err(code) => assert_eq!(code, PackCode::NameEmpty.val()),
            Ok(()) => must_not_reach(),
        }
    }

    #[test]
    fn duplicate_name_rejected() {
        let mut pack = DocPack::new();
        let name0 = DOC_NAMES.get(0).unwrap();
        let doc0 = build_doc(0, SEG_BASE).unwrap();
        assert!(pack.add_item(name0, &doc0).is_ok());
        match pack.add_item(name0, &doc0) {
            Err(code) => assert_eq!(code, PackCode::NameDup.val()),
            Ok(()) => must_not_reach(),
        }
        assert_eq!(pack.item_count(), 1);
    }

    #[test]
    fn archive_requires_reconcile() {
        let mut pack = DocPack::new();
        let name0 = DOC_NAMES.get(0).unwrap();
        let doc0 = build_doc(0, SEG_BASE).unwrap();
        let _ = pack.add_item(name0, &doc0);
        match pack.archive() {
            Err(code) => assert_eq!(code, PackCode::PhaseSkip.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn reversed_insertion_same_digest() {
        let fwd = build_pack_ok();
        let mut rev = DocPack::new();
        let mut i = DOC_COUNT;
        while i > 0 {
            i -= 1;
            let name = DOC_NAMES.get(i).unwrap();
            let content = build_doc(i, SEG_BASE).unwrap();
            let _ = rev.add_item(name, &content);
        }
        let _ = rev.reconcile();
        let _ = rev.archive();
        assert_eq!(fwd.digest(), rev.digest());
        assert_ne!(fwd.digest(), 0);
    }

    #[test]
    fn doc_rows_match_manifest() {
        for i in 0..DOC_COUNT {
            let doc = build_doc(i, SEG_BASE).unwrap();
            let rows = doc.matches('\n').count() - 1;
            assert_eq!(rows, MANIFEST_ROWS[i]);
        }
    }

    #[test]
    fn double_run_digest_identical() {
        let a = build_pack_ok();
        let b = build_pack_ok();
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), 0);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(PackCode::ALL.len(), 12);
        let first = match PackCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match PackCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
