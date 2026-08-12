struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct Immediates {
    surface_size: vec2<f32>,
    viewport_origin: vec2<f32>,
    viewport_size: vec2<f32>,
    padding: vec2<f32>,
}

var<immediate> immediates: Immediates;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var output: VertexOutput;
    output.uv = vec2<f32>(
        f32((index << 1u) & 2u),
        f32(index & 2u),
    );
    output.position = vec4<f32>(output.uv * 2.0 - 1.0, 0.0, 1.0);
    output.uv.y = 1.0 - output.uv.y;
    return output;
}

@group(0) @binding(0)
var canvas_texture: texture_2d<f32>;

@group(0) @binding(1)
var ui_texture: texture_2d<f32>;

@group(0) @binding(2)
var source_sampler: sampler;

// Color transfer and CEF alpha composition follow Graphite commit a034923,
// desktop/src/render/composite_shader.wgsl. Koharu has no overlay texture.
@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let ui_linear = srgb_to_linear(textureSample(ui_texture, source_sampler, input.uv));
    if (ui_linear.a >= 0.999) {
        return ui_linear;
    }

    // CEF supplies premultiplied pixels; restore straight sRGB before the
    // final source-over operation, matching Graphite's browser composition.
    let ui_srgb = linear_to_srgb(unpremultiply(ui_linear));
    let pixel = input.uv * immediates.surface_size;
    let viewport_end = immediates.viewport_origin + immediates.viewport_size;
    let inside_viewport = all(pixel >= immediates.viewport_origin)
        && all(pixel < viewport_end)
        && all(immediates.viewport_size > vec2<f32>(0.0));
    var canvas_srgb = vec4<f32>(0.025, 0.025, 0.025, 1.0);
    if (inside_viewport) {
        let canvas_uv = (pixel - immediates.viewport_origin) / immediates.viewport_size;
        canvas_srgb = textureSample(canvas_texture, source_sampler, canvas_uv);
        if (canvas_srgb.a < 0.001) {
            canvas_srgb = vec4<f32>(0.025, 0.025, 0.025, 1.0);
        } else if (canvas_srgb.a < 0.999) {
            canvas_srgb = blend(canvas_srgb, vec4<f32>(0.025, 0.025, 0.025, 1.0));
        }
    }

    if (ui_srgb.a < 0.001) {
        return srgb_to_linear(canvas_srgb);
    }
    return srgb_to_linear(blend(ui_srgb, canvas_srgb));
}

fn blend(foreground: vec4<f32>, background: vec4<f32>) -> vec4<f32> {
    let alpha = foreground.a + background.a * (1.0 - foreground.a);
    let rgb = foreground.rgb * foreground.a
        + background.rgb * background.a * (1.0 - foreground.a);
    return vec4<f32>(rgb, alpha);
}

fn linear_to_srgb(color: vec4<f32>) -> vec4<f32> {
    let cutoff = vec3<f32>(0.0031308);
    let low = color.rgb * 12.92;
    let high = 1.055 * pow(max(color.rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return vec4<f32>(select(low, high, color.rgb > cutoff), color.a);
}

fn srgb_to_linear(color: vec4<f32>) -> vec4<f32> {
    let cutoff = vec3<f32>(0.04045);
    let low = color.rgb / 12.92;
    let high = pow((color.rgb + 0.055) / 1.055, vec3<f32>(2.4));
    return vec4<f32>(select(low, high, color.rgb > cutoff), color.a);
}

fn unpremultiply(color: vec4<f32>) -> vec4<f32> {
    if (color.a > 0.0) {
        return vec4<f32>(color.rgb / color.a, color.a);
    }
    return vec4<f32>(0.0);
}
