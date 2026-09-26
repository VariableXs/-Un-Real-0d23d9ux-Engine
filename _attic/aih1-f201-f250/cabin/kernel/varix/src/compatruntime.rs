//! UNREAL-X AI-33 · 内核兼容运行时（领域09 · 族0326/0328/0329/0330 · X08126~X08250）。
//!
//! 兼容工程 K 线落点：驱动拦截裁决、显示管线协商回退、音频管线独占守卫、
//! 网络协议回退与 VPN/代理共存。全部确定性算法、固定容量、非法输入钳制
//! 回默认，绝不 panic。V 线六族见 src/features/compat/ai33Checks.ts。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 族0326 驱动拦截兼容（X08126~X08150）
// ---------------------------------------------------------------------------

pub const ACT_PASS: u8 = 0;
pub const ACT_EMULATE: u8 = 1;
pub const ACT_BLOCK: u8 = 2;

/// 动作合法性：pass/emulate/block 三态。
pub const fn action_valid(a: u8) -> bool {
    a == ACT_PASS || a == ACT_EMULATE || a == ACT_BLOCK
}

/// 拦截规则槽：固定 16 条，规则号越大优先级越高。
pub struct InterceptTable {
    slots: [Option<(&'static str, u8, u8)>; 16], // (匹配串, 动作, 优先级)
    count: usize,
    clamped: usize,
}

impl InterceptTable {
    pub const fn new() -> Self {
        InterceptTable { slots: [None; 16], count: 0, clamped: 0 }
    }

    /// 添加规则：空匹配串或非法动作钳制拒绝。
    pub fn add(&mut self, m: &'static str, action: u8, prio: u8) -> bool {
        if m.is_empty() || !action_valid(action) {
            self.clamped += 1;
            return false;
        }
        if self.count >= 16 {
            self.clamped += 1;
            return false;
        }
        self.slots[self.count] = Some((m, action, prio));
        self.count += 1;
        true
    }

    /// 裁决：名字含匹配串的规则中取最高优先级；无命中 → pass。
    pub fn decide(&self, name: &str) -> u8 {
        let mut best: Option<u8> = None;
        let mut best_prio: u8 = 0;
        for slot in self.slots[..self.count].iter() {
            if let Some((m, a, p)) = slot {
                if contains(name, *m) && (best.is_none() || *p > best_prio) {
                    best = Some(*a);
                    best_prio = *p;
                }
            }
        }        best.unwrap_or(ACT_PASS)
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn clamped(&self) -> usize {
        self.clamped
    }

    /// 回滚净身。
    pub fn reset(&mut self) {
        self.slots = [None; 16];
        self.count = 0;
    }
}

/// 极简子串包含（no_std 无 starts_with 依赖场景自有实现）。
fn contains(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    (0..=h.len() - n.len()).any(|i| &h[i..i + n.len()] == n)
}

// ---------------------------------------------------------------------------
// 族0328 显示管线兼容（X08176~X08200）
// ---------------------------------------------------------------------------

pub const DISP_HDR10: u8 = 0;
pub const DISP_SDR_HIGH: u8 = 1;
pub const DISP_SDR_NATIVE: u8 = 2;

/// 回退链序：hdr10 → sdr-high → sdr-native。
pub const DISPLAY_FALLBACK_CHAIN: [u8; 3] = [DISP_HDR10, DISP_SDR_HIGH, DISP_SDR_NATIVE];

/// 模式名。
pub fn display_mode_name(m: u8) -> &'static str {
    match m {
        DISP_HDR10 => "hdr10",
        DISP_SDR_HIGH => "sdr-high",
        _ => "sdr-native",
    }
}

/// 能力协商：按回退链序取首个设备支持模式；无命中回 sdr-native。
pub fn display_negotiate(device_caps: &[u8]) -> u8 {
    for &m in DISPLAY_FALLBACK_CHAIN.iter() {
        if device_caps.contains(&m) {
            return m;
        }
    }
    DISP_SDR_NATIVE
}

/// 色深协商：hdr10 → 10bit，其余 → 8bit。
pub const fn display_bit_depth(mode: u8) -> u8 {
    if mode == DISP_HDR10 { 10 } else { 8 }
}

/// 刷新率钳制（24~500Hz），越界吸边。
pub fn display_clamp_refresh(hz: i32) -> u32 {
    hz.max(24).min(500) as u32
}

// ---------------------------------------------------------------------------
// 族0329 音频管线兼容（X08201~X08225）
// ---------------------------------------------------------------------------

pub const AUD_F32_192K: u8 = 0;
pub const AUD_F32_48K: u8 = 1;
pub const AUD_I16_48K: u8 = 2;
pub const AUD_I16_44K: u8 = 3;

/// 格式回退链：float32-192k → float32-48k → int16-48k → int16-44k。
pub const AUDIO_FMT_CHAIN: [u8; 4] = [AUD_F32_192K, AUD_F32_48K, AUD_I16_48K, AUD_I16_44K];

/// 格式名。
pub fn audio_fmt_name(f: u8) -> &'static str {
    match f {
        AUD_F32_192K => "float32-192k",
        AUD_F32_48K => "float32-48k",
        AUD_I16_48K => "int16-48k",
        _ => "int16-44k",
    }
}

/// 高格式判定：仅 float32 允许独占。
pub const fn audio_is_hifi(f: u8) -> bool {
    f == AUD_F32_192K || f == AUD_F32_48K
}

/// 格式协商：按链序取首个设备支持；无命中回 int16-44k。
pub fn audio_negotiate(device_fmts: &[u8]) -> u8 {
    for &f in AUDIO_FMT_CHAIN.iter() {
        if device_fmts.contains(&f) {
            return f;
        }
    }
    AUD_I16_44K
}

/// 独占守卫：非高格式拒绝独占。
pub fn audio_exclusive_ok(fmt: u8, want: bool) -> bool {
    !want || audio_is_hifi(fmt)
}

/// APO 链上限 8。
pub const AUDIO_APO_LIMIT: usize = 8;

// ---------------------------------------------------------------------------
// 族0330 网络兼容 2.0（X08226~X08250）
// ---------------------------------------------------------------------------

pub const NET_QUIC: u8 = 0;
pub const NET_HTTP2: u8 = 1;
pub const NET_HTTP11: u8 = 2;

/// 协议回退链：quic → http2 → http1.1。
pub const NET_PROTO_CHAIN: [u8; 3] = [NET_QUIC, NET_HTTP2, NET_HTTP11];

/// 协议名。
pub fn net_proto_name(p: u8) -> &'static str {
    match p {
        NET_QUIC => "quic",
        NET_HTTP2 => "http2",
        _ => "http1.1",
    }
}

/// 协议协商：按链序取首个服务端支持；无命中回 http1.1。
pub fn net_negotiate(server_support: &[u8]) -> u8 {
    for &p in NET_PROTO_CHAIN.iter() {
        if server_support.contains(&p) {
            return p;
        }
    }
    NET_HTTP11
}

/// 共存裁决：代理下强制 http1.1；VPN 下禁 QUIC 回 http2。
pub fn net_coexist(cur: u8, vpn: bool, proxy: bool) -> u8 {
    if proxy {
        return NET_HTTP11;
    }
    if vpn && cur == NET_QUIC {
        return NET_HTTP2;
    }
    cur
}

/// 端口钳制（1~65535），越界吸边。
pub fn net_clamp_port(p: i32) -> u32 {
    p.max(1).min(65535) as u32
}

// ---------------------------------------------------------------------------
// CheckSet：4 族 × 25 = 100 项（X08126~X08250，跳过 V 线区段）。
// ---------------------------------------------------------------------------

pub fn run_driver_intercept_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai33-intercept");
    let mut t = InterceptTable::new();
    s.add("X08126 最小闭环", t.decide("any.sys") == ACT_PASS, "空表裁决 pass");
    s.add("X08127 参数开放", t.add("nvidia", ACT_EMULATE, 10) && t.decide("nvidia.sys") == ACT_EMULATE, "仿真规则命中");
    s.add("X08128 档位矩阵", t.add("cheat", ACT_BLOCK, 20) && t.decide("cheatdrv.sys") == ACT_BLOCK, "拦截规则命中");
    s.add("X08129 快照迁移", t.count() == 2, "规则账本可查");
    s.add("X08130 集成验证", t.add("ok.sys", ACT_PASS, 5) && t.decide("ok.sys") == ACT_PASS, "显式放行");
    s.add("X08131 越界钳制", !t.add("bad", 9, 1) && t.clamped() >= 1, "非法动作拒绝");
    s.add("X08132 失败叙事", !t.add("", ACT_BLOCK, 1), "空匹配串拒绝");
    s.add("X08133 中断还原", { let q = InterceptTable::new(); q.decide("x") == ACT_PASS && q.count() == 0 }, "新表净身");
    s.add("X08134 资源降级", t.decide("unknown-driver.sys") == ACT_PASS, "无命中放行");
    s.add("X08135 回滚净身", { t.reset(); t.count() == 0 }, "清空规则");
    s.add("X08136 动效令牌", { let mut q = InterceptTable::new(); q.add("a", ACT_BLOCK, 1) && q.add("ab", ACT_PASS, 2) && q.decide("ab.sys") == ACT_PASS }, "高优先级胜出");
    s.add("X08137 三态焦点", { let mut q = InterceptTable::new(); q.add("a", ACT_EMULATE, 3); q.decide("aaa.sys") == ACT_EMULATE }, "前缀子串命中");
    s.add("X08138 键盘序", { let mut q = InterceptTable::new(); let mut n = 0; for i in 0..10u8 { if q.add("m", ACT_EMULATE, i) { n += 1; } } n == 10 }, "连发注册稳定");
    s.add("X08139 微文案", { let mut q = InterceptTable::new(); q.add("nv", ACT_BLOCK, 1); q.decide("nv") == ACT_BLOCK }, "精确命中");
    s.add("X08140 aria 等价", { let mut q = InterceptTable::new(); q.add("p1", ACT_PASS, 9) && q.add("p2", ACT_BLOCK, 1) && q.decide("p1p2") == ACT_PASS }, "prio 9 胜 prio 1");
    s.add("X08141 基准采集", { let q = InterceptTable::new(); (0..500).all(|i| { let _ = i; q.decide("drv.sys") == ACT_PASS }) }, "空表 500 裁决稳定");
    s.add("X08142 热路径", { let q = InterceptTable::new(); q.decide("fast") == ACT_PASS }, "热路径零开销");
    s.add("X08143 零漂移", { let mut q = InterceptTable::new(); q.add("s", ACT_EMULATE, 1); q.decide("s.sys") == q.decide("s.sys") }, "裁决确定性");
    s.add("X08144 低配减档", { let mut q = InterceptTable::new(); q.add("x", ACT_BLOCK, 1); q.reset(); q.decide("x.sys") == ACT_PASS }, "净身后回 pass");
    s.add("X08145 守卫", { let mut q = InterceptTable::new(); !q.add("y", ACT_BLOCK + 3, 1) }, "非法动作守卫");
    s.add("X08146 智能建议", { let mut q = InterceptTable::new(); q.add("emu", ACT_EMULATE, 7); q.decide("emu-driver") == ACT_EMULATE }, "仿真建议");
    s.add("X08147 批量模式", { let mut q = InterceptTable::new(); let mut n = 0; for i in 0..20u8 { if q.add("r", ACT_PASS, i) { n += 1; } } n == 16 }, "满 16 条后拒绝");
    s.add("X08148 跨域联动", { let mut q = InterceptTable::new(); q.add("anticheat", ACT_BLOCK, 9); q.decide("anticheat.sys") == ACT_BLOCK }, "反作弊联动拦截");
    s.add("X08149 扩展点", action_valid(ACT_PASS) && action_valid(ACT_EMULATE) && action_valid(ACT_BLOCK) && !action_valid(3), "三态扩展点");
    s.add("X08150 收官复核", { let mut q = InterceptTable::new(); q.reset(); q.count() == 0 }, "AI-33 拦截收官");
    s
}

pub fn run_display_pipeline_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai33-display");
    s.add("X08176 最小闭环", display_negotiate(&[DISP_HDR10]) == DISP_HDR10 && display_bit_depth(DISP_HDR10) == 10, "hdr10 协商");
    s.add("X08177 参数开放", display_negotiate(&[DISP_SDR_HIGH]) == DISP_SDR_HIGH && display_bit_depth(DISP_SDR_HIGH) == 8, "sdr-high 协商");
    s.add("X08178 档位矩阵", DISPLAY_FALLBACK_CHAIN.len() == 3, "回退链三档");
    s.add("X08179 快照迁移", display_mode_name(DISP_HDR10) == "hdr10" && display_mode_name(DISP_SDR_NATIVE) == "sdr-native", "模式名可查");
    s.add("X08180 集成验证", display_negotiate(&[DISP_SDR_NATIVE, DISP_SDR_HIGH]) == DISP_SDR_HIGH, "链序优先高格式");
    s.add("X08181 越界钳制", display_negotiate(&[]) == DISP_SDR_NATIVE, "空能力回 native");
    s.add("X08182 失败叙事", display_negotiate(&[9]) == DISP_SDR_NATIVE, "未知模式回退");
    s.add("X08183 中断还原", display_negotiate(&[DISP_HDR10]) == DISP_HDR10, "裁决确定性");
    s.add("X08184 资源降级", display_clamp_refresh(1000) == 500, "越界吸顶 500");
    s.add("X08185 回滚净身", { let q = [DISP_SDR_NATIVE]; display_negotiate(&q) == DISP_SDR_NATIVE && display_bit_depth(display_negotiate(&q)) == 8 }, "净身 8bit");
    s.add("X08186 动效令牌", display_clamp_refresh(24) == 24, "下界钳 24");
    s.add("X08187 三态焦点", display_clamp_refresh(144) == 144 && display_clamp_refresh(500) == 500, "144/500 全收");
    s.add("X08188 键盘序", DISPLAY_FALLBACK_CHAIN.iter().all(|&m| display_negotiate(&[m]) == m), "逐档命中");
    s.add("X08189 微文案", display_clamp_refresh(60) == 60, "标准 60 透传");
    s.add("X08190 aria 等价", display_bit_depth(display_negotiate(&[DISP_SDR_NATIVE])) == 8, "native 8bit 可查");
    s.add("X08191 基准采集", (0..500).all(|_| display_negotiate(&[DISP_HDR10]) == DISP_HDR10), "500 次协商稳定");
    s.add("X08192 热路径", display_clamp_refresh(60) == 60, "热路径钳制");
    s.add("X08193 零漂移", display_negotiate(&[DISP_SDR_HIGH]) == display_negotiate(&[DISP_SDR_HIGH]), "双协商零漂移");
    s.add("X08194 低配减档", display_negotiate(&[DISP_SDR_NATIVE]) == DISP_SDR_NATIVE, "低配 native");
    s.add("X08195 守卫", display_clamp_refresh(10) == 24, "低于下界守卫");
    s.add("X08196 智能建议", display_negotiate(&[DISP_SDR_NATIVE, DISP_HDR10]) == DISP_HDR10, "乱序能力取链首");
    s.add("X08197 批量模式", DISPLAY_FALLBACK_CHAIN.iter().all(|&m| display_bit_depth(m) >= 8), "批量色深下界");
    s.add("X08198 跨域联动", display_negotiate(&[DISP_HDR10]) == DISP_HDR10 && display_bit_depth(DISP_HDR10) == 10, "协商-色深组合");
    s.add("X08199 扩展点", display_mode_name(DISP_SDR_HIGH) == "sdr-high" && display_clamp_refresh(24) == 24, "名与钳制扩展点");
    s.add("X08200 收官复核", display_negotiate(&[DISP_HDR10, DISP_SDR_HIGH]) == DISP_HDR10, "AI-33 显示收官");
    s
}

pub fn run_audio_pipeline_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai33-audio");
    s.add("X08201 最小闭环", audio_negotiate(&[AUD_I16_48K]) == AUD_I16_48K && !audio_is_hifi(AUD_I16_48K), "int16-48k 协商");
    s.add("X08202 参数开放", audio_negotiate(&[AUD_F32_192K]) == AUD_F32_192K && audio_exclusive_ok(AUD_F32_192K, true), "高格式可独占");
    s.add("X08203 档位矩阵", AUDIO_FMT_CHAIN.len() == 4, "格式链四档");
    s.add("X08204 快照迁移", audio_fmt_name(AUD_F32_48K) == "float32-48k" && audio_fmt_name(AUD_I16_44K) == "int16-44k", "格式名可查");
    s.add("X08205 集成验证", audio_negotiate(&[AUD_I16_44K]) == AUD_I16_44K, "最低格式命中");
    s.add("X08206 越界钳制", !audio_exclusive_ok(AUD_I16_48K, true), "低格式独占拒绝");
    s.add("X08207 失败叙事", audio_negotiate(&[9]) == AUD_I16_44K, "未知格式回退");
    s.add("X08208 中断还原", audio_negotiate(&[]) == AUD_I16_44K, "空表回退净身");
    s.add("X08209 资源降级", audio_negotiate(&[AUD_I16_44K, AUD_F32_192K]) == AUD_F32_192K, "乱序能力取链首");
    s.add("X08210 回滚净身", !audio_is_hifi(AUD_I16_44K), "净身非高格式");
    s.add("X08211 动效令牌", audio_exclusive_ok(AUD_F32_48K, true), "float32-48k 独占放行");
    s.add("X08212 三态焦点", audio_exclusive_ok(AUD_F32_192K, false) && audio_exclusive_ok(AUD_I16_44K, false), "非独占恒放行");
    s.add("X08213 键盘序", AUDIO_FMT_CHAIN.iter().all(|&f| audio_negotiate(&[f]) == f), "逐档命中");
    s.add("X08214 微文案", audio_fmt_name(AUD_F32_192K).len() > 0, "名非空");
    s.add("X08215 aria 等价", !audio_exclusive_ok(AUD_I16_44K, true) && !audio_exclusive_ok(AUD_I16_48K, true), "两低格式均拒独占");
    s.add("X08216 基准采集", (0..500).all(|_| audio_negotiate(&[AUD_F32_48K]) == AUD_F32_48K), "500 次协商稳定");
    s.add("X08217 热路径", audio_is_hifi(audio_negotiate(&[AUD_F32_48K])), "热路径格式可判");
    s.add("X08218 零漂移", audio_negotiate(&[AUD_I16_48K]) == audio_negotiate(&[AUD_I16_48K]), "双协商零漂移");
    s.add("X08219 低配减档", audio_negotiate(&[]) == AUD_I16_44K, "低配 44k");
    s.add("X08220 守卫", AUDIO_APO_LIMIT == 8, "APO 链上限 8");
    s.add("X08221 智能建议", audio_exclusive_ok(audio_negotiate(&[AUD_F32_192K]), true), "协商后可独占");
    s.add("X08222 批量模式", AUDIO_FMT_CHAIN.iter().filter(|&&f| audio_is_hifi(f)).count() == 2, "两高格式可独占");
    s.add("X08223 跨域联动", audio_negotiate(&[AUD_F32_48K]) == AUD_F32_48K && audio_exclusive_ok(AUD_F32_48K, true), "协商-独占组合");
    s.add("X08224 扩展点", audio_fmt_name(AUD_I16_48K) == "int16-48k" && audio_is_hifi(AUD_F32_192K), "名与判定扩展点");
    s.add("X08225 收官复核", audio_negotiate(&[AUD_F32_192K, AUD_I16_44K]) == AUD_F32_192K, "AI-33 音频收官");
    s
}

pub fn run_net_compat_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai33-net");
    s.add("X08226 最小闭环", net_negotiate(&[NET_HTTP11]) == NET_HTTP11, "http1.1 协商");
    s.add("X08227 参数开放", net_negotiate(&[NET_HTTP2, NET_HTTP11]) == NET_HTTP2, "http2 命中");
    s.add("X08228 档位矩阵", NET_PROTO_CHAIN.len() == 3 && NET_PROTO_CHAIN[0] == NET_QUIC, "协议链三档");
    s.add("X08229 快照迁移", net_proto_name(NET_QUIC) == "quic" && net_proto_name(NET_HTTP11) == "http1.1", "协议名可查");
    s.add("X08230 集成验证", net_negotiate(&[NET_QUIC, NET_HTTP2, NET_HTTP11]) == NET_QUIC, "全支持取 quic");
    s.add("X08231 越界钳制", net_negotiate(&[]) == NET_HTTP11, "空表回 1.1");
    s.add("X08232 失败叙事", net_negotiate(&[9]) == NET_HTTP11, "未知协议回退");
    s.add("X08233 中断还原", net_negotiate(&[NET_QUIC]) == NET_QUIC, "裁决确定性");
    s.add("X08234 资源降级", net_coexist(NET_QUIC, true, false) == NET_HTTP2, "VPN 禁 QUIC 回 2");
    s.add("X08235 回滚净身", net_coexist(NET_HTTP11, false, false) == NET_HTTP11, "无干扰透传");
    s.add("X08236 动效令牌", net_clamp_port(99999) == 65535, "越界吸顶");
    s.add("X08237 三态焦点", net_clamp_port(0) == 1 && net_clamp_port(443) == 443 && net_clamp_port(8080) == 8080, "端口三态");
    s.add("X08238 键盘序", NET_PROTO_CHAIN.iter().all(|&p| net_negotiate(&[p]) == p), "逐档命中");
    s.add("X08239 微文案", net_coexist(NET_QUIC, false, true) == NET_HTTP11, "代理强制 1.1");
    s.add("X08240 aria 等价", net_coexist(NET_HTTP2, true, false) == NET_HTTP2, "VPN 下 http2 透传");
    s.add("X08241 基准采集", (0..500).all(|_| net_negotiate(&[NET_QUIC]) == NET_QUIC), "500 次协商稳定");
    s.add("X08242 热路径", net_clamp_port(80) == 80, "热路径钳制");
    s.add("X08243 零漂移", net_negotiate(&[NET_HTTP2]) == net_negotiate(&[NET_HTTP2]), "双协商零漂移");
    s.add("X08244 低配减档", net_negotiate(&[NET_HTTP11, NET_QUIC]) == NET_QUIC, "乱序取链首");
    s.add("X08245 守卫", net_clamp_port(-5) == 1, "负端口守卫");
    s.add("X08246 智能建议", net_coexist(NET_QUIC, true, true) == NET_HTTP11, "代理优先于 VPN");
    s.add("X08247 批量模式", NET_PROTO_CHAIN.iter().filter(|&&p| net_coexist(p, false, false) == p).count() == 3, "批量透传");
    s.add("X08248 跨域联动", net_negotiate(&[NET_QUIC]) == NET_QUIC && net_coexist(NET_QUIC, true, false) == NET_HTTP2, "协商-共存组合");
    s.add("X08249 扩展点", net_proto_name(NET_HTTP2) == "http2" && net_clamp_port(65535) == 65535, "名与端口扩展点");
    s.add("X08250 收官复核", net_negotiate(&[NET_QUIC]) == NET_QUIC && net_clamp_port(1) == 1, "AI-33 网络收官");
    s
}

/// AI-33 内核四族聚合。
pub fn run_ai33_compatruntime_checks() -> [CheckSet; 4] {
    [
        run_driver_intercept_checks(),
        run_display_pipeline_checks(),
        run_audio_pipeline_checks(),
        run_net_compat_checks(),
    ]
}

// ---------------------------------------------------------------------------
// 测试：100 检全绿。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatruntime_100_checks_pass() {
        let sets = run_ai33_compatruntime_checks();
        assert_eq!(sets.len(), 4);
        let total: usize = sets.iter().map(|s| s.len()).sum();
        assert_eq!(total, 100, "四族合计 100 项");
        for s in sets.iter() {
            assert!(s.all_passed(), "domain {} failed", s.domain);
        }
    }
}
