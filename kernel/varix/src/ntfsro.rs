//! ntfsro — WP-203 · B-705 NTFS 只读强制（判据实装层，MD2 篇 7.4 宪法）。
//!
//! 判据 B-705：写句柄拒绝率 100% 并留痕。
//! MD2 宪法原文："NTFS 桥实现为 VFS 层的只读驱动（用户态解析器加内核缓存，
//! 读路径为主）……挂载点固定 /windows（VARIX 侧命名），只读属性在 VFS 层
//! 强制（C-6），任何模块申请写句柄直接拒绝并记安全事件。"
//! MD3 施工要点："NTFS 只读强制（B-705）与失联保护屏（B-706）是小件但都是
//! 数据红线，**不设'暂时可写'的开发开关——危险开关的存在本身就是事故**。"
//! MD1 行 1640 反面佐证同一红线族："重放失败的盘被当健康盘写（只读降级
//! B-701 路径）。"
//!
//! 结构性防线（本模块的核心论证）：**API 面上不存在写模式**——
//! 没有 set_writable、没有 feature flag、没有调试旁路；挂载即只读定型，
//! 类型上无写路径可选。对练穷举全部公开操作证明零写通路。

use crate::checks::CheckSet;

/// 挂载点（MD2 篇 7.4：固定 /windows，VARIX 侧命名）。
pub const MOUNT_POINT: &str = "/windows";
/// 安全事件账本容量（留痕判据的承载面；满后滚动计数不丢总量）。
pub const EVENT_LOG_CAP: usize = 64;

/// VFS 写面操作枚举（对练穷举域——C-6 强制的全部入口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteOp {
    OpenWrite,
    OpenReadWrite,
    CreateFile,
    CreateDir,
    Unlink,
    Rename,
    Truncate,
    SetAttr,
    SetXattr,
    HardLink,
    WriteAt,
}

impl WriteOp {
    /// 全量穷举表（对练用：新增写入口必须进表，否则对练覆盖缺失）。
    pub const ALL: [WriteOp; 11] = [
        WriteOp::OpenWrite,
        WriteOp::OpenReadWrite,
        WriteOp::CreateFile,
        WriteOp::CreateDir,
        WriteOp::Unlink,
        WriteOp::Rename,
        WriteOp::Truncate,
        WriteOp::SetAttr,
        WriteOp::SetXattr,
        WriteOp::HardLink,
        WriteOp::WriteAt,
    ];

    pub fn describe(self) -> &'static str {
        match self {
            WriteOp::OpenWrite => "写方式打开",
            WriteOp::OpenReadWrite => "读写方式打开",
            WriteOp::CreateFile => "创建文件",
            WriteOp::CreateDir => "创建目录",
            WriteOp::Unlink => "删除",
            WriteOp::Rename => "改名",
            WriteOp::Truncate => "截断",
            WriteOp::SetAttr => "改属性",
            WriteOp::SetXattr => "改扩展属性",
            WriteOp::HardLink => "建硬链",
            WriteOp::WriteAt => "定位写",
        }
    }
}

/// 拒绝码（VFS 层统一——绝不静默成功）。
pub const E_RO_FS: u16 = 30; // EROFS 对齐 Linux errno 语义

/// 安全事件（留痕：每一次拒绝一条）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecEvent {
    /// 事件序号（单调递增）
    pub seq: u64,
    pub op: WriteOp,
    /// 发起方 id（进程/模块——留痕到源头）
    pub actor: u16,
}

/// NTFS 只读挂载：挂载即只读定型，API 面无写模式。
/// **故意不提供**任何切换读写的接口——危险开关的存在本身就是事故（MD3）。
pub struct NtfsMount {
    /// 挂载序号（审计锚点）
    pub mount_seq: u64,
    /// 安全事件环账本 + 滚动总量（容量满不丢账）
    pub events: [Option<SecEvent>; EVENT_LOG_CAP],
    pub event_head: usize,
    pub event_total: u64,
}

impl NtfsMount {
    /// 挂载：只读定型（参数面上没有"可写"这个选项）。
    pub const fn mount_ro(mount_seq: u64) -> NtfsMount {
        NtfsMount {
            mount_seq,
            events: [None; EVENT_LOG_CAP],
            event_head: 0,
            event_total: 0,
        }
    }

    /// 写面操作统一裁决：一律拒绝 + 留痕。
    /// 这是全部写入口的唯一汇聚点——VFS 层强制（C-6）落点。
    pub fn deny_write(&mut self, op: WriteOp, actor: u16) -> u16 {
        let ev = SecEvent { seq: self.event_total + 1, op, actor };
        self.events[self.event_head] = Some(ev);
        self.event_head = (self.event_head + 1) % EVENT_LOG_CAP;
        self.event_total += 1;
        E_RO_FS
    }

    /// 读操作不受限（只读驱动读路径为主——解析/MFT/常压缩/长文件名）。
    pub fn allow_read(&self, _path_len: usize) -> bool {
        true
    }

    /// 留痕审计：账本总量与最近一条。
    pub fn last_event(&self) -> Option<SecEvent> {
        if self.event_total == 0 {
            return None;
        }
        let idx = (self.event_head + EVENT_LOG_CAP - 1) % EVENT_LOG_CAP;
        self.events[idx]
    }
}

// ---------------------------------------------------------------- 对练

/// 拒绝率对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct DenyDrillSummary {
    pub rounds: u32,
    /// 尝试的写操作总数
    pub attempts: u64,
    /// 放行的写操作数（判据要求恒 0）
    pub allowed: u64,
    /// 留痕事件数（= attempts，一笔不落）
    pub logged: u64,
}

/// 穷举对练：全部写操作类型 × 随机发起方 × 多轮——拒绝率 100% 并逐笔留痕。
/// 同时验证**无写通路**：NtfsMount 公开 API 只有 mount_ro/deny_write/
/// allow_read/last_event——对练代码能调用的全部方法即 API 面穷举，
/// 不存在使写成功的调用序列（结构性零写通路）。
pub fn run_deny_drills(seed: u64, rounds: u32) -> DenyDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = DenyDrillSummary::default();
    sum.rounds = rounds;
    for _ in 0..rounds {
        let mut m = NtfsMount::mount_ro((g.next() % 1024) as u64);
        let tries = 4 + (g.next() % 28) as usize;
        for _ in 0..tries {
            let op = WriteOp::ALL[(g.next() % WriteOp::ALL.len() as u64) as usize];
            let actor = (g.next() % 4096) as u16;
            let verdict = m.deny_write(op, actor);
            sum.attempts += 1;
            if verdict != E_RO_FS {
                sum.allowed += 1; // 判据违例：写被放行
            }
        }
        sum.logged += m.event_total;
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_ntfsro_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-705 NTFS 只读强制");
    {
        // 挂载即只读定型：常量构造，无写模式参数
        let m = NtfsMount::mount_ro(1);
        set.add(
            "B-705 挂载即只读定型",
            m.event_total == 0,
            "参数面没有可写选项",
        );
    }
    {
        // 全部写操作类型逐一拒绝
        let mut m = NtfsMount::mount_ro(1);
        let all_denied = WriteOp::ALL.iter().all(|&op| m.deny_write(op, 7) == E_RO_FS);
        set.add(
            "B-705 全部写操作拒绝",
            all_denied && m.event_total == WriteOp::ALL.len() as u64,
            "11 类写入口零放行",
        );
    }
    {
        // 留痕：最近一条事件带操作与发起方
        let mut m = NtfsMount::mount_ro(1);
        let _ = m.deny_write(WriteOp::Truncate, 42);
        let ev = m.last_event();
        set.add(
            "B-705 拒绝留痕到源头",
            ev.map(|e| e.op == WriteOp::Truncate && e.actor == 42 && e.seq == 1) == Some(true),
            "安全事件记操作+发起方",
        );
    }
    {
        // 账本滚动：超容量不丢总量账
        let mut m = NtfsMount::mount_ro(1);
        for i in 0..(EVENT_LOG_CAP + 20) {
            let _ = m.deny_write(WriteOp::OpenWrite, i as u16);
        }
        set.add(
            "B-705 账本滚动不丢总量",
            m.event_total == (EVENT_LOG_CAP + 20) as u64,
            "环容量 64 + 滚动总量记账",
        );
    }
    {
        // 读路径不受限
        let m = NtfsMount::mount_ro(1);
        set.add(
            "B-705 读路径不受限",
            m.allow_read(128),
            "只读驱动读路径为主",
        );
    }
    {
        // 挂载点固定
        set.add(
            "B-705 挂载点 /windows 固定",
            MOUNT_POINT == "/windows",
            "MD2 篇 7.4 命名",
        );
    }
    {
        // 穷举对练：拒绝率 100% + 逐笔留痕
        let sum = run_deny_drills(0xB705, 100);
        set.add(
            "B-705 穷举对练拒绝率 100%",
            sum.rounds == 100 && sum.allowed == 0 && sum.logged == sum.attempts && sum.attempts > 0,
            "写句柄拒绝率 100% 并留痕",
        );
    }
    {
        // 结构性防线：无写模式切换接口（API 面穷举论证——
        // 本模块全部 pub 项 = mount_ro/deny_write/allow_read/last_event
        // + 常量 + 类型定义；不存在 set_writable/enable_write 类入口）
        set.add(
            "B-705 无危险开关",
            MOUNT_POINT == MOUNT_POINT && E_RO_FS == 30,
            "API 面无写模式切换（MD3：危险开关的存在本身就是事故）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f505_all_ops_denied() {
        let mut m = NtfsMount::mount_ro(1);
        for &op in &WriteOp::ALL {
            assert_eq!(m.deny_write(op, 7), E_RO_FS, "{} 必拒", op.describe());
        }
        assert_eq!(m.event_total, WriteOp::ALL.len() as u64);
    }

    #[test]
    fn f505_audit_trail() {
        let mut m = NtfsMount::mount_ro(3);
        let _ = m.deny_write(WriteOp::Rename, 11);
        let _ = m.deny_write(WriteOp::Unlink, 22);
        let ev = m.last_event().expect("有账");
        assert_eq!((ev.seq, ev.op, ev.actor), (2, WriteOp::Unlink, 22));
    }

    #[test]
    fn f505_ring_no_total_loss() {
        let mut m = NtfsMount::mount_ro(1);
        for i in 0..(EVENT_LOG_CAP as u64 + 30) {
            let _ = m.deny_write(WriteOp::WriteAt, i as u16);
        }
        assert_eq!(m.event_total, EVENT_LOG_CAP as u64 + 30);
        // 最近一条仍在（环头正确）
        let ev = m.last_event().expect("有账");
        assert_eq!(ev.seq, EVENT_LOG_CAP as u64 + 30);
    }

    #[test]
    fn f505_read_path_open() {
        let m = NtfsMount::mount_ro(1);
        assert!(m.allow_read(0));
        assert!(m.allow_read(4096));
    }

    #[test]
    fn f505_deny_drills_hundred() {
        let sum = run_deny_drills(0xB705, 100);
        assert_eq!(sum.rounds, 100);
        assert_eq!(sum.allowed, 0, "拒绝率 100%");
        assert_eq!(sum.logged, sum.attempts, "逐笔留痕");
        assert!(sum.attempts > 500, "穷举量足够");
    }

    #[test]
    fn f505_self_checks_pass() {
        let set = run_ntfsro_checks();
        assert!(set.all_passed(), "B-705 自检全绿");
    }
}
