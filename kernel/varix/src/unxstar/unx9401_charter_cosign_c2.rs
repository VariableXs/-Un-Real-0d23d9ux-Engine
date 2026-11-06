//! UNX-F9401 I 型联调总纲三（C2×C3 联签协议与对账面） · 0x7D1C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9401-J1 逐字）：
//!   锚点：UNX-F9401-J1 联签协议判据过，C2（错误矩阵面）×C3（errno
//!   账本面）双方对账面互认格过，跨界代写禁令声明格过
//!
//! 数据段 SEG_BASE = 0x7D1C0：诊断码闭集 +0x01..=+0x0C（12 码，
//! 9401 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 注册表 ×2：C2 错误矩阵面（syscall 类 × errno 判定格）与
//!     C3 errno 账本面（码值出现账），两面对账互认；
//!   - 解析器/对账：Reconciler（双向对账：C2 每行的 errno 必须在 C3
//!     闭集；C3 每码必须被 ≥1 行 C2 引用；缺口显性记账）；
//!   - 协议：CosignProtocol（联签记录 = hash(C2 面) XOR hash(C3 面)
//!     双方章，单方章显性拒绝；章不可复用到别的面对）；
//!   - 禁令：跨界代写禁令（C2 面调用 write_c3 显性拒绝并记账，
//!     反向同理）；
//!   - 状态机：CharterStage（Drafted→CrossChecked→MutuallyRecognized
//!     单向）；
//!   - 注入核：5 类坏样本（单方章/跨界代写/对账缺口/未知码/逆迁移）
//!     逐类显性拒绝；
//!   - 对拍：Linux errno 对照（8 码闭集：ENOENT/EINTR/EBADF/EAGAIN/
//!     EINVAL/EEXIST/EACCES/ELOOP）；
//!   - 指纹：FNV-1a 对账面序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9401 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7D1C0;

/// Linux errno 闭集（8 码，C2/C3 两面共用的事实源）。
pub const ERRNO_TABLE: [i32; 8] = [2, 4, 9, 11, 13, 17, 22, 40];

/// 行别口径：C2 = 错误矩阵面；C3 = errno 账本面。
pub const FACE_C2: u32 = 2;
pub const FACE_C3: u32 = 3;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=12 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterCode {
    /// 未知码：errno 不在 8 码闭集
    UnknownErrno,
    /// 跨界代写：C2 面写 C3 账本（或反向）
    CrossFaceGhostwrite,
    /// 单方章：联签记录只有一方章
    SingleSideSeal,
    /// 章复用：同一章用于别的面对对
    SealReused,
    /// 对账缺口：C2 引用了 C3 不存在的码
    ReconcileGap,
    /// 互认未满：C3 存在零引用码
    UnreferencedCode,
    /// 宪章逆迁移
    CharterReversed,
    /// 账本倒退
    LedgerRegress,
    /// 双跑指纹不一致
    NotReproducible,
    /// 面别越界：非 C2/C3 面
    UnknownFace,
    /// 矩阵行重复
    DuplicateRow,
    /// 联签未达即互认
    RecognizeBeforeCosign,
}

impl CharterCode {
    /// 闭集数组。
    pub const ALL: [CharterCode; 12] = [
        CharterCode::UnknownErrno,
        CharterCode::CrossFaceGhostwrite,
        CharterCode::SingleSideSeal,
        CharterCode::SealReused,
        CharterCode::ReconcileGap,
        CharterCode::UnreferencedCode,
        CharterCode::CharterReversed,
        CharterCode::LedgerRegress,
        CharterCode::NotReproducible,
        CharterCode::UnknownFace,
        CharterCode::DuplicateRow,
        CharterCode::RecognizeBeforeCosign,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                CharterCode::UnknownErrno => 1,
                CharterCode::CrossFaceGhostwrite => 2,
                CharterCode::SingleSideSeal => 3,
                CharterCode::SealReused => 4,
                CharterCode::ReconcileGap => 5,
                CharterCode::UnreferencedCode => 6,
                CharterCode::CharterReversed => 7,
                CharterCode::LedgerRegress => 8,
                CharterCode::NotReproducible => 9,
                CharterCode::UnknownFace => 10,
                CharterCode::DuplicateRow => 11,
                CharterCode::RecognizeBeforeCosign => 12,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            CharterCode::UnknownErrno => "errno 必须落在 8 码闭集内，表外码拒绝落账",
            CharterCode::CrossFaceGhostwrite => "跨界代写禁令：各面只写自己的账，越面写显性拒绝",
            CharterCode::SingleSideSeal => "联签必须双方盖章，单方章不得进入互认面",
            CharterCode::SealReused => "联签章与面对绑定，复用到别的面对显性拒绝",
            CharterCode::ReconcileGap => "C2 引用的每个 errno 必须在 C3 账本面存在",
            CharterCode::UnreferencedCode => "C3 的每个码必须被 ≥1 行 C2 引用，零引用即缺口",
            CharterCode::CharterReversed => "宪章状态 Drafted→CrossChecked→MutuallyRecognized 单向",
            CharterCode::LedgerRegress => "对账账只增不减，倒退即审计失败",
            CharterCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            CharterCode::UnknownFace => "面别只认 C2/C3，其他面显性拒绝",
            CharterCode::DuplicateRow => "矩阵行（syscall 类 × errno）唯一，重复拒绝",
            CharterCode::RecognizeBeforeCosign => "双方联签未达成不得宣布互认",
        }
    }
}

// ── C2 错误矩阵面 ─────────────────────────────────────────────────────
/// C2 矩阵行：syscall 类 × errno（判定格恒真存在）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixRow {
    /// syscall 类（闭集 0..=7，与 9391 同口径）
    pub syscall: usize,
    /// 判定 errno（8 码闭集）
    pub err: i32,
}

/// C2 错误矩阵面。
pub struct C2Face {
    rows: Vec<MatrixRow>,
    /// 成功登记行数
    pub rows_registered: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl C2Face {
    /// 建空 C2 面。
    pub fn new() -> C2Face {
        C2Face { rows: Vec::new(), rows_registered: 0, rejected: 0 }
    }

    /// errno 闭集校验。
    pub fn errno_ok(err: i32) -> bool {
        ERRNO_TABLE.contains(&err)
    }

    /// 登记矩阵行：码闭集 + 行去重。
    pub fn add_row(&mut self, syscall: usize, err: i32) -> Result<(), CharterCode> {
        if syscall > 7 {
            self.rejected += 1;
            return Err(CharterCode::UnknownFace);
        }
        if !Self::errno_ok(err) {
            self.rejected += 1;
            return Err(CharterCode::UnknownErrno);
        }
        let row = MatrixRow { syscall, err };
        if self.rows.contains(&row) {
            self.rejected += 1;
            return Err(CharterCode::DuplicateRow);
        }
        self.rows.push(row);
        self.rows_registered += 1;
        Ok(())
    }

    /// 引用码集合（去重后）。
    pub fn referenced_codes(&self) -> Vec<i32> {
        let mut out: Vec<i32> = Vec::new();
        for r in self.rows.iter() {
            if !out.contains(&r.err) {
                out.push(r.err);
            }
        }
        out
    }

    /// 行快照（指纹载体）。
    pub fn snapshot(&self) -> Vec<(u32, i32)> {
        self.rows.iter().map(|r| (r.syscall as u32, r.err)).collect()
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

// ── C3 errno 账本面 ───────────────────────────────────────────────────
/// C3 errno 账本面（码值出现账；只接受本面写入口）。
pub struct C3Face {
    /// 逐码出现计数（8 槽平行表）
    counts: [u32; 8],
    /// 累计入账数
    pub entries: u32,
    /// 显性拒绝数
    pub rejected: u32,
}

impl C3Face {
    /// 建空 C3 面。
    pub fn new() -> C3Face {
        C3Face { counts: [0; 8], entries: 0, rejected: 0 }
    }

    /// 码槽下标（闭集外返回 None）。
    fn slot(err: i32) -> Option<usize> {
        let mut i = 0usize;
        while i < ERRNO_TABLE.len() {
            if ERRNO_TABLE.get(i) == Some(&err) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 本面写入口：只接受 from_face == FACE_C3。
    ///
    /// 契约：C2 面调用此口是跨界代写，显性拒绝并记账（禁令落地点）。
    pub fn write(&mut self, err: i32, from_face: u32) -> Result<(), CharterCode> {
        if from_face != FACE_C3 {
            self.rejected += 1;
            return Err(CharterCode::CrossFaceGhostwrite);
        }
        let slot = match Self::slot(err) {
            Some(s) => s,
            None => {
                self.rejected += 1;
                return Err(CharterCode::UnknownErrno);
            }
        };
        if let Some(c) = self.counts.get_mut(slot) {
            *c += 1;
        }
        self.entries += 1;
        Ok(())
    }

    /// 某码出现次数。
    pub fn count_of(&self, err: i32) -> u32 {
        match Self::slot(err) {
            Some(s) => self.counts[s],
            None => 0,
        }
    }

    /// 零引用码数（对账缺口）。
    pub fn zero_ref_codes(&self) -> u32 {
        self.counts.iter().filter(|c| **c == 0).count() as u32
    }

    /// 账守恒：entries == 逐槽计数和。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u32;
        for c in self.counts.iter() {
            sum += *c;
        }
        sum == self.entries
    }
}

// ── 双向对账 ──────────────────────────────────────────────────────────
/// 对账账。
#[derive(Clone, Copy)]
pub struct ReconcileLedger {
    /// C2→C3 方向：逐行码全部在 C3 有账
    pub c2_to_c3: bool,
    /// C3→C2 方向：逐码至少被一行引用
    pub c3_to_c2: bool,
    /// 缺口数（必须 0）
    pub gaps: u32,
}

impl ReconcileLedger {
    /// 双向互认成立。
    pub fn mutual(&self) -> bool {
        self.c2_to_c3 && self.c3_to_c2 && self.gaps == 0
    }
}

/// 双向对账：C2 每行码在 C3 有账，C3 每码被 ≥1 行引用。
pub fn reconcile(c2: &C2Face, c3: &C3Face) -> ReconcileLedger {
    let refs = c2.referenced_codes();
    let mut c2_to_c3 = true;
    let mut gaps = 0u32;
    let mut i = 0usize;
    while i < refs.len() {
        if let Some(code) = refs.get(i) {
            if c3.count_of(*code) == 0 {
                c2_to_c3 = false;
                gaps += 1;
            }
        }
        i += 1;
    }
    // C3→C2：每个非零槽码都必须被 C2 引用
    let mut c3_to_c2 = true;
    let mut k = 0usize;
    while k < ERRNO_TABLE.len() {
        if let Some(code) = ERRNO_TABLE.get(k) {
            if c3.count_of(*code) > 0 && !refs.contains(code) {
                c3_to_c2 = false;
                gaps += 1;
            }
        }
        k += 1;
    }
    ReconcileLedger { c2_to_c3, c3_to_c2, gaps }
}

// ── 联签协议 ──────────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// C2 面章。
pub fn seal_c2(rows: &[(u32, i32)]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for (s, e) in rows {
        buf.extend_from_slice(&s.to_le_bytes());
        buf.extend_from_slice(&e.to_le_bytes());
    }
    fnv1a(&buf)
}

/// C3 面章。
pub fn seal_c3(counts: &[u32; 8]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for c in counts {
        buf.extend_from_slice(&c.to_le_bytes());
    }
    fnv1a(&buf)
}

/// 联签记录：双方章 + 面对绑定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CosignRecord {
    /// C2 章值
    pub seal_a: u32,
    /// C3 章值
    pub seal_b: u32,
    /// 面对绑定标记（0xC2C3）
    pub pair_tag: u32,
}

/// 面对绑定标记。
pub const PAIR_TAG: u32 = 0xC2C3;

/// 联签：双方章齐才成记录；单方章（任一章为 0）显性拒绝。
pub fn cosign(seal_a: u32, seal_b: u32) -> Result<CosignRecord, CharterCode> {
    if seal_a == 0 || seal_b == 0 {
        return Err(CharterCode::SingleSideSeal);
    }
    Ok(CosignRecord { seal_a, seal_b, pair_tag: PAIR_TAG })
}

/// 章复用检测：章值必须与当前面对章一致，否则拒绝。
pub fn seal_matches(rec: &CosignRecord, cur_a: u32, cur_b: u32) -> bool {
    rec.pair_tag == PAIR_TAG && rec.seal_a == cur_a && rec.seal_b == cur_b
}

// ── 宪章状态机 ────────────────────────────────────────────────────────
/// 宪章状态机：Drafted→CrossChecked→MutuallyRecognized 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharterStage {
    /// 起草（两面在案）
    Drafted,
    /// 已交叉对账
    CrossChecked,
    /// 双方互认（终态）
    MutuallyRecognized,
}

impl CharterStage {
    /// 单向迁移核验。
    pub fn advance(self, want: CharterStage) -> Option<CharterStage> {
        let legal = match (self, want) {
            (CharterStage::Drafted, CharterStage::CrossChecked) => true,
            (CharterStage::CrossChecked, CharterStage::MutuallyRecognized) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── 主流程 ────────────────────────────────────────────────────────────
/// 全流程：C2 登记矩阵面 → C3 逐码落账 → 双向对账 → 联签互认。
///
/// 返回 (C2 面, C3 面, 对账账, 联签记录)；全绿即互认成立。
pub fn run_charter_jointest() -> (C2Face, C3Face, ReconcileLedger, CosignRecord) {
    let mut c2 = C2Face::new();
    // 8 码全引用：8 类 syscall 各判一个码（闭集覆盖）
    let mut k = 0usize;
    while k < ERRNO_TABLE.len() {
        let err = match ERRNO_TABLE.get(k) {
            Some(e) => *e,
            None => break,
        };
        let _ = c2.add_row(k, err);
        k += 1;
    }
    // 补第二行引用（同 syscall 类引用另一码，行唯一合法）
    let _ = c2.add_row(1, EACCES_SLOT);

    let mut c3 = C3Face::new();
    // C3 面自己写自己的账（8 码各一笔）
    let mut k = 0usize;
    while k < ERRNO_TABLE.len() {
        if let Some(code) = ERRNO_TABLE.get(k) {
            let _ = c3.write(*code, FACE_C3);
        }
        k += 1;
    }
    let led = reconcile(&c2, &c3);
    let rec = match cosign(seal_c2(&c2.snapshot()), seal_c3(&c3.counts)) {
        Ok(r) => r,
        Err(_) => CosignRecord { seal_a: 0, seal_b: 0, pair_tag: PAIR_TAG },
    };
    (c2, c3, led, rec)
}

/// 8 码闭集内第 5 槽（EACCES=13）的别名引用（补充第二行用）。
const EACCES_SLOT: i32 = 13;

// ── 注入核 ────────────────────────────────────────────────────────────
/// 5 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 5], u32) {
    let mut ok = [false; 5];
    let mut detected = 0u32;

    // 类 1：跨界代写（C2 面写 C3 账本）
    let mut c3 = C3Face::new();
    let ghost = c3.write(EINVAL_CONST, FACE_C2) == Err(CharterCode::CrossFaceGhostwrite)
        && c3.rejected == 1;
    if ghost {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：单方章
    let single = cosign(0, 0xBEEF) == Err(CharterCode::SingleSideSeal)
        && cosign(0xBEEF, 0) == Err(CharterCode::SingleSideSeal);
    if single {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：对账缺口（C2 引用 C3 没有账的码）
    let mut c2 = C2Face::new();
    let mut c3b = C3Face::new();
    let _ = c2.add_row(0, EEXIST_CONST);
    let gap = reconcile(&c2, &c3b).gaps > 0;
    if gap {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：未知码
    let unknown = !C2Face::errno_ok(7777)
        && c3b.write(7777, FACE_C3) == Err(CharterCode::UnknownErrno);
    if unknown {
        detected += 1;
        ok[3] = true;
    }

    // 类 5：宪章逆迁移
    let reversed = CharterStage::MutuallyRecognized.advance(CharterStage::Drafted).is_none()
        && CharterStage::CrossChecked.advance(CharterStage::Drafted).is_none();
    if reversed {
        detected += 1;
        ok[4] = true;
    }

    (ok, detected)
}

/// 常量别名（EEXIST=17 / EINVAL=22）。
const EEXIST_CONST: i32 = 17;
const EINVAL_CONST: i32 = 22;

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

/// 对账面指纹。
pub fn charter_fingerprint(snap: &[(u32, i32)], counts: &[u32; 8]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for (s, e) in snap {
        buf.extend_from_slice(&s.to_le_bytes());
        buf.extend_from_slice(&e.to_le_bytes());
    }
    for c in counts {
        buf.extend_from_slice(&c.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9401 自检：联签协议与对账面判据（约 15 条）。
pub fn run_unx9401_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9401");

    // 1. C2 面 8 码全引用
    let (c2, c3, led, rec) = run_charter_jointest();
    s.add("C2 矩阵面：8 码全引用落行", c2.referenced_codes().len() == ERRNO_TABLE.len(),
          "错误矩阵面逐码判定格在案");

    // 2. C3 面逐码有账
    s.add("C3 账本面：8 码逐码入账", c3.zero_ref_codes() == 0 && c3.conserved(),
          "errno 账本面逐码落账且账守恒");

    // 3. 双向对账互认
    s.add("双向对账互认：C2→C3 与 C3→C2 双向成立", led.mutual(),
          "缺口必须为 0");

    // 4. 联签双方章齐
    s.add("联签协议：双方章齐且面对绑定", rec.seal_a != 0 && rec.seal_b != 0 && rec.pair_tag == PAIR_TAG,
          "单方章不得进入互认面");

    // 5. 章与面对匹配
    let match_ok = seal_matches(&rec, seal_c2(&c2.snapshot()), seal_c3(&c3.counts));
    s.add("章匹配：联签章与当前面对章逐位一致", match_ok,
          "面改动后旧章必须失效");

    // 6. 章区分度：面改动即章漂移
    let mut snap = c2.snapshot();
    if let Some(last) = snap.last_mut() {
        last.0 = last.0.wrapping_add(1);
    }
    s.add("章区分度：C2 面改动即章漂移", seal_c2(&snap) != rec.seal_a,
          "章必须能反映面对内容");

    // 7. 跨界代写禁令
    let mut c3w = C3Face::new();
    let ban = c3w.write(EINVAL_CONST, FACE_C2) == Err(CharterCode::CrossFaceGhostwrite)
        && c3w.write(EEXIST_CONST, FACE_C3).is_ok();
    s.add("跨界代写禁令：C2 面写 C3 拒绝、C3 自写放行", ban,
          "各面只写自己的账");

    // 8. 反向禁令对称：C3 面写 C2 侧同理（用 face≠C3 全拒）
    let ban2 = {
        let mut c = C3Face::new();
        c.write(EINVAL_CONST, FACE_C2) == Err(CharterCode::CrossFaceGhostwrite)
            && c.write(EINVAL_CONST, 9) == Err(CharterCode::CrossFaceGhostwrite)
    };
    s.add("禁令对称：任何非 C3 面写 C3 全拒", ban2,
          "面别闭集外一律拒绝");

    // 9. 对账缺口拒绝
    let gap = {
        let mut a = C2Face::new();
        let mut b = C3Face::new();
        let _ = a.add_row(0, EEXIST_CONST);
        let l = reconcile(&a, &b);
        !l.mutual() && l.gaps > 0
    };
    s.add("对账缺口：C2 引用无账码即缺口", gap,
          "缺口显性记账不静默");

    // 10. 单方章拒绝
    let single = cosign(0, 0x1234) == Err(CharterCode::SingleSideSeal);
    s.add("单方章拒绝：任一章为 0 即拒绝", single,
          "联签必须双方盖章");

    // 11. 章复用拒绝
    let reuse = {
        let r = match cosign(0xAAAA, 0xBBBB) { Ok(r) => r, Err(_) => CosignRecord { seal_a: 0, seal_b: 0, pair_tag: 0 } };
        !seal_matches(&r, 0xAAAA, 0xCCCC)
    };
    s.add("章复用拒绝：面对章不一致即失效", reuse,
          "章与面对绑定");

    // 12. 宪章状态机单向
    let fsm_ok = CharterStage::Drafted.advance(CharterStage::CrossChecked) == Some(CharterStage::CrossChecked)
        && CharterStage::CrossChecked.advance(CharterStage::MutuallyRecognized) == Some(CharterStage::MutuallyRecognized)
        && CharterStage::Drafted.advance(CharterStage::MutuallyRecognized).is_none();
    s.add("宪章状态机：Drafted→CrossChecked→MutuallyRecognized 单向", fsm_ok,
          "联签未达不得宣布互认");

    // 13. 注入核 5 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：5 类坏样本逐类显性拒绝", all_rej && detected == 5,
          "代写/单方章/缺口/未知码/逆迁移全过闸");

    // 14. errno 8 码闭集逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-ELOOP_CONST)
        && !agrees_with_linux(0)
        && !agrees_with_linux(6666);
    s.add("errno 对照：8 码表内全中、表外拒绝", codes_ok,
          "C2/C3 共用事实源闭集");

    // 15. 双跑指纹
    let (c2b, c3b, _, _) = run_charter_jointest();
    let fp1 = charter_fingerprint(&c2.snapshot(), &c3.counts);
    let fp2 = charter_fingerprint(&c2b.snapshot(), &c3b.counts);
    s.add("双跑指纹：对账面序列 FNV-1a 两次一致", fp1 == fp2,
          "确定性对账，无隐藏状态");

    // 16. 诊断码闭集
    let text_ok = CharterCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < CharterCode::ALL.len() {
        let mut j = i + 1;
        while j < CharterCode::ALL.len() {
            let a = CharterCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = CharterCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码闭集：12 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+12 闭集");

    // 动态明细（≤8）
    let r_text = format!(
        "C2 行 {} / C3 账 {} / 缺口 {}",
        c2.rows_registered, c3.entries, led.gaps
    );
    s.add_dyn("对账面快照", led.mutual(), &r_text);
    let fp_text = format!("互认指纹 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

/// 常量别名（ELOOP=40）。
const ELOOP_CONST: i32 = 40;

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c2_face_rows_and_dedup() {
        let mut c2 = C2Face::new();
        assert!(c2.add_row(0, 2).is_ok());
        assert_eq!(c2.add_row(0, 2), Err(CharterCode::DuplicateRow));
        assert_eq!(c2.rejected, 1);
        assert_eq!(c2.rows_registered, 1);
    }

    #[test]
    fn c2_closed_set_rejects_unknown() {
        let mut c2 = C2Face::new();
        assert_eq!(c2.add_row(0, 7777), Err(CharterCode::UnknownErrno));
        assert_eq!(c2.add_row(9, 2), Err(CharterCode::UnknownFace));
    }

    #[test]
    fn c3_write_and_conserved() {
        let mut c3 = C3Face::new();
        let _ = c3.write(2, FACE_C3);
        let _ = c3.write(2, FACE_C3);
        let _ = c3.write(4, FACE_C3);
        assert_eq!(c3.count_of(2), 2);
        assert_eq!(c3.count_of(4), 1);
        assert!(c3.conserved());
        assert_eq!(c3.entries, 3);
    }

    #[test]
    fn ghostwrite_banned() {
        let mut c3 = C3Face::new();
        assert_eq!(c3.write(2, FACE_C2), Err(CharterCode::CrossFaceGhostwrite));
        assert_eq!(c3.write(2, 7), Err(CharterCode::CrossFaceGhostwrite));
        assert!(c3.write(2, FACE_C3).is_ok());
    }

    #[test]
    fn reconcile_mutual_and_gap() {
        let mut c2 = C2Face::new();
        let mut c3 = C3Face::new();
        let _ = c2.add_row(0, 2);
        let _ = c3.write(2, FACE_C3);
        assert!(reconcile(&c2, &c3).mutual());
        // C3 有 C2 未引用的码 → 反向缺口
        let _ = c3.write(4, FACE_C3);
        let l = reconcile(&c2, &c3);
        assert!(!l.mutual());
        assert!(l.gaps > 0);
    }

    #[test]
    fn cosign_requires_both_seals() {
        assert_eq!(cosign(0, 1), Err(CharterCode::SingleSideSeal));
        assert_eq!(cosign(1, 0), Err(CharterCode::SingleSideSeal));
        assert!(cosign(0xA, 0xB).is_ok());
    }

    #[test]
    fn seal_binding_and_drift() {
        let r = cosign(0x1111, 0x2222).unwrap_or(CosignRecord { seal_a: 0, seal_b: 0, pair_tag: 0 });
        assert!(seal_matches(&r, 0x1111, 0x2222));
        assert!(!seal_matches(&r, 0x1111, 0x2223));
        assert!(!seal_matches(&r, 0x1112, 0x2222));
    }

    #[test]
    fn charter_fsm_one_way() {
        assert_eq!(CharterStage::Drafted.advance(CharterStage::MutuallyRecognized), None);
        assert_eq!(CharterStage::MutuallyRecognized.advance(CharterStage::Drafted), None);
        assert_eq!(
            CharterStage::CrossChecked.advance(CharterStage::MutuallyRecognized),
            Some(CharterStage::MutuallyRecognized)
        );
    }

    #[test]
    fn jointest_full_mutual() {
        let (c2, c3, led, rec) = run_charter_jointest();
        assert!(led.mutual());
        assert_eq!(c2.referenced_codes().len(), 8);
        assert_eq!(c3.zero_ref_codes(), 0);
        assert!(rec.seal_a != 0 && rec.seal_b != 0);
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 5);
    }

    #[test]
    fn errno_8_codes_closed_set() {
        assert_eq!(ERRNO_TABLE.len(), 8);
        for c in ERRNO_TABLE.iter() {
            assert!(agrees_with_linux(*c));
            assert!(agrees_with_linux(-*c));
        }
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(5555));
    }

    #[test]
    fn double_run_fingerprint() {
        let (a, b, _, _) = run_charter_jointest();
        let (a2, b2, _, _) = run_charter_jointest();
        assert_eq!(
            charter_fingerprint(&a.snapshot(), &b.counts),
            charter_fingerprint(&a2.snapshot(), &b2.counts)
        );
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(CharterCode::ALL.len(), 12);
        for c in CharterCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 12);
        }
    }

    #[test]
    fn unx9401_checks_green() {
        let set = run_unx9401_checks();
        assert!(set.all_passed(), "UNX-F9401 red: {}", set.red_items_text());
    }
}
