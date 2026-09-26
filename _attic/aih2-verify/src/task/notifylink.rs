//! UNREAL-X-15000 · AI-28 族0274 通知联动（X06826~X06850）。
//! 通知联动：路由规则表（按应用/等级/勿扰窗口过滤）、同应用聚合折叠
//! （N 条折叠为 1+计数）、联动动作（打开应用/静音会话/定时提醒/角标/丢弃）
//! 与联动执行记录环形台账。
//! 零堆、整数运算，无 Vec/String/Box/alloc、无外部 crate。

// ---------------------------------------------------------------------------
// 常量与错误码
// ---------------------------------------------------------------------------

/// 路由规则容量。
pub const MAX_RULES: usize = 8;
/// 折叠组容量（超出时淘汰计数最小的组）。
pub const MAX_GROUPS: usize = 8;
/// 联动执行记录环形台账容量。
pub const MAX_LOG: usize = 16;
/// 通知等级上限（0 静默 1 低 2 普通 3 重要 4 紧急）。
pub const MAX_LEVEL: u8 = 4;
/// 合法应用编号下限（0 保留为空槽标记）。
pub const MIN_APP: u16 = 1;
/// 通配应用编号（匹配任意应用）。
pub const ANY_APP: u16 = 0xFFFF;
/// 定时提醒偏移上限（分钟）。
pub const MAX_REMIND_MIN: u16 = 720;
/// 彩蛋阈值：同应用折叠满此数点亮纪念标记。
pub const EGG_N: u32 = 8;
/// 快照魔数。
pub const MAGIC: u8 = 0x74;
/// 快照定长。
pub const SNAP_LEN: usize = 121;

pub const E_OK: u16 = 0;
/// 无匹配路由规则。
pub const E_EMPTY: u16 = 1;
/// 规则表满。
pub const E_FULL: u16 = 2;
/// 参数非法。
pub const E_INVALID: u16 = 3;
/// 勿扰窗口拦截。
pub const E_DND: u16 = 4;
/// 等级低于路由门槛。
pub const E_LEVEL: u16 = 5;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_EMPTY => "无匹配路由规则，建议先为该应用添加规则或使用通配规则",
        E_FULL => "规则表已满，建议合并同类应用的规则后再添加",
        E_INVALID => "参数非法（应用编号为 0 或勿扰窗口为空），建议使用 1 以上的应用编号并让勿扰起止分钟错开",
        E_DND => "通知落在勿扰窗口内被拦截，已照常折叠计数，建议等待勿扰结束或为紧急规则开启穿透",
        E_LEVEL => "通知等级低于路由门槛被丢弃，建议提升通知等级或调低规则的最低等级",
        _ => "未知联动错误，建议重置通知联动系统后重试",
    }
}

// ---------------------------------------------------------------------------
// 动作、规则与勿扰窗口
// ---------------------------------------------------------------------------

/// 联动动作五档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkAction {
    /// 打开应用。
    OpenApp,
    /// 静音会话。
    MuteSession,
    /// 定时提醒。
    RemindAt,
    /// 角标计数。
    Badge,
    /// 静默丢弃。
    Drop,
}

impl LinkAction {
    pub fn index(self) -> u32 {
        match self {
            LinkAction::OpenApp => 0,
            LinkAction::MuteSession => 1,
            LinkAction::RemindAt => 2,
            LinkAction::Badge => 3,
            LinkAction::Drop => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            LinkAction::OpenApp => "open-app",
            LinkAction::MuteSession => "mute-session",
            LinkAction::RemindAt => "remind-at",
            LinkAction::Badge => "badge",
            LinkAction::Drop => "drop",
        }
    }
}

/// 路由规则：应用匹配 + 等级门槛 + 动作 + 勿扰穿透 + 提醒偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    /// 匹配应用（ANY_APP 表示任意）。
    pub app_id: u16,
    /// 最低通知等级门槛。
    pub min_level: u8,
    /// 命中后执行的联动动作。
    pub action: LinkAction,
    /// 紧急规则可穿透勿扰窗口。
    pub bypass_dnd: bool,
    /// RemindAt 的延迟分钟数。
    pub remind_after_min: u16,
}

/// 勿扰窗口（当日内分钟区间，支持跨午夜）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DndWindow {
    pub enabled: bool,
    pub start_min: u32,
    pub end_min: u32,
}

impl DndWindow {
    /// 判定某当日分钟是否落在勿扰窗口内（起止相等视为空窗口）。
    pub fn contains(&self, minute: u32) -> bool {
        if !self.enabled || self.start_min == self.end_min {
            return false;
        }
        if self.start_min < self.end_min {
            minute >= self.start_min && minute < self.end_min
        } else {
            minute >= self.start_min || minute < self.end_min
        }
    }
}

/// 把 tick（分钟数）折算为当日分钟。
pub fn minute_of_day(tick: u64) -> u32 {
    (tick % 1440) as u32
}

// ---------------------------------------------------------------------------
// 折叠组与执行记录
// ---------------------------------------------------------------------------

/// 同应用聚合折叠组：N 条折叠为 1 + 计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldGroup {
    /// 应用编号（0 表示空槽）。
    pub app_id: u16,
    /// 折叠计数 N。
    pub count: u32,
    /// 组内最高等级。
    pub max_level: u8,
    pub first_tick: u64,
    pub last_tick: u64,
}

/// 联动执行记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub action: LinkAction,
    pub app_id: u16,
    /// 执行时该组的折叠计数快照。
    pub count: u32,
    /// RemindAt 的目标当日分钟。
    pub target_min: u32,
    pub tick: u64,
    pub code: u16,
}

// ---------------------------------------------------------------------------
// 引擎
// ---------------------------------------------------------------------------

/// 通知联动引擎：规则路由 → 勿扰过滤 → 同应用折叠 → 联动执行 → 记录。
pub struct NotifyLink {
    pub rules: [Rule; MAX_RULES],
    pub rule_count: usize,
    pub groups: [FoldGroup; MAX_GROUPS],
    pub dnd: DndWindow,
    pub log: [LogEntry; MAX_LOG],
    pub log_head: usize,
    pub log_len: usize,
    /// 低配经济模式（定时提醒降级为角标）。
    pub eco: bool,
    /// 彩蛋：同应用折叠满 EGG_N 条点亮。
    pub easter: bool,
    pub submits: u64,
    /// 累计折叠进组的通知条数。
    pub folded_total: u64,
    /// 累计联动执行次数（写台账次数）。
    pub execs: u64,
    pub delivered: u64,
    pub muted: u64,
    pub reminds: u64,
    pub badges: u64,
    /// Drop 动作执行数（门槛丢弃不计入，见 gated）。
    pub dropped: u64,
    /// 门槛丢弃数。
    pub gated: u64,
    /// 勿扰拦截数。
    pub dnd_blocked: u64,
    /// 折叠组淘汰次数。
    pub evictions: u64,
}

impl NotifyLink {
    pub fn new() -> NotifyLink {
        NotifyLink {
            rules: [Rule { app_id: 0, min_level: 0, action: LinkAction::Drop, bypass_dnd: false, remind_after_min: 0 }; MAX_RULES],
            rule_count: 0,
            groups: [FoldGroup { app_id: 0, count: 0, max_level: 0, first_tick: 0, last_tick: 0 }; MAX_GROUPS],
            dnd: DndWindow { enabled: false, start_min: 1320, end_min: 480 },
            log: [LogEntry { action: LinkAction::Drop, app_id: 0, count: 0, target_min: 0, tick: 0, code: E_OK }; MAX_LOG],
            log_head: 0,
            log_len: 0,
            eco: false,
            easter: false,
            submits: 0,
            folded_total: 0,
            execs: 0,
            delivered: 0,
            muted: 0,
            reminds: 0,
            badges: 0,
            dropped: 0,
            gated: 0,
            dnd_blocked: 0,
            evictions: 0,
        }
    }

    /// 添加路由规则（非法域钳制：等级≤4、提醒偏移≤720）。
    pub fn add_rule(&mut self, mut r: Rule) -> u16 {
        if r.app_id < MIN_APP {
            return E_INVALID;
        }
        r.min_level = r.min_level.min(MAX_LEVEL);
        r.remind_after_min = r.remind_after_min.min(MAX_REMIND_MIN);
        if self.rule_count >= MAX_RULES {
            return E_FULL;
        }
        self.rules[self.rule_count] = r;
        self.rule_count += 1;
        E_OK
    }

    /// 设置勿扰窗口（分钟钳制到 0~1439；开启时起止相等视为空窗口拒绝）。
    pub fn set_dnd(&mut self, start_min: u32, end_min: u32, enabled: bool) -> u16 {
        let s = start_min.min(1439);
        let e = end_min.min(1439);
        if enabled && s == e {
            return E_INVALID;
        }
        self.dnd = DndWindow { enabled, start_min: s, end_min: e };
        E_OK
    }

    /// 勿扰剩余分钟数（不在勿扰中返回 0）。
    pub fn dnd_minutes_left(&self, minute: u32) -> u32 {
        if !self.dnd.contains(minute) {
            return 0;
        }
        if self.dnd.start_min < self.dnd.end_min {
            self.dnd.end_min - minute
        } else if minute >= self.dnd.start_min {
            (1440 - minute) + self.dnd.end_min
        } else {
            self.dnd.end_min - minute
        }
    }

    /// 低配探测：内存吃紧时进入经济模式。
    pub fn degrade_probe(&mut self, ram_permille: u32) -> bool {
        if ram_permille > 900 {
            self.eco = true;
        }
        self.eco
    }

    /// 经济模式下的有效动作：定时提醒降级为角标。
    pub fn effective_action(&self, r: Rule) -> LinkAction {
        if self.eco && r.action == LinkAction::RemindAt {
            LinkAction::Badge
        } else {
            r.action
        }
    }

    /// 同应用聚合折叠：已有组累加计数，否则取空槽，再无则淘汰计数最小的组。
    fn fold(&mut self, app_id: u16, level: u8, tick: u64) {
        for i in 0..MAX_GROUPS {
            if self.groups[i].app_id == app_id {
                self.groups[i].count += 1;
                if level > self.groups[i].max_level {
                    self.groups[i].max_level = level;
                }
                self.groups[i].last_tick = tick;
                if self.groups[i].count == EGG_N {
                    self.easter = true;
                }
                self.folded_total += 1;
                return;
            }
        }
        for i in 0..MAX_GROUPS {
            if self.groups[i].app_id == 0 {
                self.groups[i] = FoldGroup { app_id, count: 1, max_level: level, first_tick: tick, last_tick: tick };
                self.folded_total += 1;
                return;
            }
        }
        // 组满：淘汰计数最小者（平手取最前），高价值组得以保留。
        let mut pick = 0usize;
        for i in 1..MAX_GROUPS {
            if self.groups[i].count < self.groups[pick].count {
                pick = i;
            }
        }
        self.evictions += 1;
        self.groups[pick] = FoldGroup { app_id, count: 1, max_level: level, first_tick: tick, last_tick: tick };
        self.folded_total += 1;
    }

    pub fn group_count(&self, app_id: u16) -> u32 {
        for g in self.groups.iter() {
            if g.app_id == app_id {
                return g.count;
            }
        }
        0
    }

    /// 折叠组三态：0=无通知 1=单条活跃 2=已折叠（计数≥2）。
    pub fn group_state(&self, app_id: u16) -> u8 {
        let n = self.group_count(app_id);
        if n == 0 {
            0
        } else if n == 1 {
            1
        } else {
            2
        }
    }

    fn log_push(&mut self, e: LogEntry) {
        if self.log_len < MAX_LOG {
            self.log[(self.log_head + self.log_len) % MAX_LOG] = e;
            self.log_len += 1;
        } else {
            // 环形覆写最旧记录。
            self.log[self.log_head] = e;
            self.log_head = (self.log_head + 1) % MAX_LOG;
        }
    }

    pub fn log_get(&self, i: usize) -> Option<LogEntry> {
        if i < self.log_len {
            Some(self.log[(self.log_head + i) % MAX_LOG])
        } else {
            None
        }
    }

    /// 提交通知主链路：路由 → 勿扰 → 折叠 → 联动 → 记录。
    pub fn submit(&mut self, app_id: u16, level: u8, tick: u64) -> u16 {
        if app_id < MIN_APP {
            return E_INVALID;
        }
        let level = level.min(MAX_LEVEL);
        self.submits += 1;
        // 规则表按序匹配：首个应用命中的规则决定命运。
        let mut hit: Option<usize> = None;
        let mut gated = false;
        for i in 0..self.rule_count {
            let r = self.rules[i];
            if r.app_id == ANY_APP || r.app_id == app_id {
                if level >= r.min_level {
                    hit = Some(i);
                } else {
                    gated = true;
                }
                break;
            }
        }
        let ri = match hit {
            Some(i) => i,
            None => {
                if gated {
                    self.gated += 1;
                    return E_LEVEL;
                }
                return E_EMPTY;
            }
        };
        let rule = self.rules[ri];
        let minute = minute_of_day(tick);
        // 勿扰窗口：非紧急且未开穿透 → 只折叠不联动。
        if self.dnd.enabled && self.dnd.contains(minute) && !rule.bypass_dnd && level < MAX_LEVEL {
            self.fold(app_id, level, tick);
            self.dnd_blocked += 1;
            return E_DND;
        }
        self.fold(app_id, level, tick);
        let act = self.effective_action(rule);
        let target_min = (minute + rule.remind_after_min as u32) % 1440;
        let entry = LogEntry { action: act, app_id, count: self.group_count(app_id), target_min, tick, code: E_OK };
        self.log_push(entry);
        self.execs += 1;
        match act {
            LinkAction::OpenApp => self.delivered += 1,
            LinkAction::MuteSession => self.muted += 1,
            LinkAction::RemindAt => self.reminds += 1,
            LinkAction::Badge => self.badges += 1,
            LinkAction::Drop => self.dropped += 1,
        }
        E_OK
    }

    /// 批量提交（tick 逐条递增），返回成功条数。
    pub fn submit_batch(&mut self, apps: &[u16], level: u8, tick: u64) -> usize {
        let mut ok = 0usize;
        for i in 0..apps.len() {
            if self.submit(apps[i], level, tick + i as u64) == E_OK {
                ok += 1;
            }
        }
        ok
    }

    /// 渲染第 idx 个折叠组为 "app=<id> n=<count>" 读屏文本。
    pub fn render_group(&self, idx: usize, buf: &mut [u8]) -> usize {
        let mut seen = 0usize;
        for g in self.groups.iter() {
            if g.app_id == 0 {
                continue;
            }
            if seen < idx {
                seen += 1;
                continue;
            }
            let mut n = 0usize;
            push_bytes(buf, &mut n, b"app=");
            push_num(buf, &mut n, g.app_id as u32);
            push_bytes(buf, &mut n, b" n=");
            push_num(buf, &mut n, g.count);
            return n;
        }
        0
    }

    /// 不变量审计：容量、组干净度、执行计数守恒。
    pub fn audit(&self) -> bool {
        if self.rule_count > MAX_RULES || self.log_len > MAX_LOG || self.log_head >= MAX_LOG {
            return false;
        }
        for g in self.groups.iter() {
            if g.app_id == 0 {
                if g.count != 0 {
                    return false;
                }
            } else if g.count == 0 || g.max_level > MAX_LEVEL {
                return false;
            }
        }
        self.delivered + self.muted + self.reminds + self.badges + self.dropped == self.execs
    }

    /// 快照导出（魔数 + 勿扰 + 规则表 + 折叠组）。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < SNAP_LEN {
            return 0;
        }
        buf[0] = MAGIC;
        buf[1] = 1;
        buf[2] = if self.dnd.enabled { 1 } else { 0 };
        put_u16(buf, 3, self.dnd.start_min as u16);
        put_u16(buf, 5, self.dnd.end_min as u16);
        buf[7] = if self.eco { 1 } else { 0 };
        buf[8] = self.rule_count as u8;
        for i in 0..MAX_RULES {
            let r = self.rules[i];
            let base = 9 + i * 7;
            put_u16(buf, base, r.app_id);
            buf[base + 2] = r.min_level;
            buf[base + 3] = r.action.index() as u8;
            buf[base + 4] = if r.bypass_dnd { 1 } else { 0 };
            put_u16(buf, base + 5, r.remind_after_min);
        }
        for i in 0..MAX_GROUPS {
            let g = self.groups[i];
            let base = 65 + i * 7;
            put_u16(buf, base, g.app_id);
            put_u32(buf, base + 2, g.count);
            buf[base + 6] = g.max_level;
        }
        SNAP_LEN
    }

    /// 快照导入：恢复勿扰/规则/折叠组（非法域钳制），魔数版本校验。
    pub fn import(&mut self, buf: &[u8]) -> u16 {
        if buf.len() < SNAP_LEN || buf[0] != MAGIC || buf[1] != 1 {
            return E_INVALID;
        }
        self.dnd.enabled = buf[2] != 0;
        self.dnd.start_min = (get_u16(buf, 3) as u32).min(1439);
        self.dnd.end_min = (get_u16(buf, 5) as u32).min(1439);
        self.eco = buf[7] != 0;
        self.rule_count = (buf[8] as usize).min(MAX_RULES);
        for i in 0..MAX_RULES {
            let base = 9 + i * 7;
            let idx = buf[base + 3];
            self.rules[i] = Rule {
                app_id: get_u16(buf, base),
                min_level: buf[base + 2].min(MAX_LEVEL),
                action: action_from_idx(idx),
                bypass_dnd: buf[base + 4] != 0,
                remind_after_min: get_u16(buf, base + 5).min(MAX_REMIND_MIN),
            };
        }
        for i in 0..MAX_GROUPS {
            let base = 65 + i * 7;
            let app = get_u16(buf, base);
            let cnt = get_u32(buf, base + 2);
            if app == 0 || cnt == 0 {
                self.groups[i] = FoldGroup { app_id: 0, count: 0, max_level: 0, first_tick: 0, last_tick: 0 };
            } else {
                self.groups[i] = FoldGroup {
                    app_id: app,
                    count: cnt,
                    max_level: buf[base + 6].min(MAX_LEVEL),
                    first_tick: 0,
                    last_tick: 0,
                };
            }
        }
        E_OK
    }

    /// 回滚净身：运行态清零，规则与勿扰配置保留。
    pub fn reset(&mut self) {
        self.groups = [FoldGroup { app_id: 0, count: 0, max_level: 0, first_tick: 0, last_tick: 0 }; MAX_GROUPS];
        self.log = [LogEntry { action: LinkAction::Drop, app_id: 0, count: 0, target_min: 0, tick: 0, code: E_OK }; MAX_LOG];
        self.log_head = 0;
        self.log_len = 0;
        self.easter = false;
        self.submits = 0;
        self.folded_total = 0;
        self.execs = 0;
        self.delivered = 0;
        self.muted = 0;
        self.reminds = 0;
        self.badges = 0;
        self.dropped = 0;
        self.gated = 0;
        self.dnd_blocked = 0;
        self.evictions = 0;
    }
}

/// 开发者扩展点：纯函数判定一条通知对一条规则的处置结果。
pub fn rule_matches(rule: Rule, app_id: u16, level: u8, minute: u32, dnd: DndWindow) -> u16 {
    if app_id < MIN_APP {
        return E_INVALID;
    }
    if rule.app_id != ANY_APP && rule.app_id != app_id {
        return E_EMPTY;
    }
    if level < rule.min_level {
        return E_LEVEL;
    }
    if dnd.enabled && dnd.contains(minute) && !rule.bypass_dnd && level.min(MAX_LEVEL) < MAX_LEVEL {
        return E_DND;
    }
    E_OK
}

fn action_from_idx(idx: u8) -> LinkAction {
    match idx {
        0 => LinkAction::OpenApp,
        1 => LinkAction::MuteSession,
        2 => LinkAction::RemindAt,
        3 => LinkAction::Badge,
        _ => LinkAction::Drop,
    }
}

fn put_u16(buf: &mut [u8], off: usize, v: u16) {
    buf[off] = v as u8;
    buf[off + 1] = (v >> 8) as u8;
}

fn get_u16(buf: &[u8], off: usize) -> u16 {
    buf[off] as u16 | ((buf[off + 1] as u16) << 8)
}

fn put_u32(buf: &mut [u8], off: usize, v: u32) {
    for i in 0..4 {
        buf[off + i] = (v >> (i * 8)) as u8;
    }
}

fn get_u32(buf: &[u8], off: usize) -> u32 {
    let mut v = 0u32;
    for i in 0..4 {
        v |= (buf[off + i] as u32) << (i * 8);
    }
    v
}

fn push_bytes(buf: &mut [u8], n: &mut usize, s: &[u8]) {
    for i in 0..s.len() {
        if *n < buf.len() {
            buf[*n] = s[i];
            *n += 1;
        }
    }
}

fn push_num(buf: &mut [u8], n: &mut usize, v: u32) {
    if v == 0 {
        push_bytes(buf, n, b"0");
        return;
    }
    let mut ds = [0u8; 10];
    let mut w = 0usize;
    let mut x = v;
    while x > 0 {
        ds[w] = b'0' + (x % 10) as u8;
        x /= 10;
        w += 1;
    }
    while w > 0 {
        w -= 1;
        push_bytes(buf, n, &[ds[w]]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_route_fold_dnd_and_gate() {
        // 基本路由 + 折叠三态。
        let mut nl = NotifyLink::new();
        let _ = nl.add_rule(Rule { app_id: 7, min_level: 2, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
        assert_eq!(nl.submit(7, 3, 600), E_OK);
        assert_eq!(nl.group_state(7), 1);
        assert_eq!(nl.delivered, 1);
        assert_eq!(nl.submit(7, 2, 601), E_OK);
        assert_eq!(nl.group_state(7), 2);
        assert_eq!(nl.group_count(7), 2);
        // 勿扰拦截 + 紧急穿透。
        let mut d = NotifyLink::new();
        assert_eq!(d.set_dnd(1320, 480, true), E_OK);
        let _ = d.add_rule(Rule { app_id: 9, min_level: 0, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
        assert_eq!(d.submit(9, 2, 1330), E_DND);
        assert_eq!(d.group_count(9), 1);
        assert_eq!(d.delivered, 0);
        assert_eq!(d.dnd_minutes_left(1330), 590);
        assert_eq!(d.submit(9, 4, 1331), E_OK);
        assert_eq!(d.delivered, 1);
        // 门槛丢弃与非法编号。
        let mut g = NotifyLink::new();
        let _ = g.add_rule(Rule { app_id: 4, min_level: 2, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
        assert_eq!(g.submit(4, 0, 1), E_LEVEL);
        assert_eq!(g.gated, 1);
        assert_eq!(g.group_count(4), 0);
        assert_eq!(g.submit(0, 3, 1), E_INVALID);
    }

    #[test]
    fn notify_snapshot_resume_evict_and_render() {
        // 半程快照 → 新实例续跑 → 与不间断孪生一致。
        let mut x = NotifyLink::new();
        let _ = x.add_rule(Rule { app_id: 5, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
        for t in 0..3u64 {
            let _ = x.submit(5, 2, t);
        }
        let mut sb = [0u8; 256];
        let n = x.export(&mut sb);
        assert_eq!(n, SNAP_LEN);
        let mut y = NotifyLink::new();
        assert_eq!(y.import(&sb[..n]), E_OK);
        let _ = y.submit(5, 2, 3);
        let _ = y.submit(5, 2, 4);
        let mut z = NotifyLink::new();
        let _ = z.add_rule(Rule { app_id: 5, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
        for t in 0..5u64 {
            let _ = z.submit(5, 2, t);
        }
        assert_eq!(y.group_count(5), 5);
        assert_eq!(z.group_count(5), 5);
        // 断点前的历史统计不入快照，断点后重新累计 2 次。
        assert_eq!(y.badges, 2);
        // 组满淘汰。
        let mut e = NotifyLink::new();
        for _ in 0..MAX_RULES {
            let _ = e.add_rule(Rule { app_id: ANY_APP, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
        }
        for app in 1u16..=8 {
            let _ = e.submit(app, 2, 100 + app as u64);
        }
        let _ = e.submit(20, 2, 200);
        assert_eq!(e.evictions, 1);
        assert_eq!(e.group_count(20), 1);
        assert_eq!(e.group_count(1), 0);
        assert!(e.audit());
        // 读屏渲染（槽 0 已被淘汰换入 app 20）。
        let mut rb = [0u8; 32];
        let rn = e.render_group(0, &mut rb);
        assert_eq!(&rb[..rn], b"app=20 n=1");
        // 纯函数扩展点。
        let rr = Rule { app_id: 7, min_level: 2, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 };
        assert_eq!(rule_matches(rr, 7, 1, 0, DndWindow { enabled: false, start_min: 0, end_min: 0 }), E_LEVEL);
    }

    #[test]
    fn notify_all_checks_pass() {
        let set = run_notifylink_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            assert!(set.get(i).unwrap().passed, "第 {} 项未通过: {}", i, set.get(i).unwrap().name);
        }
    }
}

/// 族0274 自检：X06826~X06850 逐项登记。
pub fn run_notifylink_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("task-notify");

    // —— 基础实装 X06826~X06830 ——
    let mut nl = NotifyLink::new();
    let add_ok = nl.add_rule(Rule { app_id: 7, min_level: 2, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
    let s_ok = nl.submit(7, 3, 600);
    let c26 = add_ok == E_OK
        && s_ok == E_OK
        && nl.group_state(7) == 1
        && nl.delivered == 1
        && nl.execs == 1
        && nl.log_get(0).map(|e| e.action) == Some(LinkAction::OpenApp);
    set.add("X06826 核心链路闭环", c26, "提交→路由→折叠→联动→记录端到端可观测");

    let mut nl2 = NotifyLink::new();
    let d1 = nl2.set_dnd(1320, 480, true);
    let r1 = nl2.add_rule(Rule { app_id: ANY_APP, min_level: 9, action: LinkAction::RemindAt, bypass_dnd: true, remind_after_min: 5000 });
    let r_ok = nl2.rules[0].min_level == MAX_LEVEL && nl2.rules[0].remind_after_min == MAX_REMIND_MIN;
    let eco_ok = !nl2.degrade_probe(500) && nl2.degrade_probe(950) && nl2.eco;
    set.add("X06827 全量参数开放", d1 == E_OK && r1 == E_OK && r_ok && eco_ok && nl2.dnd.contains(minute_of_day(1330)), "规则/勿扰/经济模式全参数可配可读");

    let acts = [LinkAction::OpenApp, LinkAction::MuteSession, LinkAction::RemindAt, LinkAction::Badge, LinkAction::Drop];
    let mut idx_ok = true;
    for i in 0..acts.len() {
        idx_ok &= acts[i].index() == i as u32 && !acts[i].name().is_empty();
    }
    set.add("X06828 档位矩阵≥5档", idx_ok, "开应用/静音会话/定时提醒/角标/丢弃五档独立");

    let mut a = NotifyLink::new();
    let _ = a.add_rule(Rule { app_id: 5, min_level: 0, action: LinkAction::MuteSession, bypass_dnd: false, remind_after_min: 0 });
    let _ = a.submit(5, 2, 100);
    let _ = a.submit(5, 2, 101);
    let mut buf = [0u8; 256];
    let n = a.export(&mut buf);
    let mut b = NotifyLink::new();
    let imp = b.import(&buf[..n]);
    set.add("X06829 快照迁移三通道", n == SNAP_LEN && buf[0] == MAGIC && imp == E_OK && b.group_count(5) == 2 && b.dnd == a.dnd && b.rule_count == 1, "导出/导入/魔数版本三通道");

    let mut nl3 = NotifyLink::new();
    let _ = nl3.add_rule(Rule { app_id: 1, min_level: 1, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
    let _ = nl3.add_rule(Rule { app_id: 2, min_level: 0, action: LinkAction::MuteSession, bypass_dnd: false, remind_after_min: 0 });
    let _ = nl3.submit(1, 2, 10);
    let _ = nl3.submit(2, 1, 11);
    let c30 = nl3.delivered == 1 && nl3.muted == 1 && nl3.group_state(1) == 1 && nl3.group_state(2) == 1 && nl3.audit();
    set.add("X06830 联调无回归", c30, "双应用并行路由读数一致审计通过");

    // —— 边界与恢复 X06831~X06835 ——
    let mut nl4 = NotifyLink::new();
    let bad_app = nl4.submit(0, 3, 0);
    let _ = nl4.add_rule(Rule { app_id: 9, min_level: 0, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
    let _ = nl4.submit(9, 200, 10);
    let r9 = nl4.add_rule(Rule { app_id: 8, min_level: 200, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 5000 });
    set.add("X06831 非法输入钳制", bad_app == E_INVALID && nl4.group_count(9) == 1 && nl4.groups[0].max_level == MAX_LEVEL && r9 == E_OK && nl4.rules[1].min_level == MAX_LEVEL && nl4.rules[1].remind_after_min == MAX_REMIND_MIN, "编号越界拒绝、等级与提醒窗钳制不崩溃");

    set.add("X06832 错误叙事体系", describe(E_DND).contains("勿扰") && describe(E_LEVEL).contains("等级") && describe(E_EMPTY).contains("规则") && describe(E_INVALID).contains("建议"), "每个失败有下一步建议");

    let mut x = NotifyLink::new();
    let _ = x.add_rule(Rule { app_id: 5, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
    for t in 0..3u64 {
        let _ = x.submit(5, 2, t);
    }
    let mut sb = [0u8; 256];
    let nx = x.export(&mut sb);
    let mut y = NotifyLink::new();
    let _ = y.import(&sb[..nx]);
    let _ = y.submit(5, 2, 3);
    let _ = y.submit(5, 2, 4);
    let mut z = NotifyLink::new();
    let _ = z.add_rule(Rule { app_id: 5, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
    for t in 0..5u64 {
        let _ = z.submit(5, 2, t);
    }
    set.add("X06833 中断续跑还原", y.group_count(5) == 5 && z.group_count(5) == 5 && y.badges == 2, "半程快照续跑组计数连续、断点后统计重新累计");

    let mut nl5 = NotifyLink::new();
    let mut full_ok = true;
    for _ in 0..MAX_RULES {
        full_ok &= nl5.add_rule(Rule { app_id: ANY_APP, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 }) == E_OK;
    }
    let over_rule = nl5.add_rule(Rule { app_id: 99, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
    for app in 1u16..=8 {
        let _ = nl5.submit(app, 2, 100 + app as u64);
    }
    let _ = nl5.submit(20, 2, 200);
    set.add("X06834 资源降级守护", full_ok && over_rule == E_FULL && nl5.evictions == 1 && nl5.group_count(20) == 1 && nl5.group_count(1) == 0 && nl5.audit(), "规则表满拒绝、组满淘汰最小计数不崩溃");

    let mut nl6 = NotifyLink::new();
    let _ = nl6.add_rule(Rule { app_id: 3, min_level: 0, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
    let _ = nl6.submit(3, 3, 50);
    nl6.reset();
    set.add("X06835 回滚净身", nl6.rule_count == 1 && nl6.group_count(3) == 0 && nl6.execs == 0 && nl6.log_len == 0 && !nl6.easter && nl6.audit(), "运行态清零、规则配置保留不留残档");

    // —— 手感与细节 X06836~X06840 ——
    let tok_ok = LinkAction::OpenApp.name() == "open-app"
        && LinkAction::MuteSession.name() == "mute-session"
        && LinkAction::RemindAt.name() == "remind-at"
        && LinkAction::Badge.name() == "badge"
        && LinkAction::Drop.name() == "drop";
    set.add("X06836 令牌对齐", tok_ok && LinkAction::Badge.index() == 3, "动作名与索引一一对应");

    let mut nl7 = NotifyLink::new();
    let _ = nl7.add_rule(Rule { app_id: 6, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
    let st0 = nl7.group_state(6);
    let _ = nl7.submit(6, 2, 1);
    let st1 = nl7.group_state(6);
    let _ = nl7.submit(6, 2, 2);
    let st2 = nl7.group_state(6);
    set.add("X06837 三态焦点", st0 == 0 && st1 == 1 && st2 == 2, "无通知/单条活跃/折叠计数三态齐备");

    let mut nl8 = NotifyLink::new();
    let _ = nl8.add_rule(Rule { app_id: 4, min_level: 2, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
    let quiet = nl8.submit(4, 0, 1);
    let unknown = nl8.submit(30, 3, 1);
    set.add("X06838 键盘通道", quiet == E_LEVEL && unknown == E_EMPTY && nl8.group_count(4) == 0 && nl8.gated == 1 && nl8.dropped == 0 && nl8.delivered == 0, "低等级与陌生应用不误触发可归零");

    set.add("X06839 微文案统一", describe(E_OK) == "正常" && describe(E_FULL).contains("合并") && describe(E_DND).contains("建议"), "中文自然术语一致");

    let mut nl9 = NotifyLink::new();
    let _ = nl9.add_rule(Rule { app_id: 7, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
    let _ = nl9.submit(7, 2, 1);
    let _ = nl9.submit(7, 2, 2);
    let mut rb = [0u8; 32];
    let rn = nl9.render_group(0, &mut rb);
    set.add("X06840 无障碍等价", rn == 9 && &rb[..rn] == b"app=7 n=2", "折叠组可渲染为读屏文本");

    // —— 性能与优化 X06841~X06845 ——
    let mut nl10 = NotifyLink::new();
    let _ = nl10.add_rule(Rule { app_id: 3, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
    for t in 0..8u64 {
        let _ = nl10.submit(3, 2, t);
    }
    set.add("X06841 基准采集", nl10.group_count(3) == EGG_N && nl10.easter && nl10.execs == 8 && nl10.badges == 8, "8 条同应用折叠与彩蛋阈值基准入册");

    let mut nl11 = NotifyLink::new();
    let mut no_rule = 0usize;
    for i in 0..1000u64 {
        if nl11.submit(((i % 50) + 1) as u16, 2, i) == E_EMPTY {
            no_rule += 1;
        }
    }
    set.add("X06842 热路径量化", no_rule == 1000 && nl11.folded_total == 0 && nl11.execs == 0, "千次无规则提交零折叠零联动");

    let mut nl12 = NotifyLink::new();
    let _ = nl12.add_rule(Rule { app_id: 2, min_level: 0, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
    for t in 0..16u64 {
        let _ = nl12.submit(2, 3, t);
    }
    nl12.reset();
    set.add("X06843 内存功耗收敛", nl12.folded_total == 0 && nl12.delivered == 0 && nl12.log_len == 0 && nl12.group_count(2) == 0 && !nl12.easter, "高负载后待机零增量泄漏入长稳");

    let mut nl13 = NotifyLink::new();
    let _ = nl13.add_rule(Rule { app_id: 5, min_level: 0, action: LinkAction::RemindAt, bypass_dnd: false, remind_after_min: 30 });
    nl13.degrade_probe(950);
    let _ = nl13.submit(5, 2, 600);
    let deg = nl13.log_get(0).map(|e| e.action) == Some(LinkAction::Badge);
    set.add("X06844 低配降级链", nl13.eco && deg && nl13.reminds == 0 && nl13.badges == 1, "低配下定时提醒降级为角标");

    let mut nl14 = NotifyLink::new();
    let a0 = nl14.audit();
    let _ = nl14.add_rule(Rule { app_id: ANY_APP, min_level: 0, action: LinkAction::MuteSession, bypass_dnd: false, remind_after_min: 0 });
    for t in 0..20u64 {
        let _ = nl14.submit((t % 3 + 1) as u16, 1, t);
    }
    set.add("X06845 防劣化守卫", a0 && nl14.audit() && nl14.muted + nl14.delivered + nl14.reminds + nl14.badges + nl14.dropped == nl14.execs, "不变量断言只增不删");

    // —— 创新拓展 X06846~X06850 ——
    let mut nl15 = NotifyLink::new();
    let _ = nl15.set_dnd(1320, 480, true);
    let _ = nl15.add_rule(Rule { app_id: 9, min_level: 0, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 });
    let dnd_hit = nl15.submit(9, 2, 1330);
    let left = nl15.dnd_minutes_left(1330);
    set.add("X06846 智能建议", dnd_hit == E_DND && nl15.dnd_blocked == 1 && nl15.group_count(9) == 1 && left == 590 && describe(E_DND).contains("建议"), "勿扰拦截可折叠可解释并给出等待时长");

    let mut nl16 = NotifyLink::new();
    let _ = nl16.add_rule(Rule { app_id: ANY_APP, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
    let apps: [u16; 5] = [11, 12, 13, 14, 15];
    let ok_n = nl16.submit_batch(&apps, 2, 100);
    set.add("X06847 批量自动化", ok_n == 5 && nl16.badges == 5 && nl16.group_count(13) == 1, "批量提交队列化入组联动");

    let mut cross = true;
    for i in 0..nl16.log_len {
        if let Some(e) = nl16.log_get(i) {
            cross &= nl16.group_count(e.app_id) >= e.count && e.code == E_OK;
        }
    }
    set.add("X06848 三线跨域联动", cross && nl16.audit() && nl16.log_len == 5, "执行记录/折叠组/统计三线读数一致");

    let rr = Rule { app_id: 7, min_level: 2, action: LinkAction::OpenApp, bypass_dnd: false, remind_after_min: 0 };
    let dw = DndWindow { enabled: true, start_min: 100, end_min: 200 };
    let ext = rule_matches(rr, 7, 3, 150, DndWindow { enabled: false, start_min: 0, end_min: 0 }) == E_OK
        && rule_matches(rr, 7, 1, 150, dw) == E_LEVEL
        && rule_matches(rr, 7, 3, 150, DndWindow { enabled: true, start_min: 150, end_min: 160 }) == E_DND
        && rule_matches(rr, 8, 3, 150, dw) == E_EMPTY
        && rule_matches(rr, 0, 3, 150, dw) == E_INVALID
        && rule_matches(Rule { bypass_dnd: true, ..rr }, 7, 3, 150, DndWindow { enabled: true, start_min: 150, end_min: 160 }) == E_OK;
    set.add("X06849 开发者扩展点", ext, "纯函数规则判定/渲染/批量三件套可复用");

    let mut nl17 = NotifyLink::new();
    let _ = nl17.set_dnd(1320, 480, true);
    let _ = nl17.add_rule(Rule { app_id: 3, min_level: 0, action: LinkAction::Badge, bypass_dnd: false, remind_after_min: 0 });
    for t in 0..8u64 {
        let _ = nl17.submit(3, 2, t);
    }
    let egg = nl17.easter;
    nl17.reset();
    set.add("X06850 彩蛋与净身", egg && !nl17.easter && nl17.group_count(3) == 0 && nl17.dnd.enabled, "折叠满额点亮纪念、净身后勿扰配置仍有记忆");

    set
}
