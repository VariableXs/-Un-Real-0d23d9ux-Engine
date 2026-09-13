// UNREAL-X-15000: AI-36/AI-37 批次（领域10 安全与隐私）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { SECURITY_FAMILY_IDS, runAi3637Checks } from '../checks';
import { VaultSystem, NetPrivacyManager, ScreenPrivacyShield } from '../groupA';
import { FilePrivacyManager, BioAuthSystem, FirewallCenter } from '../groupB';
import { PrivacyDashboard, AntiTracking, SecureChannel } from '../groupC';
import { DataSovereignty, VulnManager, AnomalyDetector } from '../groupD';
import { IntegrityMonitor, SessionIdentity, PhysicalSecurity, BackupSecurity, fnv1a } from '../groupE';
import { focusOrderValid, motionToken } from '../../security/core';

describe('UNREAL-X-15000 领域10 AI-36/AI-37 全量自检（X08851~X09250）', () => {
  it('400 项全部通过', () => {
    const { entries, failed } = runAi3637Checks();
    expect(entries.length).toBe(400);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
  it('族区间 X08851~X09250 连续无缺口', () => {
    const { entries } = runAi3637Checks();
    const nums = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    for (let i = 0; i < nums.length; i++) expect(nums[i]).toBe(8851 + i);
  });
  it('16 族注册表入口齐全', () => {
    expect(SECURITY_FAMILY_IDS.length).toBe(16);
    expect(SECURITY_FAMILY_IDS[0]).toBe('X08851');
    expect(SECURITY_FAMILY_IDS[15]).toBe('X09226');
  });
});

describe('AI-36 保险箱/网络/屏幕隐私', () => {
  it('保险箱默认拒绝与审计', () => {
    const v = new VaultSystem('strict');
    v.lock();
    expect(v.put({ id: 'a', sizeKb: 1, tags: [] }, 1)).toBe(false);
    v.unlock();
    expect(v.put({ id: 'a', sizeKb: 1, tags: [] }, 1)).toBe(true);
    expect(v.auditTrail().length).toBe(1);
  });
  it('网络隐私档位裁剪出站指纹', () => {
    expect(new NetPrivacyManager('transparent').outbound('https://x').referrer).not.toBe('');
    expect(new NetPrivacyManager('ghost').outbound('https://x').referrer).toBe('');
  });
  it('屏幕隐私降级链', () => {
    const s = new ScreenPrivacyShield('mask');
    s.degrade();
    expect(s.profileId).toBe('blur');
  });
});

describe('AI-36 文件/生物/防火墙', () => {
  it('敏感路径打码与粉碎', () => {
    const f = new FilePrivacyManager('shredded');
    f.markSensitive('/d/secret');
    expect(f.displayPath('/d/secret')).not.toBe('/d/secret');
    expect(f.shred('/d/secret')).toBe(true);
  });
  it('生物认证连续失败冷却', () => {
    const b = new BioAuthSystem('standard');
    b.enroll('u', 900);
    b.verify('u', false, 0);
    b.verify('u', false, 1);
    const r = b.verify('u', false, 2);
    expect(r.pass).toBe(false);
    expect(r.lockedFor).toBe(5000);
  });
  it('防火墙规则覆盖默认策略', () => {
    const fw = new FirewallCenter('strict');
    fw.addRule('open.test', 443, true);
    expect(fw.decide('open.test', 443, 1)).toBe('allow');
    expect(fw.decide('else.test', 443, 1)).toBe('deny');
  });
});

describe('AI-37 仪表盘/反追踪/加密通信', () => {
  it('仪表盘健康分与去重', () => {
    const d = new PrivacyDashboard('standard');
    d.record({ app: 'a', resource: 'r', at: 1, granted: false });
    d.record({ app: 'a', resource: 'r', at: 1, granted: false });
    expect(d.eventCount).toBe(1);
    expect(d.healthScore()).toBe(0);
  });
  it('URL 追踪参数清洗', () => {
    const t = new AntiTracking('standard');
    expect(t.scrubUrl('https://a/p?utm_x=1&keep=2')).toBe('https://a/p?keep=2');
  });
  it('加密通道往返', () => {
    const s = new SecureChannel('standard');
    const r = s.roundtrip('p', '机密数据 conf');
    expect(r.cipherDiffers).toBe(true);
    expect(r.restored).toBe(true);
  });
});

describe('AI-37 主权/漏洞/异常', () => {
  it('数据可携带导回', () => {
    const d = new DataSovereignty('local-first');
    const j = d.exportAll({ hello: '世界' });
    expect(d.importAll(j)).toEqual({ hello: '世界' });
  });
  it('漏洞 SLA 优先级', () => {
    const v = new VulnManager();
    v.file({ id: 'A', severity: 'low', component: 'c', fixed: false });
    v.file({ id: 'B', severity: 'critical', component: 'c', fixed: false });
    expect(v.slaQueue()[0]!.id).toBe('B');
  });
  it('异常基线偏差告警', () => {
    const a = new AnomalyDetector('standard');
    a.learnBaseline([10, 10, 10]);
    expect(a.observe(30, 1)).toBe(true);
    expect(a.observe(11, 2)).toBe(false);
  });
});

describe('AI-37 完整性/会话/物理/备份', () => {
  it('完整性摘要检出篡改', () => {
    const m = new IntegrityMonitor('verify');
    m.seal('f', 'v1');
    expect(m.verify('f', 'v1')).toBe(true);
    expect(m.verify('f', 'v2')).toBe(false);
    expect(fnv1a('x')).toBe(fnv1a('x'));
  });
  it('会话有效期与敏感操作门槛', () => {
    const s = new SessionIdentity(1000);
    s.issue(0);
    expect(s.alive(500)).toBe(true);
    expect(s.canSensitive(500)).toBe(false);
    s.stepUp();
    expect(s.canSensitive(500)).toBe(true);
    expect(s.alive(2000)).toBe(false);
  });
  it('物理篡改响应档位', () => {
    expect(new PhysicalSecurity('lock').tamper('lid', 1)).toBe('lock');
    expect(new PhysicalSecurity('notify').tamper('lid', 1)).toBe('log');
  });
  it('备份恢复点与去重', () => {
    const b = new BackupSecurity('daily');
    expect(b.backup(100, 1)).toBe(1);
    expect(b.backup(100, 1)).toBe(-1);
    expect(b.restorePoint(500)?.at).toBe(100);
  });
});

describe('领域10 共用核', () => {
  it('动效令牌 reduce-motion 降级', () => {
    expect(motionToken(false).curve).toBe('std');
    expect(motionToken(true).curve).toBe('linear');
  });
  it('焦点序校验', () => {
    expect(focusOrderValid([1, 2, 3], 3)).toBe(true);
    expect(focusOrderValid([1, 1], 3)).toBe(false);
  });
});
