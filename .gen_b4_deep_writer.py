# -*- coding: utf-8 -*-
"""AI-09 B4 域深化册公共生成器：读骨架册取判据 verbatim 与行数，按批数据生成深化册。"""
import re, os, sys

BASE = os.path.dirname(os.path.abspath(__file__))
UNX = os.path.join(BASE, 'docs', 'unxreal')
NL = '\n'

# 批主题与累计（域账累计以批尾计）
CUM = {16: 96000, 17: 102000, 18: 108000, 19: 114000, 20: 120000,
       21: 126000, 22: 132000, 23: 138000, 24: 144000,
       25: 150000, 26: 156000, 27: 162000, 28: 168000, 29: 174000, 30: 180000}

GRAFT = {
 16: 'NVMe 1.4 固件下载/提交命令规范口径、Linux nvme 固件更新路径惯例、Windows stornvme 固件更新面，只跟随',
 17: 'NVMe 1.4 Get Log Page 规范口径、Linux nvme log 页驱动惯例、Windows stornvme 健康页消费面，只跟随',
 18: 'NVMe 1.4 Format/Sanitize 规范口径、Linux nvme format 惯例、GPT/MBR UEFI 规范（F6760 锚承接），只跟随',
 19: 'AHCI 1.3.1 规范 HBA 寄存器章、Linux ahci 驱动初始化惯例、Windows storahci HBA 面，只跟随',
 20: 'AHCI 1.3.1 规范端口寄存器章、Linux ahci 端口路径惯例、Windows storahci 端口面，只跟随',
 21: 'AHCI 1.3.1 规范 FIS 传输章、Serial ATA 3.x FIS 语义、Linux ahci 命令引擎惯例，只跟随',
 22: 'AHCI 1.3.1 NCQ 章、Serial ATA 3.x FPDMA 语义、Linux libata NCQ 惯例，只跟随',
 23: 'ATA8-ACS 命令集（identify/SMART）、SFF-8035 SMART 语义、Linux libata 透传惯例（F6850 锚承接），只跟随',
 24: 'UEFI GPT 规范 2.9、Microsoft MBR/EBR 惯例、Linux blkid/lsblk 对照口径、设备命名 Linux devtmpfs 惯例，只跟随',
 25: 'Linux 设备生命周期（device/driver/gendisk）惯例、register_blkdev 冻结接口语义、Windows 设备栈生命周期对照，只跟随',
 26: 'PCIe 热插拔规范（SLOT/HP 中断）、Linux PCIe hotplug 惯例、NVMe ns 管理语义、SATA PHY 检测惯例，只跟随',
 27: 'Linux del_gendisk/queue freeze 惯例、打开计数与引用管理惯例、Windows IRP_MJ_CLOSE 联动对照，只跟随',
 28: 'NVMe abort/reset 语义、AHCI 超时回收惯例、Linux blk-mq timeout 框架思想（每请求必有终态铁律），只跟随',
 29: 'fio 类压测方法论、Linux error injection 惯例、双轨产线纪律（QEMU/真机分账），只跟随',
 30: '本域热插拔前段自账汇总口径、任务书验收判据 2 原文、B5 断电链联签线，只跟随',
}

THEME = {
 16: '固件下载/提交/激活双模式/slot 审计/回退演练全家族——固件段 20 条收官',
 17: '错误/健康/固件槽/命令效果/特性支持日志页全族——日志页段 20 条收官',
 18: 'format/sanitize 保护判据先行（红线双检 AI-86）/特性收尾/尾段回归总账；批尾 F6760 锚归位承接（GPT 解析与设备命名语义）——NVMe 尾段（B16-B18，60 条）收官',
 19: 'GHC/CAP/PI/IS/IE 全局寄存器语义、复位序与启停状态机、legacy IDE 缺席声明——HBA 面段 20 条收官',
 20: 'PxCLB/PxFB/PxSACT 端口寄存器全族、命令表与 PRDT、ST/FRE/FR/CR 握手——端口面段 20 条收官',
 21: 'CFIS 措辞器与五类 FIS、DMA/PIO/SDB 传输、命令中止重发与超时回收、32 槽并发——命令引擎段 20 条收官',
 22: 'NCQ 32 深度全语义（SACT/FPDMA/READ LOG EXT/互斥窗）、四档 QD 压测——NCQ 段 20 条收官',
 23: 'identify/SMART READ DATA 透传与归一化表（冻结接口 3 ATA 侧交付 B5）；批内 F6850 锚归位承接（NVMe 热插拔在途 IO 回收与队列冻结）——透传段 20 条收官',
 24: 'GPT 头 CRC32/entry 双副本/类型 GUID 表/MBR 扩展分区链/三段式命名/20 样本 lsblk 对照（F6760 锚判据兑现批）——分区段 20 条收官',
 25: '四态状态机（枚举→注册→在用→remove）/设备账本/事件通知链（冻结接口 4）——四态段 20 条收官',
 26: 'hotplug 中断拾取双路/重新枚举/add-remove 语义/配对与去抖/7×24 账口径——事件链段 20 条收官',
 27: 'remove 五步总序/EBUSY 打开计数/强制删除/gendisk 注销/队列冻结/级联注销/重插恢复——remove 段 20 条收官',
 28: '命令超时回收/队列冻结终态化/错误上抛 BIO/中断丢失兜底/SQ 死锁检测/三类注入样本库 ≥30——错误路径段 20 条收官',
 29: '抽插矩阵 18 格/QEMU 模拟抽插器/真机规程/抽插 ×50 起步账/数据完整性抽样/样本库总目——注入矩阵段 20 条收官',
 30: '抽插 200 次总账（F6850 锚判据兑现）/事件差=0 7×24 总账/每请求必有终态全域复核/域账 180,000 闭合——热插拔前段（B25-B30，120 条）收官',
}

GOV = {
 16: '固件三命令（F6701/F6702/F6703）→slot 审计（F6704）→回退演练（F6718）',
 17: '日志页框架（F6726）→健康页 B5 联签（F6722/F6737）→文案（F6735）',
 18: '红线双检（F6746）→尾段回归（F6752）→全语义汇总（F6753）→锚承接（F6760）',
 19: '寄存器面（F6761/F6762）→复位序（F6763）→缺席 ADR（F6770）→联测（F6777）',
 20: '命令表三区（F6783/F6784）→握手序（F6792）→迹线联测（F6799）',
 21: '措辞器（F6801）→五类 FIS（F6802）→超时回收（F6807）→槽管理（F6810）',
 22: '排队序（F6822）→完成处理（F6823）→错误恢复（F6826）→压测（F6833）',
 23: 'identify（F6841）→SMART 通道（F6842/F6843/F6844）→B5 联签（F6854）→锚承接（F6850）',
 24: 'GPT 头（F6861）→双副本（F6863/F6864）→GUID 表（F6865）→20 样本对照（F6873）→冻结页（F6879）',
 25: '状态机（F6881）→枚举/注册（F6882/F6883）→账本/通知链（F6886/F6887）→冻结页（F6897）',
 26: '拾取双路（F6901）→事件语义（F6903/F6904）→配对账（F6905）→冻结页（F6909）',
 27: '五步总序（F6921）→EBUSY（F6922）→注销序（F6924）→消费面联测（F6938）',
 28: '回收总序（F6941）→超时语义（F6942/F6943）→终态铁律（F6945）→样本库（F6949/F6950）',
 29: '矩阵设计（F6961）→模拟器（F6962）→×50 起步（F6964）→样本总目（F6977）',
 30: '200 次总账（F6981）→事件差总账（F6982）→全域铁律（F6983）→域账闭合（F6988）',
}

def load_sk_batch(idx):
    path = os.path.join(UNX, 'batches', 'UNX-B4-B%02d.md' % idx)
    t = open(path, encoding='utf-8').read()
    out = []
    for m in re.finditer(r'### UNX-F(\d{4}) · (.+)\n- 域/批：B4/B\d+｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：(UNX-F\1-J1 .+)', t):
        out.append({'fid': m.group(1), 'name': m.group(2), 'rows': int(m.group(3)), 'judg': m.group(4)})
    assert len(out) == 20, (idx, len(out))
    return out

def split_rows(total, seg_names):
    """按三段比例拆行数（38/32/30），求和恒等于 total。"""
    a = round(total * 0.38)
    b = round(total * 0.32)
    c = total - a - b
    assert a > 0 and b > 0 and c > 0
    return a, b, c

def write_deep_batch(idx, entries, extra_head=''):
    """entries: 20 × dict(fid, loc, bound, dep, risk, body, link, win, retest, segs(3 段名))"""
    sk = load_sk_batch(idx)
    sk_map = {e['fid']: e for e in sk}
    assert len(entries) == 20
    for e in entries:
        s = sk_map[e['fid']]
        e['name'], e['rows'], e['judg'] = s['name'], s['rows'], s['judg']
        assert e['judg'].startswith('UNX-F%s-J1 ' % e['fid'])
        a, b, c = split_rows(e['rows'], e['segs'])
        e['decomp'] = '%s %d + %s %d + %s %d；测试段不计' % (e['segs'][0], a, e['segs'][1], b, e['segs'][2], c)
        body_len = len(e['body'])
        e['_body_len'] = body_len  # 诊断用，不落盘
        short_list = [x for x in entries if x.get('_body_len', 999) < 300]
        if short_list:
            raise AssertionError('短正文: %r' % short_list)

    fid0 = 6701 + (idx - 16) * 20
    cum = CUM[idx]
    lines = []
    lines.append('# 域 UNX-B4 · 深化册 · UNX-B4-B%02d（F%d–F%d · 20 条 · 20 条新深化）' % (idx, fid0, fid0 + 19))
    lines.append('')
    lines.append('> AI-09 承办｜本批 B%02d [已深化] 收口：20 条全部为本会话新深化｜嫁接源：%s｜防重声明：与总纲 §7.3-B4 骨架条目逐条同名同判据同 ID，深化不改判据语义只补六要素与正文；批累计行数锁定 6,000，域累计 %s/240,000｜本批主题：%s｜四重治理件：%s%s' % (
        idx, GRAFT[idx], format(cum, ','), THEME[idx], GOV[idx], extra_head))
    lines.append('')
    for e in entries:
        lines.append('### UNX-F%s · %s' % (e['fid'], e['name']))
        lines.append('- 域/批：B4/B%02d｜判据：%s｜纯功能行数：%d 行（%s）｜状态：[已深化]' % (idx, e['judg'], e['rows'], e['decomp']))
        lines.append('- **定位**：%s' % e['loc'])
        lines.append('- **语义边界**：%s' % e['bound'])
        lines.append('- **依赖与嫁接源**：%s' % e['dep'])
        lines.append('- **风险与回退**：%s' % e['risk'])
        lines.append('- 正文：%s。与现存内核衔接点：%s。与 Windows 对照：%s，判据 UNX-F%s-J1 的复测方式：%s。' % (e['body'], e['link'], e['win'], e['fid'], e['retest']))
        lines.append('')
    path = os.path.join(UNX, 'deepen', 'B4-B%02d.md' % idx)
    with open(path, 'w', encoding='utf-8', newline='\n') as f:
        f.write(NL.join(lines))
    total_chars = sum(len(e['body']) for e in entries)
    print('Wrote %s: 20 entries, 6000 rows, body-chars=%d' % (path, total_chars))
    return total_chars
