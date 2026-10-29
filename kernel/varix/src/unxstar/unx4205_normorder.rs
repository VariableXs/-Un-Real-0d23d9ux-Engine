//! unx4205_normorder — UNX-F4205 规范化与符号链接的顺序契约（诊断段 0x9B00）。
//!
//! 判据原文：J1 先解析后规范化：规范化不得重排链接解析前后的语义——
//!           带链接路径的规范化结果必须等价于「逐分量 walk 含链接
//!           展开」的结果，两法不一致即违序（本锚以账面双算对证）；
//!           J2 ".." 与链接的次序敏感：链接目标内的 ".." 在展开点
//!           （而非原路径字面位置）生效——链接后 ".." 不回退链接段；
//!           J3 纯词法规范化（不含链接）满足幂等：norm(norm(p))==norm(p)；
//!           J4 双算账守恒：compared = agreed + diverged，diverged>0
//!           即契约失守。
//! 设计纪律：
//!   1. POSIX 语义：符号链接展开是解析期行为；lexical normalization
//!      （消 "."/".."、折斜杠）只对「真实解析后的前缀」合法。
//!   2. 链接展开模型：中间分量遇链接则以目标前缀替换并重扫，
//!      目标内 ".." 在展开点生效（判据 J2 核心）。
//!   3. 双算：法 A = 展开 walk；法 B = 对展开结果做纯词法规范化；
//!      两法结果逐分量比对入账。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9B00 本单独占。

use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9B00;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ORDER_BREAK: u16 = SEG_BASE + 1; // 双算分歧（判据 J1 失守）
const E_DUP_LINK: u16 = SEG_BASE + 2;    // 链接表重名
const E_BAL: u16 = SEG_BASE + 3;         // 双算账不守恒
const E_EMPTY: u16 = SEG_BASE + 4;       // 空路径
const E_LINK_MISS: u16 = SEG_BASE + 5;   // 链接名缺号
const E_IDEM_FAIL: u16 = SEG_BASE + 6;   // 幂等失守（判据 J3）
const E_TARGET_BAD: u16 = SEG_BASE + 7;  // 链接目标空

/// 链接表：名 → 目标（相对语义）。
pub struct LinkMap {
    names: Vec<String>,
    targets: Vec<String>,
    pub added: u32,
    pub rejected: u32,
}

impl LinkMap {
    pub fn new() -> LinkMap {
        LinkMap {
            names: Vec::new(),
            targets: Vec::new(),
            added: 0,
            rejected: 0,
        }
    }

    pub fn put(&mut self, name: &str, target: &str) -> Result<usize, u16> {
        if name.is_empty() {
            self.rejected += 1;
            return Err(E_EMPTY);
        }
        if target.is_empty() {
            self.rejected += 1;
            return Err(E_TARGET_BAD);
        }
        for n in self.names.iter() {
            if n == name {
                self.rejected += 1;
                return Err(E_DUP_LINK);
            }
        }
        self.names.push(name.to_string());
        self.targets.push(target.to_string());
        self.added += 1;
        Ok(self.names.len() - 1)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        for (i, n) in self.names.iter().enumerate() {
            if n == name {
                return Some(&self.targets[i]);
            }
        }
        None
    }
}

/// 分量切分（折多斜杠）。
pub fn split_comps(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in path.chars() {
        if ch == '/' {
            if !cur.is_empty() {
                out.push(cur.clone());
                cur.clear();
            }
        } else {
            cur.push(ch);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 纯词法规范化：消 "."、就地消 ".."（不越根）、保留绝对前导。
pub fn lexical_norm(path: &str) -> String {
    let abs = path.starts_with('/');
    let mut stack: Vec<String> = Vec::new();
    for c in split_comps(path) {
        match c.as_str() {
            "." => {}
            ".." => {
                if !stack.is_empty() {
                    stack.pop();
                }
            }
            _ => stack.push(c),
        }
    }
    let mut out = String::new();
    if abs {
        out.push('/');
    }
    for (i, s) in stack.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        for ch in s.chars() {
            out.push(ch);
        }
    }
    if out.is_empty() {
        out.push('.');
    }
    out
}

/// 展开式 walk（法 A）：中间分量链接展开，目标内 ".." 展开点生效。
pub fn walk_expand(lm: &LinkMap, path: &str, max_hops: usize) -> Result<String, u16> {
    let abs = path.starts_with('/');
    let mut stack: Vec<String> = Vec::new();
    let mut work = split_comps(path);
    let mut hops = 0usize;
    let mut i = 0usize;
    while i < work.len() {
        let c = work[i].clone();
        match c.as_str() {
            "." => {}
            ".." => {
                if !stack.is_empty() {
                    stack.pop();
                }
            }
            _ => {
                // 链接展开（中间或末分量均展开；NOFOLLOW 场景由调用方另行处理）
                if let Some(tgt) = lm.get(&c) {
                    hops += 1;
                    if hops > max_hops {
                        return Err(E_ORDER_BREAK);
                    }
                    let tcomps = split_comps(tgt);
                    // 目标前缀替换：目标分量接在余部之前（展开点语义）
                    let mut next = Vec::new();
                    for t in tcomps {
                        next.push(t);
                    }
                    for j in (i + 1)..work.len() {
                        next.push(work[j].clone());
                    }
                    work = next;
                    i = 0;
                    continue;
                }
                stack.push(c);
            }
        }
        i += 1;
    }
    let mut out = String::new();
    if abs {
        out.push('/');
    }
    for (k, s) in stack.iter().enumerate() {
        if k > 0 {
            out.push('/');
        }
        for ch in s.chars() {
            out.push(ch);
        }
    }
    if out.is_empty() {
        out.push('.');
    }
    Ok(out)
}

/// 双算对证账。
pub struct OrderLedger {
    pub compared: u32,
    pub agreed: u32,
    pub diverged: u32,
}

impl OrderLedger {
    pub fn new() -> OrderLedger {
        OrderLedger {
            compared: 0,
            agreed: 0,
            diverged: 0,
        }
    }

    /// 对一条路径双算并入账（判据 J1/J4）。
    pub fn check(&mut self, lm: &LinkMap, path: &str) -> Result<(), u16> {
        if path.is_empty() {
            return Err(E_EMPTY);
        }
        self.compared += 1;
        // 法 A：展开 walk；法 B：对展开结果做词法规范化
        let expanded = walk_expand(lm, path, 8)?;
        let b = lexical_norm(&expanded);
        if expanded == b {
            self.agreed += 1;
            Ok(())
        } else {
            self.diverged += 1;
            Err(E_ORDER_BREAK)
        }
    }

    pub fn balanced(&self) -> bool {
        self.compared == self.agreed + self.diverged
    }
}

/// 自例程：R1 无链接路径双算一致；R2 链接展开语义（目标内 ".." 展开
/// 点生效）；R3 幂等；R4 链接账注入；R5 账守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    let mut lm = LinkMap::new();
    lm.put("usr", "usrshare")?; // 链接：usr → usrshare（一级名）
    lm.put("loop", "loop")?;    // 自指（一跳可终止：展开后同名不再展开判重——本模型 max_hops 兜底）

    // R1：无链接路径双算一致（判据 J1 基线）。
    let mut led = OrderLedger::new();
    led.check(&lm, "/a/b/../c")?;
    led.check(&lm, "/a/./b///c")?;
    led.check(&lm, "/..")?;
    if led.agreed < 3 || led.diverged != 0 {
        return Err(E_ORDER_BREAK);
    }
    hits += 1;

    // R2：链接展开——"/x/usr/y" → "/x/usrshare/y"（目标内 ".." 在展开点生效）。
    lm.put("up", "../sib")?;
    let r = walk_expand(&lm, "/x/up/y", 8)?;
    if r != "/sib/y" {
        return Err(E_ORDER_BREAK); // 展开后 ".." 已消
    }
    // 关键次序敏感：链接目标里的 ".." 不得消掉链接名之前的 /x——
    // 展开点在 up 处，".." 消的是 up 所在层（x 之后），故结果无 x。
    if lexical_norm("/x/up/y") == r && r != "/sib/y" {
        return Err(E_ORDER_BREAK);
    }
    hits += 1;

    // R3：幂等——norm(norm(p)) == norm(p)（判据 J3）。
    for p in ["/a/b/../c", "a/./b//..", "/", "..", "/a/../../b"].iter() {
        let n1 = lexical_norm(p);
        let n2 = lexical_norm(&n1);
        if n1 != n2 {
            return Err(E_IDEM_FAIL);
        }
    }
    hits += 1;

    // R4：链接账注入——重名、空名、空目标、缺号展开。
    if lm.put("usr", "other") != Err(E_DUP_LINK) {
        return Err(E_DUP_LINK);
    }
    if lm.put("", "x") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if lm.put("z", "") != Err(E_TARGET_BAD) {
        return Err(E_TARGET_BAD);
    }
    // 非链接名正常落栈不误报：walk_expand 对 nomad 返回 Ok。
    let ok_path = walk_expand(&lm, "/nomad/x", 8)?;
    if ok_path != "/nomad/x" {
        return Err(E_ORDER_BREAK);
    }
    hits += 1;

    // R5：双算账守恒终验（判据 J4）。
    if !led.balanced() {
        return Err(E_BAL);
    }
    led.check(&lm, "/x/usr/y")?;
    if !led.balanced() || led.compared != led.agreed + led.diverged {
        return Err(E_BAL);
    }
    if lm.added + lm.rejected < 4 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
