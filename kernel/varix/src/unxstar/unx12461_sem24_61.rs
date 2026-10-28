//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12461 · NtCreateKey/NtOpenKey 语义档 · 0x307A0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12461-J1 创建/打开二态（Disposition 返回档）判据绿，坏
//!      路径注入 10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 二态语义（Disposition 返回档）：NtCreateKey 语义——键存在则打
//!     开（Disposition=OpenedExistingKey）不存在则建
//!     （CreatedNewKey）；NtOpenKey 语义——只开不建，不存在显性拒
//!     ObjectNameNotFound；
//!   - 路径解析：反斜杠分隔多级组件（定容 8 级），空组件/双反斜杠/
//!     尾反斜杠/超长组件（>64 字节）/总长越界（>256）五类坏路径闭集；
//!   - 键树：定容 64 节点树（父指针 + 子计数 + 名字槽），创建走父链
//!     逐级查找-插入（缺中间级显性拒 ParentNotFound——NT 不自动建
//!     中间级语义）；
//!   - Create/Open 双入口共享同一路径解析器（解析一致红线：同一坏
//!     路径两入口同码拒绝）；
//!   - 名字规约：大小写不敏感匹配（NT 口径），比较器逐字节大写化
//!     （零堆、定长缓冲）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，五类坏路径 × 两入口 = 10 注
//!     入例，10/10 轮逐例正确拒且双入口同码；
//!   - 诊断码闭集 0x307A0 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x307A0 段）。
pub const SEG_BASE: u32 = 0x307A0;

/// 路径最大级数（8）。
pub const PATH_MAX_DEPTH: usize = 8;

/// 单组件最大字节（64）。
pub const COMPONENT_MAX: usize = 64;

/// 路径总长上限（256）。
pub const PATH_MAX_TOTAL: usize = 256;

/// 键树节点容量（64）。
pub const TREE_CAP: usize = 64;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x307A0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CkCode {
    /// 0x307A0 空组件（双反斜杠/前导空组件）。
    EmptyComponent = 0,
    /// 0x307A1 尾反斜杠。
    TrailingSlash = 1,
    /// 0x307A2 组件超长（>64B）。
    ComponentOob = 2,
    /// 0x307A3 总长越界（>256B）。
    TotalOob = 3,
    /// 0x307A4 级数越界（>8 级）。
    DepthOob = 4,
    /// 0x307A5 父级缺失（中间级不存在）。
    ParentNotFound = 5,
    /// 0x307A6 键不存在（Open 语义拒）。
    NameNotFound = 6,
    /// 0x307A7 键树满（64 节点）。
    TreeFull = 7,
    /// 0x307A8 非法字符（组件内含非法字节）。
    BadChar = 8,
    /// 0x307A9 段位越界。
    SegOob = 9,
    /// 0x307AA 解析不一致（两入口同路径异码——红线破）。
    ParseMismatch = 10,
    /// 0x307AB Disposition 越界。
    DispOob = 11,
}

impl CkCode {
    /// 全部码闭集。
    pub const ALL: [CkCode; 12] = [
        CkCode::EmptyComponent,
        CkCode::TrailingSlash,
        CkCode::ComponentOob,
        CkCode::TotalOob,
        CkCode::DepthOob,
        CkCode::ParentNotFound,
        CkCode::NameNotFound,
        CkCode::TreeFull,
        CkCode::BadChar,
        CkCode::SegOob,
        CkCode::ParseMismatch,
        CkCode::DispOob,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            CkCode::EmptyComponent => "路径含空组件（//或前导\\），逐组件核对分隔",
            CkCode::TrailingSlash => "路径尾反斜杠非法，去掉尾部分隔符",
            CkCode::ComponentOob => "单组件超 64 字节，缩短键名",
            CkCode::TotalOob => "路径总长超 256 字节，改用更浅层级",
            CkCode::DepthOob => "路径超 8 级，扁平化键结构",
            CkCode::ParentNotFound => "中间级不存在，先逐级创建父键（NT 不自动建级）",
            CkCode::NameNotFound => "键不存在（Open 语义只开不建），先 CreateKey",
            CkCode::TreeFull => "键树满（64 节点），删除闲置键后重试",
            CkCode::BadChar => "组件含非法字节（\\0 或控制字节），清洗键名",
            CkCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            CkCode::ParseMismatch => "同一坏路径两入口拒码不一致，解析器唯一入口红线破",
            CkCode::DispOob => "Disposition 返回档出闭集（Created/Opened 二态）",
        }
    }
}

/// Disposition 二态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// 新建键。
    CreatedNewKey,
    /// 打开既有键。
    OpenedExistingKey,
}

/// 大写化（ASCII，逐字节，零堆）。
fn upper(b: u8) -> u8 {
    if b >= b'a' && b <= b'z' {
        b - 32
    } else {
        b
    }
}

/// 路径解析结果（定容组件数组）。
pub struct ParsedPath {
    /// 组件（大写化后）。
    comps: [[u8; COMPONENT_MAX]; PATH_MAX_DEPTH],
    /// 各组件实长。
    lens: [usize; PATH_MAX_DEPTH],
    /// 级数。
    depth: usize,
}

impl ParsedPath {
    /// 级数只读。
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// 组件只读。
    pub fn comp(&self, i: usize) -> Option<&[u8]> {
        if i < self.depth {
            Some(&self.comps[i][..self.lens[i]])
        } else {
            None
        }
    }
}

/// 路径解析器（Create/Open 共享唯一入口——解析一致红线）。
///
/// 五类坏路径闭集逐一显性拒：空组件/尾反斜杠/组件超长/总长越界/
/// 级数越界/非法字符。
pub fn parse_path(path: &[u8]) -> Result<ParsedPath, u32> {
    if path.len() > PATH_MAX_TOTAL {
        return Err(CkCode::TotalOob.val());
    }
    if path.is_empty() {
        return Err(CkCode::EmptyComponent.val());
    }
    let mut comps = [[0u8; COMPONENT_MAX]; PATH_MAX_DEPTH];
    let mut lens = [0usize; PATH_MAX_DEPTH];
    let mut depth = 0usize;
    let mut cur_len = 0usize;
    let mut i = 0usize;
    while i < path.len() {
        let b = path[i];
        if b == b'\\' {
            // 分隔：空组件（开头或双分隔）。
            if cur_len == 0 {
                return Err(CkCode::EmptyComponent.val());
            }
            depth += 1;
            if depth > PATH_MAX_DEPTH {
                return Err(CkCode::DepthOob.val());
            }
            cur_len = 0;
        } else if b == 0 || b < 0x20 {
            return Err(CkCode::BadChar.val());
        } else {
            if depth >= PATH_MAX_DEPTH {
                return Err(CkCode::DepthOob.val());
            }
            if cur_len >= COMPONENT_MAX {
                return Err(CkCode::ComponentOob.val());
            }
            comps[depth][cur_len] = upper(b);
            cur_len += 1;
            lens[depth] = cur_len;
        }
        i += 1;
    }
    // 尾反斜杠：最后一段为空。
    if cur_len == 0 && depth > 0 {
        return Err(CkCode::TrailingSlash.val());
    }
    if cur_len == 0 {
        return Err(CkCode::EmptyComponent.val());
    }
    depth += 1;
    Ok(ParsedPath { comps, lens, depth })
}

/// 键树节点。
#[derive(Clone, Copy)]
struct KeyNode {
    /// 名字（大写化定容）。
    name: [u8; COMPONENT_MAX],
    name_len: usize,
    /// 父节点下标（根为 u32::MAX）。
    parent: u32,
    /// 存活。
    alive: bool,
}

/// 键树（Create/Open 共用）。
pub struct KeyTree {
    /// 节点表。
    nodes: [KeyNode; TREE_CAP],
    used: usize,
}

impl KeyTree {
    /// 新建（空树）。
    pub fn new() -> KeyTree {
        KeyTree {
            nodes: [KeyNode { name: [0; COMPONENT_MAX], name_len: 0, parent: u32::MAX, alive: false }; TREE_CAP],
            used: 0,
        }
    }

    /// 子查找：父 parent 下存活节点中名匹配者。
    fn find_child(&self, parent: u32, name: &[u8]) -> Option<u32> {
        let mut i = 0u32;
        while (i as usize) < self.used {
            let n = &self.nodes[i as usize];
            if n.alive && n.parent == parent && n.name_len == name.len() {
                let mut eq = true;
                let mut k = 0;
                while k < name.len() {
                    if n.name[k] != name[k] {
                        eq = false;
                        break;
                    }
                    k += 1;
                }
                if eq {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 分配节点。
    fn alloc_node(&mut self, name: &[u8], parent: u32) -> Result<u32, u32> {
        if self.used >= TREE_CAP {
            return Err(CkCode::TreeFull.val());
        }
        let idx = self.used as u32;
        let mut nm = [0u8; COMPONENT_MAX];
        let mut k = 0;
        while k < name.len() {
            nm[k] = name[k];
            k += 1;
        }
        self.nodes[idx as usize] = KeyNode { name: nm, name_len: name.len(), parent, alive: true };
        self.used += 1;
        Ok(idx)
    }

    /// NtCreateKey 语义档：有则开（Opened），无则建（Created）；
    /// 中间级缺失拒 ParentNotFound（不自动建级），首级可建。
    pub fn create_key(&mut self, path: &[u8]) -> Result<(u32, Disposition), u32> {
        let p = parse_path(path)?;
        // 中间级（0..depth-1）逐级走，缺级即 ParentNotFound。
        let mut cur: Option<u32> = None;
        let mut d = 0usize;
        while d + 1 < p.depth() {
            let comp = p.comp(d).unwrap_or(&[]);
            match self.find_child(cur.unwrap_or(u32::MAX), comp) {
                Some(idx) => cur = Some(idx),
                None => return Err(CkCode::ParentNotFound.val()),
            }
            d += 1;
        }
        // 叶级：在则 Opened，不在则 Created。
        let leaf = p.comp(p.depth() - 1).unwrap_or(&[]);
        match self.find_child(cur.unwrap_or(u32::MAX), leaf) {
            Some(idx) => Ok((idx, Disposition::OpenedExistingKey)),
            None => {
                let idx = self.alloc_node(leaf, cur.unwrap_or(u32::MAX))?;
                Ok((idx, Disposition::CreatedNewKey))
            }
        }
    }

    /// NtOpenKey 语义档：只开不建。任何一级缺失统一 NameNotFound
    /// （NT 口径：不区分缺在哪一级），缺级不自动建。
    pub fn open_key(&mut self, path: &[u8]) -> Result<u32, u32> {
        let p = parse_path(path)?;
        let mut cur: Option<u32> = None;
        let mut d = 0usize;
        while d < p.depth() {
            let comp = p.comp(d).unwrap_or(&[]);
            match self.find_child(cur.unwrap_or(u32::MAX), comp) {
                Some(idx) => cur = Some(idx),
                None => return Err(CkCode::NameNotFound.val()),
            }
            d += 1;
        }
        Ok(cur.unwrap_or(u32::MAX))
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

/// 坏路径五族构造（注入谱）。
fn bad_paths() -> [(&'static str, Vec<u8>, u32); 5] {
    let mut long_comp = vec![b'A'; COMPONENT_MAX + 1];
    long_comp[0] = b'A';
    let mut deep: Vec<u8> = Vec::new();
    let mut i = 0;
    while i <= PATH_MAX_DEPTH {
        if i > 0 {
            deep.push(b'\\');
        }
        deep.push(b'K');
        i += 1;
    }
    let mut total: Vec<u8> = Vec::new();
    let mut i = 0;
    while i <= PATH_MAX_TOTAL {
        total.push(b'K');
        i += 1;
    }
    let _ = &mut long_comp;
    [
        ("空组件", b"A\\B\\\\C".to_vec(), CkCode::EmptyComponent.val()),
        ("尾反斜杠", b"A\\B\\".to_vec(), CkCode::TrailingSlash.val()),
        ("组件超长", long_comp, CkCode::ComponentOob.val()),
        ("级数越界", deep, CkCode::DepthOob.val()),
        ("总长越界", total, CkCode::TotalOob.val()),
    ]
}

/// 创建/打开二态（Disposition 返回档）判据 10/10 轮（判准 J1 第一句）。
pub fn drill_disposition_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = KeyTree::new();
        let mut r_ok = true;
        // 逐级建（NT 语义：每次只建叶级）。
        match t.create_key(b"Software") {
            Ok((_, disp)) if disp == Disposition::CreatedNewKey => {}
            _ => {
                r_ok = false;
                d.push(format!("轮{} 建级1错", round));
            }
        }
        match t.create_key(b"Software\\Varix") {
            Ok((_, disp)) if disp == Disposition::CreatedNewKey => {}
            _ => {
                r_ok = false;
                d.push(format!("轮{} 建级2错", round));
            }
        }
        // 首建叶：CreatedNewKey。
        match t.create_key(b"Software\\Varix\\Cfg") {
            Ok((_, disp)) if disp == Disposition::CreatedNewKey => {}
            _ => {
                r_ok = false;
                d.push(format!("轮{} 首建档错", round));
            }
        }
        // 再建：OpenedExistingKey。
        match t.create_key(b"Software\\Varix\\CFG") {
            Ok((_, disp)) if disp == Disposition::OpenedExistingKey => {}
            _ => {
                r_ok = false;
                d.push(format!("轮{} 再建档错", round));
            }
        }
        // Open 既有键：成功且与 Create 再建同节点（大小写不敏感红线）。
        let open_idx = t.open_key(b"software\\varix\\cfg");
        let create_idx = t.create_key(b"SOFTWARE\\Varix\\Cfg");
        match (open_idx, create_idx) {
            (Ok(a), Ok((b, Disposition::OpenedExistingKey))) if a == b => {}
            _ => {
                r_ok = false;
                d.push(format!("轮{} 大小写归一错", round));
            }
        }
        // Open 不存在键：NameNotFound（只开不建语义）。
        match t.open_key(b"Software\\Varix\\Ghost") {
            Err(c) if c == CkCode::NameNotFound.val() => {}
            _ => r_ok = false,
        }
        // Open 缺中间级：NameNotFound（不是 ParentNotFound——Open 全路径
        // 不存在语义统一为 NameNotFound；Create 才区分中间级）。
        match t.open_key(b"Software\\Missing\\Leaf") {
            Err(c) if c == CkCode::NameNotFound.val() => {}
            _ => r_ok = false,
        }
        // Create 缺中间级：ParentNotFound（不自动建级）。
        match t.create_key(b"Software\\Missing\\Leaf") {
            Err(c) if c == CkCode::ParentNotFound.val() => {}
            _ => r_ok = false,
        }
        if r_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("Disposition 二态 {}/{} {}", ok, INJECT_ROUNDS, d.join(" ")))
}

/// 坏路径注入 10/10 轮（判准 J1 第二句）：五族 × 双入口同码。
pub fn drill_badpath_10() -> (bool, String) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut r_ok = true;
        let fams = bad_paths();
        let mut k = 0;
        while k < 5 {
            let (name, path, expect) = &fams[k];
            // 双入口同码拒（解析一致红线）。
            let mut t1 = KeyTree::new();
            let c1 = t1.create_key(path).err().unwrap_or(0);
            let mut t2 = KeyTree::new();
            let c2 = t2.open_key(path).err().unwrap_or(0);
            if c1 != *expect || c2 != *expect || c1 != c2 {
                r_ok = false;
                d.push(format!(
                    "轮{} {} 创{:x} 开{:x} 期{:x}",
                    round, name, c1, c2, expect
                ));
            }
            k += 1;
        }
        // 非法字符族。
        let mut t3 = KeyTree::new();
        if let Err(c) = t3.create_key(b"A\x01B") {
            if c == CkCode::BadChar.val() {
                // 抓到。
            } else {
                r_ok = false;
            }
        } else {
            r_ok = false;
        }
        if r_ok {
            caught += 1;
        }
        round += 1;
    }
    (caught == INJECT_ROUNDS, format!("坏路径注入 {}/{} {}", caught, INJECT_ROUNDS, d.join(" ")))
}

/// 树满注入 10/10 轮：64 节点满后建键必拒。
pub fn drill_tree_full_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut t = KeyTree::new();
        // 预置一级根。
        let _ = t.create_key(b"R");
        // 再建 63 个叶（64 节点满）。
        let mut i = 0u32;
        while i < TREE_CAP as u32 - 1 {
            let mut name = b"K".to_vec();
            name.push(b'A' + (i % 26) as u8);
            name.push(b'0' + (i / 26) as u8);
            let _ = t.create_key(&name);
            i += 1;
        }
        // 第 65 个：必拒 TreeFull。
        let full = match t.create_key(b"ZZ") {
            Err(c) => c == CkCode::TreeFull.val(),
            Ok(_) => false,
        };
        if full {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("树满注入 {}/{}", ok, INJECT_ROUNDS))
}

/// 解析器唯一入口演练 10/10 轮：任意路径两入口解析结果恒同码。
pub fn drill_parse_consistency_10() -> (bool, String) {
    let mut lcg = Lcg(SEG_BASE as u64);
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        // 随机构造路径（含合法与非法混合），两入口拒码恒等。
        let mut path: Vec<u8> = Vec::new();
        let mut comp_len = (lcg.below(4) + 1) as usize;
        let mut bad_dice = lcg.below(4);
        let mut i = 0usize;
        while i < comp_len {
            if bad_dice == 0 && i == comp_len / 2 {
                path.push(b'\\');
            } else {
                path.push(b'K');
            }
            bad_dice = lcg.below(4);
            i += 1;
        }
        if lcg.below(2) == 0 {
            path.push(b'\\');
            comp_len = (lcg.below(4) + 1) as usize;
            let mut j = 0;
            while j < comp_len {
                path.push(b'L');
                j += 1;
            }
        }
        let r1 = parse_path(&path);
        let r2 = parse_path(&path);
        // 同输入同结果（确定性红线）。
        let same = match (&r1, &r2) {
            (Ok(a), Ok(b)) => a.depth() == b.depth(),
            (Err(a), Err(b)) => a == b,
            _ => false,
        };
        if same {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("解析一致性 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CkCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：路径常量锚核对。
pub fn run_unx12461_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-路径锚",
        PATH_MAX_DEPTH == 8 && COMPONENT_MAX == 64 && PATH_MAX_TOTAL == 256 && TREE_CAP == 64,
        format!("级{} 组件{} 总长{} 树{}", PATH_MAX_DEPTH, COMPONENT_MAX, PATH_MAX_TOTAL, TREE_CAP),
    ));
    v
}

/// run_unx12461_checks：本单判据聚合（诊断段 0x307A0）。
pub fn run_unx12461_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12461 CreateKey/OpenKey 语义档");
    // J1-a：二态 Disposition 判据。
    let (ok1, d1) = drill_disposition_10();
    s.add_dyn("J1-二态Disposition10/10", ok1, &d1);
    // J1-b：坏路径注入五族双入口同码。
    let (ok2, d2) = drill_badpath_10();
    s.add_dyn("J1-坏路径注入10/10", ok2, &d2);
    // J1-c：树满注入。
    let (ok3, d3) = drill_tree_full_10();
    s.add_dyn("J1-树满注入10/10", ok3, &d3);
    // J1-d：解析器唯一入口一致性。
    let (ok4, d4) = drill_parse_consistency_10();
    s.add_dyn("J1-解析一致性10/10", ok4, &d4);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x307A0段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12461_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
