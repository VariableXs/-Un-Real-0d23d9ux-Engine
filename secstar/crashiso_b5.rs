//! F175 崩溃隔离强化 · 批次五深化（secstar · G-G-05）。
//!
//! 批次五功能面（与 b3「注册表与预算」、b4「焦点链与退避」互补，本批
//! 管「Z 序账、dump 清单与遮罩交互」）：
//! - [`WindowZBook`]：窗口 Z 序账——应用栈序维护（崩溃者出栈 + 相对
//!   序保持——焦点链的数据底座，一处一事实）；
//! - [`DumpManifest`]：dump 清单——引用号/应用/时刻/大小四字段账
//!   （诊断中心的 dump 索引：按应用/按时段检索）；
//! - [`MaskHotkeys`]：遮罩双钮键盘面——Tab 循环 + Enter 触发 + Esc 取
//!   消（键盘用户和鼠标用户能力对等——第 4 章在遮罩面的落地）；
//! - [`mask_fade_out`]：遮罩淡出（应用重启成功 → 遮罩对称退场——
//!   进场退场对称性第 15 章）。
//!
//! 零堆纪律：定长栈 + 定长清单，无 alloc。

use super::crashiso::{MASK_ANIM_MS, MASK_DIM_PERMILLE};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 窗口 Z 序账
// ---------------------------------------------------------------------------

/// Z 序容量。
pub const Z_CAP: usize = 16;

/// Z 序栈：index 0 = 最顶。崩溃者出栈，其余相对序保持。
pub struct WindowZBook {
    stack: [Option<u32>; Z_CAP],
    pub n: usize,
}

impl WindowZBook {
    pub const fn new() -> WindowZBook {
        WindowZBook { stack: [const { None }; Z_CAP], n: 0 }
    }

    /// 应用提到最顶（激活语义——保持最近使用序）。
    pub fn raise(&mut self, app_id: u32) -> bool {
        if self.n >= Z_CAP && !self.contains(app_id) {
            return false;
        }
        // 先出栈再压顶。
        self.remove(app_id);
        for i in (1..=self.n).rev() {
            self.stack[i] = self.stack[i - 1];
        }
        self.stack[0] = Some(app_id);
        self.n += 1;
        true
    }

    pub fn remove(&mut self, app_id: u32) -> bool {
        match (0..self.n).find(|i| self.stack[*i] == Some(app_id)) {
            Some(i) => {
                for j in i..self.n - 1 {
                    self.stack[j] = self.stack[j + 1];
                }
                self.stack[self.n - 1] = None;
                self.n -= 1;
                true
            }
            None => false,
        }
    }

    pub fn contains(&self, app_id: u32) -> bool {
        (0..self.n).any(|i| self.stack[i] == Some(app_id))
    }

    /// 崩溃者的接任者（Z 序下一个——与 b4 FocusChain 同判但独立对账）。
    pub fn successor_after_remove(&mut self, crashed: u32) -> Option<u32> {
        let pos = (0..self.n).find(|i| self.stack[*i] == Some(crashed))?;
        self.remove(crashed);
        if self.n == 0 {
            return None;
        }
        Some(self.stack[pos.min(self.n - 1)].unwrap())
    }

    /// 激活序快照（栈顶向下的 app 序——诊断导出面）。
    pub fn snapshot(&self, out: &mut [Option<u32>; Z_CAP]) -> usize {
        for i in 0..self.n {
            out[i] = self.stack[i];
        }
        self.n
    }
}

// ---------------------------------------------------------------------------
// dump 清单
// ---------------------------------------------------------------------------

/// 清单容量。
pub const DUMP_MANIFEST_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DumpEntry {
    pub dump_ref: u32,
    pub app_id: u32,
    pub at_ms: u64,
    pub size_bytes: u32,
}

pub struct DumpManifest {
    entries: [Option<DumpEntry>; DUMP_MANIFEST_CAP],
    pub n: usize,
}

impl DumpManifest {
    pub const fn new() -> DumpManifest {
        DumpManifest { entries: [const { None }; DUMP_MANIFEST_CAP], n: 0 }
    }

    /// 登记（引用号唯一——同一现场不重复入册）。
    pub fn add(&mut self, e: DumpEntry) -> bool {
        if self.n >= DUMP_MANIFEST_CAP || self.entries[..self.n].iter().flatten().any(|x| x.dump_ref == e.dump_ref) {
            return false;
        }
        self.entries[self.n] = Some(e);
        self.n += 1;
        true
    }

    /// 按应用检索（诊断中心「这个应用崩了几次」）。
    pub fn by_app(&self, app_id: u32, out: &mut [u32; DUMP_MANIFEST_CAP]) -> usize {
        let mut k = 0;
        for e in self.entries[..self.n].iter().flatten() {
            if e.app_id == app_id && k < DUMP_MANIFEST_CAP {
                out[k] = e.dump_ref;
                k += 1;
            }
        }
        k
    }

    /// 总体积（诊断中心「dump 占了多少盘」——诚实披露资源占用）。
    pub fn total_bytes(&self) -> u64 {
        self.entries[..self.n].iter().flatten().map(|e| e.size_bytes as u64).sum()
    }

    /// 淘汰最旧（超容腾位——清单也内存有上限）。
    pub fn evict_oldest(&mut self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let mut oldest = 0;
        for i in 1..self.n {
            if self.entries[i].unwrap().at_ms < self.entries[oldest].unwrap().at_ms {
                oldest = i;
            }
        }
        let evicted = self.entries[oldest].take().unwrap();
        for j in oldest..self.n - 1 {
            self.entries[j] = self.entries[j + 1];
        }
        self.entries[self.n - 1] = None;
        self.n -= 1;
        Some(evicted.dump_ref)
    }
}

// ---------------------------------------------------------------------------
// 遮罩双钮键盘面
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskFocus {
    RestartButton,
    CloseMaskButton,
}

/// 遮罩焦点状态机：Tab 循环 / Enter 触发焦点钮 / Esc = 取消按钮语义。
pub struct MaskHotkeys {
    pub focus: MaskFocus,
}

impl MaskHotkeys {
    pub const fn new() -> MaskHotkeys {
        MaskHotkeys { focus: MaskFocus::RestartButton }
    }

    /// Tab 循环（双钮两态互转——焦点永不丢失）。
    pub fn tab(&mut self) {
        self.focus = match self.focus {
            MaskFocus::RestartButton => MaskFocus::CloseMaskButton,
            MaskFocus::CloseMaskButton => MaskFocus::RestartButton,
        };
    }

    /// Enter 触发当前焦点钮（true=重启请求 / false=关遮罩请求）。
    pub fn enter(&self) -> bool {
        self.focus == MaskFocus::RestartButton
    }

    /// Esc = 关遮罩钮语义（取消永远是安全出路——第 9 章红线）。
    pub fn esc(&mut self) -> bool {
        self.focus = MaskFocus::CloseMaskButton;
        false
    }
}

// ---------------------------------------------------------------------------
// 遮罩淡出（进退场对称）
// ---------------------------------------------------------------------------

/// 淡出帧暗度：与进场共用同一条 ease-out 曲线（`crashiso_b4::
/// mask_dim_at`）取补——「同一曲线的正反两用」，对称性由构造保证，
/// 不另造第二条曲线。
pub fn mask_fade_out_dim(frame: usize, total_frames: usize) -> u32 {
    if total_frames == 0 {
        return 0;
    }
    let rise = super::crashiso_b4::mask_dim_at(frame.min(total_frames - 1));
    MASK_DIM_PERMILLE.saturating_sub(rise)
}

/// 淡出单调不升 + 首帧满暗度 + 终帧归零（对称性的机械判定）。
pub fn fade_out_monotone(total_frames: usize) -> bool {
    if total_frames == 0 {
        return false;
    }
    let mut prev = MASK_DIM_PERMILLE;
    for f in 0..total_frames {
        let d = mask_fade_out_dim(f, total_frames);
        if d > prev {
            return false;
        }
        prev = d;
    }
    mask_fade_out_dim(0, total_frames) == MASK_DIM_PERMILLE
        && mask_fade_out_dim(total_frames - 1, total_frames) == 0
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_crashiso_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F175-b5");

    // 1) Z 序账：激活置顶语义（raise 后在 index 0）。
    let mut z = WindowZBook::new();
    z.raise(10);
    z.raise(20);
    z.raise(30);
    z.raise(10); // 10 再激活 → 顶
    let mut snap = [None; Z_CAP];
    z.snapshot(&mut snap);
    cs.add(
        "zbook_raise_top",
        snap[0] == Some(10) && snap[1] == Some(30) && snap[2] == Some(20) && z.n == 3,
        "",
    );

    // 2) Z 序接任：崩 30（index1）→ 接任其下一位 20（Z 序下一窗）。
    let next = z.successor_after_remove(30);
    cs.add("zbook_successor", next == Some(20) && !z.contains(30), "");

    // 3) Z 序全灭：[10,20] 崩 10 接 20、崩 20 归桌面、再崩不存在 = None。
    let s2 = z.successor_after_remove(10);
    let s3 = z.successor_after_remove(20);
    let s4 = z.successor_after_remove(10);
    cs.add(
        "zbook_exhaust",
        s2 == Some(20) && s3.is_none() && s4.is_none() && z.n == 0,
        "",
    );

    // 4) dump 清单：登记/引用号去重（同一现场不重复入册）。
    let mut m = DumpManifest::new();
    let ok = m.add(DumpEntry { dump_ref: 1, app_id: 7, at_ms: 100, size_bytes: 4096 });
    let dup = m.add(DumpEntry { dump_ref: 1, app_id: 7, at_ms: 200, size_bytes: 4096 });
    cs.add("manifest_ref_dedup", ok && !dup && m.n == 1, "");

    // 5) dump 按应用检索：app7 两条、app8 一条（诊断中心消费面）。
    m.add(DumpEntry { dump_ref: 2, app_id: 7, at_ms: 300, size_bytes: 8192 });
    m.add(DumpEntry { dump_ref: 3, app_id: 8, at_ms: 400, size_bytes: 2048 });
    let mut out = [0u32; DUMP_MANIFEST_CAP];
    let k = m.by_app(7, &mut out);
    cs.add("manifest_by_app", k == 2 && out[0] == 1 && out[1] == 2, "");

    // 6) dump 总体积：4096+8192+2048 = 14336（资源占用诚实披露）。
    cs.add("manifest_total_bytes", m.total_bytes() == 14_336, "");

    // 7) dump 淘汰最旧：evict 拿走 at_ms 最小者（清单内存有上限）。
    let evicted = m.evict_oldest();
    cs.add("manifest_evict_oldest", evicted == Some(1) && m.n == 2, "");

    // 8) 遮罩键盘：Tab 循环 / Enter 按焦点 / Esc = 关遮罩（能力对等）。
    let mut kh = MaskHotkeys::new();
    let enter_restart = kh.enter();
    kh.tab();
    let enter_close = !kh.enter();
    kh.tab();
    let back_restart = kh.enter();
    let esc_closes = !kh.esc();
    cs.add(
        "mask_hotkeys",
        enter_restart && enter_close && back_restart && esc_closes && kh.focus == MaskFocus::CloseMaskButton,
        "",
    );

    // 9) 淡出对称：单调不升 + 终帧归零（进退场对称机械判定）。
    cs.add("fade_out_monotone", fade_out_monotone(12), "");

    // 10) 淡出首帧 = 满暗度（退场起点=进场终点——对称锚）。
    cs.add("fade_out_starts_full", mask_fade_out_dim(0, 12) == MASK_DIM_PERMILLE, "");

    // 11) 主册常量贯通：遮罩 200ms / 200‰ 一处一事实。
    cs.add("consts_aligned", MASK_ANIM_MS == 200 && MASK_DIM_PERMILLE == 200, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn zbook_full_capacity() {
        // 16 窗满容：新窗 raise 拒（Z 序内存上限）。
        let mut z = WindowZBook::new();
        for i in 0..Z_CAP as u32 {
            assert!(z.raise(100 + i));
        }
        assert!(!z.raise(999));
        assert_eq!(z.n, Z_CAP);
        // 已在册的窗 raise 仍可（重激活不是新窗）。
        assert!(z.raise(100));
        assert_eq!(z.n, Z_CAP);
    }

    #[test]
    fn manifest_cap_evicts() {
        // 16 满后 add 拒、淘汰最旧腾位（清单容量纪律闭环）。
        let mut m = DumpManifest::new();
        for i in 0..DUMP_MANIFEST_CAP as u64 {
            assert!(m.add(DumpEntry { dump_ref: i as u32 + 1, app_id: 1, at_ms: i * 1000, size_bytes: 100 }));
        }
        assert_eq!(m.n, DUMP_MANIFEST_CAP);
        assert!(!m.add(DumpEntry { dump_ref: 99, app_id: 2, at_ms: 99_000, size_bytes: 100 }));
        let evicted = m.evict_oldest().unwrap();
        assert_eq!(evicted, 1); // at_ms=0 最旧
        assert!(m.add(DumpEntry { dump_ref: 99, app_id: 2, at_ms: 99_000, size_bytes: 100 }));
        assert_eq!(m.n, DUMP_MANIFEST_CAP);
    }

    #[test]
    fn fade_out_complement_of_rise() {
        // 曲线互补对称：退场第 f 帧暗度 + 进场第 f 帧暗度 == 满暗度
        // （同一曲线的正反两用——对称由构造保证）。
        for f in 0..12usize {
            assert_eq!(
                mask_fade_out_dim(f, 12) + crate::secstar::crashiso_b4::mask_dim_at(f),
                MASK_DIM_PERMILLE,
                "frame {f}"
            );
        }
    }
}
