/**
 * 七章 · 三轨交织极光着色器（3-Spline Interwoven Aurora）。
 * 独立成模块以便 tools/aurora-canvas-preview.html 无 Tauri 依赖地预览调优。
 *
 * 三条相位交错的极光带（A 主帘幕 / B 缠绕副带 / C 前景晶莹光丝）共用一条
 * 自左下蜿蜒攀升至屏幕中间偏上 (x≈0.66, y≈0.76) 的 S 型骨架，沿 t∈[0,1]：
 *   宽度包络 W(t)=Wmax·(1-t^1.4)      —— 起点宽阔漫延，端点纤细如针尖
 *   消散包络 A(t)=Amax·smoothstep(1,0.2,t) —— 抵达中间偏上区域完全水彩化消散
 * 所有边缘均为高斯/指数羽化 + FBM 侵蚀，无任何硬裁切；辉光由预乘 alpha
 * （emit 可略高于 cov）与大半径柔焦晕项共同等效 18~24px Bloom。
 */

export const VERT = `#version 300 es
layout(location=0) in vec2 aPos;
out vec2 vUv;
void main() {
  vUv = aPos * 0.5 + 0.5;
  gl_Position = vec4(aPos, 0.0, 1.0);
}`;

export const FRAG_AURORA = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform float uTime;
uniform vec2 uCam;

float hash(vec3 p) {
  p = fract(p * 0.3183099 + vec3(0.71, 0.113, 0.419));
  p *= 17.0;
  return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}
float noise(vec3 x) {
  vec3 i = floor(x); vec3 f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(mix(hash(i), hash(i + vec3(1,0,0)), f.x),
                 mix(hash(i + vec3(0,1,0)), hash(i + vec3(1,1,0)), f.x), f.y),
             mix(mix(hash(i + vec3(0,0,1)), hash(i + vec3(1,0,1)), f.x),
                 mix(hash(i + vec3(0,1,1)), hash(i + vec3(1,1,1)), f.x), f.y), f.z);
}
float fbm(vec3 p) {
  float v = 0.0; float a = 0.5;
  for (int i = 0; i < 3; i++) { v += a * noise(p); p *= 2.13; a *= 0.5; }
  return v;
}

// 三轨公共 S 型骨架：自左下 (0.02, 0.16) 蜿蜒攀升至屏幕中间偏上 (0.66, 0.76)
float spine(float t, float seed) {
  float y = mix(0.16, 0.76, t * t * (3.0 - 2.0 * t));
  y += 0.052 * sin(t * 4.6 + uTime * 0.349 + seed);        // 宏观 18s 绸缎蛇形摆动
  y += 0.026 * sin(t * 9.3 - uTime * 0.62 + seed * 2.7);   // 中频缠绕
  y += 0.011 * sin(t * 19.0 + uTime * 1.10 + seed * 5.1);  // 微观涟漪
  return y;
}

// 冷调日系水彩级联：#F0F6FF 白蓝核心 → #38BDF8 电光冰青 → #3B82F6 宝蓝 → #8B5CF6 冰紫罗兰
vec3 auroraTint(float u) {
  u = clamp(u, 0.0, 1.0);
  vec3 c = mix(vec3(0.941, 0.965, 1.000), vec3(0.220, 0.741, 0.973), smoothstep(0.02, 0.42, u));
  c = mix(c, vec3(0.231, 0.510, 0.965), smoothstep(0.38, 0.72, u));
  c = mix(c, vec3(0.545, 0.361, 0.965), smoothstep(0.66, 1.0, u));
  return c;
}

void main() {
  // 相机视差：极光层按近景随画布漂移
  vec2 p = vec2(vUv.x - uCam.x * 0.010, vUv.y + uCam.y * 0.006);

  float t = clamp(p.x / 0.68, 0.0, 1.0);             // t=1.0 即屏幕中间偏上端点
  // 消散包络：左缘 8% 柔和浮现，抵达端点前完全水彩化消散（无任何硬切边）
  float fade = smoothstep(0.0, 0.08, t) * (1.0 - smoothstep(0.25, 1.0, t));
  // 宽度包络 W(t)=Wmax·(1-t^1.4)
  float wEnv = 1.0 - pow(t, 1.4);
  // 微观呼吸 1.5s：晶莹脉动
  float pulse = 0.92 + 0.08 * sin(uTime * 4.19);
  // 帘幕褶皱光柱：纵向拉伸，沿轨迹 6s 顺滑流动；顶端羽化由高斯带自行承担
  float rays = pow(fbm(vec3(p.x * 7.5 - t * 2.4 + uTime * 1.05, t * 2.6, 3.71)), 2.4);

  vec3 emit = vec3(0.0);
  float cov = 0.0;

  // ---- Ribbon A（主帘幕）----
  float yA = spine(t, 0.0);
  float dA = p.y - yA;
  float wA = max(0.020, 0.085 * wEnv);

  // Layer 1 深层水彩晕染：4.5 倍宽度的超高斯冷青蓝底衬（≈10%）
  float haze = exp(-dA * dA / (wA * wA) * 0.10);
  emit += vec3(0.180, 0.400, 0.700) * (haze * fade * 0.16);
  cov  += haze * fade * 0.13;

  // Layer 2 中层主帘幕：FBM 侵蚀 × 褶皱光柱；核心高斯 + 3.3 倍大半径柔焦晕双重羽化
  float nA = fbm(vec3(p.x * 3.2, dA * 2.6 + t * 3.0, uTime * 0.07));
  float bandA = exp(-dA * dA / (wA * wA) * 1.35) * (0.50 + 0.80 * nA) * (0.45 + 1.05 * rays);
  float haloA = exp(-dA * dA / (wA * wA) * 0.09);
  float uA = 0.5 + dA / (wA * 1.5);
  // 樱花冰粉 #F8D4E4 仅在底缘噪声条纹处零星掠过（严格 <3% 屏占比）
  float pinkA = smoothstep(0.62, 0.90, fbm(vec3(p.x * 4.5, 7.3, uTime * 0.05)))
              * (1.0 - smoothstep(-0.30, 0.10, uA)) * 0.22;
  vec3 tintA = mix(auroraTint(uA), vec3(0.973, 0.831, 0.894), pinkA);
  emit += tintA * (bandA * 0.34 + haloA * 0.055) * fade * pulse;
  cov  += (bandA * 0.30 + haloA * 0.045) * fade * pulse;

  // ---- Ribbon B（缠绕副带）：异频正弦围绕 A 柔和缠绕、交替穿插，先行渐隐 (x≈0.58) ----
  float yB = spine(t, 2.13) - 0.015 + 0.085 * sin(t * 6.4 - uTime * 0.47 + 1.7) * wEnv;
  float dB = p.y - yB;
  float wB = max(0.012, 0.052 * wEnv);
  float nB = fbm(vec3(p.x * 4.1 + 9.0, dB * 2.2 + t * 4.0, uTime * 0.09));
  float bandB = exp(-dB * dB / (wB * wB) * 1.40) * (0.45 + 0.85 * nB) * (0.50 + 0.90 * rays);
  float haloB = exp(-dB * dB / (wB * wB) * 0.11);
  float fadeB = 1.0 - smoothstep(0.62, 0.86, t);
  float uB = 0.5 + dB / (wB * 1.5);
  emit += auroraTint(uB) * (bandB * 0.26 + haloB * 0.05) * fade * fadeB;
  cov  += (bandB * 0.22 + haloB * 0.04) * fade * fadeB;

  // ---- Layer 3 前景晶莹光丝 C：极纤细冰白光丝悬于主帘幕上方，灵动延伸、率先消散 ----
  float yC = spine(t, 4.7) + 0.048 + 0.018 * sin(t * 9.0 + uTime * 0.85);
  float dC = p.y - yC;
  float wC = max(0.0045, 0.016 * wEnv);
  float bandC = exp(-dC * dC / (wC * wC) * 1.10);
  float haloC = exp(-dC * dC / (wC * wC) * 0.07);
  float fadeC = 1.0 - smoothstep(0.55, 0.92, t);
  emit += mix(vec3(0.941, 0.965, 1.000), vec3(0.220, 0.741, 0.973), 0.30)
        * (bandC * 0.50 + haloC * 0.06) * fade * fadeC * pulse;
  cov  += (bandC * 0.42 + haloC * 0.05) * fade * fadeC * pulse;

  // 十二.4 冰晶粒子：极罕见的一颗樱粉晕冰蓝核，划过即逝
  float bucket = floor(uTime / 210.0);
  float hr = hash(vec3(bucket, 7.7, 3.1));
  vec2 sparkPos = vec2(0.15 + hr * 0.5, 0.30 + hash(vec3(bucket, 1.3, 9.2)) * 0.30);
  float life = fract(uTime / 210.0) * 210.0;
  vec2 sd = vec2(p.x - sparkPos.x - life * 0.012, p.y - sparkPos.y - life * 0.008) * 60.0;
  float spark = exp(-dot(sd, sd)) * exp(-life * 2.2);
  emit += vec3(0.95, 0.87, 0.93) * spark * 0.5;
  cov  += spark * 0.4;

  // HDR 软色调映射：高亮核心向外“揉匀散开”，亮度硬封顶
  emit = emit / (1.0 + emit * 0.5);
  // 预乘 alpha 输出：emit 高于 cov 的部分即柔焦 Bloom 辉光
  outColor = vec4(emit, clamp(cov, 0.0, 1.0));
}`;
