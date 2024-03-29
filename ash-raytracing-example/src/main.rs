use std::{
    collections::HashSet,
    ffi::{c_void, CStr, CString},
    fs::File,
    io::Write,
    os::raw::c_char,
    ptr::{self, null},
};

use ash::{
    prelude::VkResult,
    util::Align,
    vk::{self, Packed24_8},
};

#[repr(C)]
#[derive(Clone, Debug, Copy)]
struct Vertex {
    pos: [f32; 3],
}

fn main() {
    const ENABLE_VALIDATION_LAYER: bool = true;
    const WIDTH: u32 = 800;
    const HEIGHT: u32 = 600;
    const COLOR_FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

    let validation_layers: Vec<CString> = if ENABLE_VALIDATION_LAYER {
        vec![CString::new("VK_LAYER_KHRONOS_validation").unwrap()]
    } else {
        Vec::new()
    };
    let extension_names = if ENABLE_VALIDATION_LAYER {
        vec![vk::ExtDebugUtilsFn::name()]
    } else {
        Vec::new()
    };
    let validation_layers_ptr: Vec<*const i8> = validation_layers
        .iter()
        .map(|c_str| c_str.as_ptr())
        .collect();
    let extension_name_ptr = extension_names
        .iter()
        .map(|ext| ext.as_ptr())
        .collect::<Vec<_>>();

    let entry = unsafe { ash::Entry::load() }.unwrap();

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
            instance_create_info
                .enabled_extension_names(&extension_name_ptr)
                .push_next(&mut debug_utils_create_info)
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
        let priorities = [1.0];

        let queue_create_info = vk::DeviceQueueCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .queue_priorities(&priorities)
            .build();

        let mut features2 = vk::PhysicalDeviceFeatures2::default();
        unsafe {
            (instance.fp_v1_1().get_physical_device_features2)(physical_device, &mut features2)
        };

        let mut features12 = vk::PhysicalDeviceVulkan12Features::builder()
            .buffer_device_address(true)
            .vulkan_memory_model(true)
            .build();

        let mut as_feature = vk::PhysicalDeviceAccelerationStructureFeaturesKHR::builder()
            .acceleration_structure(true)
            .build();

        let mut raytracing_pipeline = vk::PhysicalDeviceRayTracingPipelineFeaturesKHR::builder()
            .ray_tracing_pipeline(true)
            .build();

        let enabled_extension_names = [
            ash::extensions::khr::RayTracingPipeline::name().as_ptr(),
            ash::extensions::khr::AccelerationStructure::name().as_ptr(),
            ash::extensions::khr::DeferredHostOperations::name().as_ptr(),
            vk::KhrSpirv14Fn::name().as_ptr(),
            vk::ExtScalarBlockLayoutFn::name().as_ptr(),
            vk::KhrGetMemoryRequirements2Fn::name().as_ptr(),
        ];

        let device_create_info = vk::DeviceCreateInfo::builder()
            .push_next(&mut features2)
            .push_next(&mut features12)
            .push_next(&mut as_feature)
            .push_next(&mut raytracing_pipeline)
            .queue_create_infos(&[queue_create_info])
            .enabled_extension_names(&enabled_extension_names)
            .build();

        unsafe { instance.create_device(physical_device, &device_create_info, None) }
            .expect("Failed to create logical Device!")
    };

    let mut rt_pipeline_properties = vk::PhysicalDeviceRayTracingPipelinePropertiesKHR::default();

    {
        let mut physical_device_properties2 = vk::PhysicalDeviceProperties2::builder()
            .push_next(&mut rt_pipeline_properties)
            .build();

        unsafe {
            instance
                .get_physical_device_properties2(physical_device, &mut physical_device_properties2);
        }
    }
    let acceleration_structure =
        ash::extensions::khr::AccelerationStructure::new(&instance, &device);

    let rt_pipeline = ash::extensions::khr::RayTracingPipeline::new(&instance, &device);

    let graphics_queue = unsafe { device.get_device_queue(queue_family_index, 0) };

    let command_pool = {
        let command_pool_create_info = vk::CommandPoolCreateInfo::builder()
            .queue_family_index(queue_family_index)
            .build();

        unsafe { device.create_command_pool(&command_pool_create_info, None) }
            .expect("Failed to create Command Pool!")
    };

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

    {
        let command_buffer = {
            let allocate_info = vk::CommandBufferAllocateInfo::builder()
                .command_buffer_count(1)
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .build();

            let command_buffers =
                unsafe { device.allocate_command_buffers(&allocate_info) }.unwrap();
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
        }

        let command_buffers = [command_buffer];

        let submit_infos = [vk::SubmitInfo::builder()
            .command_buffers(&command_buffers)
            .build()];

        unsafe {
            device
                .queue_submit(graphics_queue, &submit_infos, vk::Fence::null())
                .expect("Failed to execute queue submit.");

            device.queue_wait_idle(graphics_queue).unwrap();
            device.free_command_buffers(command_pool, &[command_buffer]);
        }
    }

    // acceleration structures

    let (vertex_count, vertex_stride, vertex_buffer) = {
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

        let mut vertex_buffer = BufferResource::new(
            vertex_buffer_size as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            &device,
            device_memory_properties,
        );

        vertex_buffer.store(&vertices, &device);

        (vertex_count, vertex_stride, vertex_buffer)
    };

    let (index_count, index_buffer) = {
        let indices: [u32; 3] = [0, 1, 2];

        let index_count = indices.len();
        let index_buffer_size = std::mem::size_of::<usize>() * index_count;

        let mut index_buffer = BufferResource::new(
            index_buffer_size as vk::DeviceSize,
            vk::BufferUsageFlags::INDEX_BUFFER
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            &device,
            device_memory_properties,
        );

        index_buffer.store(&indices, &device);
        (index_count, index_buffer)
    };

    let geometry = vk::AccelerationStructureGeometryKHR::builder()
        .geometry_type(vk::GeometryTypeKHR::TRIANGLES)
        .geometry(vk::AccelerationStructureGeometryDataKHR {
            triangles: vk::AccelerationStructureGeometryTrianglesDataKHR::builder()
                .vertex_data(vk::DeviceOrHostAddressConstKHR {
                    device_address: unsafe {
                        get_buffer_device_address(&device, vertex_buffer.buffer)
                    },
                })
                .max_vertex(vertex_count as u32 - 1)
                .vertex_stride(vertex_stride as u64)
                .vertex_format(vk::Format::R32G32B32_SFLOAT)
                .index_data(vk::DeviceOrHostAddressConstKHR {
                    device_address: unsafe {
                        get_buffer_device_address(&device, index_buffer.buffer)
                    },
                })
                .index_type(vk::IndexType::UINT32)
                .build(),
        })
        .flags(vk::GeometryFlagsKHR::OPAQUE)
        .build();

    // Create bottom-level acceleration structure

    let (bottom_as, bottom_as_buffer) = {
        let build_range_info = vk::AccelerationStructureBuildRangeInfoKHR::builder()
            .first_vertex(0)
            .primitive_count(index_count as u32 / 3)
            .primitive_offset(0)
            .transform_offset(0)
            .build();

        let geometries = [geometry];

        let mut build_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .geometries(&geometries)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .ty(vk::AccelerationStructureTypeKHR::BOTTOM_LEVEL)
            .build();

        let size_info = unsafe {
            acceleration_structure.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &build_info,
                &[index_count as u32 / 3],
            )
        };

        let bottom_as_buffer = BufferResource::new(
            size_info.acceleration_structure_size,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            &device,
            device_memory_properties,
        );

        let as_create_info = vk::AccelerationStructureCreateInfoKHR::builder()
            .ty(build_info.ty)
            .size(size_info.acceleration_structure_size)
            .buffer(bottom_as_buffer.buffer)
            .offset(0)
            .build();

        let bottom_as =
            unsafe { acceleration_structure.create_acceleration_structure(&as_create_info, None) }
                .unwrap();

        build_info.dst_acceleration_structure = bottom_as;

        let scratch_buffer = BufferResource::new(
            size_info.build_scratch_size,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            &device,
            device_memory_properties,
        );

        build_info.scratch_data = vk::DeviceOrHostAddressKHR {
            device_address: unsafe { get_buffer_device_address(&device, scratch_buffer.buffer) },
        };

        let build_command_buffer = {
            let allocate_info = vk::CommandBufferAllocateInfo::builder()
                .command_buffer_count(1)
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .build();

            let command_buffers =
                unsafe { device.allocate_command_buffers(&allocate_info) }.unwrap();
            command_buffers[0]
        };

        unsafe {
            device
                .begin_command_buffer(
                    build_command_buffer,
                    &vk::CommandBufferBeginInfo::builder()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
                        .build(),
                )
                .unwrap();

            acceleration_structure.cmd_build_acceleration_structures(
                build_command_buffer,
                &[build_info],
                &[&[build_range_info]],
            );
            device.end_command_buffer(build_command_buffer).unwrap();
            device
                .queue_submit(
                    graphics_queue,
                    &[vk::SubmitInfo::builder()
                        .command_buffers(&[build_command_buffer])
                        .build()],
                    vk::Fence::null(),
                )
                .expect("queue submit failed.");

            device.queue_wait_idle(graphics_queue).unwrap();
            device.free_command_buffers(command_pool, &[build_command_buffer]);
            scratch_buffer.destroy(&device);
        }
        (bottom_as, bottom_as_buffer)
    };

    let accel_handle = {
        let as_addr_info = vk::AccelerationStructureDeviceAddressInfoKHR::builder()
            .acceleration_structure(bottom_as)
            .build();
        unsafe { acceleration_structure.get_acceleration_structure_device_address(&as_addr_info) }
    };

    let (instance_count, instance_buffer) = {
        let transform_0: [f32; 12] = [1.0, 0.0, 0.0, -1.5, 0.0, 1.0, 0.0, 1.1, 0.0, 0.0, 1.0, 0.0];

        let transform_1: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.1, 0.0, 0.0, 1.0, 0.0];

        let transform_2: [f32; 12] = [1.0, 0.0, 0.0, 1.5, 0.0, 1.0, 0.0, 1.1, 0.0, 0.0, 1.0, 0.0];

        let instances = vec![
            vk::AccelerationStructureInstanceKHR {
                transform: vk::TransformMatrixKHR {
                    matrix: transform_0,
                },
                instance_custom_index_and_mask: Packed24_8::new(0, 0xff),
                instance_shader_binding_table_record_offset_and_flags: Packed24_8::new(
                    0,
                    vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8,
                ),
                acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                    device_handle: accel_handle,
                },
            },
            vk::AccelerationStructureInstanceKHR {
                transform: vk::TransformMatrixKHR {
                    matrix: transform_1,
                },
                instance_custom_index_and_mask: Packed24_8::new(1, 0xff),
                instance_shader_binding_table_record_offset_and_flags: Packed24_8::new(
                    0,
                    vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8,
                ),
                acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                    device_handle: accel_handle,
                },
            },
            vk::AccelerationStructureInstanceKHR {
                transform: vk::TransformMatrixKHR {
                    matrix: transform_2,
                },
                instance_custom_index_and_mask: Packed24_8::new(2, 0xff),
                instance_shader_binding_table_record_offset_and_flags: Packed24_8::new(
                    0,
                    vk::GeometryInstanceFlagsKHR::TRIANGLE_FACING_CULL_DISABLE.as_raw() as u8,
                ),
                acceleration_structure_reference: vk::AccelerationStructureReferenceKHR {
                    device_handle: accel_handle,
                },
            },
        ];

        let instance_buffer_size =
            std::mem::size_of::<vk::AccelerationStructureInstanceKHR>() * instances.len();

        let mut instance_buffer = BufferResource::new(
            instance_buffer_size as vk::DeviceSize,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::ACCELERATION_STRUCTURE_BUILD_INPUT_READ_ONLY_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            &device,
            device_memory_properties,
        );

        instance_buffer.store(&instances, &device);

        (instances.len(), instance_buffer)
    };

    let (top_as, top_as_buffer) = {
        let build_range_info = vk::AccelerationStructureBuildRangeInfoKHR::builder()
            .first_vertex(0)
            .primitive_count(instance_count as u32)
            .primitive_offset(0)
            .transform_offset(0)
            .build();

        let build_command_buffer = {
            let allocate_info = vk::CommandBufferAllocateInfo::builder()
                .command_buffer_count(1)
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .build();

            let command_buffers =
                unsafe { device.allocate_command_buffers(&allocate_info) }.unwrap();
            command_buffers[0]
        };

        unsafe {
            device
                .begin_command_buffer(
                    build_command_buffer,
                    &vk::CommandBufferBeginInfo::builder()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT)
                        .build(),
                )
                .unwrap();
            let memory_barrier = vk::MemoryBarrier::builder()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::ACCELERATION_STRUCTURE_WRITE_KHR)
                .build();
            device.cmd_pipeline_barrier(
                build_command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ACCELERATION_STRUCTURE_BUILD_KHR,
                vk::DependencyFlags::empty(),
                &[memory_barrier],
                &[],
                &[],
            );
        }

        let instances = vk::AccelerationStructureGeometryInstancesDataKHR::builder()
            .array_of_pointers(false)
            .data(vk::DeviceOrHostAddressConstKHR {
                device_address: unsafe {
                    get_buffer_device_address(&device, instance_buffer.buffer)
                },
            })
            .build();

        let geometry = vk::AccelerationStructureGeometryKHR::builder()
            .geometry_type(vk::GeometryTypeKHR::INSTANCES)
            .geometry(vk::AccelerationStructureGeometryDataKHR { instances })
            .build();

        let geometries = [geometry];

        let mut build_info = vk::AccelerationStructureBuildGeometryInfoKHR::builder()
            .flags(vk::BuildAccelerationStructureFlagsKHR::PREFER_FAST_TRACE)
            .geometries(&geometries)
            .mode(vk::BuildAccelerationStructureModeKHR::BUILD)
            .ty(vk::AccelerationStructureTypeKHR::TOP_LEVEL)
            .build();

        let size_info = unsafe {
            acceleration_structure.get_acceleration_structure_build_sizes(
                vk::AccelerationStructureBuildTypeKHR::DEVICE,
                &build_info,
                &[build_range_info.primitive_count],
            )
        };

        let top_as_buffer = BufferResource::new(
            size_info.acceleration_structure_size,
            vk::BufferUsageFlags::ACCELERATION_STRUCTURE_STORAGE_KHR
                | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            &device,
            device_memory_properties,
        );

        let as_create_info = vk::AccelerationStructureCreateInfoKHR::builder()
            .ty(build_info.ty)
            .size(size_info.acceleration_structure_size)
            .buffer(top_as_buffer.buffer)
            .offset(0)
            .build();

        let top_as =
            unsafe { acceleration_structure.create_acceleration_structure(&as_create_info, None) }
                .unwrap();

        build_info.dst_acceleration_structure = top_as;

        let scratch_buffer = BufferResource::new(
            size_info.build_scratch_size,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS | vk::BufferUsageFlags::STORAGE_BUFFER,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
            &device,
            device_memory_properties,
        );

        build_info.scratch_data = vk::DeviceOrHostAddressKHR {
            device_address: unsafe { get_buffer_device_address(&device, scratch_buffer.buffer) },
        };

        unsafe {
            acceleration_structure.cmd_build_acceleration_structures(
                build_command_buffer,
                &[build_info],
                &[&[build_range_info]],
            );
            device.end_command_buffer(build_command_buffer).unwrap();
            device
                .queue_submit(
                    graphics_queue,
                    &[vk::SubmitInfo::builder()
                        .command_buffers(&[build_command_buffer])
                        .build()],
                    vk::Fence::null(),
                )
                .expect("queue submit failed.");

            device.queue_wait_idle(graphics_queue).unwrap();
            device.free_command_buffers(command_pool, &[build_command_buffer]);
            scratch_buffer.destroy(&device);
        }

        (top_as, top_as_buffer)
    };

    let (descriptor_set_layout, graphics_pipeline, pipeline_layout, shader_group_count) = {
        let binding_flags_inner = [
            vk::DescriptorBindingFlagsEXT::empty(),
            vk::DescriptorBindingFlagsEXT::empty(),
            vk::DescriptorBindingFlagsEXT::empty(),
        ];

        let mut binding_flags = vk::DescriptorSetLayoutBindingFlagsCreateInfoEXT::builder()
            .binding_flags(&binding_flags_inner)
            .build();

        let descriptor_set_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::builder()
                    .bindings(&[
                        vk::DescriptorSetLayoutBinding::builder()
                            .descriptor_count(1)
                            .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
                            .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
                            .binding(0)
                            .build(),
                        vk::DescriptorSetLayoutBinding::builder()
                            .descriptor_count(1)
                            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                            .stage_flags(vk::ShaderStageFlags::RAYGEN_KHR)
                            .binding(1)
                            .build(),
                        vk::DescriptorSetLayoutBinding::builder()
                            .descriptor_count(1)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .stage_flags(vk::ShaderStageFlags::CLOSEST_HIT_KHR)
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
            vk::RayTracingShaderGroupCreateInfoKHR::builder()
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(0)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR)
                .build(),
            // group1 = [ chit ]
            vk::RayTracingShaderGroupCreateInfoKHR::builder()
                .ty(vk::RayTracingShaderGroupTypeKHR::TRIANGLES_HIT_GROUP)
                .general_shader(vk::SHADER_UNUSED_KHR)
                .closest_hit_shader(1)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR)
                .build(),
            // group2 = [ miss ]
            vk::RayTracingShaderGroupCreateInfoKHR::builder()
                .ty(vk::RayTracingShaderGroupTypeKHR::GENERAL)
                .general_shader(2)
                .closest_hit_shader(vk::SHADER_UNUSED_KHR)
                .any_hit_shader(vk::SHADER_UNUSED_KHR)
                .intersection_shader(vk::SHADER_UNUSED_KHR)
                .build(),
        ];

        let shader_stages = vec![
            vk::PipelineShaderStageCreateInfo::builder()
                .stage(vk::ShaderStageFlags::RAYGEN_KHR)
                .module(shader_module)
                .name(std::ffi::CStr::from_bytes_with_nul(b"main_ray_generation\0").unwrap())
                .build(),
            vk::PipelineShaderStageCreateInfo::builder()
                .stage(vk::ShaderStageFlags::CLOSEST_HIT_KHR)
                .module(shader_module)
                .name(std::ffi::CStr::from_bytes_with_nul(b"main_closest_hit\0").unwrap())
                .build(),
            vk::PipelineShaderStageCreateInfo::builder()
                .stage(vk::ShaderStageFlags::MISS_KHR)
                .module(shader_module)
                .name(std::ffi::CStr::from_bytes_with_nul(b"main_miss\0").unwrap())
                .build(),
        ];

        let pipeline = unsafe {
            rt_pipeline.create_ray_tracing_pipelines(
                vk::DeferredOperationKHR::null(),
                vk::PipelineCache::null(),
                &[vk::RayTracingPipelineCreateInfoKHR::builder()
                    .stages(&shader_stages)
                    .groups(&shader_groups)
                    .max_pipeline_ray_recursion_depth(1)
                    .layout(pipeline_layout)
                    .build()],
                None,
            )
        }
        .unwrap()[0];

        unsafe {
            device.destroy_shader_module(shader_module, None);
        }

        (
            descriptor_set_layout,
            pipeline,
            pipeline_layout,
            shader_groups.len(),
        )
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

    let handle_size_aligned = aligned_size(
        rt_pipeline_properties.shader_group_handle_size,
        rt_pipeline_properties.shader_group_base_alignment,
    ) as u64;

    let shader_binding_table_buffer = {
        let incoming_table_data = unsafe {
            rt_pipeline.get_ray_tracing_shader_group_handles(
                graphics_pipeline,
                0,
                shader_group_count as u32,
                shader_group_count * rt_pipeline_properties.shader_group_handle_size as usize,
            )
        }
        .unwrap();

        let table_size = shader_group_count * handle_size_aligned as usize;
        let mut table_data = vec![0u8; table_size];

        for i in 0..shader_group_count {
            table_data[i * handle_size_aligned as usize
                ..i * handle_size_aligned as usize
                    + rt_pipeline_properties.shader_group_handle_size as usize]
                .copy_from_slice(
                    &incoming_table_data[i * rt_pipeline_properties.shader_group_handle_size
                        as usize
                        ..i * rt_pipeline_properties.shader_group_handle_size as usize
                            + rt_pipeline_properties.shader_group_handle_size as usize],
                );
        }

        let mut shader_binding_table_buffer = BufferResource::new(
            table_size as u64,
            vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                | vk::BufferUsageFlags::TRANSFER_SRC
                | vk::BufferUsageFlags::SHADER_BINDING_TABLE_KHR,
            vk::MemoryPropertyFlags::HOST_VISIBLE,
            &device,
            device_memory_properties,
        );

        shader_binding_table_buffer.store(&table_data, &device);

        shader_binding_table_buffer
    };

    let color_buffer = {
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

        color_buffer
    };

    let descriptor_sizes = [
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::ACCELERATION_STRUCTURE_KHR,
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

    let descriptor_set = unsafe {
        device.allocate_descriptor_sets(
            &vk::DescriptorSetAllocateInfo::builder()
                .descriptor_pool(descriptor_pool)
                .set_layouts(&[descriptor_set_layout])
                .push_next(&mut count_allocate_info)
                .build(),
        )
    }
    .unwrap()[0];

    let accel_structs = [top_as];
    let mut accel_info = vk::WriteDescriptorSetAccelerationStructureKHR::builder()
        .acceleration_structures(&accel_structs)
        .build();

    let mut accel_write = vk::WriteDescriptorSet::builder()
        .dst_set(descriptor_set)
        .dst_binding(0)
        .dst_array_element(0)
        .descriptor_type(vk::DescriptorType::ACCELERATION_STRUCTURE_KHR)
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

    let buffer_info = [vk::DescriptorBufferInfo::builder()
        .buffer(color_buffer.buffer)
        .range(vk::WHOLE_SIZE)
        .build()];

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
        // |[ raygen shader ]|[ hit shader  ]|[ miss shader ]|
        // |                 |               |               |
        // | 0               | 1             | 2             | 3

        let sbt_address =
            unsafe { get_buffer_device_address(&device, shader_binding_table_buffer.buffer) };

        let sbt_raygen_region = vk::StridedDeviceAddressRegionKHR::builder()
            .device_address(sbt_address)
            .size(handle_size_aligned)
            .stride(handle_size_aligned)
            .build();

        let sbt_miss_region = vk::StridedDeviceAddressRegionKHR::builder()
            .device_address(sbt_address + 2 * handle_size_aligned)
            .size(handle_size_aligned)
            .stride(handle_size_aligned)
            .build();

        let sbt_hit_region = vk::StridedDeviceAddressRegionKHR::builder()
            .device_address(sbt_address + handle_size_aligned)
            .size(handle_size_aligned)
            .stride(handle_size_aligned)
            .build();

        let sbt_call_region = vk::StridedDeviceAddressRegionKHR::default();

        unsafe {
            device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::RAY_TRACING_KHR,
                graphics_pipeline,
            );
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::RAY_TRACING_KHR,
                pipeline_layout,
                0,
                &[descriptor_set],
                &[],
            );
            rt_pipeline.cmd_trace_rays(
                command_buffer,
                &sbt_raygen_region,
                &sbt_miss_region,
                &sbt_hit_region,
                &sbt_call_region,
                WIDTH,
                HEIGHT,
                1,
            );
            device.end_command_buffer(command_buffer).unwrap();
        }
    }

    {
        let submit_infos = [vk::SubmitInfo::builder()
            .command_buffers(&[command_buffer])
            .build()];

        unsafe {
            device
                .queue_submit(graphics_queue, &submit_infos, vk::Fence::null())
                .expect("Failed to execute queue submit.");

            device.queue_wait_idle(graphics_queue).unwrap();
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
                .queue_submit(graphics_queue, &submit_infos, vk::Fence::null())
                .expect("Failed to execute queue submit.");

            device.queue_wait_idle(graphics_queue).unwrap();
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
    png_encoder.set_color(png::ColorType::Rgba);

    let mut png_writer = png_encoder
        .write_header()
        .unwrap()
        .into_stream_writer_with_size((4 * WIDTH) as usize)
        .unwrap();

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
        device.destroy_command_pool(command_pool, None);
    }

    unsafe {
        // device.destroy_descriptor_set_layout(layout, allocation_callbacks)
        device.destroy_descriptor_pool(descriptor_pool, None);
        shader_binding_table_buffer.destroy(&device);
        device.destroy_pipeline(graphics_pipeline, None);
        device.destroy_descriptor_set_layout(descriptor_set_layout, None);
    }

    unsafe {
        device.destroy_pipeline_layout(pipeline_layout, None);
    }

    unsafe {
        acceleration_structure.destroy_acceleration_structure(bottom_as, None);
        bottom_as_buffer.destroy(&device);

        acceleration_structure.destroy_acceleration_structure(top_as, None);
        top_as_buffer.destroy(&device);

        device.destroy_image_view(image_view, None);
        device.destroy_image(image, None);
        device.free_memory(device_memory, None);
    }

    unsafe {
        color_buffer.destroy(&device);
        instance_buffer.destroy(&device);
        vertex_buffer.destroy(&device);
        index_buffer.destroy(&device);
    }

    unsafe {
        device.destroy_device(None);
    }

    unsafe {
        instance.destroy_instance(None);
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
            let has_all_extesions =
                unsafe { instance.enumerate_device_extension_properties(physical_device) }.map(
                    |exts| {
                        let set: HashSet<&CStr> = exts
                            .iter()
                            .map(|ext| unsafe {
                                CStr::from_ptr(&ext.extension_name as *const c_char)
                            })
                            .collect();

                        extensions.iter().all(|ext| set.contains(ext))
                    },
                );
            if has_all_extesions != Ok(true) {
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
        if (type_bits & 1) == 1
            && (device_memory_properties.memory_types[i as usize].property_flags & properties)
                == properties
        {
            return i;
        }
        type_bits >>= 1;
    }
    0
}

#[allow(clippy::missing_safety_doc)]
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

            let mut memory_allocate_flags_info = vk::MemoryAllocateFlagsInfo::builder()
                .flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS)
                .build();

            let mut allocate_info_builder = vk::MemoryAllocateInfo::builder();

            if usage.contains(vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS) {
                allocate_info_builder =
                    allocate_info_builder.push_next(&mut memory_allocate_flags_info);
            }

            let allocate_info = allocate_info_builder
                .allocation_size(memory_req.size)
                .memory_type_index(memory_index)
                .build();

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
            let size = std::mem::size_of_val(data) as u64;
            assert!(self.size >= size, "Data size is larger than buffer size.");
            let mapped_ptr = self.map(size, device);
            let mut mapped_slice = Align::new(mapped_ptr, std::mem::align_of::<T>() as u64, size);
            mapped_slice.copy_from_slice(data);
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

    unsafe fn destroy(self, device: &ash::Device) {
        device.destroy_buffer(self.buffer, None);
        device.free_memory(self.memory, None);
    }
}

fn aligned_size(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) & !(alignment - 1)
}

unsafe fn get_buffer_device_address(device: &ash::Device, buffer: vk::Buffer) -> u64 {
    let buffer_device_address_info = vk::BufferDeviceAddressInfo::builder()
        .buffer(buffer)
        .build();

    device.get_buffer_device_address(&buffer_device_address_info)
}
