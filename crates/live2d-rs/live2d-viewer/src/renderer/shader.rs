use anyhow::Result;
use glow::*;

pub const VERT_SRC: &str = r#"#version 330 core
layout(location = 0) in vec2 aPos;
layout(location = 1) in vec2 aUV;
out vec2 vUV;
uniform vec2 uScale;
uniform vec2 uTranslate;

void main() {
    gl_Position = vec4(aPos.x * uScale.x + uTranslate.x, aPos.y * uScale.y + uTranslate.y, 0.0, 1.0);
    vUV = vec2(aUV.x, 1.0 - aUV.y);
}
"#;

pub const FRAG_SRC: &str = r#"#version 330 core
in vec2 vUV;
out vec4 FragColor;

uniform sampler2D uTexture;
uniform vec4 uMultiplyColor;
uniform vec4 uScreenColor;
uniform float uOpacity;

void main() {
    vec4 tex = texture(uTexture, vUV);
    tex.rgb = tex.rgb * uMultiplyColor.rgb;
    tex.rgb = tex.rgb + uScreenColor.rgb - (tex.rgb * uScreenColor.rgb);
    tex.a *= uOpacity;
    FragColor = vec4(tex.rgb * tex.a, tex.a);
}
"#;

pub const MASK_VERT_SRC: &str = r#"#version 330 core
layout(location = 0) in vec2 aPos;
layout(location = 1) in vec2 aUV;
out vec2 vUV;
uniform vec2 uScale;
uniform vec2 uTranslate;

void main() {
    gl_Position = vec4(aPos.x * uScale.x + uTranslate.x, aPos.y * uScale.y + uTranslate.y, 0.0, 1.0);
    vUV = vec2(aUV.x, 1.0 - aUV.y);
}
"#;

pub const MASK_FRAG_SRC: &str = r#"#version 330 core
in vec2 vUV;
out vec4 FragColor;
uniform sampler2D uTexture;

void main() {
    // SDK convention: mask drawables use raw texture alpha, NOT drawable opacity.
    // Mask drawables (e.g. eye whites for pupil clipping) often have opacity 0
    // because they are not directly visible — they only define the clipping boundary.
    float maskAlpha = texture(uTexture, vUV).a;
    FragColor = vec4(0.0, 0.0, 0.0, maskAlpha);
}
"#;

/// Fragment shader for rendering a drawable with an applied clipping mask.
/// Uses `gl_FragCoord` to look up the mask alpha from the off-screen FBO texture.
/// Supports mask inversion via `uInvertMask` uniform (0.0 = normal, 1.0 = inverted).
pub const FRAG_MASKED_SRC: &str = r#"#version 330 core
in vec2 vUV;
out vec4 FragColor;

uniform sampler2D uTexture;
uniform sampler2D uMaskTexture;
uniform vec4 uMultiplyColor;
uniform vec4 uScreenColor;
uniform float uOpacity;
uniform vec2 uMaskSize;
uniform float uInvertMask;

void main() {
    vec2 maskUV = gl_FragCoord.xy / uMaskSize;
    float maskAlpha = texture(uMaskTexture, maskUV).a;

    // When uInvertMask = 1.0, invert so that mask drawable opaque → hidden
    // (SDK convention: inverted mask uses (1.0 - maskVal) instead of maskVal)
    // SDK convention: mask buffer has 0 inside shape, 1 outside (clear to white, ZERO,ONE_MINUS_SRC_ALPHA blend)
    // Normal: invert 0→1 so inside shape is visible; Inverted: use raw value (hide inside shape)
    float maskFactor = mix(1.0 - maskAlpha, maskAlpha, uInvertMask);

    vec4 tex = texture(uTexture, vUV);
    tex.rgb = tex.rgb * uMultiplyColor.rgb;
    tex.rgb = tex.rgb + uScreenColor.rgb - (tex.rgb * uScreenColor.rgb);
    tex.a *= uOpacity;
    FragColor = vec4(tex.rgb * tex.a * maskFactor, tex.a * maskFactor);
}
"#;

pub unsafe fn compile_program(gl: &Context, vert: &str, frag: &str) -> Result<NativeProgram> {
    let program = gl
        .create_program()
        .map_err(|e| anyhow::anyhow!("create program: {:?}", e))?;

    let vs = compile_shader(gl, VERTEX_SHADER, vert)?;
    let fs = compile_shader(gl, FRAGMENT_SHADER, frag)?;
    gl.attach_shader(program, vs);
    gl.attach_shader(program, fs);
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        let log = gl.get_program_info_log(program);
        return Err(anyhow::anyhow!("program link: {}", log));
    }
    gl.delete_shader(vs);
    gl.delete_shader(fs);
    Ok(program)
}

unsafe fn compile_shader(gl: &Context, kind: u32, src: &str) -> Result<NativeShader> {
    let shader = gl
        .create_shader(kind)
        .map_err(|e| anyhow::anyhow!("create shader: {:?}", e))?;
    gl.shader_source(shader, src);
    gl.compile_shader(shader);
    if !gl.get_shader_compile_status(shader) {
        let log = gl.get_shader_info_log(shader);
        gl.delete_shader(shader);
        anyhow::bail!("shader compile: {}", log);
    }
    Ok(shader)
}
