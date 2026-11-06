// The cel shading of the 3D fighters: the albedo (the skin texture, or the material's colour)
// in two or three flat bands of light, no gloss, an optional rim light on the silhouette.
// The bands step exactly like `srr::front::model::band`, whose constants fill `edges` and
// `levels` (see `docs/tuning-guide.md`).
//
// The vertex stage is Bevy's own (`bevy_pbr::mesh`): it already skins the mesh with the
// joints of the animation, so this file is only the fragment stage.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct Toon {
    // The material's colour, multiplied in when there is no texture to sample.
    color: vec4<f32>,
    // xyz: the direction towards the light, in world space (unit). w: unused.
    light: vec4<f32>,
    // x, y: the cosines at which the shade steps up to the half-tone and to the lit band.
    // z: the cosine to the eye under which the rim shows. w: the brightness of the rim.
    edges: vec4<f32>,
    // x, y, z: the brightness of the shade, the half-tone and the lit band. w: 1.0 when the
    // texture is bound (and replaces the colour), 0.0 otherwise.
    levels: vec4<f32>,
    // rgb: a colour mixed into the albedo by a (the white flash of a stun).
    tint: vec4<f32>,
};

@group(2) @binding(0) var<uniform> toon: Toon;
@group(2) @binding(1) var base_texture: texture_2d<f32>;
@group(2) @binding(2) var base_sampler: sampler;

// The brightness of the band `n_dot_l` falls in.
fn band(n_dot_l: f32) -> f32 {
    if n_dot_l < toon.edges.x {
        return toon.levels.x;
    }
    if n_dot_l < toon.edges.y {
        return toon.levels.y;
    }
    return toon.levels.z;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = toon.color;
#ifdef VERTEX_UVS_A
    if toon.levels.w > 0.5 {
        albedo = textureSample(base_texture, base_sampler, in.uv);
    }
#endif
    let n = normalize(in.world_normal);
    let shade = band(dot(n, normalize(toon.light.xyz)));
    let to_eye = normalize(view.world_position - in.world_position.xyz);
    var rim = 0.0;
    if dot(n, to_eye) < toon.edges.z {
        rim = toon.edges.w;
    }
    let color = mix(albedo.rgb, toon.tint.rgb, toon.tint.a) * shade + vec3<f32>(rim);
    return vec4<f32>(color, 1.0);
}
