//! H2 域旋钮清单 · 完整设计（通用十二查 #9「调优走旋钮清单」全域落位）。
//!
//! **纪律**：主册反复出现的「参数进旋钮清单」在 H2 域的单一落位——
//! 五十项的每一处可调参数在此登记：默认值、合法域（min/max）、单位、
//! **主册依据摘文**。代码里不许再出现魔法数；调优改这里，一处改处处改。
//! 变更走 [`crate::star::sbase::KnobReg`] 的既有留痕管道（钳制入档、
//! 变更日志），本模块提供域级登记表与校验/导出。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 一个旋钮的登记项。
#[derive(Clone, Copy, Debug)]
pub struct KnobDef {
    /// 旋钮 id（域内唯一，`h2.` 前缀）。
    pub id: &'static str,
    /// 所属功能项（F 编号）。
    pub item: &'static str,
    /// 人话名（设置中心直显）。
    pub name: &'static str,
    /// 默认值（主册定值或主册推导）。
    pub default: i64,
    /// 合法域下限。
    pub min: i64,
    /// 合法域上限。
    pub max: i64,
    /// 单位标注。
    pub unit: &'static str,
    /// 主册依据（判据摘文关键句——一处一事实）。
    pub basis: &'static str,
}

impl KnobDef {
    /// 值合法性（域内钳制由 KnobReg 执行，此处供校验/导入自检）。
    pub fn in_domain(&self, v: i64) -> bool {
        v >= self.min && v <= self.max
    }
}

/// H2 域旋钮登记表（唯一源——每项参数的主册依据随手可查）。
pub fn defs() -> Vec<KnobDef> {
    alloc::vec![
        // --- F251 媒体会话仲裁 ---
        KnobDef { id: "h2.f251.key_limit_ms", item: "F251", name: "媒体键响应判线", default: 50, min: 30, max: 200, unit: "ms", basis: "键响应 <50ms（F064 链）" },
        KnobDef { id: "h2.f251.osd_ring", item: "F251", name: "OSD 快照环容量", default: 32, min: 8, max: 128, unit: "条", basis: "OSD 显示应用归属（最近历史可查）" },
        // --- F252 任务栏按钮合并与分组 ---
        KnobDef { id: "h2.f252.bar_budget", item: "F252", name: "按钮位预算（占满即合并）", default: 12, min: 6, max: 32, unit: "个", basis: "占满时合并三档策略" },
        // --- F253 快速访问固定 ---
        KnobDef { id: "h2.f253.recent_folders", item: "F253", name: "自动推荐文件夹数", default: 5, min: 0, max: 10, unit: "个", basis: "最近使用的 5 个文件夹" },
        KnobDef { id: "h2.f253.recent_files", item: "F253", name: "自动推荐文件数", default: 5, min: 0, max: 10, unit: "个", basis: "最近打开的 5 个文件" },
        // --- F254 窗口透明材质规范 ---
        KnobDef { id: "h2.f254.titlebar_alpha", item: "F254", name: "标题栏最大透明度", default: 216, min: 192, max: 255, unit: "/255", basis: "标题栏（低透明度）" },
        KnobDef { id: "h2.f254.sidebar_alpha", item: "F254", name: "侧栏浮层最大透明度", default: 200, min: 160, max: 255, unit: "/255", basis: "侧栏与浮层背景衬壁纸取样模糊" },
        KnobDef { id: "h2.f254.cpu_budget", item: "F254", name: "材质 CPU 增量预算", default: 3, min: 1, max: 10, unit: "%", basis: "模糊采样走 GPU（CPU 占用增量 <3%）" },
        // --- F255 文本拖放 ---
        KnobDef { id: "h2.f255.snippet_stem", item: "F255", name: "片段文件名截取字符", default: 12, min: 4, max: 32, unit: "字", basis: "生成 .txt 片段文件（首行截取）" },
        // --- F256 横向滚动语义 ---
        KnobDef { id: "h2.f256.inertia_decay", item: "F256", name: "惯性衰减系数（‰）", default: 920, min: 800, max: 990, unit: "‰/帧", basis: "横向惯性曲线与纵向同谱（F124）" },
        // --- F257 打开方式选择器 ---
        // （排序规则为策略非参数——无旋钮，纪律：策略不许翻供）
        // --- F258 桌面右键菜单全集 ---
        // （六项顺序为对照表非参数——无旋钮）
        // --- F259 新建菜单与命名初态 ---
        KnobDef { id: "h2.f259.grid_cols", item: "F259", name: "新建落点网格列数", default: 6, min: 1, max: 16, unit: "列", basis: "落点=当前视图第一个可用网格位（F084）" },
        // --- F260 行内重命名 ---
        KnobDef { id: "h2.f260.shake_ms", item: "F260", name: "非法字符抖动时长", default: 300, min: 150, max: 600, unit: "ms", basis: "非法字符即时抖动拒绝不等到提交" },
        // --- F261 删除与 Shift+Delete ---
        KnobDef { id: "h2.f261.trash_cap_mb", item: "F261", name: "回收站容量", default: 100, min: 10, max: 4096, unit: "MB", basis: "大于回收站容量的删除直接走永久流程" },
        // --- F262 拖拽复制/移动语义 ---
        // （六组合语义为潜规则写死——无旋钮，主册：潜规则不许翻供）
        // --- F263 「发送到」菜单 ---
        KnobDef { id: "h2.f263.copy_guard", item: "F263", name: "副本递增步数上限", default: 1000, min: 100, max: 10000, unit: "步", basis: "发送到桌面自动防重名" },
        // --- F264 属性对话框 ---
        KnobDef { id: "h2.f264.stat_batch", item: "F264", name: "文件夹统计分批大小", default: 20, min: 5, max: 200, unit: "项/片", basis: "异步计算带进度（空闲切片驱动）" },
        // --- F265 地址栏可编辑与补全 ---
        KnobDef { id: "h2.f265.kids_cap", item: "F265", name: "下一级候选列表上限", default: 32, min: 8, max: 128, unit: "条", basis: "反斜杠后弹下一级目录候选列表" },
        // --- F266 后退/前进与 Alt+方向键 ---
        KnobDef { id: "h2.f266.stack_cap", item: "F266", name: "导航栈深上限", default: 100, min: 20, max: 500, unit: "步", basis: "栈深上限 100 淘汰最旧" },
        // --- F267 存储感知自动清理 ---
        KnobDef { id: "h2.f267.trash_days", item: "F267", name: "回收站超期天数", default: 30, min: 7, max: 90, unit: "天", basis: "回收站超 30 天自动清（可关）" },
        KnobDef { id: "h2.f267.temp_days", item: "F267", name: "临时目录超期天数", default: 7, min: 1, max: 30, unit: "天", basis: "临时目录超 7 天清" },
        // --- F268 磁盘空间预警 ---
        KnobDef { id: "h2.f268.warn_pct", item: "F268", name: "黄色横幅阈值", default: 10, min: 5, max: 25, unit: "%", basis: "<10% 黄色横幅" },
        KnobDef { id: "h2.f268.crit_pct", item: "F268", name: "红色推送阈值", default: 5, min: 2, max: 10, unit: "%", basis: "<5% 红色提示" },
        KnobDef { id: "h2.f268.protect_pct", item: "F268", name: "保护态阈值", default: 2, min: 1, max: 5, unit: "%", basis: "<2% 保护态（非核心写入劝退）" },
        // --- F269 长复制暂停与恢复 ---
        KnobDef { id: "h2.f269.chunk_mb", item: "F269", name: "复制块大小", default: 4, min: 1, max: 16, unit: "MB", basis: "当前 4MB 块写完即停（<1s）" },
        // --- F270 文件操作错误重试 ---
        // （三选一为交互策略——无旋钮）
        // --- F271 资源管理器多标签页 ---
        KnobDef { id: "h2.f271.tab_width", item: "F271", name: "标签宽度预算", default: 120, min: 60, max: 240, unit: "px", basis: "标签栏空间不足时收缩为图标" },
        // --- F272 文本框右键菜单 ---
        // （六项清单为审计基线——无旋钮）
        // --- F273 文档上次位置记忆 ---
        KnobDef { id: "h2.f273.sel_hint_s", item: "F273", name: "选区淡显提示时长", default: 2, min: 1, max: 5, unit: "s", basis: "上次选区若存在淡显 2 秒提示" },
        // --- F274 「所有应用」列表 ---
        // （混排规则为策略——无旋钮）
        // --- F275 开始菜单电源菜单 ---
        // （三+一项清单为唯一源——无旋钮）
        // --- F276 贴靠布局组 ---
        KnobDef { id: "h2.f276.gap_px", item: "F276", name: "贴靠呼吸缝", default: 8, min: 4, max: 16, unit: "px", basis: "区与区之间留 8px 呼吸缝" },
        KnobDef { id: "h2.f276.mem_cap", item: "F276", name: "布局记忆条数", default: 3, min: 1, max: 6, unit: "款", basis: "布局组记住最近用过的三款置顶显示" },
        // --- F277 多显示器任务栏策略 ---
        KnobDef { id: "h2.f277.focus_ms", item: "F277", name: "焦点屏判定时限", default: 200, min: 100, max: 500, unit: "ms", basis: "焦点屏判定（焦点切到副屏 200ms 内）" },
        // --- F278 投影/显示模式切换 ---
        // （四模式+指纹为策略——无旋钮）
        // --- F279 触控板手势集 ---
        KnobDef { id: "h2.f279.intent_px", item: "F279", name: "手势意图阈值", default: 24, min: 12, max: 48, unit: "px", basis: "划过不误触（区分滑动意图的阈值明写）" },
        KnobDef { id: "h2.f279.latency_ms", item: "F279", name: "手势识别延迟判线", default: 80, min: 40, max: 150, unit: "ms", basis: "手势识别延迟 <80ms" },
        // --- F280 触屏长按右键 ---
        KnobDef { id: "h2.f280.press_ms", item: "F280", name: "长按触发时长", default: 500, min: 400, max: 700, unit: "ms", basis: "长按 500ms=右键（±50ms 工艺容差）" },
        KnobDef { id: "h2.f280.menu_lift_px", item: "F280", name: "菜单上移量", default: 48, min: 24, max: 96, unit: "px", basis: "菜单出现在手指位置上方 48px 防手指遮挡" },
        KnobDef { id: "h2.f280.min_hit_px", item: "F280", name: "最小触区边长", default: 44, min: 32, max: 64, unit: "px", basis: "触屏命中目标放大到 44×44px 最小触区" },
        // --- F281 通知交互细则 ---
        KnobDef { id: "h2.f281.banner_ms", item: "F281", name: "横幅驻留时长", default: 5000, min: 3000, max: 8000, unit: "ms", basis: "横幅 5 秒自动入通知中心（±0.5s）" },
        KnobDef { id: "h2.f281.merge_s", item: "F281", name: "同应用合并窗口", default: 30, min: 10, max: 120, unit: "s", basis: "同一应用 30 秒内多条合并为一条计数" },
        KnobDef { id: "h2.f281.max_btns", item: "F281", name: "操作按钮上限", default: 2, min: 1, max: 3, unit: "个", basis: "最多两个操作按钮（超过收进「更多」）" },
        // --- F282 应用单例策略 ---
        // （三策略为声明制——无旋钮）
        // --- F283 应用启动骨架与首窗就绪 ---
        KnobDef { id: "h2.f283.feedback_ms", item: "F283", name: "点击反馈判线", default: 100, min: 50, max: 300, unit: "ms", basis: "图标点击即有反馈（<100ms）" },
        KnobDef { id: "h2.f283.frame_ms", item: "F283", name: "窗框出现判线", default: 200, min: 100, max: 500, unit: "ms", basis: "200ms 内出窗框+骨架屏" },
        KnobDef { id: "h2.f283.stall_ms", item: "F283", name: "进度原因门槛", default: 2000, min: 1000, max: 5000, unit: "ms", basis: "超过 2s 未就绪显示进度原因" },
        // --- F284 无响应判定与恢复 ---
        KnobDef { id: "h2.f284.hang_ms", item: "F284", name: "无响应判定阈值", default: 5000, min: 2000, max: 15000, unit: "ms", basis: "应用主线程 5 秒无响应判定为挂起" },
        KnobDef { id: "h2.f284.dim_pct", item: "F284", name: "挂起蒙层暗化", default: 20, min: 10, max: 40, unit: "%", basis: "内容冻结在最后一帧加暗化 20% 蒙层" },
        KnobDef { id: "h2.f284.floater_ms", item: "F284", name: "浮条自动收起", default: 5000, min: 2000, max: 10000, unit: "ms", basis: "默认不动（再等 5s 自动消失）" },
        // --- F285 图标缓存与刷新 ---
        KnobDef { id: "h2.f285.disk_cap", item: "F285", name: "磁盘缓存条目上限", default: 20000, min: 2000, max: 100000, unit: "条", basis: "磁盘缓存有上限（超限 LRU 淘汰）" },
        KnobDef { id: "h2.f285.refresh_ms", item: "F285", name: "变更更新判线", default: 1000, min: 250, max: 3000, unit: "ms", basis: "保存图片后缩略图 1s 内更新（不需要 F5）" },
        // --- F286 壁纸多屏设置 ---
        KnobDef { id: "h2.f286.rot_min", item: "F286", name: "轮换间隔下限", default: 15, min: 15, max: 60, unit: "分钟", basis: "定时轮换间隔 15 分钟-1 天可选" },
        // --- F287 附加时钟 ---
        KnobDef { id: "h2.f287.tz_cap", item: "F287", name: "附加时区上限", default: 2, min: 1, max: 4, unit: "个", basis: "支持附加两个时区" },
        // --- F288 字体管理 ---
        KnobDef { id: "h2.f288.preview_min", item: "F288", name: "预览字号下限", default: 8, min: 6, max: 16, unit: "px", basis: "预览窗（字号滑杆）" },
        KnobDef { id: "h2.f288.preview_max", item: "F288", name: "预览字号上限", default: 288, min: 96, max: 512, unit: "px", basis: "预览窗（字号滑杆）" },
        // --- F289 打印队列中心 ---
        KnobDef { id: "h2.f289.stuck_s", item: "F289", name: "卡住提醒阈值", default: 60, min: 30, max: 300, unit: "s", basis: "默认策略「卡住 60 秒提醒」" },
        // --- F290 外设状态页 ---
        // （诊断分档为策略——无旋钮）
        // --- F291 耗电排行 ---
        KnobDef { id: "h2.f291.anomaly_x", item: "F291", name: "异常耗电倍数", default: 3, min: 2, max: 5, unit: "×", basis: "高于同应用历史均值 3 倍标黄" },
        KnobDef { id: "h2.f291.top_n", item: "F291", name: "排行深度", default: 10, min: 5, max: 20, unit: "条", basis: "耗电排行 Top10" },
        // --- F292 快捷方式健康 ---
        KnobDef { id: "h2.f292.fade_pct", item: "F292", name: "断链图标淡化", default: 50, min: 30, max: 70, unit: "%", basis: "图标淡化 50%" },
        // --- F293 可移动介质接入询问 ---
        KnobDef { id: "h2.f293.timeout_s", item: "F293", name: "询问条自动收起", default: 10, min: 5, max: 30, unit: "s", basis: "10 秒无操作自动按「不做任何事」静默收起" },
        // --- F294 安全弹出与拔出保护 ---
        // （拦截为红线——无旋钮，纪律：红线不设开关）
        // --- F295 时间同步与准确性 ---
        KnobDef { id: "h2.f295.ntp_s", item: "F295", name: "NTP 静默修正阈值", default: 2, min: 1, max: 10, unit: "s", basis: "偏差 >2s 静默修正，修正事件留痕" },
        // --- F296 区域显示格式 ---
        KnobDef { id: "h2.f296.decimals", item: "F296", name: "默认小数位", default: 1, min: 0, max: 3, unit: "位", basis: "数字显示格式独立于语言可选" },
        // --- F297 壁纸暗色压暗 ---
        KnobDef { id: "h2.f297.dim_pct", item: "F297", name: "默认压暗", default: 30, min: 0, max: 50, unit: "%", basis: "深色主题下壁纸自动压暗 30%" },
        KnobDef { id: "h2.f297.desat_pct", item: "F297", name: "默认降饱和", default: 15, min: 0, max: 50, unit: "%", basis: "并降饱和 15%（可关）" },
        KnobDef { id: "h2.f297.budget_ms", item: "F297", name: "滤镜帧预算", default: 2, min: 1, max: 8, unit: "ms", basis: "GPU 合成器一次性滤镜（帧预算内）" },
        // --- F298 快速设置磁贴编辑 ---
        // （默认 8/候选 16 为清单唯一源——无旋钮）
        // --- F299 开始菜单推荐区 ---
        KnobDef { id: "h2.f299.badge_days", item: "F299", name: "新装徽标高亮期", default: 7, min: 3, max: 14, unit: "天", basis: "最近安装（新装应用高亮 7 天徽标）" },
        KnobDef { id: "h2.f299.top_n", item: "F299", name: "推荐区容量", default: 6, min: 4, max: 10, unit: "条", basis: "近期文件（F072 引擎 Top6）" },
        // --- F300 系统图标语汇总表 ---
        KnobDef { id: "h2.f300.grid_px", item: "F300", name: "图标标准栅格", default: 24, min: 16, max: 48, unit: "px", basis: "24px 标准栅格（描边 1.5px）" },
    ]
}

/// 全表自检：id 唯一、默认值在合法域内、F 编号归属 F251-F300、
/// 每条有主册依据（无依据的参数不许上桌）。
pub fn validate_all() -> Result<(), &'static str> {
    let defs = defs();
    for i in 0..defs.len() {
        let a = &defs[i];
        if !a.id.starts_with("h2.f") {
            return Err("旋钮 id 必须以 h2.f 开头");
        }
        if !a.in_domain(a.default) {
            return Err("默认值出域");
        }
        if a.basis.is_empty() {
            return Err("无主册依据的参数不许登记");
        }
        // item 字段须为 F251-F300。
        let ok_item = a.item.starts_with('F')
            && a.item[1..]
                .parse::<u32>()
                .map(|n| (251..=300).contains(&n))
                .unwrap_or(false);
        if !ok_item {
            return Err("item 不在 F251-F300 域内");
        }
        for b in &defs[i + 1..] {
            if a.id == b.id {
                return Err("id 重复");
            }
        }
    }
    Ok(())
}

/// 校验一个导入值（设置导入导出 F305 联动口——域外包导入先过这里）。
pub fn check_import(id: &str, v: i64) -> Result<(), &'static str> {
    let defs = defs();
    match defs.iter().find(|d| d.id == id) {
        None => Err("未知旋钮"),
        Some(d) => {
            if d.in_domain(v) {
                Ok(())
            } else {
                Err("值出合法域")
            }
        }
    }
}

/// 导出给开发者文档站（F135）的 Markdown 表——账本页公开判据。
pub fn export_docs() -> String {
    let mut out = String::from(
        "| 旋钮 | 项 | 名称 | 默认 | 合法域 | 单位 | 主册依据 |\n| --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for d in defs() {
        out.push_str(&alloc::format!(
            "| `{}` | {} | {} | {} | {}..{} | {} | {} |\n",
            d.id, d.item, d.name, d.default, d.min, d.max, d.unit, d.basis
        ));
    }
    out
}

pub fn run_h2knob_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2knob");
    set.add("h2knob validate", validate_all().is_ok(), "全表合法");
    let n = defs().len();
    set.add("h2knob coverage", n >= 50, "50 项均有登记（部分项为策略无旋钮）");
    // 导入校验：未知拒、出域拒、域内过。
    set.add(
        "h2knob import gate",
        check_import("h2.f267.trash_days", 45).is_ok()
            && check_import("h2.f267.trash_days", 3).is_err()
            && check_import("h2.f999.none", 1).is_err(),
        "F305 gate",
    );
    // 文档导出可达。
    let doc = export_docs();
    set.add(
        "h2knob docs export",
        doc.contains("h2.f267.trash_days") && doc.contains("主册依据"),
        "F135 source",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2knob_all_green() {
        let set = run_h2knob_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "h2knob 自检红 {f}/{p}");
    }

    #[test]
    fn defaults_are_stable() {
        // 关键默认值钉死（主册定值不许漂移）。
        let get = |id: &str| defs().iter().find(|d| d.id == id).unwrap().default;
        assert_eq!(get("h2.f267.trash_days"), 30);
        assert_eq!(get("h2.f284.hang_ms"), 5000);
        assert_eq!(get("h2.f276.gap_px"), 8);
        assert_eq!(get("h2.f280.press_ms"), 500);
    }
}
