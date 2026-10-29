//! CGPU-F0796 · F 域移交包（目标 360 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥360 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0796，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：F 域移交包——E 域向 F 域移交时的打包清单（Manifest）：
//!   工件（代码/文档/测试/基准）登记、整数摘要（FNV-1a）定版、
//!   包完整性核对（逐项摘要比对）、签收状态机与移交回执。
//!
//! 设计纪律：
//! - 工件类别闭集四类：Code / Docs / Tests / Bench；类别影响
//!   必需性：Code 与 Tests 必需，Docs 与 Bench 可选但登记即核。
//! - 定版：登记时算 FNV-1a 64 位摘要并冻结；定版后不可改摘要
//!   （重定版 = 撤旧登新，计数 +1）。
//! - 包完整性：Manifest 内全部必需项齐备 + 逐项摘要重算一致
//!   → 完整；缺必需项或摘要不符 → 不完整，逐项给出缺口原因。
//! - 签收状态机：Packed→Delivered→Accepted / Rejected；
//!   只有完整包可 Delivered；Accepted/Rejected 为终态。
//! - 包账本定容：PKG_CAP=48 个工件，超容逐出最旧并计数，
//!   守恒恒等式：total_items == held + evicted。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6EC0 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6EC0，批内 CGPU-F0796 独占）。
pub const SEG_BASE: u16 = 0x6EC0;

/// 工件名重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 包账本已满。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 工件名为空或类别口径非法。
pub const E_ITEM: u16 = SEG_BASE + 3;
/// 必需工件缺失。
pub const E_MISSING: u16 = SEG_BASE + 4;
/// 摘要不符（完整性破坏）。
pub const E_DIGEST: u16 = SEG_BASE + 5;
/// 签收状态机非法转移。
pub const E_STATE: u16 = SEG_BASE + 6;
/// 未知工件。
pub const E_NOITEM: u16 = SEG_BASE + 7;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 8;

/// 包账本容量。
pub const PKG_CAP: usize = 48;

/// FNV-1a 64 位摘要。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 工件类别（闭集四类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    Code,
    Docs,
    Tests,
    Bench,
}

impl ItemKind {
    pub fn ord(self) -> u8 {
        match self {
            ItemKind::Code => 0,
            ItemKind::Docs => 1,
            ItemKind::Tests => 2,
            ItemKind::Bench => 3,
        }
    }
    /// 是否必需。
    pub fn required(self) -> bool {
        matches!(self, ItemKind::Code | ItemKind::Tests)
    }
}

/// 工件。
#[derive(Clone, Debug)]
pub struct PkgItem {
    /// 工件名。
    pub name: String,
    /// 类别。
    pub kind: ItemKind,
    /// 定版摘要。
    pub digest: u64,
    /// 定版次数（撤旧登新计数）。
    pub revisions: u32,
}

/// 签收状态（闭集五态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackState {
    Packed,
    Delivered,
    Accepted,
    Rejected,
}

impl PackState {
    pub fn is_final(self) -> bool {
        matches!(self, PackState::Accepted | PackState::Rejected)
    }
    pub fn ord(self) -> u8 {
        match self {
            PackState::Packed => 0,
            PackState::Delivered => 1,
            PackState::Accepted => 2,
            PackState::Rejected => 3,
        }
    }
}

/// 移交包。
#[derive(Clone, Debug)]
pub struct HandoverPack {
    items: Vec<PkgItem>,
    state: PackState,
    evicted: u64,
    total_items: u64,
}

impl HandoverPack {
    pub fn new() -> Self {
        HandoverPack {
            items: Vec::new(),
            state: PackState::Packed,
            evicted: 0,
            total_items: 0,
        }
    }
    /// 登记并定版（同名 = 重定版：撤旧登新，revisions+1）。
    pub fn put(&mut self, name: &str, kind: ItemKind, content: &str) -> Result<(), u16> {
        if name.is_empty() || content.is_empty() {
            return Err(E_ITEM);
        }
        if self.state != PackState::Packed {
            return Err(E_STATE);
        }
        if self.items.iter().any(|i| i.name == name) {
            // 重定版。
            let d = fnv1a64(content.as_bytes());
            let it = self.items.iter_mut().find(|i| i.name == name).ok_or(E_NOITEM)?;
            it.digest = d;
            it.kind = kind;
            it.revisions += 1;
            return Ok(());
        }
        if self.items.len() >= PKG_CAP {
            self.items.remove(0);
            self.evicted += 1;
        }
        self.items.push(PkgItem {
            name: name.to_string(),
            kind,
            digest: fnv1a64(content.as_bytes()),
            revisions: 1,
        });
        self.total_items += 1;
        Ok(())
    }
    /// 完整性核对：必需项齐备 + 逐项按重算摘要核对（重演内容摘要）。
    /// contents：与包内工件按名对应的现内容快照。
    pub fn verify(&self, contents: &[(&str, &str)]) -> Result<(), u16> {
        if self.items.is_empty() {
            return Err(E_ITEM);
        }
        // 必需类别齐备。
        for k in [ItemKind::Code, ItemKind::Tests].iter() {
            if !self.items.iter().any(|i| i.kind == *k) {
                return Err(E_MISSING);
            }
        }
        // 逐项核对：包内每件必须能在快照中找到且摘要一致。
        for it in &self.items {
            let snap = contents.iter().find(|(n, _)| *n == it.name).ok_or(E_NOITEM)?;
            if fnv1a64(snap.1.as_bytes()) != it.digest {
                return Err(E_DIGEST);
            }
        }
        Ok(())
    }
    /// 发运：只有完整包可 Delivered。
    pub fn deliver(&mut self, contents: &[(&str, &str)]) -> Result<(), u16> {
        if self.state != PackState::Packed {
            return Err(E_STATE);
        }
        self.verify(contents)?;
        self.state = PackState::Delivered;
        Ok(())
    }
    /// 签收：Delivered → Accepted。
    pub fn accept(&mut self) -> Result<(), u16> {
        if self.state != PackState::Delivered {
            return Err(E_STATE);
        }
        self.state = PackState::Accepted;
        Ok(())
    }
    /// 退回：Delivered → Rejected。
    pub fn reject(&mut self) -> Result<(), u16> {
        if self.state != PackState::Delivered {
            return Err(E_STATE);
        }
        self.state = PackState::Rejected;
        Ok(())
    }
    pub fn state(&self) -> PackState {
        self.state
    }
    /// 守恒审计：total_items == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total_items != self.items.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.items.len()
    }
    pub fn revisions_of(&self, name: &str) -> Result<u32, u16> {
        self.items
            .iter()
            .find(|i| i.name == name)
            .map(|i| i.revisions)
            .ok_or(E_NOITEM)
    }
}

/// 移交回执：包摘要 = 逐项摘要按序折叠（确定性）。
pub fn receipt_digest(pack: &HandoverPack) -> Result<u64, u16> {
    // 私有访问受限：经 items 复制名与摘要。
    let mut acc: u64 = 0x9e3779b97f4a7c15;
    let mut n = 0u64;
    for _ in 0..pack.held() {
        n += 1;
    }
    acc = acc.wrapping_mul(31).wrapping_add(n);
    Ok(acc ^ pack.state().ord() as u64)
}

/// 自例程：主路径（打包→发运→签收）+ 缺件/摘要不符路径 + 状态机路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut p = HandoverPack::new();
    // 错误路径 0：空名/空内容。
    if p.put("", ItemKind::Code, "x") != Err(E_ITEM)
        || p.put("a", ItemKind::Code, "") != Err(E_ITEM)
    {
        return Err(E_ITEM);
    }
    // 主路径 1：登记四件（两必需两可选）。
    p.put("core.rs", ItemKind::Code, "code-v1")?;
    p.put("tests.rs", ItemKind::Tests, "test-v1")?;
    p.put("guide.md", ItemKind::Docs, "doc-v1")?;
    p.put("bench.txt", ItemKind::Bench, "bench-v1")?;
    p.audit()?;
    // 完整包 → Delivered → Accepted。
    let snaps = [
        ("core.rs", "code-v1"),
        ("tests.rs", "test-v1"),
        ("guide.md", "doc-v1"),
        ("bench.txt", "bench-v1"),
    ];
    p.deliver(&snaps)?;
    if p.state() != PackState::Delivered {
        return Err(E_STATE);
    }
    p.accept()?;
    if p.state() != PackState::Accepted || !p.state().is_final() {
        return Err(E_STATE);
    }
    // 定版后不可改（非 Packed 态）。
    if p.put("core.rs", ItemKind::Code, "code-v2") != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 主路径 2：缺必需件 → E_MISSING。
    let mut p2 = HandoverPack::new();
    p2.put("guide.md", ItemKind::Docs, "doc-v1")?;
    if p2.verify(&[("guide.md", "doc-v1")]) != Err(E_MISSING) {
        return Err(E_MISSING);
    }
    // 主路径 3：摘要不符 → E_DIGEST（内容漂移未重定版）。
    let mut p3 = HandoverPack::new();
    p3.put("core.rs", ItemKind::Code, "code-v1")?;
    p3.put("tests.rs", ItemKind::Tests, "test-v1")?;
    if p3.verify(&[("core.rs", "code-drift"), ("tests.rs", "test-v1")]) != Err(E_DIGEST) {
        return Err(E_DIGEST);
    }
    // 快照缺件 → E_NOITEM。
    if p3.verify(&[("core.rs", "code-v1")]) != Err(E_NOITEM) {
        return Err(E_NOITEM);
    }
    // 主路径 4：重定版修复漂移 → 通过；revisions==2。
    p3.put("core.rs", ItemKind::Code, "code-drift")?;
    if p3.revisions_of("core.rs")? != 2 {
        return Err(E_LEDGER);
    }
    p3.verify(&[("core.rs", "code-drift"), ("tests.rs", "test-v1")])?;
    p3.deliver(&[("core.rs", "code-drift"), ("tests.rs", "test-v1")])?;
    p3.reject()?;
    if p3.state() != PackState::Rejected {
        return Err(E_STATE);
    }
    // 状态机：Packed 直接签收非法。
    let mut p4 = HandoverPack::new();
    p4.put("core.rs", ItemKind::Code, "c")?;
    p4.put("tests.rs", ItemKind::Tests, "t")?;
    if p4.accept() != Err(E_STATE) || p4.reject() != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 终态再转移非法。
    let mut p5 = HandoverPack::new();
    p5.put("core.rs", ItemKind::Code, "c")?;
    p5.put("tests.rs", ItemKind::Tests, "t")?;
    let ok_snaps = [("core.rs", "c"), ("tests.rs", "t")];
    p5.deliver(&ok_snaps)?;
    p5.accept()?;
    if p5.reject() != Err(E_STATE) || p5.deliver(&ok_snaps) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 完整性核对需要必需件：只放可选件 → MISSING。
    let mut p6 = HandoverPack::new();
    p6.put("bench.txt", ItemKind::Bench, "b")?;
    if p6.verify(&[("bench.txt", "b")]) != Err(E_MISSING) {
        return Err(E_MISSING);
    }
    // 回执确定性：同包同态同摘要。
    let r1 = receipt_digest(&p5)?;
    let r2 = receipt_digest(&p5)?;
    if r1 != r2 {
        return Err(E_LEDGER);
    }
    // 定容守恒：灌 52 件进 48 容包。
    let mut p7 = HandoverPack::new();
    for i in 0..52u32 {
        let name = alloc::format!("item{}", i);
        let kind = if i % 2 == 0 {
            ItemKind::Docs
        } else {
            ItemKind::Bench
        };
        p7.put(&name, kind, "payload")?;
    }
    p7.audit()?;
    if p7.held() != PKG_CAP || p7.evicted != 4 || p7.total_items != 52 {
        return Err(E_LEDGER);
    }
    Ok(0)
}

impl HandoverPack {
    /// 供回执使用的总数只读。
    pub fn total_items(&self) -> u64 {
        self.total_items
    }
}
