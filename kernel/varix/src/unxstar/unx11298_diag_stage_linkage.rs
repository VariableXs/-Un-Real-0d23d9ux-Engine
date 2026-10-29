//! UNX-F11298 诊断面与装载总图联动账（0x1f6d0 段）
//!
//! 锚点：UNX-F11298-J1 诊断行按装载总图（UNX-F11259）的十二阶段对账，
//! 缺段为零、孤段为零，覆盖率 1000‰。
//!
//! 纯功能模型：装载总图把一次装载切成十二个有序阶段（内核进入、auxv 扫描、
//! 自重定位、缓存、依赖装载、符号查找、重定位、TLS、init、审计、入口跳转、
//! 退出）。诊断面每写一行都必须带阶段号，且阶段号在总图中存在、在时间上
//! 不回退。对账器把两张账叠在一起：总图有而诊断无 = 缺段；诊断有而总图无 =
//! 孤段；两者皆无才允许签发联动指纹。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f6d0;
/// 诊断行引用了总图中不存在的阶段
pub const RK_ESTAGE: u32 = SEG_BASE + 0x01;
/// 阶段号回退
pub const RK_EORDER: u32 = SEG_BASE + 0x02;
/// 冻结后登记/写入
pub const RK_EFROZEN: u32 = SEG_BASE + 0x03;
/// 空行或空阶段名
pub const RK_ELINE: u32 = SEG_BASE + 0x04;
/// 对账发现缺段
pub const RK_EMISS: u32 = SEG_BASE + 0x05;
/// 诊断面为空
pub const RK_EMPTY: u32 = SEG_BASE + 0x06;
/// 阶段 id 重复
pub const RK_EDUP: u32 = SEG_BASE + 0x07;
/// 阶段 id 非连续（期望 len+1）
pub const RK_EID: u32 = SEG_BASE + 0x08;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut x = h;
    let mut k = 0usize;
    let bytes = s.as_bytes();
    while k < bytes.len() {
        if let Some(b) = bytes.get(k) {
            x = fold_byte(x, *b);
        }
        k = k.wrapping_add(1);
    }
    x
}

fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut s = 0u32;
    while s < 32 {
        x = fold_byte(x, ((v >> s) & 0xff) as u8);
        s = s.wrapping_add(8);
    }
    x
}

// ---------------------------------------------------------------- 装载总图
/// 总图中的一个阶段：序号 + 名称 + 相位标签。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageSpec {
    pub id: u32,
    pub name: &'static str,
    pub phase: &'static str,
}

/// 装载总图：阶段必须从 1 起连续登记。
#[derive(Clone, Debug)]
pub struct TotalMap {
    stages: Vec<StageSpec>,
    seal: Option<u64>,
}

impl TotalMap {
    pub fn empty() -> Self {
        TotalMap {
            stages: Vec::new(),
            seal: None,
        }
    }

    pub fn len(&self) -> usize {
        self.stages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub fn is_frozen(&self) -> bool {
        self.seal.is_some()
    }

    pub fn stages(&self) -> &[StageSpec] {
        self.stages.as_slice()
    }

    pub fn find(&self, id: u32) -> Option<&StageSpec> {
        let mut k = 0usize;
        while k < self.stages.len() {
            if let Some(st) = self.stages.get(k) {
                if st.id == id {
                    return Some(st);
                }
            }
            k = k.wrapping_add(1);
        }
        None
    }

    /// 登记阶段：id 必须恰为 len+1；重复 id 与跳号分报两种错。
    pub fn add_stage(&mut self, stage: StageSpec) -> Result<(), u32> {
        if self.seal.is_some() {
            return Err(RK_EFROZEN);
        }
        if stage.name.is_empty() || stage.phase.is_empty() {
            return Err(RK_ELINE);
        }
        if self.find(stage.id).is_some() {
            return Err(RK_EDUP);
        }
        let want = (self.stages.len() as u32).wrapping_add(1);
        if stage.id != want {
            return Err(RK_EID);
        }
        self.stages.push(stage);
        Ok(())
    }

    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut k = 0usize;
        while k < self.stages.len() {
            if let Some(st) = self.stages.get(k) {
                h = fold_u32(h, st.id);
                h = fold_str(h, st.name);
                h = fold_str(h, st.phase);
            }
            k = k.wrapping_add(1);
        }
        h
    }

    pub fn freeze(&mut self) -> Option<u64> {
        if let Some(v) = self.seal {
            return Some(v);
        }
        let v = self.fold();
        self.seal = Some(v);
        Some(v)
    }
}

// ---------------------------------------------------------------- 诊断账
/// 一行诊断：阶段号 + 正文。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagLine {
    pub stage: u32,
    pub text: String,
}

/// 联动对账器：总图 + 诊断行 + 对账结论。
#[derive(Clone, Debug)]
pub struct Reconciler {
    map: TotalMap,
    lines: Vec<DiagLine>,
    seal: Option<u64>,
    pub emitted: u32,
    pub ingested: u32,
    pub rejects: u32,
}

impl Reconciler {
    pub fn new(map: TotalMap) -> Self {
        Reconciler {
            map,
            lines: Vec::new(),
            seal: None,
            emitted: 0,
            ingested: 0,
            rejects: 0,
        }
    }

    pub fn map(&self) -> &TotalMap {
        &self.map
    }

    pub fn lines(&self) -> &[DiagLine] {
        self.lines.as_slice()
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn is_frozen(&self) -> bool {
        self.seal.is_some()
    }

    /// 写一行诊断：阶段必须在图、正文非空、阶段号不回退。
    pub fn emit(&mut self, stage: u32, text: &str) -> Result<usize, u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(RK_EFROZEN);
        }
        if text.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(RK_ELINE);
        }
        if self.map.find(stage).is_none() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(RK_ESTAGE);
        }
        if let Some(last) = self.lines.last() {
            if stage < last.stage {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(RK_EORDER);
            }
        }
        self.lines.push(DiagLine {
            stage,
            text: text.to_string(),
        });
        self.emitted = self.emitted.wrapping_add(1);
        Ok(self.lines.len())
    }

    /// 整段导入外部诊断：先全量校验再落账，避免半截写入。
    pub fn ingest(&mut self, dump: &[(u32, &str)]) -> Result<u32, u32> {
        if self.seal.is_some() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(RK_EFROZEN);
        }
        if dump.is_empty() {
            self.rejects = self.rejects.wrapping_add(1);
            return Err(RK_ELINE);
        }
        let mut last = match self.lines.last() {
            Some(l) => l.stage,
            None => 0,
        };
        let mut k = 0usize;
        while k < dump.len() {
            let (stage, text) = match dump.get(k) {
                Some(v) => *v,
                None => break,
            };
            if text.is_empty() {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(RK_ELINE);
            }
            if self.map.find(stage).is_none() {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(RK_ESTAGE);
            }
            if stage < last {
                self.rejects = self.rejects.wrapping_add(1);
                return Err(RK_EORDER);
            }
            last = stage;
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < dump.len() {
            if let Some((stage, text)) = dump.get(j) {
                self.lines.push(DiagLine {
                    stage: *stage,
                    text: (*text).to_string(),
                });
            }
            j = j.wrapping_add(1);
        }
        self.ingested = self.ingested.wrapping_add(dump.len() as u32);
        Ok(dump.len() as u32)
    }

    /// 缺段：总图中没有任何诊断行的阶段号。
    pub fn missing(&self) -> Vec<u32> {
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < self.map.len() {
            if let Some(st) = self.map.stages().get(k) {
                let mut seen = false;
                let mut j = 0usize;
                while j < self.lines.len() {
                    if let Some(line) = self.lines.get(j) {
                        if line.stage == st.id {
                            seen = true;
                        }
                    }
                    j = j.wrapping_add(1);
                }
                if !seen {
                    out.push(st.id);
                }
            }
            k = k.wrapping_add(1);
        }
        out
    }

    /// 每阶段诊断行数（按总图顺序）。
    pub fn stage_counts(&self) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < self.map.len() {
            if let Some(st) = self.map.stages().get(k) {
                let mut n = 0u32;
                let mut j = 0usize;
                while j < self.lines.len() {
                    if let Some(line) = self.lines.get(j) {
                        if line.stage == st.id {
                            n = n.wrapping_add(1);
                        }
                    }
                    j = j.wrapping_add(1);
                }
                out.push((st.id, n));
            }
            k = k.wrapping_add(1);
        }
        out
    }

    /// 时间序：阶段号沿诊断账单调不减。
    pub fn ordered(&self) -> bool {
        let mut k = 1usize;
        while k < self.lines.len() {
            let prev = match self.lines.get(k.wrapping_sub(1)) {
                Some(l) => l.stage,
                None => return false,
            };
            let cur = match self.lines.get(k) {
                Some(l) => l.stage,
                None => return false,
            };
            if cur < prev {
                return false;
            }
            k = k.wrapping_add(1);
        }
        true
    }

    /// 覆盖率（千分比）：有诊断行的阶段占比。
    pub fn coverage_permille(&self) -> u32 {
        if self.map.is_empty() {
            return 0;
        }
        let total = self.map.len() as u32;
        let miss = self.missing().len() as u32;
        total.saturating_sub(miss).saturating_mul(1000) / total
    }

    /// 对账：空账、乱序、缺段三关过后签发联动指纹。
    pub fn reconcile(&self) -> Result<u64, u32> {
        if self.lines.is_empty() {
            return Err(RK_EMPTY);
        }
        if !self.ordered() {
            return Err(RK_EORDER);
        }
        if !self.missing().is_empty() {
            return Err(RK_EMISS);
        }
        Ok(self.fold())
    }

    pub fn fold(&self) -> u64 {
        let mut h = self.map.fold();
        let mut k = 0usize;
        while k < self.lines.len() {
            if let Some(line) = self.lines.get(k) {
                h = fold_u32(h, line.stage);
                h = fold_str(h, &line.text);
                h = fold_byte(h, b'\n');
            }
            k = k.wrapping_add(1);
        }
        h = fold_u32(h, self.emitted);
        h = fold_u32(h, self.ingested);
        fold_u32(h, self.rejects)
    }

    /// 幂等冻结：总图与诊断账一并定格。
    pub fn freeze(&mut self) -> Option<u64> {
        if let Some(v) = self.seal {
            return Some(v);
        }
        let _ = self.map.freeze();
        let v = self.fold();
        self.seal = Some(v);
        Some(v)
    }
}

// ---------------------------------------------------------------- 标准总图
static STANDARD_STAGES: [(&str, &str); 12] = [
    ("kernel-exec", "preload"),
    ("auxv-scan", "preload"),
    ("self-reloc", "preload"),
    ("cache-open", "preload"),
    ("dep-load", "link"),
    ("symbol-lookup", "link"),
    ("reloc-apply", "link"),
    ("tls-init", "link"),
    ("init-array", "init"),
    ("audit-hook", "init"),
    ("entry-jump", "run"),
    ("exit-fini", "fini"),
];

static STANDARD_LINES: [(u32, &str); 24] = [
    (1, "kernel: execve entry, argc=3"),
    (1, "kernel: stack at 0x7ffd3e4f6000"),
    (2, "auxv: 30 words scanned"),
    (2, "auxv: AT_PAGESZ=4096 AT_CLKTCK=100"),
    (3, "self-reloc: ld.so base mapped"),
    (3, "self-reloc: 412 relative relocs applied"),
    (4, "cache: /etc/ld.so.cache opened"),
    (4, "cache: 2 hits for probe"),
    (5, "dep-load: libc.so.6 mapped"),
    (5, "dep-load: depth=2 breadth=5"),
    (6, "symbol: lookup memcpy in libc"),
    (6, "symbol: version GLIBC_2.14 bound"),
    (7, "reloc: 1188 relocations applied"),
    (7, "reloc: 0 unresolved"),
    (8, "tls: static block 144 bytes"),
    (8, "tls: module 2 assigned offset 0"),
    (9, "init: DT_INIT_ARRAY 4 entries"),
    (9, "init: probe constructor done"),
    (10, "audit: no LD_AUDIT configured"),
    (10, "audit: hooks skipped"),
    (11, "entry: jump to 0x55f0d2a4c1a0"),
    (11, "entry: main returned 0"),
    (12, "fini: DT_FINI_ARRAY 2 entries"),
    (12, "fini: exit_group(0)"),
];

/// 十二阶段按 1..12 连续登记。
pub fn standard_map() -> TotalMap {
    let mut map = TotalMap::empty();
    let mut k = 0usize;
    while k < STANDARD_STAGES.len() {
        if let Some((name, phase)) = STANDARD_STAGES.get(k) {
            let id = (k as u32).wrapping_add(1);
            let _ = map.add_stage(StageSpec { id, name, phase });
        }
        k = k.wrapping_add(1);
    }
    map
}

/// 基准联动账：前十二行走 emit，后十二行走 ingest。
pub fn build_linkage() -> Reconciler {
    let mut r = Reconciler::new(standard_map());
    let mut k = 0usize;
    while k < 12 {
        if let Some((stage, text)) = STANDARD_LINES.get(k) {
            let _ = r.emit(*stage, text);
        }
        k = k.wrapping_add(1);
    }
    if let Some(rest) = STANDARD_LINES.get(12..) {
        let _ = r.ingest(rest);
    }
    r
}

/// 四路对抗：孤段、乱序、空行、缺段对账。
pub fn inject_four() -> [bool; 4] {
    let mut hits = [false; 4];

    let mut a = build_linkage();
    hits[0] = a.emit(13, "orphan stage") == Err(RK_ESTAGE);

    let mut b = Reconciler::new(standard_map());
    let _ = b.emit(2, "second stage first");
    hits[1] = b.emit(1, "back to first") == Err(RK_EORDER);

    let mut c = Reconciler::new(standard_map());
    hits[2] = c.emit(1, "") == Err(RK_ELINE);

    let mut d = Reconciler::new(standard_map());
    let mut k = 0usize;
    while k < STANDARD_LINES.len().saturating_sub(2) {
        if let Some((stage, text)) = STANDARD_LINES.get(k) {
            let _ = d.emit(*stage, text);
        }
        k = k.wrapping_add(1);
    }
    hits[3] = d.reconcile() == Err(RK_EMISS) && d.missing() == [12u32];

    hits
}

// ---------------------------------------------------------------- 判据
pub fn run_unx11298_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11298");
    let mut base = build_linkage();

    // J1 十二阶段连续
    let first = base.map().stages().first();
    let last = base.map().stages().last();
    s.add(
        "J1-十二阶段连续",
        base.map().len() == 12
            && first.map(|x| x.id) == Some(1)
            && last.map(|x| x.id) == Some(12)
            && last.map(|x| x.name) == Some("exit-fini")
            && last.map(|x| x.phase) == Some("fini"),
        "阶段 1..12 连续，末段为退出",
    );

    // J2 诊断面双路落账
    s.add(
        "J2-诊断面落账",
        base.line_count() == 24 && base.emitted == 12 && base.ingested == 12,
        "24 行：12 行直写 + 12 行整段导入",
    );

    // J3 时序与覆盖
    let counts = base.stage_counts();
    let all_two = counts.iter().all(|(_, n)| *n == 2);
    s.add(
        "J3-时序与覆盖",
        base.ordered() && base.coverage_permille() == 1000 && all_two && counts.len() == 12,
        "阶段号单调不减，每阶段两行",
    );

    // J4 对账签发
    let once = base.reconcile();
    let twice = base.reconcile();
    let digest_ok = match once {
        Ok(v) => v == base.fold() && v != 0 && twice == Ok(v),
        Err(_) => false,
    };
    s.add(
        "J4-对账签发",
        digest_ok,
        "对账通过，两次取回同一联动指纹",
    );

    // J5 缺段为零
    s.add(
        "J5-缺段为零",
        base.missing().is_empty(),
        "十二阶段全部有诊断行",
    );

    // J6 缺段可定位
    let mut dropped = Reconciler::new(standard_map());
    let mut k = 0usize;
    while k < STANDARD_LINES.len() {
        if let Some((stage, text)) = STANDARD_LINES.get(k) {
            if *stage != 11 {
                let _ = dropped.emit(*stage, text);
            }
        }
        k = k.wrapping_add(1);
    }
    s.add(
        "J6-缺段可定位",
        dropped.missing() == [11u32]
            && dropped.reconcile() == Err(RK_EMISS)
            && dropped.coverage_permille() == 916,
        "剔掉第 11 段两行后缺段定位为 [11]",
    );

    // J7 孤段拒
    let mut a = build_linkage();
    let before = a.line_count();
    s.add(
        "J7-孤段拒",
        a.emit(13, "stage thirteen") == Err(RK_ESTAGE) && a.line_count() == before,
        "总图无第 13 段，诊断行拒收",
    );

    // J8 乱序拒
    let mut b = Reconciler::new(standard_map());
    let _ = b.emit(4, "cache first");
    s.add(
        "J8-乱序拒",
        b.emit(3, "self-reloc late") == Err(RK_EORDER) && b.line_count() == 1,
        "阶段号回退即拒，首行保留",
    );

    // J9 空行拒
    let mut c = Reconciler::new(standard_map());
    s.add(
        "J9-空行拒",
        c.emit(1, "") == Err(RK_ELINE) && c.line_count() == 0,
        "空正文不入诊断账",
    );

    // J10 重段与跳号
    let mut dup = standard_map();
    let dup_r = dup.add_stage(StageSpec {
        id: 1,
        name: "kernel-exec",
        phase: "preload",
    });
    let mut gap = TotalMap::empty();
    let gap_r = gap.add_stage(StageSpec {
        id: 2,
        name: "auxv-scan",
        phase: "preload",
    });
    s.add(
        "J10-重段与跳号",
        dup_r == Err(RK_EDUP)
            && dup.len() == 12
            && gap_r == Err(RK_EID)
            && gap.len() == 0,
        "重复阶段报 EDUP，首段跳号报 EID",
    );

    // J11 空诊断面对账
    let empty = Reconciler::new(standard_map());
    s.add(
        "J11-空诊断面",
        empty.reconcile() == Err(RK_EMPTY)
            && empty.missing().len() == 12
            && empty.coverage_permille() == 0,
        "无诊断行时对账拒，缺段为全图",
    );

    // J12 阶段计数总账
    let counts = base.stage_counts();
    let total = counts.iter().fold(0u32, |a, (_, n)| a.wrapping_add(*n));
    s.add(
        "J12-阶段计数总账",
        total == 24 && counts.len() == 12,
        "十二阶段计数求和等于诊断行数",
    );

    // J13 总图联动定位
    let sev = base.map().find(7).map(|x| x.name);
    let ten = base.map().find(10).map(|x| x.phase);
    let stable = base.map().fold() == base.map().fold();
    s.add(
        "J13-总图联动定位",
        sev == Some("reloc-apply") && ten == Some("init") && stable,
        "第 7 段重定位、第 10 段审计相位可定位",
    );

    // J14 冻结拒变更
    let seal = base.freeze();
    let before = base.rejects;
    let late_emit = base.emit(12, "late");
    let late_ingest = base.ingest(&[(12u32, "late")]);
    s.add(
        "J14-冻结拒变更",
        seal.is_some()
            && late_emit == Err(RK_EFROZEN)
            && late_ingest == Err(RK_EFROZEN)
            && base.rejects == before.wrapping_add(2)
            && base.line_count() == 24,
        "冻结后直写与导入均拒，账长不变",
    );

    // J15 冻结幂等
    let again = base.freeze();
    s.add(
        "J15-冻结幂等",
        again.is_some() && again == seal && base.is_frozen(),
        "重复冻结取回同一指纹",
    );

    // J16 注入四拒
    let hits = inject_four();
    s.add(
        "J16-注入四拒",
        hits.iter().all(|h| *h),
        "孤段/乱序/空行/缺段四路全拒",
    );

    // J17 联动明细
    s.add_dyn(
        "J17-联动明细",
        base.line_count() == 24 && base.coverage_permille() == 1000,
        &format!(
            "stages={} lines={} emit={} ingest={} miss={} cov={}‰ rej={}",
            base.map().len(),
            base.line_count(),
            base.emitted,
            base.ingested,
            base.missing().len(),
            base.coverage_permille(),
            base.rejects
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11298_linkage_flow() {
        let mut base = build_linkage();
        assert_eq!(base.map().len(), 12);
        assert_eq!(base.line_count(), 24);
        assert_eq!(base.emitted, 12);
        assert_eq!(base.ingested, 12);
        assert_eq!(base.coverage_permille(), 1000);
        assert!(base.missing().is_empty());
        assert!(base.ordered());

        let signed = base.reconcile();
        assert!(signed.is_ok());
        assert_eq!(signed, base.reconcile());
        assert!(signed == Ok(base.fold()));

        let seal = base.freeze();
        assert!(seal.is_some());
        assert_eq!(base.freeze(), seal);
        assert_eq!(base.emit(12, "late").err(), Some(RK_EFROZEN));

        let mut dup = standard_map();
        let d = dup.add_stage(StageSpec {
            id: 1,
            name: "dup",
            phase: "preload",
        });
        assert_eq!(d.err(), Some(RK_EDUP));

        let mut gap = TotalMap::empty();
        let g = gap.add_stage(StageSpec {
            id: 3,
            name: "skip",
            phase: "preload",
        });
        assert_eq!(g.err(), Some(RK_EID));

        let mut r = Reconciler::new(standard_map());
        assert_eq!(r.emit(13, "x").err(), Some(RK_ESTAGE));
        assert_eq!(r.emit(2, "x").err(), None);
        assert_eq!(r.emit(1, "x").err(), Some(RK_EORDER));
        assert_eq!(r.emit(2, "").err(), Some(RK_ELINE));

        assert!(inject_four().iter().all(|h| *h));
    }

    #[test]
    fn unx11298_checks_green() {
        let s = run_unx11298_checks();
        assert!(s.all_passed(), "UNX-F11298 判据未全绿");
    }
}
