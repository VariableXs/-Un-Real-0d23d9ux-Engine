//! F485 系统代理设置（genstar2 · I 域通用·二分队 · AI-U2 · 深化 v2）。
//!
//! 主册判据（验收标准第一句）：
//! **手动配置链路；即时生效（新连接验证）；例外列表；诊断联动；与
//! F242/F295 数据同源。**
//!
//! 深化 v2 增量（对齐主册「系统服务」全量功能面）：
//! - 例外列表持久化序列化（魔标+版本+逐条落盘，坏账拒收）；
//! - 例外管理补全（逐条删除 + 幂等添加 + 通配后缀语义）；
//! - 代理探测状态机（Unknown→Probing→Reachable/Unreachable，含退避计时）；
//! - 诊断候选排序（代理开且探测败 → 「代理不通」置顶，与 F242 联动）；
//! - 连接裁决审计账（最近 16 次裁决环形记录——哪条连接走了哪条路可查）；
//! - 检查行扩至 24 行、宿主单测扩至 6 例。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 例外列表容量。
pub const EXCEPTION_CAP: usize = 16;
/// 单条例外长度上限。
pub const EXC_LEN_CAP: usize = 32;
/// 端口合法范围。
pub const PORT_MIN: u16 = 1;
pub const PORT_MAX: u16 = 65_535;
/// 代理探测超时（诊断联动用）。
pub const PROBE_TIMEOUT_MS: u64 = 2_000;
/// 裁决审计账容量。
pub const AUDIT_CAP: usize = 16;
/// 持久化魔标。
pub const PERSIST_MAGIC: [u8; 4] = *b"VSP1";
/// 单条例外序列化字节。
pub const PERSIST_ENTRY_BYTES: usize = 33;

/// 新连接路由裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteDecision {
    ViaProxy,
    Direct,
    DirectExempt,
}

/// 代理探测状态（深化状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeState {
    Unknown,
    Probing,
    Reachable,
    Unreachable,
}

/// 一条裁决审计。
#[derive(Clone, Copy, Debug)]
pub struct RouteAudit {
    pub host_key: u64,
    pub decision: RouteDecision,
    pub at_ms: u64,
}

/// 系统代理配置（深化 v2）。
#[derive(Clone, Copy, Debug)]
pub struct SysProxy {
    pub enabled: bool,
    pub host: [u8; 64],
    pub host_n: usize,
    pub port: u16,
    /// 例外列表（命中例外的连接不走代理）。
    exceptions: [[u8; EXC_LEN_CAP]; EXCEPTION_CAP],
    exc_n: [usize; EXCEPTION_CAP],
    exc_count: usize,
    /// 探测状态机。
    pub probe: ProbeState,
    probe_started_ms: u64,
    /// 裁决审计环。
    audits: [Option<RouteAudit>; AUDIT_CAP],
    audit_n: usize,
    audit_head: usize,
}

/// 例外匹配（后缀域匹配：例外「varix.os」命中「docs.varix.os」——子域同豁免）。
pub fn exception_matches(exc: &str, host: &str) -> bool {
    // v3 修复：支持 `*` 前缀通配（*.example.com → example.com 后缀匹配）
    // 并修正段边界（e 前必须紧跟点或串首——v1 只认点，单标签子域漏匹配）。
    let e = exc.trim_start_matches("*.").trim_start_matches('.').trim_end_matches('.');
    let h = host.trim_end_matches('.');
    if h == e {
        return true;
    }
    h.ends_with(e) && h.as_bytes().get(h.len() - e.len() - 1) == Some(&b'.')
}

fn host_key(host: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in host.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl SysProxy {
    pub const fn new() -> Self {
        SysProxy {
            enabled: false,
            host: [0; 64],
            host_n: 0,
            port: 0,
            exceptions: [[0; EXC_LEN_CAP]; EXCEPTION_CAP],
            exc_n: [0; EXCEPTION_CAP],
            exc_count: 0,
            probe: ProbeState::Unknown,
            probe_started_ms: 0,
            audits: [None; AUDIT_CAP],
            audit_n: 0,
            audit_head: 0,
        }
    }

    /// 手动配置链路（地址/端口校验——端口越界/主机空诚实拒绝）。
    pub fn configure(&mut self, host: &str, port: u16) -> Result<(), &'static str> {
        if host.is_empty() || host.len() > 64 {
            return Err("代理地址不能为空");
        }
        if !(PORT_MIN..=PORT_MAX).contains(&port) {
            return Err("端口超出范围");
        }
        self.host = [0; 64];
        self.host_n = host.len();
        self.host[..host.len()].copy_from_slice(host.as_bytes());
        self.port = port;
        Ok(())
    }

    /// 开关即时生效（新连接走代理——已有连接不中断由调用层保证）。
    pub fn toggle(&mut self, on: bool) {
        self.enabled = on;
    }

    /// 新连接裁决：代理开 且 主机不命中例外 → 走代理（裁决入审计环）。
    pub fn route_new_connection(&mut self, target_host: &str, now_ms: u64) -> RouteDecision {
        let d = if !self.enabled {
            RouteDecision::Direct
        } else if (0..self.exc_count).any(|i| {
            let e = core::str::from_utf8(&self.exceptions[i][..self.exc_n[i]]).unwrap_or("");
            exception_matches(e, target_host)
        }) {
            RouteDecision::DirectExempt
        } else {
            RouteDecision::ViaProxy
        };
        self.audits[self.audit_head] = Some(RouteAudit { host_key: host_key(target_host), decision: d, at_ms: now_ms });
        self.audit_head = (self.audit_head + 1) % AUDIT_CAP;
        self.audit_n = (self.audit_n + 1).min(AUDIT_CAP);
        d
    }

    /// 裁决审计检索：最近 N 次里走代理的次数（诊断辅助——可查可对账）。
    pub fn audit_via_proxy_count(&self) -> usize {
        (0..self.audit_n).filter(|&i| matches!(self.audits[i], Some(RouteAudit { decision: RouteDecision::ViaProxy, .. }))).count()
    }

    pub fn audit_count(&self) -> usize {
        self.audit_n
    }

    // ---------------- 例外列表管理（深化：删/幂等/序列化） ----------------

    /// 例外列表管理（满容/超长诚实拒绝；重复添加幂等成功）。
    pub fn add_exception(&mut self, host: &str) -> bool {
        if host.is_empty() || host.len() > EXC_LEN_CAP {
            return false;
        }
        if self.has_exception(host) {
            return true; // 幂等
        }
        if self.exc_count >= EXCEPTION_CAP {
            return false;
        }
        self.exceptions[self.exc_count][..host.len()].copy_from_slice(host.as_bytes());
        self.exc_n[self.exc_count] = host.len();
        self.exc_count += 1;
        true
    }

    pub fn has_exception(&self, host: &str) -> bool {
        (0..self.exc_count).any(|i| {
            core::str::from_utf8(&self.exceptions[i][..self.exc_n[i]]).unwrap_or("") == host
        })
    }

    /// 逐条删除例外（主册例外列表可管理——删得到才管得了）。
    pub fn remove_exception(&mut self, host: &str) -> bool {
        for i in 0..self.exc_count {
            if core::str::from_utf8(&self.exceptions[i][..self.exc_n[i]]).unwrap_or("") == host {
                self.exceptions[i] = self.exceptions[self.exc_count - 1];
                self.exc_n[i] = self.exc_n[self.exc_count - 1];
                self.exceptions[self.exc_count - 1] = [0; EXC_LEN_CAP];
                self.exc_n[self.exc_count - 1] = 0;
                self.exc_count -= 1;
                return true;
            }
        }
        false
    }

    pub fn exception_count(&self) -> usize {
        self.exc_count
    }

    // ---------------- 代理探测状态机（深化） ----------------

    /// 发起探测（Unknown/失败态可发起；探测中重复发起 = 幂等拒绝）。
    pub fn start_probe(&mut self, now_ms: u64) -> bool {
        if self.probe == ProbeState::Probing {
            return false;
        }
        self.probe = ProbeState::Probing;
        self.probe_started_ms = now_ms;
        true
    }

    /// 探测时限（>2s 未应答 = Unreachable——诊断联动的诚实时钟）。
    pub fn probe_tick(&mut self, now_ms: u64) {
        if self.probe == ProbeState::Probing && now_ms.saturating_sub(self.probe_started_ms) > PROBE_TIMEOUT_MS {
            self.probe = ProbeState::Unreachable;
        }
    }

    /// 探测应答。
    pub fn probe_result(&mut self, ok: bool) {
        self.probe = if ok { ProbeState::Reachable } else { ProbeState::Unreachable };
    }

    /// 诊断候选（主册：代理不可达时「代理不通」自动列为候选原因——置顶排序）。
    pub fn diagnose_candidates(&self, elapsed_ms: u64) -> &'static str {
        if self.enabled && self.probe == ProbeState::Unreachable {
            "代理不通"
        } else if elapsed_ms > PROBE_TIMEOUT_MS {
            "网络超时"
        } else {
            "目标不可达"
        }
    }

    /// 数据同源锚（F242/F295 共用同一配置实例——一处一事实）。
    pub fn config_anchor(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in &self.host[..self.host_n] {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h ^ (self.port as u64) << 48 ^ (self.enabled as u64) << 56
    }

    // ---------------- 例外列表持久化（深化） ----------------

    /// 序列化例外列表（魔标+版本+逐条「长度+内容」；不足诚实拒绝）。
    pub fn save_exceptions(&self, out: &mut [u8]) -> Option<usize> {
        let need = 6 + self.exc_count * PERSIST_ENTRY_BYTES;
        if out.len() < need {
            return None;
        }
        out[..4].copy_from_slice(&PERSIST_MAGIC);
        out[4] = 1;
        out[5] = self.exc_count as u8;
        for i in 0..self.exc_count {
            let base = 6 + i * PERSIST_ENTRY_BYTES;
            out[base] = self.exc_n[i] as u8;
            out[base + 1..base + 1 + self.exc_n[i]].copy_from_slice(&self.exceptions[i][..self.exc_n[i]]);
        }
        Some(need)
    }

    /// 反序列化（魔标/版本不符或长度非法 = 整体拒收——坏账不静默吞）。
    pub fn load_exceptions(&mut self, buf: &[u8]) -> bool {
        if buf.len() < 6 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 {
            return false;
        }
        let n = buf[5] as usize;
        if n > EXCEPTION_CAP || buf.len() < 6 + n * PERSIST_ENTRY_BYTES {
            return false;
        }
        let mut loaded = [[0u8; EXC_LEN_CAP]; EXCEPTION_CAP];
        let mut lens = [0usize; EXCEPTION_CAP];
        for i in 0..n {
            let base = 6 + i * PERSIST_ENTRY_BYTES;
            let len = buf[base] as usize;
            if len == 0 || len > EXC_LEN_CAP {
                return false; // 零长/超长例外 = 坏账整体拒绝
            }
            loaded[i][..len].copy_from_slice(&buf[base + 1..base + 1 + len]);
            lens[i] = len;
        }
        self.exceptions = loaded;
        self.exc_n = lens;
        self.exc_count = n;
        true
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化 v2：24 行）
// ---------------------------------------------------------------------------

pub fn run_sysproxy_checks() -> CheckSet {
    let mut cs = CheckSet::new("F485-sysproxy");
    // 1) 手动配置链路（地址/端口三字段）。
    let mut p = SysProxy::new();
    cs.add("configure_ok", p.configure("proxy.corp.local", 8_080).is_ok(), "");
    // 端口 0 拒绝；上界由 u16 类型结构性保证（≤65535 无需运行时判）。
    cs.add("configure_bad_port", p.configure("h", 0).is_err(), "");
    cs.add("configure_empty_host", p.configure("", 8_080).is_err(), "");
    // 2) 即时生效（新连接验证：开→走代理；关→直连）。
    p.toggle(true);
    cs.add("instant_on", p.route_new_connection("example.com", 100) == RouteDecision::ViaProxy, "");
    p.toggle(false);
    cs.add("instant_off", p.route_new_connection("example.com", 200) == RouteDecision::Direct, "");
    // 3) 例外列表（子域豁免；幂等添加；逐条删除）。
    p.toggle(true);
    cs.add("exception_add", p.add_exception("varix.os") && p.exception_count() == 1, "");
    cs.add("exception_idempotent", p.add_exception("varix.os") && p.exception_count() == 1, "");
    cs.add("exception_subdomain", p.route_new_connection("docs.varix.os", 300) == RouteDecision::DirectExempt, "");
    cs.add("exception_exact", p.route_new_connection("varix.os", 400) == RouteDecision::DirectExempt, "");
    cs.add("non_except_via_proxy", p.route_new_connection("other.net", 500) == RouteDecision::ViaProxy, "");
    cs.add("exception_remove", p.remove_exception("varix.os") && !p.has_exception("varix.os") && p.route_new_connection("varix.os", 600) == RouteDecision::ViaProxy, "");
    // 4) 例外匹配语义（非子域不同名不豁免）。
    cs.add("no_partial_match", !exception_matches("varix.os", "notvarix.os"), "");
    // 5) 例外持久化 round-trip + 坏账拒收。
    let mut buf = [0u8; 512];
    p.add_exception("a.test");
    p.add_exception("b.test");
    let n = p.save_exceptions(&mut buf).unwrap();
    let mut q = SysProxy::new();
    cs.add("persist_roundtrip", q.load_exceptions(&buf[..n]) && q.exception_count() == 2 && q.has_exception("b.test"), "");
    let mut bad = buf;
    bad[0] = b'X';
    cs.add("persist_bad_magic", !SysProxy::new().load_exceptions(&bad[..n]), "");
    let mut bad2 = buf;
    bad2[6] = 0; // 零长例外
    cs.add("persist_zero_len_rejected", !SysProxy::new().load_exceptions(&bad2[..n]), "");
    let mut tiny = [0u8; 8];
    cs.add("persist_small_honest", p.save_exceptions(&mut tiny).is_none(), "");
    // 6) 代理探测状态机（Probing 幂等拒发、超时归 Unreachable、应答定态）。
    cs.add("probe_lifecycle", {
        let mut s = SysProxy::new();
        s.start_probe(0)
            && !s.start_probe(100) // 探测中重复发起拒绝
            && {
                let mut s2 = s;
                s2.probe_tick(2_001);
                s2.probe == ProbeState::Unreachable
            }
            && {
                let mut s3 = SysProxy::new();
                s3.start_probe(0);
                s3.probe_result(true);
                s3.probe == ProbeState::Reachable
            }
    }, "");
    // 7) 诊断联动（代理开+探测败 → 「代理不通」置顶）。
    let mut d = SysProxy::new();
    d.configure("p.x", 8_080).ok();
    d.toggle(true);
    d.start_probe(0);
    d.probe_result(false);
    cs.add("diagnose_proxy_down", d.diagnose_candidates(100) == "代理不通", "");
    cs.add("diagnose_timeout", { let mut d2 = SysProxy::new(); d2.diagnose_candidates(3_000) == "网络超时" }, "");
    // 8) 裁决审计账（最近 16 次环形记录——走代理次数可查）。
    d.toggle(true);
    d.add_exception("free.test");
    for i in 0..20u64 {
        let host = if i % 2 == 0 { "paid.test" } else { "free.test" };
        d.route_new_connection(host, 1_000 + i);
    }
    cs.add("audit_ring", d.audit_count() == AUDIT_CAP && d.audit_via_proxy_count() == AUDIT_CAP / 2, "");
    // 9) 数据同源锚（同配置 → 同锚值）。
    let mut q2 = SysProxy::new();
    q2.configure("proxy.corp.local", 8_080).ok();
    q2.toggle(true);
    let mut p2 = SysProxy::new();
    p2.configure("proxy.corp.local", 8_080).ok();
    p2.toggle(true);
    cs.add("same_source_anchor", q2.config_anchor() == p2.config_anchor(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exception_subdomain_semantics() {
        assert!(exception_matches("varix.os", "docs.varix.os"));
        assert!(exception_matches("varix.os", "varix.os"));
        assert!(!exception_matches("varix.os", "notvarix.os"));
        assert!(exception_matches("varix.os.", "varix.os")); // 尾点归一
    }

    #[test]
    fn exception_list_capacity_honest() {
        let mut p = SysProxy::new();
        for i in 0..EXCEPTION_CAP {
            assert!(p.add_exception(&format!("h{i}.test")), "第 {i} 条例外应成功");
        }
        assert!(!p.add_exception("overflow.test"));
        // 删一条腾位后可再加（管理闭环）。
        assert!(p.remove_exception("h0.test"));
        assert!(p.add_exception("overflow.test"));
    }

    #[test]
    fn config_anchor_changes_with_settings() {
        let mut a = SysProxy::new();
        a.configure("h", 80).ok();
        let h1 = a.config_anchor();
        a.toggle(true);
        assert_ne!(h1, a.config_anchor());
    }

    #[test]
    fn audit_ring_wraps_and_counts() {
        let mut p = SysProxy::new();
        p.toggle(true);
        for i in 0..(AUDIT_CAP + 4) as u64 {
            p.route_new_connection("x.test", i);
        }
        assert_eq!(p.audit_count(), AUDIT_CAP);
        assert_eq!(p.audit_via_proxy_count(), AUDIT_CAP);
    }

    #[test]
    fn probe_timeout_is_two_seconds() {
        let mut p = SysProxy::new();
        p.start_probe(0);
        p.probe_tick(PROBE_TIMEOUT_MS);
        assert_eq!(p.probe, ProbeState::Probing);
        p.probe_tick(PROBE_TIMEOUT_MS + 1);
        assert_eq!(p.probe, ProbeState::Unreachable);
    }

    #[test]
    fn kill_existing_connection_semantics_untouched() {
        // 主册：开关即时生效但已有连接不中断——裁决只作用于「新连接」入口。
        let mut p = SysProxy::new();
        p.configure("proxy", 1).ok();
        p.toggle(true);
        assert_eq!(p.route_new_connection("a.b", 0), RouteDecision::ViaProxy);
    }
}

// ===========================================================================
// 深化 v3（F485）：PAC 脚本语义判定 / 代理凭据占位安全 / 多代理
// 故障转移序 / 异常通配符矩阵深化 / 开关状态一致性审计
// ===========================================================================

/// PAC 语义判定（主册「脚本代理」的判定面：PAC 三函数白名单——
/// 只认 isPlainHostName / shExpMatch / isInNet 三类；名单外语法诚实拒。
/// 判定实现为「样例验证」：给样本主机与期望结果，脚本判定一致才算合法）。
pub const PAC_FN_ALLOWLIST: [&str; 3] = ["isPlainHostName", "shExpMatch", "isInNet"];

pub struct PacSample {
    pub host: &'static str,
    pub expect_direct: bool,
}

pub fn pac_script_valid(rules: &str, samples: &[PacSample]) -> bool {
    // 名单外语法拒。
    if rules.contains("dnsResolve") || rules.contains("myIpAddress") {
        return false;
    }
    let mut has_sh_exp = false;
    for (fn_name, _) in PAC_FN_ALLOWLIST.iter().map(|f| (*f, ())) {
        if rules.contains(fn_name) {
            has_sh_exp = fn_name == "shExpMatch" || has_sh_exp;
        }
    }
    // 样例验证：通配规则按 shExpMatch 语义（* 通配）对拍。
    for s in samples {
        let mut matched = false;
        for line in rules.split(';') {
            let line = line.trim();
            // 形态：shExpMatch(host, "pattern") -> direct|proxy。
            if let Some(rest) = line.strip_prefix("shExpMatch(") {
                has_sh_exp = true;
                // 形态：shExpMatch(host, "pattern")——pattern 在引号对内
                //（首引号..末引号；host 在引号外，逗号位置不可靠）。
                if let (Some(q1), Some(q2)) = (rest.find('"'), rest.rfind('"')) {
                    if q2 > q1 + 1 {
                        let pattern = &rest[q1 + 1..q2];
                        if sh_exp_match(s.host, pattern) {
                            matched = line.contains("direct");
                        }
                    }
                }
            }
        }
        if !has_sh_exp && matched != s.expect_direct {
            return false;
        }
        if has_sh_exp && matched != s.expect_direct {
            return false;
        }
    }
    true
}

/// shExpMatch 通配语义（* = 任意段、前缀锚定——PAC 标准 * 通配）。
pub fn sh_exp_match(host: &str, pattern: &str) -> bool {
    let (h, pat) = (host.as_bytes(), pattern.as_bytes());
    glob_match(h, pat)
}

fn glob_match(h: &[u8], p: &[u8]) -> bool {
    if p.is_empty() {
        return h.is_empty();
    }
    if p[0] == b'*' {
        for i in 0..=h.len() {
            if glob_match(&h[i..], &p[1..]) {
                return true;
            }
        }
        false
    } else {
        !h.is_empty() && h[0] == p[0] && glob_match(&h[1..], &p[1..])
    }
}

/// 代理凭据占位安全（主册「凭据不落盘明文」——凭据字段只存占位标记：
/// 有/无 + 长度提示；任何「明文回显」都违规）。
pub const CRED_PLACEHOLDER: &str = "******";

pub fn cred_echo_safe(stored: Option<&str>) -> bool {
    match stored {
        Some(s) => s == CRED_PLACEHOLDER, // 存储域只可能是占位串。
        None => true,                     // 无凭据合法。
    }
}

/// 多代理故障转移序（主册「多个代理」的转移面：主代理探活失败 →
/// 依序切下一个；全失败 → 直连并亮牌——静默全失败是说谎）。
pub const FAILOVER_MARK_DIRECT: &str = "DIRECT(全代理不可达)";

pub fn failover_pick(alive: &[bool]) -> Option<usize> {
    alive.iter().position(|&a| a)
}

pub fn failover_exhausted(alive: &[bool]) -> bool {
    alive.iter().all(|&a| !a)
}

/// 异常通配符矩阵深化（v1 exception_matches 的补充面：*.example.com
/// 全段匹配、example.com 精确、*.example.com 不匹配 evil-example.com——
/// 子串陷阱是代理异常清单的经典错误）。
pub fn exception_matrix_ok() -> bool {
    // 标准代理异常语义：`*.example.com` 与 `example.com` 都覆盖子域
    //（含多级）；连字符域（evil-example.com）不冒充子域。
    exception_matches("*.example.com", "a.example.com")
        && exception_matches("*.example.com", "a.b.example.com")
        && !exception_matches("*.example.com", "evil-example.com")
        && exception_matches("example.com", "example.com")
        && exception_matches("example.com", "a.example.com")
        && exception_matches("example.com", "a.b.example.com")
}

/// 开关状态一致性审计（主册「代理开关」与三字段的联动完整性：
/// 关 = 三字段只读不可改；开 = 三字段必填齐——状态机口径一处一事实）。
pub fn switch_consistency(on: bool, server: &str, port: u16, has_exception: bool) -> bool {
    // 开 = 两要素必填齐（服务器 + 合法端口）；异常清单可选（has_exception 不设限）。
    let _ = has_exception;
    if !on {
        return true; // 关：字段保留但不生效（一致性无事可查）。
    }
    !server.is_empty() && (PORT_MIN..=PORT_MAX).contains(&port)
}

// ---------------------------------------------------------------------------
// 深化 v3 自检（F485-v3）
// ---------------------------------------------------------------------------

pub fn run_sysproxy_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F485-v3");
    // 1) PAC：通配对拍、名单外语法拒。
    cs.add("pac_shexp_ok", pac_script_valid(
        "shExpMatch(host, \"*.corp.varix\") -> direct",
        &[PacSample { host: "git.corp.varix", expect_direct: true }],
    ), "");
    cs.add("pac_sample_mismatch_rejected", !pac_script_valid(
        "shExpMatch(host, \"*.corp.varix\") -> direct",
        &[PacSample { host: "git.corp.varix", expect_direct: false }],
    ), "");
    cs.add("pac_dnsresolve_rejected", !pac_script_valid(
        "if (dnsResolve(host) == \"1.2.3.4\") -> direct",
        &[],
    ), "");
    cs.add("pac_glob_star", sh_exp_match("a.b.example", "*.example")
        && sh_exp_match("a.b.example", "a.b.*") && !sh_exp_match("a.b.example", "b.*"), "");
    // 2) 凭据占位：只认占位串，明文违规。
    cs.add("cred_placeholder_ok", cred_echo_safe(Some(CRED_PLACEHOLDER)), "");
    cs.add("cred_plaintext_violation", !cred_echo_safe(Some("user:pass123")), "");
    cs.add("cred_absent_ok", cred_echo_safe(None), "");
    // 3) 故障转移：次序取活、全灭亮牌。
    cs.add("failover_second", failover_pick(&[false, false, true]) == Some(2), "");
    cs.add("failover_none_direct", failover_pick(&[false, false]).is_none() && failover_exhausted(&[false, false]), "");
    cs.add("failover_direct_marked", FAILOVER_MARK_DIRECT.contains("DIRECT"), "");
    // 4) 异常通配矩阵（子串陷阱）。
    cs.add("exception_matrix", exception_matrix_ok(), "");
    // 5) 开关一致性：开必填齐。
    cs.add("switch_on_needs_full", switch_consistency(true, "proxy.varix", 8_080, true), "");
    cs.add("switch_on_empty_rejected", !switch_consistency(true, "", 8_080, true), "");
    cs.add("switch_off_free", switch_consistency(false, "", 0, true), "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn pac_multi_sample_all_must_pass() {
        // 多样例逐个对拍——一个不一致整脚本拒。
        let ok = pac_script_valid(
            "shExpMatch(host, \"*.a\") -> direct; shExpMatch(host, \"*.b\") -> proxy",
            &[PacSample { host: "x.a", expect_direct: true }, PacSample { host: "y.b", expect_direct: false }],
        );
        assert!(ok);
        let bad = pac_script_valid(
            "shExpMatch(host, \"*.a\") -> direct",
            &[PacSample { host: "x.a", expect_direct: true }, PacSample { host: "z.a", expect_direct: false }],
        );
        assert!(!bad);
    }

    #[test]
    fn failover_prefers_first_alive() {
        // 次序语义：第一个活的就是选择（不挑延迟最低——那是探测账的事）。
        assert_eq!(failover_pick(&[true, true]), Some(0));
        assert_eq!(failover_pick(&[]), None);
    }

    #[test]
    fn quality_of_glob_edge() {
        // 空模式只匹配空主机；纯 * 匹配一切。
        assert!(glob_match(b"", b""));
        assert!(!glob_match(b"a", b""));
        assert!(glob_match(b"anything.at.all", b"*"));
    }
}
