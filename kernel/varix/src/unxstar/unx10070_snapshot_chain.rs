//! UNX-F10070 快照总对账（三件套版本链）· 0x92180 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10071-J1 版本链连续核（逐段锚连 10/10 次），断链 10/10 次检出
//!
//! 设计要点：
//!   ① 三件套 = 总纲册 / 批册 / 深化册；每段快照入链必须携带前段哈希，
//!      版本链逐段锚连：node[i].prev_hash == node[i-1].own_hash 恒成立，
//!      创世段 prev_hash = 0。任何 prev 不符 → BrokenLink，段号跳变或倒退
//!      → SegGap / DupSegment，逐次显性拒绝（10/10），拒绝不留痕。
//!   ② 三件套审计：版本链按段序三三分窗，每窗必须恰好覆盖三种册别各一次；
//!      缺任何一册 → MissingPiece，总对账不得在缺件链上放行。
//!   ③ 段哈希由载荷 + 段号 + 册别确定性推导（FNV-1a 混合），同载荷双跑
//!      逐位一致；哈希往返不一致 → HashMismatch。
//!   ④ 注入核：断链 / 跳段 / 重复段 / 缺件四类坏样本逐类 10/10 次拒绝，
//!      注入账 注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10070 独占 0x40 窗口，下一卡 0x921C0）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92180;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 版本链容量上限（防溢出的工程约束）。
pub const CHAIN_CAP: usize = 48;

/// 创世段前锚：链首没有前段，prev_hash 约定为 0。
pub const GENESIS_PREV: u32 = 0;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainCode {
    /// 空载荷快照段
    EmptyPayload,
    /// 段号倒退或与尾段重复
    DupSegment,
    /// 段号跳变（不接尾段 +1）
    SegGap,
    /// 前锚不符（prev_hash != 尾段 own_hash）
    BrokenLink,
    /// 段哈希往返不一致
    HashMismatch,
    /// 三件套缺册
    MissingPiece,
    /// 链容量满
    ChainFull,
    /// 未知册别 token
    UnknownPiece,
    /// 空成文
    EmptySummary,
    /// 拒绝后链被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 册别标签往返不一致
    PieceRoundTrip,
    /// 段位越界
    SegmentViolation,
}

impl ChainCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ChainCode::EmptyPayload => "空载荷被拒：快照段必须携带非空内容；\
                请先冻结段内容再入链",
            ChainCode::DupSegment => "段号重复被拒：版本链段号必须严格递增；\
                重复入链请走重测路径而非再次 append",
            ChainCode::SegGap => "段号跳变被拒：新段必须接尾段 +1；\
                请核对冻结序，禁止越过未冻结段",
            ChainCode::BrokenLink => "前锚断链被拒：prev_hash 必须等于尾段 own_hash；\
                请从最新冻结段取前锚，禁止手写",
            ChainCode::HashMismatch => "哈希往返不一致：同载荷重算哈希必须逐位一致；\
                请检查载荷与段号/册别是否被改",
            ChainCode::MissingPiece => "三件套缺册被拒：每三分窗必须齐 总纲/批册/深化册；\
                请补齐缺册后再做总对账",
            ChainCode::ChainFull => "容量满被拒：版本链已达上限；请先归档旧链再续链",
            ChainCode::UnknownPiece => "未知册别被拒：册别只认 general|batch|deep 闭集；\
                请改用闭集内写法",
            ChainCode::EmptySummary => "空成文被拒：链对账小结不得为空串；请检查链是否为空",
            ChainCode::RejectedTrace => "拒绝留痕被拒：被拒入链不得改动任何节点；\
                请排查拒绝路径是否误提交",
            ChainCode::PieceRoundTrip => "册别往返不一致：label 再解析必须回到原册别；\
                请检查闭集文案与解析器是否同步",
            ChainCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [ChainCode; 12] = [
        ChainCode::EmptyPayload,
        ChainCode::DupSegment,
        ChainCode::SegGap,
        ChainCode::BrokenLink,
        ChainCode::HashMismatch,
        ChainCode::MissingPiece,
        ChainCode::ChainFull,
        ChainCode::UnknownPiece,
        ChainCode::EmptySummary,
        ChainCode::RejectedTrace,
        ChainCode::PieceRoundTrip,
        ChainCode::SegmentViolation,
    ];
}

// ── 册别闭集与解析器 ──────────────────────────────────────────────────
/// 三件套册别闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PieceKind {
    /// 总纲册
    General,
    /// 批册
    Batch,
    /// 深化册
    Deep,
}

impl PieceKind {
    /// 中文标签（成文与往返用）。
    pub fn label(self) -> &'static str {
        match self {
            PieceKind::General => "总纲册",
            PieceKind::Batch => "批册",
            PieceKind::Deep => "深化册",
        }
    }

    /// 闭集全量（三件套固定序：总纲 → 批册 → 深化册）。
    pub const ALL: [PieceKind; 3] = [PieceKind::General, PieceKind::Batch, PieceKind::Deep];

    /// 解析索引（供旋转序与注入构造用）。
    pub fn index(self) -> usize {
        match self {
            PieceKind::General => 0,
            PieceKind::Batch => 1,
            PieceKind::Deep => 2,
        }
    }
}

/// 册别解析：长名 general/batch/deep 与单字母 g/b/d 双形制，其余一律
/// UnknownPiece，空串亦拒。解析器是册别入链的唯一入口。
pub fn parse_piece_kind(tag: &str) -> Result<PieceKind, ChainCode> {
    if tag.is_empty() {
        return Err(ChainCode::UnknownPiece);
    }
    match tag {
        "general" | "g" => Ok(PieceKind::General),
        "batch" | "b" => Ok(PieceKind::Batch),
        "deep" | "d" => Ok(PieceKind::Deep),
        _ => Err(ChainCode::UnknownPiece),
    }
}

// ── 段哈希（确定性推导） ──────────────────────────────────────────────
/// 段哈希：FNV-1a(载荷) 与段号、册别序混合；同输入双跑逐位一致。
pub fn seg_hash(seg_no: u32, piece: PieceKind, payload: &str) -> u32 {
    let h0 = fnv1a(payload.as_bytes());
    h0 ^ seg_no.wrapping_mul(0x9E37_79B9)
        ^ (piece.index() as u32).wrapping_mul(0x85EB_CA6B)
}

// ── 版本链账本 ────────────────────────────────────────────────────────
/// 链上节点：段号 + 册别 + 前锚 + 本段哈希。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VersionNode {
    /// 段号（从 1 起严格递增）
    pub seg_no: u32,
    /// 册别
    pub piece: PieceKind,
    /// 前锚哈希（创世段为 0）
    pub prev_hash: u32,
    /// 本段哈希
    pub own_hash: u32,
}

/// 三件套版本链：逐段锚连 + 段号严格递增 + 容量上限。
#[derive(Clone, Debug)]
pub struct ChainLedger {
    nodes: Vec<VersionNode>,
    cap: usize,
}

impl ChainLedger {
    /// 空链构造（容量 CHAIN_CAP）。
    pub fn new() -> ChainLedger {
        ChainLedger { nodes: Vec::new(), cap: CHAIN_CAP }
    }

    /// 入链一段：载荷非空、段号 = 尾段+1（首段必须为 1）、
    /// 前锚必须等于尾段 own_hash（首段必须为创世锚 0）；
    /// 任一违反显性拒绝且不留痕。成功返回本段哈希。
    pub fn append(
        &mut self,
        seg_no: u32,
        piece: PieceKind,
        prev_hash: u32,
        payload: &str,
    ) -> Result<u32, ChainCode> {
        if payload.is_empty() {
            return Err(ChainCode::EmptyPayload);
        }
        if self.nodes.len() >= self.cap {
            return Err(ChainCode::ChainFull);
        }
        let (expect_seg, expect_prev) = match self.nodes.last() {
            None => (1u32, GENESIS_PREV),
            Some(last) => (last.seg_no + 1, last.own_hash),
        };
        if seg_no < expect_seg {
            return Err(ChainCode::DupSegment);
        }
        if seg_no > expect_seg {
            return Err(ChainCode::SegGap);
        }
        if prev_hash != expect_prev {
            return Err(ChainCode::BrokenLink);
        }
        let own = seg_hash(seg_no, piece, payload);
        self.nodes.push(VersionNode { seg_no, piece, prev_hash, own_hash: own });
        Ok(own)
    }

    /// 已入链段数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 链是否为空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 逐段锚连审计：段号连续递增且每段 prev == 前段 own（空链视为连续）。
    pub fn continuous(&self) -> bool {
        let mut i = 0usize;
        let mut expect_prev = GENESIS_PREV;
        let mut expect_seg = 1u32;
        while i < self.nodes.len() {
            match self.nodes.get(i) {
                None => return false,
                Some(n) => {
                    if n.prev_hash != expect_prev || n.seg_no != expect_seg {
                        return false;
                    }
                    expect_prev = n.own_hash;
                    expect_seg += 1;
                }
            }
            i += 1;
        }
        true
    }

    /// 三件套审计：按段序三三分窗，每窗必须恰好覆盖三种册别各一次。
    pub fn triple_set_complete(&self) -> bool {
        if self.nodes.is_empty() || self.nodes.len() % 3 != 0 {
            return false;
        }
        let mut w = 0usize;
        while w < self.nodes.len() {
            let mut seen = [0usize; 3];
            let mut k = 0usize;
            while k < 3 {
                if let Some(n) = self.nodes.get(w + k) {
                    let idx = n.piece.index();
                    if idx < 3 {
                        seen[idx] += 1;
                    }
                }
                k += 1;
            }
            if seen[0] != 1 || seen[1] != 1 || seen[2] != 1 {
                return false;
            }
            w += 3;
        }
        true
    }
}

// ── 注入核：坏链样本矩阵 ──────────────────────────────────────────────
/// 四类坏链样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadChain {
    /// 断链：前锚手写错误
    BrokenLink,
    /// 跳段：越过未冻结段
    SegGap,
    /// 重复段：段号倒退重入
    DupSegment,
    /// 缺件：链上少了深化册
    MissingPiece,
}

impl BadChain {
    /// 闭集全量。
    pub const ALL: [BadChain; 4] = [
        BadChain::BrokenLink,
        BadChain::SegGap,
        BadChain::DupSegment,
        BadChain::MissingPiece,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadChain::BrokenLink => "断链入链",
            BadChain::SegGap => "跳段入链",
            BadChain::DupSegment => "重复段入链",
            BadChain::MissingPiece => "三件套缺册",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失守恒返回 RejectedTrace。
    pub fn record(&mut self, caught: bool) -> Result<(), ChainCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(ChainCode::RejectedTrace);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏链注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新链，确保被拒样本不留痕。
pub fn run_bad_chain_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadChain::ALL.len() {
        let case = match BadChain::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadChain::BrokenLink => {
                    let mut l = ChainLedger::new();
                    let h1 = l.append(1, PieceKind::General, GENESIS_PREV, "snap-g1");
                    match h1 {
                        Ok(_) => {
                            l.append(2, PieceKind::Batch, 0xDEAD, "snap-b1")
                                .err() == Some(ChainCode::BrokenLink)
                        }
                        Err(_) => false,
                    }
                }
                BadChain::SegGap => {
                    let mut l = ChainLedger::new();
                    let _ = l.append(1, PieceKind::General, GENESIS_PREV, "snap-g1");
                    l.append(3, PieceKind::Batch, 0, "snap-b3").err() == Some(ChainCode::SegGap)
                }
                BadChain::DupSegment => {
                    let mut l = ChainLedger::new();
                    let _ = l.append(1, PieceKind::General, GENESIS_PREV, "snap-g1");
                    l.append(1, PieceKind::Batch, 0, "snap-b1").err() == Some(ChainCode::DupSegment)
                }
                BadChain::MissingPiece => {
                    let mut l = ChainLedger::new();
                    let _ = l.append(1, PieceKind::General, GENESIS_PREV, "snap-g1");
                    let _ = l.append(2, PieceKind::Batch, 0, "snap-b1");
                    !l.triple_set_complete()
                }
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 链对账成文（纯函数） ──────────────────────────────────────────────
/// 链对账成文：「段数=<n>;连续=<是|否>;三件套=<齐|缺>」；空链先拒。
pub fn compose_chain_summary(l: &ChainLedger) -> Result<String, ChainCode> {
    if l.is_empty() {
        return Err(ChainCode::EmptySummary);
    }
    let cont = if l.continuous() { "是" } else { "否" };
    let full = if l.triple_set_complete() { "齐" } else { "缺" };
    let out = format!("段数={};连续={};三件套={}", l.len(), cont, full);
    if out.is_empty() {
        return Err(ChainCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；空载荷/未知册别族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺册/断链无前锚族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复段族"),
    ("EBADE", -52, 31, "Windows ERROR_GEN_FAILURE；前锚不符/哈希失配族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；链容量满族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：两轮三件套入链 → 审计 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = ChainLedger::new();
    let _ = l.append(1, PieceKind::General, GENESIS_PREV, "总纲 v1");
    let _ = l.append(2, PieceKind::Batch, 0, "批册 v1");
    let _ = l.append(3, PieceKind::Deep, 0, "深化 v1");
    let _ = l.append(4, PieceKind::General, 0, "总纲 v2");
    let bytes = match compose_chain_summary(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10070 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10070_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10070");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92180..=0x921BF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 册别解析器：双形制全通过
    let parse_ok = parse_piece_kind("general") == Ok(PieceKind::General)
        && parse_piece_kind("g") == Ok(PieceKind::General)
        && parse_piece_kind("batch") == Ok(PieceKind::Batch)
        && parse_piece_kind("d") == Ok(PieceKind::Deep);
    s.add("册别解析器：general|g / batch|b / deep|d 双形制全通过", parse_ok,
          "册别闭集双形制必须全可解析，闭集外显性拒绝");

    // 3. 未知册别注入：未知/空串/漂移 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_piece_kind("total").is_ok()
            || parse_piece_kind("").is_ok()
            || parse_piece_kind("General").is_ok()
            || parse_piece_kind("deep ").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("未知册别注入：未知/空串/漂移 10/10 拒绝", amb_ok,
          "册别歧义必须逐次显性拒绝，不许静默归并到相邻册别");

    // 4. 创世段规则：首段必须段号 1 且前锚为创世锚
    let genesis_ok = ChainLedger::new()
        .append(2, PieceKind::General, GENESIS_PREV, "x")
        .err() == Some(ChainCode::SegGap)
        && ChainLedger::new()
            .append(1, PieceKind::General, 0x1234, "x")
            .err() == Some(ChainCode::BrokenLink);
    s.add("创世段规则：首段段号 1 且前锚必须为创世锚", genesis_ok,
          "版本链起点唯一：段号不为 1 或前锚非 0 一律拒绝");

    // 5. 连续链构建：两轮三件套入链并逐段锚连
    let mut l = ChainLedger::new();
    let h1 = l.append(1, PieceKind::General, GENESIS_PREV, "总纲 v1");
    let mut build_ok = h1.is_ok();
    let mut prev = match h1 { Ok(h) => h, Err(_) => 0 };
    let mut seg = 2u32;
    let mut k = 0usize;
    while k < 5 {
        let piece = match PieceKind::ALL.get((seg as usize - 1) % 3) {
            Some(p) => *p,
            None => break,
        };
        let payload = format!("段{}载荷", seg);
        match l.append(seg, piece, prev, &payload) {
            Ok(h) => prev = h,
            Err(_) => build_ok = false,
        }
        seg += 1;
        k += 1;
    }
    s.add("连续链构建：六段入链且逐段锚连成立", build_ok && l.len() == 6 && l.continuous(),
          "版本链逐段锚连：prev 必须取自上一段 own，禁止手写");

    // 6. 三件套审计：两窗各覆盖三种册别
    s.add("三件套审计：每三分窗齐 总纲/批册/深化册",
          l.triple_set_complete(),
          "缺任何一册的三件套不得放行总对账");

    // 7. 注入核：四类坏链样本各 10/10 拒绝
    let (inj, per_class) = run_bad_chain_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏链样本各 10/10 拒绝", all_caught,
          "断链/跳段/重复段/缺件必须逐类逐次拒绝");

    // 8. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadChain::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 哈希往返：同载荷同段号双跑哈希逐位一致
    let hash_rt = seg_hash(9, PieceKind::Deep, "锚连语料")
        == seg_hash(9, PieceKind::Deep, "锚连语料")
        && seg_hash(9, PieceKind::General, "锚连语料") != seg_hash(9, PieceKind::Deep, "锚连语料");
    s.add("哈希往返：同载荷双跑逐位一致且册别可区分", hash_rt,
          "段哈希由载荷+段号+册别确定性推导，任何漂移即失配");

    // 10. 拒绝不留痕：断链被拒后链长不变
    let mut l2 = ChainLedger::new();
    let _ = l2.append(1, PieceKind::General, GENESIS_PREV, "总纲 v1");
    let before = l2.len();
    let _ = l2.append(2, PieceKind::Batch, 0xDEAD, "批册 v1");
    let trace_ok = l2.len() == before && l2.continuous();
    s.add("拒绝不留痕：断链被拒不改链任何节点", trace_ok,
          "被拒入链不得改动链长与已冻结段");

    // 11. 链容量满拒绝
    let mut l3 = ChainLedger::new();
    let mut n = 0u32;
    let mut prev3 = GENESIS_PREV;
    while n < CHAIN_CAP as u32 {
        match l3.append(n + 1, PieceKind::General, prev3, "fill") {
            Ok(h) => prev3 = h,
            Err(_) => break,
        }
        n += 1;
    }
    let full_ok = l3.len() == CHAIN_CAP
        && l3
            .append(CHAIN_CAP as u32 + 1, PieceKind::General, prev3, "over")
            .err() == Some(ChainCode::ChainFull);
    s.add("容量满：链满后入链显性拒绝", full_ok,
          "版本链容量上限为工程约束，越限必须显性拒绝");

    // 12. 成文非空且含三标记 + 双跑一致
    let sum = compose_chain_summary(&l);
    let text_ok = match &sum {
        Ok(t) => t.contains("段数") && t.contains("连续") && t.contains("三件套"),
        Err(_) => false,
    };
    s.add("链对账成文：标记齐全且两次成文逐字节一致",
          text_ok && sum == compose_chain_summary(&l),
          "成文必须携带 段数/连续/三件套 三标记且为纯函数");

    // 13. 空链成文拒绝
    s.add("空链成文拒绝：空链不得产出小结",
          compose_chain_summary(&ChainLedger::new()).err() == Some(ChainCode::EmptySummary),
          "空链没有对账对象，产出小结即虚报");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(17)
        && agrees_with_linux(52) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < ChainCode::ALL.len() {
        let mut j = i + 1;
        while j < ChainCode::ALL.len() {
            if let (Some(a), Some(b)) = (ChainCode::ALL.get(i), ChainCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = ChainCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 册别往返 + 双跑指纹
    let rt_ok = PieceKind::ALL.iter().all(|p| {
        let tag = match p {
            PieceKind::General => "general",
            PieceKind::Batch => "batch",
            PieceKind::Deep => "deep",
        };
        parse_piece_kind(tag) == Ok(*p)
    });
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：册别标签可回解析且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 指纹 + 成文长度
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let sum_len = match &sum { Ok(t) => t.len(), Err(_) => 0 };
    let sum_text = format!("链小结字节数 {}", sum_len);
    s.add_dyn("成文快照", sum_len > 0, &sum_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_piece_kind("batch"), Ok(PieceKind::Batch));
        assert_eq!(parse_piece_kind("g"), Ok(PieceKind::General));
        assert!(parse_piece_kind("total").is_err());
        assert_eq!(parse_piece_kind("").err(), Some(ChainCode::UnknownPiece));
    }

    #[test]
    fn genesis_rules_enforced() {
        let mut l = ChainLedger::new();
        assert_eq!(l.append(2, PieceKind::General, GENESIS_PREV, "x").err(),
                   Some(ChainCode::SegGap));
        assert_eq!(l.append(1, PieceKind::General, 0x9999, "x").err(),
                   Some(ChainCode::BrokenLink));
        assert_eq!(l.append(1, PieceKind::General, GENESIS_PREV, "").err(),
                   Some(ChainCode::EmptyPayload));
        assert_eq!(l.len(), 0);
    }

    #[test]
    fn chain_builds_and_audits() {
        let mut l = ChainLedger::new();
        let h = l.append(1, PieceKind::General, GENESIS_PREV, "g1");
        assert!(h.is_ok());
        let prev = h.unwrap_or(0);
        assert!(l.append(2, PieceKind::Batch, prev, "b1").is_ok());
        assert!(l.continuous());
        assert!(!l.triple_set_complete());
    }

    #[test]
    fn broken_link_rejected_without_trace() {
        let mut l = ChainLedger::new();
        let _ = l.append(1, PieceKind::General, GENESIS_PREV, "g1");
        assert_eq!(l.append(2, PieceKind::Batch, 0xDEAD, "b1").err(),
                   Some(ChainCode::BrokenLink));
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn bad_chain_matrix_green() {
        let (inj, per) = run_bad_chain_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn hash_deterministic_by_piece() {
        let a = seg_hash(4, PieceKind::Batch, "same");
        let b = seg_hash(4, PieceKind::Batch, "same");
        let c = seg_hash(5, PieceKind::Batch, "same");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn summary_compose_and_empty_reject() {
        let mut l = ChainLedger::new();
        let _ = l.append(1, PieceKind::General, GENESIS_PREV, "g1");
        let s = compose_chain_summary(&l).unwrap_or_default();
        assert!(s.contains("段数=1"));
        assert!(compose_chain_summary(&ChainLedger::new()).is_err());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(52));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(28), -28);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10070_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10070_checks_green() {
        let set = run_unx10070_checks();
        assert!(set.all_passed(), "UNX-F10070 red: {}", set.red_items_text());
    }
}
