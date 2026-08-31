// n3ri_os 桌面背景：深海青绿雾 + 近垂直宽光柱 + 蓝白萤火上浮 + 近摄 bokeh + 底部 3D 透视网格地板
#import bevy_ui::ui_vertex_output::UiVertexOutput

struct Config {
    mouse_pos: vec2<f32>,
    time: f32,
    zoom: f32,
    offset: vec2<f32>,
};

@group(1) @binding(0) var<uniform> config: Config;
@group(1) @binding(1) var water_normal_tex: texture_2d<f32>;
@group(1) @binding(2) var water_normal_smp: sampler;
@group(1) @binding(3) var noise_tex: texture_2d<f32>;
@group(1) @binding(4) var noise_smp: sampler;

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2(123.34, 456.21));
    q = q + dot(q, q + 45.32);
    return fract(q.x * q.y);
}

// 上浮光点：锐利白色小点，消散时转蓝；返回每粒子着色后的 vec3
fn glow_layer(auv: vec2<f32>, t: f32, density: vec2<f32>, seed_off: f32, scale: f32, core_k: f32, halo_k: f32) -> vec3<f32> {
    let cell = floor(auv * density);
    var sum = vec3(0.0);
    for (var dx: i32 = -1; dx <= 1; dx = dx + 1) {
        for (var dy: i32 = -1; dy <= 1; dy = dy + 1) {
            let c = cell + vec2(f32(dx), f32(dy));
            let seed = hash21(c + vec2(seed_off, seed_off * 1.7));
            let speed = 0.05 + seed * 0.07;
            let life_t = fract(t * speed + seed * 9.0);
            let sway = sin(t * (0.6 + seed * 1.4) + seed * 25.0) * 0.16;
            let x_pos = c.x + 0.5 + sway;
            let y_pos = c.y + 1.1 - life_t * 1.2;
            let center = vec2(x_pos / density.x, y_pos / density.y);
            let d = distance(auv, center);
            let death = smoothstep(0.55, 1.0, life_t);
            let core = exp(-d * d * core_k * (1.0 + death * 1.5)) * 2.0;
            let halo = exp(-d * d * halo_k * (1.0 + death)) * 0.06;
            let fade_in = smoothstep(0.0, 0.25, life_t);
            let fade_out = 1.0 - smoothstep(0.72, 1.0, life_t);
            let tw = sin(t * (1.2 + seed * 3.0) + seed * 40.0) * sin(t * (2.3 + seed * 1.7) + seed * 17.0);
            let bright = 0.60 + 0.55 * max(tw, 0.0) * max(tw, 0.0);
            let pcol = mix(vec3(1.0), vec3(0.45, 0.72, 1.0), death);
            sum = sum + (vec3(1.0) * halo + pcol * core * 1.6) * fade_in * fade_out * bright;
        }
    }
    return sum * scale;
}

// 近摄 bokeh 层：稀疏大颗虚化光斑，半径随种子变化；大光斑上升慢
fn bokeh_layer(auv: vec2<f32>, t: f32, density: vec2<f32>, seed_off: f32, scale: f32) -> vec3<f32> {
    let cell = floor(auv * density);
    var sum = vec3(0.0);
    for (var dx: i32 = -1; dx <= 1; dx = dx + 1) {
        for (var dy: i32 = -1; dy <= 1; dy = dy + 1) {
            let c = cell + vec2(f32(dx), f32(dy));
            let seed = hash21(c + vec2(seed_off, seed_off * 1.7));
            let speed = 0.08 - seed * 0.04;
            let life_t = fract(t * speed + seed * 9.0);
            let sway = sin(t * (0.4 + seed * 0.8) + seed * 25.0) * 0.2;
            let x_pos = c.x + 0.5 + sway;
            let y_pos = c.y + 1.1 - life_t * 1.2;
            let center = vec2(x_pos / density.x, y_pos / density.y);
            let d = distance(auv, center);
            let k = 12000.0 - 5000.0 * seed;
            let death = smoothstep(0.55, 1.0, life_t);
            let disc = exp(-d * d * k * (1.0 + death * 1.5)) * 2.2;
            let ring = exp(-d * d * k * 0.38 * (1.0 + death)) * 0.06;
            let fade_in = smoothstep(0.0, 0.25, life_t);
            let fade_out = 1.0 - smoothstep(0.72, 1.0, life_t);
            let tw = sin(t * (1.0 + seed * 2.5) + seed * 40.0) * sin(t * (1.9 + seed * 1.5) + seed * 17.0);
            let bright = 0.55 + 0.60 * max(tw, 0.0) * max(tw, 0.0);
            let pcol = mix(vec3(1.0), vec3(0.45, 0.72, 1.0), death);
            sum = sum + (vec3(1.0) * ring + pcol * disc) * fade_in * fade_out * bright;
        }
    }
    return sum * scale;
}

// 光柱（单束）：极光式飘带——沿高度双频波纹弯曲、独立宽度/亮度/落地距离/色相
fn beam_column3(
    t: f32, uvx: f32, uv_y: f32, horizon: f32, sway: f32, ripple: f32, phase: f32,
    center: f32, core_w: f32, skirt_w: f32, amp: f32, curve: f32, land: f32, tint: vec3<f32>,
) -> vec3<f32> {
    let h = horizon - uv_y;
    let ribbon = ripple * (sin(h * 4.5 + phase + t * 0.22) * 0.6 + sin(h * 8.0 - phase * 1.3 - t * 0.15) * 0.4);
    let bx = uvx + sway * h + curve * h * abs(h) + ribbon;
    let dx = bx - center;
    let core = exp(-dx * dx / (core_w * core_w)) * 0.75;
    let skirt = exp(-dx * dx / (skirt_w * skirt_w));
    let env_wall = 1.0 - smoothstep(0.40, 0.617, uv_y);
    let env_floor = smoothstep(horizon - 0.005, horizon + 0.03, uv_y) * (1.0 - smoothstep(land - 0.05, land, uv_y));
    let env = max(env_wall, env_floor * 0.65);
    return (core + skirt) * amp * env * tint;
}

// 移动点光源：局部亮斑
fn point_light(p: vec2<f32>, pos: vec2<f32>, k: f32) -> f32 {
    let d = distance(p, pos);
    return exp(-d * d * k);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let t = config.time;
    // 推镜变换：以屏幕中心为缩放锚点，offset 平移采样 UV
    // （offset.x 负值 → 采样坐标左移 → 内容右移；正值 → 内容左移）
    let uv = (in.uv - 0.5) / max(config.zoom, 0.01) + 0.5 + config.offset;
    let aspect = in.size.x / max(in.size.y, 1.0);
    let auv = vec2(uv.x * aspect, uv.y);

    // 视差偏移：鼠标位置 [-1,1] × 深度系数
    let parallax_bg = config.mouse_pos * 0.01;       // 背景：几乎不动
    let parallax_mid = config.mouse_pos * 0.03;      // 中层：水波、光束
    let parallax_fg = config.mouse_pos * 0.06;       // 前景：网格地板
    let parallax_top = config.mouse_pos * 0.08;      // 最前：萤火粒子

    let horizon = 0.62;
    let below = uv.y - horizon;
    let floor_mask = smoothstep(-0.015, 0.05, below);
    let fog = 1.0 - smoothstep(0.03, 0.22, abs(uv.y - horizon));
    let fog_color = vec3(0.13, 0.85, 0.85);

    // 深海基底：暗部标定 #092631（G/B≈0.78 海水深邃青），顶部暗、靠地平线微亮
    var col = vec3(0.005, 0.030, 0.045);
    let depth_haze = smoothstep(0.10, 0.55, uv.y);
    col = col + vec3(0.008, 0.062, 0.075) * depth_haze;

    // 透视网格地板（前景视差）：静止网格；水滴折射式弯曲随距离增大，远处更明显
    let floor_uv = uv + parallax_fg;
    let depth = 0.10 / max(floor_uv.y - horizon, 0.003);
    let xw = (floor_uv.x - 0.5) * aspect * depth * 2.2;
    let bend_k = 0.35 + 0.65 * smoothstep(0.10, 0.90, depth);
    let wave = (sin(xw * 2.0 + t * 0.30) * 0.06 + sin(depth * 1.8 - t * 0.22) * 0.05) * bend_k;
    let z_line = depth * 2.4 + wave * 1.2;
    let gz = abs(fract(z_line * 4.0 + 0.5) - 0.5);
    let bend = 0.06 * smoothstep(0.05, 0.90, depth);
    let wave_x = (sin(depth * 1.1 + t * 0.18) * 0.6 + sin(xw * 1.1 - t * 0.13) * 0.4) * bend;
    let gx = abs(fract((xw + wave_x) * 4.4 + 0.5) - 0.5);
    let line_w = 0.007 + depth * 0.004;
    var line = (1.0 - smoothstep(0.0, line_w, gz)) * 0.75 + (1.0 - smoothstep(0.0, line_w * 0.9, gx));
    line = line * smoothstep(0.0, 0.04, below) * (1.0 - fog);
    let floor_shimmer = textureSample(noise_tex, noise_smp, floor_uv * 3.0 + vec2(t * 0.02, 0.0)).r * (1.0 - fog * 0.5);
    let floor_base = vec3(0.0018, 0.0105, 0.0243) + vec3(0.015, 0.055, 0.10) * floor_shimmer * 0.10;
    col = mix(col, floor_base, floor_mask);
    col = col + vec3(0.10, 0.32, 0.40) * line * floor_mask * 0.65;

    // 光柱系统：宽无缝光组 + 单一窄束；宽度/亮度/色相/落地距离/弯曲各自独立
    // 运动 = 微妙整体平移 + 小幅周期旋转（约5°），非大幅摆动
    let beam_auv = auv + parallax_mid * 2.0 + vec2(sin(t * 6.28318 / 47.0 + 0.6) * 0.025, 0.0);
    let sway = sin(t * 6.28318 / 20.0) * 0.08 + sin(t * 6.28318 / 33.0 + 1.7) * 0.03;
    var beams = beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 1.7), 1.7, 0.045 * aspect, 0.050, 0.12, 0.80, 0.030 * (0.75 + 0.25 * sin(t * 0.06 + 1.0)), 0.78, vec3(0.30, 0.80, 0.96));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 3.4), 3.4, 0.100 * aspect, 0.060, 0.13, 1.00, -0.020 * (0.75 + 0.25 * sin(t * 0.06 + 2.0)), 0.76, vec3(0.34, 0.84, 0.98));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 5.1), 5.1, 0.155 * aspect, 0.050, 0.12, 0.70, 0.050 * (0.75 + 0.25 * sin(t * 0.06 + 3.0)), 0.80, vec3(0.28, 0.76, 0.94));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 6.8), 6.8, 0.300 * aspect, 0.045, 0.10, 0.90, -0.040 * (0.75 + 0.25 * sin(t * 0.06 + 4.0)), 0.74, vec3(0.36, 0.86, 1.00));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 8.5), 8.5, 0.355 * aspect, 0.060, 0.12, 1.15, 0.020 * (0.75 + 0.25 * sin(t * 0.06 + 5.0)), 0.78, vec3(0.40, 0.90, 1.00));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 10.2), 10.2, 0.410 * aspect, 0.045, 0.10, 0.85, -0.030 * (0.75 + 0.25 * sin(t * 0.06 + 6.0)), 0.75, vec3(0.32, 0.82, 0.98));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 11.9), 11.9, 0.520 * aspect, 0.028, 0.06, 0.55, 0.060 * (0.75 + 0.25 * sin(t * 0.06 + 7.0)), 0.72, vec3(0.30, 0.78, 0.95));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 13.6), 13.6, 0.600 * aspect, 0.032, 0.07, 0.75, -0.050 * (0.75 + 0.25 * sin(t * 0.06 + 8.0)), 0.77, vec3(0.35, 0.83, 0.96));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 15.3), 15.3, 0.700 * aspect, 0.050, 0.11, 0.80, 0.040 * (0.75 + 0.25 * sin(t * 0.06 + 9.0)), 0.79, vec3(0.31, 0.80, 0.97));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 17.0), 17.0, 0.760 * aspect, 0.055, 0.12, 1.05, -0.030 * (0.75 + 0.25 * sin(t * 0.06 + 10.0)), 0.75, vec3(0.37, 0.87, 0.99));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 18.7), 18.7, 0.815 * aspect, 0.045, 0.10, 0.75, 0.050 * (0.75 + 0.25 * sin(t * 0.06 + 11.0)), 0.78, vec3(0.29, 0.77, 0.95));
    beams = beams + beam_column3(t, beam_auv.x, uv.y, horizon, sway, 0.030 + 0.015 * sin(t * 0.05 + 20.4), 20.4, 0.870 * aspect, 0.040, 0.09, 0.60, -0.040 * (0.75 + 0.25 * sin(t * 0.06 + 12.0)), 0.81, vec3(0.33, 0.82, 0.94));
    beams = beams * (0.60 + 0.40 * (1.0 - uv.y));
    // 光柱增益：R 压抑、G/B 提亮，亮核推向 #81FFFF 纯青（亮度系数保留手动调校值）
    col = col + beams * vec3(0.55, 1.30, 1.30) * 0.168;

    // 移动点光源：三盏缓游的微小亮光源，增强空间立体感
    let pl1 = vec2(aspect * (0.42 + 0.10 * sin(t * 0.11)), 0.30 + 0.10 * sin(t * 0.07 + 2.0));
    let pl2 = vec2(aspect * (0.55 + 0.12 * sin(t * 0.09 + 4.0)), 0.44 + 0.08 * sin(t * 0.13 + 1.0));
    let pl3 = vec2(aspect * (0.30 + 0.10 * sin(t * 0.13 + 2.5)), 0.18 + 0.08 * cos(t * 0.08 + 0.5));
    var pl = point_light(auv, pl1, 9000.0);
    pl = pl + point_light(auv, pl2, 14000.0) * 0.8;
    pl = pl + point_light(auv, pl3, 7000.0) * 0.7;
    col = col + vec3(0.85, 1.0, 1.0) * pl * 0.30;

    // 上浮光点三层 + 近摄 bokeh（最前层视差）：小点锐利白色、消散转蓝；越小上升越快
    let glow_auv = auv + parallax_top;
    var glow = glow_layer(glow_auv, t * 1.45, vec2(26.0, 16.0), 0.0, 0.22, 500000.0, 50000.0);
    glow = glow + glow_layer(glow_auv, t + 7.0, vec2(12.0, 9.0), 13.7, 0.29, 120000.0, 45000.0);
    glow = glow + glow_layer(glow_auv, t * 0.70 + 3.0, vec2(7.0, 4.0), 41.3, 1.08, 50000.0, 22000.0);
    glow = glow + bokeh_layer(glow_auv, t * 0.50 + 11.0, vec2(3.0, 2.0), 77.7, 1.00);
    glow = glow * mix(1.0, 0.45, floor_mask);
    let glow_mask = 1.0 - smoothstep(0.92, 1.0, uv.y);
    col = col + glow * glow_mask;

    // 地平线亮雾带（微光带：远处"暗但清晰"，雾只留一线青光）
    col = mix(col, fog_color, fog * 0.12);

    // 暗角（减弱：参考仅有轻微暗角）
    let vig = 1.0 - smoothstep(0.4, 1.1, distance(uv, vec2(0.5, 0.45)) * 1.3);
    col = col * mix(0.80, 1.0, vig);

    return vec4(col, 1.0);
}
