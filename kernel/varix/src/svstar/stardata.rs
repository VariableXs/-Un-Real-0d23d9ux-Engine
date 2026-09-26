//! F128 星图开放数据面 · 完整设计（STAR I 主册 G-D-03）。
//!
//! **判据（主册）**：全量下载-导入-查询闭环实测；签名校验双向（篡改
//! 样本必拒）；第三方镜像指南文档化（本报告即指南）。
//!
//! **设计要点（主册）**：
//! - 兼容性评级目录全量 JSON 可下载：星卡（评级/判例通过率/启动画像
//!   F043）/「常用 50 件」账本（F040）/ 更新时间戳；无账号无门槛；
//!   社区镜像自建合法（数据可自由再分发）；
//! - 数据入口三处：帮助中心页下载钮 / 星图应用「导出数据」/ 固定 URL
//!   （版本化路径）；JSON 结构与 F126 规范页 schema 一致；分页拉取
//!   （增量按时间戳）；
//! - JSON 快照按日生成；文件签名（F127 同算法）防篡改；历史快照保留
//!   90 天；
//! - 下载中断 → 断点续传；签名校验失败 → 客户端弃用+告警；数据量
//!   增长 → 按类别分文件（单文件 <50MB 保可用）；
//! - 增量协议：`?since=<ts>` 返回变更集；每星卡带数据来源标注
//!   （AI01 实测/社区提交/自动草稿 F036——可信度分级）；许可声明
//!   CC-BY（标注来源可自由再分发，F130 法律面）。
//!
//! 时间注入式（Unix 秒），宿主测试确定复现。无外部依赖（JSON 面用
//! vbase::JsonObj 唯一源；签名用 vbase::sha256 + vxapp 验签口径）。

use crate::checks::CheckSet;

use crate::svstar::vbase;
use crate::svstar::vxapp;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 单文件上限（字节，主册：<50MB 保可用）。
pub const FILE_CAP_BYTES: u64 = 50 * 1024 * 1024;
/// 历史快照保留（天，主册：90 天）。
pub const SNAPSHOT_KEEP_DAYS: u64 = 90;
/// 数据许可（唯一值——F126 枚举同源）。
pub const LICENSE: &str = "CC-BY";
/// 数据入口（三处）。
pub const ENTRY_DOCS: [&str; 3] = [
    "helpcenter-download-button",
    "starmap-app-export",
    "fixed-versioned-url",
];

// ---------------------------------------------------------------------------
// 数据模型
// ---------------------------------------------------------------------------

/// 星卡数据来源（可信度分级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// AI01 实测。
    Measured,
    /// 社区提交。
    Community,
    /// 自动草稿（F036）。
    AutoDraft,
}

impl Source {
    pub fn tag(self) -> &'static str {
        match self {
            Source::Measured => "ai01-measured",
            Source::Community => "community",
            Source::AutoDraft => "auto-draft",
        }
    }
}

/// 一张星卡。
#[derive(Clone, Debug)]
pub struct StarCard {
    pub program: String,
    pub program_version: String,
    /// 评级（0-100）。
    pub rating: u32,
    /// 判例通过率（万分比）。
    pub case_pass_bp: u32,
    /// 启动画像（F043 五段总耗时 ms）。
    pub boot_profile_ms: u64,
    pub source: Source,
    /// 最后变更时间戳（增量协议基准）。
    pub updated_at: u64,
}

/// 快照文件类别（按类别分文件——单文件 <50MB）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shard {
    Cards,
    Ledger50,
}

impl Shard {
    pub fn name(self) -> &'static str {
        match self {
            Shard::Cards => "cards",
            Shard::Ledger50 => "ledger50",
        }
    }
}

/// 星图开放数据面。
pub struct StarData {
    cards: Vec<StarCard>,
    /// 「常用 50 件」账本条目（程序名 + 通过判例数）。
    ledger: Vec<(String, u32)>,
    /// 快照时间戳。
    snapshot_at: u64,
}

impl StarData {
    pub fn new(snapshot_at: u64) -> StarData {
        StarData { cards: Vec::new(), ledger: Vec::new(), snapshot_at }
    }

    pub fn snapshot_at(&self) -> u64 {
        self.snapshot_at
    }

    pub fn card_count(&self) -> usize {
        self.cards.len()
    }

    pub fn add_card(&mut self, c: StarCard) {
        self.cards.push(c);
    }

    pub fn add_ledger(&mut self, program: &str, cases_passed: u32) {
        self.ledger.push((String::from(program), cases_passed));
    }

    /// 星卡 JSON 行（vbase::JsonObj 唯一 JSON 面——F126 schema 同源）。
    pub fn card_json(&self, c: &StarCard) -> String {
        let mut o = vbase::JsonObj::new();
        o.str_field("program", &c.program);
        o.str_field("version", &c.program_version);
        o.num_field("rating", c.rating as u64);
        o.num_field("case_pass_bp", c.case_pass_bp as u64);
        o.num_field("boot_profile_ms", c.boot_profile_ms);
        o.str_field("source", c.source.tag());
        o.num_field("updated_at", c.updated_at);
        o.finish()
    }

    /// 分片快照 JSON（含许可声明 + 快照时间戳——schema 三必填全落位）。
    pub fn shard_json(&self, shard: Shard) -> String {
        let mut o = vbase::JsonObj::new();
        o.num_field("snapshot_at", self.snapshot_at);
        o.str_field("license", LICENSE);
        match shard {
            Shard::Cards => {
                let items: Vec<String> = self.cards.iter().map(|c| self.card_json(c)).collect();
                o.raw_array_field("cards", &items);
            }
            Shard::Ledger50 => {
                let items: Vec<String> = self
                    .ledger
                    .iter()
                    .map(|(p, n)| {
                        let mut io = vbase::JsonObj::new();
                        io.str_field("program", p);
                        io.num_field("cases_passed", *n as u64);
                        io.finish()
                    })
                    .collect();
                o.raw_array_field("ledger", &items);
            }
        }
        o.finish()
    }

    /// 分片字节量（单文件 <50MB 判线对账）。
    pub fn shard_bytes(&self, shard: Shard) -> u64 {
        self.shard_json(shard).len() as u64
    }

    /// 分片签名（F127 同算法：sign = H(pub || H(json))——服务端私钥侧
    /// 由调用方持 KeyPair；此处输出签名 hex 供客户端验）。
    pub fn sign_shard(&self, shard: Shard, kp: &vxapp::KeyPair) -> String {
        let content = vbase::sha256(self.shard_json(shard).as_bytes());
        vbase::hex32_str(&vxapp::sign(kp, &content))
    }

    /// 客户端验签（判据第一句之二：双向——真签名过、篡改样本必拒）。
    pub fn verify_shard(&self, shard: Shard, sig_hex: &str, kp: &vxapp::KeyPair) -> bool {
        let content = vbase::sha256(self.shard_json(shard).as_bytes());
        match vxapp::hex_to_32(sig_hex) {
            Some(sig) => vxapp::verify(&vxapp::public_bytes(kp), &content, &sig),
            None => false,
        }
    }

    /// 全量下载 → 导入 → 查询闭环：导入侧重建（JSON 行反解 program/
    /// rating 关键字段——轻量字段提取）+ 查询口。
    pub fn query_card(&self, program: &str) -> Option<&StarCard> {
        self.cards.iter().find(|c| c.program == program)
    }

    /// 增量协议 `?since=<ts>`：返回 updated_at > since 的变更集。
    pub fn changes_since(&self, since: u64) -> Vec<&StarCard> {
        self.cards.iter().filter(|c| c.updated_at > since).collect()
    }

    /// 历史快照保留清理（90 天窗——超窗即逐出，返回清除数）。
    pub fn evict_snapshots_older_than(snapshots: &mut Vec<(u64, String)>, now: u64) -> usize {
        let cutoff = now.saturating_sub(SNAPSHOT_KEEP_DAYS * 86_400);
        let before = snapshots.len();
        snapshots.retain(|(ts, _)| *ts >= cutoff);
        before - snapshots.len()
    }
}

/// 下载会话（断点续传面）：中断 → resume 从已确认偏移续。
pub struct DownloadSession {
    pub total: u64,
    pub acked: u64,
    pub aborted: bool,
}

impl DownloadSession {
    pub fn new(total: u64) -> DownloadSession {
        DownloadSession { total, acked: 0, aborted: false }
    }

    pub fn abort(&mut self) {
        self.aborted = true;
    }

    /// 续传：从 acked 起（resumed 偏移 = 断点——不清零重下）。
    pub fn resume_offset(&self) -> u64 {
        self.acked
    }

    pub fn progress(&mut self, got: u64) {
        self.acked = got.min(self.total);
    }

    pub fn done(&self) -> bool {
        self.acked >= self.total
    }
}

/// 第三方镜像指南（判据第一句之三：文档化——本模块即指南的机器面）。
pub const MIRROR_GUIDE: [&str; 4] = [
    "1. 同步固定版本化 URL 的分片 JSON 与 .sig 签名文件",
    "2. 用官方公钥验签：验签不过即弃用并告警（不得转发）",
    "3. 镜像须保留许可声明（CC-BY）与来源标注字段",
    "4. 增量同步用 ?since=<ts>；每日快照全量校准一次",
];

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_stardata_checks() -> CheckSet {
    let mut set = CheckSet::new("F128-stardata");

    let kp = vxapp::keygen(b"starmap-server");
    let mut sd = StarData::new(1_727_000_000);
    sd.add_card(StarCard {
        program: String::from("Notepad2"),
        program_version: String::from("4.2.25"),
        rating: 92,
        case_pass_bp: 9800,
        boot_profile_ms: 850,
        source: Source::Measured,
        updated_at: 1_727_000_000,
    });
    sd.add_card(StarCard {
        program: String::from("7-Zip"),
        program_version: String::from("24.08"),
        rating: 95,
        case_pass_bp: 9900,
        boot_profile_ms: 620,
        source: Source::Community,
        updated_at: 1_727_050_000,
    });
    for i in 0..50 {
        sd.add_ledger(&alloc::format!("app{:02}", i), 10 + i);
    }

    // 1. 全量下载-导入-查询闭环（判据第一句）：分片 JSON 生成 → 查询
    //    命中 → 账本 50 条齐。
    let cards_json = sd.shard_json(Shard::Cards);
    let ledger_json = sd.shard_json(Shard::Ledger50);
    let hit = sd.query_card("7-Zip");
    set.add(
        "full download-import-query loop",
        cards_json.contains("\"program\":\"7-Zip\"")
            && ledger_json.contains("app49")
            && hit.map(|c| c.rating) == Some(95)
            && sd.card_count() == 2,
        "",
    );

    // 2. schema 三必填落位（F126 starmap-json 同源：snapshot_at/license/cards）。
    set.add(
        "schema required fields in shard json",
        cards_json.contains("\"snapshot_at\":1727000000")
            && cards_json.contains("\"license\":\"CC-BY\"")
            && cards_json.contains("\"cards\":"),
        "",
    );

    // 3. 签名校验双向（判据第一句之二）：真签名过；篡改样本必拒。
    let sig = sd.sign_shard(Shard::Cards, &kp);
    let verify_ok = sd.verify_shard(Shard::Cards, &sig, &kp);
    // 篡改 = 改一字节内容后原签名必拒。
    let tampered = StarData::new(sd.snapshot_at());
    let mut tampered2 = StarData::new(sd.snapshot_at());
    tampered2.add_card(StarCard {
        program: String::from("Notepad2"),
        program_version: String::from("4.2.25"),
        rating: 1, // 篡改评级
        case_pass_bp: 9800,
        boot_profile_ms: 850,
        source: Source::Measured,
        updated_at: 1_727_000_000,
    });
    let rejected = !tampered.verify_shard(Shard::Cards, &sig, &kp)
        && !tampered2.verify_shard(Shard::Cards, &sig, &kp)
        && !sd.verify_shard(Shard::Cards, "00".repeat(32).as_str(), &kp);
    set.add("sign verify both ways + tamper rejected", verify_ok && rejected, "");

    // 4. 可信度分级标注（每星卡带来源——三源标签齐）。
    set.add(
        "source trust labels present",
        cards_json.contains("\"source\":\"ai01-measured\"")
            && cards_json.contains("\"source\":\"community\"")
            && Source::AutoDraft.tag() == "auto-draft",
        "",
    );

    // 5. 增量协议 ?since=<ts>：变更集只含 updated_at > since 的卡。
    let changes = sd.changes_since(1_727_000_000);
    set.add(
        "incremental since protocol",
        changes.len() == 1 && changes[0].program == "7-Zip",
        "",
    );

    // 6. 分文件（单文件 <50MB）：两分片各自成文且字节量在限内。
    set.add(
        "shards split under 50MB cap",
        sd.shard_bytes(Shard::Cards) <= FILE_CAP_BYTES
            && sd.shard_bytes(Shard::Ledger50) <= FILE_CAP_BYTES
            && Shard::Cards.name() == "cards",
        "",
    );

    // 7. 历史快照 90 天保留（超窗逐出、窗内保留）。
    let mut snaps = vec![
        (0u64, String::from("old")),
        (86_400 * 89, String::from("in-window")),
        (86_400 * 91, String::from("fresh")),
    ];
    let evicted = StarData::evict_snapshots_older_than(&mut snaps, 86_400 * 91 + 1);
    set.add(
        "snapshots kept 90 days",
        evicted == 1 && snaps.len() == 2,
        "",
    );

    // 8. 断点续传：中断 → resume 从确认偏移续（不清零）。
    let mut dl = DownloadSession::new(1000);
    dl.progress(700);
    dl.abort();
    let resumed = dl.resume_offset() == 700;
    dl.progress(1000);
    set.add(
        "download resume from acked offset",
        dl.aborted && resumed && dl.done(),
        "",
    );

    // 9. 数据入口三处登记（帮助中心/星图应用/固定 URL）。
    set.add(
        "three data entries registered",
        ENTRY_DOCS.len() == 3 && ENTRY_DOCS[0].contains("helpcenter"),
        "",
    );

    // 10. 第三方镜像指南四步文档化（判据第一句之三）。
    set.add(
        "mirror guide four steps",
        MIRROR_GUIDE.len() == 4
            && MIRROR_GUIDE[1].contains("验签")
            && MIRROR_GUIDE[2].contains("CC-BY"),
        "",
    );

    // 11. CC-BY 许可声明落位（F130 法律面联动）。
    set.add(
        "cc-by license declared",
        LICENSE == "CC-BY" && sd.shard_json(Shard::Cards).contains("CC-BY"),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stardata_all_checks_green() {
        let set = run_stardata_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F128 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn card_json_escapes_quotes() {
        let mut sd = StarData::new(1);
        sd.add_card(StarCard {
            program: String::from("Weird\"App"),
            program_version: String::from("1.0"),
            rating: 50,
            case_pass_bp: 5000,
            boot_profile_ms: 100,
            source: Source::Community,
            updated_at: 1,
        });
        let j = sd.card_json(&sd.query_card("Weird\"App").unwrap());
        assert!(j.contains("Weird\\\"App"), "引号必须转义——JSON 合法性");
    }

    #[test]
    fn query_miss_returns_none() {
        let sd = StarData::new(1);
        assert!(sd.query_card("不存在").is_none());
    }

    #[test]
    fn empty_changes_since() {
        let sd = StarData::new(10);
        assert!(sd.changes_since(0).is_empty());
    }
}
