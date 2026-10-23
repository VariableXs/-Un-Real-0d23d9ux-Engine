//! VE-F0103 · 降级事件用户通知协议（VE-A 域 · A04b 诊断·降级·安全组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0103`
//!
//! **职责定位（锚点原文）**：降级事件按三要素协议通知用户——发生了什么（能力受限
//! 的具体表现）、为什么（原因类别）、下一步怎么办（可操作建议）；同类事件合并限频
//! 避免通知轰炸；每条通知可在总日志中心回溯原始事件与合并关系。
//!
//! **铁律**：
//! 1. **三要素**——每条通知必须有「什么/为什么/怎么办」。
//! 2. **合并限频**——同类事件窗口内合并计数。
//! 3. **可回溯**——通知携带原始事件与合并关系。
//! 4. **不静默**——模板缺失退化为最小告知。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8AE（降级通知）。
pub const SEG_BASE: u16 = 0xE8AE;

/// 模板缺失（退化最小告知）。
pub const XC_TMPL_MISSING: u16 = SEG_BASE | 0x01;
/// 通道故障。
pub const XC_CHANNEL_FAULT: u16 = SEG_BASE | 0x02;
/// 限频窗口配置错。
pub const XC_WINDOW_BAD: u16 = SEG_BASE | 0x03;
/// 未知类别。
pub const XC_UNKNOWN_CLASS: u16 = SEG_BASE | 0x04;

/// 降级类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradeClass {
    /// 性能降级。
    Performance,
    /// 功能受限。
    Feature,
    /// 安全降级。
    Security,
    /// 兼容降级。
    Compat,
}

/// 三要素通知模板。
#[derive(Clone, Debug)]
pub struct NotifyTemplate {
    pub class: DegradeClass,
    /// 文字型码：什么。
    pub what_code: u16,
    /// 文字型码：为什么。
    pub why_code: u16,
    /// 文字型码：怎么办。
    pub howto_code: u16,
    pub version: u32,
}

/// 合并窗口记录。
#[derive(Clone, Debug)]
struct MergeWindow {
    class: DegradeClass,
    tick_open: u64,
    count: u32,
    /// 首个原始事件 id。
    first_event: u64,
}

/// 通知信封（可回溯）。
#[derive(Clone, Debug)]
pub struct Notice {
    pub id: u64,
    pub class: DegradeClass,
    pub what_code: u16,
    pub why_code: u16,
    pub howto_code: u16,
    /// 合并计数（窗口内同类事件数）。
    pub merged_count: u32,
    /// 合并窗口起始 tick。
    pub window_open: u64,
    /// 首个原始事件 id（回溯锚）。
    pub first_event: u64,
}

/// 通知通道。
pub struct NotifyChannel {
    /// 通道健康位。
    healthy: bool,
    /// 待补投队列（有上限）。
    pending: Vec<Notice>,
    cap: usize,
}

impl NotifyChannel {
    pub fn new(cap: usize) -> Self {
        NotifyChannel {
            healthy: true,
            pending: Vec::new(),
            cap: if cap == 0 { 16 } else { cap },
        }
    }

    pub fn send(&mut self, n: Notice) -> Result<(), u16> {
        if !self.healthy {
            if self.pending.len() >= self.cap {
                return Err(XC_CHANNEL_FAULT);
            }
            self.pending.push(n);
            return Err(XC_CHANNEL_FAULT);
        }
        Ok(())
    }

    /// 通道恢复补投。
    pub fn recover_and_flush(&mut self) -> usize {
        self.healthy = true;
        let n = self.pending.len();
        self.pending.clear();
        n
    }

    pub fn inject_fault(&mut self) {
        self.healthy = false;
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

/// 降级通知器。
pub struct DegradeNotifier {
    templates: Vec<NotifyTemplate>,
    channel: NotifyChannel,
    windows: Vec<MergeWindow>,
    /// 限频窗口长度（tick）。
    window_len: u64,
    next_id: u64,
    next_event: u64,
    last_code: u16,
    /// 发出通知数。
    emitted: u64,
    /// 合并吞掉的事件数。
    merged_away: u64,
}

impl DegradeNotifier {
    pub fn new(window_len: u64) -> Result<Self, u16> {
        if window_len == 0 {
            return Err(XC_WINDOW_BAD);
        }
        Ok(DegradeNotifier {
            templates: Vec::new(),
            channel: NotifyChannel::new(16),
            windows: Vec::new(),
            window_len,
            next_id: 1,
            next_event: 1,
            last_code: 0,
            emitted: 0,
            merged_away: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 注册模板。
    pub fn register_template(&mut self, t: NotifyTemplate) {
        self.templates.retain(|x| x.class != t.class);
        self.templates.push(t);
    }

    /// 三要素完整性校验。
    fn triad_ok(t: &NotifyTemplate) -> bool {
        t.what_code != 0 && t.why_code != 0 && t.howto_code != 0
    }

    /// 报告降级事件：合并限频 + 三要素通知。
    pub fn report(&mut self, class: DegradeClass, tick: u64) -> Result<Option<Notice>, u16> {
        let event_id = self.next_event;
        self.next_event += 1;

        // 合并窗口判定。
        if let Some(w) = self.windows.iter_mut().find(|w| w.class == class) {
            if tick < w.tick_open + self.window_len {
                w.count += 1;
                self.merged_away += 1;
                self.last_code = 0;
                return Ok(None); // 合并吞掉，不新发通知。
            }
            // 窗口过期，重开。
            w.tick_open = tick;
            w.count = 1;
            w.first_event = event_id;
        } else {
            self.windows.push(MergeWindow {
                class,
                tick_open: tick,
                count: 1,
                first_event: event_id,
            });
        }

        // 取模板。
        let tmpl = self.templates.iter().find(|t| t.class == class).cloned();
        let (what, why, howto) = match &tmpl {
            Some(t) if Self::triad_ok(t) => (t.what_code, t.why_code, t.howto_code),
            _ => {
                // 不静默：退化最小告知（通用三要素码）。
                self.last_code = XC_TMPL_MISSING;
                (0xFFFF, 0xFFFE, 0xFFFD)
            }
        };

        let count = self
            .windows
            .iter()
            .find(|w| w.class == class)
            .map(|w| w.count)
            .unwrap_or(1);
        let first = self
            .windows
            .iter()
            .find(|w| w.class == class)
            .map(|w| w.first_event)
            .unwrap_or(event_id);

        let notice = Notice {
            id: self.next_id,
            class,
            what_code: what,
            why_code: why,
            howto_code: howto,
            merged_count: count,
            window_open: tick,
            first_event: first,
        };
        self.next_id += 1;

        match self.channel.send(notice.clone()) {
            Ok(()) => {
                self.emitted += 1;
                if self.last_code == XC_TMPL_MISSING {
                    // 保留模板缺失码但通知已发出。
                    Ok(Some(notice))
                } else {
                    self.last_code = 0;
                    Ok(Some(notice))
                }
            }
            Err(e) => {
                self.last_code = e;
                Err(e)
            }
        }
    }

    pub fn channel_mut(&mut self) -> &mut NotifyChannel {
        &mut self.channel
    }

    pub fn emitted(&self) -> u64 {
        self.emitted
    }

    pub fn merged_away(&self) -> u64 {
        self.merged_away
    }

    /// 回溯：按窗口查原始事件锚。
    pub fn trace_window(&self, class: DegradeClass) -> Option<(u64, u32)> {
        self.windows
            .iter()
            .find(|w| w.class == class)
            .map(|w| (w.first_event, w.count))
    }
}

/// 自检：三要素、合并限频、可回溯、不静默。
pub fn run_vea103_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn tmpl(c: DegradeClass) -> NotifyTemplate {
        NotifyTemplate {
            class: c,
            what_code: 0x100,
            why_code: 0x200,
            howto_code: 0x300,
            version: 1,
        }
    }

    let mut set = CheckSet::new("vea103_notify");

    // C103-01 三要素齐全
    {
        let mut n = DegradeNotifier::new(10).unwrap();
        n.register_template(tmpl(DegradeClass::Performance));
        let r = n.report(DegradeClass::Performance, 1);
        let ok = r.is_ok() && r.unwrap().is_some();
        chk(&mut set, "C103-01 三要素", ok, "通知未发");
    }

    // C103-02 同类合并限频
    {
        let mut n = DegradeNotifier::new(10).unwrap();
        n.register_template(tmpl(DegradeClass::Feature));
        let _ = n.report(DegradeClass::Feature, 1);
        let r2 = n.report(DegradeClass::Feature, 2);
        let ok = matches!(r2, Ok(None)) && n.merged_away() >= 1;
        chk(&mut set, "C103-02 合并限频", ok, "未合并");
    }

    // C103-03 窗口过期重开
    {
        let mut n = DegradeNotifier::new(5).unwrap();
        n.register_template(tmpl(DegradeClass::Feature));
        let _ = n.report(DegradeClass::Feature, 1);
        let r = n.report(DegradeClass::Feature, 100);
        chk(&mut set, "C103-03 窗口重开", r.unwrap().is_some(), "未重开");
    }

    // C103-04 模板缺失不静默
    {
        let mut n = DegradeNotifier::new(10).unwrap();
        let r = n.report(DegradeClass::Security, 1);
        let ok = r.is_ok() && n.last_code() == XC_TMPL_MISSING;
        chk(&mut set, "C103-04 不静默", ok, "静默吞事件");
    }

    // C103-05 三要素缺一退化
    {
        let mut n = DegradeNotifier::new(10).unwrap();
        n.register_template(NotifyTemplate {
            class: DegradeClass::Compat,
            what_code: 0x100,
            why_code: 0,
            howto_code: 0x300,
            version: 1,
        });
        let r = n.report(DegradeClass::Compat, 1);
        let ok = r.is_ok() && n.last_code() == XC_TMPL_MISSING;
        chk(&mut set, "C103-05 缺要素退化", ok, "缺要素未退化");
    }

    // C103-06 可回溯合并关系
    {
        let mut n = DegradeNotifier::new(10).unwrap();
        n.register_template(tmpl(DegradeClass::Performance));
        let _ = n.report(DegradeClass::Performance, 1);
        let _ = n.report(DegradeClass::Performance, 2);
        let t = n.trace_window(DegradeClass::Performance);
        let ok = t.is_some() && t.unwrap().1 == 2;
        chk(&mut set, "C103-06 可回溯", ok, "回溯链断");
    }

    // C103-07 通道故障暂存补投
    {
        let mut n = DegradeNotifier::new(10).unwrap();
        n.register_template(tmpl(DegradeClass::Feature));
        n.channel_mut().inject_fault();
        let r = n.report(DegradeClass::Feature, 1);
        let ok = r.is_err() && n.channel().pending_len() == 1;
        let flushed = n.channel_mut().recover_and_flush();
        chk(&mut set, "C103-07 故障补投", ok && flushed == 1, "补投失败");
    }

    // C103-08 窗口零长度拒绝
    {
        let r = DegradeNotifier::new(0);
        chk(
            &mut set,
            "C103-08 窗口拒绝",
            r.err() == Some(XC_WINDOW_BAD),
            "零窗口被放行",
        );
    }

    // C103-09 合并计数带入通知
    {
        let mut n = DegradeNotifier::new(10).unwrap();
        n.register_template(tmpl(DegradeClass::Security));
        let r = n.report(DegradeClass::Security, 1).unwrap().unwrap();
        chk(&mut set, "C103-09 计数带入", r.merged_count == 1, "计数缺失");
    }

    // C103-10 跨类不合并
    {
        let mut n = DegradeNotifier::new(10).unwrap();
        n.register_template(tmpl(DegradeClass::Performance));
        n.register_template(tmpl(DegradeClass::Feature));
        let a = n.report(DegradeClass::Performance, 1);
        let b = n.report(DegradeClass::Feature, 1);
        let ok = a.unwrap().is_some() && b.unwrap().is_some();
        chk(&mut set, "C103-10 跨类独立", ok, "跨类误合并");
    }

    set
}

impl DegradeNotifier {
    pub fn channel(&self) -> &NotifyChannel {
        &self.channel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea103_all_green() {
        let s = run_vea103_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
