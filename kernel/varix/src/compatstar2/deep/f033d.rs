//! F033 深化批次二 · 依赖闭包与构建账本面（compatstar2/deep · G-A-33）。
//!
//! 批次一深化覆盖 .gitattributes/.gitignore 解析/make 变量/ninja 边；本批
//! 补齐：目标依赖闭包（BFS 闭包 + 去重——ninja 全量构建计划的模型面）、
//! console 资源池（深度 1——串行化交互命令的 ninja 池语义）、git reflog
//! append-only 操作账（满容如实拒绝——零静默纪律）、patch hunk 不重叠
//! 判定（补丁应用的合法域检查）。
//!
//! 零堆纪律：定长队列与位图，无 alloc。

use crate::checks::CheckSet;

/// 闭包容量（域内口径）。
pub const CLOSURE_CAP: usize = 16;
/// 依赖边表容量。
pub const MAX_EDGES: usize = 32;
/// reflog 容量。
pub const REFLOG_CAP: usize = 16;

/// 目标依赖闭包：从 root 出发沿边表 BFS，去重后返回可达目标数
/// （写入 out，访问序 = BFS 序——确定性对拍面）。
pub fn target_closure(edges: &[(u16, u16)], root: u16, out: &mut [u16; CLOSURE_CAP]) -> usize {
    let mut visited = 0u32; // 位图（目标 id < 32）
    let mut queue = [0u16; CLOSURE_CAP];
    let (mut qh, mut qt) = (0usize, 0usize);
    if (root as u32) >= 32 {
        return 0;
    }
    queue[qt] = root;
    qt += 1;
    visited |= 1 << root; // root 自身不入 out（out = 依赖闭包，不含根）
    let mut n = 0usize;
    while qh < qt {
        let cur = queue[qh];
        qh += 1;
        for &(from, to) in edges.iter() {
            if from == cur && visited >> to & 1 == 0 {
                visited |= 1 << to;
                if n < CLOSURE_CAP {
                    out[n] = to;
                    n += 1;
                }
                if qt < CLOSURE_CAP {
                    queue[qt] = to;
                    qt += 1;
                }
            }
        }
    }
    n
}

/// console 资源池：深度 1（ninja console pool 语义——交互命令串行化）。
pub struct ConsolePool {
    pub depth: u32,
    pub busy: u32,
    /// 池满被拒计数（调度账面）。
    pub rejected: u32,
}

impl ConsolePool {
    pub const fn new() -> Self {
        ConsolePool { depth: 1, busy: 0, rejected: 0 }
    }
    pub fn acquire(&mut self) -> bool {
        if self.busy >= self.depth {
            self.rejected += 1;
            return false;
        }
        self.busy += 1;
        true
    }
    pub fn release(&mut self) -> bool {
        if self.busy == 0 {
            return false;
        }
        self.busy -= 1;
        true
    }
}

/// reflog：append-only 操作账（op: 1=commit 2=reset 3=checkout；序号单调）。
pub struct RefLog {
    pub ops: [(u8, u32); REFLOG_CAP],
    pub count: usize,
    next_seq: u32,
}

impl RefLog {
    pub const fn new() -> Self {
        RefLog { ops: [(0, 0); REFLOG_CAP], count: 0, next_seq: 1 }
    }
    /// 追加：满容如实拒绝（append-only——不覆盖不丢弃，零静默）。
    pub fn append(&mut self, op: u8) -> Option<u32> {
        if self.count >= REFLOG_CAP {
            return None;
        }
        let seq = self.next_seq;
        self.ops[self.count] = (op, seq);
        self.count += 1;
        self.next_seq += 1;
        Some(seq)
    }
}

/// patch hunk 不重叠判定：两 hunk 作用于同一文件的区间若相交则冲突
/// （hunk A [a, a+la) 与 hunk B [b, b+lb) 半开区间相交判定）。
pub fn hunks_compatible(a_off: u32, a_len: u32, b_off: u32, b_len: u32) -> bool {
    a_off + a_len <= b_off || b_off + b_len <= a_off
}

/// 域自检（深化批次二）。
pub fn run_f033d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F033-buildchain-d2");
    // 1) 依赖闭包：菱形 0→{1,2}, 1→3, 2→3 → 闭包 {1,2,3} 计 3（去重）。
    let edges = [(0u16, 1u16), (0, 2), (1, 3), (2, 3)];
    let mut out = [0u16; CLOSURE_CAP];
    let n = target_closure(&edges, 0, &mut out);
    cs.add("closure_dedup", n == 3 && out[0] == 1 && out[1] == 2 && out[2] == 3, "");
    // 2) console 池：深度 1——第二个获取被拒、释放后可再取。
    let mut pool = ConsolePool::new();
    let first = pool.acquire();
    let second = pool.acquire();
    let third = pool.release() && pool.acquire();
    cs.add("console_pool_serialized", first && !second && third && pool.rejected == 1, "");
    // 3) reflog：追加序号单调；满容如实 None（append-only 不静默丢）。
    let mut rl = RefLog::new();
    let s1 = rl.append(1);
    let s2 = rl.append(2);
    let mut full_ok = true;
    while rl.count < REFLOG_CAP {
        full_ok &= rl.append(1).is_some();
    }
    cs.add("reflog_append_only", s1 == Some(1) && s2 == Some(2) && full_ok && rl.append(3).is_none(), "");
    // 4) hunk 不重叠：相离过、相交拒、相接（恰好邻接）过。
    cs.add(
        "hunk_compat",
        hunks_compatible(0, 5, 10, 5)
            && !hunks_compatible(0, 6, 5, 5)
            && hunks_compatible(0, 5, 5, 5),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closure_empty_edges() {
        let mut out = [0u16; CLOSURE_CAP];
        assert_eq!(target_closure(&[], 0, &mut out), 0, "无边 → 空闭包");
    }

    #[test]
    fn closure_self_edge_no_loop() {
        // 自环不产生重复（visited 位图兜底）。
        let edges = [(0u16, 0u16), (0, 1)];
        let mut out = [0u16; CLOSURE_CAP];
        assert_eq!(target_closure(&edges, 0, &mut out), 1);
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f033d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
