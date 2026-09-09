fn assets_dir() -> std::path::PathBuf {
    for c in ["../../assets", "../../../assets", "assets"] {
        let p = std::path::PathBuf::from(c);
        if p.is_dir() {
            return p;
        }
    }
    panic!("assets dir not found");
}

fn strip_imports(src: &str) -> String {
    // Bevy 的 #import 在 naga 独立解析时不可用：内联 UiVertexOutput 定义
    // （与 bevy_ui_render 0.19 的 ui_vertex_output.wgsl 逐字段对照）。
    let stub = "struct UiVertexOutput {\n\
        \x20   @location(0) uv: vec2<f32>,\n\
        \x20   @location(1) border_widths: vec4<f32>,\n\
        \x20   @location(2) border_radius: vec4<f32>,\n\
        \x20   @location(3) @interpolate(flat) size: vec2<f32>,\n\
        \x20   @builtin(position) position: vec4<f32>,\n\
        };\n";
    let body: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("#import"))
        .collect::<Vec<_>>()
        .join("\n");
    stub.to_string() + &body
}

fn parse(shader: &str) -> naga::Module {
    naga::front::wgsl::parse_str(shader).expect("wgsl parse")
}

#[test]
fn desktop_background_compiles() {
    let src = std::fs::read_to_string(assets_dir().join("shaders/desktop_background.wgsl"))
        .expect("read shader");
    assert!(
        !src.contains("water_normal"),
        "water-normal binding was removed; shader must not reference it"
    );
    let module = parse(&strip_imports(&src));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("wgsl validate");
}

#[test]
fn desktop_background_skip_guards_cover_full_frame() {
    // 优化正确性：skip 条件在边界处必须精确退化为原表达式。
    // floor_mask = smoothstep(-0.015, 0.05, below)：below<=0.0 时 floor fixpoint
    // 仍要求 mask==0；uv.y>=0.82 时 beams 包络必须为 0。
    let src = std::fs::read_to_string(assets_dir().join("shaders/desktop_background.wgsl"))
        .expect("read shader");
    assert!(
        src.contains("if (floor_mask > 0.0)"),
        "floor skip guard missing"
    );
    assert!(src.contains("if (uv.y < 0.82)"), "beam skip guard missing");
    assert!(
        src.contains("if (d2 * core_k > 30.0") && src.contains("if (d2 * k > 30.0"),
        "particle early-out guards missing"
    );
    assert!(src.contains("fn point_light"), "point_light helper missing");
    // 粒子 early-out 阈值：exp(-30)≈1e-13，远低于 8bit 输出精度。
    // 阈值放宽会引入肉眼可见误差，收紧则浪费 ALU——改动此数需同步更新注释。
    assert!(
        src.contains("> 30.0") && src.contains("> 25.0"),
        "early-out thresholds changed without review"
    );
}
