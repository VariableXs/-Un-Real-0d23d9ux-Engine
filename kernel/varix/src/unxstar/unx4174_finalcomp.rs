//! unx4174_finalcomp — UNX-F4174 walk 终态判定（最后一分量特殊语义）（诊断段 0x9760）。
//!
//! 判据原文：
//!   J1 UNX-F4174-J1 末分量"必须不存在"（create 类）与"必须存在"（open 类）
//!      双模式终态判定断言，EEXIST/ENOENT 时机与 POSIX 一致（各 100 次）。
//! 设计纪律：
//!   1. 双模式闭集：MustExist（open 类：末分量缺席即 ENOENT）与
//!      MustNotExist（create 类：末分量在场即 EEXIST），模式中途不改。
//!   2. POSIX 时机：EEXIST/ENOENT 只允许在末分量阶段抛出并计入末态账
//!      （err_final）；形错与中间缺席/非目录计入中间账（err_mid），
//!      两时机账分立，错位即记账失守。
//!   3. create 类中间缺席同报 ENOENT（父目录缺失不静默补建），open 类
//!      中间缺席报 ENOENT——两模式共享中间分量语义。
//!   4. 根路径特例：open 类对 "/" 恒成立；create 类对 "/" 恒 EEXIST
//!      （根恒存在，POSIX 口径）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9760;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_ABS: u16 = SEG_BASE + 1;   // 路径非绝对
const E_BAD_COMP: u16 = SEG_BASE + 2;  // 空分量
const E_ENOENT: u16 = SEG_BASE + 3;    // 末分量/父目录缺席（POSIX ENOENT 口径）
const E_EEXIST: u16 = SEG_BASE + 4;    // 末分量已在（create 类，POSIX EEXIST 口径）
const E_NOTDIR: u16 = SEG_BASE + 5;    // 中间分量非目录
const E_BAD_NODE: u16 = SEG_BASE + 6;  // 节点槽越界
const E_DUP_NAME: u16 = SEG_BASE + 7;  // 同目录重名
const E_LEDGER: u16 = SEG_BASE + 8;    // 时机账失守（错误账错位）

pub const ASSERT_TIMES: u32 = 100; // 判据规定的双模式各断言次数

/// 终态双模式（判据 J1 的模式闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalMode {
    MustExist,    // open 类：末分量必须在场
    MustNotExist, // create 类：末分量必须缺席
}

/// walk 终态产物：Entered=末分量在场落点；CreateAt=可创建落点（父目录槽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalOutcome {
    Entered(usize),
    CreateAt(usize),
}

/// 终态判定树节点：目录/普通文件两态（链接语义归 B10，此处不越界）。
pub struct FNode {
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<usize>,
}

/// 终态判定树：0 号槽恒为根。
pub struct FinalTree {
    pub nodes: Vec<FNode>,
}

impl FinalTree {
    pub fn new() -> FinalTree {
        FinalTree {
            nodes: vec![FNode {
                name: String::from("/"),
                is_dir: true,
                children: Vec::new(),
            }],
        }
    }

    /// 建点：重名拒收。
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
        self.nodes.push(FNode {
            name: name.to_string(),
            is_dir,
            children: Vec::new(),
        });
        self.nodes[parent].children.push(id);
        Ok(id)
    }

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

    fn split<'a>(&self, path: &'a str) -> Result<Vec<&'a str>, u16> {
        if !path.starts_with('/') {
            return Err(E_NOT_ABS);
        }
        let body = &path[1..]; // '/' 单字节，前缀判定后切片安全
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

    /// 终态判定 walk：中间分量正常走，末分量按模式裁决（POSIX 时机）。
    pub fn walk(
        &self,
        path: &str,
        mode: FinalMode,
        lg: &mut FinalLedger,
    ) -> Result<FinalOutcome, u16> {
        let comps = match self.split(path) {
            Ok(c) => c,
            Err(e) => {
                lg.err_mid += 1; // 形错在入场即拒，归中间时机账
                return Err(e);
            }
        };
        if comps.is_empty() {
            // 根路径：恒存在 → open 类成立；create 类末态 EEXIST。
            return match mode {
                FinalMode::MustExist => {
                    lg.exist_ok += 1;
                    Ok(FinalOutcome::Entered(0))
                }
                FinalMode::MustNotExist => {
                    lg.create_eexist += 1;
                    lg.err_final += 1;
                    Err(E_EEXIST)
                }
            };
        }
        let mut cur = 0usize;
        let n = comps.len();
        for (i, comp) in comps.iter().enumerate() {
            if i + 1 < n {
                // 中间分量：必须在场且为目录，缺席/非目录均中间时机。
                match self.child_of(cur, comp) {
                    Some(next) => {
                        if !self.nodes[next].is_dir {
                            lg.err_mid += 1;
                            return Err(E_NOTDIR);
                        }
                        cur = next;
                    }
                    None => {
                        lg.err_mid += 1;
                        return Err(E_ENOENT);
                    }
                }
            } else {
                // 末分量：按模式裁决（EEXIST/ENOENT 只在此阶段抛出）。
                return match self.child_of(cur, comp) {
                    Some(node) => match mode {
                        FinalMode::MustExist => {
                            lg.exist_ok += 1;
                            Ok(FinalOutcome::Entered(node))
                        }
                        FinalMode::MustNotExist => {
                            lg.create_eexist += 1;
                            lg.err_final += 1;
                            Err(E_EEXIST)
                        }
                    },
                    None => match mode {
                        FinalMode::MustExist => {
                            lg.exist_enoent += 1;
                            lg.err_final += 1;
                            Err(E_ENOENT)
                        }
                        FinalMode::MustNotExist => {
                            lg.create_ok += 1;
                            Ok(FinalOutcome::CreateAt(cur))
                        }
                    },
                };
            }
        }
        Err(E_LEDGER) // 循环穷尽必已返回，此处为账面兜底
    }
}

/// 终态账：双模式断言数 + 错误时机分账（末态 vs 中间态）。
pub struct FinalLedger {
    pub exist_ok: u32,      // MustExist 成功断言数
    pub exist_enoent: u32,  // MustExist 末态 ENOENT 断言数
    pub create_ok: u32,     // MustNotExist 成功断言数
    pub create_eexist: u32, // MustNotExist 末态 EEXIST 断言数
    pub err_final: u32,     // 末态阶段错误总数（时机账）
    pub err_mid: u32,       // 中间阶段错误总数（时机账）
    pub wrong_stage: u32,   // 时机错位计数（判据要求恒为零）
}

impl FinalLedger {
    pub fn new() -> FinalLedger {
        FinalLedger {
            exist_ok: 0,
            exist_enoent: 0,
            create_ok: 0,
            create_eexist: 0,
            err_final: 0,
            err_mid: 0,
            wrong_stage: 0,
        }
    }

    /// 时机账对平：末态账恰等于 ENOENT+EEXIST 断言，错位恒零。
    pub fn timing_clean(&self) -> bool {
        self.err_final == self.exist_enoent + self.create_eexist && self.wrong_stage == 0
    }

    /// 双模式各达判据次数。
    pub fn quotas_met(&self) -> bool {
        self.exist_enoent >= ASSERT_TIMES
            && self.create_eexist >= ASSERT_TIMES
            && self.exist_ok >= ASSERT_TIMES
            && self.create_ok >= ASSERT_TIMES
    }

    /// 账面导出。
    pub fn snapshot(&self) -> [u32; 7] {
        [
            self.exist_ok,
            self.exist_enoent,
            self.create_ok,
            self.create_eexist,
            self.err_final,
            self.err_mid,
            self.wrong_stage,
        ]
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOT_ABS => "路径非绝对",
        E_BAD_COMP => "空分量",
        E_ENOENT => "末分量/父目录缺席（POSIX ENOENT 口径）",
        E_EEXIST => "末分量已在（create 类，POSIX EEXIST 口径）",
        E_NOTDIR => "中间分量非目录",
        E_BAD_NODE => "节点槽越界",
        E_DUP_NAME => "同目录重名",
        E_LEDGER => "时机账失守（错误账错位）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：双模式各 100 次 + 时机分账 + 根特例 + 中间态注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：建树 /dir/f。
    let mut t = FinalTree::new();
    let dir = t.add_node(0, "dir", true)?;
    let f = t.add_node(dir, "f", false)?;
    let mut lg = FinalLedger::new();
    hits += 1;

    // R2：open 类成功 100 次——/dir/f 恒 Entered。
    for _ in 0..ASSERT_TIMES {
        match t.walk("/dir/f", FinalMode::MustExist, &mut lg)? {
            FinalOutcome::Entered(id) => {
                if id != f {
                    return Err(E_LEDGER);
                }
            }
            FinalOutcome::CreateAt(_) => return Err(E_LEDGER),
        }
    }
    if lg.exist_ok != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：open 类末态 ENOENT 100 次——/dir/none 缺席且时机在末态。
    for _ in 0..ASSERT_TIMES {
        if t.walk("/dir/none", FinalMode::MustExist, &mut lg) != Err(E_ENOENT) {
            return Err(E_ENOENT);
        }
    }
    if lg.exist_enoent != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：create 类成功 100 次——/newname 缺席 → CreateAt(根)。
    for _ in 0..ASSERT_TIMES {
        match t.walk("/newname", FinalMode::MustNotExist, &mut lg)? {
            FinalOutcome::CreateAt(p) => {
                if p != 0 {
                    return Err(E_LEDGER);
                }
            }
            FinalOutcome::Entered(_) => return Err(E_LEDGER),
        }
    }
    if lg.create_ok != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R5：create 类末态 EEXIST 100 次——/dir/f 已在，时机在末态。
    for _ in 0..ASSERT_TIMES {
        if t.walk("/dir/f", FinalMode::MustNotExist, &mut lg) != Err(E_EEXIST) {
            return Err(E_EEXIST);
        }
    }
    if lg.create_eexist != ASSERT_TIMES {
        return Err(E_LEDGER);
    }
    if !lg.quotas_met() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R6：根特例——open "/" 成立；create "/" 末态 EEXIST。
    match t.walk("/", FinalMode::MustExist, &mut lg)? {
        FinalOutcome::Entered(0) => {}
        _ => return Err(E_LEDGER),
    }
    if t.walk("/", FinalMode::MustNotExist, &mut lg) != Err(E_EEXIST) {
        return Err(E_EEXIST);
    }
    hits += 1;

    // I1：create 类父目录缺席——ENOENT 但时机在中间账（不静默补建）。
    if t.walk("/none/x", FinalMode::MustNotExist, &mut lg) != Err(E_ENOENT) {
        return Err(E_ENOENT);
    }
    // I2：普通文件作中间目录 → E_NOTDIR（中间账）。
    if t.walk("/dir/f/x", FinalMode::MustExist, &mut lg) != Err(E_NOTDIR) {
        return Err(E_NOTDIR);
    }
    // I3：相对路径拒收（中间账）。
    if t.walk("dir/f", FinalMode::MustExist, &mut lg) != Err(E_NOT_ABS) {
        return Err(E_NOT_ABS);
    }
    if lg.err_mid != 3 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R7：时机账终审——末态账=ENOENT+EEXIST 断言，错位恒零。
    if !lg.timing_clean() {
        return Err(E_LEDGER);
    }
    let s = lg.snapshot();
    if s[4] != s[1] + s[3] {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
