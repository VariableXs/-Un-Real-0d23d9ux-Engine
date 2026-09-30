//! F037 深化批次四 · 策略规则编译面（compatstar2/deep3 · G-A-37）。
//!
//! 批次一深化覆盖解释卡/越权流/信任窗，批次二覆盖布隆前置/规则裁决，
//! 批次三覆盖审计链验证器；本批补齐【功能定义】门拦截规则「可解释/
//! 可对账」全语义对齐的编译/裁决/容错面：deny/allow 规则表编译（规则
//! 条目 = 模式 + 动作 + 优先级，定长 16 条——主册【设计细节】规则表
//! 定长纪律）、冲突检出（同模式异动作 → 高优先级胜出并记冲突账，
//! 同优先级 deny 胜出——安全侧裁决纪律）、通配符特异性评分（字面字符
//! 数越多越特异，编译收尾按特异性降序定序——解释卡「命中哪条规则」
//! 必须确定性的前提）、未匹配决策记账（无规则命中 → 默认动作 deny +
//! 未匹配计数，零静默——主册【状态与异常】默认拒绝纪律）。
//!
//! 判据对账：主册 G-A-37【功能定义】门拦截「命中规则名/规则来源」
//! 解释卡 +【设计细节】「越权全量审计」的对偶面（未命中也入账）；
//! 模式匹配为前缀 + 尾通配通行口径（域内量化，无 MS 面——如实注明）。
//!
//! 零堆纪律：定长规则表 + 借片扫描，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）

/// 规则表容量 16 条（域内模型口径——门拦截规则定长编译）。
pub const MAX_RULES: usize = 16;
/// 模式定长字节数。
pub const PATTERN_LEN: usize = 16;
/// 动作：拒绝（门拦截 deny 语义）。
pub const ACTION_DENY: u8 = 0;
/// 动作：放行。
pub const ACTION_ALLOW: u8 = 1;
/// 未命中默认动作 = deny（安全侧缺省——主册默认拒绝纪律）。
pub const DEFAULT_ACTION: u8 = ACTION_DENY;
/// 优先级数值越小越高（1 = 最高）。
pub const TOP_PRIORITY: u8 = 1;
/// 通配符字节（仅尾通配语义：'*' 吞剩余后缀）。
pub const WILDCARD: u8 = b'*';

// ---------------------------------------------------------------------------
// 规则条目

/// 一条策略规则：模式 + 动作 + 优先级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule {
    pub pattern: [u8; PATTERN_LEN],
    pub plen: usize,
    pub action: u8,
    pub priority: u8,
}

impl Rule {
    /// 从字节串构造（超长显性报错——不静默截断）。
    pub fn new(pat: &[u8], action: u8, priority: u8) -> Result<Rule, &'static str> {
        if pat.is_empty() || pat.len() > PATTERN_LEN {
            return Err("bad-pattern");
        }
        let mut p = [0u8; PATTERN_LEN];
        p[..pat.len()].copy_from_slice(pat);
        Ok(Rule { pattern: p, plen: pat.len(), action, priority })
    }

    /// 通配符特异性评分：字面字符（非 '*'）数越多越特异
    /// （解释卡裁决确定性的定序依据）。
    pub fn specificity(&self) -> usize {
        self.pattern[..self.plen].iter().filter(|&&b| b != WILDCARD).count()
    }

    /// 模式匹配：字面前缀逐字对拍；尾 '*' 吞剩余后缀；无 '*' 时须全等。
    pub fn matches(&self, path: &[u8]) -> bool {
        let wild = self.plen > 0 && self.pattern[self.plen - 1] == WILDCARD;
        let lit = if wild { self.plen - 1 } else { self.plen };
        if path.len() < lit || self.pattern[..lit] != path[..lit] {
            return false;
        }
        wild || path.len() == lit
    }
}

// ---------------------------------------------------------------------------
// 编译表：冲突检出 + 特异性定序 + 未匹配账

/// 编译后规则表：按特异性降序定序 + 冲突账 + 未匹配账。
pub struct RuleTable {
    rules: [Option<Rule>; MAX_RULES],
    pub count: usize,
    /// 同模式异动作冲突检出计数（高优先级胜出后记账）。
    pub conflicts: u32,
    /// 无规则命中计数（默认动作兜底——零静默）。
    pub unmatched: u32,
    /// 非法动作值检出计数。
    pub bad_action: u32,
}

impl RuleTable {
    pub const fn new() -> Self {
        RuleTable {
            rules: [None; MAX_RULES],
            count: 0,
            conflicts: 0,
            unmatched: 0,
            bad_action: 0,
        }
    }

    /// 编译入表：同模式异动作 → 高优先级胜出并记冲突账
    /// （同优先级 deny 胜出——安全侧裁决；同模式同动作不视为冲突）。
    pub fn compile(&mut self, r: Rule) -> Result<usize, &'static str> {
        if r.action != ACTION_DENY && r.action != ACTION_ALLOW {
            self.bad_action += 1;
            return Err("bad-action");
        }
        for i in 0..self.count {
            if let Some(e) = self.rules[i] {
                let same = e.plen == r.plen && e.pattern[..e.plen] == r.pattern[..r.plen];
                if same && e.action != r.action {
                    self.conflicts += 1;
                    if r.priority < e.priority
                        || (r.priority == e.priority && r.action == ACTION_DENY)
                    {
                        self.rules[i] = Some(r);
                    }
                    return Ok(i);
                }
            }
        }
        if self.count >= MAX_RULES {
            return Err("table-full");
        }
        self.rules[self.count] = Some(r);
        self.count += 1;
        Ok(self.count - 1)
    }

    /// 编译收尾：按特异性降序冒泡定序（同分按优先级升序）——
    /// 定序后首条命中即裁决（解释卡确定性）。
    pub fn finalize(&mut self) {
        for i in 1..self.count {
            let mut j = i;
            while j > 0 {
                let a = self.rules[j - 1].unwrap();
                let b = self.rules[j].unwrap();
                let swap = b.specificity() > a.specificity()
                    || (b.specificity() == a.specificity() && b.priority < a.priority);
                if swap {
                    self.rules.swap(j - 1, j);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
    }

    /// 裁决：定序后首条命中生效，返回（动作, 命中规则位）；
    /// 无命中 → 默认动作 deny + 未匹配记账（零静默）。
    /// 未命中返回规则位 usize::MAX（调用方可解释为「缺省规则」）。
    pub fn decide(&mut self, path: &[u8]) -> (u8, usize) {
        for i in 0..self.count {
            if let Some(r) = self.rules[i] {
                if r.matches(path) {
                    return (r.action, i);
                }
            }
        }
        self.unmatched += 1;
        (DEFAULT_ACTION, usize::MAX)
    }

    /// 定序读数：第 i 条规则（定序检查用）。
    pub fn rule_at(&self, i: usize) -> Option<Rule> {
        if i < self.count {
            self.rules[i]
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次四）

/// 域自检（深化批次四）。
pub fn run_f037f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F037-rulecomp-d4");
    // 1) 特异性评分：字面字符计数（"game1*" 5 字面 > "g*" 1 字面）。
    let r_spec = Rule::new(b"game1*", ACTION_ALLOW, 5).unwrap();
    let r_gen = Rule::new(b"g*", ACTION_DENY, 5).unwrap();
    cs.add("specificity_score", r_spec.specificity() == 5 && r_gen.specificity() == 1, "");
    // 2) 匹配语义：尾 '*' 吞后缀；无 '*' 须全等。
    cs.add(
        "match_semantics",
        r_gen.matches(b"game2.exe") && !r_spec.matches(b"game2.exe")
            && r_spec.matches(b"game1.exe") && !r_gen.matches(b"mspaint.exe"),
        "",
    );
    // 3) 编译入表 + 特异性定序：泛规则先入、specific 规则后排前。
    let mut t = RuleTable::new();
    let _ = t.compile(r_gen);
    let _ = t.compile(r_spec);
    t.finalize();
    cs.add(
        "finalize_order",
        t.count == 2
            && t.rule_at(0).unwrap().pattern[0] == b'g'
            && t.rule_at(0).unwrap().plen == 6
            && t.rule_at(1).unwrap().plen == 2,
        "",
    );
    // 4) 裁决：specific 放行命中在前；泛 deny 兜底在后。
    let (act0, idx0) = t.decide(b"game1.exe");
    let (act1, idx1) = t.decide(b"game2.exe");
    cs.add(
        "decide_specific_first",
        act0 == ACTION_ALLOW && idx0 == 0 && act1 == ACTION_DENY && idx1 == 1,
        "",
    );
    // 5) 冲突检出：同模式异动作 → 高优先级胜出 + 冲突账 +1。
    let mut tc = RuleTable::new();
    let _ = tc.compile(Rule::new(b"tool*", ACTION_DENY, 3).unwrap());
    let _ = tc.compile(Rule::new(b"tool*", ACTION_ALLOW, 2).unwrap());
    let (act_c, _) = tc.decide(b"toolx");
    cs.add("conflict_priority_wins", tc.conflicts == 1 && act_c == ACTION_ALLOW, "");
    // 6) 同优先级冲突 → deny 胜出（安全侧裁决）。
    let mut te = RuleTable::new();
    let _ = te.compile(Rule::new(b"etc*", ACTION_ALLOW, 1).unwrap());
    let _ = te.compile(Rule::new(b"etc*", ACTION_DENY, 1).unwrap());
    let (act_e, _) = te.decide(b"etcd");
    cs.add("conflict_deny_bias", te.conflicts == 1 && act_e == ACTION_DENY, "");
    // 7) 未匹配决策记账：默认 deny + 未匹配计数（零静默）。
    let mut tu = RuleTable::new();
    let _ = tu.compile(r_spec);
    tu.finalize();
    let (act_u, idx_u) = tu.decide(b"nothing.exe");
    cs.add("unmatched_ledger", act_u == ACTION_DENY && idx_u == usize::MAX && tu.unmatched == 1, "");
    // 8) 再裁决一次未匹配 → 计数如实累计（两次）。
    let _ = tu.decide(b"nothing2.exe");
    cs.add("unmatched_accumulate", tu.unmatched == 2, "");
    // 9) 非法动作值显性报错 + 计账。
    let mut tb = RuleTable::new();
    cs.add(
        "bad_action_explicit",
        matches!(tb.compile(Rule::new(b"x*", 7, 1).unwrap()), Err("bad-action"))
            && tb.bad_action == 1,
        "",
    );
    // 10) 表满显性报错（第 17 条 → Err("table-full")）。
    let mut tf = RuleTable::new();
    let mut ok_all = true;
    for i in 0..MAX_RULES {
        let mut pat = [0u8; 8];
        pat[0] = b'a' + i as u8;
        pat[1] = b'*';
        ok_all &= tf.compile(Rule::new(&pat[..2], ACTION_ALLOW, 1).unwrap()).is_ok();
    }
    cs.add(
        "table_full_explicit",
        ok_all
            && matches!(
                tf.compile(Rule::new(b"zz*", ACTION_ALLOW, 1).unwrap()),
                Err("table-full")
            ),
        "",
    );
    // 11) 裁决确定性：同输入两次裁决结果一致（解释卡可复现）。
    let (a1, i1) = t.decide(b"game1.exe");
    let (a2, i2) = t.decide(b"game1.exe");
    cs.add("decide_deterministic", a1 == a2 && i1 == i2 && t.unmatched == 0, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmatched_default_deny_counted() {
        let mut t = RuleTable::new();
        let _ = t.compile(Rule::new(b"game.exe", ACTION_ALLOW, 1).unwrap());
        t.finalize();
        let (act, idx) = t.decide(b"other.exe");
        assert_eq!(act, ACTION_DENY, "未命中默认拒绝（安全侧）");
        assert_eq!(idx, usize::MAX);
        assert_eq!(t.unmatched, 1, "未命中必须入账");
        // 命中后不再计未匹配。
        let (act2, _) = t.decide(b"game.exe");
        assert_eq!(act2, ACTION_ALLOW);
        assert_eq!(t.unmatched, 1);
    }

    #[test]
    fn specificity_ordering_known_values() {
        // 字面字符数递减定序：8 > 4 > 1。
        let r1 = Rule::new(b"game*.exe", ACTION_ALLOW, 1).unwrap();
        let r2 = Rule::new(b"game*", ACTION_ALLOW, 1).unwrap();
        let r3 = Rule::new(b"g*", ACTION_DENY, 1).unwrap();
        assert_eq!(r1.specificity(), 8);
        assert_eq!(r2.specificity(), 4);
        assert_eq!(r3.specificity(), 1);
        let mut t = RuleTable::new();
        for r in [r3, r2, r1] {
            let _ = t.compile(r);
        }
        t.finalize();
        assert_eq!(t.rule_at(0).unwrap().specificity(), 8);
        assert_eq!(t.rule_at(2).unwrap().specificity(), 1);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f037f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
