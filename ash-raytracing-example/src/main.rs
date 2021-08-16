use std::{
    collections::HashSet,
    ffi::{c_void, CStr, CString},
    fs::File,
    io::Write,
    os::raw::c_char,
    ptr::{self, null},
};

use ash::{prelude::VkResult, util::Align, vk};

#[repr(C)]
#[derive(Clone, Debug, Copy)]
struct Vertex {
    pos: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Debug, Copy)]
struct GeometryInstance {
    transform: [f32; 12],
    instance_id_and_mask: u32,
    instance_offset_and_flags: u32,
    acceleration_handle: u64,
}

impl GeometryInstance {
    fn new(
        transform: [f32; 12],
        id: u32,
        mask: u8,
        offset: u32,
        flags: vk::GeometryInstanceFlagsNV,
        acceleration_handle: u64,
    ) -> Self {
        let mut instance = GeometryInstance {
            transform,
            instance_id_and_mask: 0,
            instance_offset_and_flags: 0,
            acceleration_handle,
        };
        instance.set_id(id);
        instance.set_mask(mask);
        instance.set_offset(offset);
        instance.set_flags(flags);
        instance
    }

    fn set_id(&mut self, id: u32) {
        let id = id & 0x00ffffff;
        self.instance_id_and_mask |= id;
    }

    fn set_mask(&mut self, mask: u8) {
        let mask = mask as u32;
        self.instance_id_and_mask |= mask << 24;
    }

    fn set_offset(&mut self, offset: u32) {
        let offset = offset & 0x00ffffff;
        self.instance_offset_and_flags |= offset;
    }

    fn set_flags(&mut self, flags: vk::GeometryInstanceFlagsNV) {
        let flags = flags.as_raw() as u32;
        self.instance_offset_and_flags |= flags << 24;
    }
}

fn main() {
    const ENABLE_VALIDATION_LAYER: bool = cfg!(debug_assertions);
    const WIDTH: u32 = 800;
    const HEIGHT: u32 = 600;
    const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

    let extent = vk::Extent2D::builder().width(WIDTH).height(HEIGHT).build();

    let validation_layers: Vec<CString> = if ENABLE_VALIDATION_LAYER {
        vec![CString::new("VK_LAYER_KHRONOS_validation").unwrap()]
    } else {
        Vec::new()
    };
    let validation_layers_ptr: Vec<*const i8> = validation_layers
        .iter()
        .map(|c_str| c_str.as_ptr())
        .collect();

    let entry = unsafe { ash::Entry::new() }.unwrap();

    assert_eq!(
        check_validation_layer_support(
            &entry,
            validation_layers.iter().map(|cstring| cstring.as_c_str())
        ),
        Ok(true)
    );

    let instance = {
        let application_name = CString::new("Hello Triangle").unwrap();
        let engine_name = CString::new("No Engine").unwrap();

        let mut debug_utils_create_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::WARNING |
            // vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE |
            // vk::DebugUtilsMessageSeverityFlagsEXT::INFO |
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION,
            )
            .pfn_user_callback(Some(default_vulkan_debug_utils_callback))
            .build();

        let application_info = vk::ApplicationInfo::builder()
            .application_name(application_name.as_c_str())
            .application_version(vk::make_api_version(0, 1, 0, 0))
            .engine_name(engine_name.as_c_str())
            .engine_version(vk::make_api_version(0, 1, 0, 0))
            .api_version(vk::API_VERSION_1_2)
            .build();

        let instance_create_info = vk::InstanceCreateInfo::builder()
            .application_info(&application_info)
            .enabled_layer_names(validation_layers_ptr.as_slice());

        let instance_create_info = if ENABLE_VALIDATION_LAYER {
            instance_create_info.push_next(&mut debug_utils_create_info)
        } else {
            instance_create_info
        }
        .build();

        unsafe { entry.create_instance(&instance_create_info, None) }
            .expect("failed to create instance!")
    };

    let (physical_device, queue_family_index) = pick_physical_device_and_queue_family_indices(
        &instance,
        &[
            ash::extensions::khr::AccelerationStructure::name(),
            ash::extensions::khr::DeferredHostOperations::name(),
            ash::extensions::khr::RayTracingPipeline::name(),
        ],
    )
    .unwrap()
    .unwrap();

    let device: ash::Device = {
        let queue_create_info = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .queue_priorities(&[1.0])
            .build();

        let mut physical_device_vulkan_memory_model_features =
            vk::PhysicalDeviceVulkanMemoryModelFeatures::builder()
                .vulkan_memory_model(true)
                .build();

        let mut descriptor_indexing = vk::PhysicalDeviceDescriptorIndexingFeaturesEXT::builder()
            .descriptor_binding_variable_descriptor_count(true)
            .runtime_descriptor_array(true)
            .build();

        let mut scalar_block = vk::PhysicalDeviceScalarBlockLayoutFeaturesEXT::builder()
            .scalar_block_layout(true)
            .build();

        let mut features2 = vk::PhysicalDeviceFeatures2::default();
        unsafe {
            instance
                .fp_v1_1()
                .get_physical_device_features2(physical_device, &mut features2)
        };

        let mut raytracing_pipeline = vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::builder()
            .ray_tracing_pipeline(true)
            .build();

        let device_create_info = vk::DeviceCreateInfo::builder()
            .push_next(&mut physical_device_vulkan_memory_model_features)
            .push_next(&mut descriptor_indexing)
            .push_next(&mut scalar_block)
            .push_next(&mut raytracing_pipeline)
            .queue_create_infos(&[queue_create_info])
            .enabled_layer_names(validation_layers_ptr.as_slice())
            .enabled_extension_names(&[
                ash::extensions::khr::RayTracingPipeline::name().as_ptr(),
                ash::extensions::khr::AccelerationStructure::name().as_ptr(),
                ash::extensions::khr::DeferredHostOperations::name().as_ptr(),
                ash::extensions::nv::RayTracing::name().as_ptr(),
                vk::KhrSpirv14Fn::name().as_ptr(),
                vk::ExtDescriptorIndexingFn::name().as_ptr(),
                vk::ExtScalarBlockLayoutFn::name().as_ptr(),
                vk::KhrGetMemoryRequirements2Fn::name().as_ptr(),
            ])
            .build();

        unsafe { instance.create_device(physical_device, &device_create_info, None) }
            .expect("Failed to create logical Device!")
    };

    let ray_tracing = ash::extensions::nv::RayTracing::new(&instance, &device);
    let rt_properties =
        unsafe { ash::extensions::nv::RayTracing::get_properties(&instance, physical_device) };

    let graphics_queue = unsafe { device.get_device_queue(queue_family_index, 0) };

    let command_pool = {
        let command_pool_create_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .build();

        unsafe { device.create_command_pool(&command_pool_create_info, None) }
            .expect("Failed to create Command Pool!")
    };

    /*
    let pipeline_layout = {
        let pipeline_layout_create_info = vk::PipelineLayoutCreateInfo::default();

        unsafe { device.create_pipeline_layout(&pipeline_layout_create_info, None) }
            .expect("Failed to create pipeline layout!")
    };
    */

    let device_memory_properties =
        unsafe { instance.get_physical_device_memory_properties(physical_device) };

    let image = {
        let image_create_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(COLOR_FORMAT)
            .extent(
                vk::Extent3D::builder()
                    .width(WIDTH)
                    .height(HEIGHT)
                    .depth(1)
                    .build(),
            )
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            .usage(
                vk::ImageUsageFlags::COLOR_ATTACHMENT
                    | vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::TRANSFER_SRC,
            )
            .build();

        unsafe { device.create_image(&image_create_info, None) }.unwrap()
    };

    let device_memory = {
        let mem_reqs = unsafe { device.get_image_memory_requirements(image) };
        let mem_alloc_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(mem_reqs.size)
            .memory_type_index(get_memory_type_index(
                device_memory_properties,
                mem_reqs.memory_type_bits,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            ));

        unsafe { device.allocate_memory(&mem_alloc_info, None) }.unwrap()
    };

    unsafe { device.bind_image_memory(image, device_memory, 0) }.unwrap();

    let image_view = {
        let image_view_create_info = vk::ImageViewCreateInfo::builder()
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(COLOR_FORMAT)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            })
            .image(image)
            .build();

        unsafe { device.create_image_view(&image_view_create_info, None) }.unwrap()
    };

    let command_buffer = {
        let allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(1)
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .build();

        let command_buffers = unsafe { device.allocate_command_buffers(&allocate_info) }.unwrap();
        command_buffers[0]
    };

    unsafe {
        device.begin_command_buffer(
            command_buffer,
            &vk::CommandBufferBeginInfo::builder()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
                .build(),
        )
    }
    .unwrap();

    let image_barrier = vk::ImageMemoryBarrier::builder()
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::empty())
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::GENERAL)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::builder()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .base_mip_level(0)
                .level_count(1)
                .base_array_layer(0)
                .layer_count(1)
                .build(),
        )
        .build();

    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[image_barrier],
        );

        device.end_command_buffer(command_buffer).unwrap();

        let fence = {
            let fence_create_info = vk::FenceCreateInfo::builder()
                .flags(vk::FenceCreateFlags::SIGNALED)
                .build();

            unsafe { device.create_fence(&fence_create_info, None) }
                .expect("Failed to create Fence Object!")
        };

        let submit_infos = [vk::SubmitInfo::builder()
            .command_buffers(&[command_buffer])
            .build()];

        unsafe {
            device
                .reset_fences(&[fence])
                .expect("Failed to reset Fence!");

            device
                .queue_submit(graphics_queue, &submit_infos, fence)
                .expect("Failed to execute queue submit.");

            device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
            device.destroy_fence(fence, None);
        }
    }

    // acceleration structures

    let (vertex_count, vertex_stride, vertex_buffer, vertex_memory) = {
        let vertices = [
            Vertex {
                pos: [-0.5, -0.5, 0.0],
            },
            Vertex {
                pos: [0.0, 0.5, 0.0],
            },
            Vertex {
                pos: [0.5, -0.5, 0.0],
            },
        ];

        let vertex_count = vertices.len();
        let vertex_stride = std::mem::size_of::<Vertex>();

        let vertex_buffer_size = vertex_stride * vertex_count;

        let buffer_create_info = vk::BufferCreateInfo::builder()
            .size(vertex_buffer_size as u64)
            .usage(vk::BufferUsageFlags::VERTEX_BUFFER)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .build();

        let buffer = unsafe { device.create_buffer(&buffer_create_info, None) }.unwrap();

        let memory_req = unsafe { device.get_buffer_memory_requirements(buffer) };

        let memory_index = get_memory_type_index(
            device_memory_properties,
            memory_req.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );

        let allocate_info = vk::MemoryAllocateInfo {
            allocation_size: memory_req.size,
            memory_type_index: memory_index,
            ..Default::default()
        };

        let memory = unsafe { device.allocate_memory(&allocate_info, None).unwrap() };

        unsafe { device.bind_buffer_memory(buffer, memory, 0) }.unwrap();

        let mapped_ptr = unsafe {
            device.map_memory(
                memory,
                0,
                vertex_buffer_size as u64,
                vk::MemoryMapFlags::empty(),
            )
        }
        .unwrap();

        let mut mapped_slice = unsafe {
            Align::new(
                mapped_ptr,
                std::mem::align_of::<Vertex>() as u64,
                vertex_buffer_size as u64,
            )
        };
        mapped_slice.copy_from_slice(&vertices);
        unsafe {
            device.unmap_memory(memory);
        }
        (vertex_count, vertex_stride, buffer, memory)
    };

    let (index_count, index_buffer, index_memory) = {
        let indices = [0u16, 1, 2];

        let index_count = indices.len();
        let index_buffer_size = std::mem::size_of::<u16>() * index_count;

        let buffer_create_info = vk::BufferCreateInfo::builder()
            .size(index_buffer_size as u64)
            .usage(vk::BufferUsageFlags::INDEX_BUFFER)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .build();

        let buffer = unsafe { device.create_buffer(&buffer_create_info, None) }.unwrap();

        let memory_req = unsafe { device.get_buffer_memory_requirements(buffer) };

        let memory_index = get_memory_type_index(
            device_memory_properties,
            memory_req.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );

        let allocate_info = vk::MemoryAllocateInfo {
            allocation_size: memory_req.size,
            memory_type_index: memory_index,
            ..Default::default()
        };

        let memory = unsafe { device.allocate_memory(&allocate_info, None).unwrap() };

        unsafe { device.bind_buffer_memory(buffer, memory, 0) }.unwrap();

        let mapped_ptr = unsafe {
            device.map_memory(
                memory,
                0,
                index_buffer_size as u64,
                vk::MemoryMapFlags::empty(),
            )
        }
        .unwrap();

        let mut mapped_slice = unsafe {
            Align::new(
                mapped_ptr,
                std::mem::align_of::<u16>() as u64,
                index_buffer_size as u64,
            )
        };
        mapped_slice.copy_from_slice(&indices);
        unsafe {
            device.unmap_memory(memory);
        }
        (index_count, buffer, memory)
    };

    let geometry = vec![vk::GeometryNV::builder()
        .geometry_type(vk::GeometryTypeNV::TRIANGLES)
        .geometry(
            vk::GeometryDataNV::builder()
                .triangles(
                    vk::GeometryTrianglesNV::builder()
                        .vertex_data(vertex_buffer)
                        .vertex_offset(0)
                        .vertex_count(vertex_count as u32)
                        .vertex_stride(vertex_stride as u64)
                        .vertex_format(vk::Format::R32G32B32_SFLOAT)
                        .index_data(index_buffer)
                        .index_offset(0)
                        .index_count(index_count as u32)
                        .index_type(vk::IndexType::UINT16)
                        .build(),
                )
                .build(),
        )
        .flags(vk::GeometryFlagsNV::OPAQUE)
        .build()];

    // Create bottom-level acceleration structure

    let bottom_as = {
        let accel_info = vk::AccelerationStructureCreateInfoNV::builder()
            .compacted_size(0)
            .info(
                vk::AccelerationStructureInfoNV::builder()
                    .ty(vk::AccelerationStructureTypeNV::BOTTOM_LEVEL)
                    .geometries(&geometry)
                    // .flags(vk::BuildAccelerationStructureFlagsNV::PREFER_FAST_TRACE)
                    .build(),
            )
            .build();

        unsafe { ray_tracing.create_acceleration_structure(&accel_info, None) }.unwrap()
    };

    let memory_requirements = unsafe {
        ray_tracing.get_acceleration_structure_memory_requirements(
            &vk::AccelerationStructureMemoryRequirementsInfoNV::builder()
                .acceleration_structure(bottom_as)
                .ty(vk::AccelerationStructureMemoryRequirementsTypeNV::OBJECT)
                .build(),
        )
    };

    let bottom_as_memory = unsafe {
        device.allocate_memory(
            &vk::MemoryAllocateInfo::builder()
                .allocation_size(memory_requirements.memory_requirements.size)
                .memory_type_index(get_memory_type_index(
                    device_memory_properties,
                    memory_requirements.memory_requirements.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                ))
                .build(),
            None,
        )
    }
    .unwrap();

    unsafe {
        ray_tracing.bind_acceleration_structure_memory(&[
            vk::BindAccelerationStructureMemoryInfoNV::builder()
                .acceleration_structure(bottom_as)
                .memory(bottom_as_memory)
                .build(),
        ])
    }
    .unwrap();

    let accel_handle = unsafe { ray_tracing.get_acceleration_structure_handle(bottom_as) }.unwrap();

    let (instance_count, instance_buffer, instance_memory) = {
        let transform_0: [f32; 12] = [1.0, 0.0, 0.0, -1.5, 0.0, 1.0, 0.0, 1.1, 0.0, 0.0, 1.0, 0.0];

        let transform_1: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.1, 0.0, 0.0, 1.0, 0.0];

        let transform_2: [f32; 12] = [1.0, 0.0, 0.0, 1.5, 0.0, 1.0, 0.0, 1.1, 0.0, 0.0, 1.0, 0.0];

        let instances = vec![
            GeometryInstance::new(
                transform_0,
                0, /* instance id */
                0xff,
                0,
                vk::GeometryInstanceFlagsNV::TRIANGLE_CULL_DISABLE_NV,
                accel_handle,
            ),
            GeometryInstance::new(
                transform_1,
                1, /* instance id */
                0xff,
                0,
                vk::GeometryInstanceFlagsNV::TRIANGLE_CULL_DISABLE_NV,
                accel_handle,
            ),
            GeometryInstance::new(
                transform_2,
                2, /* instance id */
                0xff,
                0,
                vk::GeometryInstanceFlagsNV::TRIANGLE_CULL_DISABLE_NV,
                accel_handle,
            ),
        ];

        let instance_buffer_size = std::mem::size_of::<GeometryInstance>() * instances.len();

        let buffer_create_info = vk::BufferCreateInfo::builder()
            .size(instance_buffer_size as u64)
            .usage(vk::BufferUsageFlags::RAY_TRACING_NV)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .build();

        let buffer = unsafe { device.create_buffer(&buffer_create_info, None) }.unwrap();

        let memory_req = unsafe { device.get_buffer_memory_requirements(buffer) };

        let memory_index = get_memory_type_index(
            device_memory_properties,
            memory_req.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        );

        let allocate_info = vk::MemoryAllocateInfo {
            allocation_size: memory_req.size,
            memory_type_index: memory_index,
            ..Default::default()
        };

        let memory = unsafe { device.allocate_memory(&allocate_info, None).unwrap() };

        unsafe { device.bind_buffer_memory(buffer, memory, 0) }.unwrap();

        let mapped_ptr = unsafe {
            device.map_memory(
                memory,
                0,
                instance_buffer_size as u64,
                vk::MemoryMapFlags::empty(),
            )
        }
        .unwrap();

        let mut mapped_slice = unsafe {
            Align::new(
                mapped_ptr,
                std::mem::align_of::<GeometryInstance>() as u64,
                instance_buffer_size as u64,
            )
        };
        mapped_slice.copy_from_slice(&instances);
        unsafe {
            device.unmap_memory(memory);
        }
        (instances.len(), buffer, memory)
    };

    let top_as = {
        let accel_info = vk::AccelerationStructureCreateInfoNV::builder()
            .compacted_size(0)
            .info(
                vk::AccelerationStructureInfoNV::builder()
                    .ty(vk::AccelerationStructureTypeNV::TOP_LEVEL)
                    .instance_count(instance_count as u32)
                    .build(),
            )
            .build();

        unsafe { ray_tracing.create_acceleration_structure(&accel_info, None) }.unwrap()
    };

    let memory_requirements = unsafe {
        ray_tracing.get_acceleration_structure_memory_requirements(
            &vk::AccelerationStructureMemoryRequirementsInfoNV::builder()
                .acceleration_structure(top_as)
                .ty(vk::AccelerationStructureMemoryRequirementsTypeNV::OBJECT)
                .build(),
        )
    };

    let top_as_memory = unsafe {
        device.allocate_memory(
            &vk::MemoryAllocateInfo::builder()
                .allocation_size(memory_requirements.memory_requirements.size)
                .memory_type_index(get_memory_type_index(
                    device_memory_properties,
                    memory_requirements.memory_requirements.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                ))
                .build(),
            None,
        )
    }
    .unwrap();

    unsafe {
        ray_tracing.bind_acceleration_structure_memory(&[
            vk::BindAccelerationStructureMemoryInfoNV::builder()
                .acceleration_structure(top_as)
                .memory(top_as_memory)
                .build(),
        ])
    }
    .unwrap();

    // Build acceleration structures

    let bottom_as_size = {
        let requirements = unsafe {
            ray_tracing.get_acceleration_structure_memory_requirements(
                &vk::AccelerationStructureMemoryRequirementsInfoNV::builder()
                    .acceleration_structure(bottom_as)
                    .ty(vk::AccelerationStructureMemoryRequirementsTypeNV::BUILD_SCRATCH)
                    .build(),
            )
        };
        requirements.memory_requirements.size
    };

    let top_as_size = {
        let requirements = unsafe {
            ray_tracing.get_acceleration_structure_memory_requirements(
                &vk::AccelerationStructureMemoryRequirementsInfoNV::builder()
                    .acceleration_structure(top_as)
                    .ty(vk::AccelerationStructureMemoryRequirementsTypeNV::BUILD_SCRATCH)
                    .build(),
            )
        };
        requirements.memory_requirements.size
    };

    let (scratch_buffer, scratch_memory) = {
        let scratch_buffer_size = std::cmp::max(bottom_as_size, top_as_size);

        let buffer_create_info = vk::BufferCreateInfo::builder()
            .size(scratch_buffer_size as u64)
            .usage(vk::BufferUsageFlags::RAY_TRACING_NV)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .build();

        let buffer = unsafe { device.create_buffer(&buffer_create_info, None) }.unwrap();

        let memory_req = unsafe { device.get_buffer_memory_requirements(buffer) };

        let memory_index = get_memory_type_index(
            device_memory_properties,
            memory_req.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        );

        let allocate_info = vk::MemoryAllocateInfo {
            allocation_size: memory_req.size,
            memory_type_index: memory_index,
            ..Default::default()
        };

        let memory = unsafe { device.allocate_memory(&allocate_info, None).unwrap() };

        unsafe { device.bind_buffer_memory(buffer, memory, 0) }.unwrap();

        (buffer, memory)
    };

    // Build

    let build_command_buffer = {
        let allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(1)
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .build();

        let command_buffers = unsafe { device.allocate_command_buffers(&allocate_info) }.unwrap();
        command_buffers[0]
    };

    unsafe {
        device.begin_command_buffer(
            build_command_buffer,
            &vk::CommandBufferBeginInfo::builder()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
                .build(),
        )
    }
    .unwrap();

    let memory_barrier = vk::MemoryBarrier::builder()
        .src_access_mask(
            vk::AccessFlags::ACCELERATION_STRUCTURE_WRITE_NV
                | vk::AccessFlags::ACCELERATION_STRUCTURE_READ_NV,
        )
        .dst_access_mask(
            vk::AccessFlags::ACCELERATION_STRUCTURE_WRITE_NV
                | vk::AccessFlags::ACCELERATION_STRUCTURE_READ_NV,
        )
        .build();

    unsafe {
        ray_tracing.cmd_build_acceleration_structure(
            build_command_buffer,
            &vk::AccelerationStructureInfoNV::builder()
                .ty(vk::AccelerationStructureTypeNV::BOTTOM_LEVEL)
                .geometries(&geometry)
                .build(),
            vk::Buffer::null(),
            0,
            false,
            bottom_as,
            vk::AccelerationStructureNV::null(),
            scratch_buffer,
            0,
        );
    }

    unsafe {
        device.cmd_pipeline_barrier(
            build_command_buffer,
            vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_NV,
            vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_NV,
            vk::DependencyFlags::empty(),
            &[memory_barrier],
            &[],
            &[],
        );
    }

    unsafe {
        ray_tracing.cmd_build_acceleration_structure(
            build_command_buffer,
            &vk::AccelerationStructureInfoNV::builder()
                .ty(vk::AccelerationStructureTypeNV::TOP_LEVEL)
                .instance_count(instance_count as u32)
                .build(),
            instance_buffer,
            0,
            false,
            top_as,
            vk::AccelerationStructureNV::null(),
            scratch_buffer,
            0,
        );
    }

    unsafe {
        device.cmd_pipeline_barrier(
            build_command_buffer,
            vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_NV,
            vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_NV,
            vk::DependencyFlags::empty(),
            &[memory_barrier],
            &[],
            &[],
        );
    }

    unsafe {
        device.end_command_buffer(build_command_buffer).unwrap();
    }

    unsafe {
        device
            .queue_submit(
                graphics_queue,
                &[vk::SubmitInfo::builder()
                    .command_buffers(&[build_command_buffer])
                    .build()],
                vk::Fence::null(),
            )
            .expect("queue submit failed.");
    }

    unsafe {
        device.queue_wait_idle(graphics_queue).unwrap();
        device.free_command_buffers(command_pool, &[build_command_buffer]);
    }

    // render pass

    let render_pass = {
        let color_attachment = vk::AttachmentDescription {
            flags: vk::AttachmentDescriptionFlags::empty(),
            format: COLOR_FORMAT,
            samples: vk::SampleCountFlags::TYPE_1,
            load_op: vk::AttachmentLoadOp::CLEAR,
            store_op: vk::AttachmentStoreOp::STORE,
            stencil_load_op: vk::AttachmentLoadOp::DONT_CARE,
            stencil_store_op: vk::AttachmentStoreOp::DONT_CARE,
            initial_layout: vk::ImageLayout::UNDEFINED,
            final_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
        };

        let color_attachment_ref = vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        };

        let subpass = vk::SubpassDescription::builder()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&[color_attachment_ref])
            .build();

        let renderpass_create_info = vk::RenderPassCreateInfo::builder()
            .attachments(&[color_attachment])
            .subpasses(&[subpass])
            .build();

        unsafe { device.create_render_pass(&renderpass_create_info, None) }
            .expect("Failed to create render pass!")
    };

    let (descriptor_set_layout, graphics_pipeline, pipeline_layout) = {
        let mut binding_flags = vk::DescriptorSetLayoutBindingFlagsCreateInfoEXT::builder()
            .binding_flags(&[
                vk::DescriptorBindingFlagsEXT::empty(),
                vk::DescriptorBindingFlagsEXT::empty(),
                vk::DescriptorBindingFlagsEXT::empty(),
            ])
            .build();

        let descriptor_set_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::builder()
                    .bindings(&[
                        vk::DescriptorSetLayoutBinding::builder()
                            .descriptor_count(1)
                            .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_NV)
                            .stage_flags(vk::ShaderStageFlags::RAYGEN_NV)
                            .binding(0)
                            .build(),
                        vk::DescriptorSetLayoutBinding::builder()
                            .descriptor_count(1)
                            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                            .stage_flags(vk::ShaderStageFlags::RAYGEN_NV)
                            .binding(1)
                            .build(),
                        vk::DescriptorSetLayoutBinding::builder()
                            .descriptor_count(1)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .stage_flags(vk::ShaderStageFlags::CLOSEST_HIT_NV)
                            .binding(2)
                            .build(),
                    ])
                    .push_next(&mut binding_flags)
                    .build(),
                None,
            )
        }
        .unwrap();

        const SHADER: &[u8] = include_bytes!(env!("ash_raytracing_example_shader.spv"));

        let shader_module = unsafe { create_shader_module(&device, SHADER).unwrap() };

        let layouts = vec![descriptor_set_layout];
        let layout_create_info = vk::PipelineLayoutCreateInfo::builder().set_layouts(&layouts);

        let pipeline_layout =
            unsafe { device.create_pipeline_layout(&layout_create_info, None) }.unwrap();

        let shader_groups = vec![
            // group0 = [ raygen ]
            vk::RayTracingShaderGroupCreateInfoNV::builder()
                .ty(vk::RayTracingShaderGroupTypeNV::GENERAL)
                .general_shader(0)
                .closest_hit_shader(vk::SHADER_UNUSED_NV)
                .any_hit_shader(vk::SHADER_UNUSED_NV)
                .intersection_shader(vk::SHADER_UNUSED_NV)
                .build(),
            // group1 = [ chit ]
            vk::RayTracingShaderGroupCreateInfoNV::builder()
                .ty(vk::RayTracingShaderGroupTypeNV::TRIANGLES_HIT_GROUP)
                .general_shader(vk::SHADER_UNUSED_NV)
                .closest_hit_shader(1)
                .any_hit_shader(vk::SHADER_UNUSED_NV)
                .intersection_shader(vk::SHADER_UNUSED_NV)
                .build(),
            // group2 = [ miss ]
            vk::RayTracingShaderGroupCreateInfoNV::builder()
                .ty(vk::RayTracingShaderGroupTypeNV::GENERAL)
                .general_shader(2)
                .closest_hit_shader(vk::SHADER_UNUSED_NV)
                .any_hit_shader(vk::SHADER_UNUSED_NV)
                .intersection_shader(vk::SHADER_UNUSED_NV)
                .build(),
        ];

        let shader_stages = vec![
            vk::PipelineShaderStageCreateInfo::builder()
                .stage(vk::ShaderStageFlags::RAYGEN_NV)
                // .module(rgen_shader_module)
                // .name(std::ffi::CStr::from_bytes_with_nul(b"main\0").unwrap())
                .module(shader_module)
                .name(std::ffi::CStr::from_bytes_with_nul(b"main_ray_generation\0").unwrap())
                .build(),
            vk::PipelineShaderStageCreateInfo::builder()
                .stage(vk::ShaderStageFlags::CLOSEST_HIT_NV)
                // .module(chit_shader_module)
                // .name(std::ffi::CStr::from_bytes_with_nul(b"main\0").unwrap())
                .module(shader_module)
                .name(std::ffi::CStr::from_bytes_with_nul(b"main_closest_hit\0").unwrap())
                .build(),
            vk::PipelineShaderStageCreateInfo::builder()
                .stage(vk::ShaderStageFlags::MISS_NV)
                .module(shader_module)
                .name(std::ffi::CStr::from_bytes_with_nul(b"main_miss\0").unwrap())
                // .module(miss_shader_module)
                // .name(std::ffi::CStr::from_bytes_with_nul(b"main\0").unwrap())
                .build(),
        ];

        let pipeline = unsafe {
            ray_tracing.create_ray_tracing_pipelines(
                vk::PipelineCache::null(),
                &[vk::RayTracingPipelineCreateInfoNV::builder()
                    .stages(&shader_stages)
                    .groups(&shader_groups)
                    .max_recursion_depth(1)
                    .layout(pipeline_layout)
                    .build()],
                None,
            )
        }
        .unwrap()[0];

        (descriptor_set_layout, pipeline, pipeline_layout)
    };

    let framebuffer = {
        let framebuffer_create_info = vk::FramebufferCreateInfo::builder()
            .render_pass(render_pass)
            .attachments(&[image_view])
            .width(WIDTH)
            .height(HEIGHT)
            .layers(1)
            .build();

        unsafe { device.create_framebuffer(&framebuffer_create_info, None) }
            .expect("Failed to create Framebuffer!")
    };

    let command_buffer = {
        let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_buffer_count(1)
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .build();

        unsafe { device.allocate_command_buffers(&command_buffer_allocate_info) }
            .expect("Failed to allocate Command Buffers!")[0]
    };

    {
        let command_buffer_begin_info = vk::CommandBufferBeginInfo::builder()
            .flags(vk::CommandBufferUsageFlags::SIMULTANEOUS_USE)
            .build();

        unsafe { device.begin_command_buffer(command_buffer, &command_buffer_begin_info) }
            .expect("Failed to begin recording Command Buffer at beginning!");
    }

    let (shader_binding_table_buffer, shader_binding_table_memory) = {
        let group_count = 3; // Listed in vk::RayTracingPipelineCreateInfoNV
                             // let table_size = (rt_properties.shader_group_handle_size * group_count) as u64;
        let handle_size_aligned = aligned_size(
            rt_properties.shader_group_handle_size,
            rt_properties.shader_group_base_alignment,
        );
        let table_size = (aligned_size(
            rt_properties.shader_group_handle_size,
            rt_properties.shader_group_base_alignment,
        ) * group_count) as u64;
        let mut table_data: Vec<u8> = vec![0u8; table_size as usize];
        unsafe {
            ray_tracing
                .get_ray_tracing_shader_group_handles(graphics_pipeline, 0, 1, &mut table_data)
                .unwrap();

            ray_tracing
                .get_ray_tracing_shader_group_handles(
                    graphics_pipeline,
                    1,
                    1,
                    &mut table_data[handle_size_aligned as usize..],
                )
                .unwrap();

            ray_tracing
                .get_ray_tracing_shader_group_handles(
                    graphics_pipeline,
                    2,
                    1,
                    &mut table_data[2 * handle_size_aligned as usize..],
                )
                .unwrap();
        }
        let buffer_create_info = vk::BufferCreateInfo::builder()
            .size(table_size as u64)
            .usage(vk::BufferUsageFlags::TRANSFER_SRC)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .build();

        let buffer = unsafe { device.create_buffer(&buffer_create_info, None) }.unwrap();

        let memory_req = unsafe { device.get_buffer_memory_requirements(buffer) };

        let memory_index = get_memory_type_index(
            device_memory_properties,
            memory_req.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE,
        );

        let allocate_info = vk::MemoryAllocateInfo {
            allocation_size: memory_req.size,
            memory_type_index: memory_index,
            ..Default::default()
        };

        let memory = unsafe { device.allocate_memory(&allocate_info, None).unwrap() };

        unsafe { device.bind_buffer_memory(buffer, memory, 0) }.unwrap();

        let mapped_ptr =
            unsafe { device.map_memory(memory, 0, table_size as u64, vk::MemoryMapFlags::empty()) }
                .unwrap();

        let mut mapped_slice = unsafe {
            Align::new(
                mapped_ptr,
                std::mem::align_of::<u8>() as u64,
                table_size as u64,
            )
        };
        mapped_slice.copy_from_slice(&table_data);
        unsafe {
            device.unmap_memory(memory);
        }
        (buffer, memory)
    };

    let color_buffer = {
        /*
        let color0: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        let color1: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
        let color2: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
        */

        let color: [f32; 12] = [1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0];

        let buffer_size = (std::mem::size_of::<f32>() * 12) as vk::DeviceSize;

        let mut color_buffer = BufferResource::new(
            buffer_size,
            vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE,
            &device,
            device_memory_properties,
        );
        color_buffer.store(&color, &device);
        /*
        let mut color0_buffer = BufferResource::new(
            buffer_size,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE,
            &device,
            device_memory_properties,
        );
        color0_buffer.store(&color0, &device);

        let mut color1_buffer = BufferResource::new(
            buffer_size,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE,
            &device,
            device_memory_properties,
        );
        color1_buffer.store(&color1, &device);

        let mut color2_buffer = BufferResource::new(
            buffer_size,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE,
            &device,
            device_memory_properties,
        );
        color2_buffer.store(&color2, &device);
        */

        color_buffer
    };

    let descriptor_sizes = [
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::ACCELERATION_STRUCTURE_NV,
            descriptor_count: 1,
        },
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::STORAGE_IMAGE,
            descriptor_count: 1,
        },
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::STORAGE_BUFFER,
            descriptor_count: 1,
        },
    ];

    let descriptor_pool_info = vk::DescriptorPoolCreateInfo::builder()
        .pool_sizes(&descriptor_sizes)
        .max_sets(1);

    let descriptor_pool =
        unsafe { device.create_descriptor_pool(&descriptor_pool_info, None) }.unwrap();

    let mut count_allocate_info = vk::DescriptorSetVariableDescriptorCountAllocateInfo::builder()
        .descriptor_counts(&[1])
        .build();

    let descriptor_sets = unsafe {
        device.allocate_descriptor_sets(
            &vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(descriptor_pool)
                .set_layouts(&[descriptor_set_layout])
                .push_next(&mut count_allocate_info)
                .build(),
        )
    }
    .unwrap();

    let descriptor_set = descriptor_sets[0];

    let accel_structs = [top_as];
    let mut accel_info = vk::WriteDescriptorSetAccelerationStructureNV::builder()
        .acceleration_structures(&accel_structs)
        .build();

    let mut accel_write = vk::WriteDescriptorSet::builder()
        .dst_set(descriptor_set)
        .dst_binding(0)
        .dst_array_element(0)
        .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_NV)
        .push_next(&mut accel_info)
        .build();

    // This is only set by the builder for images, buffers, or views; need to set explicitly after
    accel_write.descriptor_count = 1;

    let image_info = [vk::DescriptorImageInfo::builder()
        .image_layout(vk::ImageLayout::GENERAL)
        .image_view(image_view)
        .build()];

    let image_write = vk::WriteDescriptorSet::builder()
        .dst_set(descriptor_set)
        .dst_binding(1)
        .dst_array_element(0)
        .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
        .image_info(&image_info)
        .build();

    let buffer = color_buffer.buffer;
    /*
    let buffer0 = color0_buffer.buffer;
    let buffer1 = color1_buffer.buffer;
    let buffer2 = color2_buffer.buffer;
    */

    let buffer_info = [
        vk::DescriptorBufferInfo::builder()
            .buffer(buffer)
            .range(vk::WHOLE_SIZE)
            .build(),
        /*
        vk::DescriptorBufferInfo::builder()
            .buffer(buffer0)
            .range(vk::WHOLE_SIZE)
            .build(),
        vk::DescriptorBufferInfo::builder()
            .buffer(buffer1)
            .range(vk::WHOLE_SIZE)
            .build(),
        vk::DescriptorBufferInfo::builder()
            .buffer(buffer2)
            .range(vk::WHOLE_SIZE)
            .build(),
            */
    ];

    let buffers_write = vk::WriteDescriptorSet::builder()
        .dst_set(descriptor_set)
        .dst_binding(2)
        .dst_array_element(0)
        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
        .buffer_info(&buffer_info)
        .build();

    unsafe {
        device.update_descriptor_sets(&[accel_write, image_write, buffers_write], &[]);
    }

    {
        let handle_size = rt_properties.shader_group_handle_size as u64;

        let handle_size_aligned = aligned_size(
            rt_properties.shader_group_handle_size,
            rt_properties.shader_group_base_alignment,
        ) as u64;

        // |[ raygen shader ]|[ hit shader  ]|[ miss shader ]|
        // |                 |               |               |
        // | 0               | 1             | 2             | 3

        let sbt_raygen_buffer = shader_binding_table_buffer;
        let sbt_raygen_offset = 0;

        let sbt_miss_buffer = shader_binding_table_buffer;
        let sbt_miss_offset = 2 * handle_size_aligned;
        let sbt_miss_stride = handle_size_aligned;

        let sbt_hit_buffer = shader_binding_table_buffer;
        let sbt_hit_offset = 1 * handle_size_aligned;
        let sbt_hit_stride = handle_size_aligned;

        let sbt_call_buffer = vk::Buffer::null();
        let sbt_call_offset = 0;
        let sbt_call_stride = 0;

        unsafe {
            device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::RAY_TRACING_NV,
                graphics_pipeline,
            );
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::RAY_TRACING_NV,
                pipeline_layout,
                0,
                &[descriptor_set],
                &[],
            );
            ray_tracing.cmd_trace_rays(
                command_buffer,
                sbt_raygen_buffer,
                sbt_raygen_offset,
                sbt_miss_buffer,
                sbt_miss_offset,
                sbt_miss_stride,
                sbt_hit_buffer,
                sbt_hit_offset,
                sbt_hit_stride,
                sbt_call_buffer,
                sbt_call_offset,
                sbt_call_stride,
                WIDTH,
                HEIGHT,
                1,
            );
            device.end_command_buffer(command_buffer).unwrap();
        }
    }

    let fence = {
        let fence_create_info = vk::FenceCreateInfo::builder()
            .flags(vk::FenceCreateFlags::SIGNALED)
            .build();

        unsafe { device.create_fence(&fence_create_info, None) }
            .expect("Failed to create Fence Object!")
    };

    {
        let submit_infos = [vk::SubmitInfo::builder()
            .command_buffers(&[command_buffer])
            .build()];

        unsafe {
            device
                .reset_fences(&[fence])
                .expect("Failed to reset Fence!");

            device
                .queue_submit(graphics_queue, &submit_infos, fence)
                .expect("Failed to execute queue submit.");

            device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
        }
    }

    // transfer to host

    let dst_image = {
        let dst_image_create_info = vk::ImageCreateInfo::builder()
            .image_type(vk::ImageType::TYPE_2D)
            .format(COLOR_FORMAT)
            .extent(
                vk::Extent3D::builder()
                    .width(WIDTH)
                    .height(HEIGHT)
                    .depth(1)
                    .build(),
            )
            .mip_levels(1)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::LINEAR)
            .usage(vk::ImageUsageFlags::TRANSFER_DST)
            .build();

        unsafe { device.create_image(&dst_image_create_info, None) }.unwrap()
    };

    let dst_device_memory = {
        let dst_mem_reqs = unsafe { device.get_image_memory_requirements(dst_image) };
        let dst_mem_alloc_info = vk::MemoryAllocateInfo::builder()
            .allocation_size(dst_mem_reqs.size)
            .memory_type_index(get_memory_type_index(
                device_memory_properties,
                dst_mem_reqs.memory_type_bits,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            ));

        unsafe { device.allocate_memory(&dst_mem_alloc_info, None) }.unwrap()
    };
    unsafe { device.bind_image_memory(dst_image, dst_device_memory, 0) }.unwrap();

    let copy_cmd = {
        let allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1)
            .build();

        unsafe { device.allocate_command_buffers(&allocate_info) }.unwrap()[0]
    };

    {
        let cmd_begin_info = vk::CommandBufferBeginInfo::builder().build();

        unsafe { device.begin_command_buffer(copy_cmd, &cmd_begin_info) }.unwrap();
    }

    {
        let image_barrier = vk::ImageMemoryBarrier::builder()
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .image(dst_image)
            .subresource_range(
                vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1)
                    .build(),
            )
            .build();

        unsafe {
            device.cmd_pipeline_barrier(
                copy_cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[image_barrier],
            );
        }
    }

    {
        let copy_region = vk::ImageCopy::builder()
            .src_subresource(
                vk::ImageSubresourceLayers::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1)
                    .build(),
            )
            .dst_subresource(
                vk::ImageSubresourceLayers::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1)
                    .build(),
            )
            .extent(
                vk::Extent3D::builder()
                    .width(WIDTH)
                    .height(HEIGHT)
                    .depth(1)
                    .build(),
            )
            .build();

        unsafe {
            device.cmd_copy_image(
                copy_cmd,
                image,
                vk::ImageLayout::GENERAL,
                dst_image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[copy_region],
            );
        }
    }

    {
        let image_barrier = vk::ImageMemoryBarrier::builder()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::MEMORY_READ)
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .image(dst_image)
            .subresource_range(
                vk::ImageSubresourceRange::builder()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1)
                    .build(),
            )
            .build();

        unsafe {
            device.cmd_pipeline_barrier(
                copy_cmd,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[image_barrier],
            );
        }
    }

    {
        let submit_infos = [vk::SubmitInfo {
            s_type: vk::StructureType::SUBMIT_INFO,
            p_next: ptr::null(),
            wait_semaphore_count: 0,
            p_wait_semaphores: null(),
            p_wait_dst_stage_mask: null(),
            command_buffer_count: 1,
            p_command_buffers: &copy_cmd,
            signal_semaphore_count: 0,
            p_signal_semaphores: null(),
        }];

        unsafe {
            device.end_command_buffer(copy_cmd).unwrap();

            device
                .reset_fences(&[fence])
                .expect("Failed to reset Fence!");

            device
                .queue_submit(graphics_queue, &submit_infos, fence)
                .expect("Failed to execute queue submit.");

            device.wait_for_fences(&[fence], true, u64::MAX).unwrap();
        }
    }

    let subresource_layout = {
        let subresource = vk::ImageSubresource::builder()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .build();

        unsafe { device.get_image_subresource_layout(dst_image, subresource) }
    };

    let data: *const u8 = unsafe {
        device
            .map_memory(
                dst_device_memory,
                0,
                vk::WHOLE_SIZE,
                vk::MemoryMapFlags::empty(),
            )
            .unwrap() as _
    };

    let mut data = unsafe { data.offset(subresource_layout.offset as isize) };

    let mut png_encoder = png::Encoder::new(File::create("out.png").unwrap(), WIDTH, HEIGHT);

    png_encoder.set_depth(png::BitDepth::Eight);
    png_encoder.set_color(png::ColorType::RGBA);

    let mut png_writer = png_encoder
        .write_header()
        .unwrap()
        .into_stream_writer_with_size((4 * WIDTH) as usize);

    for _ in 0..HEIGHT {
        let row = unsafe { std::slice::from_raw_parts(data, 4 * WIDTH as usize) };
        png_writer.write_all(row).unwrap();
        data = unsafe { data.offset(subresource_layout.row_pitch as isize) };
    }

    png_writer.finish().unwrap();

    unsafe {
        device.unmap_memory(dst_device_memory);
        device.free_memory(dst_device_memory, None);
        device.destroy_image(dst_image, None);
    }

    // clean up

    unsafe {
        device.destroy_fence(fence, None);
    }

    unsafe {
        device.destroy_command_pool(command_pool, None);
    }

    unsafe { device.destroy_framebuffer(framebuffer, None) };

    unsafe {
        device.destroy_pipeline(graphics_pipeline, None);
    }

    unsafe {
        device.destroy_pipeline_layout(pipeline_layout, None);
    }

    unsafe {
        device.destroy_render_pass(render_pass, None);
    }

    unsafe {
        device.destroy_image_view(image_view, None);
        device.destroy_image(image, None);
        device.free_memory(device_memory, None);
    }

    unsafe {
        device.destroy_device(None);
    }
}

fn check_validation_layer_support<'a>(
    entry: &ash::Entry,
    required_validation_layers: impl IntoIterator<Item = &'a CStr>,
) -> VkResult<bool> {
    let supported_layers: HashSet<CString> = entry
        .enumerate_instance_layer_properties()?
        .into_iter()
        .map(|layer_property| unsafe {
            CStr::from_ptr(layer_property.layer_name.as_ptr()).to_owned()
        })
        .collect();

    Ok(required_validation_layers
        .into_iter()
        .all(|l| supported_layers.contains(l)))
}

fn pick_physical_device_and_queue_family_indices(
    instance: &ash::Instance,
    extensions: &[&CStr],
) -> VkResult<Option<(vk::PhysicalDevice, u32)>> {
    Ok(unsafe { instance.enumerate_physical_devices() }?
        .into_iter()
        .find_map(|physical_device| {
            if unsafe { instance.enumerate_device_extension_properties(physical_device) }.map(
                |exts| {
                    let set: HashSet<&CStr> = exts
                        .iter()
                        .map(|ext| unsafe { CStr::from_ptr(&ext.extension_name as *const c_char) })
                        .collect();

                    extensions.iter().all(|ext| set.contains(ext))
                },
            ) != Ok(true)
            {
                return None;
            }

            let graphics_family =
                unsafe { instance.get_physical_device_queue_family_properties(physical_device) }
                    .into_iter()
                    .enumerate()
                    .find(|(_, device_properties)| {
                        device_properties.queue_count > 0
                            && device_properties
                                .queue_flags
                                .contains(vk::QueueFlags::GRAPHICS)
                    });

            graphics_family.map(|(i, _)| (physical_device, i as u32))
        }))
}

unsafe fn create_shader_module(device: &ash::Device, code: &[u8]) -> VkResult<vk::ShaderModule> {
    let shader_module_create_info = vk::ShaderModuleCreateInfo {
        s_type: vk::StructureType::SHADER_MODULE_CREATE_INFO,
        p_next: ptr::null(),
        flags: vk::ShaderModuleCreateFlags::empty(),
        code_size: code.len(),
        p_code: code.as_ptr() as *const u32,
    };

    device.create_shader_module(&shader_module_create_info, None)
}

fn get_memory_type_index(
    device_memory_properties: vk::PhysicalDeviceMemoryProperties,
    mut type_bits: u32,
    properties: vk::MemoryPropertyFlags,
) -> u32 {
    for i in 0..device_memory_properties.memory_type_count {
        if (type_bits & 1) == 1 {
            if (device_memory_properties.memory_types[i as usize].property_flags & properties)
                == properties
            {
                return i;
            }
        }
        type_bits >>= 1;
    }
    0
}

pub unsafe extern "system" fn default_vulkan_debug_utils_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _p_user_data: *mut c_void,
) -> vk::Bool32 {
    let severity = match message_severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE => "[Verbose]",
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => "[Warning]",
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => "[Error]",
        vk::DebugUtilsMessageSeverityFlagsEXT::INFO => "[Info]",
        _ => "[Unknown]",
    };
    let types = match message_type {
        vk::DebugUtilsMessageTypeFlagsEXT::GENERAL => "[General]",
        vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE => "[Performance]",
        vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION => "[Validation]",
        _ => "[Unknown]",
    };
    let message = CStr::from_ptr((*p_callback_data).p_message);
    println!("[Debug]{}{}{:?}", severity, types, message);

    vk::FALSE
}

#[derive(Clone)]
struct BufferResource {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: vk::DeviceSize,
}

impl BufferResource {
    fn new(
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        memory_properties: vk::MemoryPropertyFlags,
        device: &ash::Device,
        device_memory_properties: vk::PhysicalDeviceMemoryProperties,
    ) -> Self {
        unsafe {
            let buffer_info = vk::BufferCreateInfo::builder()
                .size(size)
                .usage(usage)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .build();

            let buffer = device.create_buffer(&buffer_info, None).unwrap();

            let memory_req = device.get_buffer_memory_requirements(buffer);

            let memory_index = get_memory_type_index(
                device_memory_properties,
                memory_req.memory_type_bits,
                memory_properties,
            );

            let allocate_info = vk::MemoryAllocateInfo {
                allocation_size: memory_req.size,
                memory_type_index: memory_index,
                ..Default::default()
            };

            let memory = device.allocate_memory(&allocate_info, None).unwrap();

            device.bind_buffer_memory(buffer, memory, 0).unwrap();

            BufferResource {
                buffer,
                memory,
                size,
            }
        }
    }

    fn store<T: Copy>(&mut self, data: &[T], device: &ash::Device) {
        unsafe {
            let size = (std::mem::size_of::<T>() * data.len()) as u64;
            let mapped_ptr = self.map(size, device);
            let mut mapped_slice = Align::new(mapped_ptr, std::mem::align_of::<T>() as u64, size);
            mapped_slice.copy_from_slice(&data);
            self.unmap(device);
        }
    }

    fn map(&mut self, size: vk::DeviceSize, device: &ash::Device) -> *mut std::ffi::c_void {
        unsafe {
            let data: *mut std::ffi::c_void = device
                .map_memory(self.memory, 0, size, vk::MemoryMapFlags::empty())
                .unwrap();
            data
        }
    }

    fn unmap(&mut self, device: &ash::Device) {
        unsafe {
            device.unmap_memory(self.memory);
        }
    }
}

fn aligned_size(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) & !(alignment - 1)
}
