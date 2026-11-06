//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F5462 · 回归矩阵账 · 0x72F40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B2-B34）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F5462-J1 回归矩阵账落定（40 批 × 断言集覆盖清单），
//!     矩阵可导出且零缺批；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 40 批（b01..b40）× 每批 25 条断言（A01..A25）覆盖位图；格只许覆一
//!     次（重覆拒），封盘后再填拒，批号/断言号越界拒；
//!   · 填格序确定性派生：批内断言按 7 倍置换序走，保证 1000 格全覆且
//!     可复算（派生复核器逐格对照公式，一致率 100%）；
//!   · 缺批探针：39 批半成品矩阵必须被缺批检出器抓到（MatrixHole）；
//!   · 矩阵可导出：@VXM1 头 + 每批一行覆盖清单（A01,A02,...），双路
//!     渲染（format 一体成型 vs 逐字段拼接）逐字节对拍；
//!   · 失真注入 10 路逐字失真，核验器 10/10 必抓；
//!   · 开销口径：簿记（头渲染+格簿记）vs 载荷（清单正文+校验和），≤2%。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72F40，VX3-1103 回归矩阵窗口独占）。
pub const SEG_BASE: u32 = 0x72F40;
/// 批数（判据原文 40 批，冻结）。
pub const BATCH_N: usize = 40;
/// 每批断言数（25 条，冻结）。
pub const ASSERT_N: usize = 25;
/// 置换乘子（批内断言遍历序 = (i*7) mod 25，与 25 互素保证全覆）。
pub const PERM_MUL: usize = 7;
/// 失真注入轮数（判据 10 路语义，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 开销占比上限（≤2% = 200 bps，冻结）。
pub const OVERHEAD_CAP_BPS: u64 = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatCode {
    /// 批号越界：批下标 ≥40
    BatchRange,
    /// 断言号越界：断言下标 ≥25
    AssertRange,
    /// 矩阵倒退：封盘后再填格、状态机平移/倒退一律拒
    CursorBack,
    /// 重覆覆盖：同一格二次落账（格只许覆一次）
    CellDup,
    /// 缺批缺格：导出件核验发现覆盖不满（缺批或缺断言）
    MatrixHole,
    /// 导出件破损：头字段缺失、数值域不合法或校验和对不上
    ExportBroken,
    /// 对拍失真：导出路径与直读路径逐字节不一致
    CrossMiss,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl MatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MatCode::BatchRange => "批号越界：批下标必须 <40，先核批账再落格",
            MatCode::AssertRange => "断言号越界：断言下标必须 <25，先核断言集再落格",
            MatCode::CursorBack => "矩阵倒退：封盘后再填格一律拒，状态机只进不退",
            MatCode::CellDup => "重覆覆盖：同一格只许覆一次，二次落账按簿记坏拒",
            MatCode::MatrixHole => "缺批缺格：覆盖不满 40×25，先按缺批检出器补齐再导出",
            MatCode::ExportBroken => "导出件破损：头字段缺失或校验和对不上，重出矩阵件",
            MatCode::CrossMiss => "对拍失真：导出件与直读渲染不一致，逐字节定位差异批行",
            MatCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [MatCode; 8] = [
        MatCode::BatchRange,
        MatCode::AssertRange,
        MatCode::CursorBack,
        MatCode::CellDup,
        MatCode::MatrixHole,
        MatCode::ExportBroken,
        MatCode::CrossMiss,
        MatCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    for i in 0..(MatCode::ALL.len()) {
        if MatCode::ALL[i].val() == code {
            return MatCode::ALL[i].detail();
        }
    }
    "未知诊断码：不在 0x72F40 段闭集内"
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatState {
    /// 草稿：一格未覆。
    Draft,
    /// 填充中：至少一格已覆，尚未满覆盖。
    Filling,
    /// 已满覆盖：40×25 全覆，可导出定案。
    Complete,
    /// 已封盘：矩阵冻结，任何再填按倒退拒。
    Sealed,
}

impl MatState {
    /// 单调序值：Draft=0 < Filling=1 < Complete=2 < Sealed=3。
    pub fn rank(self) -> u8 {
        match self {
            MatState::Draft => 0,
            MatState::Filling => 1,
            MatState::Complete => 2,
            MatState::Sealed => 3,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: MatState) -> bool {
        to.rank() > self.rank()
    }
}

/// 批名：下标 → b01..b40。
pub fn batch_name(b: usize) -> String {
    format!("b{:02}", b + 1)
}

/// 断言名：下标 → A01..A25。
pub fn assert_name(a: usize) -> String {
    format!("A{:02}", a + 1)
}

/// 填格序置换：批内第 s 步走断言 (s*7) mod 25（与 25 互素，全覆可复算）。
pub fn perm_assert(s: usize) -> usize {
    (s.wrapping_mul(PERM_MUL)) % ASSERT_N
}

/// 回归矩阵：覆盖位图 + 填格账 + 状态机。
#[derive(Clone, Debug)]
pub struct RegrMatrix {
    pub epoch: u64,
    /// 覆盖位图：covered[批][断言]。
    pub covered: Vec<Vec<bool>>,
    /// 有效落格数（=满覆盖时 1000）。
    pub fills: u64,
    /// 被拒的重覆落格数（显性记账，不静默吞）。
    pub dups: u64,
    pub state: MatState,
}

impl RegrMatrix {
    pub fn new(epoch: u64) -> RegrMatrix {
        let mut covered: Vec<Vec<bool>> = Vec::new();
        let mut b = 0usize;
        while b < BATCH_N {
            covered.push((0..ASSERT_N).map(|_| false).collect());
            b += 1;
        }
        RegrMatrix { epoch, covered, fills: 0, dups: 0, state: MatState::Draft }
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance_state(&mut self, to: MatState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(MatCode::CursorBack.val());
        }
        self.state = to;
        Ok(())
    }

    /// 封盘：矩阵冻结。
    pub fn seal(&mut self) -> Result<(), u32> {
        self.advance_state(MatState::Sealed)
    }

    /// 落一格：越界/封盘/重覆三类显性拒；满覆盖自动推进 Complete。
    pub fn fill_cell(&mut self, b: usize, a: usize) -> Result<(), u32> {
        if b >= BATCH_N {
            return Err(MatCode::BatchRange.val());
        }
        if a >= ASSERT_N {
            return Err(MatCode::AssertRange.val());
        }
        if self.state == MatState::Sealed {
            return Err(MatCode::CursorBack.val());
        }
        if self.covered[b][a] {
            self.dups += 1;
            return Err(MatCode::CellDup.val());
        }
        self.covered[b][a] = true;
        self.fills += 1;
        if self.state == MatState::Draft {
            self.state = MatState::Filling;
        }
        if self.complete() && self.state == MatState::Filling {
            self.state = MatState::Complete;
        }
        Ok(())
    }

    /// 满覆盖判定：40 批 × 25 断言全真。
    pub fn complete(&self) -> bool {
        for b in 0..BATCH_N {
            for a in 0..ASSERT_N {
                if !self.covered[b][a] {
                    return false;
                }
            }
        }
        true
    }

    /// 缺批检出：返回存在未覆格的批下标清单（零缺批判定 = 清单空）。
    pub fn missing_batches(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for b in 0..BATCH_N {
            let mut a = 0usize;
            let mut any_missing = false;
            while a < ASSERT_N {
                if a < self.covered[b].len() && !self.covered[b][a] {
                    any_missing = true;
                }
                a += 1;
            }
            if any_missing || self.covered[b].len() < ASSERT_N {
                out.push(b);
            }
        }
        out
    }

    /// 已覆盖格数（对账口径，与 fills 独立复算）。
    pub fn covered_count(&self) -> u64 {
        let mut n = 0u64;
        for b in 0..(self.covered.len()) {
            for a in 0..(self.covered[b].len()) {
                if self.covered[b][a] {
                    n += 1;
                }
            }
        }
        n
    }
}

/// 派生填格器：按确定性序把 40×25 全覆（每格恰好一次）。
pub fn fill_derived(m: &mut RegrMatrix) -> Result<u64, u32> {
    for s in 0..(BATCH_N * ASSERT_N) {
        let b = s / ASSERT_N;
        let a = perm_assert(s % ASSERT_N);
        m.fill_cell(b, a)?;
    }
    Ok(m.fills)
}

pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for i in 0..(bytes.len()) {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

/// 批行渲染（导出路径）：b01|list=A01,A02,...|cov=25/25（format 一体成型）。
pub fn render_batch_line(m: &RegrMatrix, b: usize) -> String {
    let mut list = String::new();
    let mut a = 0usize;
    let mut first = true;
    while a < ASSERT_N {
        if m.covered[b][a] {
            if first {
                first = false;
            } else {
                list.push(',');
            }
            list.push_str(&assert_name(a));
        }
        a += 1;
    }
    let mut cov = 0u32;
    for a2 in 0..ASSERT_N {
        if m.covered[b][a2] {
            cov += 1;
        }
    }
    format!("{}|list={}|cov={}/{}\n", batch_name(b), list, cov, ASSERT_N)
}

/// 批行渲染（直读路径）：逐字段拼接（结构独立，供对拍基准）。
pub fn direct_batch_line(m: &RegrMatrix, b: usize) -> String {
    let mut s = String::new();
    s.push_str(&batch_name(b));
    s.push_str("|list=");
    let mut a = 0usize;
    let mut first = true;
    while a < ASSERT_N {
        if m.covered[b][a] {
            if first {
                first = false;
            } else {
                s.push(',');
            }
            s.push_str(&assert_name(a));
        }
        a += 1;
    }
    let mut cov = 0u32;
    for a2 in 0..ASSERT_N {
        if m.covered[b][a2] {
            cov += 1;
        }
    }
    s.push_str("|cov=");
    s.push_str(&cov.to_string());
    s.push('/');
    s.push_str(&ASSERT_N.to_string());
    s.push('\n');
    s
}

/// 全矩阵正文渲染（导出路径）：40 批行顺序成型。
pub fn render_matrix_body(m: &RegrMatrix) -> String {
    let mut s = String::new();
    for b in 0..BATCH_N {
        s.push_str(&render_batch_line(m, b));
    }
    s
}

/// 全矩阵正文渲染（直读路径）：40 批行逐字段拼接。
pub fn direct_matrix_body(m: &RegrMatrix) -> String {
    let mut s = String::new();
    for b in 0..BATCH_N {
        s.push_str(&direct_batch_line(m, b));
    }
    s
}

/// 快照头：@VXM1|epoch|batches|asserts|covered|sum;
pub fn header_mat(epoch: u64, batches: u64, asserts: u64, covered: u64, sum: u32) -> String {
    format!(
        "@VXM1|epoch={}|batches={}|asserts={}|covered={}|sum={:08X};\n",
        epoch, batches, asserts, covered, sum
    )
}

/// 解析后的矩阵头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatHeader {
    pub epoch: u64,
    pub batches: u64,
    pub asserts: u64,
    pub covered: u64,
    pub sum: u32,
}

/// 字节查找（ASCII 正文专用）。
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() {
        if h[i] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 子串查找（ASCII 专用）。
pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() {
        return None;
    }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] {
            k += 1;
        }
        if k == pat.len() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 十进制无符号解析（checked，防溢出）。
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u64 = 0;
    for i in 0..(b.len()) {
        if b[i] < b'0' || b[i] > b'9' {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as u64)?;
    }
    Some(v)
}

/// 8 位十六进制解析（校验和域专用）。
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    let b = s.as_bytes();
    let mut v: u32 = 0;
    for i in 0..8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
    }
    Some(v)
}

/// 矩阵头解析：逐 token 扫描，未知字段即破损。
pub fn parse_header(hdr: &str) -> Option<MatHeader> {
    let mut epoch: Option<u64> = None;
    let mut batches: Option<u64> = None;
    let mut asserts: Option<u64> = None;
    let mut covered: Option<u64> = None;
    let mut sum: Option<u32> = None;
    let bts = hdr.as_bytes();
    let mut i = 0usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') {
            Some(p) => p,
            None => bts.len(),
        };
        let tok = &hdr[i..end];
        if !tok.is_empty() {
            let eq = find_byte(tok.as_bytes(), 0, b'=')?;
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            match k {
                "epoch" => epoch = Some(parse_dec(v)?),
                "batches" => batches = Some(parse_dec(v)?),
                "asserts" => asserts = Some(parse_dec(v)?),
                "covered" => covered = Some(parse_dec(v)?),
                "sum" => sum = Some(parse_hex8(v)?),
                _ => return None,
            }
        }
        if end >= bts.len() {
            break;
        }
        i = end + 1;
    }
    Some(MatHeader { epoch: epoch?, batches: batches?, asserts: asserts?, covered: covered?, sum: sum? })
}

/// 导出矩阵件：快照头 + 40 批行（要求满覆盖，缺批缺格拒导）。
pub fn export_matrix(m: &RegrMatrix) -> Result<String, u32> {
    if !m.complete() {
        return Err(MatCode::MatrixHole.val());
    }
    let body = render_matrix_body(m);
    let hdr = header_mat(m.epoch, BATCH_N as u64, ASSERT_N as u64, m.covered_count(), fnv1a(body.as_bytes()));
    let mut out = hdr;
    out.push_str(&body);
    Ok(out)
}

/// 正文裸控制符扫描（<0x20 且非换行 → 破损）。
fn body_control_scan(body: &str) -> Result<(), u32> {
    let b = body.as_bytes();
    for i in 0..(b.len()) {
        if b[i] < 0x20 && b[i] != b'\n' {
            return Err(MatCode::ExportBroken.val());
        }
    }
    Ok(())
}

/// 矩阵件核验：头字段 + 满覆盖 + 正文逐字节对拍 + 校验和 + 行数锚。
pub fn verify_matrix(m: &RegrMatrix, out: &str) -> Result<(), u32> {
    let ob = out.as_bytes();
    if out.len() < 7 || !out.starts_with("@VXM1|") {
        return Err(MatCode::ExportBroken.val());
    }
    let semi = match find_byte(ob, 0, b';') {
        Some(p) => p,
        None => return Err(MatCode::ExportBroken.val()),
    };
    let h = match parse_header(&out[6..semi]) {
        Some(h) => h,
        None => return Err(MatCode::ExportBroken.val()),
    };
    if h.epoch != m.epoch || h.batches != BATCH_N as u64 || h.asserts != ASSERT_N as u64 {
        return Err(MatCode::CrossMiss.val());
    }
    if h.covered != (BATCH_N * ASSERT_N) as u64 {
        return Err(MatCode::MatrixHole.val());
    }
    let mut start = semi + 1;
    if start < out.len() && ob[start] == b'\n' {
        start += 1;
    }
    let body = &out[start..];
    body_control_scan(body)?;
    if body != render_matrix_body(m) {
        return Err(MatCode::CrossMiss.val());
    }
    let lines = count_lines(body);
    if lines != BATCH_N {
        return Err(MatCode::MatrixHole.val());
    }
    if h.sum != fnv1a(body.as_bytes()) {
        return Err(MatCode::ExportBroken.val());
    }
    Ok(())
}

/// 正文行数（'\n' 计数）。
pub fn count_lines(s: &str) -> usize {
    let b = s.as_bytes();
    let mut n = 0usize;
    for i in 0..(b.len()) {
        if b[i] == b'\n' {
            n += 1;
        }
    }
    n
}

/// 派生复核器：逐格对照填格公式 (批=s/25, 断言=(s%25)*7%25)，
/// 一致率 100% 返回 true（账与公式互证）。
pub fn derivation_check(m: &RegrMatrix) -> bool {
    for s in 0..(BATCH_N * ASSERT_N) {
        let b = s / ASSERT_N;
        let a = perm_assert(s % ASSERT_N);
        if b >= m.covered.len() || a >= m.covered[b].len() || !m.covered[b][a] {
            return false;
        }
    }
    m.covered_count() == (BATCH_N * ASSERT_N) as u64
}

/// 复制 [lo, hi] 闭区间字节（越界自动夹到流长）。
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}

/// 头域单字节改写：定位 pat，把其后首个字节改写（保证与原值不同）。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) {
        Some(pp) => pp + pat.len(),
        None => usize::MAX,
    };
    for i in 0..(b.len()) {
        if i == tgt {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else {
            v.push(b[i]);
        }
    }
}

/// 快照头行尾下标（头行 ';' 后换行的位置；找不到取全长）。
fn header_line_end(b: &[u8]) -> usize {
    match find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')) {
        Some(x) => x,
        None => b.len(),
    }
}

/// 对黄金矩阵件注入第 kind 路失真，返回坏件（内容全 ASCII，安全）。
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = header_line_end(b);
    match kind % 10 {
        0 => {
            let mut flipped = false;
            for i in 0..(b.len()) {
                if i > he && b[i] != b'\n' && !flipped {
                    v.push(b[i] ^ 0x01);
                    flipped = true;
                } else {
                    v.push(b[i]);
                }
            }
        }
        1 => patch_after(b, b"covered=", b'9', b'1', &mut v),
        2 => patch_after(b, b"sum=", b'0', b'F', &mut v),
        5 => patch_after(b, b"epoch=", b'9', b'1', &mut v),
        7 => patch_after(b, b"batches=", b'0', b'1', &mut v),
        3 => {
            let le = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, le + 1, b.len(), &mut v);
        }
        4 => push_range(b, 0, b.len().saturating_sub(7), &mut v), // 截尾：去末 6 字节
        6 => {
            let l1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            let l2 = find_byte(b, l1 + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, l2, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l2 + 1, b.len(), &mut v);
        }
        8 => {
            push_range(b, 0, he, &mut v);
            v.push(0x01);
            push_range(b, he + 1, b.len(), &mut v);
        }
        _ => {
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"b99|list=A01|cov=1/25\n";
            for k in 0..(tail.len()) {
                v.push(tail[k]);
            }
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮对新鲜黄金矩阵件注入一路失真，核验器必须抓到
///（黄金件本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    for r in 0..rounds {
        let mut m = RegrMatrix::new(7 + r as u64);
        let _ = fill_derived(&mut m)?;
        let out = export_matrix(&m)?;
        verify_matrix(&m, &out)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_matrix(&m, &distort(&out, kind)).is_err());
    }
    Ok(led)
}

/// 缺批检出：39 批满 + 1 批缺 1 格的半成品矩阵，缺批检出器必须报 1 批，
/// 且导出与核验均按缺批拒（MatrixHole）。
pub fn probe_missing_batch() -> bool {
    let mut m = RegrMatrix::new(9);
    let mut s = 0usize;
    while s < BATCH_N * ASSERT_N {
        let b = s / ASSERT_N;
        let a = perm_assert(s % ASSERT_N);
        if b == 17 && a == perm_assert(0) {
            s += 1;
            continue;
        }
        let _ = m.fill_cell(b, a);
        s += 1;
    }
    let miss = m.missing_batches();
    let miss_ok = miss.len() == 1 && miss[0] == 17;
    let export_rej = export_matrix(&m).is_err();
    let _ = m.fill_cell(17, perm_assert(0));
    let export_ok = export_matrix(&m).is_ok();
    miss_ok && export_rej && export_ok
}

/// 双路对拍：导出路径 40 批行 vs 直读路径 40 批行逐字节全等。
pub fn matrix_cross(m: &RegrMatrix) -> Result<u32, u32> {
    for b in 0..BATCH_N {
        let a = match m.covered.get(b) {
            Some(row) => row,
            None => return Err(MatCode::BatchRange.val()),
        };
        let _ = a;
        if render_batch_line(m, b) != direct_batch_line(m, b) {
            return Err(MatCode::CrossMiss.val());
        }
    }
    if render_matrix_body(m) != direct_matrix_body(m) {
        return Err(MatCode::CrossMiss.val());
    }
    Ok(BATCH_N as u32)
}

/// 全链：派生填格 1000 格 → 导出+核验 → 双路对拍 → 派生复核 → 六拒探
/// → 开销闸。返回 (正旗数, 拒旗数, 载荷账, 填格数)。
pub fn golden_run() -> Result<(u32, u32, u64, u64), u32> {
    let mut m = RegrMatrix::new(7);
    let fills = fill_derived(&mut m)?;
    let out = export_matrix(&m)?;
    verify_matrix(&m, &out)?;
    let crossed = matrix_cross(&m)?;
    let deriv = derivation_check(&m);
    let mut m2 = RegrMatrix::new(7);
    let _ = m2.fill_cell(0, 0);
    let rej_dup = m2.fill_cell(0, 0).is_err();
    let rej_batch = m2.fill_cell(BATCH_N, 0).is_err();
    let rej_assert = m2.fill_cell(0, ASSERT_N).is_err();
    m.seal()?;
    let rej_sealed = m.fill_cell(0, 0).is_err();
    let mut m3 = RegrMatrix::new(7);
    for s in 0..((BATCH_N * ASSERT_N) - 1) {
        let b = s / ASSERT_N;
        let a = perm_assert(s % ASSERT_N);
        let _ = m3.fill_cell(b, a);
    }
    let rej_hole = export_matrix(&m3).is_err() && m3.missing_batches().len() == 1;
    let rej_tamper = verify_matrix(&m, &distort(&out, 2)).is_err();
    let body = render_matrix_body(&m);
    let hdr = header_mat(m.epoch, BATCH_N as u64, ASSERT_N as u64, m.covered_count(), 0);
    let admin = hdr.len() as u64 + 2;
    let payload = body.len() as u64 * 2;
    let bps = admin.saturating_mul(10_000) / payload;
    if bps > OVERHEAD_CAP_BPS {
        return Err(MatCode::ExportBroken.val());
    }
    let flags = (verify_matrix(&m, &out).is_ok()) as u32 + (crossed == BATCH_N as u32) as u32
        + deriv as u32 + probe_missing_batch() as u32;
    let rejects = rej_dup as u32 + rej_batch as u32 + rej_assert as u32
        + rej_sealed as u32 + rej_hole as u32 + rej_tamper as u32;
    Ok((flags, rejects, payload, fills))
}

/// 序 A：黄金全链四正六拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 6, _, _)) => Ok(()),
        _ => Err(MatCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(MatCode::CrossMiss.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72F40 && MatCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = BATCH_N == 40 && ASSERT_N == 25 && PERM_MUL == 7
        && JUDGE_ROUNDS == 10 && OVERHEAD_CAP_BPS == 200 && in_window
        && (PERM_MUL * 8) % ASSERT_N != 0 && (PERM_MUL * 25) % ASSERT_N == 0;
    if ok {
        Ok(())
    } else {
        Err(MatCode::NotReproducible.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MatCode::NotReproducible.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    for i in 0..(MatCode::ALL.len()) {
        let v = MatCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

pub fn header_roundtrip() -> bool {
    let sum = fnv1a(b"probe");
    let hm = header_mat(11, 40, 25, 1000, sum);
    let a = match parse_header(&hm[6..hm.len() - 2]) {
        Some(h) => h.epoch == 11 && h.batches == 40 && h.asserts == 25 && h.covered == 1000 && h.sum == sum,
        None => false,
    };
    let bad = parse_header("epoch=7|ghost=1|sum=00000000").is_none()
        && parse_header("epoch=x|sum=00000000").is_none()
        && parse_header("epoch=7|sum=00").is_none();
    a && bad
}

pub fn probe_state_mono() -> bool {
    let mut m = RegrMatrix::new(5);
    let a = m.advance_state(MatState::Filling).is_ok();
    let b = m.advance_state(MatState::Draft).is_err();
    let c = m.advance_state(MatState::Filling).is_err();
    let d = m.advance_state(MatState::Complete).is_ok();
    let e = m.advance_state(MatState::Sealed).is_ok();
    let f = m.advance_state(MatState::Draft).is_err();
    let g = m.seal().is_err();
    a && b && c && d && e && f && g
}

pub fn writeback_summary() -> Result<String, u32> {
    let mut m = RegrMatrix::new(7);
    let fills = fill_derived(&mut m)?;
    let out = export_matrix(&m)?;
    verify_matrix(&m, &out)?;
    let crossed = matrix_cross(&m)?;
    Ok(format!(
        "状态=已完成 batches={} asserts={} fills={} crossed={} lines={}",
        BATCH_N, ASSERT_N, fills, crossed, count_lines(&out)
    ))
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, p, c)) => (*f == 4 && *r == 6, format!("flags={} rejects={} payload={} fills={}", f, r, p, c)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-矩阵账落定", gp, gd));
    let mut m = RegrMatrix::new(7);
    let fr = fill_derived(&mut m);
    let full = m.complete() && m.missing_batches().is_empty();
    v.push(("J1-满覆盖零缺批", full && fr.is_ok(), format!("fills={} covered={}", m.fills, m.covered_count())));
    let out = export_matrix(&m);
    let vp = match &out {
        Ok(o) => verify_matrix(&m, o).is_ok(),
        Err(_) => false,
    };
    v.push(("J1-矩阵可导出", vp, String::from("@VXM1 头+40 批行，五重闸全过")));
    let cr = matrix_cross(&m);
    v.push(("J1-双路对拍100%", matches!(&cr, Ok(n) if *n == BATCH_N as u32), format!("crossed={:?}", cr)));
    v.push(("J1-派生复核100%", derivation_check(&m), String::from("填格账与 (s*7)%25 置换公式逐格互证")));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    let body = render_matrix_body(&m);
    let hdr = header_mat(m.epoch, BATCH_N as u64, ASSERT_N as u64, m.covered_count(), 0);
    let bps = (hdr.len() as u64 + 2).saturating_mul(10_000) / (body.len() as u64 * 2);
    v.push((
        "J1-开销占比≤2%",
        bps <= OVERHEAD_CAP_BPS,
        format!("admin={} payload={} bps={} cap={}", hdr.len() + 2, body.len() * 2, bps, OVERHEAD_CAP_BPS),
    ));
    v.push(("J1-缺批检出器", probe_missing_batch(), String::from("39 批半成品缺批检出=1 且导出拒")));
    let hd = String::from("建头→解析逐域对账；未知字段/坏数字/短校验和必败");
    v.push(("J1-快照头校验", header_roundtrip(), hd));
    let mut m2 = RegrMatrix::new(7);
    let r1 = m2.fill_cell(0, 0).is_ok();
    let r2 = m2.fill_cell(0, 0).is_err();
    let r3 = m2.fill_cell(BATCH_N, 0).is_err();
    let r4 = m2.fill_cell(0, ASSERT_N).is_err();
    let st = probe_state_mono();
    v.push((
        "J2-单调守卫",
        r1 && r2 && r3 && r4 && st,
        format!("覆{}重{}批{}断{} 状态机={}", r1, r2, r3, r4, st),
    ));
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72F40 窗口")));
    v
}

pub fn run_unx5462_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F5462 回归矩阵账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
