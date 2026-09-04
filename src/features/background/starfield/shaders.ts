/**
 * "Aurora Sky" — WebGL 2.0 pipeline, three scene layers + post:
 *
 *   Pass A (fullscreen quad → opaque): night sky — layered indigo gradient,
 *          twilight afterglow, milky-way band, comet, meteors, satellite,
 *          hero star, crepuscular god rays. Film grain, dithering.
 *   Pass B (gl.POINTS, alpha blend): poisson-disk stars with async twinkle
 *          and diffraction-cross flare stars.
 *   Pass C (fullscreen quad, PREMULTIPLIED alpha): photoreal aurora curtains
 *          with baked-texture high-frequency striations, silk fibers, ray
 *          clusters, drifting lenticular clouds (occlude aurora) and the
 *          meteor layer.
 *
 *   Post (tier-gated): bright-pass → separable gaussian blur → composite
 *   (soft-focus HDR bloom + final triangle dithering).
 */

export const QUAD_VS = `#version 300 es
precision highp float;
const vec2 P[3] = vec2[3](vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
out vec2 vUv;
void main() {
  gl_Position = vec4(P[gl_VertexID], 0.0, 1.0);
  vUv = gl_Position.xy * 0.5 + 0.5;
}`;

// ---------- shared noise GLSL (injected into scene fragments) ----------

const NOISE_GLSL = `
float hash12(vec2 p) {
  p = fract(p * vec2(233.34, 851.73));
  p += dot(p, p + 23.45);
  return fract(p.x * p.y);
}
float vnoise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash12(i), hash12(i + vec2(1, 0)), u.x),
             mix(hash12(i + vec2(0, 1)), hash12(i + vec2(1, 1)), u.x), u.y);
}
float fbm(vec2 p) {
  float v = 0.0;
  float amp = 0.55;
  for (int i = 0; i < 4; i++) {
    v += amp * vnoise(p);
    p = p * 2.13 + vec2(11.3, 7.9);
    amp *= 0.5;
  }
  return v;
}
float hash13(vec3 p) {
  p = fract(p * vec3(233.34, 851.73, 417.21));
  p += dot(p, p.zyx + 31.32);
  return fract((p.x + p.y) * p.z);
}
float vnoise3(vec3 p) {
  vec3 i = floor(p);
  vec3 f = fract(p);
  vec3 u = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(mix(hash13(i), hash13(i + vec3(1, 0, 0)), u.x),
        mix(hash13(i + vec3(0, 1, 0)), hash13(i + vec3(1, 1, 0)), u.x), u.y),
    mix(mix(hash13(i + vec3(0, 0, 1)), hash13(i + vec3(1, 0, 1)), u.x),
        mix(hash13(i + vec3(0, 1, 1)), hash13(i + vec3(1, 1, 1)), u.x), u.y),
    u.z);
}
float fbm3(vec3 p) {
  float v = 0.0;
  float amp = 0.55;
  for (int i = 0; i < 4; i++) {
    v += amp * vnoise3(p);
    p = p * 2.09 + vec3(11.3, 7.9, 5.1);
    amp *= 0.5;
  }
  return v;
}`;

// ---------- Pass A: night-sky base + comet ----------

export const SKY_FS = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform vec2 uRes;
uniform float uTime;
uniform vec2  uCam;
uniform float uDpr;
uniform sampler2D uGrain;   // baked detail texture
${NOISE_GLSL}

void main() {
  vec2 uv = vUv + uCam * uDpr / uRes;
  float t = uTime;

  // Layered night gradient: warm-tinted horizon haze —?luminous low sky —?  // open blue —?deep indigo zenith. Multi-stop, never a flat wash.
  vec3 zenith = vec3(0.055, 0.105, 0.245);  // deep indigo zenith
  vec3 upper  = vec3(0.100, 0.185, 0.390);  // upper-mid indigo blue
  vec3 azure  = vec3(0.170, 0.290, 0.520);  // rich blue
  vec3 low    = vec3(0.290, 0.410, 0.630);  // open blue
  vec3 hor   = vec3(0.490, 0.580, 0.750);  // luminous horizon haze
  vec3 col = mix(upper, zenith, smoothstep(0.62, 0.97, uv.y));
  col = mix(azure, col, smoothstep(0.40, 0.66, uv.y));
  col = mix(low, col, smoothstep(0.20, 0.44, uv.y));
  col = mix(hor, col, smoothstep(0.06, 0.24, uv.y));

  // Milky-way star cloud: a granular luminous band drifting through the
  // upper-left sky —?two noise scales over a soft gaussian spine.
  vec2 mw = uv - vec2(0.300, 0.780);
  float mwSpine = exp(-pow(dot(mw, normalize(vec2(0.85, 0.53))) * 6.5, 2.0));
  float mwGrain = 0.55 + 0.45 * fbm(uv * 9.0 + 3.0);
  mwGrain *= 0.65 + 0.35 * fbm(uv * 22.0 - 5.0);
  col += vec3(0.090, 0.120, 0.205) * mwSpine * mwGrain;

  // Twilight afterglow: salmon-pink hovering above the right ridgeline, a
  // softer lavender-rose wash on the left, plus a faint violet band between
  // the warm horizon and the blue sky —?the photo's layered dusk gradient.
  float rGlow = exp(-pow((uv.y - 0.205) * 7.5, 2.0)) * smoothstep(0.40, 0.92, uv.x);
  col = mix(col, vec3(0.925, 0.635, 0.655), rGlow * 0.55);
  float lGlow = exp(-pow((uv.y - 0.170) * 7.0, 2.0)) * smoothstep(0.38, 0.02, uv.x);
  col = mix(col, vec3(0.815, 0.600, 0.760), lGlow * 0.34);
  float duskBand = exp(-pow((uv.y - 0.26) * 6.0, 2.0)) * 0.10;
  col = mix(col, vec3(0.42, 0.34, 0.52), duskBand);

  // Nebula grain: faint baked-texture variation so the gradient is alive.
  col *= 0.988 + 0.024 * texture(uGrain, uv * 4.5 + 0.2).b;

  // The whole sky breathes (2%).
  col *= 1.0 + 0.02 * sin(t * 0.10);

  // Directional air: the left sky leans cool-violet, the right leans warm —?  // a faint horizontal hue drift that gives the atmosphere a compass.
  col *= mix(vec3(0.975, 0.965, 1.030), vec3(1.035, 0.990, 0.950), uv.x);
  // Large-scale colour noise: uneven gas in the air —?never a flat wash.
  col += vec3(0.020, 0.014, 0.030) * (fbm(uv * 3.1 + 11.0) - 0.5);

  // Comet: bright head with a long tapering tail streaming up-right,
  // shimmering gently so the upper-left keeps its living focal point.
  const vec2 COMET = vec2(0.225, 0.645);
  vec2 cd = uv - COMET;
  vec2 cdir = normalize(vec2(0.36, 0.93));
  float along = dot(cd, cdir);
  float perp = abs(dot(cd, vec2(-cdir.y, cdir.x)));
  float shimmer = 0.75 + 0.25 * sin(t * 0.9 + sin(t * 2.33) * 0.8);
  float tail = exp(-perp * 240.0) * exp(-max(along, 0.0) * 13.0)
             * smoothstep(-0.012, 0.002, along);
  float head = exp(-dot(cd, cd) * 42000.0);
  vec3 cometC = vec3(0.82, 0.90, 1.0);
  col += cometC * (head * 1.35 + tail * 0.42 * shimmer) * 0.80;
  // A slow satellite crossing the sky every ~90 s — a patient tiny light.
  float sat = fract(t / 90.0);
  vec2 sp = mix(vec2(-0.02, 0.88), vec2(1.02, 0.70), sat);
  float sd = length(uv - sp);
  col += vec3(0.80, 0.90, 1.0) * exp(-sd * sd * 2.4e6) * 0.55;

  // Hero star with a slowly breathing diffraction cross.
  vec2 hs = uv - vec2(0.072, 0.905);
  float hc = exp(-dot(hs, hs) * 260000.0);
  float crossF = max(exp(-abs(hs.x) * 900.0 - abs(hs.y) * 40.0),
                     exp(-abs(hs.y) * 900.0 - abs(hs.x) * 40.0));
  float hp = 0.80 + 0.20 * sin(t * 1.1);
  col += vec3(0.95, 0.97, 1.0) * (hc * 2.0 + crossF * 0.26) * hp;

  // Crepuscular god rays leaking through the upper-right cloud stack.
  float gap = uv.x - (0.760 + 0.012 * sin(t * 0.05)) - (0.70 - uv.y) * 0.14;
  float godray = max(0.0, 1.0 - abs(gap) / 0.045)
               * exp(-pow((uv.y - 0.46) * 2.1, 2.0));
  col += vec3(0.34, 0.44, 0.64) * godray * (0.055 + 0.030 * sin(t * 0.13));

  // Watercolor grain (subtle film texture).
  float grain = (hash12(gl_FragCoord.xy * 0.9 + fract(t) * 13.7) - 0.5) * 0.020;
  col += col * grain;

  // Soft corner vignette for focus.
  float r = distance(uv, vec2(0.5)) * 1.6;
  col *= 1.0 - min(r * r, 1.0) * 0.09;

  // Triangle dithering kills gradient banding on the big blue washes.
  float d1 = hash12(gl_FragCoord.xy + fract(t) * 17.0);
  float d2 = hash12(gl_FragCoord.yx * 1.37 + 7.31);
  col += (d1 + d2 - 1.0) * (1.3 / 255.0);
  outColor = vec4(col, 1.0);
}`;

// ---------- Pass C: translucent volumetric aurora + drifting clouds ----------
// Output is PREMULTIPLIED alpha —?the renderer blends with (ONE, 1-SRC_ALPHA).
// Composition (matching the reference photo): the main emerald dome
// launches from mid-left (x—?.30) and climbs a steep concave arc to the
// upper right (exiting near x—?.95); a salmon-red ray cluster burns on the
// left flank; teal curtains fill the right sky; a low teal glow band hugs
// the horizon above the mountains. Dark lenticular clouds composite OVER
// the aurora so they occlude it exactly as in the photograph.

export const AURORA_FS = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform vec2 uRes;
uniform float uTime;
uniform float uAurora;     // curtain count ladder 0..3
uniform float uFlow;       // silk flowing animation
uniform float uDetail;     // L10 inner fold detail
uniform sampler2D uGrain;    // baked tileable fbm — fine ray striations
uniform vec2  uCam;
uniform float uDpr;
${NOISE_GLSL}

// 3-octave fbm — the aurora pass covers the whole sky, so every octave
// counts. The soft plasmatic look loses nothing at three octaves.
float fbm3o(vec2 p) {
  float v = 0.0;
  float amp = 0.55;
  for (int i = 0; i < 3; i++) {
    v += amp * vnoise(p);
    p = p * 2.13 + vec2(11.3, 7.9);
    amp *= 0.5;
  }
  return v;
}

// Photoreal aurora ladder: emerald branch (deep teal —?saturated emerald —?// mint —?yellow-green —?near-white hot filaments, cyan cold edges,
// magenta-pink fringes) and the pink branch for the rose curtains (deep
// rose —?magenta —?pink —?warm pale). Many hues weaving together, like the
// reference's candy-band sky.
vec3 auroraRamp(float e, float alt, float cyanAmt, float pinkAmt, float salmon) {
  vec3 teal  = vec3(0x0e, 0x8a, 0x7a) / 255.0; // #0E8A7A deep teal
  vec3 green = vec3(0x2a, 0xd9, 0x8a) / 255.0; // #2AD98A saturated emerald
  vec3 mint  = vec3(0x7c, 0xf0, 0xb4) / 255.0; // #7CF0B4 mint
  vec3 ylg   = vec3(0xc8, 0xf0, 0xa8) / 255.0; // #C8F0A8 yellow-green
  vec3 hot   = vec3(0xf0, 0xfc, 0xf4) / 255.0; // near-white hot
  vec3 cyan  = vec3(0x35, 0xd8, 0xe8) / 255.0; // #35D8E8 cyan cold edge
  vec3 pink  = vec3(0xf0, 0xa8, 0xe8) / 255.0; // #F0A8E8 magenta-pink fringe
  vec3 c1 = mix(teal, green, smoothstep(0.02, 0.30, e));
  c1 = mix(c1, cyan, smoothstep(0.24, 0.55, e) * (0.20 + 0.40 * cyanAmt));
  c1 = mix(c1, mint, smoothstep(0.42, 0.70, e));
  c1 = mix(c1, ylg, smoothstep(0.66, 0.88, e));
  c1 = mix(c1, hot, smoothstep(0.86, 0.99, e));
  c1 = mix(c1, pink, clamp(pinkAmt * smoothstep(0.04, 0.20, alt)
        * (0.50 + 0.50 * (1.0 - smoothstep(0.55, 0.85, e))), 0.0, 0.88));
  // Warm yellow-green breath at the curtain's base —?low-altitude oxygen
  // glow, like the reference's lime roots.
  c1 = mix(c1, vec3(0.80, 0.92, 0.55),
           clamp((0.09 - alt) * 9.0, 0.0, 1.0) * 0.30 * smoothstep(0.15, 0.35, e));
  vec3 rose   = vec3(0x8a, 0x28, 0x58) / 255.0; // #8A2858 deep rose
  vec3 mag    = vec3(0xd8, 0x48, 0x90) / 255.0; // #D84890 magenta
  vec3 salm   = vec3(0xff, 0x8e, 0xc0) / 255.0; // #FF8EC0 pink
  vec3 paleH  = vec3(0xff, 0xd8, 0xe8) / 255.0; // #FFD8E8 warm pale hot
  vec3 c2 = mix(rose, mag, smoothstep(0.02, 0.32, e));
  c2 = mix(c2, salm, smoothstep(0.30, 0.62, e));
  c2 = mix(c2, paleH, smoothstep(0.80, 0.98, e));
  c2 = mix(c2, pink, clamp(pinkAmt * smoothstep(0.04, 0.20, alt) * 0.55, 0.0, 0.80));
  return mix(c1, c2, clamp(salmon, 0.0, 1.0));
}

// Sweeping celestial arc —?parametrized per curtain via shape = (base,
// height, power): y = base + height·x^power, plus slow macro breathing
// (~20 s / ~11 s) and a tiny fbm ridge so no two curtains wobble alike.
float arcPath(float x, float t, float seed, float flow, vec3 shape) {
  float xs = clamp(x, 0.0, 1.0);
  float y = shape.x + shape.y * pow(xs, shape.z);
  y += 0.022 * sin(xs * 2.7 + seed * 5.1 + t * 0.31 * flow);
  y += 0.010 * sin(xs * 5.1 - seed * 2.3 - t * 0.57 * flow);
  y += (fbm3(vec3(xs * 1.8, seed * 7.3, t * 0.028)) - 0.5) * 0.024;
  return y;
}

// Analytic slope of the parabola + sway (the slow fbm ridge contributes
// almost nothing —?skipping it keeps this branch cheap for ray fanning).
float arcSlope(float x, float t, float seed, float flow, vec3 shape) {
  float xs = max(clamp(x, 0.0, 1.0), 1e-3);
  float s = shape.y * shape.z * pow(xs, shape.z - 1.0);
  s += 0.0594 * cos(xs * 2.7 + seed * 5.1 + t * 0.31 * flow);
  s += 0.0510 * cos(xs * 5.1 - seed * 2.3 - t * 0.57 * flow);
  return s;
}

// One aurora curtain: bottom-tension, top-fanning, open toward deep space.
// The BOTTOM edge is a hair-thin white-hot field-line arc wrapped in a soft
// halo; above it, vertically-stretched ray shafts lean off the arc normal
// and diverge with height, dissolving with a purely exponential atmospheric
// falloff —?no upper boundary anywhere. xw windows the curtain horizontally
// so each cluster occupies its own stretch of sky, like the photograph.
// Returns premultiplied color.
vec4 curtain(vec2 uv, float t, float seed, float flow, float detail,
             vec3 shape, float lift, float alphaIn, float cyanAmt,
             float pinkAmt, float salmon, vec2 xw) {
  float win = smoothstep(xw.x, xw.x + 0.10, uv.x) * (1.0 - smoothstep(xw.y - 0.10, xw.y, uv.x));
  if (win < 0.003) return vec4(0.0);
  float d = uv.y - arcPath(uv.x, t, seed, flow, shape) - lift;
  if (d < -0.28 || d > 1.05) return vec4(0.0); // coherent early-out, no seam
  float ad = abs(d);
  float above = max(d, 0.0);

  // Luminous bottom field-line + white-blue bloom hugging both sides.
  float line = exp(-ad * 120.0);
  float halo = exp(-ad * 16.0) * exp(-max(-d, 0.0) * 3.2);

  // Per-column curtain height: some shafts tower, others vanish (organic).
  float fall = mix(2.2, 4.8, fbm3o(vec2(uv.x * 3.1 + seed * 4.7, seed * 2.9)));
  // Rays fan out: the sampling coordinate leans off the arc normal and
  // diverges with height, plus a noise-driven spread per column.
  float slope = arcSlope(uv.x, t, seed, flow, shape);
  float fanN = fbm3o(vec2(uv.x * 5.0 + seed * 13.0, seed * 1.7)) - 0.5;
  float sx = uv.x - above * (slope * 0.34 + fanN * 0.16);

  // Vertically-stretched ray field: high x-frequency, LOW y-frequency.
  float n = fbm3o(vec2(sx * 16.0 + seed * 9.0 + t * 0.06 * flow, above * 2.6 - t * 0.015));
  float fil = pow(clamp(1.0 - abs(n * 2.0 - 1.0), 0.0, 1.0), 3.2);
  // Interleaved pink blades ride the same noise so pink rays are coherent
  // full blades, not speckle.
  float rayPink = smoothstep(0.60, 0.80, n) * smoothstep(0.02, 0.10, above);
  float pinkEff = clamp(pinkAmt * (0.35 + rayPink * 1.6), 0.0, 1.25);
  // The hot border breathes with the ray bases: brighter at each blade's
  // foot, dimmer between blades —?never a uniform streak.
  line *= 0.62 + 0.50 * fil;
  // Ray contrast is height-graded: hugging the line the curtain is a
  // continuous luminous band, distinct blades with dark gaps open above.
  float rayMix = smoothstep(0.04, 0.36, above);
  float ray = mix(0.45, 0.06, rayMix) + mix(0.60, 1.90, rayMix) * fil;
  // Curtain folds: broad bright/dim patches travelling along the arc give
  // the curtain its "breathing" patches of light (the photo's uneven glow).
  float seg = 0.72 + 0.42 * sin(sx * 2.4 + seed * 7.0 + t * 0.11 * flow)
            * sin(sx * 5.7 - seed * 3.0 + t * 0.05);
  ray *= seg;
  // Silk fibers: a cheap mid-frequency modulation all tiers can afford.
  float n3 = vnoise(vec2(sx * 44.0 + t * 0.05 * flow, above * 3.0 - seed));
  ray *= 0.80 + 0.40 * n3;
  // Baked-texture ray striations: fine vertical filaments — the anti-plastic
  // detail layer that makes each ray read as a distinct shaft of light.
  float stri = texture(uGrain, vec2(sx * 110.0 + seed * 13.0, above * 3.5 - t * 0.02)).r;
  stri = pow(clamp(1.0 - abs(stri * 2.0 - 1.0), 0.0, 1.0), 1.6);
  ray *= 0.66 + 0.68 * stri;
  // Silk micro-fibres from a second texture octave.
  float silk = texture(uGrain, vec2(sx * 58.0 + t * 0.035, above * 8.0 - seed)).g;
  ray *= 0.84 + 0.32 * silk;
  // Fast 1.5 s pulse —?high-energy particles colliding with the atmosphere.
  ray *= 1.0 + 0.22 * sin(t * 4.19 + n * 27.0 + seed * 8.0);
  // Mid 6 s folds travelling along the arc: brightness + width breathe.
  ray *= 0.72 + 0.34 * sin(1.0472 * t - sx * 5.5 + seed * 9.4);
  if (detail > 0.5) {
    // L10 counter-drifting micro-folds double the silk texture.
    float n2 = fbm3o(vec2(sx * 30.0 - t * 0.14 + seed * 3.0, above * 4.5));
    ray *= 0.84 + 0.32 * pow(clamp(1.0 - abs(n2 * 2.0 - 1.0), 0.0, 1.0), 2.0);
  }

  // Bottom-tension envelope: tall photoreal curtains —?exponential
  // thinning with a high feathering band, wisps dying far up.
  float env = exp(-above * fall);
  env *= 1.0 - 0.30 * smoothstep(0.16, 0.62, above);
  env *= 1.0 - 0.45 * smoothstep(0.42, 0.95, above);
  // Bottom-tension: energy piles up gently right over the field-line.
  env *= 1.0 + 0.26 * exp(-above * 7.0);
  float body = env * ray * smoothstep(-0.010, 0.012, d);

  // Wide ambient glow cage: the curtain lights up the sky far around it —?  // this is what makes the aurora feel like it ENVELOPS the night.
  float cage = exp(-ad * 4.5) * 0.085 * alphaIn;
  float e = body * 0.92 + halo * (0.16 + 0.14 * body) + line * 1.45 + cage;
  e = 1.0 - exp(-1.6 * e); // soft HDR shoulder —?no flat white blocks
  vec3 c = auroraRamp(e, above, cyanAmt, pinkEff, salmon);
  float a = clamp(e * alphaIn, 0.0, 0.60) * win;
  // Fine alpha grain: kills the plastic smoothness of the body.
  a *= 0.90 + 0.20 * texture(uGrain, vec2(uv.x * 64.0 + seed * 9.0, uv.y * 38.0)).b;
  return vec4(c * a, a); // premultiplied
}

// Floating cosmic light dust: tiny sparks hugging the luminous line and
// riding up inside the curtain, drifting in slow pseudo-Brownian motion
// and fading in/out over their life cycle.
vec4 lightDust(vec2 uv, float t, float seed, float flow, vec3 shape,
               float violet, float pink, vec2 xw) {
  float win = smoothstep(xw.x, xw.x + 0.10, uv.x) * (1.0 - smoothstep(xw.y - 0.10, xw.y, uv.x));
  if (win < 0.003) return vec4(0.0);
  float cx = arcPath(uv.x, t, seed, flow, shape);
  float above = uv.y - cx;
  float near = exp(-abs(above) * 6.0) + exp(-max(above, 0.0) * 3.0) * 0.30 * step(0.0, above);
  vec4 acc = vec4(0.0);
  for (int k = 0; k < 2; k++) {
    float fk = float(k);
    vec2 g = uv * vec2(54.0, 32.0) + fk * 17.3 + seed * 4.0;
    vec2 cell = floor(g);
    vec2 f = fract(g);
    float h = hash12(cell + fk * 7.77);
    vec2 pos = vec2(hash12(cell + 3.1), hash12(cell + 9.4));
    // Brownian wander: layered noise fields push each mote around slowly.
    pos += 0.30 * vec2(fbm3o(cell * 0.37 + t * 0.055 * flow),
                       fbm3o(cell * 0.37 + 5.0 - t * 0.045 * flow)) - 0.15;
    float dist = length(f - pos);
    // Life cycle: gentle fade-in —?hold —?fade-out, ~14-28 s, desynced.
    float life = fract(t * (0.036 + 0.036 * h) * flow + h * 7.0);
    float pulse = sin(life * 6.2831853);
    float bright = smoothstep(0.0, 0.30, pulse) * smoothstep(1.0, 0.70, pulse);
    float mote = smoothstep(0.13, 0.0, dist) * bright;
    vec3 c = mix(vec3(0.85, 0.98, 0.92), vec3(0.72, 0.95, 1.0), step(0.5, h));
    c = mix(c, vec3(0.98, 0.85, 0.95), step(0.85, h) * (0.35 + 0.65 * pink));
    acc += vec4(c * mote, mote) * 0.22;
  }
  return acc * near * win;
}

// One soft lenticular cloud mass: elongated fbm-textured ellipse that
// shears organically and sways almost imperceptibly.
float cloudBlob(vec2 uv, vec2 c, vec2 r, float t, float seed) {
  c.x += 0.010 * sin(t * 0.012 + seed * 2.4);   // slow breathing drift
  vec2 q = (uv - c) / r;
  q.x -= 0.22 * (fbm3o(vec2(uv.y * 2.1 + seed * 7.0, seed * 2.3 + t * 0.004)) - 0.5);
  q.y *= 1.35;
  float d = length(q);
  float n = fbm3o(vec2(uv.x * 5.0 + seed * 11.0 + t * 0.005, uv.y * 9.0 - seed * 3.0));
  return clamp(smoothstep(1.15, 0.25, d) * (0.15 + 0.85 * n), 0.0, 1.0);
}

// Dark navy cloud field, composited OVER the aurora (and the stars, since
// this pass draws after them): heavy stacked lenses on the upper right,
// thin wisps up top, all with a warm pink under-light near the horizon.
vec4 cloudLayer(vec2 uv, float t) {
  float a = 0.0;
  // Upper-right lens stack (the reference's main cloud bank).
  a = max(a, cloudBlob(uv, vec2(0.735, 0.615), vec2(0.150, 0.075), t, 1.0) * 1.10);
  a = max(a, cloudBlob(uv, vec2(0.878, 0.502), vec2(0.120, 0.080), t, 2.0) * 1.05);
  a = max(a, cloudBlob(uv, vec2(0.652, 0.432), vec2(0.095, 0.052), t, 3.0) * 0.90);
  a = max(a, cloudBlob(uv, vec2(0.958, 0.342), vec2(0.105, 0.065), t, 4.0) * 1.00);
  // Mountain-waist clouds wrapping the big volcano's flanks.
  a = max(a, cloudBlob(uv, vec2(0.615, 0.300), vec2(0.140, 0.026), t, 9.0) * 0.85);
  a = max(a, cloudBlob(uv, vec2(0.875, 0.235), vec2(0.115, 0.024), t, 10.0) * 0.80);
  // A wisp over the left peak's shoulder.
  a = max(a, cloudBlob(uv, vec2(0.085, 0.560), vec2(0.085, 0.030), t, 11.0) * 0.70);
  // High cirrus filaments, barely-there streaks up top.
  a = max(a, cloudBlob(uv, vec2(0.350, 0.930), vec2(0.150, 0.011), t, 12.0) * 0.45);
  a = max(a, cloudBlob(uv, vec2(0.610, 0.880), vec2(0.120, 0.010), t, 13.0) * 0.40);
  // Low cumulus hugging the right horizon.
  a = max(a, cloudBlob(uv, vec2(0.945, 0.205), vec2(0.080, 0.028), t, 14.0) * 0.75);
  a = max(a, cloudBlob(uv, vec2(0.155, 0.930), vec2(0.160, 0.019), t, 6.0) * 0.50);
  a = max(a, cloudBlob(uv, vec2(0.470, 0.948), vec2(0.110, 0.016), t, 7.0) * 0.45);
  a = max(a, cloudBlob(uv, vec2(0.030, 0.760), vec2(0.105, 0.021), t, 8.0) * 0.40);
  vec3 col = vec3(0.038, 0.058, 0.150);              // dark navy mass
  float low = smoothstep(0.52, 0.24, uv.y);           // near-horizon clouds
  col = mix(col, vec3(0.58, 0.34, 0.42), low * 0.55); // catch pink light
  // Haze-tinted waist clouds blend into the mountain air.
  float waist = smoothstep(0.36, 0.20, uv.y);
  col = mix(col, vec3(0.16, 0.24, 0.38), waist * 0.45);
  // Aurora-stained edges: clouds passing under the emerald dome catch its
  // green light on their rims (the reference's glowing cloud flanks).
  float arcY = 0.36 + 0.66 * pow(clamp(uv.x, 0.0, 1.0), 1.70);
  float prox = exp(-abs(uv.y - (arcY + 0.16)) * 5.0);
  col = mix(col, vec3(0.10, 0.32, 0.24), prox * 0.38);
  a = clamp(a, 0.0, 0.72);
  return vec4(col * a, a);
}

// Shooting stars: two independent meteor streams on long random periods,
// each a bright head with a fading trail streaking down the sky. Active
// only a fraction of their cycle so they land as "surprise" moments.
vec4 meteorLayer(vec2 uv, float t) {
  vec4 acc = vec4(0.0);
  for (int k = 0; k < 2; k++) {
    float seed = k == 0 ? 0.37 : 0.81;
    float period = 11.0 + 7.0 * fract(seed * 7.31);
    float ph = fract((t + seed * 29.3) / period);
    // Visible only during the first ~18% of the cycle.
    float act = smoothstep(0.0, 0.05, ph) * (1.0 - smoothstep(0.12, 0.18, ph));
    if (act <= 0.0) continue;
    vec2 a = vec2(0.12 + 0.74 * fract(seed * 3.17), 0.98 - 0.25 * fract(seed * 5.71));
    vec2 b = a + vec2(0.30 * (fract(seed * 9.13) > 0.5 ? 1.0 : -1.0),
                      -0.26 - 0.18 * fract(seed * 11.7));
    float u = clamp(ph / 0.18, 0.0, 1.0);
    vec2 head = mix(a, b, u);
    vec2 d = uv - head;
    vec2 dir = normalize(b - a);
    float along = dot(d, dir);
    float perp = abs(dot(d, vec2(-dir.y, dir.x)));
    float trail = exp(-perp * 260.0) * exp(-max(-along, 0.0) * 26.0)
                * exp(-max(along, 0.0) * 200.0);
    float headG = exp(-dot(d, d) * 52000.0);
    float fade = act * (0.45 + 0.55 * u);
    float al = clamp(headG * 1.4 + trail * 0.42, 0.0, 1.0) * fade;
    vec3 c = vec3(0.90, 0.95, 1.0) * headG * 1.5 + vec3(0.72, 0.83, 1.0) * trail * 0.5;
    acc = vec4(c * al + acc.rgb * (1.0 - al), al + acc.a * (1.0 - al));
  }
  return acc;
}

void main() {
  vec2 uv = vUv + uCam * uDpr / uRes;
  float t = uTime;
  vec4 acc = vec4(0.0);
  if (uAurora > 0.5) {
    float flow = uFlow > 0.5 ? 1.0 : 0.12;
    // Global activity surge: the whole aurora breathes on a ~33 s cycle.
    float surge = 0.90 + 0.10 * sin(t * 0.19 + 1.3);
    // Grand emerald dome filling the center-right sky, exiting the top.
    vec3 domeShape = vec3(0.36, 0.66, 1.70);
    acc += curtain(uv, t, 0.00, flow, uDetail, domeShape, 0.000, 0.50 * surge, 0.18, 0.45, 0.0, vec2(0.30, 1.02));
    acc += lightDust(uv, t, 0.00, flow, domeShape, 0.0, 0.55, vec2(0.30, 1.02));
    if (uAurora > 1.5) {
      // Green ray cluster crowding the upper-left corner.
      acc += curtain(uv, t * 0.95, 0.23, flow * 0.90, uDetail,
                     vec3(0.42, 0.52, 1.30), 0.000, 0.34, 0.60, 0.10, 0.0, vec2(0.00, 0.30));
      // Rose-pink curtain rising from behind the left peak —?the photo's
      // magenta flames.
      acc += curtain(uv, t * 1.04, 0.47, flow * 1.05, uDetail,
                     vec3(0.32, 0.56, 1.40), 0.000, 0.58, 0.05, 0.70, 1.0, vec2(0.03, 0.38));
      // Teal sister dome on the right.
      acc += curtain(uv, t * 0.93, 0.37, flow * 0.85, uDetail,
                     vec3(0.46, 0.42, 1.30), 0.040, 0.28, 1.0, 0.12, 0.0, vec2(0.66, 1.02));
      acc += lightDust(uv, t * 1.06, 0.37, flow * 0.85, vec3(0.46, 0.42, 1.30), 1.0, 0.0, vec2(0.66, 1.02));
    }
    if (uAurora > 2.5) {
      // High magenta fringe band over the dome.
      acc += curtain(uv, t * 1.12, 0.71, flow * 1.15, uDetail, domeShape, 0.150, 0.16, 0.30, 1.0, 0.0, vec2(0.34, 0.92));
      // Magenta wisp veil drifting over the upper-left.
      acc += curtain(uv, t * 0.86, 0.29, flow * 0.60, uDetail,
                     vec3(0.50, 0.40, 1.20), 0.330, 0.10, 1.0, 0.35, 0.90, vec2(0.10, 0.48));
      // Far teal veil on the right sky.
      acc += curtain(uv, t * 0.80, 0.59, flow * 0.75, uDetail,
                     vec3(0.46, 0.42, 1.30), 0.220, 0.10, 1.0, 0.20, 0.0, vec2(0.66, 1.02));
    }
  }
  // Soft-clamp the aurora energy, composite the meteor showers, then the
  // clouds (clouds sit in front of aurora AND meteors).
  float a = min(acc.a, 0.30) + max(acc.a - 0.30, 0.0) * 0.26;
  float k = a / max(acc.a, 1e-4);
  vec4 au = vec4(acc.rgb * k, a);
  vec4 mt = uAurora > 0.5 ? meteorLayer(uv, t) : vec4(0.0);
  au = vec4(mt.rgb + au.rgb * (1.0 - mt.a), mt.a + au.a * (1.0 - mt.a));
  vec4 cl = cloudLayer(uv, t);
  vec3 rgb = cl.rgb + au.rgb * (1.0 - cl.a);
  float af = cl.a + au.a * (1.0 - cl.a);
  outColor = vec4(rgb, af);
}`;

// Aurora composite: full-res sky+stars under the half-res aurora layer
// (premultiplied over). Linear upscaling doubles as free soft blur — the
// aurora is plasmatic light, so the half-res render is visually lossless.
export const AURCOMP_FS = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uScene;
uniform sampler2D uAur;
void main() {
  vec3 c = texture(uScene, vUv).rgb;
  vec4 a = texture(uAur, vUv);
  outColor = vec4(a.rgb + c * (1.0 - a.a), 1.0);
}`;

export const STAR_VS = `#version 300 es
precision highp float;
layout(location = 0) in vec2 aPos;      // world px inside the span box
layout(location = 1) in float aSize;    // base size px
layout(location = 2) in float aDepth;   // 0.1 far .. 1.0 near
layout(location = 3) in vec2 aAnim;     // phase [0,2π), freq [0.5,2.0] Hz

uniform vec2  uRes;
uniform float uDpr;
uniform float uTime;
uniform vec2  uCam;
uniform float uSpanW;
uniform float uSpanH;
uniform float uMargin;
uniform float uGlobalPulse;  // L2
uniform float uTwoLayer;     // L3
uniform float uIndepTwinkle; // L4
uniform float uBokeh;        // L7 zoom velocity magnitude
uniform float uMotion;       // dynamic strength 0..1

out vec3 vColor;
out float vAlpha;
out float vFlare;   // 1 —?brightest elite star (subtle diffraction cross)
out float vBright;  // live brightness 0..1
out float vCoreR;   // core radius relative to sprite half-size

float hash1(float n) { return fract(sin(n) * 43758.5453123); }

void main() {
  float depth = aDepth;
  if (uTwoLayer > 0.5) depth = depth < 0.45 ? 0.2 : 1.0;

  bool nearStar = aSize > 1.9;
  bool midStar = aSize > 1.05 && !nearStar;

  // Z-layer parallax: dust 0.1 · main sequence 0.4 · near 1.0.
  float pf = nearStar ? 1.0 : midStar ? 0.4 : 0.1;
  vec2 eff = aPos - uCam * pf;

  // Infinite-field wrap so roaming never runs off the star field.
  vec2 span = vec2(uSpanW, uSpanH);
  vec2 sp = mod(eff + uMargin, span) - uMargin;
  vec2 screenCss = sp;
  gl_Position = vec4((screenCss * uDpr) / uRes * 2.0 - 1.0, 0.0, 1.0);

  // Independent async twinkle: every star breathes on its own phase and
  // frequency (dual incommensurate sines) —?real atmospheric scintillation.
  float wave;
  if (uGlobalPulse > 0.5) {
    wave = sin(uTime * 0.15 * 6.2831853);
  } else if (uIndepTwinkle > 0.5) {
    float s1 = sin(uTime * aAnim.y * 6.2831853 + aAnim.x);
    float s2 = sin(uTime * aAnim.y * 2.6180339 + aAnim.x * 1.7);
    wave = mix(s1, s2, 0.5);
  } else {
    wave = 0.0;
  }
  float tw = 0.78 + 0.22 * wave * (0.30 + 0.70 * uMotion);

  // Optical star colors: crisp moon-white with cold blue / faint violet /
  // cherry-pink accents —?never warm.
  float h = hash1(aAnim.x * 13.37 + aAnim.y * 71.7);
  vec3 white = vec3(1.0, 1.0, 1.0);
  vec3 iceW  = vec3(0.894, 0.941, 0.992); // #e4f0fd faint ice white
  vec3 pale  = vec3(0.847, 0.910, 0.980); // #d8e8fa pale blue
  vec3 lblue = vec3(0.690, 0.804, 0.941); // #b0cdf0 light blue
  vec3 lav   = vec3(0.874, 0.816, 0.980); // #dfd0fa soft violet dust
  vec3 pink  = vec3(0.976, 0.878, 0.933); // #f9e0ee pale cherry dust
  vec3 accent= vec3(0.561, 0.706, 0.871); // #8fb4de deep light-blue accent
  vec3 colr = h < 0.38 ? white : h < 0.62 ? iceW : h < 0.76 ? pale
            : h < 0.86 ? lblue : h < 0.92 ? lav : h < 0.96 ? pink : accent;
  colr *= nearStar ? 1.0 : midStar ? 0.95 : 0.92;
  colr *= 0.97 + 0.05 * sin(uTime * 3.0 + aAnim.x * 5.0);

  // Vignette falloff at the corners (gentle —?the sky is bright now).
  vec2 uvN = screenCss * uDpr / uRes;
  float dmax = distance(vec2(0.5), vec2(sqrt(0.5)));
  float dc = distance(uvN, vec2(0.5)) / dmax;
  float vig = 1.0 - min(dc * dc, 1.0) * 0.14;

  float baseA = nearStar ? 1.0 : midStar ? 0.95 : 0.85;
  float magHash = hash1(aAnim.y * 91.3 + aAnim.x * 17.7);
  float mag = 0.58 + 0.42 * pow(magHash, 1.7);
  float bright = clamp(baseA * tw * mag, 0.18, 1.0);
  bool elite = nearStar && uIndepTwinkle > 0.5 && magHash > 0.945;
  vAlpha = bright * vig;
  vColor = colr;
  vFlare = elite ? 1.0 : 0.0;
  vBright = bright;

  float glowBreath = nearStar ? 1.0 + 0.16 * sin(uTime * aAnim.y * 3.14 + aAnim.x) : 1.0;
  float bokehSpread = nearStar ? 1.0 + min(abs(uBokeh) * 0.9, 1.6) : 1.0;
  float px = aSize * glowBreath * bokehSpread;
  float halo = elite ? px * 10.0 : px * 3.2;
  gl_PointSize = max(halo * uDpr, 1.25);
  vCoreR = px / max(halo, 1e-3);
}`;

export const STAR_FS = `#version 300 es
precision highp float;
in vec3 vColor;
in float vAlpha;
in float vFlare;
in float vBright;
in float vCoreR;
out vec4 outColor;
uniform float uSquare;  // L3/L4 raw pixel points
uniform float uFlareOn; // L6+ subtle diffraction cross on elite stars

float sdCross(vec2 p, float thin) {
  float ax = exp(-abs(p.x) * 26.0 * thin) * exp(-abs(p.y) * 3.2);
  float ay = exp(-abs(p.y) * 26.0 * thin) * exp(-abs(p.x) * 3.2);
  return max(ax, ay);
}

void main() {
  vec2 p = gl_PointCoord * 2.0 - 1.0;
  float d = length(p);
  // Crisp white-hot core with tight falloff + soft optical halo.
  float core = smoothstep(vCoreR * 1.25, vCoreR * 0.30, d);
  vec3 col = mix(vec3(1.0), vColor, (1.0 - core) * 0.85);
  float a = core;

  if (uSquare > 0.5) {
    a = step(max(abs(p.x), abs(p.y)) / max(vCoreR, 1e-3), 1.0);
  } else {
    // Soft-focus halo: faint icy airy disk around the white core.
    a += 0.28 * smoothstep(1.0, vCoreR * 0.8, d) * core;
    if (vFlare > 0.5) a += 0.12 * exp(-d * 2.1);
  }

  if (uFlareOn > 0.5 && vFlare > 0.5) {
    // Realistic diffraction: a hair-thin 4-point diamond cross, breathing
    // with brightness. Cold blue along one axis, faint cherry pink on the
    // other —?spectral dispersion, never warm amber.
    float thin = mix(1.75, 0.85, clamp(vBright, 0.0, 1.0));
    float f = sdCross(p, thin);
    float axisMix = clamp(abs(p.x) - abs(p.y) + 0.5, 0.0, 1.0);
    vec3 disp = mix(vec3(0.62, 0.76, 1.0), vec3(1.0, 0.82, 0.91), axisMix);
    a = max(a, f * (0.55 + 0.75 * vBright) * 0.9);
    col = mix(col, disp, f * 0.32);
  }

  if (a <= 0.004) discard;
  outColor = vec4(col, clamp(a, 0.0, 1.0) * vAlpha);
}`;

// ---------- post-process: screen-space soft-focus bloom ----------

export const BRIGHT_VS = `#version 300 es
precision highp float;
out vec2 vUv;
void main() {
  vec2 pos = vec2((gl_VertexID << 1) & 2, gl_VertexID & 2);
  vUv = pos;
  gl_Position = vec4(pos * 2.0 - 1.0, 0.0, 1.0);
}`;

export const BRIGHT_FS = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uScene;
uniform float uThreshold; // bright-pass threshold (dark sky —?cores only)
void main() {
  vec3 c = texture(uScene, vUv).rgb;
  float l = max(c.r, max(c.g, c.b));
  float knee = 0.10;
  float soft = clamp((l - uThreshold + knee) / (2.0 * knee), 0.0, 1.0);
  float w = max(soft * soft * (uThreshold + knee * soft - l) / max(l, 1e-4),
                (l - uThreshold) / max(l, 1e-4));
  outColor = vec4(c * clamp(w, 0.0, 1.0), 1.0);
}`;

export const BLUR_FS = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uTex;
uniform vec2 uDir; // texel-scaled blur direction
void main() {
  // 9-tap gaussian, sigma —?2.2 texels at quarter resolution.
  vec3 c = texture(uTex, vUv).rgb * 0.227027;
  c += (texture(uTex, vUv + uDir * 1.3846).rgb
      + texture(uTex, vUv - uDir * 1.3846).rgb) * 0.316216;
  c += (texture(uTex, vUv + uDir * 3.2308).rgb
      + texture(uTex, vUv - uDir * 3.2308).rgb) * 0.070270;
  outColor = vec4(c, 1.0);
}`;

export const COMPOSITE_FS = `#version 300 es
precision highp float;
in vec2 vUv;
out vec4 outColor;
uniform sampler2D uScene;
uniform sampler2D uBloom;
uniform float uIntensity;
float hash12(vec2 p) {
  p = fract(p * vec2(233.34, 851.73));
  p += dot(p, p + 23.45);
  return fract(p.x * p.y);
}
void main() {
  vec3 scene = texture(uScene, vUv).rgb;
  vec3 bloom = texture(uBloom, vUv).rgb;
  vec3 col = scene + bloom * uIntensity;
  // Unified night grade: a whisper of cool blue-violet ties every element
  // into one palette (the reference's single-mood colour harmony).
  col *= vec3(0.985, 0.995, 1.025);
  // Final triangle dither (—?.5 %, two noises summed —?triangular PDF): the
  // bloom add re-introduces banding risk on the big cold glow washes.
  float d1 = hash12(gl_FragCoord.xy);
  float d2 = hash12(gl_FragCoord.xy * 1.71 + 4.7);
  col += (d1 + d2 - 1.0) * (1.3 / 255.0);
  outColor = vec4(col, 1.0);
}`;
