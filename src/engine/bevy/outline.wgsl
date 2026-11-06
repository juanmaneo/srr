// The ink outline of the 3D fighters: an inverted hull. The mesh is drawn a second time with
// every vertex pushed out along its normal by `width` (world units) and its front faces culled
// (`OutlineMaterial::specialize`), so only the back of the hull shows, as a dark rim around the
// silhouette like the lines of a manga panel. The skinning is Bevy's own, from the joints of
// the animation, so the outline follows every pose.

#import bevy_pbr::{
    mesh_functions,
    skinning,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

struct Outline {
    // The ink.
    color: vec4<f32>,
    // x: how far the hull is pushed out, in world units. y, z, w: unused.
    width: vec4<f32>,
};

@group(2) @binding(0) var<uniform> outline: Outline;

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
    let pushed = world_position.xyz + normalize(world_normal) * outline.width.x;
    out.world_position = vec4<f32>(pushed, 1.0);
    out.world_normal = world_normal;
    out.position = position_world_to_clip(pushed);
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return outline.color;
}
