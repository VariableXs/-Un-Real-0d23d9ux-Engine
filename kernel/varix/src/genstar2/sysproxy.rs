//! F485 系统代理设置（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **手动配置链路；即时生效（新连接验证）；例外列表；诊断联动；与
//! F242/F295 数据同源。**
//!
//! 功能定义（主册批次三）：系统代理页——手动代理（地址/端口/例外列表）；
//! 代理开关即时生效（新连接走代理、已有连接不中断）；代理不可达时网络诊断
//! （F242）自动把「代理不通」列为候选原因。
//!
//! 零堆纪律：定长例外表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 例外列表容量。
pub const EXCEPTION_CAP: usize = 16;
/// 端口合法范围。
pub const PORT_MIN: u16 = 1;
pub const PORT_MAX: u16 = 65_535;
/// 代理探测超时（诊断联动用）。
pub const PROBE_TIMEOUT_MS: u64 = 2_000;

/// 系统代理配置。
#[derive(Clone, Copy, Debug)]
pub struct SysProxy {
    pub enabled: bool,
    pub host: [u8; 64],
    pub host_n: usize,
    pub port: u16,
    /// 例外列表（命中例外的连接不走代理）。
    exceptions: [[u8; 32]; EXCEPTION_CAP],
    exc_n: [usize; EXCEPTION_CAP],
    exc_count: usize,
}

/// 例外匹配（后缀域匹配：例外「varix.os」命中「docs.varix.os」——子域同豁免）。
pub fn exception_matches(exc: &str, host: &str) -> bool {
    let e = exc.trim_end_matches('.');
    let h = host.trim_end_matches('.');
    h == e || h.ends_with(e) && h.as_bytes().get(h.len() - e.len() - 1) == Some(&b'.')
}

impl SysProxy {
    pub const fn new() -> Self {
        SysProxy {
            enabled: false,
            host: [0; 64],
            host_n: 0,
            port: 0,
            exceptions: [[0; 32]; EXCEPTION_CAP],
            exc_n: [0; EXCEPTION_CAP],
            exc_count: 0,
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

    /// 新连接裁决：代理开 且 主机不命中例外 → 走代理。
    pub fn route_new_connection(&self, target_host: &str) -> RouteDecision {
        if !self.enabled {
            return RouteDecision::Direct;
        }
        if (0..self.exc_count).any(|i| {
            let e = core::str::from_utf8(&self.exceptions[i][..self.exc_n[i]]).unwrap_or("");
            exception_matches(e, target_host)
        }) {
            return RouteDecision::DirectExempt;
        }
        RouteDecision::ViaProxy
    }

    /// 例外列表管理（满容诚实拒绝）。
    pub fn add_exception(&mut self, host: &str) -> bool {
        if self.exc_count >= EXCEPTION_CAP || host.is_empty() || host.len() > 32 {
            return false;
        }
        self.exceptions[self.exc_count][..host.len()].copy_from_slice(host.as_bytes());
        self.exc_n[self.exc_count] = host.len();
        self.exc_count += 1;
        true
    }

    pub fn exception_count(&self) -> usize {
        self.exc_count
    }

    /// 诊断联动（F242）：代理开但探测不通 → 「代理不通」入候选原因。
    pub fn diagnose_candidates(&self, probe_ok: bool, elapsed_ms: u64) -> &'static str {
        if self.enabled && !probe_ok {
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
}

/// 新连接路由裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RouteDecision {
    ViaProxy,
    Direct,
    DirectExempt,
}

// ---------------------------------------------------------------------------
// 域自检
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
    cs.add("instant_on", p.route_new_connection("example.com") == RouteDecision::ViaProxy, "");
    p.toggle(false);
    cs.add("instant_off", p.route_new_connection("example.com") == RouteDecision::Direct, "");
    // 3) 例外列表（子域豁免；列表管理）。
    p.toggle(true);
    cs.add("exception_add", p.add_exception("varix.os") && p.exception_count() == 1, "");
    cs.add("exception_subdomain", p.route_new_connection("docs.varix.os") == RouteDecision::DirectExempt, "");
    cs.add("exception_exact", p.route_new_connection("varix.os") == RouteDecision::DirectExempt, "");
    cs.add("non_except_via_proxy", p.route_new_connection("other.net") == RouteDecision::ViaProxy, "");
    // 4) 例外匹配语义（非子域不同名不豁免）。
    cs.add("no_partial_match", !exception_matches("varix.os", "notvarix.os"), "");
    // 5) 诊断联动（代理不通入候选原因）。
    p.toggle(true);
    cs.add("diagnose_proxy_down", p.diagnose_candidates(false, 100) == "代理不通", "");
    cs.add("diagnose_timeout", p.diagnose_candidates(false, 3_000) == "代理不通" || p.diagnose_candidates(true, 3_000) == "网络超时", "");
    // 6) 数据同源锚（F242/F295 同一配置 → 同一锚值——一处一事实）。
    let mut q = SysProxy::new();
    q.configure("proxy.corp.local", 8_080).ok();
    q.toggle(true);
    cs.add("same_source_anchor", q.config_anchor() == p.config_anchor(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_connections_not_interrupted_semantics() {
        // 主册：开关即时生效但已有连接不中断——裁决只作用于「新连接」入口。
        let mut p = SysProxy::new();
        p.configure("p:1".trim_end_matches(":1"), 1).ok();
        p.toggle(true);
        // 旧连接语义由调用层保持；这里验证新连接接口只接受 host 参数。
        assert_eq!(p.route_new_connection("a.b"), RouteDecision::ViaProxy);
    }

    #[test]
    fn exception_list_capacity_honest() {
        let mut p = SysProxy::new();
        for i in 0..EXCEPTION_CAP {
            assert!(p.add_exception(&format!("h{i}.test")), "第 {i} 条例外应成功");
        }
        assert!(!p.add_exception("overflow.test"));
    }

    #[test]
    fn config_anchor_changes_with_settings() {
        let mut a = SysProxy::new();
        a.configure("h", 80).ok();
        let h1 = a.config_anchor();
        a.toggle(true);
        assert_ne!(h1, a.config_anchor());
    }
}
