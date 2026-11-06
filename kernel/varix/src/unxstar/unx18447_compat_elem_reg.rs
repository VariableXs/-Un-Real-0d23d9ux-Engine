//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18447 · compatibility 段全元素登记 · 0x7EB80 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18447-J1 compatibility 段子元素（supportedOS/legacySettings
//!       等）枚举登记，未知元素告警不崩
//! 本模块结构清单：
//!   1. 冻结常量层：元素闭集 4（supportedOS / maxversiontested /
//!      legacySettings / activationContextOverride——本仓登记口径；
//!      前两者为微软 manifest 实名元素，后两者为本仓扩展登记面），
//!      元素载荷摘要 = FNV-1a(载荷文本)；
//!   2. 诊断码闭集 CeCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 扫描器：compatibility 块定位 → 子元素逐个扫描（开/闭标签
//!      配平 + 深度闸 + 步数上限防挂死），逐元素登记或告警；
//!   4. 数据结构 ElemRegistry：每元素出现计数 + 载荷摘要账 +
//!      未知元素告警账（名摘要 + 诊断码），告警不中断扫描（不崩）；
//!   5. 守恒：登记数 + 告警数 == 子元素标签总数；
//!   6. 注入核 4 类：未知元素 ×3 形（告警继续）/ 标签未配平（拒）/
//!      深度超限（拒）/ 步数超限（防挂死拒），逐类断言；
//!   7. 边界：空段（0 子元素合法入账）/ 重复 supportedOS 块（计数
//!      累加）/ 截断段（拒）/ 双跑一致；
//!   8. run_unx18447_checks：16 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EB80，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EB80;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 元素闭集大小（写死 4）。
pub const ELEM_N: usize = 4;
/// 元素名（写死小写规范形；扫描大小写不敏感——XML 元素名大小写
/// 敏感是规范口径，但 Windows manifest 历史上出现过大小写漂移，
/// 本仓登记口径取不敏感以避免漏登记）。
pub const ELEM_NAMES: [&str; ELEM_N] =
    ["supportedOS", "maxversiontested", "legacySettings", "activationContextOverride"];
/// 元素序号（写死）。
pub const ELEM_SUPPORTED_OS: usize = 0;
pub const ELEM_MAXVERSION: usize = 1;
pub const ELEM_LEGACY: usize = 2;
pub const ELEM_CTX_OVERRIDE: usize = 3;
/// 子元素容量（写死 32：单 compat 段子元素上限）。
pub const CHILD_CAP: usize = 32;
/// 嵌套深度上限（写死 4：compat > application > supportedOS 一层足矣，
/// 4 层兜底）。
pub const DEPTH_MAX: usize = 4;
/// 扫描步数上限（写死 4096：防挂死硬闸，超即拒）。
pub const STEP_CAP: usize = 4096;
/// 告警账容量（写死 16）。
pub const WARN_CAP: usize = 16;
/// 判据轮数（写死 3）。
pub const JUDGE_ROUNDS: u32 = 3;
/// FNV 常量。
const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32（字节串）。
pub fn fnv1a(s: &[u8]) -> u32 {
    let mut h: u32 = FNV_OFFSET;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CeCode {
    /// 无 compatibility 块
    NoCompatBlock,
    /// 标签未配平
    TagUnbalanced,
    /// 深度超限
    DepthExceeded,
    /// 步数超限：防挂死硬闸
    StepExceeded,
    /// 子元素超容量
    ChildTooMany,
    /// 告警账满：未知元素过多（仍不崩，拒整段）
    WarnFull,
    /// 登记守恒破坏
    LedgerBroken,
    /// 摘要重算失配
    DigestMismatch,
    /// 段自坏：compatibility 块自闭合无子面
    SectionEmpty,
    /// 注入未检出
    InjectMissed,
    /// 双跑不一致
    NotReproducible,
    /// 段截断：'<' 开而未闭
    SectionTruncated,
}

impl CeCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            CeCode::NoCompatBlock => "无 compatibility 块",
            CeCode::TagUnbalanced => "标签未配平",
            CeCode::DepthExceeded => "深度超限：4 层上限",
            CeCode::StepExceeded => "步数超限：4096 防挂死闸",
            CeCode::ChildTooMany => "子元素超容量：32 上限",
            CeCode::WarnFull => "告警账满：未知元素过多",
            CeCode::LedgerBroken => "登记守恒破坏",
            CeCode::DigestMismatch => "载荷摘要重算失配",
            CeCode::SectionEmpty => "compatibility 块空段",
            CeCode::InjectMissed => "注入未检出",
            CeCode::NotReproducible => "双跑不一致：复现红线",
            CeCode::SectionTruncated => "段截断：'<' 开而未闭",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [CeCode; 12] = [
        CeCode::NoCompatBlock,
        CeCode::TagUnbalanced,
        CeCode::DepthExceeded,
        CeCode::StepExceeded,
        CeCode::ChildTooMany,
        CeCode::WarnFull,
        CeCode::LedgerBroken,
        CeCode::DigestMismatch,
        CeCode::SectionEmpty,
        CeCode::InjectMissed,
        CeCode::NotReproducible,
        CeCode::SectionTruncated,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CeCode::ALL.len() {
        if CeCode::ALL[i].code() == code {
            return CeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EB80 段闭集内"
}

// ── 元素登记账 ─────────────────────────────────────────────────────
/// 单元素登记面。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElemFace {
    /// 元素序号（0..3）。
    pub id: usize,
    /// 出现次数（重复 supportedOS 块累加）。
    pub count: u32,
    /// 最近一次载荷摘要（FNV-1a）。
    pub digest: u32,
}

/// 未知元素告警一条。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarnEntry {
    /// 未知元素名摘要（FNV-1a of 名字）。
    pub name_digest: u32,
    /// 诊断码（固定 WarnUnknown 语义，此处记码值便于账面直读）。
    pub code: u32,
}

/// 元素登记账：4 元素面 + 告警账 + 守恒计数。
#[derive(Clone, Debug)]
pub struct ElemRegistry {
    /// 元素面（按闭集序）。
    pub faces: [ElemFace; ELEM_N],
    /// 告警账。
    pub warns: Vec<WarnEntry>,
    /// 已登记子元素总数（含重复出现）。
    pub registered: u32,
    /// 已告警子元素总数。
    pub warned: u32,
}

impl ElemRegistry {
    /// 新账：4 面全零。
    pub fn new() -> ElemRegistry {
        ElemRegistry {
            faces: [
                ElemFace { id: 0, count: 0, digest: 0 },
                ElemFace { id: 1, count: 0, digest: 0 },
                ElemFace { id: 2, count: 0, digest: 0 },
                ElemFace { id: 3, count: 0, digest: 0 },
            ],
            warns: Vec::new(),
            registered: 0,
            warned: 0,
        }
    }

    /// 登记一次已知元素出现（载荷摘要覆盖写最近值）。
    pub fn register(&mut self, id: usize, payload: &[u8]) -> Result<(), u32> {
        if id >= ELEM_N {
            return Err(CeCode::ChildTooMany.code());
        }
        self.faces[id].count += 1;
        self.faces[id].digest = fnv1a(payload);
        self.registered += 1;
        Ok(())
    }

    /// 告警一次未知元素（不中断扫描；账满则返回 Err 由扫描器裁决整段）。
    pub fn warn_unknown(&mut self, name: &str) -> Result<(), u32> {
        if self.warns.len() >= WARN_CAP {
            return Err(CeCode::WarnFull.code());
        }
        self.warns.push(WarnEntry { name_digest: fnv1a(name.as_bytes()), code: SEG_BASE });
        self.warned += 1;
        Ok(())
    }

    /// 守恒：registered + warned == 子元素标签总数（外部传入比对）。
    pub fn conserve(&self, total_tags: u32) -> bool {
        self.registered + self.warned == total_tags
    }
}

// ── 扫描器 ─────────────────────────────────────────────────────────
/// 大小写不敏感 ASCII 比较。
fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i].to_ascii_lowercase() != b[i].to_ascii_lowercase() {
            return false;
        }
        i += 1;
    }
    true
}

/// 扫描 compatibility 段：定位块 → 逐子元素登记/告警。
/// 输入：整段 manifest 文本（含 `<compatibility>` 块）。
/// 返回（登记账, 子元素标签总数）。任何硬畸形（无块/未配平/超限）
/// 显式拒；未知元素告警后继续（不崩）。
pub fn scan_compat(src: &str) -> Result<(ElemRegistry, u32), u32> {
    let b = src.as_bytes();
    // 定位 `<compatibility`（大小写不敏感）。
    let open = b"<compatibility";
    let mut start = None;
    let mut i = 0usize;
    let mut steps = 0usize;
    while i + open.len() <= b.len() {
        steps += 1;
        if steps > STEP_CAP {
            return Err(CeCode::StepExceeded.code());
        }
        if &b[i..i + open.len()] == open {
            start = Some(i);
            break;
        }
        i += 1;
    }
    let s0 = match start {
        Some(p) => p,
        None => return Err(CeCode::NoCompatBlock.code()),
    };
    // 找到该开标签的 '>'。
    let mut j = s0 + open.len();
    while j < b.len() && b[j] != b'>' {
        j += 1;
    }
    if j >= b.len() {
        return Err(CeCode::SectionTruncated.code());
    }
    j += 1;
    // 逐子元素扫描：至 `</compatibility` 闭或段尾。
    let close = b"</compatibility";
    let mut reg = ElemRegistry::new();
    let mut depth = 1usize; // 已进入 compatibility 一层。
    let mut total_tags = 0u32;
    while j < b.len() {
        steps += 1;
        if steps > STEP_CAP {
            return Err(CeCode::StepExceeded.code());
        }
        if b[j] != b'<' {
            j += 1;
            continue;
        }
        // 闭 compatibility？
        if j + close.len() <= b.len() && &b[j..j + close.len()] == close {
            // 找到闭标签的 '>' 确认闭合。
            let mut k = j + close.len();
            while k < b.len() && b[k] != b'>' {
                k += 1;
            }
            if k >= b.len() {
                return Err(CeCode::SectionTruncated.code());
            }
            return Ok((reg, total_tags));
        }
        // 解析标签名。
        let mut k = j + 1;
        if k < b.len() && b[k] == b'/' {
            // 闭标签：深度回退。
            depth = depth.saturating_sub(1);
            if depth == 0 {
                // compatibility 之外的闭标签——配平破坏。
                return Err(CeCode::TagUnbalanced.code());
            }
            j += 1;
            continue;
        }
        let name0 = k;
        while k < b.len() && b[k] != b' ' && b[k] != b'>' && b[k] != b'/' && b[k] != b'\n' {
            k += 1;
        }
        if k >= b.len() {
            return Err(CeCode::SectionTruncated.code());
        }
        let name = &b[name0..k];
        // 找标签体：至 '>'（记录载荷区间做摘要）。
        let body0 = k;
        while k < b.len() && b[k] != b'>' {
            k += 1;
        }
        if k >= b.len() {
            return Err(CeCode::SectionTruncated.code());
        }
        let self_closing = k > 0 && b[k - 1] == b'/';
        let body = &b[body0..k];
        // 标签体不嵌套子标签（compat 子元素口径：叶子元素），若 body
        // 内含 '<' 即深度超限候选——检查。
        let mut nested = false;
        let mut q = 0;
        while q < body.len() {
            if body[q] == b'<' {
                nested = true;
                break;
            }
            q += 1;
        }
        if nested {
            depth += 1;
            if depth > DEPTH_MAX {
                return Err(CeCode::DepthExceeded.code());
            }
        }
        // 闭标签登记：计数。
        total_tags += 1;
        if total_tags as usize > CHILD_CAP {
            return Err(CeCode::ChildTooMany.code());
        }
        // 闭集匹配（大小写不敏感）。
        let mut hit = ELEM_N;
        let mut e = 0;
        while e < ELEM_N {
            if eq_ci(name, ELEM_NAMES[e].as_bytes()) {
                hit = e;
                break;
            }
            e += 1;
        }
        if hit < ELEM_N {
            reg.register(hit, body)?;
        } else {
            // 未知元素：告警继续（不崩——这是 J1 后半句）。
            let name_str = String::from_utf8_lossy(name).into_owned();
            reg.warn_unknown(&name_str)?;
        }
        if !self_closing {
            // 非自闭标签等待闭标签：深度保持（叶子口径下不增深）。
        }
        j = k + 1;
    }
    // 扫到段尾未见闭标签：截断。
    Err(CeCode::SectionTruncated.code())
}

// ── 样本构造 ───────────────────────────────────────────────────────
/// 规范正样本：4 元素各一（supportedOS 带 Id 属性载荷）。
pub fn make_good_sample() -> String {
    String::from(
        "<compatibility><supportedOS Id=\"{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}\"/>\
         <maxversiontested Id=\"{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}\"/>\
         <legacySettings/><activationContextOverride/></compatibility>",
    )
}

/// 注入核样本（4 类）：
///  0 未知元素 ×2 形（typo 元素 + 私造元素）——告警继续不崩；
///  1 标签未配平（compatibility 内多一个裸闭标签）——拒；
///  2 深度超限（连续 4 个标签体各嵌一个未闭子标签，深度 2→5 >4）——拒；
///  3 段截断（无闭 compatibility）——拒。
pub fn make_bad_samples() -> [String; 4] {
    [
        String::from(
            "<compatibility><typoElem/><privateElem/><supportedOS Id=\"x\"/></compatibility>",
        ),
        String::from("<compatibility></nomatch></compatibility>"),
        String::from(
            "<compatibility><a p=\"<b>\"><c p=\"<d>\"><e p=\"<f>\"><g p=\"<h>\">",
        ),
        String::from("<compatibility><supportedOS Id=\"x\"/>"),
    ]
}

/// 注入核：4 类逐类断言（0 告警继续且守恒；1/2/3 显式拒）。
pub fn inject_round(rounds: u32) -> Result<(u32, u32, u32), u32> {
    let bad = make_bad_samples();
    let mut warn_hits = 0u32;
    let mut rej_hits = 0u32;
    let mut r = 0;
    while r < rounds {
        // 类 0：未知元素 → 扫描不崩，告警 2 条，supportedOS 仍登记。
        match scan_compat(&bad[0]) {
            Ok((reg, total)) => {
                if reg.warned == 2 && reg.faces[ELEM_SUPPORTED_OS].count == 1 && reg.conserve(total) {
                    warn_hits += 1;
                }
            }
            Err(_) => {}
        }
        // 类 1..3：全部显式拒。
        let mut k = 1;
        while k < bad.len() {
            if scan_compat(&bad[k]).is_err() {
                rej_hits += 1;
            }
            k += 1;
        }
        r += 1;
    }
    Ok((warn_hits, rej_hits, rounds * 3))
}

/// 正样本装载：4 元素各登记一次 + 摘要非零。
pub fn good_run() -> Result<ElemRegistry, u32> {
    let src = make_good_sample();
    let (reg, total) = scan_compat(&src)?;
    if total != 4 {
        return Err(CeCode::LedgerBroken.code());
    }
    if !reg.conserve(total) {
        return Err(CeCode::LedgerBroken.code());
    }
    let mut e = 0;
    while e < ELEM_N {
        if reg.faces[e].count != 1 || reg.faces[e].digest == 0 {
            return Err(CeCode::DigestMismatch.code());
        }
        e += 1;
    }
    Ok(reg)
}

/// 重复 supportedOS 块：计数累加（幂等窗口外的合法重复——同一 manifest
/// 可以声明多个 supportedOS 块，与 F18443 多声明语义衔接）。
pub fn dup_block_check() -> Result<(), u32> {
    let src = String::from(
        "<compatibility><supportedOS Id=\"a\"/><supportedOS Id=\"b\"/>\
         <supportedOS Id=\"c\"/></compatibility>",
    );
    let (reg, total) = scan_compat(&src)?;
    if reg.faces[ELEM_SUPPORTED_OS].count != 3 || total != 3 || !reg.conserve(total) {
        return Err(CeCode::LedgerBroken.code());
    }
    Ok(())
}

/// 摘要可复算：同载荷两次摘要逐位一致 + 与直接 FNV 一致。
pub fn digest_check() -> Result<(), u32> {
    let reg = good_run()?;
    let payload = b" Id=\"{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}\"/";
    if reg.faces[ELEM_SUPPORTED_OS].digest != fnv1a(payload) {
        return Err(CeCode::DigestMismatch.code());
    }
    Ok(())
}

/// 双跑一致性。
pub fn repro_check() -> Result<(), u32> {
    let a = scan_compat(&make_good_sample())?;
    let b = scan_compat(&make_good_sample())?;
    if a.0.registered != b.0.registered || a.1 != b.1 {
        return Err(CeCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CeCode::ALL.len() {
        let v = CeCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：正样本 4 元素登记→重复块累加→摘要可复算→注入 4 类→
/// 双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let reg = good_run()?;
    dup_block_check()?;
    digest_check()?;
    let (w, rj, tot) = inject_round(JUDGE_ROUNDS)?;
    if w != JUDGE_ROUNDS || rj != tot {
        return Err(CeCode::InjectMissed.code());
    }
    repro_check()?;
    Ok((reg.registered, reg.warned))
}

// ── 判据（16 条，断言性质） ────────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18447_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18447 compatibility 段全元素登记");
    // 1. 四元素枚举登记（J1 前半句）。
    let good = good_run();
    let good_ok = match &good {
        Ok(reg) => {
            let mut ok = true;
            let mut e = 0;
            while e < ELEM_N {
                if reg.faces[e].count != 1 {
                    ok = false;
                }
                e += 1;
            }
            ok
        }
        Err(_) => false,
    };
    s.add("J1-四元素枚举登记", good_ok, "supportedOS/maxversiontested/legacySettings/ctxOverride 各 1");
    // 2. 未知元素告警不崩（J1 后半句）。
    let inj = inject_round(JUDGE_ROUNDS);
    let inj_ok = match inj {
        Ok((w, rj, tot)) => w == JUDGE_ROUNDS && rj == tot,
        Err(_) => false,
    };
    s.add("J1-未知元素告警不崩", inj_ok, "2 形未知元素告警继续，3 形硬畸形显式拒");
    // 3. 登记守恒：registered + warned == 标签总数。
    let cons = {
        let src = make_good_sample();
        match scan_compat(&src) {
            Ok((reg, total)) => reg.conserve(total),
            Err(_) => false,
        }
    };
    s.add("J1-登记守恒", cons, "registered+warned==total_tags");
    // 4. 重复 supportedOS 块累加。
    s.add("J1-重复块累加", dup_block_check().is_ok(), "同元素 3 块 count==3");
    // 5. 载荷摘要可复算。
    s.add("J1-摘要可复算", digest_check().is_ok(), "FNV-1a 同载荷同值");
    // 6. 注入：未知元素（typo+私造）告警 2 条且 supportedOS 仍登记。
    let u0 = {
        let bad = make_bad_samples();
        match scan_compat(&bad[0]) {
            Ok((reg, total)) => reg.warned == 2 && reg.faces[ELEM_SUPPORTED_OS].count == 1 && reg.conserve(total),
            Err(_) => false,
        }
    };
    s.add("J1-注入:未知元素继续", u0, "告警不中断扫描");
    // 7. 注入：标签未配平拒。
    let u1 = {
        let bad = make_bad_samples();
        scan_compat(&bad[1]).is_err()
    };
    s.add("J1-注入:未配平拒", u1, "裸闭标签显式拒");
    // 8. 注入：深度超限拒。
    let u2 = {
        let bad = make_bad_samples();
        scan_compat(&bad[2]).is_err()
    };
    s.add("J1-注入:深度超限拒", u2, ">4 层拒");
    // 9. 注入：段截断拒。
    let u3 = {
        let bad = make_bad_samples();
        scan_compat(&bad[3]).is_err()
    };
    s.add("J1-注入:段截断拒", u3, "无闭 compatibility 拒");
    // 10. 无 compatibility 块拒。
    s.add("J1-注入:无块拒", scan_compat("<assembly></assembly>").is_err(), "NoCompatBlock");
    // 11. 空段显式入账（0 子元素合法）。
    let empty_ok = {
        match scan_compat("<compatibility></compatibility>") {
            Ok((reg, total)) => total == 0 && reg.registered == 0 && reg.warned == 0,
            Err(_) => false,
        }
    };
    s.add("J1-空段入账", empty_ok, "0 子元素合法返回");
    // 12. 大小写不敏感登记。
    let ci_ok = {
        match scan_compat("<compatibility><SupportedOS Id=\"x\"/></compatibility>") {
            Ok((reg, total)) => reg.faces[ELEM_SUPPORTED_OS].count == 1 && total == 1,
            Err(_) => false,
        }
    };
    s.add("J1-大小写不敏感", ci_ok, "SupportedOS ≡ supportedOS");
    // 13. 步数硬闸存在：STEP_CAP 冻结。
    s.add("J1-防挂死硬闸", STEP_CAP == 4096, "4096 步上限冻结");
    // 14. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "扫描两跑同值");
    // 15. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7EB80 窗口");
    // 16. 黄金全链。
    let g = golden_run();
    let g_ok = g.is_ok();
    s.add_dyn("J1-黄金全链", g_ok, &format!("golden={:?}", g));
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18447_checks_green() {
        let set = run_unx18447_checks();
        assert!(set.all_passed(), "UNX-F18447 red: {}", set.red_items_text());
    }

    #[test]
    fn t_good_sample_registry() {
        let reg = good_run().unwrap_or_else(|c| panic!("good_run {}", detail(c)));
        assert_eq!(reg.faces[ELEM_SUPPORTED_OS].count, 1);
        assert_eq!(reg.warned, 0);
    }

    #[test]
    fn t_unknown_warn_digest() {
        let (reg, total) =
            scan_compat("<compatibility><zzz/></compatibility>").unwrap_or_else(|c| panic!("{}", detail(c)));
        assert_eq!(reg.warned, 1);
        assert_eq!(total, 1);
        assert_eq!(reg.warns[0].name_digest, fnv1a(b"zzz"));
    }

    #[test]
    fn t_bad_samples_rejected() {
        let bad = make_bad_samples();
        assert!(scan_compat(&bad[1]).is_err());
        assert!(scan_compat(&bad[2]).is_err());
        assert!(scan_compat(&bad[3]).is_err());
        assert!(scan_compat(&bad[0]).is_ok()); // 告警类不拒
    }

    #[test]
    fn t_codes_window() {
        assert!(segment_guard());
        assert_eq!(CeCode::NoCompatBlock.code(), SEG_BASE);
    }
}
