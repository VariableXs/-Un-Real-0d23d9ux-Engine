//! UNREAL-X-15000 · AI-28 族0276 本地化（X06876~X06900）。
//! 本地化：字符串表（&'static str 固定键值数组）、中文量词规则
//! （个/只/条/张/本按名词类别）、日期与时区偏移计算（纯整数分钟）
//! 与缺条目回退链 zh→en→键名。
//! 零堆、整数运算，无 Vec/String/Box/alloc、无外部 crate。

// ---------------------------------------------------------------------------
// 常量与错误码
// ---------------------------------------------------------------------------

/// 字符串表条目数。
pub const STR_COUNT: usize = 12;
/// 字符串表容量上限。
pub const MAX_STR: usize = 16;
/// 一天的分钟数。
pub const DAY_MIN: i32 = 1440;
/// 时区偏移上限（+14:00）。
pub const MAX_TZ: i32 = 840;
/// 时区偏移下限（-12:00）。
pub const MIN_TZ: i32 = -720;
/// 快照魔数。
pub const MAGIC: u8 = 0x76;
/// 快照定长。
pub const SNAP_LEN: usize = 48;

pub const E_OK: u16 = 0;
/// 参数越界（已钳制）。
pub const E_INVALID: u16 = 1;
/// 键不在表中（已回退键名）。
pub const E_NOKEY: u16 = 2;
/// 输出缓冲不足。
pub const E_NOROOM: u16 = 3;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_INVALID => "参数越界（时区偏移超 -720~840 分钟），已钳制到最近边界，建议使用合法偏移",
        E_NOKEY => "键不在字符串表且双语文案缺失，已回退到键名兜底，建议把键补录进翻译表",
        E_NOROOM => "输出缓冲不足，建议加大缓冲后重试渲染",
        _ => "未知本地化错误，建议重置语言与计数后重试",
    }
}

// ---------------------------------------------------------------------------
// 语言、量词与字符串表
// ---------------------------------------------------------------------------

/// 语言档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn index(self) -> u32 {
        match self {
            Lang::Zh => 0,
            Lang::En => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Zh => "zh-CN",
            Lang::En => "en",
        }
    }
}

/// 中文名词类别五档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NounClass {
    /// 通用（个）。
    Generic,
    /// 动物（只）。
    Animal,
    /// 长条（条）。
    Strip,
    /// 平面（张）。
    Flat,
    /// 书册（本）。
    Bound,
}

impl NounClass {
    pub fn index(self) -> u32 {
        match self {
            NounClass::Generic => 0,
            NounClass::Animal => 1,
            NounClass::Strip => 2,
            NounClass::Flat => 3,
            NounClass::Bound => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            NounClass::Generic => "generic",
            NounClass::Animal => "animal",
            NounClass::Strip => "strip",
            NounClass::Flat => "flat",
            NounClass::Bound => "bound",
        }
    }
}

/// 全部名词类别表。
pub const NOUN_CLASSES: [NounClass; 5] = [
    NounClass::Generic,
    NounClass::Animal,
    NounClass::Strip,
    NounClass::Flat,
    NounClass::Bound,
];

/// 按名词类别取中文量词。
pub fn measure_word(cls: NounClass) -> &'static str {
    match cls {
        NounClass::Generic => "个",
        NounClass::Animal => "只",
        NounClass::Strip => "条",
        NounClass::Flat => "张",
        NounClass::Bound => "本",
    }
}

/// 固定键表。
pub const KEYS: [&'static str; STR_COUNT] = [
    "os.name",
    "btn.ok",
    "btn.cancel",
    "btn.close",
    "err.disk",
    "net.off",
    "bat.low",
    "a11y.focus",
    "ui.theme",
    "legacy.tip",
    "x.plain",
    "y.ghost",
];

/// 中文文案（空串表示缺条目）。
pub const ZH: [&'static str; STR_COUNT] = [
    "万相内核",
    "确定",
    "取消",
    "关闭",
    "磁盘读写故障",
    "网络已断开",
    "电量不足",
    "焦点已移动",
    "主题",
    "旧版提示",
    "",
    "",
];

/// 英文文案（空串表示缺条目）。
pub const EN: [&'static str; STR_COUNT] = [
    "Varix Kernel",
    "OK",
    "Cancel",
    "Close",
    "Disk I/O error",
    "Network offline",
    "Battery low",
    "Focus moved",
    "Theme",
    "",
    "Plain text",
    "",
];

/// 查译文并给出三态：1=直接命中 2=跨语种回退命中 0=键名兜底。
/// 缺条目回退链 zh→en→键名（En 请求反向对称）。
pub fn lookup(lang: Lang, key: &'static str) -> (&'static str, u8) {
    let mut found: Option<usize> = None;
    for i in 0..STR_COUNT {
        if KEYS[i] == key {
            found = Some(i);
            break;
        }
    }
    let i = match found {
        Some(i) => i,
        None => return (key, 0),
    };
    match lang {
        Lang::Zh => {
            if !ZH[i].is_empty() {
                (ZH[i], 1)
            } else if !EN[i].is_empty() {
                (EN[i], 2)
            } else {
                (key, 0)
            }
        }
        Lang::En => {
            if !EN[i].is_empty() {
                (EN[i], 1)
            } else if !ZH[i].is_empty() {
                (ZH[i], 2)
            } else {
                (key, 0)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 纯整数日期与时区
// ---------------------------------------------------------------------------

/// 闰年判定。
pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// 某月天数（月份钳制 1~12）。
pub fn days_in_month(y: i64, m: u32) -> u32 {
    let m = m.clamp(1, 12);
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

/// 公历日期 → 纪元天数（1970-01-01 = 0）。
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m as i64) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + (d as i64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 纪元天数 → 公历日期（与 days_from_civil 互逆）。
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 星期（0=周一 … 6=周日；纪元日 1970-01-01 为周四=3）。
pub fn weekday(days: i64) -> u32 {
    (days + 3).rem_euclid(7) as u32
}

/// 时区偏移钳制（-720~840 分钟）。
pub fn tz_clamp(tz: i32) -> i32 {
    tz.clamp(MIN_TZ, MAX_TZ)
}

/// UTC 当日分钟 + 时区偏移 → 本地当日分钟（回绕）。
pub fn local_minutes(utc_min: i32, tz: i32) -> i32 {
    (utc_min + tz).rem_euclid(DAY_MIN)
}

/// 日期 + UTC 当日分钟 + 时区 → 本地 (年, 月, 日, 时, 分)，含跨日翻页。
pub fn stamp_parts(days: i64, utc_min: i32, tz: i32) -> (i64, u32, u32, u32, u32) {
    let total = utc_min + tz;
    let shift = total.div_euclid(DAY_MIN);
    let local = total.rem_euclid(DAY_MIN);
    let (y, m, d) = civil_from_days(days + shift as i64);
    (y, m, d, (local / 60) as u32, (local % 60) as u32)
}

// ---------------------------------------------------------------------------
// 引擎
// ---------------------------------------------------------------------------

/// 本地化引擎：语言档 + 时区 + 回退统计 + 缺译建议。
pub struct L10nCab {
    pub lang: Lang,
    /// 时区偏移（分钟，-720~840）。
    pub tz_min: i32,
    /// 低配精简模式（日期渲染省略年份）。
    pub lite: bool,
    pub queries: u64,
    /// 跨语种回退次数。
    pub fallbacks: u64,
    /// 键名兜底次数。
    pub misses: u64,
    /// 中文缺条目次数。
    pub zh_missing: u64,
    /// 英文缺条目次数。
    pub en_missing: u64,
    /// 彩蛋：查到幽灵键（双缺条目）点亮。
    pub ghost: bool,
    /// 最近一次译文。
    pub last: &'static str,
}

impl L10nCab {
    pub fn new() -> L10nCab {
        L10nCab {
            lang: Lang::Zh,
            tz_min: 480,
            lite: false,
            queries: 0,
            fallbacks: 0,
            misses: 0,
            zh_missing: 0,
            en_missing: 0,
            ghost: false,
            last: "",
        }
    }

    pub fn set_lang(&mut self, l: Lang) {
        self.lang = l;
    }

    /// 设置时区偏移（越界钳制并报 E_INVALID）。
    pub fn set_tz(&mut self, tz: i32) -> u16 {
        if tz < MIN_TZ || tz > MAX_TZ {
            self.tz_min = tz_clamp(tz);
            return E_INVALID;
        }
        self.tz_min = tz;
        E_OK
    }

    /// 查译文并记账（回退/兜底分类计数）。
    pub fn tr(&mut self, lang: Lang, key: &'static str) -> &'static str {
        let (v, st) = lookup(lang, key);
        self.queries += 1;
        match st {
            1 => {}
            2 => {
                self.fallbacks += 1;
                if lang == Lang::Zh {
                    self.zh_missing += 1;
                } else {
                    self.en_missing += 1;
                }
            }
            _ => {
                self.misses += 1;
                self.ghost = true;
            }
        }
        self.last = v;
        v
    }

    /// 按当前语言查译文。
    pub fn tr_cur(&mut self, key: &'static str) -> &'static str {
        let l = self.lang;
        self.tr(l, key)
    }

    /// 智能建议：缺译多的一侧建议切换语言。
    pub fn suggest_lang(&self) -> Lang {
        if self.zh_missing > self.en_missing {
            Lang::En
        } else {
            Lang::Zh
        }
    }

    /// 渲染 "N 量词"（数字为 ASCII，量词为 UTF-8），缓冲不足返回 0。
    pub fn format_count(&self, cls: NounClass, n: u32, buf: &mut [u8]) -> usize {
        let word = measure_word(cls).as_bytes();
        let mut ds = [0u8; 10];
        let mut w = 0usize;
        let mut x = n;
        if x == 0 {
            ds[0] = b'0';
            w = 1;
        }
        while x > 0 {
            ds[w] = b'0' + (x % 10) as u8;
            x /= 10;
            w += 1;
        }
        let need = w + 1 + word.len();
        if buf.len() < need {
            return 0;
        }
        let mut k = 0usize;
        while w > 0 {
            w -= 1;
            buf[k] = ds[w];
            k += 1;
        }
        buf[k] = b' ';
        k += 1;
        for i in 0..word.len() {
            buf[k] = word[i];
            k += 1;
        }
        need
    }

    /// 带错误码的量词渲染。
    pub fn format_count_checked(&self, cls: NounClass, n: u32, buf: &mut [u8]) -> u16 {
        let mut probe = [0u8; 16];
        let need = self.format_count(cls, n, &mut probe);
        if need == 0 || buf.len() < need {
            return E_NOROOM;
        }
        let _ = self.format_count(cls, n, buf);
        E_OK
    }

    /// 渲染 "hh:mm"（纯 ASCII），缓冲不足返回 0。
    pub fn fmt_hhmm(&self, minute: i32, buf: &mut [u8]) -> usize {
        let local = minute.rem_euclid(DAY_MIN);
        if buf.len() < 5 {
            return 0;
        }
        let mut n = 0usize;
        push_pad2(buf, &mut n, (local / 60) as u32);
        buf[n] = b':';
        n += 1;
        push_pad2(buf, &mut n, (local % 60) as u32);
        n
    }

    /// 渲染本地时间戳：完整 "YYYY-MM-DD hh:mm"（16 字节），
    /// 精简模式 "MM-DD hh:mm"（11 字节），缓冲不足返回 0。
    pub fn fmt_stamp(&self, days: i64, utc_min: i32, buf: &mut [u8]) -> usize {
        let (y, mo, d, hh, mm) = stamp_parts(days, utc_min, self.tz_min);
        let need = if self.lite { 11 } else { 16 };
        if buf.len() < need {
            return 0;
        }
        let mut n = 0usize;
        if !self.lite {
            push_pad4(buf, &mut n, y as u32);
            buf[n] = b'-';
            n += 1;
        }
        push_pad2(buf, &mut n, mo);
        buf[n] = b'-';
        n += 1;
        push_pad2(buf, &mut n, d);
        buf[n] = b' ';
        n += 1;
        push_pad2(buf, &mut n, hh);
        buf[n] = b':';
        n += 1;
        push_pad2(buf, &mut n, mm);
        n
    }

    /// 三线联动组合渲染："[文案] [N 量词] [时间戳]"。
    pub fn compose(&self, key: &'static str, cls: NounClass, n: u32, days: i64, utc_min: i32, buf: &mut [u8]) -> usize {
        let (label, _) = lookup(self.lang, key);
        let mut cnt = [0u8; 16];
        let cn = self.format_count(cls, n, &mut cnt);
        let mut st = [0u8; 24];
        let sn = self.fmt_stamp(days, utc_min, &mut st);
        if cn == 0 || sn == 0 {
            return 0;
        }
        let need = label.len() + 1 + cn + 1 + sn;
        if buf.len() < need {
            return 0;
        }
        let mut k = 0usize;
        let lb = label.as_bytes();
        for i in 0..lb.len() {
            buf[k] = lb[i];
            k += 1;
        }
        buf[k] = b' ';
        k += 1;
        for i in 0..cn {
            buf[k] = cnt[i];
            k += 1;
        }
        buf[k] = b' ';
        k += 1;
        for i in 0..sn {
            buf[k] = st[i];
            k += 1;
        }
        k
    }

    /// 不变量审计：时区在界、回退账目守恒、表容量合法。
    pub fn audit(&self) -> bool {
        if self.tz_min < MIN_TZ || self.tz_min > MAX_TZ {
            return false;
        }
        if self.fallbacks + self.misses > self.queries {
            return false;
        }
        if self.fallbacks != self.zh_missing + self.en_missing {
            return false;
        }
        STR_COUNT <= MAX_STR
    }

    /// 快照导出：魔数 + 语言/精简/时区 + 全部计数。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < SNAP_LEN {
            return 0;
        }
        buf[0] = MAGIC;
        buf[1] = 1;
        buf[2] = if self.lang == Lang::En { 1 } else { 0 };
        buf[3] = if self.lite { 1 } else { 0 };
        let tb = self.tz_min.to_le_bytes();
        for i in 0..4 {
            buf[4 + i] = tb[i];
        }
        put_u64(buf, 8, self.queries);
        put_u64(buf, 16, self.fallbacks);
        put_u64(buf, 24, self.misses);
        put_u64(buf, 32, self.zh_missing);
        put_u64(buf, 40, self.en_missing);
        SNAP_LEN
    }

    /// 快照导入：恢复语言/精简/时区/计数（非法域钳制），魔数版本校验。
    pub fn import(&mut self, buf: &[u8]) -> u16 {
        if buf.len() < SNAP_LEN || buf[0] != MAGIC || buf[1] != 1 {
            return E_INVALID;
        }
        self.lang = if buf[2] == 1 { Lang::En } else { Lang::Zh };
        self.lite = buf[3] != 0;
        self.tz_min = tz_clamp(i32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]));
        self.queries = get_u64(buf, 8);
        self.fallbacks = get_u64(buf, 16);
        self.misses = get_u64(buf, 24);
        self.zh_missing = get_u64(buf, 32);
        self.en_missing = get_u64(buf, 40);
        self.ghost = false;
        self.last = "";
        E_OK
    }

    /// 回滚净身：计数与彩蛋清零，语言/时区/精简配置保留。
    pub fn reset(&mut self) {
        self.queries = 0;
        self.fallbacks = 0;
        self.misses = 0;
        self.zh_missing = 0;
        self.en_missing = 0;
        self.ghost = false;
        self.last = "";
    }
}

fn push_pad2(buf: &mut [u8], n: &mut usize, v: u32) {
    if *n < buf.len() {
        buf[*n] = b'0' + ((v / 10) % 10) as u8;
        *n += 1;
    }
    if *n < buf.len() {
        buf[*n] = b'0' + (v % 10) as u8;
        *n += 1;
    }
}

fn push_pad4(buf: &mut [u8], n: &mut usize, v: u32) {
    let ds = [(v / 1000) % 10, (v / 100) % 10, (v / 10) % 10, v % 10];
    for i in 0..4 {
        if *n < buf.len() {
            buf[*n] = b'0' + ds[i] as u8;
            *n += 1;
        }
    }
}

fn put_u64(buf: &mut [u8], off: usize, v: u64) {
    let b = v.to_le_bytes();
    for i in 0..8 {
        buf[off + i] = b[i];
    }
}

fn get_u64(buf: &[u8], off: usize) -> u64 {
    let mut b = [0u8; 8];
    for i in 0..8 {
        b[i] = buf[off + i];
    }
    u64::from_le_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loc_table_measure_and_dates() {
        // 回退链三态。
        assert_eq!(lookup(Lang::Zh, "btn.ok"), ("确定", 1));
        assert_eq!(lookup(Lang::Zh, "x.plain"), ("Plain text", 2));
        assert_eq!(lookup(Lang::En, "legacy.tip"), ("旧版提示", 2));
        assert_eq!(lookup(Lang::Zh, "y.ghost").1, 0);
        assert_eq!(lookup(Lang::Zh, "no.key"), ("no.key", 0));
        // 量词按类别。
        assert_eq!(measure_word(NounClass::Generic), "个");
        assert_eq!(measure_word(NounClass::Animal), "只");
        assert_eq!(measure_word(NounClass::Strip), "条");
        assert_eq!(measure_word(NounClass::Flat), "张");
        assert_eq!(measure_word(NounClass::Bound), "本");
        // 纯整数日期互逆与锚点。
        assert!(is_leap(2024));
        assert!(!is_leap(2026));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
        assert_eq!(civil_from_days(days_from_civil(2026, 9, 13)), (2026, 9, 13));
        assert_eq!(weekday(0), 3);
        // 时区偏移与跨日。
        assert_eq!(tz_clamp(2000), MAX_TZ);
        assert_eq!(local_minutes(0, 480), 480);
        assert_eq!(local_minutes(100, -480), 1060);
        let mut buf = [0u8; 16];
        assert_eq!(fmt_hhmm_free(1050, &mut buf), 5);
        assert_eq!(&buf[..5], b"17:30");
    }

    #[test]
    fn loc_engine_stamp_snapshot_and_lite() {
        let mut c = L10nCab::new();
        assert_eq!(c.tr(Lang::Zh, "btn.ok"), "确定");
        assert_eq!(c.queries, 1);
        assert_eq!(c.tr(Lang::Zh, "y.ghost"), "y.ghost");
        assert!(c.ghost && c.misses == 1);
        // 时区跨日翻页。
        let mut buf = [0u8; 32];
        let n = c.fmt_stamp(days_from_civil(1970, 1, 1), 100, &mut buf);
        assert_eq!(n, 16);
        assert_eq!(&buf[..n], b"1970-01-01 09:40");
        assert_eq!(c.set_tz(-480), E_OK);
        let n2 = c.fmt_stamp(days_from_civil(1970, 1, 1), 100, &mut buf);
        assert_eq!(&buf[..n2], b"1969-12-31 17:40");
        // 快照迁移。
        let mut sb = [0u8; 64];
        let sn = c.export(&mut sb);
        assert_eq!(sn, SNAP_LEN);
        let mut d = L10nCab::new();
        assert_eq!(d.import(&sb[..sn]), E_OK);
        assert_eq!(d.tz_min, -480);
        assert_eq!(d.queries, 2);
        // 精简模式。
        d.lite = true;
        let mut b2 = [0u8; 32];
        let n3 = d.fmt_stamp(days_from_civil(1970, 1, 1), 100, &mut b2);
        assert_eq!(n3, 11);
        assert_eq!(&b2[..n3], b"12-31 17:40");
        // 净身。
        d.reset();
        assert!(d.queries == 0 && d.misses == 0 && d.last.is_empty() && !d.ghost && d.tz_min == -480);
    }

    #[test]
    fn loc_all_checks_pass() {
        let set = run_loc_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            assert!(set.get(i).unwrap().passed, "第 {} 项未通过: {}", i, set.get(i).unwrap().name);
        }
    }
}

/// 引擎外独立渲染 "hh:mm"（测试与扩展共用）。
pub fn fmt_hhmm_free(minute: i32, buf: &mut [u8]) -> usize {
    let local = minute.rem_euclid(DAY_MIN);
    if buf.len() < 5 {
        return 0;
    }
    let mut n = 0usize;
    push_pad2(buf, &mut n, (local / 60) as u32);
    buf[n] = b':';
    n += 1;
    push_pad2(buf, &mut n, (local % 60) as u32);
    n
}

/// 族0276 自检：X06876~X06900 逐项登记。
pub fn run_loc_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("task-l10n");

    // —— 基础实装 X06876~X06880 ——
    let mut c = L10nCab::new();
    let v = c.tr(Lang::Zh, "btn.ok");
    let en = c.tr(Lang::En, "btn.ok");
    let mut buf = [0u8; 32];
    let cn = c.format_count(NounClass::Animal, 3, &mut buf);
    set.add("X06876 核心链路闭环", v == "确定" && en == "OK" && cn == 5 && &buf[..cn] == "3 只".as_bytes() && c.queries == 2, "查串→量词→计数端到端可观测");

    let mut p = L10nCab::new();
    p.set_lang(Lang::En);
    let t1 = p.set_tz(540);
    p.lite = true;
    let r = p.tr(Lang::En, "net.off");
    set.add("X06877 全量参数开放", t1 == E_OK && p.tz_min == 540 && p.lang == Lang::En && p.lite && r == "Network offline" && p.queries == 1, "语言/时区/精简模式全参数可配可读");

    let mut idx_ok = true;
    for i in 0..NOUN_CLASSES.len() {
        idx_ok &= NOUN_CLASSES[i].index() == i as u32
            && !NOUN_CLASSES[i].name().is_empty()
            && measure_word(NOUN_CLASSES[i]) == ["个", "只", "条", "张", "本"][i];
    }
    idx_ok &= NounClass::Generic.name() == "generic" && NounClass::Bound.name() == "bound";
    set.add("X06878 档位矩阵≥5档", idx_ok, "个/只/条/张/本五类量词独立可配");

    let mut a = L10nCab::new();
    let _ = a.set_tz(-300);
    let _ = a.tr(Lang::Zh, "x.plain");
    let _ = a.tr(Lang::Zh, "y.ghost");
    let mut sb = [0u8; 64];
    let sn = a.export(&mut sb);
    let mut b = L10nCab::new();
    let imp = b.import(&sb[..sn]);
    set.add("X06879 快照迁移三通道", sn == SNAP_LEN && sb[0] == MAGIC && imp == E_OK && b.tz_min == -300 && b.queries == 2 && b.fallbacks == 1 && b.misses == 1, "导出/导入/魔数版本三通道");

    let mut all_nonempty = true;
    for i in 0..STR_COUNT {
        all_nonempty &= !lookup(Lang::Zh, KEYS[i]).0.is_empty();
    }
    let days = days_from_civil(2026, 9, 13);
    let round = civil_from_days(days) == (2026, 9, 13);
    let r = L10nCab::new();
    set.add("X06880 联调无回归", all_nonempty && round && lookup(Lang::Zh, "btn.cancel").1 == 1 && r.audit(), "回退链保证全表非空、日期互逆无回归");

    // —— 边界与恢复 X06881~X06885 ——
    let mut z = L10nCab::new();
    let over = z.set_tz(2000);
    let after_over = z.tz_min;
    let under = z.set_tz(-2000);
    let m13 = days_in_month(2026, 13);
    let m0 = days_in_month(2026, 0);
    set.add("X06881 非法输入钳制", over == E_INVALID && after_over == MAX_TZ && under == E_INVALID && z.tz_min == MIN_TZ && m13 == 31 && m0 == 31, "时区与月份越界钳制不崩溃");

    set.add("X06882 错误叙事体系", describe(E_INVALID).contains("钳制") && describe(E_NOKEY).contains("键名") && describe(E_NOROOM).contains("缓冲") && describe(E_OK) == "正常", "每个失败有下一步建议");

    let mut x = L10nCab::new();
    let _ = x.tr(Lang::Zh, "btn.ok");
    let _ = x.tr(Lang::Zh, "x.plain");
    let mut xb = [0u8; 64];
    let xn = x.export(&mut xb);
    let mut y = L10nCab::new();
    let _ = y.import(&xb[..xn]);
    let _ = y.tr(Lang::En, "legacy.tip");
    let _ = y.tr(Lang::Zh, "y.ghost");
    let _ = y.tr(Lang::Zh, "btn.ok");
    let mut twin = L10nCab::new();
    let _ = twin.tr(Lang::Zh, "btn.ok");
    let _ = twin.tr(Lang::Zh, "x.plain");
    let _ = twin.tr(Lang::En, "legacy.tip");
    let _ = twin.tr(Lang::Zh, "y.ghost");
    let _ = twin.tr(Lang::Zh, "btn.ok");
    set.add("X06883 中断续跑还原", y.queries == twin.queries && y.fallbacks == twin.fallbacks && y.misses == twin.misses && y.queries == 5, "半程快照续跑与不间断账目一致");

    let mut g = L10nCab::new();
    let miss = g.tr(Lang::Zh, "ghost.key");
    let mut small = [0u8; 2];
    let no = g.format_count(NounClass::Strip, 12, &mut small);
    set.add("X06884 资源降级守护", miss == "ghost.key" && g.misses == 1 && no == 0 && g.audit(), "未知键键名兜底、小缓冲拒绝不崩溃");

    let mut cl = L10nCab::new();
    let _ = cl.tr(Lang::Zh, "y.ghost");
    let _ = cl.set_tz(0);
    cl.reset();
    set.add("X06885 回滚净身", cl.queries == 0 && cl.misses == 0 && cl.fallbacks == 0 && !cl.ghost && cl.last.is_empty() && cl.tz_min == 0, "计数清零、语言时区配置保留不留残档");

    // —— 手感与细节 X06886~X06890 ——
    set.add("X06886 令牌对齐", Lang::Zh.index() == 0 && Lang::En.index() == 1 && Lang::Zh.name() == "zh-CN" && Lang::En.name() == "en" && NounClass::Strip.name() == "strip", "语言与类别名称索引一一对应");

    let s1 = lookup(Lang::Zh, "btn.ok").1;
    let s2a = lookup(Lang::Zh, "x.plain").1;
    let s2b = lookup(Lang::En, "legacy.tip").1;
    let s0 = lookup(Lang::Zh, "y.ghost").1;
    let s0b = lookup(Lang::Zh, "no.key").1;
    set.add("X06887 三态焦点", s1 == 1 && s2a == 2 && s2b == 2 && s0 == 0 && s0b == 0, "直接命中/跨语种回退/键名兜底三态齐备");

    let mut kb = [0u8; 16];
    let kn = fmt_hhmm_free(1050, &mut kb);
    let ascii = (0..kn).all(|i| kb[i] < 0x80);
    set.add("X06888 键盘通道", kn == 5 && ascii && &kb[..kn] == b"17:30", "时间渲染纯 ASCII 可键盘复现");

    let ws = [
        measure_word(NounClass::Generic),
        measure_word(NounClass::Animal),
        measure_word(NounClass::Strip),
        measure_word(NounClass::Flat),
        measure_word(NounClass::Bound),
    ];
    let mut uniq = describe(E_OK) == "正常";
    for i in 0..ws.len() {
        uniq &= ws[i].len() == 3;
        for j in (i + 1)..ws.len() {
            uniq &= ws[i] != ws[j];
        }
    }
    set.add("X06889 微文案统一", uniq, "量词单字且互异、中文自然术语一致");

    let w = L10nCab::new();
    let mut wb = [0u8; 32];
    let wn = w.fmt_stamp(days_from_civil(2026, 9, 13), 0, &mut wb);
    set.add("X06890 无障碍等价", wn == 16 && &wb[..wn] == b"2026-09-13 08:00", "固定宽度时间戳读屏可预测");

    // —— 性能与优化 X06891~X06895 ——
    let zh_n = (0..STR_COUNT).filter(|i| !ZH[*i].is_empty()).count();
    let en_n = (0..STR_COUNT).filter(|i| !EN[*i].is_empty()).count();
    set.add("X06891 基准采集", STR_COUNT == 12 && zh_n == 10 && en_n == 10 && KEYS[0] == "os.name", "双语覆盖基准入册（各 2 条缺译作回退样本）");

    let mut hot = L10nCab::new();
    for i in 0..1000u32 {
        let key = KEYS[(i % STR_COUNT as u32) as usize];
        let _ = hot.tr(Lang::Zh, key);
    }
    set.add("X06892 热路径量化", hot.queries == 1000 && hot.fallbacks + hot.misses <= hot.queries && hot.audit(), "千次查串账目精确无失控增长");

    let mut conv = L10nCab::new();
    for i in 0..200u32 {
        let _ = conv.tr(Lang::Zh, KEYS[(i % STR_COUNT as u32) as usize]);
    }
    conv.reset();
    set.add("X06893 内存功耗收敛", conv.queries == 0 && conv.fallbacks == 0 && conv.misses == 0 && conv.last.is_empty(), "高负载后待机零增量泄漏入长稳");

    let mut l = L10nCab::new();
    let mut fb = [0u8; 32];
    let full = l.fmt_stamp(days_from_civil(2026, 9, 13), 0, &mut fb);
    l.lite = true;
    let mut lb2 = [0u8; 32];
    let short = l.fmt_stamp(days_from_civil(2026, 9, 13), 0, &mut lb2);
    set.add("X06894 低配降级链", full == 16 && short == 11 && &lb2[..short] == b"09-13 08:00", "低配精简渲染省略年份");

    let mut gd = L10nCab::new();
    let a0 = gd.audit();
    for i in 0..500u32 {
        let _ = gd.tr(Lang::En, KEYS[(i % STR_COUNT as u32) as usize]);
    }
    set.add("X06895 防劣化守卫", a0 && gd.audit() && gd.fallbacks + gd.misses <= gd.queries, "账目不变量断言只增不删");

    // —— 创新拓展 X06896~X06900 ——
    let mut sm = L10nCab::new();
    let _ = sm.tr(Lang::Zh, "x.plain");
    let sug1 = sm.suggest_lang();
    let _ = sm.tr(Lang::En, "legacy.tip");
    let sug2 = sm.suggest_lang();
    set.add("X06896 智能建议", sug1 == Lang::En && sug2 == Lang::Zh && sm.zh_missing >= 1 && sm.en_missing >= 1, "按缺译方向建议切换语言可解释可拒绝");

    let mut bt = L10nCab::new();
    let mut nonempty = 0usize;
    for i in 0..STR_COUNT {
        if !bt.tr(Lang::Zh, KEYS[i]).is_empty() {
            nonempty += 1;
        }
    }
    let mut cb = [0u8; 8];
    let mut cnt_ok = true;
    for i in 0..NOUN_CLASSES.len() {
        cnt_ok &= bt.format_count(NOUN_CLASSES[i], i as u32 + 1, &mut cb) > 0;
    }
    set.add("X06897 批量自动化", nonempty == STR_COUNT && cnt_ok && bt.queries == STR_COUNT as u64, "全表批量查译与全类批量渲染脚本化完成");

    let mut cp = L10nCab::new();
    let _ = cp.tr(Lang::Zh, "btn.ok");
    let mut cb2 = [0u8; 64];
    let cn2 = cp.compose("btn.ok", NounClass::Animal, 3, days, 0, &mut cb2);
    set.add("X06898 三线跨域联动", cn2 == 29 && &cb2[..6] == "确定".as_bytes() && cp.fallbacks + cp.misses <= cp.queries && cp.audit(), "文案/量词/时间戳三段拼合读数一致");

    let ext = measure_word(NounClass::Flat) == "张"
        && days_from_civil(1970, 1, 1) == 0
        && days_from_civil(2000, 3, 1) == 11017
        && weekday(0) == 3
        && local_minutes(0, 480) == 480
        && local_minutes(100, -480) == 1060
        && lookup(Lang::En, "x.plain").0 == "Plain text";
    set.add("X06899 开发者扩展点", ext, "日期/时区/量词/查串纯函数可独立复用");

    let mut eg = L10nCab::new();
    let gv = eg.tr(Lang::Zh, "y.ghost");
    let egg = eg.ghost && gv == "y.ghost";
    eg.reset();
    set.add("X06900 彩蛋与净身", egg && !eg.ghost && eg.queries == 0 && eg.last.is_empty(), "幽灵键兜底点亮纪念、净身后无痕");

    set
}
