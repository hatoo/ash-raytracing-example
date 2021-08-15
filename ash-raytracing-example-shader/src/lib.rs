#![cfg_attr(
    target_arch = "spirv",
    no_std,
    feature(register_attr),
    register_attr(spirv)
)]

#[cfg(not(target_arch = "spirv"))]
use spirv_std::macros::spirv;

use spirv_std::{
    glam::{uvec2, vec2, vec3, vec4, UVec3, Vec2, Vec3, Vec4},
    image::Image,
    ray_tracing::{AccelerationStructure, RayFlags},
};

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
    #[spirv(uniform, descriptor_set = 0, binding = 2)] colors: &[Vec3; 3],
) {
    *out = colors[id as usize];
}

#[spirv(ray_generation)]
pub fn main_ray_generation(
    #[spirv(launch_id)] launch_id: UVec3,
    #[spirv(launch_size)] launch_size: UVec3,
    #[spirv(descriptor_set = 0, binding = 0)] top_level_as: &AccelerationStructure,
    #[spirv(descriptor_set = 0, binding = 1)] image: &Image!(2D, format=rgba8, sampled=false),
    // #[spirv(ray_payload)] payload: &mut Vec3,
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

    let mut payload = Vec3::ZERO;

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
            &mut payload,
        );
    }

    unsafe {
        image.write(uvec2(launch_id.x, launch_id.y), payload.extend(1.0));
    }
}
