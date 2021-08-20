#![cfg_attr(
    target_arch = "spirv",
    no_std,
    feature(register_attr),
    register_attr(spirv)
)]
#![feature(asm)]

use spirv_std::macros::gpu_only;
#[cfg(not(target_arch = "spirv"))]
use spirv_std::macros::spirv;

use spirv_std::num_traits::Float;
use spirv_std::{
    arch::report_intersection,
    glam::{uvec2, vec2, vec3, vec4, UVec3, Vec2, Vec3, Vec4},
    image::Image,
    ray_tracing::{AccelerationStructure, RayFlags},
};

pub struct PushConstants {
    x: f32,
}

#[spirv(fragment)]
pub fn main_fs(output: &mut Vec4, color: Vec3) {
    *output = color.extend(1.0);
}

#[spirv(vertex)]
pub fn main_vs(
    #[spirv(vertex_index)] vert_id: i32,
    #[spirv(position, invariant)] out_pos: &mut Vec4,
    color: &mut Vec3,
) {
    *out_pos = vec4(
        (vert_id - 1) as f32,
        ((vert_id & 1) * 2 - 1) as f32,
        0.0,
        1.0,
    );

    *color = [
        vec3(1.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
        vec3(0.0, 0.0, 1.0),
    ][vert_id as usize];
}

#[spirv(miss)]
pub fn main_miss(#[spirv(incoming_ray_payload)] out: &mut Vec3) {
    *out = vec3(0.5, 0.5, 0.5);
}

#[spirv(closest_hit)]
pub fn main_closest_hit(
    #[spirv(incoming_ray_payload)] out: &mut Vec3,
    #[spirv(instance_id)] id: u32,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 2)] colors: &[Vec3],
) {
    *out = colors[id as usize];
}

#[spirv(ray_generation)]
pub fn main_ray_generation(
    #[spirv(launch_id)] launch_id: UVec3,
    #[spirv(launch_size)] launch_size: UVec3,
    #[spirv(push_constant)] constants: &PushConstants,
    #[spirv(descriptor_set = 0, binding = 0)] top_level_as: &AccelerationStructure,
    #[spirv(descriptor_set = 0, binding = 1)] image: &Image!(2D, format=rgba32f, sampled=false),
    #[spirv(ray_payload)] payload: &mut Vec3,
) {
    let pixel_center = vec2(launch_id.x as f32, launch_id.y as f32) + vec2(0.5, 0.5);
    let in_uv = pixel_center / vec2(launch_size.x as f32, launch_size.y as f32);

    let d = in_uv * 2.0 - Vec2::ONE;
    let aspect_ratio = launch_size.x as f32 / launch_size.y as f32;

    let origin = vec3(0.0, 0.0, -2.0);
    let direction = vec3(d.x * aspect_ratio, -d.y, 1.0).normalize();
    let cull_mask = 0xff;
    let tmin = 0.001;
    let tmax = 1000.0;

    *payload = Vec3::ZERO;

    unsafe {
        top_level_as.trace_ray(
            RayFlags::OPAQUE,
            cull_mask,
            0,
            0,
            0,
            origin,
            tmin,
            direction,
            tmax,
            payload,
        );
    }

    let pos = uvec2(launch_id.x, launch_id.y);
    let prev: Vec4 = image.read(pos);

    unsafe {
        image.write(pos, prev + (*payload * constants.x).extend(1.0));
    }
}

#[spirv(intersection)]
pub fn sphere_intersection(
    #[spirv(object_ray_origin)] ray_origin: Vec3,
    #[spirv(object_ray_direction)] ray_direction: Vec3,
    #[spirv(world_ray_origin)] world_ray_origin: Vec3,
    #[spirv(world_ray_direction)] world_ray_direction: Vec3,
    #[spirv(ray_tmin)] t_min: f32,
    #[spirv(ray_tmax)] t_max: f32,
    #[spirv(hit_attribute)] hit_pos: &mut Vec3,
) {
    let oc = ray_origin;
    let a = ray_direction.length_squared();
    let half_b = oc.dot(ray_direction);
    let c = oc.length_squared() - 1.0;

    let discriminant = half_b * half_b - a * c;
    if discriminant < 0.0 {
        return;
    }

    let sqrtd = discriminant.sqrt();

    let root0 = (-half_b - sqrtd) / a;
    let root1 = (-half_b + sqrtd) / a;

    if root0 >= t_min {
        if root0 <= t_max {
            *hit_pos = world_ray_origin + root0 * world_ray_direction;
            unsafe {
                report_intersection(root0, 0);
            }
        }
    }

    if root1 >= t_min {
        if root1 <= t_max {
            *hit_pos = world_ray_origin + root1 * world_ray_direction;
            unsafe {
                report_intersection(root1, 0);
            }
        }
    }
}

#[gpu_only]
#[inline(never)]
unsafe fn object_to_world() -> [Vec3; 4] {
    let mut result = Default::default();

    asm! {
        "OpName %gl_ObjectToWorldEXT \"gl_ObjectToWorldEXT\"",
        "OpDecorate %gl_ObjectToWorldEXT BuiltIn ObjectToWorldNV",
        "%float = OpTypeFloat 32",
        "%v3float = OpTypeVector %float 3",
        "%mat4v3float = OpTypeMatrix %v3float 4",
        "%_ptr_Input_mat4v3float = OpTypePointer Generic %mat4v3float",
        "%gl_ObjectToWorldEXT = OpVariable %_ptr_Input_mat4v3float Input",
        "%col0 = OpCompositeExtract %v3float %gl_ObjectToWorldEXT 0",
        "%col1 = OpCompositeExtract %v3float %gl_ObjectToWorldEXT 1",
        "%col2 = OpCompositeExtract %v3float %gl_ObjectToWorldEXT 2",
        "%col3 = OpCompositeExtract %v3float %gl_ObjectToWorldEXT 3",
        "%result = OpCompositeConstruct typeof*{result} %col0 %col1 %col2 %col3",
        result = in(reg) &mut result,
    }

    result
}

#[gpu_only]
#[spirv(closest_hit)]
pub fn sphere_closest_hit(
    #[spirv(hit_attribute)] hit_pos: &Vec3,
    #[spirv(incoming_ray_payload)] out: &mut Vec3,
) {
    let result = unsafe { object_to_world() };
    *out = *hit_pos - result[3];
}
