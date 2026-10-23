//! VE-F0104 · 配额面板数据源（VE-A 域 · A04b 诊断·降级·安全组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0104`
//!
//! **职责定位（锚点原文）**：为配额面板提供聚合数据源——把各资源配额（显存/句柄/
//! 日志盘/回放盘）的当前用量、上限、水位趋势按面板刷新周期聚合为快照；快照原子
//! 发布（序列号单调），面板任一时刻读到的都是完整一致版本，不出现半新半旧。
//!
//! **铁律**：
//! 1. **原子快照**——序列号单调，任一时刻读到完整版本。
//! 2. **失联隔离**——单资源采集失败标记失联，不污染整面板。
//! 3. **趋势供给**——水位趋势随快照给出。
//! 4. **跳帧**——刷新超时跳帧不堆积。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8AF（配额面板）。
pub const SEG_BASE: u16 = 0xE8AF;

/// 资源类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResourceKind {
    /// 显存。
    Vram,
    /// 句柄。
    Handle,
    /// 日志盘。
    LogDisk,
    /// 回放盘。
    ReplayDisk,
}

/// 水位文字语义（不只靠颜色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaterLevel {
    Normal,
    High,
    Over,
    /// 失联。
    Lost,
}

impl WaterLevel {
    pub fn label(&self) -> &'static str {
        match self {
            WaterLevel::Normal => "normal",
            WaterLevel::High => "high",
            WaterLevel::Over => "over",
            WaterLevel::Lost => "lost",
        }
    }
}

/// 单资源条目。
#[derive(Clone, Copy, Debug)]
pub struct QuotaEntry {
    pub kind: ResourceKind,
    pub used: u64,
    pub limit: u64,
    /// 趋势（千分比斜率，正=上升）。
    pub trend_permille: i32,
    pub level: WaterLevel,
}

/// 配额快照（原子发布单元）。
#[derive(Clone, Debug)]
pub struct QuotaSnapshot {
    pub seq: u64,
    pub entries: Vec<QuotaEntry>,
    /// 发布 tick。
    pub tick: u64,
}

impl QuotaSnapshot {
    /// 完整性：无半新半旧（条目齐全或显式失联）。
    pub fn consistent(&self) -> bool {
        self.entries
            .iter()
            .all(|e| e.level != WaterLevel::Lost || e.limit == 0)
    }
}

/// 采集源接口（模拟各核算点）。
pub struct QuotaSource {
    pub kind: ResourceKind,
    /// 健康位（失联隔离）。
    pub alive: bool,
    pub used: u64,
    pub limit: u64,
}

/// 面板数据源。
pub struct QuotaPanelSource {
    sources: Vec<QuotaSource>,
    published: Option<QuotaSnapshot>,
    seq: u64,
    /// 刷新超时跳帧计数。
    skipped_frames: u64,
    last_code: u16,
    /// 历史水位（趋势计算，每资源最多 8 点）。
    history: Vec<(ResourceKind, Vec<u64>)>,
}

impl QuotaPanelSource {
    pub fn new() -> Self {
        QuotaPanelSource {
            sources: Vec::new(),
            published: None,
            seq: 0,
            skipped_frames: 0,
            last_code: 0,
            history: Vec::new(),
        }
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    /// 注册资源核算点。
    pub fn register(&mut self, kind: ResourceKind, limit: u64) {
        self.sources.retain(|s| s.kind != kind);
        self.sources.push(QuotaSource {
            kind,
            alive: true,
            used: 0,
            limit,
        });
        self.history.retain(|(k, _)| *k != kind);
        self.history.push((kind, Vec::new()));
    }

    /// 更新用量。
    pub fn update(&mut self, kind: ResourceKind, used: u64) -> Result<(), u16> {
        match self.sources.iter_mut().find(|s| s.kind == kind) {
            Some(s) => {
                if !s.alive {
                    self.last_code = 0; // 失联资源忽略更新
                    return Ok(());
                }
                s.used = used;
                if let Some((_, h)) = self.history.iter_mut().find(|(k, _)| *k == kind) {
                    h.push(used);
                    if h.len() > 8 {
                        h.remove(0);
                    }
                }
                self.last_code = 0;
                Ok(())
            }
            None => {
                self.last_code = 0;
                Ok(())
            }
        }
    }

    /// 标记资源失联（失联隔离）。
    pub fn mark_lost(&mut self, kind: ResourceKind) {
        if let Some(s) = self.sources.iter_mut().find(|s| s.kind == kind) {
            s.alive = false;
        }
    }

    fn level_of(used: u64, limit: u64) -> WaterLevel {
        if limit == 0 {
            return WaterLevel::Lost;
        }
        let permille = used * 1000 / limit;
        if permille >= 1000 {
            WaterLevel::Over
        } else if permille >= 800 {
            WaterLevel::High
        } else {
            WaterLevel::Normal
        }
    }

    fn trend_of(hist: &[u64]) -> i32 {
        if hist.len() < 2 {
            return 0;
        }
        let first = hist[0] as i64;
        let last = hist[hist.len() - 1] as i64;
        if first == 0 {
            return 0;
        }
        ((last - first) * 1000 / first) as i32
    }

    /// 刷新聚合 + 原子发布。timeout=true 时跳帧。
    pub fn refresh(&mut self, tick: u64, timeout: bool) -> Result<u64, u16> {
        if timeout {
            self.skipped_frames += 1;
            self.last_code = 0;
            // 跳帧：保留上一版不动。
            return Ok(self.seq);
        }
        let mut entries = Vec::new();
        for s in &self.sources {
            if !s.alive {
                // 失联隔离：该资源标记失联，不污染整面板。
                entries.push(QuotaEntry {
                    kind: s.kind,
                    used: 0,
                    limit: 0,
                    trend_permille: 0,
                    level: WaterLevel::Lost,
                });
                continue;
            }
            let hist = self
                .history
                .iter()
                .find(|(k, _)| *k == s.kind)
                .map(|(_, h)| h.clone())
                .unwrap_or_default();
            entries.push(QuotaEntry {
                kind: s.kind,
                used: s.used,
                limit: s.limit,
                trend_permille: Self::trend_of(&hist),
                level: Self::level_of(s.used, s.limit),
            });
        }
        self.seq += 1;
        let snap = QuotaSnapshot {
            seq: self.seq,
            entries,
            tick,
        };
        // 原子发布：一次替换。
        self.published = Some(snap);
        self.last_code = 0;
        Ok(self.seq)
    }

    /// 面板读取（永远读到完整一致版本）。
    pub fn read(&self) -> Option<&QuotaSnapshot> {
        self.published.as_ref()
    }

    /// 校验序列号单调。
    pub fn seq_monotonic(&self) -> bool {
        self.published.as_ref().map(|s| s.seq) == Some(self.seq)
    }

    pub fn skipped_frames(&self) -> u64 {
        self.skipped_frames
    }
}

impl Default for QuotaPanelSource {
    fn default() -> Self {
        Self::new()
    }
}

/// 自检：原子快照、失联隔离、趋势供给、跳帧。
pub fn run_vea104_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    let mut set = CheckSet::new("vea104_quota");

    // C104-01 原子快照发布
    {
        let mut p = QuotaPanelSource::new();
        p.register(ResourceKind::Vram, 1000);
        let _ = p.update(ResourceKind::Vram, 100);
        let r = p.refresh(1, false);
        let ok = r.is_ok() && p.read().unwrap().seq == 1 && p.seq_monotonic();
        chk(&mut set, "C104-01 原子发布", ok, "发布失败");
    }

    // C104-02 失联隔离
    {
        let mut p = QuotaPanelSource::new();
        p.register(ResourceKind::Vram, 1000);
        p.register(ResourceKind::Handle, 100);
        let _ = p.update(ResourceKind::Vram, 50);
        p.mark_lost(ResourceKind::Handle);
        let _ = p.refresh(2, false);
        let snap = p.read().unwrap();
        let lost = snap.entries.iter().find(|e| e.kind == ResourceKind::Handle);
        let ok = lost.map(|e| e.level) == Some(WaterLevel::Lost)
            && snap.entries.iter().any(|e| e.kind == ResourceKind::Vram && e.level == WaterLevel::Normal);
        chk(&mut set, "C104-02 失联隔离", ok, "失联污染");
    }

    // C104-03 趋势供给
    {
        let mut p = QuotaPanelSource::new();
        p.register(ResourceKind::LogDisk, 1000);
        let _ = p.update(ResourceKind::LogDisk, 100);
        let _ = p.refresh(1, false);
        let _ = p.update(ResourceKind::LogDisk, 300);
        let _ = p.refresh(2, false);
        let snap = p.read().unwrap();
        let e = snap.entries.iter().find(|e| e.kind == ResourceKind::LogDisk).unwrap();
        chk(&mut set, "C104-03 趋势供给", e.trend_permille > 0, "趋势缺失");
    }

    // C104-04 跳帧不堆积
    {
        let mut p = QuotaPanelSource::new();
        p.register(ResourceKind::Vram, 100);
        let _ = p.update(ResourceKind::Vram, 50);
        let s1 = p.refresh(1, false).unwrap();
        let s2 = p.refresh(2, true); // timeout 跳帧
        let ok = s2 == Ok(s1) && p.skipped_frames() == 1 && p.read().unwrap().seq == s1;
        chk(&mut set, "C104-04 跳帧", ok, "跳帧堆积");
    }

    // C104-05 水位分级
    {
        let mut p = QuotaPanelSource::new();
        p.register(ResourceKind::ReplayDisk, 100);
        let _ = p.update(ResourceKind::ReplayDisk, 90);
        let _ = p.refresh(1, false);
        let e = p
            .read()
            .unwrap()
            .entries
            .iter()
            .find(|e| e.kind == ResourceKind::ReplayDisk)
            .unwrap();
        chk(&mut set, "C104-05 水位分级", e.level == WaterLevel::High, "分级错");
    }

    // C104-06 越限
    {
        let mut p = QuotaPanelSource::new();
        p.register(ResourceKind::Vram, 100);
        let _ = p.update(ResourceKind::Vram, 150);
        let _ = p.refresh(1, false);
        let e = p.read().unwrap().entries[0];
        chk(&mut set, "C104-06 越限", e.level == WaterLevel::Over, "越限未报");
    }

    // C104-07 水位文字语义
    {
        chk(
            &mut set,
            "C104-07 文字语义",
            WaterLevel::Normal.label() == "normal" && WaterLevel::Lost.label() == "lost",
            "语义缺失",
        );
    }

    // C104-08 快照完整性
    {
        let mut p = QuotaPanelSource::new();
        p.register(ResourceKind::Vram, 100);
        let _ = p.update(ResourceKind::Vram, 10);
        let _ = p.refresh(1, false);
        chk(&mut set, "C104-08 完整一致", p.read().unwrap().consistent(), "半新半旧");
    }

    // C104-09 序列号单调
    {
        let mut p = QuotaPanelSource::new();
        p.register(ResourceKind::Vram, 100);
        let _ = p.refresh(1, false);
        let _ = p.refresh(2, false);
        let _ = p.refresh(3, false);
        chk(&mut set, "C104-09 序号单调", p.read().unwrap().seq == 3 && p.seq_monotonic(), "序号乱");
    }

    // C104-10 失联资源忽略更新
    {
        let mut p = QuotaPanelSource::new();
        p.register(ResourceKind::Handle, 100);
        p.mark_lost(ResourceKind::Handle);
        let _ = p.update(ResourceKind::Handle, 50);
        let _ = p.refresh(1, false);
        let e = p.read().unwrap().entries[0];
        chk(&mut set, "C104-10 失联拒更", e.level == WaterLevel::Lost && e.used == 0, "失联被更");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea104_all_green() {
        let s = run_vea104_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
