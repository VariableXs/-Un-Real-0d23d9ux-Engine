//! F244 注册表扩展方法（[`HotkeyReg`] 的后续 `impl` 块）：冲突预检与
//! 派发留痕、序列键（chord）运行面、批量查询与应用注销、持久化导出
//! 导入、批量改键与检索。
//!
//! 本文件是 `super`（registry）的子模块——表内核私有方法与字段对本
//! 模块可见（后代模块可见性），类型本体与核心注册/改键面见 `super`。

use alloc::vec::Vec;

use super::super::keys::{Combo, MOD_MASK};
use super::{
    chord_uid, AppId, Change, ChangeKind, ChordRec, ChordStep, DispatchRec, Entry, HotkeyReg,
    Occupant, RebindOutcome, Source, CATEGORY_NAMES, CHORD_TIMEOUT_MS, DISPATCH_CAP, HK_CAP,
    PRESETS, PRESET_CAT, uid_of_label,
};

impl HotkeyReg {
    // -- 查询 / 派发 -----------------------------------------------------------

    /// 槽序过滤收集（批量查询面的共享实现——消重归并：list /
    /// entries_of_app / system_entries / entry_labels 四处同构的
    /// 「槽序遍历 + 谓词筛选 + out 容量截断」）。返回写入条数。
    pub(crate) fn collect_slots<P: Fn(&Entry) -> bool>(&self, pred: P, out: &mut [Entry]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if pred(e) && k < out.len() {
                out[k] = *e;
                k += 1;
            }
        }
        k
    }

    /// 派发：键位 → 条目（禁用条目仍返回，由调用方按 enabled 处理——
    /// 设置中心「可查」与运行面「不触发」分离）。
    pub fn dispatch(&self, combo: Combo) -> Option<Entry> {
        self.probe(combo.code()).and_then(|i| self.slots[i])
    }

    /// 活跃派发：只命中启用条目（运行面用这个）。
    pub fn dispatch_active(&self, combo: Combo) -> Option<Entry> {
        self.dispatch(combo).filter(|e| e.enabled)
    }

    pub fn entry_by_uid(&self, uid: u32) -> Option<Entry> {
        self.find_uid_idx(uid).and_then(|i| self.slots[i])
    }

    /// 槽位直读（fuzz/审计面用；越界 None）。
    pub fn slot_at(&self, i: usize) -> Option<Entry> {
        if i < HK_CAP {
            self.slots[i]
        } else {
            None
        }
    }

    /// 全表快照（槽序，诊断面）。
    pub fn list(&self, out: &mut [Entry]) -> usize {
        self.collect_slots(|_| true, out)
    }

    fn overridden_count(&self) -> usize {
        self.slots.iter().flatten().filter(|e| e.overridden || !e.enabled).count()
    }

    // -- 冲突预检 / 派发留痕 ---------------------------------------------------

    /// 键位占用预检（设置页「此键位已被占用」实时提示的数据面——
    /// register/rebind 冲突门的只读版）。
    pub fn conflict_of(&self, combo: Combo) -> Option<Entry> {
        self.probe(combo.code()).and_then(|i| self.slots[i])
    }

    /// 带留痕派发（运行面唯一入口）：命中即写派发环并按槽位计热。
    /// 禁用条目同样留痕（active=false）但不触发——可查与可触发分离。
    pub fn dispatch_logged(&mut self, combo: Combo, ts: u64) -> Option<Entry> {
        let hit = self.dispatch(combo);
        if let Some(e) = hit {
            let uid = e.uid();
            if let Some(i) = self.probe(combo.code()) {
                self.hits[i] = self.hits[i].wrapping_add(1);
            }
            self.dlog.push(DispatchRec { ts, uid, combo, active: e.enabled });
        }
        hit
    }

    /// 派发留痕（新→旧，最多 32 条）。
    pub fn dispatch_log(&self) -> [Option<DispatchRec>; DISPATCH_CAP] {
        let mut out = [const { None }; DISPATCH_CAP];
        for (k, rec) in self.dlog.newest_first().iter().enumerate() {
            out[k] = Some(*rec);
        }
        out
    }

    /// 派发环条数（容量有界性核对）。
    pub fn dispatch_log_len(&self) -> usize {
        self.dlog.len()
    }

    /// 某 uid 的累计派发次数（热度画像面；未命中过为 0）。
    pub fn hits_for(&self, uid: u32) -> u32 {
        self.find_uid_idx(uid).map_or(0, |i| self.hits[i])
    }

    /// 派发总次数（= 全槽计数之和；画像对账面）。
    pub fn total_hits(&self) -> u32 {
        self.hits.iter().fold(0u32, |a, &h| a.wrapping_add(h))
    }

    /// 某条 uid 的变更链（旧→新——设置页条目详情的历史页签）。
    pub fn changes_for_uid(&self, uid: u32) -> Vec<Change> {
        let mut out: Vec<Change> = self
            .changes
            .newest_first()
            .iter()
            .copied()
            .filter(|c| c.uid == uid)
            .collect();
        out.reverse();
        out
    }

    /// 变更环条数。
    pub fn change_log_len(&self) -> usize {
        self.changes.len()
    }

    // -- 序列键（chord） -------------------------------------------------------

    /// 序列键登记：两段都不得与单键表任何键位冲突（否则单键被序列
    /// 「吞掉」——结构性排除），第一段在序列表内必须唯一（命中即武装，
    /// 二义性不允许）。表满显性拒绝。
    pub fn register_chord(&mut self, label: &'static str, first: Combo, second: Combo, ts: u64) -> bool {
        if !(first.sane() && second.sane()) {
            return false;
        }
        if self.probe(first.code()).is_some() || self.probe(second.code()).is_some() {
            return false;
        }
        if self.chords.iter().flatten().any(|c| c.first == first) {
            return false;
        }
        for slot in self.chords.iter_mut() {
            if slot.is_none() {
                *slot = Some(ChordRec { label, first, second, ts, overridden: false });
                self.version = self.version.wrapping_add(1);
                return true;
            }
        }
        false
    }

    /// 按语义名定位序列键槽（消重归并：unregister_chord / chord_rebind
    /// 两处同构的「按 label 找槽」查找面）。
    fn chord_slot_idx(&self, label: &str) -> Option<usize> {
        self.chords.iter().position(|s| matches!(s, Some(c) if c.label == label))
    }

    /// 序列键注销（按语义名；未在册如实返回 false）。
    pub fn unregister_chord(&mut self, label: &str) -> bool {
        // let-else 展平（原 for > if let > if 嵌套 3 层 → 1 层）。
        let Some(i) = self.chord_slot_idx(label) else {
            return false;
        };
        self.chords[i] = None;
        self.version = self.version.wrapping_add(1);
        true
    }

    /// 序列键改段（label 在册才可改；新段位重过冲突门）。
    pub fn chord_rebind(&mut self, label: &str, first: Combo, second: Combo) -> bool {
        if !(first.sane() && second.sane()) {
            return false;
        }
        // let-else 展平（原 for > if let > if 嵌套 3 层 → 1 层）。
        let Some(i) = self.chord_slot_idx(label) else {
            return false;
        };
        if self.chord_segment_conflicts(label, first) || self.chord_segment_conflicts(label, second) {
            return false;
        }
        let Some(c) = self.chords[i] else {
            return false;
        };
        self.chords[i] = Some(ChordRec { label: c.label, first, second, ts: c.ts, overridden: true });
        self.version = self.version.wrapping_add(1);
        true
    }

    /// 段位冲突复核：该段码不得等于任何「其他序列键的段」或「单键表
    /// 键位」（自身旧段除外——改段前的旧值不算冲突）。
    fn chord_segment_conflicts(&self, label: &str, seg: Combo) -> bool {
        if self.probe(seg.code()).is_some() {
            return true;
        }
        self.chords
            .iter()
            .flatten()
            .any(|c| c.label != label && (c.first == seg || c.second == seg))
    }

    /// 武装态单步（chord_key 内联提取——嵌套 4 层压至 1 层）：等待窗内
    /// 第二段命中 → Fired；其余情况回落。武装态在本步内一律清除（与
    /// 原实现等价：原 Fired 分支与回落路径均清武装，本函数无中途观察者）。
    fn chord_armed_step(&mut self, combo: Combo, ts: u64) -> Option<ChordStep> {
        let (idx, t0) = self.chord_armed?;
        self.chord_armed = None;
        let fired = ts.saturating_sub(t0) <= CHORD_TIMEOUT_MS
            && self.chords[idx].map_or(false, |c| c.second == combo);
        if fired {
            return self.chords[idx].map(|c| ChordStep::Fired { label: c.label });
        }
        None
    }

    /// 运行面逐键判定：Idle（继续普通派发）/ Armed（吞键等待）/ Fired。
    /// 等待窗超时自动回落（时间注入，模块不持时钟）。
    pub fn chord_key(&mut self, combo: Combo, ts: u64) -> ChordStep {
        // 1. 武装态优先：窗内认第二段；超时/不匹配回落（提取面
        //    chord_armed_step——嵌套压平，判定次序不变）。
        if let Some(step) = self.chord_armed_step(combo, ts) {
            return step;
        }
        // 2. 第一段命中 → 武装（本击吞掉，不派发动作）。
        for (i, slot) in self.chords.iter().enumerate() {
            if slot.map_or(false, |c| c.first == combo) {
                self.chord_armed = Some((i, ts));
                return ChordStep::Armed;
            }
        }
        ChordStep::Idle
    }

    /// 当前武装中的序列键语义名（设置页「等待第二段」提示面）。
    pub fn chord_armed_label(&self) -> Option<&'static str> {
        self.chord_armed
            .and_then(|(i, _)| self.chords[i].as_ref())
            .map(|c| c.label)
    }

    /// 显性超时清除（设置页测试面的主动复位；返回清除前是否武装）。
    pub fn expire_chord(&mut self) -> bool {
        let was = self.chord_armed.is_some();
        self.chord_armed = None;
        was
    }

    /// 序列键条数。
    pub fn chord_count(&self) -> usize {
        self.chords.iter().flatten().count()
    }

    /// 按语义名查序列键。
    pub fn chord_by_label(&self, label: &str) -> Option<ChordRec> {
        self.chords.iter().flatten().find(|c| c.label == label).copied()
    }

    /// 序列键语义名清单（去重列出；写入 out 返回条数）。
    pub fn chord_labels(&self, out: &mut [&'static str]) -> usize {
        let mut k = 0;
        for c in self.chords.iter().flatten() {
            if k < out.len() {
                out[k] = c.label;
                k += 1;
            }
        }
        k
    }

    /// 键位是否为某序列键的第一段（派发路由的预判面）。
    pub fn is_chord_first(&self, combo: Combo) -> bool {
        self.chords.iter().flatten().any(|c| c.first == combo)
    }

    /// 序列键段位审计：全部段位不与单键表冲突、第一段互异——注册门
    /// 的独立复核面（结构被未来改动破坏时先红）。
    pub fn audit_chord_no_collision(&self) -> bool {
        let recs: Vec<ChordRec> = self.chords.iter().flatten().copied().collect();
        for c in recs.iter() {
            if self.probe(c.first.code()).is_some() || self.probe(c.second.code()).is_some() {
                return false;
            }
        }
        for i in 0..recs.len() {
            for j in (i + 1)..recs.len() {
                if recs[i].first == recs[j].first {
                    return false;
                }
            }
        }
        true
    }

    /// 单键表两两键位码互异审计（线性探测表的结构性保证——独立复核
    /// 面，与探测/删除单测互补）。
    pub fn audit_conflict_free(&self) -> bool {
        let entries: Vec<Entry> = self.slots.iter().flatten().copied().collect();
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                if entries[i].combo.code() == entries[j].combo.code() {
                    return false;
                }
            }
        }
        true
    }

    // -- 批量查询 / 应用注销 ---------------------------------------------------

    /// 某应用的全部条目（注册序；写入 out 返回条数）。
    pub fn entries_of_app(&self, app: &AppId, out: &mut [Entry]) -> usize {
        self.collect_slots(|e| e.source == Source::App(*app), out)
    }

    /// 某应用的全部 uid（注销对账面）。
    pub fn app_uids(&self, app: &AppId, out: &mut [u32]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if e.source == Source::App(*app) && k < out.len() {
                out[k] = e.uid();
                k += 1;
            }
        }
        k
    }

    /// 系统条目清单（预置走查面）。
    pub fn system_entries(&self, out: &mut [Entry]) -> usize {
        self.collect_slots(|e| e.is_system(), out)
    }

    /// 全表语义名清单（诊断导出面）。
    pub fn entry_labels(&self, out: &mut [&'static str]) -> usize {
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if k < out.len() {
                out[k] = e.label;
                k += 1;
            }
        }
        k
    }

    /// 应用批量注销（卸载清理）：逐条走后退移位删除，留痕 Unregister。
    /// 反复「找第一条 → 删」直至无——回避迭代中探测链搬移的遍历陷阱。
    pub fn unregister_app(&mut self, app: AppId, ts: u64) -> usize {
        let mut n = 0;
        while let Some(idx) = self
            .slots
            .iter()
            .position(|s| matches!(s, Some(e) if e.source == Source::App(app)))
        {
            // position 命中 ⇒ 槽位必在册；None 防御分支显性退出（不 panic）。
            let Some(e) = self.slots[idx] else {
                break;
            };
            let uid = e.uid();
            self.remove_at(idx);
            self.count -= 1;
            self.push_change(Change { ts, kind: ChangeKind::Unregister, uid, old: e.combo, new: Combo::none() });
            n += 1;
        }
        if n > 0 {
            self.version += 1;
        }
        n
    }

    // -- 预置分组 --------------------------------------------------------------

    /// 预置分组计数（设置中心分类树概览）：只统计**已在册**的预置条目
    /// ——安装半途/被注销时计数如实反映缺额，与 [`PRESET_CAT`] 对应。
    pub fn preset_category_counts(&self) -> [u32; 4] {
        let mut counts = [0u32; 4];
        for (i, (label, _, _)) in PRESETS.iter().enumerate() {
            if self.find_uid_idx(uid_of_label(label)).is_some() {
                counts[PRESET_CAT[i] as usize] += 1;
            }
        }
        counts
    }

    // -- 运行面专用派发 --------------------------------------------------------

    /// 活跃派发带留痕（运行面用这个）：只认启用条目，禁用命中留痕但
    /// 返回 None——「可查」与「可触发」在一个入口内分流。
    pub fn dispatch_active_logged(&mut self, combo: Combo, ts: u64) -> Option<Entry> {
        let hit = self.dispatch_logged(combo, ts).filter(|e| e.enabled);
        hit
    }

    // -- 批量改键 / 占用查询 / 检索 -------------------------------------------

    /// 批量改键（配置装配面）：逐条过冲突门，返回成功数——部分成功
    /// 不是静默：调用方可用 rebind 逐条复核失败项。
    pub fn rebind_many(&mut self, pairs: &[(u32, Combo)], ts: u64) -> usize {
        let mut ok = 0;
        for &(uid, combo) in pairs.iter() {
            if self.rebind(uid, combo, ts) == RebindOutcome::Done {
                ok += 1;
            }
        }
        ok
    }

    /// 批量占用查询（设置中心「导入前预检」面）：对申请键位清单逐个
    /// 给占用方（无占用为 None），数量与输入对齐。
    pub fn find_occupants(&self, combos: &[Combo], out: &mut [Option<Occupant>]) -> usize {
        let n = combos.len().min(out.len());
        for (o, &c) in out[..n].iter_mut().zip(combos.iter()) {
            // 占用方构造走公共面 occupant_at（与注册/改键冲突门同源）。
            *o = self.probe(c.code()).map(|i| self.occupant_at(i));
        }
        n
    }

    /// 按语义名前缀检索（设置中心搜索框数据面；系统条目 label 前缀）。
    pub fn entries_by_prefix(&self, prefix: &str, out: &mut [Entry]) -> usize {
        let pb = prefix.as_bytes();
        let mut k = 0;
        for e in self.slots.iter().flatten() {
            if e.label.len() >= pb.len() && &e.label.as_bytes()[..pb.len()] == pb && k < out.len() {
                out[k] = *e;
                k += 1;
            }
        }
        k
    }

    /// 分类叶面：某预置分组的语义名清单（设置中心分类树展开）。
    pub fn preset_labels_by_cat(&self, cat: usize, out: &mut [&'static str]) -> usize {
        if cat >= CATEGORY_NAMES.len() {
            return 0;
        }
        let mut k = 0;
        for (i, (label, _, _)) in PRESETS.iter().enumerate() {
            if PRESET_CAT[i] as usize == cat && k < out.len() {
                out[k] = *label;
                k += 1;
            }
        }
        k
    }

    // -- 序列键差异面持久化 -----------------------------------------------------

    /// 导出序列键差异（被改过段的条目）：`VXHC` 头 + 条数 u16LE + 每条
    /// 12 字节（uid u32LE、第一段 mods+key u16LE、第二段 mods+key）。
    /// label 是 'static 语义名不入差异面——导入按 uid 改已在册条目段位。
    pub fn export_chords(&self, out: &mut [u8]) -> usize {
        let n = self.chords.iter().flatten().filter(|c| c.overridden).count();
        let need = 6 + n * 12;
        if out.len() < need {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXHC");
        out[4..6].copy_from_slice(&(n as u16).to_le_bytes());
        let mut p = 6;
        for c in self.chords.iter().flatten() {
            if c.overridden {
                out[p..p + 4].copy_from_slice(&chord_uid(c).to_le_bytes());
                out[p + 4] = c.first.mods;
                out[p + 5..p + 7].copy_from_slice(&c.first.key.to_le_bytes());
                out[p + 7] = c.second.mods;
                out[p + 9..p + 11].copy_from_slice(&c.second.key.to_le_bytes());
                p += 12;
            }
        }
        need
    }

    /// 导入序列键差异：逐条按 uid 定位在册序列键 → 过冲突门 → 改段。
    /// 返回成功数；未在册 uid/冲突/越界段跳过并如实计数（缓冲截断面）。
    pub fn import_chords(&mut self, buf: &[u8]) -> usize {
        if buf.len() < 6 || &buf[0..4] != b"VXHC" {
            return 0;
        }
        let n = u16::from_le_bytes([buf[4], buf[5]]) as usize;
        let mut applied = 0;
        for k in 0..n {
            let base = 6 + k * 12;
            if base + 12 > buf.len() {
                break;
            }
            let uid = u32::from_le_bytes([buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]);
            let first = Combo::new(buf[base + 4], u16::from_le_bytes([buf[base + 5], buf[base + 6]]));
            let second = Combo::new(buf[base + 7], u16::from_le_bytes([buf[base + 9], buf[base + 10]]));
            let label = self
                .chords
                .iter()
                .flatten()
                .find(|c| chord_uid(c) == uid)
                .map(|c| c.label);
            // map_or 归并（原 if let > if 嵌套 2 层 → 1 层）：命中即过
            // 冲突门改段，成功计数。
            if label.map_or(false, |l| self.chord_rebind(l, first, second)) {
                applied += 1;
            }
        }
        applied
    }

    /// 槽位热度向量全量导出（与槽序一一对应——设置中心热度排序与
    /// 诊断对账面；派发计数的成批读出，免逐 uid 查表）。
    pub fn hits_snapshot(&self, out: &mut [u32]) -> usize {
        let n = HK_CAP.min(out.len());
        out[..n].copy_from_slice(&self.hits[..n]);
        n
    }

    /// 序列键全量导出（诊断面；表序写入 out 返回条数）。
    pub fn chords_all(&self, out: &mut [ChordRec]) -> usize {
        let mut k = 0;
        for c in self.chords.iter().flatten() {
            if k < out.len() {
                out[k] = *c;
                k += 1;
            }
        }
        k
    }

    // -- 持久化（用户改键差异面） ---------------------------------------------

    /// 导出用户差异（改键/禁用条目）：`VXHK` 头 + 条数 u16LE +
    /// 每条 9 字节（uid u32LE、mods、key u16LE、flags）。缓冲不足返回 0。
    pub fn export_overrides(&self, out: &mut [u8]) -> usize {
        let n = self.overridden_count();
        let need = 6 + n * 9;
        if out.len() < need || need > 0xFFFF + 6 {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXHK");
        out[4..6].copy_from_slice(&(n as u16).to_le_bytes());
        let mut p = 6;
        for e in self.slots.iter().flatten() {
            if e.overridden || !e.enabled {
                out[p..p + 4].copy_from_slice(&e.uid().to_le_bytes());
                out[p + 4] = e.combo.mods;
                out[p + 5..p + 7].copy_from_slice(&e.combo.key.to_le_bytes());
                out[p + 7] = e.enabled as u8 | ((e.overridden as u8) << 1);
                p += 9;
            }
        }
        p
    }

    /// 单条差异装载（import_overrides 内联提取——嵌套 4 层压至 1 层，
    /// 语义与执行顺序与原实现逐行一致）：冲突复核 → 键位变化走「摘除
    /// +家址重插」（修障登记：与 rebind 同病同修，原地改写会让新键位
    /// 的家址探测链够不着条目，装载后派发丢失）→ 留痕。返回是否生效。
    fn load_override_entry(&mut self, uid: u32, target: Combo, flags: u8, ts: u64) -> bool {
        let Some(idx) = self.find_uid_idx(uid) else {
            return false;
        };
        if self.probe(target.code()).map_or(false, |j| j != idx) {
            return false;
        }
        // uid 命中 ⇒ 槽位必在册；None 防御分支显性跳过（不 panic）。
        let Some(mut e) = self.slots[idx] else {
            return false;
        };
        let old = e.combo;
        if old != target {
            // 摘一插一必有空槽；不成立的兜底是显性回滚并留痕，不静默
            // （回滚段归并 reinsert_entry 公共面）。
            let was_overridden = e.overridden;
            let was_enabled = e.enabled;
            self.remove_at(idx);
            e.combo = target;
            e.overridden = flags & 2 != 0;
            e.enabled = flags & 1 == 1;
            if !self.place_at_home(e) {
                self.reinsert_entry(e, old, was_overridden, was_enabled, ts);
            }
        } else {
            e.overridden = flags & 2 != 0;
            e.enabled = flags & 1 == 1;
            self.slots[idx] = Some(e);
        }
        self.push_change(Change { ts, kind: ChangeKind::Rebind, uid, old, new: target });
        true
    }

    /// 导入用户差异（启动装载面）：逐条冲突复核——目标键位已被其他
    /// 条目占用的差异跳过（先到先得同样适用于装载序）；返回生效条数。
    pub fn import_overrides(&mut self, buf: &[u8], ts: u64) -> usize {
        if buf.len() < 6 || buf[0..4] != *b"VXHK" {
            return 0;
        }
        let cnt = u16::from_le_bytes([buf[4], buf[5]]) as usize;
        if buf.len() < 6 + cnt * 9 {
            return 0;
        }
        let mut applied = 0;
        let mut p = 6;
        for _ in 0..cnt {
            let uid = u32::from_le_bytes([buf[p], buf[p + 1], buf[p + 2], buf[p + 3]]);
            let mods = buf[p + 4] & MOD_MASK;
            let key = u16::from_le_bytes([buf[p + 5], buf[p + 6]]);
            let flags = buf[p + 7];
            p += 9;
            let target = Combo::new(mods, key);
            if !target.sane() {
                continue;
            }
            if self.load_override_entry(uid, target, flags, ts) {
                applied += 1;
            }
        }
        if applied > 0 {
            self.version += 1;
        }
        applied
    }
}
