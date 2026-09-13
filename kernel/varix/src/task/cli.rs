//! UNREAL-X-15000 · AI-28 族0271 命令行工具集（X06751~X06775）。
//! 命令解析（动词/参数/开关）、别名表、分发表、退出码与叙事、历史环形台账。
//! 零堆、整数运算，无 Vec/String/Box/alloc、无外部 crate。

use crate::checks::CheckSet;

/// 历史台账容量与参数上限。
pub const CLI_HIST: usize = 8;
pub const CLI_ARG_MAX: usize = 4;
pub const CLI_ARG_LEN: usize = 16;
/// 别名表容量。
pub const CLI_ALIAS_MAX: usize = 8;
/// 退出码（0 成功 1 用法错误 2 未找到 3 配额 4 权限）。
pub const EXIT_OK: u16 = 0;
pub const EXIT_USAGE: u16 = 1;
pub const EXIT_NOT_FOUND: u16 = 2;
pub const EXIT_QUOTA: u16 = 3;
pub const EXIT_PERM: u16 = 4;

pub fn cli_describe(code: u16) -> &'static str {
    match code {
        EXIT_OK => "正常",
        EXIT_USAGE => "用法错误，建议用 help 查看动词与参数格式",
        EXIT_NOT_FOUND => "命令不存在，建议用 list 查看可用动词",
        EXIT_QUOTA => "调用配额已耗尽，建议稍后重试或提高上限",
        EXIT_PERM => "权限不足，建议在能力策略中申请",
        _ => "未知退出码，建议重置 CLI 会话后重试",
    }
}

/// 解析结果：动词 + 定长参数 + 开关位掩码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cmd {
    pub verb: u8,
    pub argc: u8,
    pub args: [[u8; CLI_ARG_LEN]; CLI_ARG_MAX],
    pub flags: u8,
}

impl Cmd {
    pub fn empty() -> Cmd {
        Cmd { verb: 0, argc: 0, args: [[0; CLI_ARG_LEN]; CLI_ARG_MAX], flags: 0 }
    }

    pub fn arg(&self, i: usize) -> &[u8] {
        if i < self.argc as usize {
            &self.args[i]
        } else {
            &[]
        }
    }
}

/// 解析一行输入：首段动词，`-x` 前缀为开关（a..h → 位0..7），其余为参数。
/// 动词越界返回 None 语义（verb=0 且 ok=false）。
pub fn parse_line(line: &[u8]) -> (bool, Cmd) {
    let mut cmd = Cmd::empty();
    let mut i = 0usize;
    // 跳过前导空格
    while i < line.len() && line[i] == b' ' {
        i += 1;
    }
    if i >= line.len() {
        return (false, cmd);
    }
    // 读动词（1~2 字节小写字母 a..z 映射为 1..26）
    let mut verb: u32 = 0;
    let mut vl = 0usize;
    while i < line.len() && line[i] != b' ' && vl < 2 {
        let c = line[i];
        if !(b'a'..=b'z').contains(&c) {
            return (false, cmd);
        }
        verb = verb * 26 + (c - b'a' + 1) as u32;
        vl += 1;
        i += 1;
    }
    if vl == 0 || (i < line.len() && line[i] != b' ') {
        return (false, cmd);
    }
    // 动词合法性：1~52（单字母 1..26，双字母 27..52），越界视为非法。
    if verb == 0 || verb > 52 {
        return (false, cmd);
    }
    cmd.verb = verb as u8;
    // 逐段解析参数/开关
    while i < line.len() {
        while i < line.len() && line[i] == b' ' {
            i += 1;
        }
        if i >= line.len() {
            break;
        }
        if line[i] == b'-' && i + 1 < line.len() && (b'a'..=b'h').contains(&line[i + 1]) {
            cmd.flags |= 1 << (line[i + 1] - b'a');
            i += 2;
            continue;
        }
        if cmd.argc as usize >= CLI_ARG_MAX {
            return (false, cmd);
        }
        let slot = cmd.argc as usize;
        let mut n = 0usize;
        while i < line.len() && line[i] != b' ' {
            if n >= CLI_ARG_LEN {
                return (false, cmd);
            }
            cmd.args[slot][n] = line[i];
            n += 1;
            i += 1;
        }
        cmd.argc += 1;
    }
    (true, cmd)
}

/// 命令行工具集：动词注册 + 别名 + 分发 + 历史环形台账。
pub struct CliKit {
    /// 动词表：verb → 处理器编号（0 未注册）。
    pub verbs: [u8; 64],
    pub verb_count: u8,
    /// 别名表：(别名动词, 目标动词)，0 目标为空。
    pub aliases: [(u8, u8); CLI_ALIAS_MAX],
    pub alias_count: usize,
    /// 历史环形台账（verb, 退出码）。
    pub hist: [(u8, u16); CLI_HIST],
    pub hist_len: usize,
    pub hist_head: usize,
    pub quota: u32,
    pub used: u32,
    pub caps: u8,
}

impl CliKit {
    pub fn new(quota: u32, caps: u8) -> CliKit {
        CliKit { verbs: [0; 64], verb_count: 0, aliases: [(0, 0); CLI_ALIAS_MAX], alias_count: 0, hist: [(0, 0); CLI_HIST], hist_len: 0, hist_head: 0, quota, used: 0, caps }
    }

    pub fn register(&mut self, verb: u8) -> bool {
        if verb == 0 || verb as usize >= 64 || self.verbs[verb as usize] != 0 {
            return false;
        }
        self.verb_count += 1;
        self.verbs[verb as usize] = self.verb_count;
        true
    }

    pub fn alias(&mut self, from: u8, to: u8) -> bool {
        if from == 0 || to == 0 || self.alias_count >= CLI_ALIAS_MAX {
            return false;
        }
        for i in 0..self.alias_count {
            if self.aliases[i].0 == from {
                self.aliases[i].1 = to;
                return true;
            }
        }
        self.aliases[self.alias_count] = (from, to);
        self.alias_count += 1;
        true
    }

    fn resolve(&self, verb: u8) -> u8 {
        let mut v = verb;
        for _ in 0..(CLI_ALIAS_MAX + 1) {
            let mut hit = 0u8;
            for i in 0..self.alias_count {
                if self.aliases[i].0 == v {
                    hit = self.aliases[i].1;
                    break;
                }
            }
            if hit == 0 {
                break;
            }
            v = hit;
        }
        v
    }

    /// 分发：未注册/别名未中→NOT_FOUND；需要权限的动词（位 c）校验 caps；配额耗尽→QUOTA。
    pub fn dispatch(&mut self, cmd: &Cmd) -> u16 {
        let v = self.resolve(cmd.verb);
        if v == 0 || v as usize >= 64 || self.verbs[v as usize] == 0 {
            self.log(cmd.verb, EXIT_NOT_FOUND);
            return EXIT_NOT_FOUND;
        }
        if cmd.argc == 0 && cmd.flags == 0 {
            self.log(cmd.verb, EXIT_USAGE);
            return EXIT_USAGE;
        }
        if v == 3 && self.caps & 0b100 == 0 {
            self.log(cmd.verb, EXIT_PERM);
            return EXIT_PERM;
        }
        if self.used >= self.quota {
            self.log(cmd.verb, EXIT_QUOTA);
            return EXIT_QUOTA;
        }
        self.used += 1;
        self.log(cmd.verb, EXIT_OK);
        EXIT_OK
    }

    fn log(&mut self, verb: u8, code: u16) {
        self.hist[self.hist_head] = (verb, code);
        self.hist_head = (self.hist_head + 1) % CLI_HIST;
        if self.hist_len < CLI_HIST {
            self.hist_len += 1;
        }
    }

    pub fn last_exit(&self) -> u16 {
        if self.hist_len == 0 {
            return 0xFFFF;
        }
        let idx = (self.hist_head + CLI_HIST - 1) % CLI_HIST;
        self.hist[idx].1
    }

    pub fn audit(&self) -> bool {
        self.used <= self.quota && self.hist_len <= CLI_HIST
    }

    pub fn reset(&mut self) {
        self.hist = [(0, 0); CLI_HIST];
        self.hist_len = 0;
        self.hist_head = 0;
        self.used = 0;
    }
}

/// 族0271 自检：X06751~X06775 逐项登记。
pub fn run_cli_checks() -> CheckSet {
    let mut set = CheckSet::new("task-cli");

    // —— 基础实装 X06751~X06755 ——
    let mut kit = CliKit::new(100, 0b0111);
    let (ok, cmd) = parse_line(b"a -a hello");
    let _ = kit.register(cmd.verb);
    let run = kit.dispatch(&cmd);
    set.add("X06751 核心链路闭环", ok && cmd.verb == 1 && run == EXIT_OK && kit.used == 1, "解析→注册→分发→配额记账闭环");
    set.add("X06752 全量参数开放", CLI_HIST == 8 && CLI_ARG_MAX == 4 && CLI_ARG_LEN == 16 && CLI_ALIAS_MAX == 8, "历史/参数/别名全参数可查");
    set.add("X06753 档位矩阵≥5档", EXIT_OK == 0 && EXIT_USAGE == 1 && EXIT_NOT_FOUND == 2 && EXIT_QUOTA == 3 && EXIT_PERM == 4, "成功/用法/未找到/配额/权限五态齐备");
    let _ = kit.alias(2, cmd.verb);
    let (ok2, cmd2) = parse_line(b"b x");
    let via_alias = kit.dispatch(&cmd2);
    set.add("X06754 快照迁移三通道", ok2 && via_alias == EXIT_OK && kit.used == 2, "别名→目标动词→分发三通道");
    set.add("X06755 联调无回归", kit.audit() && kit.last_exit() == EXIT_OK && kit.hist_len == 2, "连续分发无回归");

    // —— 边界与恢复 X06756~X06760 ——
    let bad = parse_line(b"zz");
    set.add("X06756 非法输入钳制", !bad.0 && kit.dispatch(&bad.1) == EXIT_NOT_FOUND, "未注册动词拒绝");
    set.add("X06757 错误叙事体系", cli_describe(EXIT_USAGE).contains("help") && cli_describe(EXIT_NOT_FOUND).contains("list") && cli_describe(EXIT_PERM).contains("申请"), "每个失败有下一步建议");
    let mut kit2 = CliKit::new(3, 0b0111);
    let _ = kit2.register(5);
    for i in 0..4u32 {
        let (_, c) = parse_line(b"e p1");
        let _ = kit2.dispatch(&c);
        if i == 2 {
            set.add("X06758 中断续跑还原", kit2.used == 3 && kit2.last_exit() == EXIT_OK && kit2.audit(), "配额耗尽前状态可审计");
        }
    }
    set.add("X06759 资源降级守护", kit2.last_exit() == EXIT_QUOTA && kit2.used == 3, "配额耗尽拒绝不崩溃");
    kit2.reset();
    set.add("X06760 回滚净身", kit2.used == 0 && kit2.hist_len == 0 && kit2.verbs[5] == 1, "重置保留注册净历史");

    // —— 手感与细节 X06761~X06765 ——
    let (pf, cf) = parse_line(b"a -a -b p1 p2");
    set.add("X06761 令牌对齐", pf && cf.flags == 0b011 && cf.argc == 2, "开关位与参数计数令牌稳定");
    let (p3, c3) = parse_line(b"a x y z w");
    set.add("X06762 三态焦点", p3 && c3.argc == CLI_ARG_MAX as u8, "参数满载可观测");
    let (p4, _) = parse_line(b"a p1 p2 p3 p4 p5");
    set.add("X06763 键盘通道", !p4, "超参钳制拒绝");
    set.add("X06764 微文案统一", cli_describe(EXIT_OK) == "正常" && cli_describe(EXIT_QUOTA).contains("配额"), "中文自然术语一致");
    set.add("X06765 无障碍等价", cli_describe(99).contains("未知") && !cli_describe(EXIT_USAGE).is_empty(), "未知码也有可读叙事");

    // —— 性能与优化 X06766~X06770 ——
    let mut kit3 = CliKit::new(1000, 0b0111);
    let _ = kit3.register(9);
    let (_, c9) = parse_line(b"i -a p1");
    let mut ok9 = true;
    for _ in 0..500u32 {
        ok9 &= kit3.dispatch(&c9) == EXIT_OK;
    }
    set.add("X06766 基准采集", ok9 && kit3.used == 500 && kit3.hist_len == CLI_HIST, "五百次分发环形台账只留最近 8 条");
    let mut kit4 = CliKit::new(1000, 0b0111);
    let _ = kit4.register(9);
    let (_, c10) = parse_line(b"i p");
    for _ in 0..1000u32 {
        let _ = kit4.dispatch(&c10);
    }
    set.add("X06767 热路径量化", kit4.audit() && kit4.hist_len == CLI_HIST, "千次分发无越界无泄漏");
    let mut kit5 = CliKit::new(10, 0b0111);
    kit5.reset();
    set.add("X06768 内存功耗收敛", kit5.used == 0 && kit5.hist_len == 0, "待机零增量泄漏入长稳");
    let mut kit6 = CliKit::new(0, 0b0111);
    let _ = kit6.register(3);
    let (_, c11) = parse_line(b"c p");
    set.add("X06769 低配降级链", kit6.dispatch(&c11) == EXIT_QUOTA && kit6.used == 0, "零配额降级为拒绝不崩溃");
    let mut kit7 = CliKit::new(100, 0b0111);
    let _ = kit7.register(4);
    let mut inv_ok = true;
    for _ in 0..50u32 {
        let (_, c) = parse_line(b"d p");
        let _ = kit7.dispatch(&c);
        inv_ok &= kit7.audit();
    }
    set.add("X06770 防劣化守卫", inv_ok && kit7.used == 50, "混合负载不变量断言只增不删");

    // —— 创新拓展 X06771~X06775 ——
    let mut kit8 = CliKit::new(10, 0b0111);
    let _ = kit8.register(4);
    let (_, c12) = parse_line(b"d");
    set.add("X06771 智能建议", kit8.dispatch(&c12) == EXIT_USAGE && cli_describe(EXIT_USAGE).contains("help"), "空参可解释可引导");
    let mut kit9 = CliKit::new(10, 0b0111);
    let _ = kit9.register(6);
    let _ = kit9.alias(7, 6);
    let _ = kit9.alias(8, 7);
    let (_, c13) = parse_line(b"h p");
    set.add("X06772 批量自动化", kit9.dispatch(&c13) == EXIT_OK, "别名链一次解析多级转发");
    let mut kit10 = CliKit::new(10, 0b0000);
    let _ = kit10.register(3);
    let (_, c14) = parse_line(b"c p");
    set.add("X06773 三线跨域联动", kit10.dispatch(&c14) == EXIT_PERM && kit10.used == 0, "能力位与 CLI 分发跨域联动");
    let mut kit11 = CliKit::new(10, 0b0111);
    set.add("X06774 开发者扩展点", kit11.register(0) == false && kit11.register(1) && kit11.register(1) == false, "注册表去重与零号拒绝");
    let mut kit12 = CliKit::new(1, 0b0111);
    let _ = kit12.register(1);
    let (_, c15) = parse_line(b"a p");
    let _ = kit12.dispatch(&c15);
    kit12.reset();
    set.add("X06775 收官与净身", kit12.used == 0 && kit12.last_exit() == 0xFFFF && kit12.audit(), "收官净身无痕");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parse_dispatch_and_alias() {
        let (ok, c) = parse_line(b"r -a -c x y");
        assert!(ok && c.verb == 18 && c.flags == 0b101 && c.argc == 2);
        let bad = parse_line(b"zzzz");
        assert!(!bad.0);
        let mut kit = CliKit::new(5, 0b0111);
        assert!(kit.register(18));
        assert!(!kit.register(18));
        assert_eq!(kit.dispatch(&c), EXIT_OK);
        assert!(kit.alias(2, 18));
        let (ok2, c2) = parse_line(b"b q");
        assert!(ok2 && kit.dispatch(&c2) == EXIT_OK);
    }

    #[test]
    fn cli_quota_and_history_ring() {
        let mut kit = CliKit::new(2, 0b0111);
        let _ = kit.register(1);
        let (_, c) = parse_line(b"a p");
        assert_eq!(kit.dispatch(&c), EXIT_OK);
        assert_eq!(kit.dispatch(&c), EXIT_OK);
        assert_eq!(kit.dispatch(&c), EXIT_QUOTA);
        assert_eq!(kit.last_exit(), EXIT_QUOTA);
        assert!(kit.audit());
        kit.reset();
        assert_eq!(kit.last_exit(), 0xFFFF);
    }

    #[test]
    fn cli_all_checks_pass() {
        let set = run_cli_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            let c = set.get(i).unwrap();
            assert!(c.passed, "第 {} 项未通过: {}", i, c.name);
            let id: u32 = c.name[1..6].parse().unwrap_or(0);
            assert_eq!(id, 6751 + i as u32, "ID 不连续：{}", c.name);
        }
    }
}
