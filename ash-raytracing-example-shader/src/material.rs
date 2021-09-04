use spirv_std::glam::{vec3, Vec3, Vec4, Vec4Swizzles};
#[allow(unused_imports)]
use spirv_std::num_traits::Float;

use crate::{
    bool::Bool32,
    hittable::HitRecord,
    math::{random_in_unit_sphere, IsNearZero},
    rand::DefaultRng,
    ray::Ray,
};

#[derive(Clone, Default)]
pub struct Scatter {
    pub color: Vec3,
    pub ray: Ray,
}

pub trait Material {
    fn scatter(
        &self,
        ray: &Ray,
        hit_record: &HitRecord,
        rng: &mut DefaultRng,
        scatter: &mut Scatter,
    ) -> Bool32;
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
struct EnumMaterialData {
    v0: Vec4,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct EnumMaterial {
    data: EnumMaterialData,
    t: u32,
}

struct Lambertian<'a> {
    data: &'a EnumMaterialData,
}

struct Metal<'a> {
    data: &'a EnumMaterialData,
}

struct Dielectric<'a> {
    data: &'a EnumMaterialData,
}

fn reflect(v: Vec3, n: Vec3) -> Vec3 {
    v - 2.0 * v.dot(n) * n
}

fn refract(uv: Vec3, n: Vec3, etai_over_etat: f32) -> Vec3 {
    let cos_theta = (-uv).dot(n).min(1.0);
    let r_out_perp = etai_over_etat * (uv + cos_theta * n);
    let r_out_parallel = -(1.0 - r_out_perp.length_squared()).abs().sqrt() * n;
    r_out_perp + r_out_parallel
}

fn reflectance(cosine: f32, ref_idx: f32) -> f32 {
    let r0 = (1.0 - ref_idx) / (1.0 + ref_idx);
    let r0 = r0 * r0;
    r0 + (1.0 - r0) * (1.0 - cosine).powf(5.0)
}

impl<'a> Lambertian<'a> {
    fn albedo(&self) -> Vec3 {
        self.data.v0.xyz()
    }
}

impl<'a> Material for Lambertian<'a> {
    fn scatter(
        &self,
        ray: &Ray,
        hit_record: &HitRecord,
        rng: &mut DefaultRng,
        scatter: &mut Scatter,
    ) -> Bool32 {
        let scatter_direction = hit_record.normal + random_in_unit_sphere(rng).normalize();

        let scatter_direction = if scatter_direction.is_near_zero().into() {
            hit_record.normal
        } else {
            scatter_direction
        };

        let scatterd = Ray {
            origin: hit_record.position,
            direction: scatter_direction,
            time: ray.time,
        };

        *scatter = Scatter {
            color: self.albedo(),
            ray: scatterd,
        };
        Bool32::TRUE
    }
}

impl<'a> Metal<'a> {
    fn albedo(&self) -> Vec3 {
        self.data.v0.xyz()
    }

    fn fuzz(&self) -> f32 {
        self.data.v0.w
    }
}

impl<'a> Material for Metal<'a> {
    fn scatter(
        &self,
        ray: &Ray,
        hit_record: &HitRecord,
        rng: &mut DefaultRng,
        scatter: &mut Scatter,
    ) -> Bool32 {
        let reflected = reflect(ray.direction.normalize(), hit_record.normal);
        let scatterd = reflected + self.fuzz() * random_in_unit_sphere(rng);
        if scatterd.dot(hit_record.normal) > 0.0 {
            *scatter = Scatter {
                color: self.albedo(),
                ray: Ray {
                    origin: hit_record.position,
                    direction: scatterd,
                    time: ray.time,
                },
            };
            Bool32::TRUE
        } else {
            Bool32::FALSE
        }
    }
}

impl<'a> Dielectric<'a> {
    fn ir(&self) -> f32 {
        self.data.v0.x
    }
}

impl<'a> Material for Dielectric<'a> {
    fn scatter(
        &self,
        ray: &Ray,
        hit_record: &HitRecord,
        rng: &mut DefaultRng,
        scatter: &mut Scatter,
    ) -> Bool32 {
        let refraction_ratio = if hit_record.front_face.into() {
            1.0 / self.ir()
        } else {
            self.ir()
        };

        let unit_direction = ray.direction.normalize();
        let cos_theta = (-unit_direction).dot(hit_record.normal).min(1.0);
        let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();
        let cannot_refract = refraction_ratio * sin_theta > 1.0;

        let direction = if Bool32::new(cannot_refract)
            .or(Bool32::new(
                reflectance(cos_theta, refraction_ratio) > rng.next_f32(),
            ))
            .into()
        {
            reflect(unit_direction, hit_record.normal)
        } else {
            refract(unit_direction, hit_record.normal, refraction_ratio)
        };

        *scatter = Scatter {
            color: vec3(1.0, 1.0, 1.0),
            ray: Ray {
                origin: hit_record.position,
                direction,
                time: ray.time,
            },
        };
        Bool32::TRUE
    }
}

impl Material for EnumMaterial {
    fn scatter(
        &self,
        ray: &Ray,
        hit_record: &HitRecord,
        rng: &mut DefaultRng,
        scatter: &mut Scatter,
    ) -> Bool32 {
        match self.t {
            0 => Lambertian { data: &self.data }.scatter(ray, hit_record, rng, scatter),
            1 => Metal { data: &self.data }.scatter(ray, hit_record, rng, scatter),
            _ => Dielectric { data: &self.data }.scatter(ray, hit_record, rng, scatter),
        }
    }
}
