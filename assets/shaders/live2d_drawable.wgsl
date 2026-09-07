// n3ri_live2d drawable fragment shader.
//
// One shader covers all drawable variants, switched by uniform flags:
//   flags.x = opacity          (Core drawable opacity)
//   flags.y = masked           (1.0 → multiply alpha by shared mask RTT)
//   flags.z = solid            (1.0 → mask shape pass, output alpha only)
//   flags.w = inverted mask    (csmIsInvertedMask: 1.0 → factor = mask alpha,
//                               0.0 → factor = 1 - alpha)
//   mult_col / scr_col         Cubism MultiplyColor / ScreenColor tints
//                              (white / black are the neutral values)
//   vp.xy  = mask RTT size in pixels
//
// Masking mirrors live2d-viewer's FBO approach, with channel packing:
//   1. Mask camera clears mask RTT to WHITE (alpha=1 → outside mask)
//   2. Mask shapes written per-lane (write_mask = R/G/B/A per group)
//   3. 打包：4 个 mask 组共享一张 RTT 的 RGBA 四通道，`u.vp.z` = 本组通道号
//   4. maskFactor = 1.0 - laneValue (white=hidden, transparent=visible)

#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct Live2dUniforms {
    flags: vec4<f32>,
    mult_col: vec4<f32>,
    scr_col: vec4<f32>,
    vp: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> u: Live2dUniforms;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var src_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var src_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var mask_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var mask_samp: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    var c = textureSample(src_tex, src_samp, mesh.uv);

    // Mask-camera pass: output mask shape alpha as the mask value.
    // Convention (matches Cubism SDK / live2d-viewer): FBO cleared to WHITE
    // (alpha=1 → outside mask → fully hidden). The pipeline `write_mask`
    // routes the blended result into this group's own lane only — same output
    // for every lane, the target mask selects R/G/B/A. Other lanes untouched,
    // so 4 groups sharing one RTT never clobber each other.
    if (u.flags.z > 0.5) {
        return vec4(0.0, 0.0, 0.0, c.a);
    }

    // Cubism color tints. Multiply neutral is white; screen neutral is black.
    let mult = c.rgb * u.mult_col.rgb;
    let scr = clamp(u.scr_col.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let screened = vec3<f32>(1.0) - (vec3<f32>(1.0) - mult) * (vec3<f32>(1.0) - scr);
    c = vec4<f32>(screened, c.a);

    var alpha: f32 = c.a * u.flags.x;

    // Mask sampling — must be done in the MASK RTT's coordinate space, i.e.
    // the MAIN camera's framebuffer pixels (mask cameras share the main
    // camera's transform, full-window RTT).
    //
    // `mesh.position` (@builtin(position)) is the CURRENT camera's framebuffer
    // pixel, which only matches the mask RTT for the main camera. The head
    // camera renders to a 256×256 RTT with a zoomed transform, so its pixels
    // map to a completely different mask region (bottom-left corner) — masked
    // drawables like the eye irises got a garbage mask factor and turned white.
    //
    // Fix: use `mesh.world_position` (interpolated world coords — identical
    // for every camera rendering the same entity). For the main camera
    // (centered, ortho scale 1) a world point lands at framebuffer pixel
    // (wx, view_h - wy): Bevy world Y is up, framebuffer/texture-v Y is down.
    // Hence mask_uv = (wx / vw, 1 - wy / vh). For the main camera this is
    // bit-identical to the old position.xy / vp.xy; for any other camera it
    // now samples the same mask value the main camera would at that point.
    if (u.flags.y > 0.5) {
        let mask_uv = vec2<f32>(
            mesh.world_position.x / u.vp.x,
            1.0 - mesh.world_position.y / u.vp.y,
        );
        let m = textureSample(mask_tex, mask_samp, mask_uv);
        // 打包 RTT：本组 mask 存在 `u.vp.z` 通道（0=R..3=A），只读自己那条。
        let lane = i32(u.vp.z + 0.5);
        var mask_alpha = m.a;
        if (lane == 0) {
            mask_alpha = m.r;
        } else if (lane == 1) {
            mask_alpha = m.g;
        } else if (lane == 2) {
            mask_alpha = m.b;
        }
        // SDK convention (csmIsInvertedMask): normal drawables are visible
        // OUTSIDE the mask shape (1 - alpha); inverted drawables INSIDE it.
        // Reference live2d-viewer: mix(1.0 - maskAlpha, maskAlpha, uInvertMask)
        let mask_factor = mix(1.0 - mask_alpha, mask_alpha, u.flags.w);
        alpha = alpha * mask_factor;
    }

    // Premultiplied alpha output — matches blend mode ONE, ONE_MINUS_SRC_ALPHA.
    // Reference live2d-viewer: FragColor = vec4(tex.rgb * tex.a, tex.a)
    return vec4<f32>(c.rgb * alpha, alpha);
}
