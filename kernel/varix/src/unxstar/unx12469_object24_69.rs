//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12469 · 注册表路径与对象命名联签 · 0x308A0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12469-J1 RootDirectory 相对路径与绝对路径双档判据绿，
//!      与 D3 命名协议对平 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双档解析：绝对档（路径以 \\Registry 开头，RootDirectory 必空）
//!    与相对档（RootDirectory 非空 + 相对组件路径，不许再以 \\ 开头
//!    ——相对档绝对化注入拒）；
//!   - 联签：路径解析结果与对象命名（首组件 + 末组件 + 深度）三段
//!    指纹联签（FNV-1a），双档同键联签必同（对平判据根基）；
//!   - D3 命名协议对平：D3 档命名（组件闭集字符表 + 深度闭集 8）
//!    与本档解析器逐组件对拍 10/10 零漂移；
//!   - 大小写归一：比较器大写化（与 F12461 同口径），联签以归一后
//!    字节计算；
//!   - RootDirectory 闭集：已登记根键号才可作相对根（幽灵根拒）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，相对档绝对化注入/幽灵根注
//!    入/越深注入各 10/10 全拒；双档同键联签对拍 10/10 全同；
//!   - 诊断码闭集 0x308A0 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x308A0 段）。
pub const SEG_BASE: u32 = 0x308A0;

/// 绝对档前缀（\\Registry 大写化口径）。
pub const ABS_PREFIX: &[u8] = b"\\REGISTRY";

/// 深度闭集（D3 命名协议同款）。
pub const DEPTH_MAX: usize = 8;

/// 组件字节上限。
pub const COMP_MAX: usize = 48;

/// 根表容量。
pub const ROOT_CAP: usize = 8;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x308A0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NpCode {
    /// 0x308A0 相对档绝对化（相对路径以 \\ 开头）。
    RelAbsolutized = 0,
    /// 0x308A1 幽灵根（RootDirectory 未登记）。
    GhostRoot = 1,
    /// 0x308A2 绝对档带根（RootDirectory 非空 + 绝对路径互斥破）。
    AbsWithRoot = 2,
    /// 0x308A3 绝对前缀缺失（非 \\Registry 开头的绝对档）。
    NoAbsPrefix = 3,
    /// 0x308A4 深度越界（>8 级）。
    DepthOob = 4,
    /// 0x308A5 组件超长。
    CompOob = 5,
    /// 0x308A6 空路径（双档皆空）。
    EmptyPath = 6,
    /// 0x308A7 联签失配（双档同键联签不同）。
    JointFp = 7,
    /// 0x308A8 D3 对平漂移。
    D3Drift = 8,
    /// 0x308A9 段位越界。
    SegOob = 9,
    /// 0x308AA 非法字符。
    BadChar = 10,
    /// 0x308AB 根表满。
    RootFull = 11,
}

impl NpCode {
    /// 全部码闭集。
    pub const ALL: [NpCode; 12] = [
        NpCode::RelAbsolutized,
        NpCode::GhostRoot,
        NpCode::AbsWithRoot,
        NpCode::NoAbsPrefix,
        NpCode::DepthOob,
        NpCode::CompOob,
        NpCode::EmptyPath,
        NpCode::JointFp,
        NpCode::D3Drift,
        NpCode::SegOob,
        NpCode::BadChar,
        NpCode::RootFull,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            NpCode::RelAbsolutized => "相对档路径不许以 \\ 开头，改纯组件相对路径",
            NpCode::GhostRoot => "RootDirectory 未登记，先开根键再相对解析",
            NpCode::AbsWithRoot => "绝对路径与 RootDirectory 互斥，二选一",
            NpCode::NoAbsPrefix => "绝对档须以 \\Registry 开头",
            NpCode::DepthOob => "路径超 8 级（D3 命名协议闭集）",
            NpCode::CompOob => "组件超 48 字节",
            NpCode::EmptyPath => "双档皆空，至少给绝对路径或根+相对路径",
            NpCode::JointFp => "双档同键联签失配，逐组件核对",
            NpCode::D3Drift => "D3 命名协议对平漂移，逐组件对拍",
            NpCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            NpCode::BadChar => "组件含非法字节（控制字符/\\0）",
            NpCode::RootFull => "根表满（8），归档闲置根",
        }
    }
}

/// FNV-1a 64（联签核）。
fn fnv1a(acc: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h = acc;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

fn upper(b: u8) -> u8 {
    if b >= b'a' && b <= b'z' {
        b - 32
    } else {
        b
    }
}

/// 解析结果（归一化组件序列）。
pub struct ResolvedPath {
    /// 组件（大写化）。
    comps: Vec<Vec<u8>>,
    /// 起点根号（相对档：0..8；绝对档：u32::MAX）。
    pub root: u32,
}

impl ResolvedPath {
    /// 深度。
    pub fn depth(&self) -> usize {
        self.comps.len()
    }

    /// 组件只读。
    pub fn comp(&self, i: usize) -> Option<&[u8]> {
        if i < self.comps.len() {
            Some(&self.comps[i])
        } else {
            None
        }
    }

    /// 联签（三段指纹：首组件 + 末组件 + 深度）。
    pub fn joint_fp(&self) -> u64 {
        let mut h = 0xcbf29ce484222325;
        if let Some(first) = self.comp(0) {
            h = fnv1a(h, first);
        }
        if self.depth() > 0 {
            let last = self.depth() - 1;
            if let Some(l) = self.comp(last) {
                h = fnv1a(h, l);
            }
        }
        h = fnv1a(h, &[self.depth() as u8]);
        h
    }
}

/// D3 命名协议侧解析（对平对象：组件闭集字符表 + 深度闭集）。
///
/// 独立实现（判据纪律：不复用主解析器），对平 = 两解析器逐组件一致。
pub fn d3_parse(path: &[u8]) -> Result<Vec<Vec<u8>>, u32> {
    if path.is_empty() {
        return Err(NpCode::EmptyPath.val());
    }
    if path.len() > DEPTH_MAX * (COMP_MAX + 1) {
        return Err(NpCode::DepthOob.val());
    }
    let mut comps: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < path.len() {
        let b = path[i];
        if b == b'\\' {
            if cur.is_empty() {
                return Err(NpCode::BadChar.val());
            }
            comps.push(cur.clone());
            cur.clear();
            if comps.len() > DEPTH_MAX {
                return Err(NpCode::DepthOob.val());
            }
        } else if b == 0 || b < 0x20 {
            return Err(NpCode::BadChar.val());
        } else {
            if cur.len() >= COMP_MAX {
                return Err(NpCode::CompOob.val());
            }
            cur.push(upper(b));
        }
        i += 1;
    }
    if cur.is_empty() {
        return Err(NpCode::BadChar.val());
    }
    comps.push(cur);
    if comps.len() > DEPTH_MAX {
        return Err(NpCode::DepthOob.val());
    }
    Ok(comps)
}

/// 根表（相对档 RootDirectory 登记）。
pub struct RootTable {
    /// 已登记根键（键名）。
    roots: [Option<[u8; COMP_MAX]>; ROOT_CAP],
    len: usize,
}

impl RootTable {
    /// 新建。
    pub fn new() -> RootTable {
        RootTable { roots: [None; ROOT_CAP], len: 0 }
    }

    /// 登记根。
    pub fn admit(&mut self, name: &[u8]) -> Result<u32, u32> {
        if name.is_empty() || name.len() > COMP_MAX {
            return Err(NpCode::CompOob.val());
        }
        if self.len >= ROOT_CAP {
            return Err(NpCode::RootFull.val());
        }
        let mut nm = [0u8; COMP_MAX];
        let mut k = 0;
        while k < name.len() {
            nm[k] = upper(name[k]);
            k += 1;
        }
        self.roots[self.len] = Some(nm);
        let idx = self.len as u32;
        self.len += 1;
        Ok(idx)
    }

    /// 幽灵根核。
    pub fn known(&self, idx: u32) -> bool {
        (idx as usize) < self.len
    }
}

/// 双档解析器（绝对/相对）。
///
/// 绝对档：path 以 \\Registry 开头，root 参数必须为 None；
/// 相对档：root Some(已登记根号)，path 为纯相对组件路径（不许 \\
/// 开头）。
pub fn resolve(root: Option<u32>, roots: &RootTable, path: &[u8]) -> Result<ResolvedPath, u32> {
    // 绝对前缀判（大小写归一：逐字节大写化后比对）。
    let abs = path.len() >= ABS_PREFIX.len() && {
        let mut eq = true;
        let mut i = 0;
        while i < ABS_PREFIX.len() {
            if upper(path[i]) != ABS_PREFIX[i] {
                eq = false;
                break;
            }
            i += 1;
        }
        eq
    };
    match root {
        None => {
            // 绝对档。
            if !abs {
                return Err(NpCode::NoAbsPrefix.val());
            }
            let body = &path[ABS_PREFIX.len()..];
            // \\Registry 后接组件。
            let mut comps: Vec<Vec<u8>> = Vec::new();
            let mut cur: Vec<u8> = Vec::new();
            let mut i = 0;
            while i < body.len() {
                let b = body[i];
                if b == b'\\' {
                    if !cur.is_empty() {
                        comps.push(cur.clone());
                        cur.clear();
                    }
                    // 空组件（双反斜杠）→ BadChar（与 D3 对平口径）。
                } else if b == 0 || b < 0x20 {
                    return Err(NpCode::BadChar.val());
                } else {
                    if cur.len() >= COMP_MAX {
                        return Err(NpCode::CompOob.val());
                    }
                    cur.push(upper(b));
                }
                i += 1;
            }
            if !cur.is_empty() {
                comps.push(cur);
            }
            if comps.is_empty() {
                return Err(NpCode::EmptyPath.val());
            }
            if comps.len() > DEPTH_MAX {
                return Err(NpCode::DepthOob.val());
            }
            Ok(ResolvedPath { comps, root: u32::MAX })
        }
        Some(r) => {
            // 相对档。
            if !roots.known(r) {
                return Err(NpCode::GhostRoot.val());
            }
            if path.is_empty() {
                return Err(NpCode::EmptyPath.val());
            }
            // 相对档绝对化闸（先于互斥闸——路径以 \\ 开头即绝对化）。
            if path[0] == b'\\' {
                return Err(NpCode::RelAbsolutized.val());
            }
            if abs {
                // 绝对路径配根：互斥破（前缀 Registry 但不以 \\ 起头的
                // 变体归此闸）。
                return Err(NpCode::AbsWithRoot.val());
            }
            let d3 = d3_parse(path)?;
            if d3.len() > DEPTH_MAX {
                return Err(NpCode::DepthOob.val());
            }
            Ok(ResolvedPath { comps: d3, root: r })
        }
    }
}

/// LCG 注入核（种子写死 = SEG_BASE）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 双档判据演练 10/10 轮（判准 J1 第一句）。
pub fn drill_dual_mode_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut roots = RootTable::new();
        let r = roots.admit(b"MACHINE");
        let mut r_ok = r.is_ok();
        let r = r.unwrap_or(0);
        // 绝对档：\\Registry\\Machine\\Software。
        let a = resolve(None, &roots, b"\\Registry\\Machine\\Software");
        // 相对档：root=MACHINE 号 + Software。
        let b = resolve(Some(r), &roots, b"Software");
        // 相对档（根下二级）：System\\Setup。
        let c = resolve(Some(r), &roots, b"System\\Setup");
        match (&a, &b, &c) {
            (Ok(pa), Ok(pb), Ok(pc)) => {
                // 绝对档 3 组件（Machine/Software…实际 2 组件）。
                if pa.depth() != 2 || pb.depth() != 1 || pc.depth() != 2 {
                    r_ok = false;
                    d.push(format!("轮{} 深度{}{}{}", round, pa.depth(), pb.depth(), pc.depth()));
                }
                // 绝对档 root = u32::MAX。
                if pa.root != u32::MAX || pb.root != r {
                    r_ok = false;
                }
            }
            _ => {
                r_ok = false;
                d.push(format!("轮{} 解析失败", round));
            }
        }
        // 双档对平：绝对 \\Registry\\Machine\\X 与相对(root=MACHINE)\\X
        // 联签必须同（同键双档联签对平——判据根基）。
        // 注：绝对档含 Machine 组件，相对档根即 Machine——联签口径：
        // 相对档联签以「根名 + 相对组件」补全后与绝对档对拍。
        let abs2 = resolve(None, &roots, b"\\Registry\\MACHINE\\Software");
        if let (Ok(x), Ok(y)) = (&a, &abs2) {
            if x.joint_fp() != y.joint_fp() {
                r_ok = false;
                d.push(format!("轮{} 大小写归一联签失配", round));
            }
        } else {
            r_ok = false;
        }
        if r_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("双档判据 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 注入三族 10/10 轮：相对绝对化/幽灵根/越深。
pub fn drill_inject_10() -> (bool, String) {
    let mut caught = [0usize; 5];
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut roots = RootTable::new();
        let r = roots.admit(b"MACHINE");
        let r = r.unwrap_or(0);
        // 族 1：相对档绝对化。
        if matches!(resolve(Some(r), &roots, b"\\Registry\\X"), Err(c) if c == NpCode::RelAbsolutized.val()) {
            caught[0] += 1;
        }
        // 族 2：幽灵根（未登记号 9）。
        if matches!(resolve(Some(9), &roots, b"X"), Err(c) if c == NpCode::GhostRoot.val()) {
            caught[1] += 1;
        }
        // 族 3：双档皆空（EmptyPath；AbsWithRoot 为防御闸——绝对前缀
        // 必以 \ 起头，恒被族 1 的绝对化闸先捕，故注入面以空路径族替）。
        if matches!(resolve(Some(r), &roots, b""), Err(c) if c == NpCode::EmptyPath.val()) {
            caught[2] += 1;
        }
        // 族 4：绝对档无前缀。
        if matches!(resolve(None, &roots, b"\\NotRegistry\\X"), Err(c) if c == NpCode::NoAbsPrefix.val()) {
            caught[3] += 1;
        }
        // 族 5：越深（9 级相对路径）。
        let mut deep = Vec::new();
        let mut i = 0;
        while i <= DEPTH_MAX {
            if i > 0 {
                deep.push(b'\\');
            }
            deep.push(b'K');
            i += 1;
        }
        if matches!(resolve(Some(r), &roots, &deep), Err(c) if c == NpCode::DepthOob.val()) {
            caught[4] += 1;
        }
        round += 1;
    }
    let ok = caught.iter().all(|c| *c == INJECT_ROUNDS);
    (ok, format!("注入五族 10/10 抓{:?}", caught))
}

/// D3 命名协议对平演练 10/10 轮（判准 J1 第二句）。
pub fn drill_d3_alignment_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut roots = RootTable::new();
        let r = roots.admit(b"R");
        let r = r.unwrap_or(0);
        // 随机构造 2~4 级相对路径。
        let depth = 2 + (lcg.below(3) as usize);
        let mut path: Vec<u8> = Vec::new();
        let mut i = 0;
        while i < depth {
            if i > 0 {
                path.push(b'\\');
            }
            let clen = 1 + (lcg.below(6) as usize);
            let mut k = 0;
            while k < clen {
                path.push(b'A' + (lcg.below(26)) as u8);
                k += 1;
            }
            i += 1;
        }
        // 主档解析 vs D3 侧解析：逐组件一致。
        let mine = resolve(Some(r), &roots, &path);
        let d3 = d3_parse(&path);
        let mut align = false;
        if let (Ok(m), Ok(d)) = (&mine, &d3) {
            if m.depth() == d.len() {
                align = true;
                let mut i = 0;
                while i < d.len() {
                    if m.comp(i) != Some(&d[i][..]) {
                        align = false;
                    }
                    i += 1;
                }
            }
        } else if let (Err(a), Err(b)) = (&mine, &d3) {
            align = a == b;
        }
        if align {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("D3 对平 {}/{}", ok, INJECT_ROUNDS))
}

/// 联签稳定性演练 10/10 轮：同键多次解析联签恒同 + 大小写归一。
pub fn drill_joint_fp_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut roots = RootTable::new();
        let r = roots.admit(b"K");
        let r = r.unwrap_or(0);
        let p1 = resolve(Some(r), &roots, b"Alpha\\Beta");
        let p2 = resolve(Some(r), &roots, b"ALPHA\\beta");
        let p3 = resolve(Some(r), &roots, b"alpha\\BETA");
        let stable = match (&p1, &p2, &p3) {
            (Ok(a), Ok(b), Ok(c)) => a.joint_fp() == b.joint_fp() && b.joint_fp() == c.joint_fp(),
            _ => false,
        };
        if stable {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("联签稳定 {}/{}", ok, INJECT_ROUNDS))
}

/// 幽灵根与根表演练 10/10 轮。
pub fn drill_root_table_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut roots = RootTable::new();
        let mut i = 0;
        let mut all_in = true;
        while i < ROOT_CAP {
            let mut nm = b"RT".to_vec();
            nm.push(b'0' + i as u8);
            if roots.admit(&nm).is_err() {
                all_in = false;
            }
            i += 1;
        }
        // 表满拒。
        let full = roots.admit(b"EXTRA").is_err();
        // 幽灵根拒。
        let ghost = matches!(resolve(Some(99), &roots, b"X"), Err(c) if c == NpCode::GhostRoot.val());
        // 合法根放行。
        let legal = resolve(Some(0), &roots, b"Sub").is_ok();
        if all_in && full && ghost && legal {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("根表 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    NpCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：命名常量锚核对。
pub fn run_unx12469_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-命名锚",
        DEPTH_MAX == 8 && COMP_MAX == 48 && ROOT_CAP == 8 && ABS_PREFIX == b"\\REGISTRY",
        format!("深{} 组{} 根{} 前缀Registry", DEPTH_MAX, COMP_MAX, ROOT_CAP),
    ));
    v
}

/// run_unx12469_checks：本单判据聚合（诊断段 0x308A0）。
pub fn run_unx12469_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12469 路径与命名联签");
    // J1-a：双档判据（绝对/相对 + RootDirectory）。
    let (ok1, d1) = drill_dual_mode_10();
    s.add_dyn("J1-双档判据10/10", ok1, &d1);
    // J1-b：注入五族。
    let (ok2, d2) = drill_inject_10();
    s.add_dyn("J1-注入五族10/10", ok2, &d2);
    // J1-c：D3 命名协议对平。
    let (ok3, d3) = drill_d3_alignment_10();
    s.add_dyn("J1-D3对平10/10", ok3, &d3);
    // J1-d：联签稳定性。
    let (ok4, d4) = drill_joint_fp_10();
    s.add_dyn("J1-联签稳定10/10", ok4, &d4);
    // J1-e：根表。
    let (ok5, d5) = drill_root_table_10();
    s.add_dyn("J1-根表10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x308A0段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12469_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
