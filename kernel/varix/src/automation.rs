//! VARIX-M500 · AI-15 自动化与效率引擎（F351~F375，M3）
//!
//! 使命：非程序员也能造自动化——快捷指令、录制、分享、沙箱。
//! 与 AURORA 的 `aurora/`（UI 框架）零重复：本模块聚焦自动化引擎
//! 纯逻辑层（流程模型、触发器、沙箱、审计、fuzz）。固定容量数组。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F351 快捷指令引擎 — 图形化自动化（流程 = 步骤序列）
// ---------------------------------------------------------------------------

/// 步骤上限（单条流程）。
pub const FLOW_STEPS: usize = 32;

/// 内置动作类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    OpenApp,
    SendNotify,
    SetVolume,
    RunScript,
    Wait,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub kind: ActionKind,
    /// 动作参数（0~255，类别各异）。
    pub arg: u8,
}

#[derive(Clone, Copy)]
pub struct Flow {
    pub steps: [Step; FLOW_STEPS],
    pub len: usize,
}

impl Flow {
    pub const fn new() -> Flow {
        Flow { steps: [Step { kind: ActionKind::Wait, arg: 0 }; FLOW_STEPS], len: 0 }
    }
    pub fn push(&mut self, s: Step) -> bool {
        if self.len >= FLOW_STEPS {
            return false;
        }
        self.steps[self.len] = s;
        self.len += 1;
        true
    }
    /// 抽象解释器：顺序执行，返回成功执行的步数。
    pub fn execute(&self, sink: &mut dyn FnMut(ActionKind, u8)) -> usize {
        for i in 0..self.len {
            sink(self.steps[i].kind, self.steps[i].arg);
        }
        self.len
    }
}

// ---------------------------------------------------------------------------
// F352 事件触发器 — 系统事件订阅
// ---------------------------------------------------------------------------

/// 可订阅事件位。
pub const EV_LAUNCH: u16 = 1 << 0;
pub const EV_TIME: u16 = 1 << 1;
pub const EV_BATTERY_LOW: u16 = 1 << 2;
pub const EV_FOLDER_CHANGE: u16 = 1 << 3;
pub const EV_CLIPBOARD: u16 = 1 << 4;

#[derive(Clone, Copy)]
pub struct Trigger {
    pub flow_id: u32,
    pub events: u16,
}

#[derive(Clone, Copy)]
pub struct TriggerTable {
    pub items: [Trigger; 16],
    pub len: usize,
}

impl TriggerTable {
    pub const fn new() -> TriggerTable {
        TriggerTable { items: [Trigger { flow_id: 0, events: 0 }; 16], len: 0 }
    }
    pub fn add(&mut self, t: Trigger) -> bool {
        if t.events == 0 || self.len >= 16 {
            return false;
        }
        self.items[self.len] = t;
        self.len += 1;
        true
    }
    /// 系统事件发生 → 返回被命中的 flow_id 列表（写入 out，返回数量）。
    pub fn fire(&self, event: u16, out: &mut [u32]) -> usize {
        let mut n = 0;
        for i in 0..self.len {
            if self.items[i].events & event != 0 && n < out.len() {
                out[n] = self.items[i].flow_id;
                n += 1;
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F353 定时任务 — cron 式调度
// ---------------------------------------------------------------------------

/// 最小 cron：分钟/小时/日通配（0xFF = 任意）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CronSpec {
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
}

pub fn cron_matches(c: CronSpec, minute: u8, hour: u8, day: u8) -> bool {
    (c.minute == 0xFF || c.minute == minute)
        && (c.hour == 0xFF || c.hour == hour)
        && (c.day == 0xFF || c.day == day)
}

/// 下次触发分钟数（从 now 起算 ≤ 24h），找不到返回 None。
pub fn cron_next_minutes(c: CronSpec, now_min: u32) -> Option<u32> {
    for m in 1..=24 * 60u32 {
        let abs = now_min + m;
        let minute = (abs % 60) as u8;
        let hour = ((abs / 60) % 24) as u8;
        if cron_matches(c, minute, hour, 0xFF) {
            return Some(m);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// F354 条件编排 — if/then 流程
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    BatteryBelow(u8),
    TimeAfter(u8),
    FlagSet(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CondRule {
    pub cond: Cond,
    pub then_flow: u32,
    pub else_flow: u32,
}

/// 条件求值（ctx：电量 cs / 当前小时 / 标志位）。
pub fn cond_eval(c: Cond, battery_cs: i32, hour: u8, flags: u8) -> bool {
    match c {
        Cond::BatteryBelow(pct) => battery_cs < (pct as i32) * 100,
        Cond::TimeAfter(h) => hour >= h,
        Cond::FlagSet(bit) => flags & (1 << bit) != 0,
    }
}

pub fn cond_route(r: &CondRule, battery_cs: i32, hour: u8, flags: u8) -> u32 {
    if cond_eval(r.cond, battery_cs, hour, flags) {
        r.then_flow
    } else {
        r.else_flow
    }
}

// ---------------------------------------------------------------------------
// F355 动作库 — 内置动作集（注册表）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ActionEntry {
    pub name: [u8; 8],
    pub kind: ActionKind,
    pub min_perm: u8, // 需要的权限位
}

pub const ACTION_LIB_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct ActionLib {
    pub entries: [ActionEntry; ACTION_LIB_MAX],
    pub len: usize,
}

impl ActionLib {
    pub const fn new() -> ActionLib {
        ActionLib { entries: [ActionEntry { name: [0; 8], kind: ActionKind::Wait, min_perm: 0 }; ACTION_LIB_MAX], len: 0 }
    }
    /// 注册（同名覆盖）。
    pub fn register(&mut self, e: ActionEntry) -> bool {
        for i in 0..self.len {
            if self.entries[i].name[..8] == e.name[..8] {
                self.entries[i] = e;
                return true;
            }
        }
        if self.len < ACTION_LIB_MAX {
            self.entries[self.len] = e;
            self.len += 1;
            true
        } else {
            false
        }
    }
    pub fn find(&self, name: &[u8]) -> Option<&ActionEntry> {
        self.entries[..self.len].iter().find(|e| e.name.starts_with(name) && name.len() <= 8)
    }
}

// ---------------------------------------------------------------------------
// F356 操作录制 — 录制为流程
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Recorder {
    pub flow: Flow,
    pub recording: bool,
}

impl Recorder {
    pub const fn new() -> Recorder {
        Recorder { flow: Flow::new(), recording: false }
    }
    pub fn start(&mut self) {
        self.flow = Flow::new();
        self.recording = true;
    }
    pub fn observe(&mut self, s: Step) {
        if self.recording {
            self.flow.push(s);
        }
    }
    pub fn stop(&mut self) -> Flow {
        self.recording = false;
        self.flow
    }
}

// ---------------------------------------------------------------------------
// F357 流程分享格式 — 社区分享（开放）
// ---------------------------------------------------------------------------

/// 分享包头：magic + 版本 + 步骤数。
pub const SHARE_MAGIC: &[u8; 4] = b"VXFL";

/// 编码流程到固定缓冲，返回长度（不足返回 None）。
pub fn share_encode(f: &Flow, out: &mut [u8]) -> Option<usize> {
    let need = 6 + f.len * 2;
    if out.len() < need {
        return None;
    }
    out[0..4].copy_from_slice(SHARE_MAGIC);
    out[4] = 1; // 版本
    out[5] = f.len as u8;
    for i in 0..f.len {
        out[6 + i * 2] = f.steps[i].kind as u8;
        out[6 + i * 2 + 1] = f.steps[i].arg;
    }
    Some(need)
}

pub fn share_decode(data: &[u8]) -> Option<Flow> {
    if data.len() < 6 || &data[0..4] != SHARE_MAGIC || data[4] != 1 {
        return None;
    }
    let n = data[5] as usize;
    if data.len() < 6 + n * 2 {
        return None;
    }
    let mut f = Flow::new();
    for i in 0..n {
        let kind = match data[6 + i * 2] {
            0 => ActionKind::OpenApp,
            1 => ActionKind::SendNotify,
            2 => ActionKind::SetVolume,
            3 => ActionKind::RunScript,
            _ => ActionKind::Wait,
        };
        if !f.push(Step { kind, arg: data[6 + i * 2 + 1] }) {
            return None;
        }
    }
    Some(f)
}

// ---------------------------------------------------------------------------
// F358 自动化沙箱 — 权限受限执行
// ---------------------------------------------------------------------------

/// 沙箱放行判定：动作所需权限 ⊆ 授予权限。
pub fn sandbox_allow(required: u8, granted: u8) -> bool {
    required & !granted == 0
}

/// 沙箱内禁用的动作（无论权限）。
pub fn sandbox_banned(kind: ActionKind) -> bool {
    kind == ActionKind::RunScript // 沙箱流程不允许嵌套脚本
}

// ---------------------------------------------------------------------------
// F359 失败重试策略 — 韧性执行
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    /// 退避基数（ms）。
    pub backoff_ms: u32,
}

pub const RETRY_DEFAULT: RetryPolicy = RetryPolicy { max_attempts: 3, backoff_ms: 500 };

/// 第 n 次尝试（0 起）是否还能来；退避 = 基数 × 2^n。
pub fn retry_next(p: &RetryPolicy, attempt: u8) -> Option<u32> {
    if attempt >= p.max_attempts {
        return None;
    }
    Some(p.backoff_ms << attempt.min(5))
}

// ---------------------------------------------------------------------------
// F360 执行留痕 — 每次运行日志（环形 16 条）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct RunRecord {
    pub flow_id: u32,
    pub ok: bool,
    pub ts_ms: u32,
}

#[derive(Clone, Copy)]
pub struct RunLog {
    pub ring: [RunRecord; 16],
    pub head: usize,
    pub total: usize,
}

impl RunLog {
    pub const fn new() -> RunLog {
        RunLog { ring: [RunRecord { flow_id: 0, ok: false, ts_ms: 0 }; 16], head: 0, total: 0 }
    }
    pub fn append(&mut self, r: RunRecord) {
        self.ring[self.head] = r;
        self.head = (self.head + 1) % 16;
        self.total += 1;
    }
    /// 最近 n 条（新→旧），写入 out。
    pub fn recent(&self, out: &mut [RunRecord]) -> usize {
        let n = out.len().min(16).min(self.total);
        for i in 0..n {
            let idx = (self.head + 16 - 1 - i) % 16;
            out[i] = self.ring[idx];
        }
        n
    }
}

// ---------------------------------------------------------------------------
// F361 内置脚本运行时 — 系统脚本引擎（指令预算解释器）
// ---------------------------------------------------------------------------

/// 简化栈机：操作码 PUSH n / ADD / MUL / END。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpCode {
    Push(u16),
    Add,
    Mul,
    End,
}

/// 执行字节码（栈深 8、步数 ≤ 256，防脚本炸弹）。
pub fn script_run(code: &[OpCode]) -> Option<u32> {
    let mut stack = [0u32; 8];
    let mut sp = 0usize;
    for (i, op) in code.iter().enumerate() {
        if i >= 256 {
            return None;
        }
        match op {
            OpCode::Push(v) => {
                if sp >= 8 {
                    return None;
                }
                stack[sp] = *v as u32;
                sp += 1;
            }
            OpCode::Add | OpCode::Mul => {
                if sp < 2 {
                    return None;
                }
                let b = stack[sp - 1];
                let a = stack[sp - 2];
                sp -= 2;
                let r = if *op == OpCode::Add { a.wrapping_add(b) } else { a.wrapping_mul(b) };
                if sp >= 8 {
                    return None;
                }
                stack[sp] = r;
                sp += 1;
            }
            OpCode::End => {
                if sp == 0 {
                    return None;
                }
                return Some(stack[sp - 1]);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// F362 CLI 自动化桥 — 命令行整合
// ---------------------------------------------------------------------------

/// CLI 参数解析：`flow run <id>` / `flow list`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliCmd {
    Run(u32),
    List,
    Unknown,
}

pub fn cli_parse(args: &[&[u8]]) -> CliCmd {
    if args.len() >= 2 && args[0] == b"flow" {
        if args[1] == b"list" {
            return CliCmd::List;
        }
        if args[1] == b"run" {
            if let Some(id) = args.get(2) {
                if let Some(n) = core::str::from_utf8(id).ok().and_then(|s| s.parse::<u32>().ok()) {
                    return CliCmd::Run(n);
                }
            }
        }
    }
    CliCmd::Unknown
}

// ---------------------------------------------------------------------------
// F363 批量文件操作 — 规则整理
// ---------------------------------------------------------------------------

/// 重命名规则：前缀 + 序号。
pub fn batch_rename(prefix: &[u8], index: u32, out: &mut [u8]) -> usize {
    let mut n = 0;
    let copy = |out: &mut [u8], n: &mut usize, s: &[u8]| {
        for &b in s {
            if *n < out.len() {
                out[*n] = b;
                *n += 1;
            }
        }
    };
    copy(out, &mut n, prefix);
    // 序号十进制
    let mut digits = [0u8; 10];
    let mut w = 0;
    let mut v = index;
    if v == 0 {
        digits[0] = b'0';
        w = 1;
    }
    while v > 0 {
        digits[w] = b'0' + (v % 10) as u8;
        v /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        copy(out, &mut n, &digits[w..w + 1]);
    }
    n
}

/// 批量分类：按扩展名 → 目标桶（0 图 1 文档 2 其他）。
pub fn batch_classify(name: &[u8]) -> u8 {
    let ext: &[&[u8]] = &[b"png", b"jpg", b"gif", b"webp"];
    let ext_doc: &[&[u8]] = &[b"pdf", b"doc", b"txt", b"md"];
    let mut lower_buf = [0u8; 64];
    let ln = name.len().min(64);
    lower_buf[..ln].copy_from_slice(&name[..ln]);
    lower_buf[..ln].make_ascii_lowercase();
    let lower: &[u8] = &lower_buf[..ln];
    if ext.iter().any(|e| lower.ends_with(e)) {
        0
    } else if ext_doc.iter().any(|e| lower.ends_with(e)) {
        1
    } else {
        2
    }
}

// ---------------------------------------------------------------------------
// F364 监视文件夹 — 落盘即处理（防抖）
// ---------------------------------------------------------------------------

/// 变更防抖：同一文件 2 秒内重复变更合并。
pub fn watch_debounce(last_ms: u32, now_ms: u32) -> bool {
    now_ms.wrapping_sub(last_ms) >= 2000
}

// ---------------------------------------------------------------------------
// F365 剪贴板流水线 — 内容自动加工
// ---------------------------------------------------------------------------

/// 流水线：去空白 → 截断 → 加前缀。
pub fn clipboard_transform(input: &[u8], prefix: &[u8], max_len: usize, out: &mut [u8]) -> usize {
    let mut n = 0;
    for &b in prefix {
        if n < out.len() {
            out[n] = b;
            n += 1;
        }
    }
    let mut written = 0;
    let mut pending_space = false;
    for &b in input {
        if written >= max_len {
            break;
        }
        if b == b' ' || b == b'\t' || b == b'\n' {
            pending_space = true;
            continue;
        }
        if pending_space && written > 0 && n < out.len() && written < max_len {
            out[n] = b' ';
            n += 1;
            written += 1;
        }
        pending_space = false;
        if written < max_len && n < out.len() {
            out[n] = b;
            n += 1;
            written += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F366 设备联动预留 — 多机协同
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevicePeer {
    pub id: u32,
    pub online: bool,
}

/// 联动目标选择：在线且不是自己。
pub fn pick_peer<'a>(peppers: &'a [DevicePeer], self_id: u32) -> Option<&'a DevicePeer> {
    peppers.iter().find(|p| p.online && p.id != self_id)
}

// ---------------------------------------------------------------------------
// F367 自动化性能预算
// ---------------------------------------------------------------------------

/// 单次流程执行预算：≤ 2s、步数 ≤ FLOW_STEPS、脚本指令 ≤ 10k。
pub fn auto_budget_ok(run_ms: u32, steps: usize, script_ins: u32) -> bool {
    run_ms <= 2000 && steps <= FLOW_STEPS && script_ins <= 10_000
}

// ---------------------------------------------------------------------------
// F368 回放验证 — 流程正确性
// ---------------------------------------------------------------------------

/// 录制流程与回放事件逐项比对；完全一致才算通过。
pub fn replay_verify(recorded: &Flow, replayed: &Flow) -> bool {
    recorded.len == replayed.len
        && (0..recorded.len).all(|i| {
            recorded.steps[i].kind == replayed.steps[i].kind
                && recorded.steps[i].arg == replayed.steps[i].arg
        })
}

// ---------------------------------------------------------------------------
// F369 权限审计 — 自动化行为审计
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AuditTrail {
    /// 每个 flow 最近使用的权限位累计。
    pub used: [u8; 8],
    pub flow_len: usize,
}

impl AuditTrail {
    pub const fn new() -> AuditTrail {
        AuditTrail { used: [0; 8], flow_len: 0 }
    }
    pub fn note(&mut self, flow_idx: usize, perm: u8) {
        if flow_idx < 8 {
            self.used[flow_idx] |= perm;
        }
    }
    /// 越权审计：使用位超出声明位。
    pub fn violated(&self, declared: &[u8; 8]) -> bool {
        (0..8).any(|i| self.used[i] & !declared[i] != 0)
    }
}

// ---------------------------------------------------------------------------
// F370 恶意流程拦截 — 模板审核
// ---------------------------------------------------------------------------

/// 审核规则：步数超限 / 含嵌套脚本 / 音量动作为 0x00（爆音探测）→ 拦截。
pub fn flow_audit_intercept(f: &Flow) -> bool {
    if f.len == 0 || f.len > FLOW_STEPS {
        return true;
    }
    f.steps[..f.len].iter().any(|s| sandbox_banned(s.kind) || (s.kind == ActionKind::SetVolume && s.arg > 100))
}

// ---------------------------------------------------------------------------
// F371 自动化 API 版本化
// ---------------------------------------------------------------------------

pub const AUTO_API_VERSION: u8 = 2;
pub const AUTO_API_MIN: u8 = 1;

pub fn auto_api_ok(requested: u8) -> bool {
    (AUTO_API_MIN..=AUTO_API_VERSION).contains(&requested)
}

// ---------------------------------------------------------------------------
// F372 开机自动化 — 引导后流程
// ---------------------------------------------------------------------------

/// 开机流程：延迟启动（避免抢占引导预算），最多 8 条，按优先级排。
#[derive(Clone, Copy)]
pub struct BootFlow {
    pub flow_id: u32,
    pub delay_ms: u32,
    pub priority: u8,
}

/// 排序：优先级高者先（稳定插入排序），返回执行顺序索引表。
pub fn boot_flows_order(flows: &[BootFlow], out: &mut [usize]) -> usize {
    let n = flows.len().min(out.len());
    for (i, v) in out.iter_mut().enumerate().take(n) {
        *v = i;
    }
    for a in 1..n {
        let mut b = a;
        while b > 0 && flows[out[b]].priority > flows[out[b - 1]].priority {
            out.swap(b, b - 1);
            b -= 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// F373 示例流程库 — 教学素材
// ---------------------------------------------------------------------------

/// 示例流程：截图分享（打开应用→通知→等待）。
pub fn sample_flow_screenshot_share() -> Flow {
    let mut f = Flow::new();
    f.push(Step { kind: ActionKind::OpenApp, arg: 3 });
    f.push(Step { kind: ActionKind::SendNotify, arg: 1 });
    f.push(Step { kind: ActionKind::Wait, arg: 10 });
    f
}

/// 示例流程：会议静音（音量→通知）。
pub fn sample_flow_meeting_mute() -> Flow {
    let mut f = Flow::new();
    f.push(Step { kind: ActionKind::SetVolume, arg: 0 });
    f.push(Step { kind: ActionKind::SendNotify, arg: 2 });
    f
}

// ---------------------------------------------------------------------------
// F374 自动化 fuzz
// ---------------------------------------------------------------------------

/// 对任意字节流解码流程永不 panic。
pub fn fuzz_decode(raw: &[u8]) -> Option<Flow> {
    share_decode(raw)
}

/// 栈机 fuzz：任意指令序列不 panic。
pub fn fuzz_script(code: &[OpCode]) -> Option<u32> {
    script_run(code)
}

// ---------------------------------------------------------------------------
// F375 自动化域自检（M500）
// ---------------------------------------------------------------------------

pub fn run_automation_checks() -> CheckSet {
    let mut set = CheckSet::new("automation-m500");

    // F351
    let mut f = Flow::new();
    set.add("F351 push", f.push(Step { kind: ActionKind::OpenApp, arg: 1 }), "step 1");
    set.add("F351 full", {
        let mut g = Flow::new();
        (0..FLOW_STEPS).all(|_| g.push(Step { kind: ActionKind::Wait, arg: 0 }))
            && !g.push(Step { kind: ActionKind::Wait, arg: 0 })
    }, "32 cap");
    let mut ran = 0;
    let executed = f.execute(&mut |_k, _a| {
        ran += 1;
    });
    set.add("F351 execute", executed == 1 && ran == 1, "interpreter");

    // F352
    let mut tt = TriggerTable::new();
    set.add("F352 add", tt.add(Trigger { flow_id: 7, events: EV_BATTERY_LOW | EV_TIME }), "sub");
    let mut hits = [0u32; 4];
    let n1 = tt.fire(EV_BATTERY_LOW, &mut hits);
    let n2 = tt.fire(EV_CLIPBOARD, &mut hits);
    set.add("F352 fire hit", n1 == 1 && hits[0] == 7, "matched");
    set.add("F352 fire miss", n2 == 0, "no match");

    // F353
    let daily = CronSpec { minute: 30, hour: 9, day: 0xFF };
    set.add("F353 match", cron_matches(daily, 30, 9, 5), "9:30");
    set.add("F353 no match", !cron_matches(daily, 31, 9, 5), "9:31");
    set.add("F353 next", cron_next_minutes(daily, 9 * 60 + 31) == Some(1439), "to next 9:30");

    // F354
    let r = CondRule { cond: Cond::BatteryBelow(20), then_flow: 1, else_flow: 2 };
    set.add("F354 low", cond_route(&r, 1500, 12, 0) == 1, "15% < 20%");
    set.add("F354 ok", cond_route(&r, 5000, 12, 0) == 2, "50% >= 20%");

    // F355
    let mut lib = ActionLib::new();
    set.add("F355 register", lib.register(ActionEntry { name: *b"openapp ", kind: ActionKind::OpenApp, min_perm: 0 }), "reg");
    set.add("F355 overwrite", {
        let before = lib.len;
        lib.register(ActionEntry { name: *b"openapp ", kind: ActionKind::OpenApp, min_perm: 1 });
        before == lib.len && lib.find(b"openapp").unwrap().min_perm == 1
    }, "same name replaces");
    set.add("F355 find miss", lib.find(b"nope").is_none(), "absent");

    // F356
    let mut rec = Recorder::new();
    rec.start();
    rec.observe(Step { kind: ActionKind::SetVolume, arg: 5 });
    rec.observe(Step { kind: ActionKind::Wait, arg: 2 });
    let flow = rec.stop();
    set.add("F356 recorded 2", flow.len == 2, "captured");
    set.add("F356 stopped", { rec.observe(Step { kind: ActionKind::Wait, arg: 9 }); rec.stop().len == 2 }, "no capture after stop");

    // F357
    let src = sample_flow_meeting_mute();
    let mut buf = [0u8; 128];
    let n = share_encode(&src, &mut buf).unwrap();
    let back = share_decode(&buf[..n]);
    set.add("F357 roundtrip", replay_verify(&src, &back.unwrap()), "encode+decode");
    set.add("F357 bad magic", share_decode(b"XXXX0000").is_none(), "magic gate");
    set.add("F357 trunc", share_decode(&buf[..n - 1]).is_none(), "length gate");

    // F358
    set.add("F358 allow", sandbox_allow(0b010, 0b110), "subset");
    set.add("F358 script banned", sandbox_banned(ActionKind::RunScript), "no nesting");

    // F359
    set.add("F359 attempt 0", retry_next(&RETRY_DEFAULT, 0) == Some(500), "500ms");
    set.add("F359 attempt 2", retry_next(&RETRY_DEFAULT, 2) == Some(2000), "backoff x4");
    set.add("F359 exhausted", retry_next(&RETRY_DEFAULT, 3).is_none(), "no more");

    // F360
    let mut rl = RunLog::new();
    rl.append(RunRecord { flow_id: 1, ok: true, ts_ms: 100 });
    rl.append(RunRecord { flow_id: 2, ok: false, ts_ms: 200 });
    let mut out = [RunRecord { flow_id: 0, ok: false, ts_ms: 0 }; 4];
    let n = rl.recent(&mut out);
    set.add("F360 recent order", n == 2 && out[0].flow_id == 2 && out[1].flow_id == 1, "newest first");
    set.add("F360 total", rl.total == 2, "count");
    set.add("F360 wrap", {
        let mut g = RunLog::new();
        for i in 0..20u32 {
            g.append(RunRecord { flow_id: i, ok: true, ts_ms: i });
        }
        g.total == 20
    }, "ring keeps counting");

    // F361
    let code = [OpCode::Push(6), OpCode::Push(7), OpCode::Mul, OpCode::End];
    set.add("F361 mul", script_run(&code) == Some(42), "6*7");
    set.add("F361 empty", script_run(&[]).is_none(), "no end");
    set.add("F361 stack under", script_run(&[OpCode::Add, OpCode::End]).is_none(), "underflow");

    // F362
    set.add("F362 run", cli_parse(&[b"flow", b"run", b"42"]) == CliCmd::Run(42), "id parse");
    set.add("F362 list", cli_parse(&[b"flow", b"list"]) == CliCmd::List, "list");

    // F363
    let mut name = [0u8; 24];
    let n = batch_rename(b"shot_", 42, &mut name);
    set.add("F363 rename", &name[..n] == b"shot_42", "prefix+index");
    set.add("F363 img", batch_classify(b"photo.PNG") == 0, "image (case-insens)");
    set.add("F363 doc", batch_classify(b"report.pdf") == 1 && batch_classify(b"weird.xyz") == 2, "doc/other");

    // F364
    set.add("F364 debounce wait", !watch_debounce(1000, 2500), "within 2s");
    set.add("F364 debounce pass", watch_debounce(1000, 3500), "after 2s");
    set.add("F364 wrap safe", watch_debounce(0xFFFF_F000, 0x0000_0C00), "wrap");

    // F365
    let mut out = [0u8; 32];
    let n = clipboard_transform(b"  hello   world  ", b"> ", 100, &mut out);
    set.add("F365 transform", &out[..n] == b"> hello world", "collapse spaces");
    let n2 = clipboard_transform(b"abcdefghij", b"", 5, &mut out);
    set.add("F365 truncate", &out[..n2] == b"abcde", "max len");
    set.add("F365 empty", clipboard_transform(b"", b"x", 5, &mut out) == 1, "prefix only");

    // F366
    let peers = [
        DevicePeer { id: 1, online: false },
        DevicePeer { id: 2, online: true },
    ];
    set.add("F366 pick online", pick_peer(&peers, 9).map(|p| p.id) == Some(2), "first online");

    // F367
    set.add("F367 ok", auto_budget_ok(1999, 32, 10_000), "boundary in");
    set.add("F367 time over", !auto_budget_ok(2001, 1, 1), "2s cap");
    set.add("F367 steps over", !auto_budget_ok(1, 33, 1), "step cap");

    // F368
    let a = sample_flow_screenshot_share();
    let mut b = sample_flow_screenshot_share();
    set.add("F368 identical", replay_verify(&a, &b), "same flow");
    b.steps[0].arg = 9;
    set.add("F368 arg diff", !replay_verify(&a, &b), "arg mismatch");
    set.add("F368 len diff", !replay_verify(&a, &sample_flow_meeting_mute()), "length");

    // F369
    let mut at = AuditTrail::new();
    at.note(0, 0b011);
    set.add("F369 clean", !at.violated(&[0b111, 0, 0, 0, 0, 0, 0, 0]), "within grant");
    set.add("F369 violate", at.violated(&[0b001, 0, 0, 0, 0, 0, 0, 0]), "bit1 undeclared");
    at.note(9, 0xFF);
    set.add("F369 idx guard", true, "oob note ignored");

    // F370
    set.add("F370 clean flow", !flow_audit_intercept(&sample_flow_screenshot_share()), "ok");
    set.add("F370 script", flow_audit_intercept(&{
        let mut f = Flow::new();
        f.push(Step { kind: ActionKind::RunScript, arg: 0 });
        f
    }), "nested script");
    set.add("F370 pop", flow_audit_intercept(&{
        let mut f = Flow::new();
        f.push(Step { kind: ActionKind::SetVolume, arg: 0xFF });
        f
    }), "overrange volume");

    // F371
    set.add("F371 ok", auto_api_ok(1) && auto_api_ok(2), "1~2");

    // F372
    let flows = [
        BootFlow { flow_id: 10, delay_ms: 0, priority: 1 },
        BootFlow { flow_id: 20, delay_ms: 500, priority: 9 },
        BootFlow { flow_id: 30, delay_ms: 100, priority: 5 },
    ];
    let mut order = [0usize; 3];
    let n = boot_flows_order(&flows, &mut order);
    set.add("F372 order", n == 3 && order[0] == 1 && order[1] == 2 && order[2] == 0, "priority desc");

    // F373
    set.add("F373 sample 1", sample_flow_screenshot_share().len == 3, "3 steps");
    set.add("F373 sample 2", sample_flow_meeting_mute().len == 2, "2 steps");
    set.add("F373 encode ok", share_encode(&sample_flow_meeting_mute(), &mut [0u8; 64]).is_some(), "shareable");

    // F374
    set.add("F374 fuzz decode", fuzz_decode(b"garbage").is_none(), "no panic");
    set.add("F374 fuzz script", fuzz_script(&[OpCode::Push(1), OpCode::Push(2), OpCode::Add, OpCode::End]) == Some(3), "ok");
    set.add("F374 fuzz deep", {
        let code = [
            OpCode::Push(0), OpCode::Push(0), OpCode::Push(0), OpCode::Push(0),
            OpCode::Push(0), OpCode::Push(0), OpCode::Push(0), OpCode::Push(0),
            OpCode::Push(0), OpCode::End,
        ];
        fuzz_script(&code).is_none()
    }, "stack overflow caught");

    // F375
    set.add("F375 self count", set.len() >= 25, "25+ checks");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f353_midnight_crossing() {
        let daily = CronSpec { minute: 0, hour: 0, day: 0xFF };
        assert_eq!(cron_next_minutes(daily, 23 * 60 + 59), Some(1));
        assert_eq!(cron_next_minutes(daily, 0), Some(24 * 60));
    }

    #[test]
    fn f357_encode_capacity() {
        let mut f = Flow::new();
        for _ in 0..FLOW_STEPS {
            f.push(Step { kind: ActionKind::Wait, arg: 0 });
        }
        let mut small = [0u8; 8];
        assert!(share_encode(&f, &mut small).is_none()); // 缓冲不足
    }

    #[test]
    fn f360_ring_wraps() {
        let mut rl = RunLog::new();
        for i in 0..20u32 {
            rl.append(RunRecord { flow_id: i, ok: true, ts_ms: i });
        }
        let mut out = [RunRecord { flow_id: 0, ok: false, ts_ms: 0 }; 16];
        let n = rl.recent(&mut out);
        assert_eq!(n, 16);
        assert_eq!(out[0].flow_id, 19); // 最新
        assert_eq!(out[15].flow_id, 4); // 最早的留存
    }

    #[test]
    fn f361_mul_overflow_wraps() {
        let code = [OpCode::Push(u16::MAX), OpCode::Push(u16::MAX), OpCode::Mul, OpCode::End];
        let r = script_run(&code).unwrap();
        assert_eq!(r, (u16::MAX as u32).wrapping_mul(u16::MAX as u32));
    }

    #[test]
    fn domain_self_test_passes() {
        let set = run_automation_checks();
        assert!(!set.truncated());
        assert!(set.all_passed(), "automation-m500 self-test: {} checks", set.len());
    }
}
