/* Strip the landscape pass from shaders.ts + renderer.ts.
 * Keeps: sky, stars, aurora, clouds, meteors. Removes: LANDSCAPE_FS and all
 * renderer wiring for it. Rewires the detail texture into the aurora pass
 * for high-frequency striations (anti-plastic clarity). */
const fs = require("fs");

// ---- shaders.ts: remove LANDSCAPE_FS, enhance AURORA with detail texture ----
{
  const f = "src/features/background/starfield/shaders.ts";
  let s = fs.readFileSync(f, "utf8");
  const start = s.indexOf("// ---------- Pass D: landscape");
  const end = s.indexOf("// ---------- post-process: screen-space soft-focus bloom ----------");
  if (start < 0 || end < 0 || end <= start) {
    console.log("LANDSCAPE block not found", start, end);
    process.exit(1);
  }
  s = s.slice(0, start) + s.slice(end);

  // Header doc rewrite
  s = s.replace(
    /\/\*\*?\n \* "Aurora Lake Night"[\s\S]*?\*\/\n/,
`/**
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
`)
    ;

  // Add grain sampler + striations to the aurora shader
  const auroraAnchor = "uniform float uDetail;     // L10 inner fold detail";
  if (!s.includes(auroraAnchor)) { console.log("aurora anchor missing"); process.exit(1); }
  s = s.replace(
    auroraAnchor,
    auroraAnchor + "\nuniform sampler2D uGrain;    // baked tileable fbm — fine ray striations"
  );

  // High-frequency vertical striations + silk fibers + alpha grain in curtain()
  const rayAnchor = "  float n3 = vnoise(vec2(sx * 44.0 + t * 0.05 * flow, above * 3.0 - seed));\n  ray *= 0.80 + 0.40 * n3;";
  if (!s.includes(rayAnchor)) { console.log("ray anchor missing"); process.exit(1); }
  s = s.replace(
    rayAnchor,
    rayAnchor +
`
  // Baked-texture ray striations: fine vertical filaments — the anti-plastic
  // detail layer that makes each ray read as a distinct shaft of light.
  float stri = texture(uGrain, vec2(sx * 110.0 + seed * 13.0, above * 3.5 - t * 0.02)).r;
  stri = pow(clamp(1.0 - abs(stri * 2.0 - 1.0), 0.0, 1.0), 1.6);
  ray *= 0.66 + 0.68 * stri;
  // Silk micro-fibres from a second texture octave.
  float silk = texture(uGrain, vec2(sx * 58.0 + t * 0.035, above * 8.0 - seed)).g;
  ray *= 0.84 + 0.32 * silk;`
  );

  // Alpha grain kills the smooth "plastic" banding of the curtain body
  const alphaAnchor = "  float a = clamp(e * alphaIn, 0.0, 0.60) * win;\n  return vec4(c * a, a); // premultiplied";
  if (!s.includes(alphaAnchor)) { console.log("alpha anchor missing"); process.exit(1); }
  s = s.replace(
    alphaAnchor,
    "  float a = clamp(e * alphaIn, 0.0, 0.60) * win;\n" +
    "  // Fine alpha grain: kills the plastic smoothness of the body.\n" +
    "  a *= 0.90 + 0.20 * texture(uGrain, vec2(uv.x * 64.0 + seed * 9.0, uv.y * 38.0)).b;\n" +
    "  return vec4(c * a, a); // premultiplied"
  );

  // Subtle nebula texture in the sky pass so the gradient is never flat
  const skyAnchor = "  // The whole sky breathes (2%).";
  if (!s.includes(skyAnchor)) { console.log("sky anchor missing"); process.exit(1); }
  s = s.replace(
    skyAnchor,
    "  // Nebula grain: faint baked-texture variation so the gradient is alive.\n" +
    "  col *= 0.988 + 0.024 * texture(uGrain, uv * 4.5 + 0.2).b;\n\n" +
    skyAnchor
  );
  // Sky needs the sampler declared
  const skyUniforms = "uniform vec2  uCam;\nuniform float uDpr;\n";
  const skyIdx = s.indexOf(SKY_MARKER());

  function SKY_MARKER() { return "__SKY_NOOP__"; }
  // insert uGrain into SKY_FS uniforms (first occurrence after SKY_FS start)
  const skyStart = s.indexOf("export const SKY_FS");
  const camIdx = s.indexOf("uniform vec2  uCam;", skyStart);
  const dprIdx = s.indexOf("uniform float uDpr;", camIdx);
  if (dprIdx < 0) { console.log("sky uniforms missing"); process.exit(1); }
  const insertAt = s.indexOf("\n", dprIdx) + 1;
  s = s.slice(0, insertAt) + "uniform sampler2D uGrain;   // baked detail texture\n" + s.slice(insertAt);

  fs.writeFileSync(f, s);
  console.log("shaders.ts stripped + aurora/sky detail wired");
}

// ---- renderer.ts: remove landscape pass wiring ----
{
  const f = "src/features/background/starfield/renderer.ts";
  let s = fs.readFileSync(f, "utf8");

  // import list: drop LANDSCAPE_FS
  s = s.replace(
    /import \{\n  AURORA_FS, BLUR_FS, BRIGHT_FS, BRIGHT_VS, COMPOSITE_FS, LANDSCAPE_FS, QUAD_VS, SKY_FS, STAR_FS, STAR_VS,\n\} from "\.\/shaders";/,
    'import {\n  AURORA_FS, BLUR_FS, BRIGHT_FS, BRIGHT_VS, COMPOSITE_FS, QUAD_VS, SKY_FS, STAR_FS, STAR_VS,\n} from "./shaders";'
  );

  // drop landProg link
  s = s.replace("  const landProg = link(gl, QUAD_VS, LANDSCAPE_FS);\n", "");
  s = s.replace(
    /if \(!skyProg \|\| !auroraProg \|\| !starProg \|\| !landProg \|\| !brightProg \|\| !blurProg \|\| !compProg\) return null;/,
    "if (!skyProg || !auroraProg || !starProg || !brightProg || !blurProg || !compProg) return null;"
  );

  // drop landscape uniform lookups
  const lUniforms = /  const lScene = gl\.getUniformLocation\(landProg, "uScene"\);\n  const lDetail = gl\.getUniformLocation\(landProg, "uDetail"\);\n  const lRes = gl\.getUniformLocation\(landProg, "uRes"\);\n  const lTime = gl\.getUniformLocation\(landProg, "uTime"\);\n  const lCam = gl\.getUniformLocation\(landProg, "uCam"\);\n  const lDpr = gl\.getUniformLocation\(landProg, "uDpr"\);\n  const lLod = gl\.getUniformLocation\(landProg, "uLod"\);\n/;
  s = s.replace(lUniforms, "");
  // add aurora grain uniform lookup after aDpr
  s = s.replace(
    '  const aDpr = gl.getUniformLocation(auroraProg, "uDpr");',
    '  const aDpr = gl.getUniformLocation(auroraProg, "uDpr");\n  const aGrain = gl.getUniformLocation(auroraProg, "uGrain");'
  );

  // drop landT target management
  s = s.replace(/  let landT: Target \| null = null;\n/, "");
  s = s.replace(/    destroyTarget\(landT\);\n/g, "");
  s = s.replace(/    landT = needLand \? makeTarget\(w, h, true\) : null;\n/, "");
  s = s.replace(/      if \(needLand && !landT\) landT = makeTarget\(w, h, true\);\n/, "");
  s = s.replace(/      else if \(!needLand && landT\) \{\n        destroyTarget\(landT\);\n        landT = null;\n      \}\n/, "");
  s = s.replace(/    landT = makeTarget\(w, h, true\);\n/g, "");
  s = s.replace(/      sceneT = landT = bloomA = bloomB = null;/, "      sceneT = bloomA = bloomB = null;");

  // drawScene: bind grain texture during the aurora pass
  s = s.replace(
    '    // Pass C — translucent aurora curtains, PREMULTIPLIED alpha over stars.',
`    // Pass C — translucent aurora curtains, PREMULTIPLIED alpha over stars.
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, detailTex);
    gl.uniform1i(aGrain, 1);`
  );
  s = s.replace(
    "    gl.drawArrays(gl.TRIANGLES, 0, 3);\n    gl.disable(gl.BLEND);\n  }",
`    gl.drawArrays(gl.TRIANGLES, 0, 3);
    gl.activeTexture(gl.TEXTURE1);
    gl.bindTexture(gl.TEXTURE_2D, null);
    gl.disable(gl.BLEND);
  }`
  );

  // drawLandscape function removal
  const dlStart = s.indexOf("  /** Pass D — terrain, village and mirror lake composited over the sky. */");
  const dlEnd = s.indexOf("  return {", dlStart);
  if (dlStart < 0 || dlEnd < 0) { console.log("drawLandscape not found"); process.exit(1); }
  s = s.slice(0, dlStart) + s.slice(dlEnd);

  // draw() pipeline rewrite: scene straight to screen on no-bloom tiers
  const drawStart = s.indexOf("    draw(f: GpuFrame): void {");
  const drawEnd = s.indexOf("    dispose(): void {", drawStart);
  if (drawStart < 0 || drawEnd < 0) { console.log("draw() not found"); process.exit(1); }
  s = s.slice(0, drawStart) +
`    draw(f: GpuFrame): void {
      if (disposed || starCount === 0) return;
      const W = gl.drawingBufferWidth;
      const H = gl.drawingBufferHeight;

      if (f.tier.bloom) ensureTargets(W, H);

      if (f.tier.bloom && sceneT && bloomA && bloomB) {
        // Bloom chain: sky scene → FBO, bright-pass at ¼ res, separable blur,
        // then composite back to the default framebuffer.
        gl.bindFramebuffer(gl.FRAMEBUFFER, sceneT.fbo);
        drawScene(f);

        gl.bindFramebuffer(gl.FRAMEBUFFER, bloomA.fbo);
        gl.viewport(0, 0, bloomA.w, bloomA.h);
        gl.useProgram(brightProg);
        gl.bindVertexArray(emptyVao);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, sceneT.tex);
        gl.uniform1i(bScene, 0);
        gl.uniform1f(bThresh, BLOOM_THRESHOLD);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

        blur(bloomA, bloomB, 1.75, 0);
        blur(bloomB, bloomA, 0, 1.75);

        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        gl.viewport(0, 0, W, H);
        gl.useProgram(compProg);
        gl.bindVertexArray(emptyVao);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, sceneT.tex);
        gl.uniform1i(cScene, 0);
        gl.activeTexture(gl.TEXTURE1);
        gl.bindTexture(gl.TEXTURE_2D, bloomA.tex);
        gl.uniform1i(cBloom, 1);
        gl.uniform1f(cInt, BLOOM_INTENSITY);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, null);
      } else {
        // No-bloom tiers render straight to the screen.
        gl.bindFramebuffer(gl.FRAMEBUFFER, null);
        drawScene(f);
      }
    },

` + s.slice(drawEnd);

  // ensureTargets signature back to two args (drop needLand)
  s = s.replace(/    if \(sceneT && sceneT\.w === w && sceneT\.h === h\) \{\n      \/\/ Tier moved across the bloom gate[\s\S]*?\n    \}\n    return;\n  \}/,
`    if (sceneT && sceneT.w === w && sceneT.h === h) return;
  }`);
  s = s.replace(/  function ensureTargets\(w: number, h: number, needLand: boolean\): void \{/,
    "  function ensureTargets(w: number, h: number): void {");
  s = s.replace(/      ensureTargets\(W, H, !!f\.tier\.bloom\);/, "      ensureTargets(W, H);");

  fs.writeFileSync(f, s);
  console.log("renderer.ts simplified");
}
