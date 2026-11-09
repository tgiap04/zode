// Fades the frame already drawn, for `window_opacity`: one triangle covering
// the target, whose output the blend state ignores (`src = zero`) and whose
// blend constant scales what is there (`dst = constant`).

@vertex
fn vs_fade(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_fade() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
