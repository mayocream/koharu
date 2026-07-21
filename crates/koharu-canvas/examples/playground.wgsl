struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One oversized triangle covers the window without a vertex buffer.
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    let positions = array(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let position = positions[vertex_index];
    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv = vec2<f32>((position.x + 1.0) * 0.5, (1.0 - position.y) * 0.5);
    return output;
}

@group(0) @binding(0) var canvas_texture: texture_2d<f32>;
@group(0) @binding(1) var canvas_sampler: sampler;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return srgb_to_linear(textureSample(canvas_texture, canvas_sampler, input.uv));
}

// koharu-canvas stores display-referred sRGB bytes in an unorm texture. An
// sRGB window surface expects linear shader output and performs the final
// encoding itself.
fn srgb_to_linear(color: vec4<f32>) -> vec4<f32> {
    let low = color.rgb / 12.92;
    let high = pow((color.rgb + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(low, high, color.rgb > vec3<f32>(0.04045)), color.a);
}
