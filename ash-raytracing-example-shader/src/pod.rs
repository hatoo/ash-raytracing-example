use bytemuck::{Pod, Zeroable};
use spirv_std::glam::{vec3, Vec3};
#[allow(unused_imports)]
use spirv_std::num_traits::Float;

#[derive(Clone, Copy, Default, Zeroable, Pod)]
#[repr(C)]
pub struct EnumMaterialPod {
    data: [f32; 4],
    t: u32,
    _pad: [f32; 3],
}

impl EnumMaterialPod {
    pub fn new_lambertian(albedo: Vec3) -> Self {
        Self {
            data: [albedo.x, albedo.y, albedo.z, 0.0],
            t: 0,
            _pad: [0.0, 0.0, 0.0],
        }
    }

    pub fn new_metal(albedo: Vec3, fuzz: f32) -> Self {
        Self {
            data: [albedo.x, albedo.y, albedo.z, fuzz],
            t: 1,
            _pad: [0.0, 0.0, 0.0],
        }
    }

    pub fn new_dielectric(ir: f32) -> Self {
        Self {
            data: [ir, 0.0, 0.0, 0.0],
            t: 2,
            _pad: [0.0, 0.0, 0.0],
        }
    }
}
