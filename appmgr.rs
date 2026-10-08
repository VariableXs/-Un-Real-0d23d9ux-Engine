//! VARIX-M500 · AI-14 应用生态与 SDK 深化（F326~F350，M3）
//!
//! 使命：第三方 30 分钟出应用——manifest、打包、热重载、评分、插件。
//! 与 VARIX-500 的 `pkgstore.rs`（包存储）零重复：本模块聚焦应用
//! 生命周期元层（manifest、打包、签名、索引、增量更新、调试、评分、
//! 插件、i18n、a11y 审计）。纯逻辑 + 固定容量数组。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F326 应用 manifest 规范 — 清单格式
// ---------------------------------------------------------------------------

pub const MANIFEST_MAGIC: &[u8; 4] = b"VXMF";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ManifestHead {
    pub api: u8,
    /// 权限位（bit0 文件 bit1 网络 bit2 相机 bit3 麦克风）。
    pub perms: u8,
    pub entry_len: u8,
}

pub const PERM_FILE: u8 = 1 << 0;
pub const PERM_NET: u8 = 1 << 1;
pub const PERM_CAMERA: u8 = 1 << 2;
pub const PERM_MIC: u8 = 1 << 3;

/// 解析 manifest 头：magic + api(1..=3) + 权限位 ≤0x0F。
pub fn parse_manifest(data: &[u8]) -> Option<ManifestHead> {
    if data.len() < 8 || &data[0..4] != MANIFEST_MAGIC {
        return None;
    }
    let api = data[4];
    if !(1..=3).contains(&api) {
        return None;
    }
    let perms = data[5];
    if perms & !0x0F != 0 {
        return None;
    }
    let entry_len = data[6];
    if data.len() < 8 + entry_len as usize {
        return None;
    }
    Some(ManifestHead { api, perms, entry_len })
}

// ---------------------------------------------------------------------------
// F327 应用打包器 — 一键封装（条目表）
// ---------------------------------------------------------------------------

/// 打包条目：名字（8B）+ 偏移 + 长度，固定 16 条。
pub const PACK_ENTRIES: usize = 16;

#[derive(Clone, Copy)]
pub struct PackIndex {
    pub names: [[u8; 8]; PACK_ENTRIES],
    pub offsets: [u32; PACK_ENTRIES],
    pub lengths: [u32; PACK_ENTRIES],
    pub len: usize,
}

impl PackIndex {
    pub const fn new() -> PackIndex {
        PackIndex { names: [[0; 8]; PACK_ENTRIES], offsets: [0; PACK_ENTRIES], lengths: [0; PACK_ENTRIES], len: 0 }
    }
    /// 追加条目（追加式打包，offset 由累计长度决定）。
    pub fn append(&mut self, name: &[u8], length: u32) -> Option<u32> {
        if name.is_empty() || name.len() > 8 || self.len >= PACK_ENTRIES {
            return None;
        }
        let off = if self.len == 0 { 0 } else { self.offsets[self.len - 1] + self.lengths[self.len - 1] };
        let mut key = [0u8; 8];
        key[..name.len()].copy_from_slice(name);
        self.names[self.len] = key;
        self.offsets[self.len] = off;
        self.lengths[self.len] = length;
        self.len += 1;
        Some(off)
    }
    pub fn lookup(&self, name: &[u8]) -> Option<(u32, u32)> {
        for i in 0..self.len {
            if self.names[i][..name.len()] == *name && self.names[i][name.len()] == 0 {
                return Some((self.offsets[i], self.lengths[i]));
            }
        }
        None
    }
    pub fn total_len(&self) -> u32 {
        if self.len == 0 {
            0
        } else {
            self.offsets[self.len - 1] + self.lengths[self.len - 1]
        }
    }
}

// ---------------------------------------------------------------------------
// F328 签名工作流 — 应用签名（简化哈希链）
// ---------------------------------------------------------------------------

/// FNV-1a 32 位内容哈希。
pub fn app_hash_fnv(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 签名 = hash ^ 密钥折叠（占位实现，非密码学安全）。
pub fn app_sign(bytes: &[u8], key: u32) -> u32 {
    app_hash_fnv(bytes) ^ key.rotate_left(3)
}

pub fn app_verify(bytes: &[u8], key: u32, sig: u32) -> bool {
    app_sign(bytes, key) == sig
}

// ---------------------------------------------------------------------------
// F329 应用索引 — 本地目录（固定 32）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AppIndexEntry {
    pub app_id: u32,
    pub category: u8,
    pub size_kb: u32,
}

#[derive(Clone, Copy)]
pub struct AppIndex {
    pub entries: [AppIndexEntry; 32],
    pub len: usize,
}

impl AppIndex {
    pub const fn new() -> AppIndex {
        AppIndex { entries: [AppIndexEntry { app_id: 0, category: 0, size_kb: 0 }; 32], len: 0 }
    }
    /// 去重插入。
    pub fn insert(&mut self, e: AppIndexEntry) -> bool {
        for i in 0..self.len {
            if self.entries[i].app_id == e.app_id {
                return false;
            }
        }
        if self.len >= 32 {
            return false;
        }
        self.entries[self.len] = e;
        self.len += 1;
        true
    }
    pub fn by_category(&self, cat: u8) -> usize {
        self.entries[..self.len].iter().filter(|e| e.category == cat).count()
    }
}

// ---------------------------------------------------------------------------
// F330 增量更新器 — 差分升级（块级 bsdiff 风格简化）
// ---------------------------------------------------------------------------

/// 计算新旧版本相同前缀/后缀块，返回需传输的字节数。
pub fn delta_transfer_bytes(old: &[u8], new: &[u8]) -> u32 {
    let mut prefix = 0usize;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0usize;
    while suffix < old.len() - prefix && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    (new.len() - prefix - suffix) as u32
}

/// 差分收益判定：节省 ≥50% 才值得走增量。
pub fn delta_worth_it(old_len: u32, delta_len: u32) -> bool {
    old_len > 0 && delta_len * 2 <= old_len
}

// ---------------------------------------------------------------------------
// F331 版本时间线 — 应用回滚（版本链）
// ---------------------------------------------------------------------------

/// 固定 8 层版本栈。
pub const VERSION_STACK: usize = 8;

#[derive(Clone, Copy)]
pub struct VersionTimeline {
    pub versions: [u32; VERSION_STACK],
    pub len: usize,
}

impl VersionTimeline {
    pub const fn new() -> VersionTimeline {
        VersionTimeline { versions: [0; VERSION_STACK], len: 0 }
    }
    /// 推入新版本（满了挤掉最旧）。
    pub fn push(&mut self, ver: u32) {
        if self.len < VERSION_STACK {
            self.versions[self.len] = ver;
            self.len += 1;
        } else {
            self.versions.copy_within(1.., 0);
            self.versions[VERSION_STACK - 1] = ver;
        }
    }
    /// 回滚：弹出当前，返回上一版本。
    pub fn rollback(&mut self) -> Option<u32> {
        if self.len < 2 {
            return None;
        }
        self.len -= 1;
        Some(self.versions[self.len - 1])
    }
    pub fn current(&self) -> Option<u32> {
        if self.len == 0 {
            None
        } else {
            Some(self.versions[self.len - 1])
        }
    }
}

// ---------------------------------------------------------------------------
// F332 权限申请展示 — 理由呈现
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermRequest {
    pub perm_bit: u8,
    pub has_reason: bool,
}

/// 无理由的权限申请直接拒绝展示。
pub fn perm_request_ok(r: &PermRequest) -> bool {
    r.perm_bit != 0 && r.perm_bit & !0x0F == 0 && r.has_reason
}

/// 高危权限（相机/麦克风）额外要求理由长度下限（字节）。
pub fn perm_reason_long_enough(r: &PermRequest, reason_len: usize) -> bool {
    let high_risk = r.perm_bit & (PERM_CAMERA | PERM_MIC) != 0;
    if !high_risk {
        true
    } else {
        reason_len >= 16
    }
}

// ---------------------------------------------------------------------------
// F333 隔离画像 — 应用资源视角
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AppSandbox {
    pub app_id: u32,
    pub mem_kb: u32,
    pub fds: u8,
}

/// 隔离上限：内存 ≤ 256MB，fd ≤ 64。
pub fn sandbox_within(s: &AppSandbox) -> bool {
    s.mem_kb <= 256 * 1024 && s.fds <= 64
}

/// 超限 → 建议配额（KB）。
pub fn sandbox_quota_kb(s: &AppSandbox) -> u32 {
    if s.mem_kb > 256 * 1024 {
        256 * 1024
    } else {
        s.mem_kb
    }
}

// ---------------------------------------------------------------------------
// F334 交互式文档站 — 可运行示例（开放）
// ---------------------------------------------------------------------------

/// 示例代码片段合法性：长度受限 + 无 syscall 保留字。
pub fn example_snippet_ok(code: &[u8]) -> bool {
    if code.is_empty() || code.len() > 4096 {
        return false;
    }
    for bad in [&b"raw_syscall"[..], &b"/dev/mem"[..]] {
        if code.windows(bad.len()).any(|w| w == bad) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F335 Varix Playground — 沙盒试玩
// ---------------------------------------------------------------------------

/// 试玩会话预算：指令数 + 时间片，超出即终止。
#[derive(Clone, Copy)]
pub struct PlaySession {
    pub instructions_left: u32,
    pub ms_left: u32,
}

pub fn play_step(s: &mut PlaySession, cost_ins: u32, cost_ms: u32) -> bool {
    if s.instructions_left >= cost_ins && s.ms_left >= cost_ms {
        s.instructions_left -= cost_ins;
        s.ms_left -= cost_ms;
        true
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// F336 应用脚手架 — 模板库
// ---------------------------------------------------------------------------

/// 模板槽：gui/cli/service/widget。
pub const SCAFFOLD_TEMPLATES: [[u8; 8]; 4] = [*b"gui     ", *b"cli     ", *b"service ", *b"widget  "];

pub fn scaffold_template_ok(name: &[u8]) -> bool {
    SCAFFOLD_TEMPLATES.iter().any(|t| t.starts_with(name) && !name.is_empty() && name.len() <= 8)
}

/// 生成的脚手架文件清单（固定 5 文件）。
pub const SCAFFOLD_FILES: [&[u8]; 5] = [b"manifest", b"main.vx", b"layout", b"icon", b"readme"];

// ---------------------------------------------------------------------------
// F337 vxbuild 工具链 — 统一 CLI（构建阶段机）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildStage {
    Parse,
    TypeCheck,
    Codegen,
    Link,
    Package,
}

/// 构建流水线推进；返回 None 表示流水线结束。
pub fn build_next(s: BuildStage) -> Option<BuildStage> {
    match s {
        BuildStage::Parse => Some(BuildStage::TypeCheck),
        BuildStage::TypeCheck => Some(BuildStage::Codegen),
        BuildStage::Codegen => Some(BuildStage::Link),
        BuildStage::Link => Some(BuildStage::Package),
        BuildStage::Package => None,
    }
}

// ---------------------------------------------------------------------------
// F338 调试协议 — 应用调试通道
// ---------------------------------------------------------------------------

/// 调试命令帧：[cmd, len_lo, len_hi, payload...]
pub fn dbg_frame_len(frame: &[u8]) -> Option<usize> {
    if frame.len() < 3 {
        return None;
    }
    let n = frame[1] as usize | ((frame[2] as usize) << 8);
    if frame.len() == 3 + n {
        Some(n)
    } else {
        None
    }
}

pub const DBG_CMD_BREAK: u8 = 0x01;
pub const DBG_CMD_STEP: u8 = 0x02;
pub const DBG_CMD_READ: u8 = 0x03;

// ---------------------------------------------------------------------------
// F339 热重载 — 改动即生效
// ---------------------------------------------------------------------------

/// 内容哈希不变 → 跳过重载。
pub fn hot_reload_needed(old_hash: u32, new_hash: u32) -> bool {
    old_hash != new_hash
}

/// 重载安全窗口：仅当应用处于 idle 状态才可热替换。
pub fn hot_reload_safe(state: u8) -> bool {
    state == 0 // 0=idle
}

// ---------------------------------------------------------------------------
// F340 C FFI 桥 — 本机互操作（ABI 检查）
// ---------------------------------------------------------------------------

/// FFI 签名合法性：返回值 ≤ 16 字节、参数 ≤ 8 个。
pub fn ffi_sig_ok(ret_bytes: u32, args: u8) -> bool {
    ret_bytes <= 16 && args <= 8
}

/// 符号名规范：C ABI 只允许字母数字下划线。
pub fn ffi_symbol_ok(name: &[u8]) -> bool {
    !name.is_empty()
        && name.iter().all(|&c| c == b'_' || c.is_ascii_alphanumeric())
        && !name[0].is_ascii_digit()
}

// ---------------------------------------------------------------------------
// F341 脚本桥 — 内嵌解释器框架（字节码预算）
// ---------------------------------------------------------------------------

/// 脚本执行预算：字节码 ≤ 64KB、栈深 ≤ 128。
pub fn script_budget_ok(code_len: u32, stack_depth: u8) -> bool {
    code_len <= 64 * 1024 && stack_depth <= 128
}

// ---------------------------------------------------------------------------
// F342 应用性能评分 — 上架基准
// ---------------------------------------------------------------------------

/// 评分：冷启动 ≤500ms、内存 ≤64MB、崩溃率 ≤1‰ 各占 1/3。
pub fn app_score(cold_ms: u32, mem_kb: u32, crash_permille: u32) -> u8 {
    let mut score = 0;
    if cold_ms <= 500 {
        score += 34;
    } else if cold_ms <= 1000 {
        score += 17;
    }
    if mem_kb <= 64 * 1024 {
        score += 33;
    } else if mem_kb <= 128 * 1024 {
        score += 16;
    }
    if crash_permille <= 1 {
        score += 33;
    } else if crash_permille <= 5 {
        score += 16;
    }
    score
}

// ---------------------------------------------------------------------------
// F343 崩溃聚合 — 本地崩溃云（指纹归并）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct CrashBucket {
    pub sig: u32,
    pub count: u16,
}

#[derive(Clone, Copy)]
pub struct CrashAgg {
    pub buckets: [CrashBucket; 8],
    pub len: usize,
}

impl CrashAgg {
    pub const fn new() -> CrashAgg {
        CrashAgg { buckets: [CrashBucket { sig: 0, count: 0 }; 8], len: 0 }
    }
    pub fn record(&mut self, sig: u32) {
        for i in 0..self.len {
            if self.buckets[i].sig == sig {
                self.buckets[i].count = self.buckets[i].count.saturating_add(1);
                return;
            }
        }
        if self.len < 8 {
            self.buckets[self.len] = CrashBucket { sig, count: 1 };
            self.len += 1;
        }
    }
    pub fn top(&self) -> Option<(u32, u16)> {
        if self.len == 0 {
            return None;
        }
        let mut best = 0;
        for i in 1..self.len {
            if self.buckets[i].count > self.buckets[best].count {
                best = i;
            }
        }
        Some((self.buckets[best].sig, self.buckets[best].count))
    }
}

// ---------------------------------------------------------------------------
// F344 兼容矩阵运行器 — 测试床
// ---------------------------------------------------------------------------

/// 矩阵运行：目标三元组 × API 级别，全过才算通过。
pub const MATRIX_TARGETS: [[u8; 4]; 3] = [*b"x866", *b"arm6", *b"rv64"];

pub fn matrix_pass(results: &[bool; 3]) -> bool {
    results.iter().all(|&r| r)
}

// ---------------------------------------------------------------------------
// F345 指标公开 — 评分数据开放
// ---------------------------------------------------------------------------

/// 渲染一行公开指标：`APP 123 score=67 crash=0`。
pub fn render_public_metrics(out: &mut [u8], app_id: u32, score: u8, crash_permille: u32) -> usize {
    use crate::checks::{push_str, push_usize};
    let mut n = 0;
    push_str(out, &mut n, "APP ");
    push_usize(out, &mut n, app_id as usize);
    push_str(out, &mut n, " score=");
    push_usize(out, &mut n, score as usize);
    push_str(out, &mut n, " crash=");
    push_usize(out, &mut n, crash_permille as usize);
    n
}

// ---------------------------------------------------------------------------
// F346 插件协议 — 应用内扩展
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginManifest {
    pub host_api: u8,
    /// 插件声明的宿主能力位。
    pub caps: u8,
}

/// 插件能力必须是宿主授予集合的子集。
pub fn plugin_caps_ok(p: &PluginManifest, granted: u8) -> bool {
    p.caps & !granted == 0 && p.host_api <= 2
}

// ---------------------------------------------------------------------------
// F347 应用 i18n 库 — 国际化框架
// ---------------------------------------------------------------------------

/// 字符串表：id → 文本偏移（每语言一张），固定 16 条。
#[derive(Clone, Copy)]
pub struct I18nTable {
    pub ids: [u16; 16],
    pub offsets: [u16; 16],
    pub len: usize,
}

impl I18nTable {
    pub const fn new() -> I18nTable {
        I18nTable { ids: [0; 16], offsets: [0; 16], len: 0 }
    }
    pub fn put(&mut self, id: u16, offset: u16) -> bool {
        for i in 0..self.len {
            if self.ids[i] == id {
                self.offsets[i] = offset;
                return true;
            }
        }
        if self.len < 16 {
            self.ids[self.len] = id;
            self.offsets[self.len] = offset;
            self.len += 1;
            true
        } else {
            false
        }
    }
    pub fn get(&self, id: u16) -> Option<u16> {
        self.ids[..self.len].iter().position(|&x| x == id).map(|i| self.offsets[i])
    }
    /// 翻译覆盖检查：所有 id 在两种语言里都有。
    pub fn coverage_ok(&self, other: &I18nTable) -> bool {
        (0..self.len).all(|i| other.get(self.ids[i]).is_some())
    }
}

// ---------------------------------------------------------------------------
// F348 无障碍审计工具 — a11y 检查
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A11yRow {
    pub has_label: bool,
    pub focusable: bool,
    pub has_focus_indicator: bool,
    pub min_touch_dp: u16,
}

pub fn a11y_audit(rows: &[A11yRow]) -> usize {
    rows.iter()
        .filter(|r| {
            (r.focusable && !r.has_focus_indicator)
                || (r.focusable && !r.has_label)
                || (r.min_touch_dp > 0 && r.min_touch_dp < 44)
        })
        .count()
}

// ---------------------------------------------------------------------------
// F349 SDK fuzz — 工具链对抗
// ---------------------------------------------------------------------------

/// 对任意输入字节流，manifest 解析不 panic 且返回合法 Option。
pub fn fuzz_manifest(raw: &[u8]) -> Option<ManifestHead> {
    parse_manifest(raw)
}

/// 打包器 fuzz：非法名字被拒绝。
pub fn fuzz_pack_name(name: &[u8]) -> bool {
    let mut p = PackIndex::new();
    p.append(name, 1).is_some()
}

// ---------------------------------------------------------------------------
// F350 生态域自检（M500）
// ---------------------------------------------------------------------------

pub fn run_appmgr_checks() -> CheckSet {
    let mut set = CheckSet::new("appmgr-m500");

    // F326
    let mut mf = [0u8; 10];
    mf[0..4].copy_from_slice(MANIFEST_MAGIC);
    mf[4] = 2;
    mf[5] = PERM_FILE | PERM_NET;
    mf[6] = 2;
    mf[8] = b'm';
    mf[9] = b'n';
    set.add("F326 parse", parse_manifest(&mf).is_some(), "valid");
    set.add("F326 bad api", { let mut b = mf; b[4] = 9; parse_manifest(&b).is_none() }, "api range");

    // F327
    let mut pk = PackIndex::new();
    let o1 = pk.append(b"main.vx", 100);
    let o2 = pk.append(b"icon", 40);
    set.add("F327 offsets", o1 == Some(0) && o2 == Some(100), "sequential");
    set.add("F327 lookup", pk.lookup(b"icon") == Some((100, 40)), "found");
    set.add("F327 total", pk.total_len() == 140, "sum");

    // F328
    let sig = app_sign(b"payload", 0x1234);
    set.add("F328 verify", app_verify(b"payload", 0x1234, sig), "sign+verify");
    set.add("F328 tamper", !app_verify(b"payloae", 0x1234, sig), "content changed");

    // F329
    let mut ai = AppIndex::new();
    set.add("F329 insert", ai.insert(AppIndexEntry { app_id: 7, category: 1, size_kb: 300 }), "first");
    set.add("F329 dup", !ai.insert(AppIndexEntry { app_id: 7, category: 2, size_kb: 1 }), "dedup");
    set.add("F329 category", ai.by_category(1) == 1, "count");

    // F330
    let old = b"AAAAAA BBBB";
    let new_ = b"AAAAAA CCCC";
    let d = delta_transfer_bytes(old, new_);
    set.add("F330 delta 4", d == 4, "only CCCC differs");
    set.add("F330 worth", !delta_worth_it(11, 6), "50% gate");

    // F331
    let mut vt = VersionTimeline::new();
    vt.push(10);
    vt.push(11);
    set.add("F331 current", vt.current() == Some(11), "top");
    set.add("F331 rollback", vt.rollback() == Some(10), "one back");
    set.add("F331 empty", VersionTimeline::new().rollback().is_none(), "no versions");

    // F332
    let r = PermRequest { perm_bit: PERM_NET, has_reason: true };
    set.add("F332 ok", perm_request_ok(&r), "with reason");
    set.add("F332 no reason", !perm_request_ok(&PermRequest { perm_bit: PERM_NET, has_reason: false }), "blocked");
    let cam = PermRequest { perm_bit: PERM_CAMERA, has_reason: true };
    set.add("F332 cam short", !perm_reason_long_enough(&cam, 5), "needs 16B");
    set.add("F332 cam ok", perm_reason_long_enough(&cam, 20), "long enough");

    // F333
    let sb = AppSandbox { app_id: 1, mem_kb: 300 * 1024, fds: 10 };
    set.add("F333 over", !sandbox_within(&sb), "300MB > 256MB");
    set.add("F333 quota", sandbox_quota_kb(&sb) == 256 * 1024, "capped");
    set.add("F333 within", sandbox_within(&AppSandbox { app_id: 1, mem_kb: 1000, fds: 5 }), "ok");

    // F334
    set.add("F334 ok", example_snippet_ok(b"print(hello)"), "clean snippet");
    set.add("F334 raw blocked", !example_snippet_ok(b"raw_syscall(1)"), "syscall banned");
    set.add("F334 empty", !example_snippet_ok(b""), "empty");

    // F335
    let mut ps = PlaySession { instructions_left: 100, ms_left: 50 };
    set.add("F335 step ok", play_step(&mut ps, 40, 20), "within budget");
    set.add("F335 step deny", !play_step(&mut ps, 100, 1), "insufficient");

    // F336
    set.add("F336 gui", scaffold_template_ok(b"gui"), "template hit");
    set.add("F336 unknown", !scaffold_template_ok(b"kernel"), "not a template");
    set.add("F336 files", SCAFFOLD_FILES.len() == 5, "5 files");

    // F337
    let mut st = BuildStage::Parse;
    let mut steps = 0;
    while let Some(next) = build_next(st) {
        st = next;
        steps += 1;
    }
    set.add("F337 pipeline", st == BuildStage::Package && steps == 4, "5 stages");

    // F338
    let frame = [DBG_CMD_READ, 2, 0, 0xAA, 0xBB];
    set.add("F338 frame", dbg_frame_len(&frame) == Some(2), "len match");
    set.add("F338 bad len", dbg_frame_len(&[1, 5, 0, 0xAA]).is_none(), "mismatch");
    set.add("F338 short", dbg_frame_len(&[1, 0]).is_none(), "too short");

    // F339
    set.add("F339 hash diff", hot_reload_needed(1, 2), "changed");
    set.add("F339 hash same", !hot_reload_needed(5, 5), "unchanged");
    set.add("F339 idle only", hot_reload_safe(0) && !hot_reload_safe(1), "state gate");

    // F340
    set.add("F340 sig", ffi_sig_ok(16, 8) && !ffi_sig_ok(24, 2), "ret limit");
    set.add("F340 symbol", ffi_symbol_ok(varix_ffi_name()) && !ffi_symbol_ok(b"9bad"), "naming");

    // F341
    set.add("F341 ok", script_budget_ok(1024, 64), "within");
    set.add("F341 code big", !script_budget_ok(65 * 1024, 1), "code cap");
    set.add("F341 stack deep", !script_budget_ok(10, 200), "stack cap");

    // F342
    set.add("F342 full", app_score(400, 60_000, 0) == 100, "all pass");
    set.add("F342 mid", app_score(800, 90_000, 3) == 49, "partial");
    set.add("F342 bad", app_score(5000, 999_999, 99) == 0, "all fail");

    // F343
    let mut ca = CrashAgg::new();
    ca.record(0xAAAA);
    ca.record(0xAAAA);
    ca.record(0xBBBB);
    set.add("F343 aggregate", ca.top() == Some((0xAAAA, 2)), "top sig");
    set.add("F343 empty", CrashAgg::new().top().is_none(), "no crashes");

    // F344
    set.add("F344 matrix pass", matrix_pass(&[true, true, true]), "all targets");
    set.add("F344 matrix fail", !matrix_pass(&[true, false, true]), "one fails");

    // F345
    let mut buf = [0u8; 48];
    let n = render_public_metrics(&mut buf, 123, 67, 0);
    let text = core::str::from_utf8(&buf[..n]).unwrap();
    set.add("F345 render", text == "APP 123 score=67 crash=0", "format");

    // F346
    let pm = PluginManifest { host_api: 1, caps: 0b101 };
    set.add("F346 caps ok", plugin_caps_ok(&pm, 0b111), "subset");
    set.add("F346 caps deny", !plugin_caps_ok(&pm, 0b010), "beyond grant");
    set.add("F346 api deny", !plugin_caps_ok(&PluginManifest { host_api: 9, caps: 0 }, 0xFF), "api range");

    // F347
    let mut zh = I18nTable::new();
    zh.put(1, 10);
    zh.put(2, 20);
    let mut en = I18nTable::new();
    en.put(1, 30);
    set.add("F347 get", zh.get(2) == Some(20), "lookup");
    set.add("F347 coverage fail", !zh.coverage_ok(&en), "id2 missing");
    en.put(2, 40);
    set.add("F347 coverage ok", zh.coverage_ok(&en), "complete");

    // F348
    let rows = [
        A11yRow { has_label: true, focusable: true, has_focus_indicator: true, min_touch_dp: 48 },
        A11yRow { has_label: true, focusable: true, has_focus_indicator: false, min_touch_dp: 48 },
        A11yRow { has_label: true, focusable: false, has_focus_indicator: false, min_touch_dp: 30 },
    ];
    set.add("F348 audit 2", a11y_audit(&rows) == 2, "row1 no indicator, row2 small touch");
    set.add("F348 clean", a11y_audit(&rows[..1]) == 0, "first row fine");
    set.add("F348 no label", a11y_audit(&[A11yRow { has_label: false, focusable: true, has_focus_indicator: true, min_touch_dp: 48 }]) == 1, "label required");

    // F349
    set.add("F349 fuzz manifest", fuzz_manifest(b"garbage!!").is_none(), "no panic");
    set.add("F349 fuzz name long", !fuzz_pack_name(b"0123456789"), "too long");
    set.add("F349 fuzz name empty", !fuzz_pack_name(b""), "empty rejected");

    // F350
    set.add("F350 self count", set.len() >= 25, "25+ checks");

    set
}

fn varix_ffi_name() -> &'static [u8] {
    b"varix_open_window"
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f327_pack_full() {
        let mut p = PackIndex::new();
        for i in 0..PACK_ENTRIES {
            assert!(p.append(&[b'a' + i as u8], 10).is_some());
        }
        assert!(p.append(b"overflow", 1).is_none());
        assert_eq!(p.total_len(), PACK_ENTRIES as u32 * 10);
    }

    #[test]
    fn f330_prefix_and_suffix_both_shared() {
        // 前缀 AAAA + 后缀 ZZZZ 都共享，只传中间。
        let d = delta_transfer_bytes(b"AAAAXZZZZ", b"AAAAYZZZZ");
        assert_eq!(d, 1);
    }

    #[test]
    fn f331_version_stack_eviction() {
        let mut vt = VersionTimeline::new();
        for v in 1..=9u32 {
            vt.push(v);
        }
        assert_eq!(vt.current(), Some(9)); // 1 被挤掉
        assert_eq!(vt.rollback(), Some(8));
    }

    #[test]
    fn f342_score_boundaries() {
        // 其余两项都超限，只看启动分：500ms 整 34 分，501ms 折半 17 分。
        assert_eq!(app_score(500, 200_000, 99), 34);
        assert_eq!(app_score(501, 200_000, 99), 17);
    }

    #[test]
    fn domain_self_test_passes() {
        let set = run_appmgr_checks();
        assert!(!set.truncated());
        assert!(set.all_passed(), "appmgr-m500 self-test: {} checks", set.len());
    }
}
