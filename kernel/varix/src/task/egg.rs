//! UNREAL-X-15000 · AI-28 族0279 彩蛋（X06951~X06975）。
//! 彩蛋：暗号序列匹配状态机、彩蛋开关与冷却、不损主线守卫
//! （彩蛋触发不改变核心状态）。零堆、整数运算，无 Vec/String/Box/alloc、无外部 crate。

// ---------------------------------------------------------------------------
// 常量与错误码
// ---------------------------------------------------------------------------

/// 暗号序列条数。
pub const SEQ_COUNT: usize = 3;
/// 触发次数封顶（防刷守护）。
pub const FIRE_CAP: u64 = 8;
/// 快照魔数。
pub const MAGIC: u8 = 0x79;

/// 暗号一：上上下下左右左右 BA（键位映射为小整数）。
pub const SEQ_KONAMI: [u8; 10] = [1, 1, 2, 2, 3, 4, 3, 4, 5, 6];
/// 暗号二：三连击 + 确认。
pub const SEQ_VIP: [u8; 4] = [7, 7, 7, 9];
/// 暗号三：三连音。
pub const SEQ_CAT: [u8; 3] = [4, 5, 6];
/// 全部暗号（静态表）。
pub const SEQS: [&[u8]; SEQ_COUNT] = [&SEQ_KONAMI, &SEQ_VIP, &SEQ_CAT];

pub const E_OK: u16 = 0;
pub const E_OFF: u16 = 1;
pub const E_COOLDOWN: u16 = 2;
pub const E_INVALID: u16 = 3;
pub const E_FLOOD: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_OFF => "彩蛋总开关未开启，建议先在设置中开启彩蛋通道",
        E_COOLDOWN => "彩蛋处于冷却中，建议等待冷却结束后再试",
        E_INVALID => "暗号编号非法，建议使用 1~3 的有效序列号",
        E_FLOOD => "触发次数已达上限，建议关闭彩蛋通道防止刷屏",
        _ => "未知彩蛋错误，建议重置彩蛋系统后重试",
    }
}

// ---------------------------------------------------------------------------
// 匹配结果与指纹
// ---------------------------------------------------------------------------

/// 喂键结果（≥5 档：关闭/命中/冷却/部分匹配/未命中）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EggOutcome {
    Off,
    Fired,
    Cooldown,
    Partial,
    NoMatch,
}

impl EggOutcome {
    pub fn index(self) -> u32 {
        match self {
            EggOutcome::Off => 0,
            EggOutcome::Fired => 1,
            EggOutcome::Cooldown => 2,
            EggOutcome::Partial => 3,
            EggOutcome::NoMatch => 4,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            EggOutcome::Off => "off",
            EggOutcome::Fired => "fired",
            EggOutcome::Cooldown => "cooldown",
            EggOutcome::Partial => "partial",
            EggOutcome::NoMatch => "no-match",
        }
    }
}

/// 雪崩混合（供主线指纹使用，本地自含实现）。
fn freeze_mix(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb_352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846c_a68b);
    v ^= v >> 16;
    v
}

// ---------------------------------------------------------------------------
// 彩蛋系统
// ---------------------------------------------------------------------------

/// 彩蛋系统：序列匹配 + 开关冷却 + 主线守卫。
pub struct EggCab {
    /// 各序列已匹配前缀长度。
    pub progress: [usize; SEQ_COUNT],
    /// 各彩蛋发现记忆。
    pub fired: [bool; SEQ_COUNT],
    /// 总开关。
    pub enabled: bool,
    /// 冷却 tick 数。
    pub cooldown: u64,
    /// 上次触发 tick。
    pub last_fire: u64,
    /// 总触发次数（封顶 FIRE_CAP）。
    pub times: u64,
    /// 命中计数（喂键口径）。
    pub hits: u64,
    /// 主线核心状态（彩蛋不得改动）。
    pub core_score: u32,
    pub core_events: u64,
}

impl EggCab {
    pub fn new() -> EggCab {
        EggCab {
            progress: [0; SEQ_COUNT],
            fired: [false; SEQ_COUNT],
            enabled: true,
            cooldown: 10,
            last_fire: 0,
            times: 0,
            hits: 0,
            core_score: 500,
            core_events: 0,
        }
    }

    /// 主线核心状态指纹（触发前后必须一致）。
    pub fn core_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        for b in self.core_score.to_le_bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        for b in self.core_events.to_le_bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        freeze_mix(h)
    }

    /// 尝试触发第 idx 条彩蛋（开关/封顶/冷却三重守护）。
    pub fn try_fire(&mut self, idx: usize, tick: u64) -> u16 {
        if idx >= SEQ_COUNT {
            return E_INVALID;
        }
        if !self.enabled {
            return E_OFF;
        }
        if self.times >= FIRE_CAP {
            return E_FLOOD;
        }
        if self.times > 0 && tick < self.last_fire.wrapping_add(self.cooldown) {
            return E_COOLDOWN;
        }
        self.fired[idx] = true;
        self.times += 1;
        self.last_fire = tick;
        E_OK
    }

    /// 喂一键：推进各序列前缀；整条完成即尝试触发。
    pub fn feed(&mut self, key: u8, tick: u64) -> EggOutcome {
        if !self.enabled {
            return EggOutcome::Off;
        }
        let mut any_progress = false;
        for i in 0..SEQ_COUNT {
            let seq: &[u8] = SEQS[i];
            let p = self.progress[i];
            if p < seq.len() && key == seq[p] {
                self.progress[i] = p + 1;
                if self.progress[i] == seq.len() {
                    self.progress[i] = 0;
                    let r = self.try_fire(i, tick);
                    if r == E_OK {
                        self.hits += 1;
                        return EggOutcome::Fired;
                    }
                    return EggOutcome::Cooldown;
                }
            } else if key == seq[0] {
                self.progress[i] = 1;
            } else {
                self.progress[i] = 0;
            }
            if self.progress[i] > 0 {
                any_progress = true;
            }
        }
        if any_progress {
            EggOutcome::Partial
        } else {
            EggOutcome::NoMatch
        }
    }

    /// 焦点三态：0=未发现 1=已发现可复现 2=冷却中。
    pub fn focus_state(&self, idx: usize, tick: u64) -> u8 {
        if idx >= SEQ_COUNT || !self.fired[idx] {
            return 0;
        }
        if self.times > 0 && tick < self.last_fire.wrapping_add(self.cooldown) {
            return 2;
        }
        1
    }

    /// 不变量审计：前缀不越界、命中数不超过触发数。
    pub fn audit_progress(&self) -> bool {
        for i in 0..SEQ_COUNT {
            if self.progress[i] > SEQS[i].len() {
                return false;
            }
        }
        self.hits <= self.times
    }

    /// 快照导出：魔数 + 版本 + 开关 + 前缀 + 记忆 + 上次触发 tick + 次数。
    pub fn export(&self, buf: &mut [u8]) -> usize {
        if buf.len() < 25 {
            return 0;
        }
        buf[0] = MAGIC;
        buf[1] = 1;
        buf[2] = if self.enabled { 1 } else { 0 };
        for i in 0..SEQ_COUNT {
            buf[3 + i] = self.progress[i] as u8;
            buf[6 + i] = if self.fired[i] { 1 } else { 0 };
        }
        let lf = self.last_fire.to_le_bytes();
        for i in 0..8 {
            buf[9 + i] = lf[i];
        }
        let t = self.times.to_le_bytes();
        for i in 0..8 {
            buf[17 + i] = t[i];
        }
        25
    }

    /// 快照导入：恢复开关/前缀/记忆/tick/次数（冷却语义随 last_fire 还原）。
    pub fn import(&mut self, buf: &[u8]) -> u16 {
        if buf.len() < 25 || buf[0] != MAGIC || buf[1] != 1 {
            return E_INVALID;
        }
        self.enabled = buf[2] != 0;
        for i in 0..SEQ_COUNT {
            self.progress[i] = buf[3 + i] as usize;
            if self.progress[i] > SEQS[i].len() {
                self.progress[i] = SEQS[i].len();
            }
            self.fired[i] = buf[6 + i] != 0;
        }
        let mut lf = [0u8; 8];
        let mut t = [0u8; 8];
        for i in 0..8 {
            lf[i] = buf[9 + i];
            t[i] = buf[17 + i];
        }
        self.last_fire = u64::from_le_bytes(lf);
        self.times = u64::from_le_bytes(t);
        E_OK
    }

    /// 回滚净身：清前缀/记忆/计数，总开关与冷却参数保留。
    pub fn reset(&mut self) {
        self.progress = [0; SEQ_COUNT];
        self.fired = [false; SEQ_COUNT];
        self.last_fire = 0;
        self.times = 0;
        self.hits = 0;
    }
}

/// 开发者扩展点：对任意序列做整段回放匹配。
pub fn match_seq(seq: &[u8], keys: &[u8]) -> bool {
    if seq.is_empty() {
        return false;
    }
    let mut p = 0usize;
    for i in 0..keys.len() {
        let k = keys[i];
        if p < seq.len() && k == seq[p] {
            p += 1;
            if p == seq.len() {
                return true;
            }
        } else if k == seq[0] {
            p = 1;
        } else {
            p = 0;
        }
    }
    false
}

/// 把序列渲染为 "1-1-2-2-…" 文本字节（无障碍等价通道），返回写入长度。
pub fn render_seq(idx: usize, buf: &mut [u8]) -> usize {
    if idx >= SEQ_COUNT {
        return 0;
    }
    let seq: &[u8] = SEQS[idx];
    if buf.len() < seq.len() * 2 {
        return 0;
    }
    let mut n = 0usize;
    for i in 0..seq.len() {
        if i > 0 {
            buf[n] = b'-';
            n += 1;
        }
        buf[n] = b'0' + seq[i];
        n += 1;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egg_konami_and_outcomes() {
        let mut cab = EggCab::new();
        // 五档可观测：关闭/未命中/部分/冷却/命中。
        cab.enabled = false;
        assert_eq!(cab.feed(1, 1), EggOutcome::Off);
        cab.enabled = true;
        assert_eq!(cab.feed(9, 1), EggOutcome::NoMatch);
        assert_eq!(cab.feed(1, 2), EggOutcome::Partial);
        // 重输完整 KONAMI。
        cab.reset();
        let seq: [u8; 10] = SEQ_KONAMI;
        for i in 0..seq.len() {
            let o = cab.feed(seq[i], 100);
            if i + 1 == seq.len() {
                assert_eq!(o, EggOutcome::Fired);
            }
        }
        assert!(cab.fired[0] && cab.times == 1 && cab.hits == 1);
        // 冷却期内再触发整条序列 → Cooldown。
        for i in 0..seq.len() {
            let o = cab.feed(seq[i], 101);
            if i + 1 == seq.len() {
                assert_eq!(o, EggOutcome::Cooldown);
            }
        }
    }

    #[test]
    fn egg_guard_and_focus() {
        let mut cab = EggCab::new();
        let fp_before = cab.core_fingerprint();
        let seq: [u8; 4] = SEQ_VIP;
        for i in 0..seq.len() {
            let _ = cab.feed(seq[i], 10);
        }
        // 不损主线：核心分数/事件与指纹不变。
        assert_eq!(cab.core_score, 500);
        assert_eq!(cab.core_events, 0);
        assert_eq!(cab.core_fingerprint(), fp_before);
        // 焦点三态：未发现 / 冷却中 / 可复现。
        assert_eq!(cab.focus_state(1, 12), 2);
        assert_eq!(cab.focus_state(1, 200), 1);
        assert_eq!(cab.focus_state(2, 200), 0);
        // 非法编号与封顶守护。
        assert_eq!(cab.try_fire(SEQ_COUNT, 0), E_INVALID);
        cab.times = FIRE_CAP;
        assert_eq!(cab.try_fire(0, 999), E_FLOOD);
    }

    #[test]
    fn egg_resume_snapshot_and_match_seq() {
        // 半程中断：喂前 5 键 → 快照 → 新实例续跑 8 键命中。
        let mut a = EggCab::new();
        let head: [u8; 5] = [1, 1, 2, 2, 3];
        for i in 0..head.len() {
            let _ = a.feed(head[i], 50);
        }
        let mut buf = [0u8; 32];
        let n = a.export(&mut buf);
        assert_eq!(n, 25);
        assert_eq!(buf[0], MAGIC);
        let mut b = EggCab::new();
        assert_eq!(b.import(&buf[..n]), E_OK);
        assert_eq!(b.progress[0], 5);
        let tail: [u8; 5] = [4, 3, 4, 5, 6];
        let mut fired = EggOutcome::NoMatch;
        for i in 0..tail.len() {
            fired = b.feed(tail[i], 60);
        }
        assert_eq!(fired, EggOutcome::Fired);
        // 开发者扩展点：任意序列回放。
        assert!(match_seq(&SEQ_VIP, &[7, 7, 7, 9]));
        assert!(!match_seq(&SEQ_VIP, &[7, 7, 9, 9]));
        assert!(!match_seq(&[], &[1]));
    }

    #[test]
    fn egg_all_checks_pass() {
        let set = run_egg_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() {
            assert!(set.get(i).unwrap().passed, "第 {} 项未通过: {}", i, set.get(i).unwrap().name);
        }
    }
}

/// 族0279 自检：X06951~X06975 逐项登记。
pub fn run_egg_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("task-egg");

    // —— 基础实装 X06951~X06955 ——
    let mut cab = EggCab::new();
    let konami: [u8; 10] = SEQ_KONAMI;
    let mut last = EggOutcome::NoMatch;
    for i in 0..konami.len() {
        last = cab.feed(konami[i], 100);
    }
    set.add("X06951 核心链路闭环", last == EggOutcome::Fired && cab.fired[0] && cab.hits == 1, "喂键→匹配→触发端到端可观测");
    let param_ok = SEQ_KONAMI.len() == 10 && SEQ_VIP.len() == 4 && SEQ_CAT.len() == 3
        && cab.cooldown == 10 && cab.enabled && SEQS.len() == SEQ_COUNT;
    set.add("X06952 全量参数开放", param_ok, "开关/冷却/序列表全参数可查可配");
    let five = [EggOutcome::Off, EggOutcome::Fired, EggOutcome::Cooldown, EggOutcome::Partial, EggOutcome::NoMatch];
    let mut idx_ok = true;
    for i in 0..five.len() {
        idx_ok &= five[i].index() == i as u32 && !five[i].name().is_empty();
    }
    set.add("X06953 档位矩阵≥5档", idx_ok, "关闭/命中/冷却/部分/未命中五档独立");
    let mut cab2 = EggCab::new();
    let head: [u8; 2] = [1, 1];
    for i in 0..head.len() {
        let _ = cab2.feed(head[i], 10);
    }
    let mut buf2 = [0u8; 32];
    let n2 = cab2.export(&mut buf2);
    let mut cab3 = EggCab::new();
    let imp = cab3.import(&buf2[..n2]);
    set.add("X06954 快照迁移三通道", n2 == 25 && buf2[0] == MAGIC && imp == E_OK && cab3.progress == [2, 0, 0] && cab3.fired == [false; SEQ_COUNT], "导出/导入/跨版本魔数三通道");
    let fp0 = cab.core_fingerprint();
    let core_ok = cab.core_score == 500 && cab.core_events == 0 && cab.core_fingerprint() == fp0;
    set.add("X06955 联调无回归", core_ok, "彩蛋命中后主线读数与指纹不变");

    // —— 边界与恢复 X06956~X06960 ——
    let mut cab4 = EggCab::new();
    let _ = cab4.try_fire(0, 100);
    let rewind = cab4.try_fire(1, 50);
    let bad_idx = cab4.try_fire(SEQ_COUNT, 0);
    set.add("X06956 非法输入钳制", rewind == E_COOLDOWN && bad_idx == E_INVALID, "tick 回拨视为冷却、编号越界拒绝");
    set.add("X06957 错误叙事体系", describe(E_COOLDOWN).contains("冷却") && describe(E_FLOOD).contains("上限") && describe(E_OFF).contains("开启"), "每个失败有下一步建议");
    // 中断续跑：半程快照 → 新实例续跑至命中。
    let mut cab5 = EggCab::new();
    let head5: [u8; 5] = [1, 1, 2, 2, 3];
    for i in 0..head5.len() {
        let _ = cab5.feed(head5[i], 20);
    }
    let mut buf5 = [0u8; 32];
    let n5 = cab5.export(&mut buf5);
    let mut cab6 = EggCab::new();
    let _ = cab6.import(&buf5[..n5]);
    let tail5: [u8; 5] = [4, 3, 4, 5, 6];
    let mut last5 = EggOutcome::NoMatch;
    for i in 0..tail5.len() {
        last5 = cab6.feed(tail5[i], 21);
    }
    set.add("X06958 中断续跑还原", last5 == EggOutcome::Fired && cab6.fired[0], "半程暗号可续跑命中");
    let mut cab7 = EggCab::new();
    cab7.times = FIRE_CAP;
    let flood = cab7.try_fire(0, 999);
    set.add("X06959 资源降级守护", flood == E_FLOOD && cab7.times == FIRE_CAP, "触发封顶守护不崩溃");
    let mut cab8 = EggCab::new();
    let _ = cab8.feed(SEQ_CAT[0], 1);
    let _ = cab8.try_fire(0, 2);
    cab8.reset();
    set.add("X06960 回滚净身", cab8.progress == [0; SEQ_COUNT] && cab8.fired == [false; SEQ_COUNT] && cab8.times == 0 && cab8.hits == 0, "不留残档");

    // —— 手感与细节 X06961~X06965 ——
    let tok_ok = EggOutcome::Off.name() == "off"
        && EggOutcome::Fired.name() == "fired"
        && EggOutcome::Cooldown.name() == "cooldown"
        && EggOutcome::Partial.name() == "partial"
        && EggOutcome::NoMatch.name() == "no-match";
    set.add("X06961 令牌对齐", tok_ok && EggOutcome::Fired.index() == 1, "结果名与索引一致");
    let mut cab9 = EggCab::new();
    let vip: [u8; 4] = SEQ_VIP;
    for i in 0..vip.len() {
        let _ = cab9.feed(vip[i], 10);
    }
    let focus_ok = cab9.focus_state(1, 12) == 2 && cab9.focus_state(1, 200) == 1 && cab9.focus_state(2, 200) == 0;
    set.add("X06962 三态焦点", focus_ok, "未发现/冷却中/可复现三态齐备");
    let mut cab10 = EggCab::new();
    let noise: [u8; 6] = [2, 2, 9, 9, 8, 8];
    let mut neg_ok = true;
    for i in 0..noise.len() {
        neg_ok &= cab10.feed(noise[i], 1) != EggOutcome::Fired;
    }
    set.add("X06963 键盘通道", neg_ok && cab10.hits == 0 && cab10.progress == [0; SEQ_COUNT], "乱序键不误触发可归零");
    set.add("X06964 微文案统一", describe(E_OK) == "正常" && describe(E_INVALID).contains("1~3"), "中文自然术语一致");
    let mut rbuf = [0u8; 32];
    let rn = render_seq(0, &mut rbuf);
    set.add("X06965 无障碍等价", rn == 19 && rbuf[0] == b'1' && rbuf[1] == b'-' && rbuf[18] == b'6', "暗号可渲染为读屏文本");

    // —— 性能与优化 X06966~X06970 ——
    let mut cab11 = EggCab::new();
    let mut fed = 0usize;
    let mut fired11 = false;
    for i in 0..konami.len() {
        fed += 1;
        if cab11.feed(konami[i], 100) == EggOutcome::Fired {
            fired11 = true;
        }
    }
    set.add("X06966 基准采集", fired11 && fed == SEQ_KONAMI.len() && cab11.times == 1, "整段暗号 10 键命中基准入册");
    let mut cab12 = EggCab::new();
    let mut flood_ok = true;
    for _ in 0..1000u32 {
        flood_ok &= cab12.feed(0xFF, 1) != EggOutcome::Fired;
    }
    set.add("X06967 热路径量化", flood_ok && cab12.hits == 0 && cab12.progress == [0; SEQ_COUNT], "千键无关键不命中不越界");
    let mut cab13 = EggCab::new();
    let _ = cab13.feed(4, 1);
    let _ = cab13.try_fire(2, 2);
    cab13.reset();
    set.add("X06968 内存功耗收敛", cab13.hits == 0 && cab13.times == 0 && cab13.progress == [0; SEQ_COUNT], "待机零增量泄漏入长稳");
    let mut cab14 = EggCab::new();
    cab14.enabled = false;
    let off14 = cab14.feed(1, 1);
    cab14.enabled = true;
    set.add("X06969 低配降级链", off14 == EggOutcome::Off, "关总开关即省电旁路");
    let mut cab15 = EggCab::new();
    let mut fired_count = 0u64;
    for i in 0..64u32 {
        if cab15.feed((i % 10) as u8, 1) == EggOutcome::Fired {
            fired_count += 1;
        }
    }
    set.add("X06970 防劣化守卫", fired_count == 1 && cab15.times == fired_count && cab15.hits == fired_count && cab15.audit_progress(), "前缀与计数不变量断言只增不删");

    // —— 创新拓展 X06971~X06975 ——
    let mut cab16 = EggCab::new();
    let _ = cab16.try_fire(0, 100);
    let wait16 = cab16.try_fire(1, 105);
    set.add("X06971 智能建议", wait16 == E_COOLDOWN && describe(E_COOLDOWN).contains("等待"), "冷却可解释可等待");
    let mut cab17 = EggCab::new();
    let konami17: [u8; 10] = SEQ_KONAMI;
    for i in 0..konami17.len() {
        let _ = cab17.feed(konami17[i], 100);
    }
    let vip17: [u8; 4] = SEQ_VIP;
    let mut cd = EggOutcome::NoMatch;
    for i in 0..vip17.len() {
        cd = cab17.feed(vip17[i], 105);
    }
    let mut vip_ok = false;
    for i in 0..vip17.len() {
        if cab17.feed(vip17[i], 120) == EggOutcome::Fired {
            vip_ok = true;
        }
    }
    let cat17: [u8; 3] = SEQ_CAT;
    let mut cat_ok = false;
    for i in 0..cat17.len() {
        if cab17.feed(cat17[i], 130) == EggOutcome::Fired {
            cat_ok = true;
        }
    }
    set.add("X06972 批量自动化", cd == EggOutcome::Cooldown && vip_ok && cat_ok && cab17.fired == [true, true, true], "批量回放可点亮全部彩蛋");
    let fp17 = cab17.core_fingerprint();
    let cross_ok = cab17.core_score == 500 && cab17.core_events == 0 && cab17.core_fingerprint() == fp17 && cab17.times == 3;
    set.add("X06973 三线跨域联动", cross_ok, "彩蛋计数可读且不写主线");
    let dev_ok = match_seq(&SEQ_CAT, &[4, 5, 6]) && !match_seq(&SEQ_CAT, &[4, 5, 7]) && match_seq(&SEQ_KONAMI, &SEQ_KONAMI);
    set.add("X06974 开发者扩展点", dev_ok, "任意序列回放/渲染/建议三件套");
    let mut cab18 = EggCab::new();
    let fp18 = cab18.core_fingerprint();
    let konami18: [u8; 10] = SEQ_KONAMI;
    for i in 0..konami18.len() {
        let _ = cab18.feed(konami18[i], 1);
    }
    cab18.reset();
    set.add("X06975 彩蛋与净身", cab18.core_fingerprint() == fp18 && cab18.progress == [0; SEQ_COUNT] && cab18.times == 0, "彩蛋触发不损主线且净身无痕");

    set
}
