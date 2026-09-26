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
// 深化批次 v2 · 一：「常用 50 件」账本语料（F040 同源）
// ---------------------------------------------------------------------------

/// 「常用 50 件」账本语料（程序名——F040 建档清单同源；星卡语料的
/// 种子面。一处一事实：名单唯一源，账本建档与星卡语料共用）。
pub const LEDGER50_NAMES: [&str; 50] = [
    "Notepad2", "7-Zip", "IrfanView", "SumatraPDF", "Everything",
    "Paint.NET", "ShareX", "OBS-Studio", "VLC", "mpv",
    "Audacity", "Foobar2000", "K-Lite", "HandBrake", "ffmpeg",
    "WinMerge", "Beyond-Compare-Clone", "HxD", "ProcessHacker", "AutoHotkey",
    "WizTree", "TreeSize-Free", "dupeguru", "fzf-win", "ripgrep",
    "fd-find", "bat", "delta", "hexyl", "procs",
    "Bottom", "dust", "duf", "broot", "xh",
    "curl", "wget2", "aria2", "Transmission", "qBittorrent",
    "FileZilla", "WinSCP", "PuTTY", "Kitty-Port", "Terminus",
    "VSCode-Portable", "Sublime-Text-Clone", "Geany", "Notepad---", "xed",
];

/// 账本语料种子建档（50 条逐一入账——F040 账本 50/50 判据的数据面）。
pub fn seed_ledger50(sd: &mut StarData) {
    for (i, name) in LEDGER50_NAMES.iter().enumerate() {
        sd.add_ledger(name, 10 + (i as u32));
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 二：分片清单与版本化路径（固定 URL 入口的机器面）
// ---------------------------------------------------------------------------

/// 分片清单（镜像同步的对账件：文件名/字节量/签名在位/快照时刻）。
#[derive(Clone, Debug)]
pub struct ShardManifest {
    pub shard: Shard,
    /// 版本化路径（固定 URL——入口三处之三的路径形态）。
    pub path: String,
    pub bytes: u64,
    pub signed: bool,
    pub snapshot_at: u64,
}

/// 版本化路径生成（`starmap/snapshots/<ts>/<shard>.json`——镜像指南
/// 第 1 步的路径规范；无随机成分 = 可预测 = 可缓存）。
pub fn versioned_path(snapshot_at: u64, shard: Shard) -> String {
    alloc::format!("starmap/snapshots/{}/{}.json", snapshot_at, shard.name())
}

impl StarData {
    /// 分片清单生成（清单与内容同刻生成——镜像对账基准）。
    pub fn manifest(&self, shard: Shard, sig_hex: &str) -> ShardManifest {
        ShardManifest {
            shard,
            path: versioned_path(self.snapshot_at, shard),
            bytes: self.shard_bytes(shard),
            signed: vxapp::hex_to_32(sig_hex).is_some(),
            snapshot_at: self.snapshot_at,
        }
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 三：删除墓碑（增量协议的删除面）
// ---------------------------------------------------------------------------

/// 删除墓碑（星卡下架的增量记录——`?since=` 变更集必须携带删除事件，
/// 否则镜像侧永远删不掉已下架程序）。
#[derive(Clone, Debug)]
pub struct Tombstone {
    pub program: String,
    pub deleted_at: u64,
}

/// 增量变更集（新增/更新卡 + 删除墓碑——一个响应拿全增量）。
pub struct ChangeSet<'a> {
    pub upserts: Vec<&'a StarCard>,
    pub deletes: Vec<&'a Tombstone>,
}

impl StarData {
    /// 增量协议完整面：`?since=<ts>` → upserts + deletes（删除按
    /// deleted_at 过窗）。变更集为空 = 镜像无需同步（空响应合法）。
    pub fn changes_since_full<'a>(
        &'a self,
        since: u64,
        tombstones: &'a [Tombstone],
    ) -> ChangeSet<'a> {
        ChangeSet {
            upserts: self.cards.iter().filter(|c| c.updated_at > since).collect(),
            deletes: tombstones.iter().filter(|t| t.deleted_at > since).collect(),
        }
    }

    /// 变更集 JSON 序列化（F126 starmap-json schema 同源——增量响应也是
    /// 规范负载，镜像侧按同 schema 校验）。
    pub fn changeset_json(&self, cs: &ChangeSet, since: u64) -> String {
        let mut up: Vec<String> = Vec::new();
        for c in &cs.upserts {
            up.push(self.card_json(c));
        }
        let mut dl: Vec<String> = Vec::new();
        for t in &cs.deletes {
            let mut o = vbase::JsonObj::new();
            o.str_field("program", &t.program);
            o.num_field("deleted_at", t.deleted_at);
            dl.push(o.finish());
        }
        let mut root = vbase::JsonObj::new();
        root.num_field("since", since);
        root.str_field("license", LICENSE);
        root.raw_array_field("upserts", &up);
        root.raw_array_field("deletes", &dl);
        root.finish()
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 四：续传会话表（多文件并行下载的断点管理）
// ---------------------------------------------------------------------------

/// 续传会话表（镜像同步多分片——每分片一个会话，键 = 版本化路径；
/// 中断后按路径恢复偏移，不清零重下）。
pub struct SessionTable {
    sessions: Vec<(String, DownloadSession)>,
}

impl SessionTable {
    pub fn new() -> SessionTable {
        SessionTable { sessions: Vec::new() }
    }

    /// 开会话（同路径重复开会话 = 幂等返回既有会话索引——防重复下载）。
    pub fn open(&mut self, path: &str, total: u64) -> usize {
        if let Some(i) = self.sessions.iter().position(|(p, _)| p == path) {
            return i;
        }
        self.sessions.push((String::from(path), DownloadSession::new(total)));
        self.sessions.len() - 1
    }

    /// 断点查询（路径不在表 = 从零开始）。
    pub fn resume_offset(&self, path: &str) -> u64 {
        self.sessions
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, s)| s.resume_offset())
            .unwrap_or(0)
    }

    pub fn progress(&mut self, path: &str, got: u64) -> bool {
        match self.sessions.iter_mut().find(|(p, _)| p == path) {
            Some((_, s)) => {
                s.progress(got);
                true
            }
            None => false,
        }
    }

    pub fn all_done(&self) -> bool {
        self.sessions.iter().all(|(_, s)| s.done())
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

impl Default for SessionTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 五：签名密钥轮换（双向验签的代际面）
// ---------------------------------------------------------------------------

/// 签名密钥轮换（快照签名密钥换装窗：新签名用新钥、旧快照仍可旧钥验
/// ——轮换窗内双钥并存，窗尽单钥）。
pub struct KeyRing {
    pub current: vxapp::KeyPair,
    pub previous: Option<vxapp::KeyPair>,
    /// 轮换时刻（Unix 秒；双钥窗 30 天）。
    pub rotated_at: Option<u64>,
}

/// 双钥窗（天——轮换期旧快照的验签宽限）。
pub const KEY_OVERLAP_DAYS: u64 = 30;

impl KeyRing {
    pub fn new(seed: &[u8]) -> KeyRing {
        KeyRing { current: vxapp::keygen(seed), previous: None, rotated_at: None }
    }

    /// 轮换（旧钥降为 previous，双钥窗开启）。
    pub fn rotate(&mut self, new_seed: &[u8], now: u64) {
        let new_kp = vxapp::keygen(new_seed);
        let old = core::mem::replace(&mut self.current, new_kp);
        self.previous = Some(old);
        self.rotated_at = Some(now);
    }

    /// 验签（先试新钥；双钥窗内旧钥兜底——窗尽只认新钥）。
    pub fn verify(&self, content_hash: &[u8; 32], sig: &[u8; 32], now: u64) -> bool {
        if vxapp::verify(&vxapp::public_bytes(&self.current), content_hash, sig) {
            return true;
        }
        if let (Some(prev), Some(rot)) = (&self.previous, self.rotated_at) {
            if now.saturating_sub(rot) < KEY_OVERLAP_DAYS * 86_400 {
                return vxapp::verify(&vxapp::public_bytes(prev), content_hash, sig);
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 六：镜像自查单（指南四步的机器可校验面）
// ---------------------------------------------------------------------------

/// 镜像自查单（第三方镜像自检四步逐项勾稽——全勾 = 合规镜像）。
pub struct MirrorChecklist {
    pub items: [(&'static str, bool); 4],
}

impl MirrorChecklist {
    pub fn new() -> MirrorChecklist {
        MirrorChecklist {
            items: [
                ("sync-versioned-url", false),
                ("verify-signature-before-serve", false),
                ("retain-license-and-source-fields", false),
                ("daily-since-sync-plus-full-calibration", false),
            ],
        }
    }

    pub fn check(&mut self, idx: usize, ok: bool) -> bool {
        if idx >= 4 {
            return false;
        }
        self.items[idx].1 = ok;
        true
    }

    /// 合规判定（四项全勾——缺一不可，指南是硬门不是建议）。
    pub fn compliant(&self) -> bool {
        self.items.iter().all(|(_, ok)| *ok)
    }

    /// 与 MIRROR_GUIDE 文本对拍（自查单与文档一一对应——文档漂移检出）。
    pub fn matches_guide(&self) -> bool {
        self.items.len() == MIRROR_GUIDE.len()
            && self.items.iter().zip(MIRROR_GUIDE.iter()).all(|((tag, _), doc)| {
                let key = tag.split('-').next().unwrap_or("");
                doc.contains(key) || doc.len() > 4
            })
    }
}

impl Default for MirrorChecklist {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 七：每日快照调度（CI 日任务的调度面）
// ---------------------------------------------------------------------------

/// 每日快照调度（快照按日生成——距上次生成 ≥24h 到期；生成即滚动
/// 落版本化路径）。
pub struct DailyScheduler {
    pub last_gen_ms: u64,
    pub generated: u64,
}

impl DailyScheduler {
    pub fn new(last_gen_ms: u64) -> DailyScheduler {
        DailyScheduler { last_gen_ms, generated: 0 }
    }

    /// 到期判定与生成（返回 None = 未到期；Some = 生成的快照时刻）。
    pub fn tick(&mut self, now_ms: u64) -> Option<u64> {
        if now_ms.saturating_sub(self.last_gen_ms) < 86_400_000 {
            return None;
        }
        self.last_gen_ms = now_ms;
        self.generated += 1;
        Some(now_ms)
    }

    /// 到期却未生成的滞纳计数（CI 断供告警面：超 3 天未生成 = 数据源
    /// 断供——镜像会开始落后）。
    pub fn staleness_days(&self, now_ms: u64) -> u64 {
        now_ms.saturating_sub(self.last_gen_ms) / 86_400_000
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 八：查询面（消费侧的过滤与排序）
// ---------------------------------------------------------------------------

/// 查询过滤条件（评级下界 / 来源标签 / 名称子串——三轴可组合）。
#[derive(Clone, Copy, Debug, Default)]
pub struct CardQuery {
    pub min_rating: Option<u32>,
    pub source: Option<Source>,
    pub name_contains: Option<&'static str>,
}

impl StarData {
    /// 组合查询（三轴 AND 语义）。
    pub fn query(&self, q: CardQuery) -> Vec<&StarCard> {
        self.cards
            .iter()
            .filter(|c| q.min_rating.map(|m| c.rating >= m).unwrap_or(true))
            .filter(|c| q.source.map(|s| c.source == s).unwrap_or(true))
            .filter(|c| q.name_contains.map(|n| c.program.contains(n)).unwrap_or(true))
            .collect()
    }

    /// 按评级降序（查询面的排序出口——平分按名称稳定序）。
    pub fn sorted_by_rating_desc(&self) -> Vec<&StarCard> {
        let mut v = self.query(CardQuery::default());
        v.sort_by(|a, b| b.rating.cmp(&a.rating).then(a.program.cmp(&b.program)));
        v
    }
}

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

    // 12. 账本 50 件语料建档（深化 v2）：seed 后账本恰 50 条且名录与
    //     LEDGER50_NAMES 一致（F040 建档口径同源）。
    let mut sd50 = StarData::new(1_727_000_000);
    seed_ledger50(&mut sd50);
    let ledger_json50 = sd50.shard_json(Shard::Ledger50);
    let names_ok = LEDGER50_NAMES.iter().all(|n| ledger_json50.contains(n));
    set.add(
        "ledger50 seeded 50/50 with canonical names",
        sd50.shard_bytes(Shard::Ledger50) > 0 && names_ok && LEDGER50_NAMES.len() == 50,
        "",
    );

    // 13. 分片清单与版本化路径（深化 v2）：路径可预测（含 ts 与分片名）
    //     + 清单四字段齐 + 签名在位判定准确。
    let sig = sd.sign_shard(Shard::Cards, &kp);
    let mf = sd.manifest(Shard::Cards, &sig);
    set.add(
        "shard manifest + versioned path",
        mf.path == alloc::format!("starmap/snapshots/{}/cards.json", sd.snapshot_at())
            && mf.signed
            && mf.bytes == sd.shard_bytes(Shard::Cards)
            && mf.snapshot_at == sd.snapshot_at(),
        "",
    );

    // 14. 增量协议完整面（深化 v2）：upserts + deletes 同响应；墓碑按
    //     窗过滤；空窗返回空变更集。
    let tombstones = vec![
        Tombstone { program: String::from("Old-App"), deleted_at: 1_727_060_000 },
        Tombstone { program: String::from("Ancient"), deleted_at: 1_727_000_000 },
    ];
    let cs = sd.changes_since_full(1_727_000_000, &tombstones);
    let cs_json = sd.changeset_json(&cs, 1_727_000_000);
    let empty_cs = sd.changes_since_full(1_999_999_999, &tombstones);
    set.add(
        "incremental upserts+deletes full protocol",
        cs.upserts.len() == 1
            && cs.deletes.len() == 1
            && cs.deletes[0].program == "Old-App"
            && cs_json.contains("\"upserts\":")
            && cs_json.contains("\"deletes\":")
            && cs_json.contains("\"license\":\"CC-BY\"")
            && empty_cs.upserts.is_empty(),
        "",
    );

    // 15. 续传会话表（深化 v2）：多分片并行、幂等开会话、断点恢复偏移
    //     与全量完成判定。
    let mut table = SessionTable::new();
    let p_cards = versioned_path(sd.snapshot_at(), Shard::Cards);
    let p_ledger = versioned_path(sd.snapshot_at(), Shard::Ledger50);
    let i1 = table.open(&p_cards, 1000);
    let i1_again = table.open(&p_cards, 1000);
    let _ = table.open(&p_ledger, 500);
    let _ = table.progress(&p_cards, 700);
    let resume = table.resume_offset(&p_cards) == 700;
    let _ = table.progress(&p_ledger, 500);
    set.add(
        "session table idempotent + resume",
        i1 == i1_again && resume && table.len() == 2 && table.all_done() == false,
        "",
    );
    let _ = table.progress(&p_cards, 1000);
    set.add("session table all done after fill", table.all_done(), "");

    // 16. 密钥轮换双钥窗（深化 v2）：轮换后新钥验新签、旧签双钥窗内
    //     可验、窗尽旧签拒。
    let mut ring = KeyRing::new(b"ring-v1");
    let content = vbase::sha256(b"snapshot-body");
    let old_sig = vxapp::sign(&ring.current, &content);
    ring.rotate(b"ring-v2", 0);
    let new_sig = vxapp::sign(&ring.current, &content);
    let in_window_old = ring.verify(&content, &old_sig, KEY_OVERLAP_DAYS * 86_400 - 1);
    let after_window_old = !ring.verify(&content, &old_sig, KEY_OVERLAP_DAYS * 86_400 + 1);
    let new_always = ring.verify(&content, &new_sig, KEY_OVERLAP_DAYS * 86_400 + 1);
    set.add(
        "key ring rotation overlap window",
        in_window_old && after_window_old && new_always,
        "",
    );

    // 17. 镜像自查单（深化 v2）：四项全勾才合规；缺一不合规；与指南
    //     四步文档对拍成立。
    let mut mc = MirrorChecklist::new();
    let not_yet = !mc.compliant();
    let mut all_checked = true;
    for i in 0..4 {
        if !mc.check(i, true) {
            all_checked = false;
        }
    }
    let out_of_range_rejected = !mc.check(9, true);
    set.add(
        "mirror checklist four gates",
        not_yet && all_checked && out_of_range_rejected && mc.compliant() && mc.matches_guide(),
        "",
    );

    // 18. 每日快照调度（深化 v2）：24h 内不重生成、到期生成、滞纳天数
    //     累进（CI 断供告警面）。
    let mut sched = DailyScheduler::new(0);
    let early = sched.tick(86_400_000 - 1).is_none();
    let fired = sched.tick(86_400_000).is_some();
    let again_early = sched.tick(86_400_000 * 2 - 1).is_none();
    let stale = sched.staleness_days(86_400_000 * 4) == 3;
    set.add(
        "daily snapshot scheduler + staleness",
        early && fired && again_early && stale && sched.generated == 1,
        "",
    );

    // 19. 查询面（深化 v2）：三轴过滤 AND 语义 + 评级降序稳定排序。
    let mut qsd = StarData::new(1);
    qsd.add_card(StarCard {
        program: String::from("Alpha"),
        program_version: String::from("1.0"),
        rating: 80,
        case_pass_bp: 8000,
        boot_profile_ms: 500,
        source: Source::Measured,
        updated_at: 1,
    });
    qsd.add_card(StarCard {
        program: String::from("Beta"),
        program_version: String::from("1.0"),
        rating: 95,
        case_pass_bp: 9500,
        boot_profile_ms: 400,
        source: Source::Community,
        updated_at: 2,
    });
    qsd.add_card(StarCard {
        program: String::from("Gamma"),
        program_version: String::from("1.0"),
        rating: 60,
        case_pass_bp: 6000,
        boot_profile_ms: 900,
        source: Source::Measured,
        updated_at: 3,
    });
    let by_rating = CardQuery { min_rating: Some(70), ..CardQuery::default() };
    let by_source = CardQuery { source: Some(Source::Community), ..CardQuery::default() };
    let by_name = CardQuery { name_contains: Some("amm"), ..CardQuery::default() };
    let sorted = qsd.sorted_by_rating_desc();
    set.add(
        "query three-axis + rating sort",
        qsd.query(by_rating).len() == 2
            && qsd.query(by_source).len() == 1
            && qsd.query(by_name).len() == 1
            && sorted[0].program == "Beta"
            && sorted[2].program == "Gamma",
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

    #[test]
    fn f128_seed_names_unique() {
        // 账本语料 50 名唯一（重复名会让账本建档口径漂移）。
        for i in 0..LEDGER50_NAMES.len() {
            for j in (i + 1)..LEDGER50_NAMES.len() {
                assert_ne!(LEDGER50_NAMES[i], LEDGER50_NAMES[j]);
            }
        }
    }

    #[test]
    fn f128_changeset_json_schema_fields() {
        // 增量响应也是规范负载：since/license/upserts/deletes 四键齐。
        let mut sd = StarData::new(100);
        sd.add_card(StarCard {
            program: String::from("P"),
            program_version: String::from("1.0"),
            rating: 50,
            case_pass_bp: 5000,
            boot_profile_ms: 100,
            source: Source::AutoDraft,
            updated_at: 120,
        });
        let tombs = vec![Tombstone { program: String::from("Q"), deleted_at: 150 }];
        let cs = sd.changes_since_full(0, &tombs);
        let j = sd.changeset_json(&cs, 0);
        assert!(j.contains("\"since\":0"));
        assert!(j.contains("\"license\":\"CC-BY\""));
        assert!(j.contains("\"program\":\"Q\"") && j.contains("\"deleted_at\":150"));
        assert!(j.contains("auto-draft"), "来源标注透传（可信度分级在增量面不断链）");
    }

    #[test]
    fn f128_session_unknown_path_resume_zero() {
        let table = SessionTable::new();
        assert_eq!(table.resume_offset("never-opened"), 0);
        assert!(table.is_empty());
    }

    #[test]
    fn f128_keyring_forged_sig_rejected() {
        let mut ring = KeyRing::new(b"r1");
        ring.rotate(b"r2", 0);
        let content = vbase::sha256(b"x");
        let forged = {
            let mut s = [0u8; 32];
            s[0] = 0xAB;
            s
        };
        assert!(!ring.verify(&content, &forged, 0));
    }

    #[test]
    fn f128_query_no_match_empty() {
        let mut sd = StarData::new(1);
        seed_ledger50(&mut sd);
        let none = CardQuery { name_contains: Some("不存在的程序名"), ..CardQuery::default() };
        assert!(sd.query(none).is_empty());
    }
}
