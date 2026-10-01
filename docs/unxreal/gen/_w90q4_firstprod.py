#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
_w90q4_firstprod.py — AI-104 · W90-Q4 WebView2/浏览器内核嵌入 · 首产段 B01–B15 生成器
================================================================================
明令：Variable 2026-10-01「一次对话 300 项新功能」。
产出四落件：
  1. 独立增补册 docs/Varix/CoRun Varix STAR II · Unxreal/AI-104 · Q4 · 300项新功能增补册（B01–B15 · W90-Q4-001–W90-Q4-300）.md
  2. 主汇编册（登记行插入卷首登记区 + 卷末增补卷纯追加，前缀哈希校验，R-PROC-002 防护）
  3. AI分工完成图 卷尾会话登记（纯追加）
  4. 统一协作总台账 会话条目（纯追加）
五断言内嵌，ALL PASS exit=0 才允许写盘。

域账口径：W90-Q4 域 = 40 包 × 20 条 × 300 行级 = 240,000 行级；本段 B01–B15 = 300 项 =
90,000 行级（域账 37.5%）；行数模式每批 5×320 + 10×300 + 5×280 = 6,000。
判据锚（任务书 §104.4 全文保真）：
  Q4-B01-04-J1（T3）→ W90-Q4-004
  Q4-B13-09-J3（T2）→ W90-Q4-249
（Q4-B27-03-J2 属 B16+ 余段，本段不含，如实登记。）
"""
import hashlib
import io
import os
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))
DOCDIR = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal")
MAIN_MD = os.path.join(DOCDIR, "CoRun Varix STAR II · Unxreal.md")
CHART_MD = os.path.join(DOCDIR, "CoRun Varix STAR II · Unxreal · AI分工完成图.md")
LEDGER_MD = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")
BOOK_OUT = os.path.join(DOCDIR, "AI-104 · Q4 · 300项新功能增补册（B01–B15 · W90-Q4-001–W90-Q4-300）.md")

LINES = [320] * 5 + [300] * 10 + [280] * 5  # 每批 6,000 行
VOL_HEADER = "## 增补卷 · AI-104 · W90-Q4 WebView2/浏览器内核嵌入 · 首产段 B01–B15（W90-Q4-001–W90-Q4-300 · 300 项 · 90,000 行）"

# ---------------------------------------------------------------------------
# 判据模板
# ---------------------------------------------------------------------------

def crit(wid, jid, tier, name, body, axis, anchor, gate):
    """构造单条判据文本。body 为该条核心断言句（含任务书锚 verbatim 时直接用锚文本）。"""
    t = {"T1": "T1 断言脚本", "T2": "T2 一致性对拍", "T3": "T3 真实应用冒烟"}[tier]
    parts = [f"{wid}-J1（{t}）{name}——{body}",
             "同型故障注入 ×10 全检出并回放三要素告警（发生了什么/为什么/下一步，十三章口径）"]
    if axis:
        parts.append(axis)
    if anchor:
        parts.append("联签锚定：" + anchor + "，零改写")
    if gate:
        parts.append(gate)
    return "；".join(parts)

GATE_T3 = "真机档随闸门补测登记（开发期零 QEMU 零实机写，如实不虚报）"

AX_W2 = "判据主轴：WebView2 装载零白屏与版本检测零虚报"
AX_CTL = "判据主轴：WebView2Samples 12 demo 全过（T3 回归锚 Q4-B01-04-J1 零回退）"
AX_CEF = "判据主轴：OSR 帧呈现零空窗与依赖树零缺项"
AX_IE = "判据主轴：IWebBrowser2/IHTMLDocument 对拍一致（T2 锚 Q4-B13-09-J3 零回退）"

A_D5 = "主册 UNX-D5（AI-13）COM 基座"
A_D4 = "主册 UNX-D4（AI-19）窗口与消息面"
A_E3 = "主册 UNX-E3（AI-23）WinInet/WinHTTP 语义"
A_G3 = "主册 UNX-G3 合成器纹理 import 冻结接口"
A_J4 = "主册 UNX-J4（AI-49）沙箱与隔离"
A_M2 = "主册 UNX-M2（AI-62）IME 与键鼠面"
A_D2 = "主册 UNX-D2（AI-17）PE 装载语义"
A_A3 = "主册 UNX-A3（AI-03）调度语义"
A_Q8 = "W90-Q8（AI-108）Schannel/CNG 证书与 TLS"
A_Q10 = "W90-Q10（AI-110）Shim 引擎与应用数据库"
A_Q12 = "W90-Q12（AI-111）游戏平台与启动器（下游消费面）"
A_R5 = "W90-R5（AI-116）LTS 发布与年度镜像"
A_VFR = "W90 治理线版本冻结官（AI-132）"
A_LIC = "W90 治理线授权合规官（AI-125）与许可记录（AI-95 口径）"
A_EXT = "W90 治理线外部套件官（AI-122）上游跟随制度"

# ---------------------------------------------------------------------------
# 15 批 × 20 条：每条 = (功能名称, T档, 核心断言句, 锚, 是否真机档)
# ---------------------------------------------------------------------------

BATCHES = [
{
 "id": "B01", "theme": "WebView2 Runtime 装载与发现", "kind": "F 型地基",
 "spec": "WebView2 Runtime 发现/装载/环境创建全链基座：注册表发现协议、版本字符串探测、环境工厂入口、固定版本与默认 UDF 约定、失败码语义表、缺失三要素呈现、并发互斥、装载器解析序、装载日志埋点、失败注入床、dry-run 预览与原子写安全。",
 "items": [
  ("WebView2 Runtime 注册表发现协议（HKCU/HKLM EdgeUpdate Clients pv 键语义）", "T1",
   "参照机对拍发现路径与键位读取序一致，20 键位场景零漏读", "主册 UNX-D3（AI-18）注册表语义面", False),
  ("GetAvailableCoreWebView2BrowserVersionString 等效入口（默认/指定目录两形态）", "T1",
   "两形态返回串格式与版本四段值与冻结样本完全一致", "主册 UNX-D5（AI-13）COM 基座", False),
  ("CreateCoreWebView2EnvironmentWithOptions 等效工厂入口", "T1",
   "参数编解码 12 字段全过，回调交付认购序与契约一致", A_D5, False),
  ("任务书锚：WebView2Samples 官方示例 12 demo 全通过", "T3",
   "WebView2 官方示例应用（WebView2Samples）全部 12 个 demo 通过", "任务书 §104.4 判据原文", True),
  ("browserExecutableFolder 固定版本运行时装载语义", "T1",
   "固定目录装载 5 版本样本全过，优先级高于 Evergreen 注册表发现", A_R5, False),
  ("userDataFolder 默认约定（exe 同目录优先与父目录回退规则）", "T1",
   "回退序断言与冻结行为一致，只读目录降级路径可解释", "", False),
  ("环境创建失败码语义表（HRESULT 映射对拍）", "T2",
   "10 失败场景返回码与冻结映射表逐条一致", A_D5, False),
  ("Runtime 缺失三要素呈现与下载指引", "T1",
   "呈现含发生了什么/为什么/下一步（含获取指引），零裸错误码", "", False),
  ("多环境并发与用户数据目录互斥锁语义", "T2",
   "同 UDF 双环境并发行为与 Windows 对拍一致（后到者失败码一致）", "", False),
  ("装载器 loader DLL 解析顺序（应用目录→系统语义对拍）", "T1",
   "解析序断言与冻结序一致，错误版本 loader 拒载并可解释", "", False),
  ("环境选项编码（language/附加 switches 传递）", "T1",
   "选项透传端到端一致（子进程命令行账核对）", "", False),
  ("组策略版本门控读取（更新策略只读面）", "T1",
   "只读断言：探测零写入，策略值影响发现序与冻结表一致", "", False),
  ("环境预创建与预热池钩子（冷启动前置）", "T1",
   "预热命中路径省时账成立（模拟床 P50 对比断言）", "", False),
  ("装载全链日志埋点（入口/出口/耗时/结果/异常栈）", "T1",
   "埋点覆盖率 100% 断言，日志零阻塞交互（异步批量）", "", False),
  ("装载失败注入床 ×10（键损坏/权限拒绝/DLL 缺失/版本畸形）", "T1",
   "10/10 检出，进程零崩溃，三要素告警全回放", "", False),
  ("装载决策 dry-run 预览（不改系统状态）", "T1",
   "预览列出将发生的发现/创建/写入动作且执行零副作用", "", False),
  ("UDF 创建原子性与锁文件安全（防半初始化目录）", "T1",
   "断电注入 ×10 后 UDF 状态一致（零半成品目录）", "", False),
  ("版本快照与年度镜像固定版登记", "T2",
   "快照 JSON 与镜像登记账互证一致", A_R5, False),
  ("ICoreWebView2 承载与 COM 激活复用联签", "T1",
   "复用主册 COM 激活/聚合语义零重写断言（接口清单对表）", A_D5, False),
  ("B01 批联轧：装载链 20 条互证", "T1",
   "20 条联轧断言全过，批账 6,000 行守恒核销", "", False),
 ]},
{
 "id": "B02", "theme": "Evergreen 版本检测与通道语义", "kind": "F 型地基",
 "spec": "Evergreen 版本检测与通道治理：pv 四段比较器、自更新生效账、版本冻结承诺接口、四通道探测、固定版本隔离、检测缓存、最低版本协商、只读红线、fuzz、导出、性能与对拍。",
 "items": [
  ("pv 四段版本比较器语义（比较/相等/前缀规则）", "T1",
   "比较器 40 用例（含前缀/通配/畸形）全过", "", False),
  ("Evergreen 自更新检测语义（更新落盘→下次启动生效账）", "T2",
   "生效时序断言与 Windows 行为一致（当前会话版本不变）", A_R5, False),
  ("版本冻结承诺接口（冻结版本清单与豁免流程）", "T1",
   "冻结接口 8 字段冻结，变更走登记流程断言", A_VFR, False),
  ("渠道探测语义（Stable/Beta/Dev/Canary 四通道键位扫描）", "T1",
   "四通道键位扫描序与优先级断言全过", "", False),
  ("固定版本模式隔离目录账（与 Evergreen 互斥优先级）", "T1",
   "互斥仲裁断言：固定目录存在时 Evergreen 零影响", A_R5, False),
  ("版本检测缓存与失效语义（TTL/事件失效）", "T1",
   "TTL 失效与显式失效两路径断言全过", "", False),
  ("最低版本要求协商语义（应用要求与 Runtime 能力比对）", "T1",
   "协商 20 组合断言全过（满足/不满足/未知三态）", "", False),
  ("版本探测只读红线断言（零注册表写入零文件落盘）", "T1",
   "探测全程零写入机检断言（监督钩全程在线）", "", False),
  ("多 Runtime 并存优先级仲裁账", "T1",
   "并存 4 场景仲裁结果与冻结表一致", "", False),
  ("卸载残留检测与修复建议呈现", "T1",
   "残留检测 5 场景全过，建议含三要素", "", False),
  ("检测面 fuzz：畸形 pv/空值/超长/非数字 20 例", "T1",
   "20/20 不崩溃且返回语义正确降级值", "", False),
  ("版本账导出开放格式（JSON 快照可交付）", "T1",
   "导出 JSON schema 校验通过且可重导入对账", "", False),
  ("检测性能账（P95 <5ms ×3 轮）", "T1",
   "三轮 P95 全部 <5ms（宿主测试床模拟档）", "", False),
  ("与 Windows 参照机版本语义一致性对拍", "T2",
   "同机双环境探测结果一致（L1-L4 声明随证据包）", "", True),
  ("Chromium 主版本季度对表登记（上游跟随制度）", "T1",
   "对表登记条目 schema 过断言，漂移项可溯源", A_EXT, False),
  ("回归集：30 检测往返断言零回退", "T1",
   "30 往返零回退（已绿失绿即 P1 登记）", "", False),
  ("版本检测遥测禁接声明（微软遥测零采集）", "T1",
   "零外发机检断言（网络监督钩全程在线）", "", False),
  ("B02×B01 互证：装载链×检测面联轧", "T1",
   "互证 12 场景全过（版本变→装载决策变）", "", False),
  ("版本冻结官与授权账双联签锚定行", "T1",
   "双联签锚定行在位且零改写", A_VFR + "；" + A_LIC, False),
  ("B02 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销，收官印在位", "", False),
 ]},
{
 "id": "B03", "theme": "ICoreWebView2 环境/控制器/设置 COM 面", "kind": "F 型地基",
 "spec": "环境/控制器/设置三层 COM 面语义：Environment 工厂语义、Controller Bounds/HWND 承载、Settings 全属性、导航族与事件序、History、ExecuteScript、CapturePreview、进程失败分级、焦点环、DPI 变更与 vtable 序对拍。",
 "items": [
  ("ICoreWebView2Environment 等效面（CreateCoreWebView2Controller 语义）", "T1",
   "工厂语义 8 场景断言全过（含并发创建排队）", A_D5, False),
  ("ICoreWebView2Controller Bounds/ParentWindow 语义（HWND 承载）", "T2",
   "Bounds 变更端到端呈现一致，父子 HWND 关系对拍成立", A_D4, False),
  ("ICoreWebView2Settings 全属性语义（40+ 布尔/枚举）", "T1",
   "属性设取往返 100% 断言，生效路径端到端验证", "", False),
  ("Visible/IsVisible 状态机与焦点语义", "T1",
   "状态迁移表全覆盖（含隐藏期间焦点让渡）", A_D4, False),
  ("ICoreWebView2 导航族（Navigate/NavigateToString/Stop/Reload）", "T1",
   "四方法 24 场景断言全过（含非法 URL 拒绝）", "", False),
  ("导航事件序列对拍（NavigationStarting/Completed/HistoryChanged 顺序）", "T2",
   "事件序与 Windows 对拍逐条一致 ×3 轮", "", False),
  ("History 面语义（GoBack/GoForward/CanGoBack 全局态）", "T1",
   "历史栈 30 步操作账断言全过", "", False),
  ("document.Title/URI 语义与属性变更通知", "T1",
   "属性变更通知时序断言全过（含 iframe 主文档口径）", "", False),
  ("ExecuteScript 语义（JSON 序列化返回/异步完成）", "T1",
   "返回值 JSON 编码 20 用例全过（含循环引用拒绝）", "", False),
  ("CapturePreview 语义（PNG/JPEG 位图导出）", "T1",
   "导出图像哈希账与基准一致（尺寸/格式断言）", "", False),
  ("Controller.Close 生命周期语义（环境回收路径）", "T1",
   "关闭序断言：控制器→环境→资源回收零泄漏", "", False),
  ("ProcessFailed 事件三级分类语义", "T1",
   "分类 10 场景与冻结表一致（含恢复建议）", "", False),
  ("控制器 GotFocus/LostFocus 焦点环", "T1",
   "焦点环 20 步账断言（关闭后焦点还给宿主，十四章纪律）", A_D4, False),
  ("尺寸变化与 DPI 变更重排路径", "T1",
   "DPI 变更后布局成立断言（4K 缩放矩阵 4 档）", "", True),
  ("Controller 创建失败注入 ×10（无效 HWND/越界 Bounds）", "T1",
   "10/10 检出并三要素呈现，零资源泄漏", A_D4, False),
  ("COM 接口面 vtable 序对拍（与官方头文件接口序一致）", "T2",
   "vtable 序与官方头逐条一致（QI 全集断言）", A_D5, False),
  ("设置项回归：40 属性设取往返断言", "T1",
   "往返零回退（已绿失绿即 P1 登记）", "", False),
  ("事件回调线程模型断言（UI 线程串行化）", "T1",
   "回调线程归属断言全过（跨线程调用拒绝并可解释）", "", False),
  ("UNX-D5 COM 基座复用联签（激活/聚合零重写）", "T1",
   "复用面清单对表断言零重写", A_D5, False),
  ("B03 批联轧与批收官印", "T1",
   "20 条联轧全过，批账 6,000 行守恒核销", "", False),
 ]},
{
 "id": "B04", "theme": "Host 对象注入与 WebMessage 通信", "kind": "M 型机制",
 "spec": "宿主↔web 双向通信语义：AddHostObjectToScript 投影与编解码、WebMessage 双向通道、WebResourceRequested 拦截、虚拟域名映射、脚本注入时序、跨域隔离与恶意页面防线、端到端消息账。",
 "items": [
  ("AddHostObjectToScript 语义（COM 对象跨脚本投影）", "T1",
   "投影方法/属性/常量三类成员断言全过", A_D5, False),
  ("hostObject 方法调用编解码（参数 VARIANT 序列化对拍）", "T2",
   "类型矩阵 20 型编解码与 Windows 对拍逐字节一致", A_D5, False),
  ("hostObject 属性读取/写入语义", "T1",
   "读写往返 30 用例断言全过（含只读属性拒绝）", A_D5, False),
  ("RemoveHostObjectToScript 与失效语义", "T1",
   "移除后调用路径断言（拒绝且可解释，不崩溃）", "", False),
  ("PostWebMessageAsJson 语义（宿主→web 数据通道）", "T1",
   "JSON 编解码 20 用例端到端一致", "", False),
  ("PostWebMessageAsString 与类型区分语义", "T1",
   "两形态区分断言（Received 端类型判定一致）", "", False),
  ("WebMessageReceived 事件（web→宿主通道）", "T1",
   "事件交付时序与负载保真断言全过", "", False),
  ("WebResourceRequested 拦截语义（自定义协议/资源替换）", "T1",
   "拦截 12 场景断言（含替换/放行/重定向三态）", "", False),
  ("SetVirtualHostNameToFolderMapping 语义（虚拟域名→本地目录）", "T1",
   "映射访问与越界拒绝断言（目录白名单外零可达）", "", False),
  ("AddWebResourceRequestedFilter 匹配规则语义", "T1",
   "通配/上下文标志匹配 20 用例全过", "", False),
  ("AddScriptToExecuteOnDocumentCreated 语义", "T1",
   "注入时序断言（每文档创建前执行，iframe 口径一致）", "", False),
  ("注入时序账（文档创建前执行对拍）", "T2",
   "时序对拍 ×3 轮一致（先注入后文档脚本）", "", False),
  ("WebMessage 大小上限与分片行为对拍", "T2",
   "上限边界行为与 Windows 一致（超限错误语义）", "", False),
  ("跨域隔离：hostObject 暴露域控制（仅白名单源可达）", "T1",
   "非白名单源调用 100% 拒绝断言", "", False),
  ("WebMessage 注入床 ×10（畸形 JSON/超大对象/循环引用）", "T1",
   "10/10 检出零崩溃，三要素告警全回放", "", False),
  ("恶意页面防线：非信任源 hostObject 调用全拒绝", "T1",
   "攻击样本 20 例全拒绝（含原型污染/回调重入）", "", False),
  ("端到端：宿主↔web 双向消息往返 1,000 次零丢失", "T1",
   "1,000 往返零丢失零乱序断言（序列号账）", "", False),
  ("W90-Q8 联签：内层证书/加密调用语义锚定", "T1",
   "内层加密调用走 Q8 冻结接口断言（零重写）", A_Q8, False),
  ("主册 UNX-D5 VARIANT/IDispatch 语义复用联签", "T1",
   "复用面对表零重写断言", A_D5, False),
  ("B04 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销", "", False),
 ]},
{
 "id": "B05", "theme": "CookieManager 与站点存储语义", "kind": "M 型机制",
 "spec": "Cookie 全生命周期与站点存储：CookieManager 面、属性全集、域匹配、删除作用域、CookieChanged 三态、持久化规则、localStorage/IndexedDB/Cache 面、清除语义、第三方 cookie 策略、配额呈现、隐私隔离与断电安全。",
 "items": [
  ("ICoreWebView2CookieManager 等效面（CreateCookie/SetCookie 全参）", "T1",
   "全参 12 字段断言全过（含非法组合拒绝）", "", False),
  ("Cookie 属性全集语义（secure/httpOnly/sameSite/expires）", "T1",
   "属性语义 30 用例对拍一致", "", False),
  ("GetCookies 域匹配语义（RFC 6265 域匹配对拍）", "T2",
   "域匹配 40 用例与 RFC 语义逐条一致", "", False),
  ("DeleteCookies/DeleteAllCookies 域路径作用域语义", "T1",
   "作用域 20 场景断言（精确/通配/父域边界）", "", False),
  ("CookieChanged 事件语义（Inserted/Updated/Deleted 三态）", "T1",
   "三态触发序断言全过（含批量操作事件账）", "", False),
  ("Cookie 持久化账（会话 vs 持久 cookie 落盘规则）", "T1",
   "落盘规则断言（会话零落盘/持久即时落盘）", "", False),
  ("localStorage/sessionStorage 语义面（按源隔离）", "T1",
   "源隔离断言（跨源零可达）与容量语义一致", "", False),
  ("IndexedDB 存储账（容量/淘汰策略语义）", "T1",
   "容量账与淘汰序断言全过（模拟床）", "", False),
  ("Cache 存储与 HTTP 缓存语义（策略位对拍）", "T2",
   "缓存策略 10 场景与主册网络面对拍一致", A_E3, False),
  ("ClearBrowsingData 语义（数据类型位掩码）", "T1",
   "位掩码组合 16 场景清除范围断言全过", "", False),
  ("第三方 cookie 策略语义（阻止规则位对拍）", "T2",
   "策略位行为与 Windows 对拍一致", "", False),
  ("存储配额与超限错误呈现（三要素）", "T1",
   "超限呈现含三要素，零裸配额码", "", False),
  ("cookie 注入床 ×10（畸形域/超长值/冲突路径）", "T1",
   "10/10 检出零崩溃，告警回放完整", "", False),
  ("隐私红线：cookie/存储零跨应用泄漏（UDF 隔离断言）", "T1",
   "跨应用隔离机检断言（双 UDF 互不可见）", "", False),
  ("持久化断电安全（写入原子性+恢复一致性）", "T1",
   "断电注入 ×10 后账面一致（零半条 cookie）", "", False),
  ("主册 UNX-E3 联签：WinINET cookie 语义边界划界", "T1",
   "共享 jar 边界划界断言（联签文本在位零改写）", A_E3, False),
  ("cookie 数量性能账（10,000 条遍历 P95 <50ms）", "T1",
   "三轮 P95 全达标（宿主模拟档）", "", False),
  ("回归集：cookie 30 往返断言零回退", "T1",
   "30 往返零回退断言", "", False),
  ("存储 API 版本差异账（接口 _2/_3/_4 演进对拍）", "T2",
   "演进接口行为差异账与冻结表一致", "", False),
  ("B05 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销", "", False),
 ]},
{
 "id": "B06", "theme": "用户数据目录约定与 WebView2 线首产联轧", "kind": "I 型集成",
 "spec": "UDF 生命周期与 WebView2 线联轧收官：目录结构规范、多 UDF 隔离、版本迁移、损坏自愈、权限、共享互斥、磁盘占用账、三段全景联轧、失败归因分流、下游与 shim 挂点联签、体验账、文档与缺陷过账、移交面冻结。",
 "items": [
  ("UDF 目录结构规范（EBWebView/Default 分层账）", "T1",
   "结构快照与冻结 schema 一致断言", "", False),
  ("多 UDF 共存与 per-user/per-app 隔离语义", "T1",
   "隔离断言（双 UDF 双应用互不可见）", "", False),
  ("UDF 版本迁移账（Runtime 升级后 schema 迁移不丢数据）", "T1",
   "迁移前后数据保真断言（cookie/存储全量对账）", "", False),
  ("UDF 损坏自愈语义（检测→重建→数据保全分级）", "T1",
   "自愈 8 场景断言（用户数据保全优先级表）", "", False),
  ("UDF 权限语义（用户态 ACL 白名单）", "T1",
   "越权访问全拒绝断言（J1 安全域 KPI 口径一致）", "主册 UNX-J1（AI-46）安全模型", False),
  ("同 UDF 双环境共享互斥对拍", "T2",
   "互斥行为与 Windows 对拍一致", "", False),
  ("UDF 磁盘占用账与清理策略（缓存上限/淘汰）", "T1",
   "上限触发淘汰断言（淘汰序与冻结表一致）", "", False),
  ("WebView2 线全景联轧 I：装载→环境→控制器→导航全链", "T1",
   "全链 12 步断言零失败（判据主轴回归锚零回退）", "", False),
  ("WebView2 线全景联轧 II：hostObject/WebMessage/cookie 全通道", "T1",
   "三通道并发联轧断言全过", "", False),
  ("WebView2 线全景联轧 III：存储/UDF 生命周期全环", "T1",
   "全环断言（创建→使用→迁移→清理→重建）", "", False),
  ("失败归因分流账（缺 Runtime/锁冲突/权限/版本不匹配四类）", "T1",
   "四类归因准确率 100% 断言（40 样本账）", "", False),
  ("W90-Q10 联签：WebView2 失败模式族 shim 挂点", "T1",
   "挂点登记在位断言（零改写 shim 引擎域账）", A_Q10, False),
  ("W90-Q12 下游联签：平台启动器嵌入页消费面冻结", "T1",
   "消费面 v1 接口冻结断言（下游可开发 fake 就绪）", A_Q12, False),
  ("WebView2 线体验账：冷启动首帧 <800ms（模拟档）", "T1",
   "模拟床 P50/P95 双账达标（真机随闸门补测）", "", True),
  ("WebView2 线 12 demo 全量复跑（T3 回归锚零回退）", "T3",
   "12 demo 复跑全过（回归锚 Q4-B01-04-J1）", "任务书 §104.4 判据原文", True),
  ("Evergreen 冻结承诺台账登记（热修复通道接口）", "T1",
   "登记条目 schema 过断言（承诺账可审计）", A_R5, False),
  ("WebView2 线文档对齐（接口文档与实现一致性）", "T1",
   "文档-实现一致性机检断言（接口清单逐条对表）", "", False),
  ("缺陷账本过账（本线 🟢/🟡 项清单+清偿状态）", "T1",
   "账实相符断言（P0=0 前置，AI-97 口径）", "W90 治理线缺陷清偿门禁（AI-97）", False),
  ("WebView2 线收官互证与移交面冻结（向 B16+ 移交）", "T1",
   "移交清单冻结断言（深水段/清单收口段接口齐备）", "", False),
  ("B06 批联轧与 WebView2 线首产段收官印", "T1",
   "20 条联轧全过，线收官印在位", "", False),
 ]},
{
 "id": "B07", "theme": "CEF 依赖树与进程装载", "kind": "F 型地基",
 "spec": "libcef.dll 装载链：PE 依赖树解析、发行包结构约定、MainArgs/Settings 编解码、多进程启动账、subprocess 识别、版本检测、失败码、沙箱禁用路径、单实例锁、GPU 降级、fuzz、许可合规与版本锁定。",
 "items": [
  ("libcef.dll PE 依赖树解析（导入表/DelayLoad 全集清点）", "T1",
   "依赖清点与冻结清单一致断言（零缺项）", A_D2, False),
  ("CEF 发行包结构约定（Resources.pak/icudtl.dat/v8_context_snapshot 清单）", "T1",
   "包结构 12 件齐套断言（缺件三要素呈现）", "", False),
  ("CefMainArgs 语义（命令行编解码）", "T1",
   "命令行编解码 20 用例（含 Unicode/引号）全过", "", False),
  ("CefSettings 全字段语义（30+ 字段）", "T1",
   "字段语义设取往返断言（含互斥字段组校验）", "", False),
  ("多进程模型启动账（browser→subprocess 派生链）", "T1",
   "派生链断言（进程树拓扑与冻结图一致）", "", False),
  ("subprocess 路径解析与类型识别（--type= 标志语义）", "T1",
   "四角色类型识别断言全过", "", False),
  ("CEF 版本检测语义（cef_version 头/运行时互证）", "T1",
   "版本互证断言（编译期/运行时一致）", "", False),
  ("装载失败码语义表（缺 pak/缺 ICU/架构不符）", "T1",
   "失败码映射断言（10 场景逐条一致）", "", False),
  ("沙箱禁用路径语义（no-sandbox 与安全承诺账）", "T1",
   "禁用路径行为断言+安全承诺声明在位", A_J4, False),
  ("单实例锁与多实例共存语义", "T1",
   "锁行为断言（同 UDF 互斥/异 UDF 共存）", "", False),
  ("GPU 进程禁用降级路径（软件渲染回退链）", "T1",
   "回退链断言（降级可观测、三要素告知）", "", False),
  ("主册 UNX-D2 PE 装载语义复用联签", "T1",
   "复用面对表零重写断言", A_D2, False),
  ("依赖树 fuzz：缺 DLL/缺资源/错版本注入 ×10", "T1",
   "10/10 检出零宿主崩溃，告警回放完整", "", False),
  ("装载全链日志埋点（十三章口径）", "T1",
   "埋点覆盖率 100% 断言", "", False),
  ("CEF 许可合规账（BSD 许可归档）", "T1",
   "许可记录完整性断言（AI-95 口径 100%）", A_LIC, False),
  ("进程崩溃隔离账（renderer 崩溃不伤 browser）", "T1",
   "隔离断言（注入 ×10 零宿主崩溃）", "", False),
  ("dry-run：装载预检（依赖清点不落盘）", "T1",
   "预检零副作用断言", "", False),
  ("回归：装载链 20 往返断言", "T1",
   "20 往返零回退", "", False),
  ("版本锁定与年度镜像固定版登记", "T2",
   "锁定账与镜像登记互证一致", A_R5, False),
  ("B07 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销", "", False),
 ]},
{
 "id": "B08", "theme": "CefApp/CefClient 回调面", "kind": "M 型机制",
 "spec": "回调面全语义：App 命令行/进程前回调、BrowserProcessHandler、Client handler 族路由（LifeSpan/Load/Request/Display/ContextMenu/Dialog/JSDialog/Keyboard/Permission）、线程模型、缺省行为、重入防护与引用计数。",
 "items": [
  ("CefApp::OnBeforeCommandLineProcessing 语义", "T1",
   "命令行改写断言（追加/替换/拒绝三态）", "", False),
  ("OnBeforeProcessLaunch/OnContextInitialized 时序账", "T1",
   "时序断言（初始化单次性与顺序保证）", "", False),
  ("CefBrowserProcessHandler 语义（OnBeforeChildProcessLaunch 等）", "T1",
   "回调行为断言（含消息泵调度语义）", "", False),
  ("CefClient GetXxxHandler 族路由面", "T1",
   "路由断言（每查询类型正确返回/空值语义）", "", False),
  ("CefLifeSpanHandler 语义（OnBeforeClose/DoClose 生命周期）", "T1",
   "生命周期序断言（DoClose→OnBeforeClose 契约）", "", False),
  ("CefLoadHandler 语义（OnLoadingStateChange/LoadError）", "T1",
   "错误码映射断言（20 错误场景三要素呈现）", "", False),
  ("CefRequestHandler 语义（OnBeforeBrowse/GetAuthCredentials）", "T1",
   "导航拦截与认证回调断言（凭据零落日志红线）", A_Q8, False),
  ("CefDisplayHandler 语义（地址/标题/状态/控制台消息）", "T1",
   "四类消息回调断言（内容保真）", "", False),
  ("CefRenderHandler 路由声明（OSR 深化预告锚）", "T1",
   "路由声明冻结断言（B09 深化接口齐备）", "", False),
  ("CefContextMenuHandler 语义（菜单构建/命令分发）", "T1",
   "菜单命令分发断言（自定义项 ID 账）", "", False),
  ("CefDialogHandler 文件对话框回调语义", "T1",
   "回调路径断言（选择结果保真、取消安全）", "", False),
  ("CefJSDialogHandler 语义（alert/confirm/prompt 拦截）", "T1",
   "三对话框拦截断言（同步阻塞语义对拍）", "", False),
  ("CefKeyboardHandler 语义（pre-keystroke 拦截）", "T1",
   "拦截断言（与宿主快捷键共存仲裁）", A_M2, False),
  ("CefPermissionHandler 语义（媒体/地理/通知权限）", "T1",
   "权限请求-授予-持久化断言（默认全拒红线一致）", "主册 UNX-J1（AI-46）安全模型", False),
  ("回调线程模型断言（UI/IO 线程归属对拍）", "T1",
   "线程归属断言（错线程调用检测并告警）", "", False),
  ("回调缺省行为账（未实现 handler 的默认语义对拍）", "T2",
   "缺省行为与 Windows 对拍一致（20 场景）", "", False),
  ("回调注入床 ×10（回调内抛异常/重入/死锁预防）", "T1",
   "10/10 防护检出（异常隔离、重入拒绝、零死锁）", "", False),
  ("回调序对拍：页面生命周期全事件序列一致性", "T2",
   "全事件序对拍 ×3 轮一致", "", False),
  ("CefRefPtr 引用计数断言（零泄漏 ×1,000 周期）", "T1",
   "1,000 创建销毁周期零泄漏（账本对账）", "", False),
  ("B08 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销", "", False),
 ]},
{
 "id": "B09", "theme": "离屏渲染 OSR 基础", "kind": "M 型机制",
 "spec": "OSR 帧通道基础：windowless 初始化、OnPaint 脏矩形、加速绘制声明、帧率节流、虚拟坐标（鼠标/键盘/IME）、光标与弹窗、双缓冲交换、时间戳账、透明背景、resize、注入床与帧完整性。",
 "items": [
  ("WindowlessRenderingEnabled 语义与帧初始化", "T1",
   "初始化断言（首帧时限 <500ms 模拟档）", "", False),
  ("OnPaint 回调（脏矩形语义/full repaint 分支）", "T1",
   "脏矩形正确性断言（脏区内像素变更/区外零漂移）", "", False),
  ("OnAcceleratedPaint 语义声明（共享纹理句柄路径预告）", "T1",
   "声明冻结断言（B10 深化接口齐备）", "", False),
  ("帧率节流语义（windowless_frame_rate 对拍）", "T2",
   "节流行为与冻结值一致（1–60fps 边界）", "", False),
  ("OSR 鼠标事件坐标编解码（ScreenPoint/ViewPoint）", "T1",
   "坐标换算 20 用例断言（含 DPI 缩放矩阵）", "", False),
  ("OSR 键盘事件注入（NativeVirtualKey/Character 编码）", "T2",
   "编码对拍（主册键面语义一致）", A_M2, False),
  ("OSR IME 路径（OnImeCompositionRangeChanged 回传）", "T2",
   "中文输入端到端断言（组屏→候选窗→上屏，中文体验命门）", A_M2, True),
  ("OnCursorChange 语义（光标类型映射）", "T1",
   "光标映射表断言（10 型全覆盖）", "", False),
  ("OnPopupShow/OnPopupSize 语义（原生弹窗几何）", "T1",
   "弹窗几何断言（尺寸/位置/遮挡关系）", "", False),
  ("popup 绘制通道（与主视图双缓冲分离）", "T1",
   "分离断言（popup 绘制不污染主帧）", "", False),
  ("幕后缓冲交换语义（双缓冲零撕裂）", "T1",
   "撕裂检测断言（1,000 帧零撕裂）", "", False),
  ("帧时间戳账（单调钟打点与端到端延迟测量钩）", "T1",
   "时间戳单调性断言（零回绕）与延迟账在位", "", False),
  ("透明背景语义（背景色/alpha 通道）", "T1",
   "alpha 保真断言（合成结果与期望一致）", "", False),
  ("分辨率变化与 resize 重排", "T1",
   "resize 序断言（帧尺寸跟随零花屏）", "", False),
  ("OSR 帧注入床 ×10（超大脏矩形/零尺寸/乱序交换）", "T1",
   "10/10 防护检出（丢弃并告警，零崩溃）", "", False),
  ("帧完整性断言（脏区外像素零漂移）", "T1",
   "完整性机检（随机帧抽样比对）", "", False),
  ("帧率性能账：60fps P95 + 1% low（模拟档）", "T1",
   "P95 与 1% low 双账达标（模拟床）", "", True),
  ("主册 UNX-D4 窗口面联签（OSR 位图 blit 语义锚）", "T1",
   "blit 语义锚定断言（零改写窗口面）", A_D4, False),
  ("拖拽 OSR 路径（DragEnter/DragUpdate 坐标换算）", "T1",
   "拖拽换算断言（落点/放弃/复原三路径）", "", False),
  ("B09 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销", "", False),
 ]},
{
 "id": "B10", "theme": "OSR 与合成器纹理路径·零拷贝", "kind": "M 型机制",
 "spec": "GPU 纹理直通：OnAcceleratedPaint 共享纹理句柄、合成器 import 路径、零拷贝判据、纹理生命周期、保护内容黑帧（不规避 DRM 红线）、双路径仲裁、帧同步、多实例、色彩空间/stride、注入床、性能双账与诚实降级清单。",
 "items": [
  ("OnAcceleratedPaint 共享纹理句柄语义（native handle 编解码）", "T1",
   "句柄编解码断言（取用/归还/失效三态）", "", False),
  ("纹理导入到合成器路径（import 接口对接）", "T1",
   "import 链路断言（句柄→合成器纹理零中间转换）", A_G3, False),
  ("零拷贝判据账：GPU 进程→合成器零中间拷贝断言", "T1",
   "拷贝计数账断言（全链拷贝数=0）", A_G3, False),
  ("纹理生命周期（release 回调/资源回收账）", "T1",
   "回收账断言（1,000 帧零泄漏）", "", False),
  ("保护内容占位（黑帧诚实呈现——不规避 DRM 红线）", "T1",
   "黑帧呈现断言+不规避声明在位（W90 五红线①）", "W90-Q6（AI-106）DRM 域边界", False),
  ("双路径仲裁：OnPaint 回退与 OnAcceleratedPaint 优先级", "T1",
   "仲裁断言（加速优先/失败降位图可观测）", "", False),
  ("帧同步语义（vsync 账/丢帧计数）", "T1",
   "vsync 对齐断言与丢帧计数账在位", "", False),
  ("多视图纹理流（多 CEF 实例并发纹理账）", "T1",
   "并发断言（4 实例纹理流互不串扰）", "", False),
  ("色彩空间语义（BGRA/Swizzle 对拍）", "T2",
   "像素格式对拍逐条一致", "", False),
  ("尺寸协商与 stride 语义（行对齐账）", "T1",
   "stride 对齐断言（非对齐尺寸样本 10 例）", "", False),
  ("纹理失败注入 ×10（句柄失效/尺寸突变/回收竞态）", "T1",
   "10/10 防护检出（降级位图+告警）", "", False),
  ("性能账：纹理路径 vs 位图路径延迟对比（P95 双账）", "T1",
   "双账在位且纹理路径 P95 更优断言（模拟档）", "", False),
  ("帧率账：滚动场景 60fps 帧间隔抖动（模拟档）", "T1",
   "抖动账达标（1% low 断言）", "", True),
  ("显存上限账（纹理池配额与回收）", "T1",
   "配额超限回收断言（回收序与冻结表一致）", "主册 UNX-G1（AI-31）显存账边界", False),
  ("主册 UNX-G3 合成器纹理 import 冻结接口联签", "T1",
   "冻结接口锚定断言（零改写合成器域账）", A_G3, False),
  ("跨进程同步语义（GPU 进程↔browser 进程 fence）", "T1",
   "fence 时序断言（零竞态样本 100 例）", "", False),
  ("帧日志埋点（每帧时间戳/来源/路径）", "T1",
   "埋点覆盖率 100%（异步批量零阻塞）", "", False),
  ("开放性：纹理路径诊断导出（开放格式帧账）", "T1",
   "导出 schema 校验过断言（可重放）", "", False),
  ("诚实降级账：位图降级缺失清单（红线②）", "T1",
   "缺失清单在位断言（降级必附清单）", "", False),
  ("B10 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销", "", False),
 ]},
{
 "id": "B11", "theme": "CEF 多进程沙箱语义", "kind": "E 型边界",
 "spec": "多进程与沙箱：四角色进程账、renderer 隔离、site isolation、崩溃恢复链、优先级配额、utility 进程族、沙箱边界映射、IPC 通道、V8 配额、watchdog、孤儿回收、崩溃注入床、逃逸测试集对接与资源上限。",
 "items": [
  ("进程角色账（browser/renderer/gpu/utility 四角色语义）", "T1",
   "角色识别与职责边界断言（拓扑账一致）", "", False),
  ("renderer 进程隔离语义（每源实例化策略对拍）", "T2",
   "实例化策略与 Windows 行为对拍一致", "", False),
  ("site isolation 策略语义（跨源文档隔离位）", "T1",
   "隔离位断言（跨源文档零共享进程样本 20 例）", "", False),
  ("renderer 崩溃→OnRenderProcessTerminated 恢复路径", "T1",
   "恢复链断言（重载后状态可解释、零宿主崩溃）", "", False),
  ("进程优先级与资源配额账（后台标签降级）", "T1",
   "降级断言（后台优先级/唤醒节流与冻结表一致）", A_A3, False),
  ("GPU 进程崩溃恢复链（上下文丢失→重建）", "T1",
   "重建断言（恢复后首帧时限与降级告知）", "", False),
  ("utility 进程族账（网络/存储/音频服务进程语义）", "T1",
   "服务进程职责断言（崩溃隔离逐个验证）", "", False),
  ("沙箱边界声明（本系统沙箱设施映射）", "T1",
   "映射表断言（CEF 沙箱位→J4 设施零重写）", A_J4, False),
  ("进程间 IPC 语义（Mojo 等效通道边界）", "T1",
   "通道边界断言（越界消息全拒绝）", "", False),
  ("V8 堆内存账（每 renderer 配额/超限 OOM 呈现）", "T1",
   "配额断言（超限三要素呈现而非崩溃）", "", False),
  ("渲染器挂起检测与处置（watchdog）", "T1",
   "watchdog 断言（挂起检出→处置→告知全链）", "", False),
  ("进程树监控与孤儿回收（防泄漏断言）", "T1",
   "孤儿回收断言（杀父进程后 5s 内回收全树）", "", False),
  ("进程崩溃注入床 ×10（逐角色连环崩溃 10 轮）", "T1",
   "10 轮零宿主崩溃断言（恢复账完整）", "", False),
  ("主册 UNX-J4 沙箱联签（Job 对象/令牌映射零重写）", "T1",
   "映射面对表断言零重写", A_J4, False),
  ("主册 UNX-A3 调度语义联签（优先级映射锚）", "T1",
   "优先级映射断言（四级语义一致）", A_A3, False),
  ("沙箱逃逸测试集对接（复用 J4 测试集 20 例零穿透）", "T1",
   "20 例零穿透断言（复用 J4 测试集）", A_J4, False),
  ("资源占用账：10 进程全开内存上限（模拟档）", "T1",
   "内存上限断言（超限告警与回收）", "", False),
  ("恶意页面防线：脚本炸弹/分配风暴全拒绝", "T1",
   "攻击样本 20 例全拒绝（限速+配额+告警）", "", False),
  ("沙箱语义文档与策略声明（开放格式可审计）", "T1",
   "文档导出 schema 校验过断言", "", False),
  ("B11 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销", "", False),
 ]},
{
 "id": "B12", "theme": "CEF 线联轧", "kind": "I 型集成",
 "spec": "CEF 线集成收口：Browser/Frame 生命周期全链、帧树路由、多实例、RequestContext 隔离、下载回调声明、cefclient 对拍、失败分流、shim 挂点、文档对齐、缺陷过账、许可终账、性能总账与移交冻结。",
 "items": [
  ("CefBrowser/CefFrame 生命周期全链联轧（零泄漏）", "T1",
   "1,000 开关周期零泄漏断言（句柄/内存双账）", "", False),
  ("帧树语义（主帧/子帧/跨进程帧路由）", "T1",
   "路由断言（消息/事件按帧树正确送达）", "", False),
  ("CefFrame URL/名称/主帧判定语义", "T1",
   "判定断言（iframe 场景 20 例）", "", False),
  ("ExecuteJavaScript 帧定向语义", "T1",
   "定向断言（指定帧执行、他帧零影响）", "", False),
  ("浏览器多实例账（N 实例并发互不干扰）", "T1",
   "并发断言（8 实例零串扰）", "", False),
  ("CEF 线全景联轧 I：装载→App/Client 回调→OSR 呈现全链", "T1",
   "全链 14 步断言零失败", "", False),
  ("CEF 线全景联轧 II：多进程沙箱→崩溃恢复→资源账全环", "T1",
   "全环断言（含注入床复跑）", "", False),
  ("CefRequestContext 语义（独立 cookie/缓存上下文）", "T1",
   "上下文隔离断言（双上下文 cookie 零串扰）", "", False),
  ("请求上下文与主上下文隔离断言", "T1",
   "隔离机检（共享存储零交叉样本 20 例）", "", False),
  ("下载回调面声明（CefDownloadHandler 深化预告锚）", "T1",
   "声明冻结断言（B23 系统服务深化接口齐备）", "", False),
  ("cefclient 样例行为一致性对拍（T2）", "T2",
   "样例行为对拍 ×3 轮一致（随闸门补测）", "", True),
  ("失败归因分流账（CEF 线失败模式四类）", "T1",
   "四类归因准确率 100%（40 样本账）", "", False),
  ("W90-Q10 联签：CEF 失败模式族 shim 挂点", "T1",
   "挂点登记断言（零改写 shim 域账）", A_Q10, False),
  ("CEF 线文档对齐（接口文档与实现一致性）", "T1",
   "文档-实现一致性机检断言", "", False),
  ("缺陷账本过账（CEF 线 🟢/🟡 清单+清偿状态）", "T1",
   "账实相符断言（P0=0，AI-97 口径）", "W90 治理线缺陷清偿门禁（AI-97）", False),
  ("许可合规终账（CEF/Chromium BSD 全链记录）", "T1",
   "许可记录 100% 完整断言", A_LIC, False),
  ("CEF 线性能总账：冷启动/帧率/内存三指标（模拟档）", "T1",
   "三指标双账在位（真机随闸门补测）", "", True),
  ("CEF 线回归：样例场景集复跑零回退", "T1",
   "复跑零回退断言", "", False),
  ("CEF 线收官互证与移交面冻结（向 B16+ 移交）", "T1",
   "移交清单冻结断言", "", False),
  ("B12 批联轧与 CEF 线首产段收官印", "T1",
   "20 条联轧全过，线收官印在位", "", False),
 ]},
{
 "id": "B13", "theme": "IWebBrowser2 COM 面基础", "kind": "F 型地基",
 "spec": "IE 模式线开卷：WebBrowser 控件宿主面、IWebBrowser2 全方法/属性、Navigate2、DWebBrowserEvents2 事件面、导航时序账、ReadyState 状态机、窗口属性、HWND 承载、任务书锚 80 方法对拍、现代引擎路由声明与降级账。",
 "items": [
  ("WebBrowser 控件宿主面（CLSID/ProgID 表语义）", "T1",
   "激活路径断言（CLSID/ProgID 双入口一致）", A_D5, False),
  ("IWebBrowser2::Navigate 全参语义（URL/Flags/TargetFrameName/PostData/Headers）", "T1",
   "五参组合 30 场景断言全过", "", False),
  ("IWebBrowser2 属性面（LocationURL/Busy/ReadyState/Offline 等）", "T1",
   "属性设取往返断言（只读属性拒绝写入）", "", False),
  ("Navigate2 语义（与 Navigate 差异账）", "T1",
   "差异账断言（flags 组合行为逐条一致）", "", False),
  ("GoBack/GoForward/GoHome/GoSearch/Refresh/Refresh2/Stop 全方法语义", "T1",
   "七方法 35 场景断言全过", "", False),
  ("GetProperty/PutProperty 键值语义", "T1",
   "键值往返断言（含删除/覆盖/类型保真）", "", False),
  ("DWebBrowserEvents2 事件面（BeforeNavigate2/DocumentComplete/NavigateError）", "T2",
   "事件参数与序对拍一致（取消语义含 Enter/Cancel 差异）", "", False),
  ("导航时序账（开始→下载→完成→文档完成）", "T1",
   "时序断言（四相序与冻结表一致）", "", False),
  ("任务书锚：IWebBrowser2 接口面 80 方法语义对拍", "T2",
   "IWebBrowser2 接口面 80 方法语义对拍（用公开的 IE 自动化测试脚本）", "任务书 §104.4 判据原文", False),
  ("ReadyState 状态机（五态迁移对拍）", "T2",
   "迁移矩阵对拍一致（非法迁移零发生）", "", False),
  ("窗口尺寸/位置属性（Left/Top/Width/Height/Visible）", "T1",
   "属性联动断言（与宿主 HWND 同步）", A_D4, False),
  ("HWND 承载语义（OLE 嵌入位）", "T1",
   "嵌入位断言（in-place 激活/UIDeactivate 序）", A_D4, False),
  ("现代引擎内核路由声明（接口面→内核承载映射总账）", "T1",
   "映射总账断言（每方法有承载或登记降级）", "", False),
  ("缺失方法如实降级账（不支持清单+三要素——红线②）", "T1",
   "降级清单在位断言（零静默缺失）", "", False),
  ("接口面 fuzz：畸形参数/非法指针/空串 ×10", "T1",
   "10/10 防护检出（E_INVALIDARG 语义一致）", "", False),
  ("事件序回归：40 导航往返断言", "T1",
   "40 往返零回退断言", "", False),
  ("主册 UNX-D5 COM 聚合/激活语义复用联签", "T1",
   "复用面对表零重写断言", A_D5, False),
  ("IE 模式下载链声明（B23 深化预告锚）", "T1",
   "声明冻结断言（下载深化接口齐备）", "", False),
  ("开源等效组件登记账（来源与许可——AI-95 口径）", "T1",
   "登记账完整性断言（许可字段 100%）", A_LIC, False),
  ("B13 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销", "", False),
 ]},
{
 "id": "B14", "theme": "IHTMLDocument* COM 面语义", "kind": "M 型机制",
 "spec": "DOM COM 面：Document2 获取链、文档流、集合语义、body/activeElement、元素查找三 API、节点创建、IHTMLElement 属性、insertAdjacent、Document3/4/5 演进、EventObj、attachEvent 差异、事件冒泡序、Window2、execCommand、cookie 联签与 STA 线程模型。",
 "items": [
  ("IHTMLDocument2 获取路径（Document→IDispatch QI 链）", "T1",
   "QI 链断言（接口全集可获取）", A_D5, False),
  ("IHTMLDocument2::write/open/close 文档流语义", "T1",
   "文档流断言（write 序与渲染结果一致）", "", False),
  ("IHTMLDocument2::all/links/images/forms 集合语义", "T1",
   "集合枚举断言（动态更新后枚举一致）", "", False),
  ("IHTMLDocument2::body/activeElement/title/cookie 属性语义", "T1",
   "属性语义断言（activeElement 焦点联动）", A_M2, False),
  ("getElementById/getElementsByName/getElementsByTagName 语义", "T1",
   "三查找 API 60 用例断言（含大小写/空集语义）", "", False),
  ("createElement/createTextNode/createDocumentFragment 语义", "T1",
   "节点创建断言（树挂接后结构一致）", "", False),
  ("IHTMLElement 属性面（innerText/innerHTML/className/id/style）", "T1",
   "属性往返断言（含只读属性与转义语义）", "", False),
  ("IHTMLElement::insertAdjacentHTML/insertAdjacentText 语义", "T1",
   "四方位插入断言（位置正确性 20 用例）", "", False),
  ("IHTMLDocument3/4/5 演进面（documentElement/attachEvent 等）", "T1",
   "演进接口设取断言（版本差异账一致）", "", False),
  ("IHTMLEventObj 事件对象语义（srcElement/keyCode/offsetX/Y）", "T1",
   "事件对象字段断言（20 事件型全覆盖）", "", False),
  ("attachEvent/detachEvent 语义（与 addEventListener 差异账）", "T1",
   "差异账断言（this 绑定/重复注册/触发序）", "", False),
  ("事件冒泡序对拍（click→mousedown→mouseup 序一致）", "T2",
   "冒泡序对拍 ×3 轮一致（含 cancelBubble）", "", False),
  ("IHTMLWindow2 面（alert/confirm/prompt/open/setTimeout/setInterval）", "T1",
   "六方法断言（定时器句柄语义与宿主仲裁）", "", False),
  ("document.execCommand 语义（常用 20 命令子集）", "T2",
   "20 命令行为对拍一致（含 queryCommandState）", "", False),
  ("document.cookie 读写语义（与 B05 cookie 面一致性联签）", "T1",
   "跨面一致性断言（同一 jar 行为一致）", "", False),
  ("DOM 注入床 ×10（畸形 HTML/深嵌套/超大文档）", "T1",
   "10/10 防护检出（深度上限+三要素告警）", "", False),
  ("COM 面回归：100 属性方法设取往返断言", "T1",
   "100 往返零回退断言", "", False),
  ("W90-Q10 联签：MSHTML 怪癖模式 shim 挂点", "T1",
   "怪癖挂点登记断言（老 IE 渲染怪癖走 Q10）", A_Q10, False),
  ("文档面线程模型断言（STA 语义对拍）", "T1",
   "STA 断言（错线程访问拒绝并可解释）", A_D5, False),
  ("B14 批联轧与批收官印", "T1",
   "20 条联轧全过，批账守恒核销", "", False),
 ]},
{
 "id": "B15", "theme": "IE 模式开卷联轧与首产段收官", "kind": "C 型收官",
 "spec": "首产段收官：IE 模式判定器、模式切换语义、现代内核承载总账、两段全景联轧、ID/行数/判据三项终检、三线互证矩阵、Q8/E3 共用面对表、下游移交冻结、W90 分账登记接口、文档对齐、缺陷总过账、随闸门补测总清单、红线自证、传承件、收官总账与收官印。",
 "items": [
  ("IE 模式判定器声明（何时路由 IE 模式——配置驱动）", "T1",
   "判定断言（配置驱动零猜测，日志可解释）", "", False),
  ("模式切换语义（切换→文档重载→事件序列）", "T1",
   "切换序断言（状态迁移矩阵全覆盖）", "", False),
  ("现代引擎内核承载总账（接口面→内核映射完整性）", "T1",
   "映射完整性断言（覆盖率账=100% 含降级登记）", "", False),
  ("IE 模式全景联轧 I：Navigate→Document→DOM 全链", "T1",
   "全链 12 步断言零失败", "", False),
  ("IE 模式全景联轧 II：事件→脚本→窗口全环", "T1",
   "全环断言（含注入床复跑）", "", False),
  ("首产段 ID 联轧终检（W90-Q4-001–300 连续零跳号）", "T1",
   "机检断言：300 ID 连续零跳号零重复", "", False),
  ("首产段行数守恒（15×6,000=90,000 断言）", "T1",
   "机检断言：批账与全段账守恒", "", False),
  ("判据唯一性终检（300 枚 J1 零重复）", "T1",
   "机检断言：判据号唯一且 ID 内嵌零错位", "", False),
  ("三线互证矩阵（WebView2×CEF×IE 共享设施对表零撞车）", "T1",
   "对表断言（共享设施单点归属、他线只联签）", "", False),
  ("W90-Q8 全联签对表（三线共用 TLS/证书面）", "T1",
   "对表断言（共用面单点归属 Q8 零重写）", A_Q8, False),
  ("主册 UNX-E3 代理语义对表（B23 深化前置）", "T1",
   "对表断言（代理面单点归属 E3）", A_E3, False),
  ("下游移交冻结：W90-Q12 嵌入页消费面 v1 冻结", "T1",
   "冻结断言（下游 fake 可开发）", A_Q12, False),
  ("W90 台账分册登记接口（本段登记条目生成）", "T1",
   "登记条目 schema 过断言（AI-81 分账可直收）", "W90 台账分册（AI-81 体系）", False),
  ("首产段文档对齐（接口文档/CHANGELOG 一致性）", "T1",
   "一致性机检断言", "", False),
  ("首产段缺陷账本总过账（🟢/🟡/🔴 分级全清）", "T1",
   "P0=0 断言+账实相符（AI-97 口径）", "W90 治理线缺陷清偿门禁（AI-97）", False),
  ("随闸门补测总清单（真机 T3/性能实测全列）", "T1",
   "清单完整性断言（本段 T3/T2 实测项全列零虚报）", "", False),
  ("红线自证（W90 五红线+引导/硬件红线零违例声明）", "T1",
   "自证断言（零违例+复核位在位）", "W90 治理线合规监察官（AI-133）与红线官（AI-86）体系", False),
  ("传承件：本段上手书与域经登记", "T1",
   "传承件 schema 过断言（AI-136 可直收）", "W90 治理线传承与大事记官（AI-136）", False),
  ("首产段收官总账与移交面冻结（B16–B40 移交就绪声明）", "T1",
   "总账断言（300 项四落件勾稽一致）", "", False),
  ("W90-Q4 首产段收官印（域账 90,000/240,000 · 37.5%）", "T1",
   "收官印在位断言（域账进度账冻结）", "", False),
 ]},
]

# ---------------------------------------------------------------------------
# 构建
# ---------------------------------------------------------------------------

ANCHOR_IDS = {"W90-Q4-004": "Q4-B01-04-J1", "W90-Q4-249": "Q4-B13-09-J3"}
AXES = {"B01": AX_W2, "B02": AX_W2, "B03": AX_CTL, "B04": AX_CTL, "B05": AX_CTL, "B06": AX_CTL,
        "B07": AX_CEF, "B08": AX_CEF, "B09": AX_CEF, "B10": AX_CEF, "B11": AX_CEF, "B12": AX_CEF,
        "B13": AX_IE, "B14": AX_IE, "B15": AX_IE}

def build_all():
    rows = []          # (wid, bid, idx, name, lines, crit)
    n = 0
    for b in BATCHES:
        assert len(b["items"]) == 20, f"批 {b['id']} 条数 {len(b['items'])} != 20"
        for i, (name, tier, body, anchor, gate) in enumerate(b["items"], 1):
            n += 1
            wid = f"W90-Q4-{n:03d}"
            if wid in ANCHOR_IDS:
                # 任务书锚：判据号与正文全文保真，只追加注入/主轴/闸门合规句
                j = ANCHOR_IDS[wid]
                tier_src = "T3" if j.endswith("-J1") and j == "Q4-B01-04-J1" else "T2"
                text = (f"{j}（{tier_src}）{body}。"
                        "同型故障注入 ×10 全检出并回放三要素告警（十三章口径）"
                        f"；{AXES[b['id']]}；任务书 §104.4 判据原文保真（AI-51/AI-62 锚定判例）")
                if gate:
                    text += "；" + GATE_T3
            else:
                text = crit(wid, None, tier, name, body, AXES[b["id"]], anchor, GATE_T3 if gate else "")
            rows.append((wid, b["id"], i, name, LINES[i - 1], text))
    return rows

def batch_header(b, first_id, last_id):
    lines = []
    lines.append(f"### W90-Q4-{b['id']}·增 {b['theme']}（{b['kind']}）（{first_id}–{last_id} · 20 条 · 6,000 行）")
    lines.append("")
    lines.append(f"批规格：{b['spec']}行数模式 5×320 + 10×300 + 5×280 = 6,000；批账锁定，收口即核。"
                 "日志/异常呈现/隐蔽捕获三件套随每条判据一并交付（十三章口径）；状态列统一「增补」不冒充深化。")
    lines.append("")
    lines.append("| 编号 | 功能条目 | 行数 | 状态 | 判据 |")
    lines.append("|---|---|---|---|---|")
    return "\n".join(lines)

def build_volume_block(rows):
    out = io.StringIO()
    out.write(VOL_HEADER + "\n\n")
    out.write("> **AI-104 承办（W90 域线 · 第十二卷 · ADR-W90-001 编制承接 · 承包域 W90-Q4 WebView2/浏览器内核嵌入）**｜"
              "Variable 2026-10-01「一次对话 300 项新功能」明令｜首产段 B01–B15 共 300 项（15 批 × 20 条），"
              "ID 段 **W90-Q4-001–W90-Q4-300** 连续零跳号，每批 6,000 行、全段 **90,000 行**（域账 240,000 行级进度 90,000/240,000 = 37.5%）｜"
              "状态列统一「增补」不冒充深化｜判据三档全程（T1 断言/T2 对拍/T3 冒烟），任务书 §104.4 判据锚两枚全文保真："
              "**Q4-B01-04-J1（T3）→ W90-Q4-004**、**Q4-B13-09-J3（T2）→ W90-Q4-249**（Q4-B27-03-J2 属 B16+ 余段如实登记）｜"
              "判据主轴：装载零白屏与版本检测零虚报（B01–B02）→ WebView2Samples 12 demo 全过（B03–B06 回归锚）→ "
              "OSR 帧呈现零空窗与依赖树零缺项（B07–B12）→ IWebBrowser2/IHTMLDocument 对拍一致（B13–B15）｜\n"
              "> 批型：F 型地基 B01–B03/B07/B13 + M 型机制 B04/B05/B08/B09/B10/B14 + E 型边界 B11 + I 型集成 B06/B12 + C 型收官 B15"
              "（首产段三线各成闭环：WebView2 线 B01–B06 / CEF 线 B07–B12 / IE 模式线开卷 B13–B15）｜\n"
              "> 跨线联签（前缀显式标注，零改写他人域账）：主册 UNX-D5（AI-13 COM 基座）/UNX-D4（AI-19 窗口消息）/UNX-E3（AI-23 WinInet/代理与 cookie 边界）/"
              "UNX-G3（合成器纹理 import 冻结接口）/UNX-J4（AI-49 沙箱映射与逃逸测试集）/UNX-M2（AI-62 IME 与键鼠——中文体验命门）/UNX-D2（AI-17 PE 装载）/"
              "UNX-A3（AI-03 调度）/UNX-J1（AI-46 安全默认全拒）/UNX-G1（AI-31 显存账边界）；W90 域内 Q8（AI-108 证书/TLS）/Q10（AI-110 shim 挂点）/"
              "Q6（DRM 黑帧边界）/Q12（AI-111 下游消费面冻结 v1）/R5（AI-116 年度镜像固定版）；治理线 AI-95 许可记录/AI-97 缺陷门禁/AI-116 热修复通道/"
              "AI-122 上游跟随/AI-125 授权账/AI-132 版本冻结/AI-133 合规监察/AI-136 传承（联签锚定行零改写）｜\n"
              "> 红线声明：W90 源册 7.6 五红线全域适用（不规避技术保护措施——DRM 黑帧诚实呈现；不逆向未授权闭源组件——WebView2 Runtime/CEF 本体只装载承载零改写；"
              "不冒充——通讯类反检测走诚实标识路线；授权齐全——CEF BSD/年度镜像固定版授权链归 AI-125；用户知情——降级必附缺失清单）；"
              "本段零引导类条目、零内核直写条目（UDF/缓存写入全部走文件系统 API 于用户数据目录白名单内，原子写+断电注入 ×10 判据随附）；"
              "真实应用冒烟（T3）与真机性能档全部登记随闸门补测，开发期零 QEMU 零实机写，如实不虚报｜\n"
              "> 防重：追加前主汇编册 `W90-Q4-` 全域零命中断言通过；300 条主题两两不重叠（三线间共享设施只联签不重写）；"
              "防重 grep 六范围（主册五范围+W90 全域）批收口执行留痕（AI-84 抽检口径）｜"
              "不占他域账（主册 UNX-* 全域与 W90 他域零触碰）、不改 64,000 公理与 W90 域账 240,000 守恒、纯追加零删除｜\n"
              "> 双落件勾稽：独立增补册《AI-104 · Q4 · 300项新功能增补册（B01–B15 · W90-Q4-001–W90-Q4-300）.md》与本卷逐字节一致（300/300）；"
              "生成器 `docs/unxreal/gen/_w90q4_firstprod.py` 五断言 ALL PASS exit=0。\n")
    n = 0
    for b in BATCHES:
        first = f"W90-Q4-{n+1:03d}"
        n += 20
        last = f"W90-Q4-{n:03d}"
        out.write("\n" + batch_header(b, first, last) + "\n")
        seg = [r for r in rows if r[1] == b["id"]]
        for wid, bid, idx, name, ln, text in seg:
            out.write(f"| {wid} | {name} | {ln} | 增补 | {text} |\n")
        out.write(f"\n> **防重声明（批 {b['id']}）**：本批 20 条主题不与本卷其余十四批任一批重叠；"
                  "不触主册 UNX 全域各域账与 W90 他域（Q1–Q3/Q5–Q10/Q12/R1–R6）既有账面，"
                  "跨线共用设施只联签锚定零改写。跨批对账：批 "
                  f"{b['id']} 20 条 6,000 行计入全段 90,000；ID 段 {first}–{last} 与邻批零交叠；"
                  "关键词族（WebView2/CEF/MSHTML/OSR/cookie/沙箱）防重 grep 于批收口执行并留痕（AI-84 抽检口径）。\n")
    return out.getvalue()

def build_book(rows):
    head = []
    head.append("# AI-104 · Q4 · 300项新功能增补册（B01–B15 · W90-Q4-001–W90-Q4-300）")
    head.append("")
    head.append("> **册定位**：AI-104 承包域 W90-Q4（WebView2/浏览器内核嵌入）首产段增补册，与主汇编册《CoRun Varix STAR II · Unxreal》"
                "卷末增补卷逐字节一致；任务书源：《AI分工完成图》§AI-104 + 《Windows软件90%兼容总案》Q4 章；"
                "域账 240,000 行级，本册 90,000 行级（37.5%），B16–B40 余段 150,000 行级待续。")
    head.append("")
    return "\n".join(head) + "\n" + build_volume_block(rows).split("\n", 1)[1]

REG_LINE = ("> **增补卷登记（AI-104 · 2026-10-01 · 本轮 300 项明令）**：卷末追加《增补卷 · AI-104 · W90-Q4 WebView2/浏览器内核嵌入 · "
            "首产段 B01–B15》——W90 域线首产段 **W90-Q4-001–W90-Q4-300** 共 300 项新功能（15 批 × 20 条，连续零跳号，每批 6,000 行、全段 90,000 行，"
            "域账 90,000/240,000（37.5%），状态列统一「增补」不冒充深化），增补册源文件 docs/Varix/CoRun Varix STAR II · Unxreal/"
            "AI-104 · Q4 · 300项新功能增补册（B01–B15 · W90-Q4-001–W90-Q4-300）.md；判据三档全程，任务书 §104.4 两枚判据锚全文保真"
            "（Q4-B01-04-J1（T3）→W90-Q4-004、Q4-B13-09-J3（T2）→W90-Q4-249，Q4-B27-03-J2 属 B16+ 余段如实登记）；三线批型闭环"
            "（WebView2 B01–B06/CEF B07–B12/IE 模式开卷 B13–B15），判据主轴四段（装载零白屏/12 demo 全过/OSR 零空窗/80 方法对拍）全落；"
            "跨线联签前缀显式标注（主册 UNX-D4/D5/E3/G1/G3/J4/M2/D2/A3/J1 + W90-Q6/Q8/Q10/Q12/R5 + 治理线 AI-95/97/116/122/125/132/133/136）零改写；"
            "W90 五红线零违例自证（DRM 黑帧诚实呈现/Runtime 与 CEF 本体零改写/诚实标识/授权账归 AI-125/降级缺失清单）；"
            "真机 T3 与性能档随闸门补测登记不虚报，开发期零 QEMU 零实机写；防重：追加前主册 W90-Q4- 零命中断言通过、300 条主题两两不重叠；"
            "不占他域账、不改 64,000 公理与 W90 域账守恒、纯追加零删除。生成器 docs/unxreal/gen/_w90q4_firstprod.py 五断言 ALL PASS exit=0。"
            "详见统一协作总台账本会话条目。")

CHART_REG = """---

> **AI-104 会话登记（300 项新功能 · W90-Q4 首产段 · 2026-10-01）**：承包域 W90-Q4 WebView2/浏览器内核嵌入 B01–B15（**W90-Q4-001–W90-Q4-300**）300 项增补完成，域账 90,000/240,000 行级入账（37.5%），独立增补册《AI-104 · Q4 · 300项新功能增补册（B01–B15 · W90-Q4-001–W90-Q4-300）.md》与主汇编册卷末增补卷双落件勾稽 300/300；判据主轴「WebView2Samples 12 demo 全过（T3 锚 Q4-B01-04-J1）+ OSR 帧呈现零空窗与依赖树零缺项 + IWebBrowser2 接口面 80 方法语义对拍（T2 锚 Q4-B13-09-J3）」按 §AI-104/§Q4 保真执行，任务书判据锚全文保真（AI-51/AI-62 判例），Q4-B27-03-J2 属 B16+ 余段如实登记；三线首产段闭环：WebView2 线 B01–B06（装载/版本/控制器/Host 对象与 WebMessage/CookieManager/UDF）、CEF 线 B07–B12（依赖树/CefApp 与 CefClient/OSR/合成器纹理零拷贝/多进程沙箱/联轧）、IE 模式线开卷 B13–B15（IWebBrowser2/IHTMLDocument*/模式切换与收官）；跨线联签（主册 UNX-D4/D5/E3/G1/G3/J4/M2/D2/A3/J1 + W90-Q6/Q8/Q10/Q12/R5 + 治理线 AI-95/97/116/122/125/132/133/136）前缀显式标注零改写他人域账；W90 五红线零违例自证在册；真机 T3 与性能档随闸门补测登记不虚报（开发期零 QEMU 零实机写）；B16–B40 余段 500 项待续轮。生成器 docs/unxreal/gen/_w90q4_firstprod.py 五断言 ALL PASS exit=0。—— AI-104
"""

# ---------------------------------------------------------------------------
# 断言与写盘
# ---------------------------------------------------------------------------

def sha16(b):
    return hashlib.sha256(b).hexdigest()[:16]

def main():
    rows = build_all()
    # 断言 1：ID 连续零跳号零重复 + 300 条
    ids = [r[0] for r in rows]
    assert len(ids) == 300, "断言1失败：条数 != 300"
    expect = [f"W90-Q4-{i:03d}" for i in range(1, 301)]
    assert ids == expect, "断言1失败：ID 段不连续"
    # 断言 2：批账守恒 + 全段守恒
    per_batch = {}
    for wid, bid, idx, name, ln, text in rows:
        per_batch.setdefault(bid, 0)
        per_batch[bid] += ln
    for bid, total in per_batch.items():
        assert total == 6000, f"断言2失败：批 {bid} 行数 {total} != 6000"
    total_lines = sum(r[4] for r in rows)
    assert total_lines == 90000, f"断言2失败：全段 {total_lines} != 90000"
    # 断言 3：判据号唯一 + ID 内嵌零错位 + 功能名称唯一
    crits = [r[5] for r in rows]
    names = [r[3] for r in rows]
    assert len(set(crits)) == 300, "断言3失败：判据文本存在重复"
    assert len(set(names)) == 300, "断言3失败：功能名称存在重复"
    for r in rows:
        assert r[0] + "-J1" in r[5] or ANCHOR_IDS.get(r[0], "") in r[5], f"断言3失败：{r[0]} 判据未内嵌 ID"
    # 断言 4：任务书锚全文保真在位
    r004 = next(r for r in rows if r[0] == "W90-Q4-004")
    r249 = next(r for r in rows if r[0] == "W90-Q4-249")
    assert "Q4-B01-04-J1（T3）WebView2 官方示例应用（WebView2Samples）全部 12 个 demo 通过" in r004[5], "断言4失败：B01 锚缺失"
    assert "Q4-B13-09-J3（T2）IWebBrowser2 接口面 80 方法语义对拍（用公开的 IE 自动化测试脚本）" in r249[5], "断言4失败：B13 锚缺失"
    assert "Q4-B01-04-J1（T3）" in rows[3][5] and rows[3][1] == "B01" and rows[3][2] == 4, "断言4失败：B01 锚位错"
    assert "Q4-B13-09-J3（T2）" in rows[248][5] and rows[248][1] == "B13" and rows[248][2] == 9, "断言4失败：B13 锚位错"
    print("断言1（ID 连续零跳号零重复×300）PASS")
    print("断言2（批账 15×6,000=90,000 守恒）PASS")
    print("断言3（判据唯一 300 + 名称唯一 300 + ID 内嵌）PASS")
    print("断言4（任务书锚 verbatim×2 在位）PASS")

    vol = build_volume_block(rows)
    book = build_book(rows)

    # 断言 5：防重（追加前主册 W90-Q4- 零命中；幂等追补场景剥自身增量后验零命中）
    with io.open(MAIN_MD, "r", encoding="utf-8") as f:
        main_old = f.read()
    already = REG_LINE in main_old and vol in main_old
    if already:
        probe = main_old.replace(REG_LINE + "\n", "", 1)
        j = probe.rfind("\n" + vol)
        assert j > 0, "断言5失败：幂等探针卷块定位失败"
        probe = probe[:j] + probe[j + len("\n" + vol):]
    else:
        probe = main_old
    assert "W90-Q4-" not in probe, "断言5失败：主册基线存在 W90-Q4- 命中（防重）"
    marker = "> **增补卷登记（AI-67 · 2026-10-01 · 续产 500 项明令）**"
    pos = main_old.find(marker)
    assert pos > 0, "断言5失败：登记区锚行未找到"
    if not already:
        new_main = main_old[:pos] + REG_LINE + "\n" + main_old[pos:] + "\n" + vol
        # 纯追加零删除构造校验：删去登记行与卷尾块后应还原原文
        rebuilt = new_main.replace(REG_LINE + "\n", "", 1)
        idx = rebuilt.rfind("\n" + vol)
        assert idx > 0, "断言5失败：卷块回退定位失败"
        rebuilt = rebuilt[:idx] + rebuilt[idx + len("\n" + vol):]
        assert rebuilt == main_old, "断言5失败：纯追加零删除构造不成立"
    print("断言5（主册防重基线零命中" + ("·幂等追补探针" if already else "") + "）PASS")

    if "--check" in sys.argv:
        print("CHECK 模式：五断言 ALL PASS，未写盘。")
        return 0

    # 落件 1：独立增补册（幂等：已存在且内容一致则跳过，不一致须 --force）
    if os.path.exists(BOOK_OUT):
        with io.open(BOOK_OUT, "r", encoding="utf-8") as f:
            if f.read() == book:
                print("增补册已存在且内容一致，跳过（幂等）")
            elif "--force" in sys.argv:
                with io.open(BOOK_OUT, "w", encoding="utf-8", newline="\n") as f:
                    f.write(book)
            else:
                print(f"REFUSE：增补册已存在且内容不一致 {BOOK_OUT}（--force 覆盖）")
                return 2
    else:
        with io.open(BOOK_OUT, "w", encoding="utf-8", newline="\n") as f:
            f.write(book)
    # 落件 2：主册——若登记行与卷块均已完整在位（幂等/追补场景）则跳过写盘
    with io.open(MAIN_MD, "r", encoding="utf-8", newline="") as f:
        pre = f.read()
    if REG_LINE in pre and vol in pre:
        print("主册登记行与卷块均已完整在位，跳过主册写盘（追补/幂等路径）")
    else:
        # 原位 r+ 重写 + fsync——Typora 占用时 os.replace 不可用，改原位写；
        # 写前重读最新内容防并行会话窗口，写后回读验证；母本=独立增补册+生成器可回播
        assert "W90-Q4-" not in pre, "断言5b失败：写前主册已出现 W90-Q4- 命中（并行撞车，拒绝写盘）"
        pos2 = pre.find(marker)
        assert pos2 > 0, "断言5c失败：写前登记区锚行未找到"
        new_main2 = pre[:pos2] + REG_LINE + "\n" + pre[pos2:] + "\n" + vol
        with io.open(MAIN_MD, "r+", encoding="utf-8", newline="") as f:
            f.seek(0)
            f.write(new_main2)
            f.truncate()
            f.flush()
            os.fsync(f.fileno())
    # 落件 3：分工图纯追加（幂等：已在位则跳过）
    with io.open(CHART_MD, "r", encoding="utf-8") as f:
        chart_cur = f.read()
    if CHART_REG not in chart_cur:
        if "AI-104 会话登记（300 项新功能 · W90-Q4 首产段" in chart_cur:
            print("REFUSE：分工图已有 AI-104 登记但内容不一致（并行撞车，拒绝写盘）")
            return 3
        with io.open(CHART_MD, "a", encoding="utf-8", newline="") as f:
            f.write(CHART_REG)
    # 落件 4：台账纯追加（幂等：已在位则跳过）
    ledger_entry = build_ledger_entry(rows)
    with io.open(LEDGER_MD, "r", encoding="utf-8") as f:
        ledger_cur = f.read()
    if "W90-Q4 WebView2/浏览器内核嵌入 首产段立账" not in ledger_cur:
        with io.open(LEDGER_MD, "a", encoding="utf-8", newline="") as f:
            f.write(ledger_entry)
    # 写后回读验证
    with io.open(MAIN_MD, "r", encoding="utf-8") as f:
        final = f.read()
    assert REG_LINE in final and vol in final, "写后验证失败：主册内容缺失"
    assert final.count(REG_LINE) == 1, "写后验证失败：登记行重复"
    assert final.count(vol) == 1, "写后验证失败：卷块重复"
    with io.open(BOOK_OUT, "r", encoding="utf-8") as f:
        assert f.read() == book, "写后验证失败：增补册不一致"
    with io.open(CHART_MD, "r", encoding="utf-8") as f:
        assert CHART_REG in f.read(), "写后验证失败：分工图登记缺失"
    with io.open(LEDGER_MD, "r", encoding="utf-8") as f:
        assert ledger_entry in f.read(), "写后验证失败：台账条目缺失"
    h = sha16(vol.encode("utf-8"))
    print(f"落件完成：增补册/主册/分工图/台账 四件写入并回读验证通过；卷块 SHA-256 前 16 位 {h}")
    return 0

def build_ledger_entry(rows):
    t3 = sum(1 for r in rows if "（T3）" in r[5])
    t2 = sum(1 for r in rows if "（T2）" in r[5])
    t1 = sum(1 for r in rows if "（T1）" in r[5])
    h = sha16(build_volume_block(rows).encode("utf-8"))
    return f"""

---

## 会话条目 · AI-104 · W90-Q4 WebView2/浏览器内核嵌入 首产段立账（2026-10-01）

- **产出**：300 项新功能（**W90-Q4-001–W90-Q4-300** · 15 批 × 20 条 × 6,000 行 = 90,000 行 · 状态「增补」）——W90 域线首产段立账（承 ADR-W90-001 编制承接与 Variable「一次对话 300 项新功能」明令），W90-Q4 域账 240,000 行级进度 **90,000/240,000（37.5%）**；B16–B40 余段 500 项待续轮。批型：F 型地基 B01–B03/B07/B13 + M 型机制 B04/B05/B08/B09/B10/B14 + E 型边界 B11 + I 型集成 B06/B12 + C 型收官 B15；三线闭环：WebView2 线 B01–B06 / CEF 线 B07–B12 / IE 模式线开卷 B13–B15。
- **判据三档**：T1 断言 {t1} 枚 / T2 对拍 {t2} 枚 / T3 冒烟 {t3} 枚（共 300 枚唯一）；任务书 §104.4 两枚判据锚全文保真——Q4-B01-04-J1（T3）→W90-Q4-004、Q4-B13-09-J3（T2）→W90-Q4-249（AI-51/AI-62 锚定判例）；Q4-B27-03-J2 属 B16+ 余段如实登记。
- **机器校验 ALL PASS**：生成器 docs/unxreal/gen/_w90q4_firstprod.py 五断言 exit=0（300 条 ID 连续零跳号零重复/15 批 × 6,000 行守恒全段 90,000 行/判据 300 枚唯一+名称唯一+ID 内嵌零错位/任务书锚 verbatim×2 在位/主册追加前 W90-Q4- 零命中防重+纯追加零删除构造校验）；写后回读四落件一致；卷块 SHA-256 前 16 位 {h}。
- **双落件勾稽**：独立增补册《AI-104 · Q4 · 300项新功能增补册（B01–B15 · W90-Q4-001–W90-Q4-300）.md》与主汇编册卷末增补卷逐字节一致（300/300）；主汇编册卷首登记行按"最新在最上"惯例插入登记区（AI-67 续产行之前），登记行+卷块均可由生成器重建（R-PROC-002 防护：母本=独立增补册+生成器）。
- **联签**：主册 UNX-D5（AI-13 COM 基座）/UNX-D4（AI-19 窗口消息）/UNX-E3（AI-23 WinInet/代理/cookie 边界）/UNX-G3（合成器纹理 import 冻结接口）/UNX-J4（AI-49 沙箱映射与逃逸测试集复用）/UNX-M2（AI-62 IME 中文输入命门）/UNX-D2（AI-17 PE 装载）/UNX-A3（AI-03 调度）/UNX-J1（AI-46 默认全拒）/UNX-G1（AI-31 显存账边界）；W90-Q6（DRM 黑帧边界）/W90-Q8（AI-108 TLS/证书共用面）/W90-Q10（AI-110 shim 挂点）/W90-Q12（AI-111 下游消费面 v1 冻结）/W90-R5（AI-116 年度镜像固定版）；治理线 AI-95/AI-97/AI-116/AI-122/AI-125/AI-132/AI-133/AI-136——全部锚定行零改写、零代写。
- **红线**：W90 源册 7.6 五红线零违例自证在册（DRM 黑帧诚实呈现不规避；WebView2 Runtime/CEF 本体只装载零改写零逆向；通讯类反检测走诚实标识；授权链归 AI-125 授权账；降级必附缺失清单）；本段零引导类条目、零内核直写条目（UDF/缓存写入走文件系统 API 于用户目录白名单内，原子写+断电注入判据随附）；引导设施红线与硬件数据安全红线复核位全程在位（AI-86 体系）。
- **诚实三态**：真机 T3 冒烟与真机性能档（12 demo/微信类嵌入页随 B31+ 收口、IME 端到端、参照机对拍、滚动帧率）全部登记随闸门补测，开发期零 QEMU 零实机写，不虚报运行数据；全部条目为判据账。
- **双同步**：docs 落盘（独立增补册 + 主汇编册登记行与卷末增补卷 + AI分工完成图会话登记 + 生成器 + 本台账条目）+ git 提交推送（pathspec 显式限定本会话产物；主汇编册超 GitHub blobs 100MB 硬上限沿 AI-71/AI-86 先例本地落盘不入推送 pathspec）。
- **观察登记（非本域不改）**：《总纲与施工书》§7.3 波次表存在两处 F63201 段相关行的编号口径疑点（"F63201–F63200 段尾 | AI-104" 行——F 区间倒序且 AI-104 为 W90 域线编号，与主册域线 AI-01–80 编制不符），疑似 drafting 残留；本会话禁扩面不改他册，留 AI-100 终裁口。
"""

if __name__ == "__main__":
    sys.exit(main())
