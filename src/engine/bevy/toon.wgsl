// The cel shading of the 3D fighters, the way the anime games shade their characters: the
// albedo (the skin texture, or the material's colour) kept exactly as painted in the light,
// multiplied by a cool, slightly purple tint in the shadow and a deeper one facing away, the
// edges crisp but feathered; a small stepped highlight and a thin rim light on the lit side;
// no gloss, no gradient anywhere. The maths is `srr::front::model` (`shade`, `specular`, `rim`,
// `step`), whose constants fill the uniform (see `docs/tuning-guide.md`): the two must agree.
//
// The vertex stage is Bevy's own (`bevy_pbr::mesh`): it already skins the mesh with the
// joints of the animation, so this file is only the fragment stage.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

struct Toon {
    // The material's colour, multiplied in when there is no texture to sample.
    color: vec4<f32>,
    // xyz: the direction towards the light, in world space (unit). w: 1.0 when the texture is
    // bound (and replaces the colour), 0.0 otherwise.
    light: vec4<f32>,
    // rgb: the tint of the deep shadow. a: the half-Lambert under which it shows.
    deep: vec4<f32>,
    // rgb: the tint of the shadow. a: the half-Lambert under which it shows (above: the light).
    shadow: vec4<f32>,
    // x: the exponent of the highlight. y: the value of n_dot_h ^ x above which it shows.
    // z: its strength. w: the half-width of the feather of every edge.
    specular: vec4<f32>,
    // x: the cosine to the eye under which the rim shows. y: its strength. z, w: unused.
    rim: vec4<f32>,
    // rgb: a colour mixed into the albedo by a (the white flash of a stun).
    tint: vec4<f32>,
};

@group(2) @binding(0) var<uniform> toon: Toon;
@group(2) @binding(1) var base_texture: texture_2d<f32>;
@group(2) @binding(2) var base_sampler: sampler;

// `x` stepped at `edge`, feathered: `srr::front::model::step`.
fn step_at(edge: f32, x: f32) -> f32 {
    let feather = toon.specular.w;
    return smoothstep(edge - feather, edge + feather, x);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var albedo = toon.color;
#ifdef VERTEX_UVS_A
    if toon.light.w > 0.5 {
        albedo = textureSample(base_texture, base_sampler, in.uv);
    }
#endif
    let n = normalize(in.world_normal);
    let l = normalize(toon.light.xyz);
    let v = normalize(view.world_position - in.world_position.xyz);
    let n_dot_l = dot(n, l);
    let half_lambert = n_dot_l * 0.5 + 0.5;

    // The diffuse: deep shadow, shadow, light (`shade`).
    let shadow = step_at(toon.deep.a, half_lambert);
    let lit = step_at(toon.shadow.a, half_lambert);
    let shaded = mix(toon.deep.rgb, toon.shadow.rgb, shadow);
    let diffuse = mix(shaded, vec3<f32>(1.0), lit);

    // The highlight and the rim, on the lit side only (`specular`, `rim`).
    let h = normalize(l + v);
    let n_dot_h = max(dot(n, h), 0.0);
    let highlight = step_at(toon.specular.y, pow(n_dot_h, toon.specular.x)) * lit * toon.specular.z;
    let n_dot_v = max(dot(n, v), 0.0);
    let rim = (1.0 - step_at(toon.rim.x, n_dot_v)) * lit * toon.rim.y;

    let base = mix(albedo.rgb, toon.tint.rgb, toon.tint.a);
    // The light of the highlight and the rim: half the surface's colour, half white.
    let glow = mix(base, vec3<f32>(1.0), 0.5) * (highlight + rim);
    return vec4<f32>(base * diffuse + glow, 1.0);
}
