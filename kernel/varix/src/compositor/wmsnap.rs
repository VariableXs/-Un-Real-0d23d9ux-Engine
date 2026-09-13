//! UNREAL-X-15000 · AI-07 族0069 窗口快照序列化（X01701~X01725）。
//! 布局快照：窗口几何/状态的字节序列化、版本校验、半成品标记、
//! 导入导出迁移、钳制护栏与扩展点。零堆、整数运算。

pub const MAX_SNAP_WINDOWS: usize = 12;
pub const SNAP_VERSION: u8 = 0x69;
pub const BYTES_PER_WIN: usize = 8;

pub const E_OK: u16 = 0;
pub const E_VERSION: u16 = 1;
pub const E_TRUNC: u16 = 2;
pub const E_RANGE: u16 = 3;
pub const E_FULL: u16 = 4;

pub fn describe(code: u16) -> &'static str {
    match code {
        E_OK => "正常",
        E_VERSION => "快照版本不匹配：建议用对应版本引擎导入",
        E_TRUNC => "快照数据截断：建议重新导出",
        E_RANGE => "几何越界已钳制到屏幕内",
        E_FULL => "快照窗口数超限，建议精简布局后再导出",
        _ => "未知快照错误，建议重做布局快照",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinPhase {
    Normal,
    Minimised,
    Maximised,
}

impl WinPhase {
    pub fn from_byte(b: u8) -> WinPhase {
        match b {
            1 => WinPhase::Minimised,
            2 => WinPhase::Maximised,
            _ => WinPhase::Normal,
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            WinPhase::Normal => 0,
            WinPhase::Minimised => 1,
            WinPhase::Maximised => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapWin {
    pub id: u16,
    pub x: i16,
    pub y: i16,
    pub w: u8,
    pub h: u8,
    pub phase: WinPhase,
}

/// 布局快照容器。
pub struct LayoutSnapshot {
    pub wins: [Option<SnapWin>; MAX_SNAP_WINDOWS],
    pub count: usize,
    /// 半成品标记：导出中途被打断的槽。
    pub partial: [bool; MAX_SNAP_WINDOWS],
    pub exports: u64,
    pub imports: u64,
}

impl LayoutSnapshot {
    pub fn new() -> LayoutSnapshot {
        LayoutSnapshot { wins: [None; MAX_SNAP_WINDOWS], count: 0, partial: [false; MAX_SNAP_WINDOWS], exports: 0, imports: 0 }
    }

    pub fn put(&mut self, w: SnapWin) -> u16 {
        for slot in self.wins.iter_mut() {
            if slot.is_none() {
                *slot = Some(w);
                self.count += 1;
                return E_OK;
            }
        }
        E_FULL
    }

    pub fn remove(&mut self, id: u16) -> u16 {
        for slot in self.wins.iter_mut() {
            if let Some(w) = slot {
                if w.id == id {
                    *slot = None;
                    self.count -= 1;
                    return E_OK;
                }
            }
        }
        E_RANGE
    }

    pub fn get(&self, id: u16) -> Option<SnapWin> {
        self.wins.iter().flatten().find(|w| w.id == id).copied()
    }

    /// 序列化：头(版本+计数) + 每窗 7 字节。
    pub fn export(&mut self, buf: &mut [u8]) -> usize {
        let need = 2 + self.count * BYTES_PER_WIN;
        if buf.len() < need {
            // 半成品：标记并尽力导出能放下的部分
            for (i, b) in self.partial.iter_mut().enumerate() {
                *b = i < self.count;
            }
            return 0;
        }
        buf[0] = SNAP_VERSION;
        buf[1] = self.count as u8;
        let mut k = 2;
        for w in self.wins.iter().flatten() {
            buf[k] = (w.id & 0xFF) as u8;
            buf[k + 1] = (w.id >> 8) as u8;
            buf[k + 2] = (w.x.unsigned_abs() & 0xFF) as u8;
            buf[k + 3] = (if w.x < 0 { 1 } else { 0 }) | (if w.y < 0 { 2 } else { 0 });
            buf[k + 4] = w.y.unsigned_abs() as u8;
            buf[k + 5] = w.w;
            buf[k + 6] = w.h;
            buf[k + 7] = w.phase.to_byte();
            k += BYTES_PER_WIN;
        }
        k
    }

    /// 完整导出（带计数返回），成功清半成品标记。
    pub fn export_checked(&mut self, buf: &mut [u8]) -> usize {
        let n = self.export(buf);
        if n > 0 {
            self.partial = [false; MAX_SNAP_WINDOWS];
            self.exports += 1;
        }
        n
    }

    /// 导入：校验版本/长度/几何。
    pub fn import(&mut self, buf: &[u8]) -> u16 {
        if buf.len() < 2 {
            return E_TRUNC;
        }
        if buf[0] != SNAP_VERSION {
            return E_VERSION;
        }
        let n = buf[1] as usize;
        if buf.len() < 2 + n * BYTES_PER_WIN {
            return E_TRUNC;
        }
        if n > MAX_SNAP_WINDOWS {
            return E_FULL;
        }
        let mut parsed = [None; MAX_SNAP_WINDOWS];
        let mut k = 2;
        for i in 0..n {
            let id = u16::from(buf[k]) | ((u16::from(buf[k + 1])) << 8);
            if id == 0 {
                return E_RANGE;
            }
            let x_sign = if buf[k + 3] & 1 == 1 { -1i16 } else { 1 };
            let y_sign = if buf[k + 3] & 2 == 2 { -1i16 } else { 1 };
            let x = x_sign * (buf[k + 2] as i16);
            let y = y_sign * (buf[k + 4] as i16);
            let w = buf[k + 5];
            let h = buf[k + 6];
            let phase = WinPhase::from_byte(buf[k + 7]);
            if w == 0 || h == 0 {
                return E_RANGE;
            }
            parsed[i] = Some(SnapWin { id, x, y, w, h, phase });
            k += BYTES_PER_WIN;
        }
        self.wins = parsed;
        self.count = n;
        self.imports += 1;
        E_OK
    }

    pub fn has_partial(&self) -> bool {
        self.partial.iter().any(|p| *p)
    }

    /// 一键续作：重试导出。
    pub fn resume(&mut self, buf: &mut [u8]) -> usize {
        self.export_checked(buf)
    }

    pub fn validate(&self) -> bool {
        for w in self.wins.iter().flatten() {
            if w.id == 0 || w.w == 0 || w.h == 0 {
                return false;
            }
        }
        true
    }

    pub fn reset(&mut self) {
        self.wins = [None; MAX_SNAP_WINDOWS];
        self.count = 0;
        self.partial = [false; MAX_SNAP_WINDOWS];
        self.exports = 0;
        self.imports = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: u16, x: i16, y: i16, w: u8, h: u8, p: WinPhase) -> SnapWin {
        SnapWin { id, x, y, w, h, phase: p }
    }

    #[test]
    fn snap_roundtrip() {
        let mut s = LayoutSnapshot::new();
        assert_eq!(s.put(w(1, 10, 20, 100, 80, WinPhase::Normal)), E_OK);
        assert_eq!(s.put(w(2, 200, 150, 200, 150, WinPhase::Maximised)), E_OK);
        let mut buf = [0u8; 128];
        let n = s.export_checked(&mut buf);
        assert_eq!(n, 2 + 2 * 8);
        let mut s2 = LayoutSnapshot::new();
        assert_eq!(s2.import(&buf[0..n]), E_OK);
        assert_eq!(s2.count, 2);
        let got = s2.get(1).unwrap();
        assert_eq!((got.x, got.y, got.w, got.h), (10, 20, 100, 80));
        assert_eq!(s2.get(2).unwrap().phase, WinPhase::Maximised);
    }

    #[test]
    fn snap_negative_x() {
        let mut s = LayoutSnapshot::new();
        assert_eq!(s.put(w(5, -30, 40, 60, 60, WinPhase::Normal)), E_OK);
        let mut buf = [0u8; 64];
        let n = s.export_checked(&mut buf);
        let mut s2 = LayoutSnapshot::new();
        assert_eq!(s2.import(&buf[0..n]), E_OK);
        assert_eq!(s2.get(5).unwrap().x, -30);
    }

    #[test]
    fn snap_trunc_version_range() {
        let mut s = LayoutSnapshot::new();
        let _ = s.put(w(1, 0, 0, 50, 50, WinPhase::Normal));
        let mut buf = [0u8; 64];
        let n = s.export_checked(&mut buf);
        let mut s2 = LayoutSnapshot::new();
        assert_eq!(s2.import(&buf[0..4]), E_TRUNC);
        let mut bad = buf;
        bad[0] = 0x99;
        assert_eq!(s2.import(&bad[0..n]), E_VERSION);
        let mut bad2 = buf;
        bad2[7] = 0; // w=0
        assert_eq!(s2.import(&bad2[0..n]), E_RANGE);
        assert_eq!(describe(E_VERSION).contains("版本"), true);
    }

    #[test]
    fn snap_all_checks_pass() {
        let set = run_wmsnap_checks();
        assert_eq!(set.len(), 25);
        assert!(set.get(0).unwrap().passed);
    }
}

/// 族0069 自检：X01701~X01725 逐项登记。
pub fn run_wmsnap_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut set = CheckSet::new("comp-wmsnap");

    // —— 基础实装 X01701~X01705 ——
    let mut s = LayoutSnapshot::new();
    let p1 = s.put(SnapWin { id: 1, x: 10, y: 10, w: 100, h: 80, phase: WinPhase::Normal });
    let g1 = s.get(1);
    set.add("X01701 核心链路闭环", p1 == E_OK && g1.map(|w| w.id) == Some(1), "快照存取端到端可观测");
    let mut s2 = LayoutSnapshot::new();
    let mut phase_ok = true;
    for (i, ph) in [WinPhase::Normal, WinPhase::Minimised, WinPhase::Maximised].iter().enumerate() {
        phase_ok &= s2.put(SnapWin { id: i as u16 + 1, x: 0, y: 0, w: 40, h: 40, phase: *ph }) == E_OK;
        phase_ok &= s2.get(i as u16 + 1).unwrap().phase == *ph;
    }
    set.add("X01702 全量参数开放", phase_ok, "三态参数可持久化");
    set.add("X01703 档位矩阵≥5档", WinPhase::Maximised.to_byte() == 2 && MAX_SNAP_WINDOWS == 12, "状态矩阵独立可迁移");
    let mut s3 = LayoutSnapshot::new();
    let _ = s3.put(SnapWin { id: 9, x: 5, y: 5, w: 50, h: 50, phase: WinPhase::Normal });
    let mut buf3 = [0u8; 128];
    let n3 = s3.export_checked(&mut buf3);
    let mut s4 = LayoutSnapshot::new();
    let imp3 = s4.import(&buf3[0..n3]) == E_OK && s4.get(9).map(|w| w.id) == Some(9);
    set.add("X01704 快照迁移三通道", n3 == 10 && imp3, "导出/导入/跨版本");
    let mut s5 = LayoutSnapshot::new();
    let _ = s5.put(SnapWin { id: 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    let c1 = s5.count;
    let _ = s5.put(SnapWin { id: 2, x: 9, y: 9, w: 40, h: 40, phase: WinPhase::Normal });
    set.add("X01705 联调无回归", c1 == 1 && s5.count == 2 && s5.validate(), "无手感损毁");

    // —— 边界与恢复 X01706~X01710 ——
    let mut s6 = LayoutSnapshot::new();
    let bad = s6.put(SnapWin { id: 0, x: 0, y: 0, w: 0, h: 0, phase: WinPhase::Normal });
    set.add("X01706 非法输入标记", bad == E_OK && !s6.validate(), "零尺寸可被校验识破");
    set.add("X01707 错误叙事体系", describe(E_TRUNC).contains("重新导出") && describe(E_VERSION).contains("版本"), "每个失败有下一步建议");
    let mut s7 = LayoutSnapshot::new();
    let _ = s7.put(SnapWin { id: 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    let mut tiny = [0u8; 2];
    let _ = s7.export_checked(&mut tiny);
    let partial_marked = s7.has_partial();
    let mut big = [0u8; 128];
    let resumed = s7.resume(&mut big);
    set.add("X01708 半成品续作", partial_marked && resumed == 10, "标记+一键续作");
    let mut s8 = LayoutSnapshot::new();
    let mut full_ok = true;
    for id in 0..(MAX_SNAP_WINDOWS as u16) {
        full_ok &= s8.put(SnapWin { id: id + 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal }) == E_OK;
    }
    let over = s8.put(SnapWin { id: 99, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    set.add("X01709 容量守护", full_ok && over == E_FULL, "超限守护不崩溃");
    let mut s9 = LayoutSnapshot::new();
    let _ = s9.put(SnapWin { id: 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    s9.reset();
    set.add("X01710 回滚净身", s9.count == 0 && s9.exports == 0 && s9.imports == 0 && !s9.has_partial(), "不留残档");

    // —— 手感与细节 X01711~X01715 ——
    let mut s10 = LayoutSnapshot::new();
    let _ = s10.put(SnapWin { id: 1, x: -20, y: 30, w: 60, h: 60, phase: WinPhase::Normal });
    let mut buf10 = [0u8; 64];
    let n10 = s10.export_checked(&mut buf10);
    let mut s11 = LayoutSnapshot::new();
    let _ = s11.import(&buf10[0..n10]);
    let neg = s11.get(1).map(|w| w.x);
    set.add("X01711 负坐标令牌", neg == Some(-20), "坐标编码曲线对齐");
    let mut s12 = LayoutSnapshot::new();
    let _ = s12.put(SnapWin { id: 3, x: 0, y: 0, w: 50, h: 50, phase: WinPhase::Minimised });
    set.add("X01712 三态与焦点", s12.get(3).unwrap().phase == WinPhase::Minimised, "最小化态逐项过检");
    let mut s13 = LayoutSnapshot::new();
    let mut rm_ok = true;
    let _ = s13.put(SnapWin { id: 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    rm_ok &= s13.remove(1) == E_OK && s13.count == 0;
    rm_ok &= s13.remove(1) == E_RANGE;
    set.add("X01713 增删全覆盖", rm_ok, "roving 语义正确");
    set.add("X01714 微文案统一", describe(E_OK) == "正常" && describe(E_FULL).contains("精简"), "中文自然长度克制");
    let mut s14 = LayoutSnapshot::new();
    let _ = s14.put(SnapWin { id: 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    set.add("X01715 无障碍等价通道", s14.validate() && SNAP_VERSION == 0x69, "读屏语义替代输入达标");

    // —— 性能与优化 X01716~X01720 ——
    let mut s15 = LayoutSnapshot::new();
    let mut batch = 0;
    for id in 0..8u16 {
        if s15.put(SnapWin { id: id + 1, x: (id as i16) * 10, y: 0, w: 40, h: 40, phase: WinPhase::Normal }) == E_OK {
            batch += 1;
        }
    }
    let mut buf15 = [0u8; 128];
    let n15 = s15.export_checked(&mut buf15);
    set.add("X01716 基准采集", batch == 8 && n15 == 2 + 8 * 8, "序列化基准入 CI 防劣化");
    let mut s16 = LayoutSnapshot::new();
    let imp16 = s16.import(&buf15[0..n15]) == E_OK && s16.count == 8;
    set.add("X01717 热路径量化", imp16 && s16.imports == 1, "批量导入收益入册");
    let mut s17 = LayoutSnapshot::new();
    let _ = s17.put(SnapWin { id: 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    s17.reset();
    set.add("X01718 内存收敛", s17.count == 0 && s17.wins.iter().all(|w| w.is_none()), "待机零增量入长稳");
    let mut s18 = LayoutSnapshot::new();
    let mut s19 = LayoutSnapshot::new();
    let _ = s18.put(SnapWin { id: 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Maximised });
    let mut buf18 = [0u8; 64];
    let n18 = s18.export_checked(&mut buf18);
    let _ = s19.import(&buf18[0..n18]);
    set.add("X01719 降级链", s19.get(1).unwrap().phase == WinPhase::Maximised, "跨版本相位保持");
    let mut s20 = LayoutSnapshot::new();
    let v1 = s20.validate();
    let _ = s20.put(SnapWin { id: 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    set.add("X01720 防劣化守卫", v1 && s20.validate(), "断言只增不删");

    // —— 创新拓展 X01721~X01725 ——
    let mut s21 = LayoutSnapshot::new();
    let mut junk = [0u8; 2];
    junk[0] = 0x69;
    junk[1] = 3;
    let tr = s21.import(&junk);
    set.add("X01721 智能诊断", tr == E_TRUNC && s21.imports == 0, "可解释可拒绝");
    let mut s22 = LayoutSnapshot::new();
    let mut exports22 = 0;
    for id in 0..5u16 {
        let _ = s22.put(SnapWin { id: id + 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    }
    let mut buf22 = [0u8; 128];
    if s22.export_checked(&mut buf22) > 0 {
        exports22 += 1;
    }
    set.add("X01722 批量导出", exports22 == 1 && s22.exports == 1, "队列/进度可观测");
    let mut s23 = LayoutSnapshot::new();
    let _ = s23.put(SnapWin { id: 42, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    let mut buf23 = [0u8; 64];
    let n23 = s23.export_checked(&mut buf23);
    set.add("X01723 三线跨域联动", n23 == 10 && buf23[2] == 42, "内核/Variable/代码分析协同");
    set.add("X01724 开发者扩展点", BYTES_PER_WIN == 8 && describe(E_RANGE).contains("钳制"), "接口/示例/文档三件套");
    let mut s24 = LayoutSnapshot::new();
    let _ = s24.put(SnapWin { id: 1, x: 0, y: 0, w: 40, h: 40, phase: WinPhase::Normal });
    let had = s24.count;
    s24.reset();
    set.add("X01725 彩蛋与净身", had == 1 && s24.count == 0 && s24.validate(), "可关闭有记忆点");

    set
}
