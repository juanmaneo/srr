// The ink outline of the 3D fighters: an inverted hull. The mesh is drawn a second time with
// every vertex pushed out along its normal and its front faces culled
// (`OutlineMaterial::specialize`), so only the back of the hull shows, as a dark rim around the
// silhouette like the lines of a manga panel. The hull is pushed by `width` *pixels*, whatever
// the distance (`srr::front::model::outline_offset`), and inked in a dark version of the colour
// under it (the texture times `shade`), the way the anime games colour their lines: brown-red
// on skin, deep blue on blue cloth, never a flat black. The skinning is Bevy's own, from the
// joints of the animation, so the outline follows every pose.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    skinning,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

struct Outline {
    // The material's colour, inked when there is no texture to sample.
    color: vec4<f32>,
    // rgb: what the colour under the line is multiplied by. a: 1.0 when the texture is bound.
    shade: vec4<f32>,
    // x: the thickness of the line, in pixels of the viewport. y, z, w: unused.
    width: vec4<f32>,
};

@group(2) @binding(0) var<uniform> outline: Outline;
@group(2) @binding(1) var base_texture: texture_2d<f32>;
@group(2) @binding(2) var base_sampler: sampler;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
#ifdef SKINNED
    let world_from_local = skinning::skin_model(vertex.joint_indices, vertex.joint_weights);
#else
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
#endif
#ifdef VERTEX_NORMALS
#ifdef SKINNED
    let world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    let world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
#else
    let world_normal = vec3<f32>(0.0, 1.0, 0.0);
#endif
    let world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
    // The world length `width` pixels cover at this depth: the projection scales a world
    // height at the depth `w` by `clip_from_view[1][1] / w` onto the two units of clip space,
    // which the viewport spreads over its height (`outline_offset`).
    let clip = position_world_to_clip(world_position.xyz);
    let offset = outline.width.x * 2.0 * clip.w / (view.clip_from_view[1][1] * view.viewport.w);
    let pushed = world_position.xyz + normalize(world_normal) * offset;
    out.world_position = vec4<f32>(pushed, 1.0);
    out.world_normal = world_normal;
    out.position = position_world_to_clip(pushed);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var under = outline.color;
#ifdef VERTEX_UVS_A
    if outline.shade.a > 0.5 {
        under = textureSample(base_texture, base_sampler, in.uv);
    }
#endif
    return vec4<f32>(under.rgb * outline.shade.rgb, 1.0);
}
