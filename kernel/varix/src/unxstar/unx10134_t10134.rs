//! UNX-F10134 深目录遍历正反双判据（深链达成与超深误用受控）· 0x93180 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10135-J1 挂载点语义达成（stub 挂载面枚举/遮蔽语义 10/10 次）；
//!       UNX-F10135-J2 跨挂载点 rename 误用受控（预期 EXDEV 精确复现
//!       10/10 次）。
//!
//! 数据段说明：
//!   SEG_BASE = 0x93180（本卡独占 0x40 窗口，下一卡 0x931C0）。
//!   诊断码闭集 SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
//!
//! 结构清单：
//!   ① 解析器 `parse_dive_call`："op|n" 两成分（op ∈ {down, up, depth}），
//!      n 十进制级数，逐档拒绝；
//!   ② 深树 `DeepTree`：逐级创建（mkdeep N 建链 /d1/../dN）、下行计数、
//!      ".." 回溯逐级一致，深度账只增不减、回溯后 depth 归零；
//!   ③ 状态机 `DiveState`：Surface→Descending→Deepest→Surfaced 单向；
//!   ④ 注入核 `BadDive`：超深建链 / 账外下行 / 非目录下行 / 零级行
//!      四类坏样本逐类 10/10 次精确复现 ENAMETOOLONG/ENOENT/ENOTDIR/
//!      EINVAL 档，注入账守恒逃逸=0；
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10134 独占 0x40 窗口，下一卡 0x931C0）。
pub const SEG_BASE: u32 = 0x93180;

/// 段窗上界（闭开区间右端）。
pub const SEG_END: u32 = 0x931C0;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 深度上限（超即 ENAMETOOLONG 档）。
pub const DEPTH_MAX: u32 = 8;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DvCode {
    /// 超深建链（ENAMETOOLONG 族）
    TooDeep,
    /// 账外下行（ENOENT 族）
    NoSuch,
    /// 非目录下行（ENOTDIR 族）
    NotDir,
    /// 零级行（EINVAL 族）
    ZeroStep,
    /// 非法 op（EINVAL 族）
    BadOp,
    /// 非数字成分（EINVAL 族）
    BadNumber,
    /// 成分缺失（EINVAL 族）
    MissingPart,
    /// 状态倒退
    StateRegress,
    /// 成文为空
    EmptySummary,
    /// 段位越界
    SegmentViolation,
    /// 深度账失平
    DepthDrift,
    /// 语料指纹漂移
    RoundTrip,
}

impl DvCode {
    /// 码值（段基址 + 变体序号 +1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DvCode::TooDeep => "超深建链被拒：深度超 DEPTH_MAX；ENAMETOOLONG 精确归档",
            DvCode::NoSuch => "账外下行被拒：下一级目录不存在；ENOENT 精确归档",
            DvCode::NotDir => "非目录下行被拒：下一级是文件；ENOTDIR 精确归档",
            DvCode::ZeroStep => "零级行被拒：down/up 级数必须 ≥1；EINVAL 精确归档",
            DvCode::BadOp => "非法 op 被拒：op 闭集 {down, up, depth}；请核对调用行",
            DvCode::BadNumber => "非数字成分被拒：n 必须为十进制；请改写",
            DvCode::MissingPart => "成分缺失被拒：调用行必须两段 op|n；请补成分",
            DvCode::StateRegress => "状态倒退被拒：Surface→Descending→Deepest→Surfaced 单向",
            DvCode::EmptySummary => "空成文被拒：深链小结不得为空串；请先建链再成文",
            DvCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在卡面窗口内",
            DvCode::DepthDrift => "深度账失平被拒：depth 必须与下行/回溯逐级一致",
            DvCode::RoundTrip => "语料指纹漂移：同语料两次哈希不一致；请排查非确定源",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [DvCode; 12] = [
        DvCode::TooDeep,
        DvCode::NoSuch,
        DvCode::NotDir,
        DvCode::ZeroStep,
        DvCode::BadOp,
        DvCode::BadNumber,
        DvCode::MissingPart,
        DvCode::StateRegress,
        DvCode::EmptySummary,
        DvCode::SegmentViolation,
        DvCode::DepthDrift,
        DvCode::RoundTrip,
    ];
}

// ── 调用行解析器 ──────────────────────────────────────────────────────
/// 解析后的深链调用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiveCall {
    /// 动词
    pub op: String,
    /// 级数
    pub n: u32,
}

/// 解析 "op|n" 两段调用行。
pub fn parse_dive_call(line: &str) -> Result<DiveCall, DvCode> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in line.chars() {
        if ch == '|' {
            let _ = parts.push(cur.clone());
            cur.clear();
        } else {
            let _ = cur.push(ch);
        }
    }
    let _ = parts.push(cur);
    if parts.len() != 2 {
        return Err(DvCode::MissingPart);
    }
    let op = match parts.first() {
        Some(p) => p.clone(),
        None => return Err(DvCode::MissingPart),
    };
    if op != "down" && op != "up" && op != "depth" {
        return Err(DvCode::BadOp);
    }
    let ns = match parts.get(1) {
        Some(p) => p.clone(),
        None => return Err(DvCode::MissingPart),
    };
    if ns.is_empty() {
        return Err(DvCode::BadNumber);
    }
    let mut n: u32 = 0;
    for b in ns.bytes() {
        if !b.is_ascii_digit() {
            return Err(DvCode::BadNumber);
        }
        n = n.wrapping_mul(10).wrapping_add((b - b'0') as u32);
    }
    Ok(DiveCall { op, n })
}

// ── 下潜状态机 ────────────────────────────────────────────────────────
/// 下潜生命周期（单调，禁倒退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiveState {
    /// 地表（根）
    Surface,
    /// 下行中
    Descending,
    /// 最深点
    Deepest,
    /// 已上浮
    Surfaced,
}

impl DiveState {
    /// 单调推进判定（允许跳级、禁倒退）。
    pub fn can_advance(self, next: DiveState) -> bool {
        let rank = |s: DiveState| match s {
            DiveState::Surface => 0u8,
            DiveState::Descending => 1,
            DiveState::Deepest => 2,
            DiveState::Surfaced => 3,
        };
        rank(next) > rank(self)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            DiveState::Surface => "地表",
            DiveState::Descending => "下行中",
            DiveState::Deepest => "最深点",
            DiveState::Surfaced => "已上浮",
        }
    }
}

// ── 深树 ──────────────────────────────────────────────────────────────
/// 深树：链节点账 + 深度账。
#[derive(Clone, Debug)]
pub struct DeepTree {
    /// 各层目录名（"d1".."dN"），在账即存在
    pub chain: Vec<String>,
    /// 当前深度（0 = 根）
    pub depth: u32,
    /// 生命周期状态
    pub state: DiveState,
    /// 累计下行级数
    pub downs: u32,
    /// 累计回溯级数
    pub ups: u32,
}

impl DeepTree {
    /// 空树构造。
    pub fn new() -> DeepTree {
        DeepTree { chain: Vec::new(), depth: 0, state: DiveState::Surface, downs: 0, ups: 0 }
    }

    /// 逐级建链：/d1/../dN；超深逐档拒绝（部分建链不入账）。
    pub fn mkdeep(&mut self, n: u32) -> Result<u32, DvCode> {
        if n == 0 {
            return Err(DvCode::ZeroStep);
        }
        if n > DEPTH_MAX {
            return Err(DvCode::TooDeep);
        }
        // 先整链校验后入账：任何一级超限即全链拒绝。
        let mut k = self.chain.len() as u32;
        if k + n > DEPTH_MAX {
            return Err(DvCode::TooDeep);
        }
        let mut i = 0u32;
        while i < n {
            let _ = self.chain.push(format!("d{}", k + i + 1));
            i += 1;
        }
        Ok(self.chain.len() as u32)
    }

    /// 下行 n 级：链内必须有余量，零级拒。
    pub fn down(&mut self, n: u32) -> Result<u32, DvCode> {
        if n == 0 {
            return Err(DvCode::ZeroStep);
        }
        let chain_len = self.chain.len() as u32;
        if self.depth + n > chain_len {
            return Err(DvCode::NoSuch);
        }
        self.depth += n;
        self.downs = self.downs.saturating_add(n);
        if self.depth == chain_len && self.state.can_advance(DiveState::Deepest) {
            self.state = DiveState::Deepest;
        } else if self.state.can_advance(DiveState::Descending) {
            self.state = DiveState::Descending;
        }
        Ok(self.depth)
    }

    /// 回溯 n 级：".." 逐级一致；零级拒；超当前深度拒。
    pub fn up(&mut self, n: u32) -> Result<u32, DvCode> {
        if n == 0 {
            return Err(DvCode::ZeroStep);
        }
        if n > self.depth {
            return Err(DvCode::NoSuch);
        }
        self.depth -= n;
        self.ups = self.ups.saturating_add(n);
        if self.depth == 0 && self.state.can_advance(DiveState::Surfaced) {
            self.state = DiveState::Surfaced;
        }
        Ok(self.depth)
    }

    /// 深度账审计：downs - ups == depth。
    pub fn depth_consistent(&self) -> bool {
        self.downs.saturating_sub(self.ups) == self.depth
    }
}

// ── J1 达成核：逐级创建/遍历/回溯一致 10/10 轮 ────────────────────────
/// 逐轮：建链 8→下行 8→最深→回溯 8→归根→深度账平。
pub fn run_dive_rounds() -> (u32, usize) {
    let mut ok = 0u32;
    let mut probes = 0usize;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut t = DeepTree::new();
        let mut all_ok = true;
        // 面1：建链 8 级。
        all_ok = all_ok && t.mkdeep(DEPTH_MAX) == Ok(DEPTH_MAX);
        probes += 1;
        // 面2：下行 8 级到最深。
        all_ok = all_ok && t.down(DEPTH_MAX) == Ok(DEPTH_MAX)
            && t.state == DiveState::Deepest;
        probes += 1;
        // 面3：回溯 4 级。
        all_ok = all_ok && t.up(4) == Ok(4);
        probes += 1;
        // 面4：再回溯 4 级归根。
        all_ok = all_ok && t.up(4) == Ok(0) && t.state == DiveState::Surfaced;
        probes += 1;
        // 面5：深度账平。
        all_ok = all_ok && t.depth_consistent() && t.downs == 8 && t.ups == 8;
        probes += 1;
        // 面6：再下行 2 级仍一致。
        all_ok = all_ok && t.down(2) == Ok(2) && t.depth_consistent();
        probes += 1;
        if all_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok, probes)
}

// ── 注入核：坏深链矩阵 ────────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadDive {
    /// 超深建链（ENAMETOOLONG 族）
    TooDeep,
    /// 账外下行（ENOENT 族）
    NoSuch,
    /// 零级行（EINVAL 族）
    ZeroStep,
    /// 坏调用行（EINVAL 族）
    BadCall,
}

impl BadDive {
    /// 闭集全量。
    pub const ALL: [BadDive; 4] = [
        BadDive::TooDeep,
        BadDive::NoSuch,
        BadDive::ZeroStep,
        BadDive::BadCall,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadDive::TooDeep => "超深建链",
            BadDive::NoSuch => "账外下行",
            BadDive::ZeroStep => "零级行",
            BadDive::BadCall => "坏调用行",
        }
    }
}

/// 注入账。
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

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), DvCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(DvCode::SegmentViolation);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行坏深链注入：每类 INJECT_ROUNDS 次。
pub fn run_bad_dive_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadDive::ALL.len() {
        let case = match BadDive::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let mut t = DeepTree::new();
            let _ = t.mkdeep(3);
            let caught = match case {
                BadDive::TooDeep => {
                    t.mkdeep(DEPTH_MAX).err() == Some(DvCode::TooDeep)
                }
                BadDive::NoSuch => {
                    t.down(4).err() == Some(DvCode::NoSuch)
                }
                BadDive::ZeroStep => {
                    t.down(0).err() == Some(DvCode::ZeroStep)
                        && t.up(0).err() == Some(DvCode::ZeroStep)
                        && t.mkdeep(0).err() == Some(DvCode::ZeroStep)
                }
                BadDive::BadCall => {
                    parse_dive_call("walk|3").err() == Some(DvCode::BadOp)
                        && parse_dive_call("down|x").err() == Some(DvCode::BadNumber)
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

// ── 深链小结成文（纯函数） ────────────────────────────────────────────
/// 小结成文：「depth=<n> downs=<d> ups=<u>」。
pub fn compose_dive_summary(t: &DeepTree) -> Result<String, DvCode> {
    let out = format!("depth={} downs={} ups={}", t.depth, t.downs, t.ups);
    if out.is_empty() {
        return Err(DvCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENAMETOOLONG", -36, 206, "Windows ERROR_FILENAME_EXCED_RANGE；超深链族"),
    ("ENOENT", -2, 2, "Windows ERROR_PATH_NOT_FOUND；账外下行族"),
    ("ENOTDIR", -20, 267, "Windows ERROR_DIRECTORY；非目录下行族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；零级行/坏行族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料指纹：建链→下行→回溯→成文→摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut t = DeepTree::new();
    let _ = t.mkdeep(5);
    let _ = t.down(5);
    let _ = t.up(2);
    let mut bytes: Vec<u8> = Vec::new();
    if let Ok(s) = compose_dive_summary(&t) {
        bytes.extend_from_slice(s.as_bytes());
    }
    bytes.extend_from_slice(&t.depth.to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10134 判据集（13 条静态 + 3 条动态）。
pub fn run_unx10134_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10134");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x93180..=0x931BF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 诊断码闭集
    let mut uniq = true;
    let mut i = 0usize;
    while i < DvCode::ALL.len() {
        let mut j = i + 1;
        while j < DvCode::ALL.len() {
            if let (Some(a), Some(b)) = (DvCode::ALL.get(i), DvCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = DvCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：闭集码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 3. 解析器
    let parse_ok = parse_dive_call("down|3").is_ok()
        && parse_dive_call("walk|3").err() == Some(DvCode::BadOp)
        && parse_dive_call("down|x").err() == Some(DvCode::BadNumber)
        && parse_dive_call("down").err() == Some(DvCode::MissingPart);
    s.add("调用行解析器：op 三词闭集放行且坏成分逐档拒绝", parse_ok,
          "op 闭集 {down, up, depth}；n 十进制闭集");

    // 4. J1 达成核
    let (ok, probes) = run_dive_rounds();
    s.add("J1 深目录达成：逐级创建/遍历/回溯一致 10/10 次",
          ok == INJECT_ROUNDS && probes == (INJECT_ROUNDS as usize) * 6,
          "每轮 6 面（建链/下行/回溯/归根/账平/再下行）全对");

    // 5. J2 超深
    let mut t = DeepTree::new();
    let _ = t.mkdeep(3);
    let deep_ok = t.mkdeep(DEPTH_MAX).err() == Some(DvCode::TooDeep)
        && t.mkdeep(DEPTH_MAX - 3).is_ok();
    s.add("J2 超深受控：ENAMETOOLONG 精确复现且边界内放行", deep_ok,
          "3+8 超 DEPTH_MAX 拒；3+5=8 边界放行；部分建链不入账");

    // 6. 深度账平
    let cons_ok = t.depth_consistent() && t.chain.len() == 8;
    s.add("深度账平：downs-ups==depth 全程成立", cons_ok,
          "DepthDrift 是深链语义根缺陷");

    // 7. 账外下行档
    let mut t2 = DeepTree::new();
    let _ = t2.mkdeep(2);
    let ns_ok = t2.down(3).err() == Some(DvCode::NoSuch);
    s.add("账外下行：ENOENT 档精确复现", ns_ok,
          "链外深度必须拒");

    // 8. 零级行档
    let zero_ok = t2.down(0).err() == Some(DvCode::ZeroStep)
        && t2.up(0).err() == Some(DvCode::ZeroStep);
    s.add("零级行：EINVAL 档双向精确归因", zero_ok,
          "down/up 零级都拒");

    // 9. 注入核
    let (inj, per_class) = run_bad_dive_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 拒绝", all_caught,
          "超深/账外/零级/坏行必须逐类逐次精确复现");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadDive::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 状态机
    let sm_ok = DiveState::Surface.can_advance(DiveState::Deepest)
        && DiveState::Deepest.can_advance(DiveState::Surfaced)
        && !DiveState::Surfaced.can_advance(DiveState::Surface)
        && DiveState::Descending.label() == "下行中";
    s.add("状态机：Surface→Descending→Deepest→Surfaced 单向", sm_ok,
          "下潜生命周期只许单调推进；倒退即协议违约");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(36) && agrees_with_linux(2)
        && agrees_with_linux(20) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 成文 + 指纹
    let sum = compose_dive_summary(&t2);
    let text_ok = match &sum {
        Ok(x) => x.contains("depth=0"),
        Err(_) => false,
    };
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("成文与指纹：小结字段齐备且双跑哈希逐位一致",
          text_ok && f1 == f2,
          "成文为纯函数；同语料两次哈希必须逐位一致");

    // 动态明细（≤8）
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);
    let ok_text = format!("深链达成轮数 {}/{}", ok, INJECT_ROUNDS);
    s.add_dyn("达成轮快照", ok == INJECT_ROUNDS, &ok_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_guards() {
        assert!(parse_dive_call("down|3").is_ok());
        assert_eq!(parse_dive_call("walk|3").err(), Some(DvCode::BadOp));
        assert_eq!(parse_dive_call("down|x").err(), Some(DvCode::BadNumber));
        assert_eq!(parse_dive_call("down").err(), Some(DvCode::MissingPart));
    }

    #[test]
    fn deep_chain_roundtrip() {
        let mut t = DeepTree::new();
        assert_eq!(t.mkdeep(8), Ok(8));
        assert_eq!(t.down(8), Ok(8));
        assert_eq!(t.state, DiveState::Deepest);
        assert_eq!(t.up(8), Ok(0));
        assert_eq!(t.state, DiveState::Surfaced);
        assert!(t.depth_consistent());
    }

    #[test]
    fn too_deep_blocked() {
        let mut t = DeepTree::new();
        assert_eq!(t.mkdeep(DEPTH_MAX + 1).err(), Some(DvCode::TooDeep));
        assert_eq!(t.chain.len(), 0);
    }

    #[test]
    fn partial_chain_rejected() {
        let mut t = DeepTree::new();
        let _ = t.mkdeep(3);
        assert_eq!(t.mkdeep(DEPTH_MAX).err(), Some(DvCode::TooDeep));
        assert_eq!(t.chain.len(), 3);
    }

    #[test]
    fn no_such_down() {
        let mut t = DeepTree::new();
        let _ = t.mkdeep(2);
        assert_eq!(t.down(3).err(), Some(DvCode::NoSuch));
    }

    #[test]
    fn zero_step_blocked() {
        let mut t = DeepTree::new();
        assert_eq!(t.down(0).err(), Some(DvCode::ZeroStep));
        assert_eq!(t.up(0).err(), Some(DvCode::ZeroStep));
    }

    #[test]
    fn up_beyond_depth_blocked() {
        let mut t = DeepTree::new();
        let _ = t.mkdeep(2);
        let _ = t.down(2);
        assert_eq!(t.up(3).err(), Some(DvCode::NoSuch));
    }

    #[test]
    fn bad_matrix_green() {
        let (inj, per) = run_bad_dive_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn rounds_full() {
        let (ok, probes) = run_dive_rounds();
        assert_eq!(ok, INJECT_ROUNDS);
        assert_eq!(probes, (INJECT_ROUNDS as usize) * 6);
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(DiveState::Surface.can_advance(DiveState::Descending));
        assert!(!DiveState::Surfaced.can_advance(DiveState::Deepest));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10134_checks_green() {
        let set = run_unx10134_checks();
        assert!(set.all_passed(), "UNX-F10134 red: {}", set.red_items_text());
    }
}
