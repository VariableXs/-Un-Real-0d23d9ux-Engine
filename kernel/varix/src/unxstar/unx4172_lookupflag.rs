//! unx4172_lookupflag — UNX-F4172 LOOKUP_FOLLOW/NOFOLLOW 标志位（诊断段 0x9720）。
//!
//! 判据原文：
//!   J1 UNX-F4172-J1 中间分量恒跟随、末分量按标志（FOLLOW 跟随/NOFOLLOW
//!      返回链接本体）断言各 1,000 次——与 B10 链接矩阵衔接接口锚定。
//! 设计纪律：
//!   1. 跟随语义二值闭集 Follow/NoFollow，标志只裁决末分量，中途不改旗。
//!   2. 中间分量命中符号链接恒展开重走，展开逐层入账，模块内上限 8 层
//!      防环（B10 的 40 层正式上限由 UNX-F4186 承载，本条不越界）。
//!   3. 原始位掩码入口 LOOKUP_FOLLOW=0x0001 / LOOKUP_NOFOLLOW=0x0002
//!      （Linux namei.h 口径），双位同置、零位或含未知位一律拒收记账。
//!   4. 断言账独立可导出：中间恒跟随账与末分量分派账各满 1,000 次，
//!      且与展开账对平方绿——此即与 B10 链接矩阵的衔接接口锚。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9720;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_ABS: u16 = SEG_BASE + 1;   // 路径非绝对（缺 '/' 根前缀）
const E_BAD_COMP: u16 = SEG_BASE + 2;  // 空分量（双斜杠/尾斜杠/空路径体）
const E_NOENT: u16 = SEG_BASE + 3;     // 分量在解析树中不存在
const E_BAD_FLAG: u16 = SEG_BASE + 4;  // 标志位非法（双位同置/零位/未知位）
const E_MID_CAP: u16 = SEG_BASE + 5;   // 中间展开超模块内 8 层上限
const E_BAD_NODE: u16 = SEG_BASE + 6;  // 节点索引越界（内部账失守）
const E_DUP_NAME: u16 = SEG_BASE + 7;  // 同目录重名建点（树账失守）
const E_LEDGER: u16 = SEG_BASE + 8;    // 断言账与展开账对平失败

/// Linux namei.h 口径的原始查找标志位。
pub const LOOKUP_FOLLOW: u16 = 0x0001;
pub const LOOKUP_NOFOLLOW: u16 = 0x0002;

/// 模块内中间展开上限（B10 正式 40 层另有专条 UNX-F4186）。
pub const MID_CAP: u32 = 8;
/// 判据 J1 规定的断言次数（中间恒跟随与末分量分派各一次口径）。
pub const ASSERT_TIMES: u32 = 1000;

/// 末分量跟随语义二值闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FollowFlag {
    Follow,   // 末分量跟随：解析到链接目标真身
    NoFollow, // 末分量不跟随：停在链接本体
}

/// walk 终态：Real=真身落点；Body=末分量链接本体（NoFollow 专属）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LastOutcome {
    Real(usize),
    Body(usize),
}

/// 解析树节点：目录/普通文件/符号链接三态，字段全账面。
pub struct Node {
    pub name: String,         // 分量名（根为 "/"）
    pub is_dir: bool,         // 目录位
    pub is_link: bool,        // 符号链接位
    pub target: String,       // 链接目标串（绝对路径），非链接恒空
    pub children: Vec<usize>, // 子节点槽位（目录专属）
}

/// 解析树：0 号槽恒为根。
pub struct LinkTree {
    pub nodes: Vec<Node>,
}

impl LinkTree {
    pub fn new() -> LinkTree {
        LinkTree {
            nodes: vec![Node {
                name: "/".to_string(),
                is_dir: true,
                is_link: false,
                target: String::new(),
                children: Vec::new(),
            }],
        }
    }

    /// 在 parent 下建目录或普通文件；重名拒收（树账失守防线）。
    pub fn add_node(&mut self, parent: usize, name: &str, is_dir: bool) -> Result<usize, u16> {
        if parent >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        if name.is_empty() || name.contains('/') {
            return Err(E_BAD_COMP);
        }
        if self.child_of(parent, name).is_some() {
            return Err(E_DUP_NAME);
        }
        let id = self.nodes.len();
        self.nodes.push(Node {
            name: name.to_string(),
            is_dir,
            is_link: false,
            target: String::new(),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    /// 在 parent 下建符号链接（目标串在此不做存在性校验）。
    pub fn add_link(&mut self, parent: usize, name: &str, target: &str) -> Result<usize, u16> {
        if parent >= self.nodes.len() {
            return Err(E_BAD_NODE);
        }
        if name.is_empty() || name.contains('/') || target.is_empty() {
            return Err(E_BAD_COMP);
        }
        if self.child_of(parent, name).is_some() {
            return Err(E_DUP_NAME);
        }
        let id = self.nodes.len();
        self.nodes.push(Node {
            name: name.to_string(),
            is_dir: false,
            is_link: true,
            target: target.to_string(),
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        Ok(id)
    }

    /// 目录内按名找子（线性扫描，索引访问前全比长度）。
    fn child_of(&self, dir: usize, name: &str) -> Option<usize> {
        if dir >= self.nodes.len() {
            return None;
        }
        for &c in &self.nodes[dir].children {
            if c < self.nodes.len() && self.nodes[c].name == name {
                return Some(c);
            }
        }
        None
    }

    /// 绝对路径拆分量；根路径返回空表；空分量拒收。
    fn split<'a>(&self, path: &'a str) -> Result<Vec<&'a str>, u16> {
        if !path.starts_with('/') {
            return Err(E_NOT_ABS);
        }
        let body = &path[1..]; // '/' 为单字节，前缀判定后切片安全
        if body.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for c in body.split('/') {
            if c.is_empty() {
                return Err(E_BAD_COMP);
            }
            out.push(c);
        }
        Ok(out)
    }

    /// 不跟随解析绝对路径（仅用于展开目标串的落点定位）。
    fn resolve_abs(&self, path: &str) -> Result<usize, u16> {
        let comps = self.split(path)?;
        let mut cur = 0usize;
        for c in comps {
            cur = self.child_of(cur, c).ok_or(E_NOENT)?;
        }
        Ok(cur)
    }

    /// 链接链展开：逐层记账，超 MID_CAP 拒收（中间/末跟随共用）。
    fn follow_chain(&self, mut node: usize, ledger: &mut MidLedger) -> Result<usize, u16> {
        let mut layers = 0u32;
        while self.nodes[node].is_link {
            layers += 1;
            ledger.mid_expands += 1;
            if layers > MID_CAP {
                return Err(E_MID_CAP);
            }
            node = self.resolve_abs(&self.nodes[node].target)?;
        }
        Ok(node)
    }

    /// walk 主入口：中间分量恒跟随，末分量按 FollowFlag 裁决（判据 J1）。
    pub fn walk(
        &self,
        path: &str,
        last: FollowFlag,
        ledger: &mut MidLedger,
    ) -> Result<LastOutcome, u16> {
        let comps = self.split(path)?;
        if comps.is_empty() {
            return Ok(LastOutcome::Real(0)); // 根无链接语义，恒真身
        }
        let mut cur = 0usize;
        let n = comps.len();
        for (i, comp) in comps.iter().enumerate() {
            let next = self.child_of(cur, comp).ok_or(E_NOENT)?;
            if i + 1 < n {
                // 中间分量：恒跟随（J1 左半，标志不参与）
                ledger.mid_follows += 1;
                cur = self.follow_chain(next, ledger)?;
            } else {
                // 末分量：按标志裁决（J1 右半）
                match last {
                    FollowFlag::Follow => {
                        ledger.last_follow += 1;
                        cur = self.follow_chain(next, ledger)?;
                    }
                    FollowFlag::NoFollow => {
                        ledger.last_nofollow += 1;
                        if self.nodes[next].is_link {
                            return Ok(LastOutcome::Body(next));
                        }
                        cur = next;
                    }
                }
            }
        }
        Ok(LastOutcome::Real(cur))
    }

    /// 原始位掩码入口：掩码→二值闭集，非法组合拒收并记错误账。
    pub fn walk_raw(
        &self,
        path: &str,
        flags: u16,
        ledger: &mut MidLedger,
    ) -> Result<LastOutcome, u16> {
        let f = match flags {
            LOOKUP_FOLLOW => FollowFlag::Follow,
            LOOKUP_NOFOLLOW => FollowFlag::NoFollow,
            _ => {
                ledger.rejects += 1;
                return Err(E_BAD_FLAG);
            }
        };
        self.walk(path, f, ledger)
    }
}

/// 中间恒跟随 + 末分量分派断言账（判据 J1 的账面载体）。
pub struct MidLedger {
    pub mid_follows: u32,   // 中间分量恒跟随次数
    pub mid_expands: u32,   // 链接展开层数（逐层 +1）
    pub last_follow: u32,   // 末分量 FOLLOW 断言次数
    pub last_nofollow: u32, // 末分量 NOFOLLOW 断言次数
    pub rejects: u32,       // 非法标志拒收数（错误账）
}

impl MidLedger {
    pub fn new() -> MidLedger {
        MidLedger {
            mid_follows: 0,
            mid_expands: 0,
            last_follow: 0,
            last_nofollow: 0,
            rejects: 0,
        }
    }

    /// 对平：末分量两账各达判据次数；展开账不超中间账×上限。
    pub fn conserved(&self) -> bool {
        let total = self.last_follow + self.last_nofollow;
        let cap_all = (self.mid_follows as u64) * (MID_CAP as u64);
        total == ASSERT_TIMES * 2 && (self.mid_expands as u64) <= cap_all
    }

    /// 账面导出（衔接 B10 矩阵的接口锚读面）。
    pub fn snapshot(&self) -> [u32; 5] {
        [
            self.mid_follows,
            self.mid_expands,
            self.last_follow,
            self.last_nofollow,
            self.rejects,
        ]
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_ABS => "路径非绝对（缺 '/' 根前缀）",
        E_BAD_COMP => "空分量（双斜杠/尾斜杠/空路径体）",
        E_NOENT => "分量在解析树中不存在",
        E_BAD_FLAG => "标志位非法（双位同置/零位/未知位）",
        E_MID_CAP => "中间展开超模块内 8 层上限",
        E_BAD_NODE => "节点索引越界（内部账失守）",
        E_DUP_NAME => "同目录重名建点（树账失守）",
        E_LEDGER => "断言账与展开账对平失败",
        _ => "未知诊断码",
    }
}

/// 确定性演练：建树→中间恒跟随 1000→末分量双旗各 1000→掩码与边界注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建树 /bin/ls、/ln_bin→/bin、/ln2→/ln_bin（链中链）。
    let mut t = LinkTree::new();
    let bin = t.add_node(0, "bin", true)?;
    let _ls = t.add_node(bin, "ls", false)?;
    let lnb = t.add_link(0, "ln_bin", "/bin")?;
    let ln2 = t.add_link(0, "ln2", "/ln_bin")?;
    if t.nodes.len() != 5 {
        return Err(E_BAD_NODE);
    }
    hits += 1;

    // R2：中间分量恒跟随 1,000 次——/ln_bin/ls 落到真身 ls。
    let mut lg = MidLedger::new();
    for _ in 0..ASSERT_TIMES {
        match t.walk("/ln_bin/ls", FollowFlag::Follow, &mut lg)? {
            LastOutcome::Real(id) => {
                if t.nodes[id].name != "ls" {
                    return Err(E_LEDGER);
                }
            }
            LastOutcome::Body(_) => return Err(E_LEDGER),
        }
    }
    if lg.mid_follows != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：末分量 FOLLOW 1,000 次——/ln_bin 展开到 bin 目录真身。
    for _ in 0..ASSERT_TIMES {
        match t.walk("/ln_bin", FollowFlag::Follow, &mut lg)? {
            LastOutcome::Real(id) => {
                if t.nodes[id].name != "bin" {
                    return Err(E_LEDGER);
                }
            }
            LastOutcome::Body(_) => return Err(E_LEDGER),
        }
    }
    if lg.last_follow != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：末分量 NOFOLLOW 1,000 次——/ln2 停在链接本体（不展开）。
    for _ in 0..ASSERT_TIMES {
        match t.walk("/ln2", FollowFlag::NoFollow, &mut lg)? {
            LastOutcome::Body(id) => {
                if id != ln2 || t.nodes[id].name != "ln2" {
                    return Err(E_LEDGER);
                }
            }
            LastOutcome::Real(_) => return Err(E_LEDGER),
        }
    }
    if lg.last_nofollow != ASSERT_TIMES || !lg.conserved() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：原始掩码入口——两合法掩码 + 链中链中间分量 /ln2/ls 仍真身。
    match t.walk_raw("/ln2/ls", LOOKUP_FOLLOW, &mut lg)? {
        LastOutcome::Real(id) => {
            if t.nodes[id].name != "ls" {
                return Err(E_LEDGER);
            }
        }
        LastOutcome::Body(_) => return Err(E_LEDGER),
    }
    match t.walk_raw("/ln_bin", LOOKUP_NOFOLLOW, &mut lg)? {
        LastOutcome::Body(id) => {
            if id != lnb {
                return Err(E_LEDGER);
            }
        }
        LastOutcome::Real(_) => return Err(E_LEDGER),
    }
    hits += 1;

    // I1：非法掩码三连——零位/双位同置/未知位，全拒收且错误账 +3。
    let r0 = lg.rejects;
    if t.walk_raw("/bin", 0x0000, &mut lg) != Err(E_BAD_FLAG) {
        return Err(E_BAD_FLAG);
    }
    if t.walk_raw("/bin", LOOKUP_FOLLOW | LOOKUP_NOFOLLOW, &mut lg) != Err(E_BAD_FLAG) {
        return Err(E_BAD_FLAG);
    }
    if t.walk_raw("/bin", 0x0004, &mut lg) != Err(E_BAD_FLAG) {
        return Err(E_BAD_FLAG);
    }
    if lg.rejects != r0 + 3 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I2：相对路径拒收；I3：空分量拒收；I4：不存在分量拒收。
    if t.walk("bin/ls", FollowFlag::Follow, &mut lg) != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    if t.walk("//bin", FollowFlag::Follow, &mut lg) != Err(E_BAD_COMP) {
        return Err(E_BAD_COMP);
    }
    if t.walk("/bin/nope", FollowFlag::Follow, &mut lg) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    hits += 1;

    // I5：中间展开超 8 层——9 级链接链触发 E_MID_CAP（末分量/中间两口径）。
    let names = ["c9", "c8", "c7", "c6", "c5", "c4", "c3", "c2", "c1"];
    let mut tgt = String::from("/bin");
    for n in names.iter() {
        t.add_link(0, n, &tgt)?;
        tgt.clear();
        tgt.push('/');
        tgt.push_str(n);
    }
    if t.walk("/c1", FollowFlag::Follow, &mut lg) != Err(E_MID_CAP) {
        return Err(E_MID_CAP);
    }
    if t.walk("/c1/x", FollowFlag::Follow, &mut lg) != Err(E_MID_CAP) {
        return Err(E_MID_CAP); // 中间位置同样拦截
    }
    hits += 1;

    // R6：对账终审——两断言账各 1,000、展开账对平、快照可导出。
    if !lg.conserved() {
        return Err(E_LEDGER);
    }
    let s = lg.snapshot();
    if s[2] != ASSERT_TIMES || s[3] != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
