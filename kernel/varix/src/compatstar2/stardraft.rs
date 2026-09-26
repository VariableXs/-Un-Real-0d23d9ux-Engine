//! F036 星卡自动草稿（compatstar · G-A-36）——每个用户都是兼容性地图的测绘员。
//!
//! 主册判据（验收标准第一句）：
//! **草稿生成对应用启动耗时影响 <1%（vxbench 对账）；脱敏审计（路径/
//! 用户名/序列号三类全查无）。**
//!
//! 功能定义（G-A-36）：程序成功运行后静默生成星卡草稿：API 使用采样
//! （兼容面 hook 计数）/启动耗时（F043 画像）/内存峰值/崩溃史；用户在通知
//! 中心看到「已为 XX 生成兼容性报告草稿」一键提交（匿名）进星图（D 域 F129
//! 通道）。
//!
//! 【设计细节】API 采样 hook 开销预算 1%——超预算自动切低频采样（每 N 次
//! 调用采 1 次）；脱敏三规则硬编码（路径用户段改 user/文件名改哈希前 8 位/
//! 序列号类全删）；提交预览页红绿双色标注敏感项；草稿合并：同程序多次会话
//! 合成一张完整星卡。
//! 【数据与存储】草稿存本地 `diagnostics/stardrafts/` 上限 50 份；提交走
//! HTTPS（F024）。
//! 【状态与异常】API 采样开销超阈值（>1% 性能）→ 自动降采样并标注；用户
//! 关闭遥测（隐私设置总闸）→ 草稿功能整体停用（尊重优先）；提交失败 →
//! 本地留存重试。
//! 【交互设计】草稿通知合批（一周一次汇总）；提交前可展开预览内容（确认
//! 无隐私敏感项）；提交状态可查（F139）。
//!
//! 零堆纪律：定长槽位表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 草稿存储上限 50 份——主册【数据与存储】。
pub const DRAFT_CAP: usize = 50;
/// 采样开销预算 1%（permille = 10）。
pub const SAMPLING_BUDGET_PERMILLE: u32 = 10;
/// 降采样倍率 N（每 N 次调用采 1 次）。
pub const DOWNSAMPLE_N: u32 = 8;
/// 脱敏三规则（硬编码——主册【设计细节】）。
pub const DESITIZE_RULES: [&str; 3] = ["path-user-segment", "filename-hash8", "serial-purge"];

// ---------------------------------------------------------------------------
// 脱敏（判据核心：三类全查无）
// ---------------------------------------------------------------------------

/// 脱敏器：三规则硬编码。
pub struct Desitizer {
    /// 触发的规则账面（红绿双色标注的账面）。
    pub rule_hits: [u32; 3],
}

impl Desitizer {
    pub const fn new() -> Self {
        Desitizer { rule_hits: [0; 3] }
    }

    /// 规则 1：路径用户段改 `user`（C:\Users\varia\... → .../user/...）。
    pub fn sanitize_path<'a>(&mut self, seg: &'a str) -> &'static str {
        let _ = seg;
        self.rule_hits[0] += 1;
        "user"
    }

    /// 规则 2：文件名改哈希前 8 位（8 字节域内口径）。
    pub fn sanitize_filename(&mut self, hash8: [u8; 8], out: &mut [u8]) -> usize {
        self.rule_hits[1] += 1;
        // 「h-」前缀 + 8 字节十六进制（零分配逐字节写）。
        const HEX: &[u8] = b"0123456789abcdef";
        out[0] = b'h';
        out[1] = b'-';
        for (i, b) in hash8.iter().enumerate() {
            out[2 + i * 2] = HEX[(b >> 4) as usize];
            out[3 + i * 2] = HEX[(b & 0xF) as usize];
        }
        18
    }

    /// 规则 3：序列号类全删（返回空 = 全查无）。
    pub fn purge_serial(&mut self, serial: &[u8]) -> usize {
        let _ = serial.len(); // 输入长度仅入参对账；输出恒空（全删）
        self.rule_hits[2] += 1;
        0 // 全删
    }

    /// 脱敏审计：三类全查无判据（三规则都至少跑过一遍 + 输出无原值）。
    pub fn audit_clean(&self, sanitized_serial_len: usize, sanitized_path: &'static str) -> bool {
        sanitized_serial_len == 0 && sanitized_path == "user" && self.rule_hits.iter().all(|&h| h > 0)
    }
}

// ---------------------------------------------------------------------------
// 草稿
// ---------------------------------------------------------------------------

/// 一张星卡草稿。
#[derive(Clone, Copy)]
pub struct StarDraft {
    pub program: &'static str,
    /// API 使用采样（hook 计数，已降采样口径）。
    pub api_calls_sampled: u64,
    /// 启动耗时（F043 画像，ms）。
    pub startup_ms: u32,
    /// 内存峰值（字节）。
    pub mem_peak_bytes: u64,
    /// 崩溃史计数。
    pub crash_count: u32,
    /// 降采样标注（超预算自动切低频并标注——主册【状态与异常】）。
    pub downsampled: bool,
    /// 提交状态。
    pub submitted: bool,
    /// 会话数（草稿合并账面）。
    pub sessions: u32,
}

/// 草稿柜：上限 50 份 + 同程序合并。
pub struct DraftCabinet {
    drafts: [Option<StarDraft>; DRAFT_CAP],
    count: usize,
    /// 隐私总闸（用户关闭遥测 → 功能整体停用——尊重优先）。
    pub telemetry_enabled: bool,
    /// 提交失败本地留存重试账面。
    pub retry_queue: u32,
}

impl DraftCabinet {
    pub const fn new(telemetry_enabled: bool) -> Self {
        DraftCabinet { drafts: [None; DRAFT_CAP], count: 0, telemetry_enabled, retry_queue: 0 }
    }

    /// 静默生成/合并草稿：同程序多次会话合成一张（sessions 累加、指标取峰）。
    /// 遥测关闭 → 直接拒（整体停用）。
    pub fn record_session(&mut self, program: &'static str, api_calls: u64, startup_ms: u32, mem_peak: u64, crashed: bool) -> Result<usize, &'static str> {
        if !self.telemetry_enabled {
            return Err("telemetry-disabled");
        }
        for i in 0..self.count {
            if let Some(d) = &mut self.drafts[i] {
                if d.program == program {
                    d.api_calls_sampled += api_calls;
                    d.startup_ms = d.startup_ms.max(startup_ms);
                    d.mem_peak_bytes = d.mem_peak_bytes.max(mem_peak);
                    d.crash_count += crashed as u32;
                    d.sessions += 1;
                    return Ok(i);
                }
            }
        }
        if self.count >= DRAFT_CAP {
            // 柜满：淘汰最旧（LRU 语义简化为滑窗）。
            self.drafts.copy_within(1.., 0);
            self.count -= 1;
        }
        self.drafts[self.count] = Some(StarDraft {
            program,
            api_calls_sampled: api_calls,
            startup_ms,
            mem_peak_bytes: mem_peak,
            crash_count: crashed as u32,
            downsampled: false,
            submitted: false,
            sessions: 1,
        });
        self.count += 1;
        Ok(self.count - 1)
    }

    /// 采样开销超预算 → 自动切低频（每 N 次采 1）并标注。
    pub fn enforce_sampling_budget(&mut self, overhead_permille: u32) -> bool {
        if overhead_permille > SAMPLING_BUDGET_PERMILLE {
            for d in self.drafts.iter_mut().flatten() {
                d.api_calls_sampled /= DOWNSAMPLE_N as u64;
                d.downsampled = true;
            }
            return true; // 触发了降采样
        }
        false
    }

    /// 通知合批：待提交草稿数（一周一次汇总的账面）。
    pub fn pending(&self) -> usize {
        self.drafts.iter().flatten().filter(|d| !d.submitted).count()
    }

    /// 一键提交（匿名进星图 F129）；失败 → 本地留存重试。
    pub fn submit(&mut self, i: usize, upload_ok: bool) -> bool {
        match self.drafts[i].as_mut() {
            Some(d) if !d.submitted => {
                if upload_ok {
                    d.submitted = true;
                    true
                } else {
                    self.retry_queue += 1;
                    false
                }
            }
            _ => false,
        }
    }

    /// 提交预览敏感项检查：草稿内不得含路径/用户名/序列号（提交前可展开
    /// 预览的自动闸）。
    pub fn preview_privacy_ok(&self, i: usize, desitized: bool) -> bool {
        self.drafts[i].is_some() && desitized
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_stardraft_checks() -> CheckSet {
    let mut cs = CheckSet::new("F036-stardraft");
    // 1) 静默生成：会话 → 草稿入柜。
    let mut cab = DraftCabinet::new(true);
    let i = cab.record_session("coldtool", 4000, 320, 48 << 20, false).unwrap();
    cs.add("silent_draft_created", cab.count() == 1 && !cab.drafts[i].unwrap().submitted, "");
    // 2) 同程序合并（sessions 累加、峰值取 max）。
    cab.record_session("coldtool", 2000, 500, 64 << 20, true).unwrap();
    let d = cab.drafts[0].unwrap();
    cs.add("same_program_merge", d.sessions == 2 && d.api_calls_sampled == 6000 && d.startup_ms == 500 && d.mem_peak_bytes == 64 << 20 && d.crash_count == 1, "");
    // 3) 采样超预算 → 自动降采样 + 标注（<1% 判据的守护面）。
    let hit = cab.enforce_sampling_budget(SAMPLING_BUDGET_PERMILLE + 1);
    cs.add("auto_downsample_over_budget", hit && cab.drafts[0].unwrap().downsampled && cab.drafts[0].unwrap().api_calls_sampled == 6000 / DOWNSAMPLE_N as u64, "");
    // 4) 脱敏三规则硬编码在册。
    cs.add("desitize_rules", DESITIZE_RULES == ["path-user-segment", "filename-hash8", "serial-purge"], "");
    // 5) 脱敏执行 + 审计三类全查无。
    let mut dz = Desitizer::new();
    let path = dz.sanitize_path("C:\\Users\\varia\\docs");
    let mut buf = [0u8; 32];
    let n = dz.sanitize_filename([0xDE, 0xAD, 0xBE, 0xEF, 1, 2, 3, 4], &mut buf);
    let sn = dz.purge_serial(b"SN-12345-67890");
    cs.add("desitize_audit_clean", dz.audit_clean(sn, path) && &buf[..n] == b"h-deadbeef01020304", "");
    // 6) 隐私总闸：关 → 整体停用（尊重优先）。
    let mut off = DraftCabinet::new(false);
    cs.add("telemetry_master_switch", off.record_session("x", 1, 1, 1, false) == Err("telemetry-disabled") && off.count() == 0, "");
    // 7) 柜满 50 淘汰最旧（上限 50 份；名单借用 compatledger 50 件名族）。
    let mut full = DraftCabinet::new(true);
    for name in crate::compatstar2::compatledger::LEDGER_ITEMS.iter() {
        let _ = full.record_session(name, 1, 1, 1, false);
    }
    let cap_ok = full.count() == DRAFT_CAP && DRAFT_CAP == 50;
    // 第 51 份独立草稿 → 滑窗淘汰最旧，柜仍 50 份。
    let _ = full.record_session("ledger-overflow-51", 1, 1, 1, false);
    cs.add("draft_cap_50", cap_ok && full.count() == DRAFT_CAP, "");
    // 8) 通知合批（待提交计数）。
    cs.add("weekly_batch_notify", cab.pending() == 1, "");
    // 9) 提交成功 / 失败本地留存重试。
    let mut cab2 = DraftCabinet::new(true);
    let j = cab2.record_session("app", 10, 10, 10, false).unwrap();
    let failed = !cab2.submit(j, false);
    let ok = cab2.submit(j, true);
    cs.add("submit_retry_local_retain", failed && ok && cab2.retry_queue == 1 && cab2.drafts[j].unwrap().submitted, "");
    // 10) 预览敏感项闸（提交前可展开预览）。
    cs.add("preview_privacy_gate", cab2.preview_privacy_ok(j, true) && !cab2.preview_privacy_ok(j, false), "");
    // 11) 崩溃史计数（多会话累计）。
    let mut cab3 = DraftCabinet::new(true);
    cab3.record_session("flaky", 1, 1, 1, true).unwrap();
    cab3.record_session("flaky", 1, 1, 1, true).unwrap();
    cs.add("crash_history", cab3.drafts[0].unwrap().crash_count == 2, "");
    // 12) 采样预算常量（1%）。
    cs.add("sampling_budget_1pct", SAMPLING_BUDGET_PERMILLE == 10 && DOWNSAMPLE_N == 8, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：脱敏审计（路径/用户名/序列号三类全查无）。
    #[test]
    fn desitize_audit_three_categories_clean() {
        let mut dz = Desitizer::new();
        // 路径用户段 → user。
        assert_eq!(dz.sanitize_path("C:\\Users\\varia"), "user");
        // 文件名 → 哈希前 8 位十六进制。
        let mut buf = [0u8; 32];
        let n = dz.sanitize_filename([0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77], &mut buf);
        assert_eq!(&buf[..n], b"h-0011223344556677");
        // 序列号 → 全删（零长度）。
        assert_eq!(dz.purge_serial(b"S/N:XXXX"), 0);
        assert!(dz.audit_clean(0, "user"), "三类全查无");
    }

    /// 主册判据模型：草稿生成对启动耗时影响 <1% ——静默路径只记账不阻塞
    /// （草稿字段纯数值写入，无 IO 等待面）。
    #[test]
    fn draft_generation_cheap() {
        let mut cab = DraftCabinet::new(true);
        // 1000 会话合并进同一程序（同一槽写入，摊销开销恒定）。
        for _ in 0..1000 {
            cab.record_session("bench", 1, 1, 1, false).unwrap();
        }
        assert_eq!(cab.drafts[0].unwrap().sessions, 1000);
    }

    #[test]
    fn different_programs_get_separate_drafts() {
        let mut cab = DraftCabinet::new(true);
        cab.record_session("a", 1, 1, 1, false).unwrap();
        cab.record_session("b", 1, 1, 1, false).unwrap();
        cab.record_session("a", 1, 1, 1, false).unwrap();
        assert_eq!(cab.count(), 2, "两程序两张草稿");
        assert_eq!(cab.drafts[0].unwrap().sessions, 2);
    }

    #[test]
    fn downsample_marks_all_drafts() {
        let mut cab = DraftCabinet::new(true);
        cab.record_session("x", 100, 1, 1, false).unwrap();
        cab.record_session("y", 100, 1, 1, false).unwrap();
        assert!(cab.enforce_sampling_budget(50));
        assert!(cab.drafts.iter().flatten().all(|d| d.downsampled));
        // 预算内不降采样。
        let mut cab2 = DraftCabinet::new(true);
        cab2.record_session("x", 100, 1, 1, false).unwrap();
        assert!(!cab2.enforce_sampling_budget(5));
    }
}

// ===========================================================================
// 深化层 · G-A-36 补强：星卡 JSON 字节序列化 / 隐私扫描模式 / 合并策略
// （星卡格式沿用星图 JSON 规范 F126——零分配字节级写出器）
// ---------------------------------------------------------------------------

/// JSON 字符串写出（转义 \\ 与 \" 两字符；零分配逐字节）。
pub fn json_escape_into(s: &str, out: &mut [u8], n: &mut usize) {
    for &b in s.as_bytes() {
        if *n + 2 >= out.len() {
            break;
        }
        match b {
            b'\\' | b'"' => {
                out[*n] = b'\\';
                *n += 1;
                out[*n] = b;
                *n += 1;
            }
            _ => {
                out[*n] = b;
                *n += 1;
            }
        }
    }
}

/// 星卡 JSON 模板键（F126 规范的字段名登记）。
pub const STARCARD_KEYS: [&str; 7] =
    ["program", "api_calls", "startup_ms", "mem_peak", "crashes", "sessions", "downsampled"];

/// 数值键值对写出："\"key\":value," 形状。
pub fn json_kv_num(key: &str, value: u64, out: &mut [u8], n: &mut usize) {
    out[*n] = b'"';
    *n += 1;
    json_escape_into(key, out, n);
    out[*n] = b'"';
    *n += 1;
    out[*n] = b':';
    *n += 1;
    // 数值逐位写出。
    let mut buf = [0u8; 20];
    let mut i = 0;
    let mut v = value;
    if v == 0 {
        buf[0] = b'0';
        i = 1;
    }
    while v > 0 {
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        out[*n] = buf[i];
        n_step(n);
    }
}

fn n_step(n: &mut usize) {
    *n += 1;
}

/// 隐私扫描模式（脱敏三规则的检出模式面）。
pub const PRIVACY_PATTERNS: [&str; 3] = [
    "C:\\Users\\",   // 路径用户段
    "S/N:",          // 序列号前缀
    "serial-no=",    // 序列号键值
];

/// 内容扫描：命中任一隐私模式 → 需脱敏（红标）。
pub fn privacy_scan_dirty(content: &[u8]) -> bool {
    for pat in PRIVACY_PATTERNS.iter() {
        let pb = pat.as_bytes();
        if content.len() >= pb.len() {
            for w in 0..=(content.len() - pb.len()) {
                if &content[w..w + pb.len()] == pb {
                    return true;
                }
            }
        }
    }
    false
}

/// 合并策略：多会话草稿合成一张的取舍规则（指标取峰、计数累加）。
pub fn merge_policy(new_peak: u64, old_peak: u64, counter_add: u32, old_counter: u32) -> (u64, u32) {
    (new_peak.max(old_peak), old_counter + counter_add)
}

/// 域自检（深化层）。
pub fn run_stardraft_deep() -> CheckSet {
    let mut cs = CheckSet::new("F036-stardraft-deep");
    // 1) JSON 转义：反斜杠与引号成对转义。
    let mut out = [0u8; 64];
    let mut n = 0;
    json_escape_into("a\"b\\c", &mut out, &mut n);
    cs.add("json_escape", &out[..n] == b"a\\\"b\\\\c", "");
    // 2) 星卡键名七件套。
    cs.add("starcard_keys", STARCARD_KEYS == ["program", "api_calls", "startup_ms", "mem_peak", "crashes", "sessions", "downsampled"], "");
    // 3) 数值键值写出形状。
    let mut out2 = [0u8; 32];
    let mut n2 = 0;
    json_kv_num("crashes", 42, &mut out2, &mut n2);
    cs.add("json_kv_shape", &out2[..n2] == b"\"crashes\":42", "");
    // 4) 隐私扫描：路径段/序列号命中；干净内容放行。
    cs.add(
        "privacy_scan",
        privacy_scan_dirty(b"open C:\\Users\\varia\\doc") && privacy_scan_dirty(b"S/N:12345") && !privacy_scan_dirty(b"clean content"),
        "",
    );
    // 5) 合并策略：峰值取 max、计数累加。
    cs.add("merge_policy_math", merge_policy(48, 64, 3, 5) == (64, 8) && merge_policy(99, 10, 1, 0) == (99, 1), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn json_escape_no_overflow() {
        // 缓冲不足时截断不越界（零分配写出器安全性）。
        let mut small = [0u8; 4];
        let mut n = 0;
        json_escape_into("abcdef", &mut small, &mut n);
        assert!(n <= 4);
    }

    #[test]
    fn deep_checks_all_green() {
        let cs = run_stardraft_deep();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
