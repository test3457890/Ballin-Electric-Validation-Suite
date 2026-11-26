//! Minimal, audited Vulkan layer (ash). All `unsafe` Vulkan calls of the
//! crate live in this module.
//!
//! Safety invariants relied on throughout:
//! * Every handle is created from the `ash::Device`/`ash::Instance` stored in
//!   the same owning struct and destroyed exactly once in `Drop`, in reverse
//!   creation order, after `device_wait_idle`.
//! * Create-info structs only borrow locals that outlive the call.
//! * Mapped pointers are only dereferenced within the mapped range, on
//!   host-coherent memory, after the GPU work that wrote them has completed
//!   (fence wait) — never concurrently with GPU access.
//! * SPIR-V is loaded from bytes compiled from the GLSL in `shaders/` and
//!   validated with `spirv-val` at build time (`shaders/build.sh`).

use ash::vk;
use std::io::Cursor;
use std::time::Duration;

pub const KAT_SPV: &[u8] = include_bytes!("../shaders/spv/kat.comp.spv");
pub const VRAM_SPV: &[u8] = include_bytes!("../shaders/spv/vram.comp.spv");
pub const VERT_SPV: &[u8] = include_bytes!("../shaders/spv/fullscreen.vert.spv");
pub const FRAG_SPV: &[u8] = include_bytes!("../shaders/spv/fur.frag.spv");

#[derive(Debug)]
pub enum VkError {
    /// Vulkan loader or instance unavailable.
    Unavailable(String),
    DeviceLost,
    Timeout,
    OutOfMemory,
    Other(String),
}

impl std::fmt::Display for VkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VkError::Unavailable(s) => write!(f, "Vulkan unavailable: {s}"),
            VkError::DeviceLost => {
                f.write_str("VK_ERROR_DEVICE_LOST (GPU reset, driver fault or hardware error)")
            }
            VkError::Timeout => f.write_str("GPU did not complete work within the time limit"),
            VkError::OutOfMemory => f.write_str("out of device or host memory"),
            VkError::Other(s) => f.write_str(s),
        }
    }
}

impl From<vk::Result> for VkError {
    fn from(r: vk::Result) -> Self {
        match r {
            vk::Result::ERROR_DEVICE_LOST => VkError::DeviceLost,
            vk::Result::TIMEOUT => VkError::Timeout,
            vk::Result::ERROR_OUT_OF_DEVICE_MEMORY | vk::Result::ERROR_OUT_OF_HOST_MEMORY => {
                VkError::OutOfMemory
            }
            other => VkError::Other(format!("{other:?}")),
        }
    }
}

pub type VkResult<T> = Result<T, VkError>;

/// Identity and capabilities of one physical device.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceInfo {
    pub index: usize,
    pub name: String,
    pub vendor_id: u32,
    pub vendor: String,
    pub device_id: u32,
    pub device_type: String,
    /// True for CPU-emulated devices (e.g. Mesa llvmpipe/lavapipe).
    pub software: bool,
    pub api_version: String,
    pub driver_version: u32,
    /// PCI address (domain:bus:dev.fn) when VK_EXT_pci_bus_info is supported.
    pub pci_address: Option<String>,
    pub device_local_bytes: u64,
    pub max_storage_buffer_range: u32,
    pub max_alloc_bytes: Option<u64>,
    pub max_workgroup_count_x: u32,
    pub has_compute: bool,
    pub has_graphics: bool,
}

pub fn vendor_name(id: u32) -> &'static str {
    match id {
        0x10de => "NVIDIA",
        0x1002 => "AMD",
        0x8086 => "Intel",
        0x13b5 => "ARM",
        0x5143 => "Qualcomm",
        0x106b => "Apple",
        0x10005 => "Mesa (software)",
        _ => "unknown",
    }
}

pub struct Instance {
    _entry: ash::Entry,
    pub instance: ash::Instance,
    api_1_1: bool,
}

impl Instance {
    pub fn new() -> VkResult<Instance> {
        // SAFETY: loading the system Vulkan loader library; ash validates
        // that the required entry points resolve.
        let entry = unsafe { ash::Entry::load() }
            .map_err(|e| VkError::Unavailable(format!("Vulkan loader not found: {e}")))?;
        // SAFETY: plain query on a loaded entry.
        let loader_version = unsafe { entry.try_enumerate_instance_version() }
            .ok()
            .flatten()
            .unwrap_or(vk::API_VERSION_1_0);
        let api = if loader_version >= vk::API_VERSION_1_1 {
            vk::API_VERSION_1_1
        } else {
            vk::API_VERSION_1_0
        };
        let app = vk::ApplicationInfo::default()
            .application_name(c"BE Validation")
            .application_version(1)
            .engine_name(c"be-gpu")
            .api_version(api);
        let ci = vk::InstanceCreateInfo::default().application_info(&app);
        // SAFETY: `app` and `ci` outlive the call; no extensions or layers requested.
        let instance = unsafe { entry.create_instance(&ci, None) }
            .map_err(|e| VkError::Unavailable(format!("vkCreateInstance failed: {e:?}")))?;
        Ok(Instance {
            _entry: entry,
            instance,
            api_1_1: api >= vk::API_VERSION_1_1,
        })
    }

    pub fn physical_devices(&self) -> VkResult<Vec<vk::PhysicalDevice>> {
        // SAFETY: valid instance.
        Ok(unsafe { self.instance.enumerate_physical_devices() }?)
    }

    pub fn describe(&self, index: usize, pd: vk::PhysicalDevice) -> DeviceInfo {
        // SAFETY: `pd` was enumerated from this instance; all calls are queries.
        unsafe {
            let p = self.instance.get_physical_device_properties(pd);
            let name = p
                .device_name_as_c_str()
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "unknown".into());
            let mem = self.instance.get_physical_device_memory_properties(pd);
            let device_local_bytes = mem
                .memory_heaps_as_slice()
                .iter()
                .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
                .map(|h| h.size)
                .max()
                .unwrap_or(0);
            let qf = self
                .instance
                .get_physical_device_queue_family_properties(pd);
            let exts: Vec<String> = self
                .instance
                .enumerate_device_extension_properties(pd)
                .unwrap_or_default()
                .iter()
                .filter_map(|e| {
                    e.extension_name_as_c_str()
                        .ok()
                        .map(|c| c.to_string_lossy().into_owned())
                })
                .collect();
            let mut pci_address = None;
            let mut max_alloc_bytes = None;
            if self.api_1_1 {
                let mut pci = vk::PhysicalDevicePCIBusInfoPropertiesEXT::default();
                let mut maint3 = vk::PhysicalDeviceMaintenance3Properties::default();
                let has_pci = exts.iter().any(|e| e == "VK_EXT_pci_bus_info");
                let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut maint3);
                if has_pci {
                    p2 = p2.push_next(&mut pci);
                }
                self.instance.get_physical_device_properties2(pd, &mut p2);
                max_alloc_bytes = Some(maint3.max_memory_allocation_size);
                if has_pci {
                    pci_address = Some(format!(
                        "{:04x}:{:02x}:{:02x}.{}",
                        pci.pci_domain, pci.pci_bus, pci.pci_device, pci.pci_function
                    ));
                }
            }
            let software = p.device_type == vk::PhysicalDeviceType::CPU;
            DeviceInfo {
                index,
                name,
                vendor_id: p.vendor_id,
                vendor: vendor_name(p.vendor_id).into(),
                device_id: p.device_id,
                device_type: format!("{:?}", p.device_type),
                software,
                api_version: format!(
                    "{}.{}.{}",
                    vk::api_version_major(p.api_version),
                    vk::api_version_minor(p.api_version),
                    vk::api_version_patch(p.api_version)
                ),
                driver_version: p.driver_version,
                pci_address,
                device_local_bytes,
                max_storage_buffer_range: p.limits.max_storage_buffer_range,
                max_alloc_bytes,
                max_workgroup_count_x: p.limits.max_compute_work_group_count[0],
                has_compute: qf
                    .iter()
                    .any(|q| q.queue_flags.contains(vk::QueueFlags::COMPUTE)),
                has_graphics: qf
                    .iter()
                    .any(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS)),
            }
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: all devices created from this instance are dropped first
        // (Device holds an Arc-free borrow-by-lifetime; callers drop Device first).
        unsafe { self.instance.destroy_instance(None) };
    }
}

pub struct Buffer {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub size: u64,
    pub mapped: Option<*mut u8>,
}

pub struct Device<'i> {
    pub inst: &'i Instance,
    pub pd: vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue: vk::Queue,
    pub family: u32,
    pub mem: vk::PhysicalDeviceMemoryProperties,
    pub pool: vk::CommandPool,
    buffers: Vec<Buffer>,
    modules: Vec<vk::ShaderModule>,
    pipelines: Vec<vk::Pipeline>,
    layouts: Vec<vk::PipelineLayout>,
    set_layouts: Vec<vk::DescriptorSetLayout>,
    desc_pools: Vec<vk::DescriptorPool>,
    images: Vec<(vk::Image, vk::DeviceMemory, vk::ImageView)>,
    render_passes: Vec<vk::RenderPass>,
    framebuffers: Vec<vk::Framebuffer>,
    fence: vk::Fence,
    pub timeout: Duration,
}

impl<'i> Device<'i> {
    /// Create a logical device with one queue supporting compute (and
    /// graphics when `graphics` is true).
    pub fn new(inst: &'i Instance, pd: vk::PhysicalDevice, graphics: bool) -> VkResult<Device<'i>> {
        // SAFETY: `pd` belongs to `inst`; create-info borrows live locals.
        unsafe {
            let qf = inst
                .instance
                .get_physical_device_queue_family_properties(pd);
            let need = if graphics {
                vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE
            } else {
                vk::QueueFlags::COMPUTE
            };
            let family = qf
                .iter()
                .position(|q| q.queue_flags.contains(need))
                .ok_or_else(|| VkError::Other("no suitable queue family".into()))?
                as u32;
            let prio = [1.0f32];
            let qci = vk::DeviceQueueCreateInfo::default()
                .queue_family_index(family)
                .queue_priorities(&prio);
            let dci =
                vk::DeviceCreateInfo::default().queue_create_infos(std::slice::from_ref(&qci));
            let device = inst.instance.create_device(pd, &dci, None)?;
            let queue = device.get_device_queue(family, 0);
            let mem = inst.instance.get_physical_device_memory_properties(pd);
            let pool = device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?;
            let fence = device.create_fence(&vk::FenceCreateInfo::default(), None)?;
            Ok(Device {
                inst,
                pd,
                device,
                queue,
                family,
                mem,
                pool,
                buffers: vec![],
                modules: vec![],
                pipelines: vec![],
                layouts: vec![],
                set_layouts: vec![],
                desc_pools: vec![],
                images: vec![],
                render_passes: vec![],
                framebuffers: vec![],
                fence,
                timeout: Duration::from_secs(20),
            })
        }
    }

    fn memory_type(
        &self,
        bits: u32,
        want: vk::MemoryPropertyFlags,
        fallback: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        let types = &self.mem.memory_types[..self.mem.memory_type_count as usize];
        let find = |f: vk::MemoryPropertyFlags| {
            types
                .iter()
                .enumerate()
                .position(|(i, t)| bits & (1 << i) != 0 && t.property_flags.contains(f))
        };
        find(want).or_else(|| find(fallback)).map(|i| i as u32)
    }

    /// Create a storage buffer. `host` buffers are host-visible, coherent and
    /// persistently mapped; others prefer device-local memory. Returns the
    /// index into the device's buffer list.
    pub fn buffer(&mut self, size: u64, host: bool) -> VkResult<usize> {
        // SAFETY: standard buffer creation; memory bound once; mapping covers
        // exactly `size` bytes of host-coherent memory.
        unsafe {
            let usage = vk::BufferUsageFlags::STORAGE_BUFFER
                | vk::BufferUsageFlags::TRANSFER_DST
                | vk::BufferUsageFlags::TRANSFER_SRC;
            let buffer = self.device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )?;
            let req = self.device.get_buffer_memory_requirements(buffer);
            let (want, fb) = if host {
                (
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )
            } else {
                (
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                    vk::MemoryPropertyFlags::empty(),
                )
            };
            let Some(mt) = self.memory_type(req.memory_type_bits, want, fb) else {
                self.device.destroy_buffer(buffer, None);
                return Err(VkError::Other("no suitable memory type".into()));
            };
            let memory = match self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(mt),
                None,
            ) {
                Ok(m) => m,
                Err(e) => {
                    self.device.destroy_buffer(buffer, None);
                    return Err(e.into());
                }
            };
            self.device.bind_buffer_memory(buffer, memory, 0)?;
            let mapped = if host {
                Some(
                    self.device
                        .map_memory(memory, 0, size, vk::MemoryMapFlags::empty())?
                        as *mut u8,
                )
            } else {
                None
            };
            self.buffers.push(Buffer {
                buffer,
                memory,
                size,
                mapped,
            });
            Ok(self.buffers.len() - 1)
        }
    }

    pub fn buf(&self, i: usize) -> &Buffer {
        &self.buffers[i]
    }

    /// Read `n` u32 words from a host buffer (after the GPU work completed).
    pub fn read_u32(&self, i: usize, n: usize) -> Vec<u32> {
        let b = &self.buffers[i];
        let p = b.mapped.expect("host buffer");
        assert!((n * 4) as u64 <= b.size);
        let mut out = vec![0u32; n];
        // SAFETY: mapped, coherent, in range, GPU idle (callers wait on fence).
        unsafe { std::ptr::copy_nonoverlapping(p as *const u32, out.as_mut_ptr(), n) };
        out
    }

    pub fn read_bytes(&self, i: usize, n: usize) -> Vec<u8> {
        let b = &self.buffers[i];
        let p = b.mapped.expect("host buffer");
        assert!(n as u64 <= b.size);
        let mut out = vec![0u8; n];
        // SAFETY: as in read_u32.
        unsafe { std::ptr::copy_nonoverlapping(p, out.as_mut_ptr(), n) };
        out
    }

    pub fn zero(&self, i: usize) {
        let b = &self.buffers[i];
        if let Some(p) = b.mapped {
            // SAFETY: mapped range, GPU idle.
            unsafe { std::ptr::write_bytes(p, 0, b.size as usize) };
        }
    }

    fn module(&mut self, spv: &[u8]) -> VkResult<vk::ShaderModule> {
        let words = ash::util::read_spv(&mut Cursor::new(spv))
            .map_err(|e| VkError::Other(format!("invalid SPIR-V: {e}")))?;
        // SAFETY: words is valid, validated SPIR-V.
        let m = unsafe {
            self.device
                .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&words), None)
        }?;
        self.modules.push(m);
        Ok(m)
    }

    /// Compute pipeline with `bindings` storage buffers and `push` bytes of
    /// push constants. Returns (pipeline, layout, set_layout).
    pub fn compute_pipeline(
        &mut self,
        spv: &[u8],
        bindings: u32,
        push: u32,
    ) -> VkResult<(vk::Pipeline, vk::PipelineLayout, vk::DescriptorSetLayout)> {
        let m = self.module(spv)?;
        // SAFETY: create-infos borrow locals; handles tracked for Drop.
        unsafe {
            let b: Vec<_> = (0..bindings)
                .map(|i| {
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(i)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::COMPUTE)
                })
                .collect();
            let sl = self.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&b),
                None,
            )?;
            self.set_layouts.push(sl);
            let pcr = vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
                .offset(0)
                .size(push);
            let layout = self.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(std::slice::from_ref(&sl))
                    .push_constant_ranges(std::slice::from_ref(&pcr)),
                None,
            )?;
            self.layouts.push(layout);
            let stage = vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::COMPUTE)
                .module(m)
                .name(c"main");
            let ci = vk::ComputePipelineCreateInfo::default()
                .stage(stage)
                .layout(layout);
            let p = self
                .device
                .create_compute_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(&ci),
                    None,
                )
                .map_err(|(_, e)| VkError::from(e))?[0];
            self.pipelines.push(p);
            Ok((p, layout, sl))
        }
    }

    /// Allocate `count` descriptor sets, each binding the given buffer lists.
    pub fn descriptor_sets(
        &mut self,
        layout: vk::DescriptorSetLayout,
        sets: &[Vec<usize>],
    ) -> VkResult<Vec<vk::DescriptorSet>> {
        let total: u32 = sets.iter().map(|s| s.len() as u32).sum();
        // SAFETY: pool sized for exactly these sets; writes reference live buffers.
        unsafe {
            let sizes = [vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: total.max(1),
            }];
            let pool = self.device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(sets.len() as u32)
                    .pool_sizes(&sizes),
                None,
            )?;
            self.desc_pools.push(pool);
            let layouts = vec![layout; sets.len()];
            let ds = self.device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts),
            )?;
            for (set, bufs) in ds.iter().zip(sets) {
                let infos: Vec<_> = bufs
                    .iter()
                    .map(|b| {
                        vk::DescriptorBufferInfo::default()
                            .buffer(self.buffers[*b].buffer)
                            .offset(0)
                            .range(vk::WHOLE_SIZE)
                    })
                    .collect();
                let writes: Vec<_> = infos
                    .iter()
                    .enumerate()
                    .map(|(i, info)| {
                        vk::WriteDescriptorSet::default()
                            .dst_set(*set)
                            .dst_binding(i as u32)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .buffer_info(std::slice::from_ref(info))
                    })
                    .collect();
                self.device.update_descriptor_sets(&writes, &[]);
            }
            Ok(ds)
        }
    }

    /// Record commands with `f`, submit, and wait (bounded by `timeout`).
    pub fn submit(&self, f: impl FnOnce(&ash::Device, vk::CommandBuffer)) -> VkResult<()> {
        // SAFETY: one-time command buffer from our pool; fence reset before
        // use; we wait for completion before freeing the command buffer.
        unsafe {
            let cb = self.device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(self.pool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )?[0];
            let result = (|| -> VkResult<()> {
                self.device.begin_command_buffer(
                    cb,
                    &vk::CommandBufferBeginInfo::default()
                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
                )?;
                f(&self.device, cb);
                self.device.end_command_buffer(cb)?;
                self.device.reset_fences(&[self.fence])?;
                let si = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cb));
                self.device
                    .queue_submit(self.queue, std::slice::from_ref(&si), self.fence)?;
                self.device
                    .wait_for_fences(&[self.fence], true, self.timeout.as_nanos() as u64)?;
                Ok(())
            })();
            if !matches!(result, Err(VkError::Timeout) | Err(VkError::DeviceLost)) {
                self.device.free_command_buffers(self.pool, &[cb]);
            }
            result
        }
    }

    /// Barrier between dependent compute dispatches (and host readback).
    pub fn compute_barrier(d: &ash::Device, cb: vk::CommandBuffer, host: bool) {
        let mb = vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::SHADER_WRITE)
            .dst_access_mask(if host {
                vk::AccessFlags::HOST_READ
            } else {
                vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE
            });
        let dst = if host {
            vk::PipelineStageFlags::HOST
        } else {
            vk::PipelineStageFlags::COMPUTE_SHADER
        };
        // SAFETY: recording into a command buffer in the recording state.
        unsafe {
            d.cmd_pipeline_barrier(
                cb,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                dst,
                vk::DependencyFlags::empty(),
                &[mb],
                &[],
                &[],
            )
        };
    }

    /// Offscreen graphics target: RGBA8 color image + render pass + framebuffer
    /// + pipeline for fullscreen.vert/fur.frag. Returns handles.
    pub fn graphics_target(&mut self, w: u32, h: u32) -> VkResult<GraphicsTarget> {
        let vs = self.module(VERT_SPV)?;
        let fs = self.module(FRAG_SPV)?;
        // SAFETY: standard offscreen setup; every handle is tracked for Drop.
        unsafe {
            let format = vk::Format::R8G8B8A8_UNORM;
            let image = self.device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width: w,
                        height: h,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(
                        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                    )
                    .initial_layout(vk::ImageLayout::UNDEFINED),
                None,
            )?;
            let req = self.device.get_image_memory_requirements(image);
            let mt = self
                .memory_type(
                    req.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                    vk::MemoryPropertyFlags::empty(),
                )
                .ok_or_else(|| VkError::Other("no image memory type".into()))?;
            let mem = self.device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(mt),
                None,
            )?;
            self.device.bind_image_memory(image, mem, 0)?;
            let view = self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(format)
                    .subresource_range(color_range()),
                None,
            )?;
            self.images.push((image, mem, view));
            let att = vk::AttachmentDescription::default()
                .format(format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
                .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL);
            let cref = vk::AttachmentReference::default()
                .attachment(0)
                .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
            let sub = vk::SubpassDescription::default()
                .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
                .color_attachments(std::slice::from_ref(&cref));
            let dep = vk::SubpassDependency::default()
                .src_subpass(0)
                .dst_subpass(vk::SUBPASS_EXTERNAL)
                .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
                .dst_stage_mask(vk::PipelineStageFlags::TRANSFER)
                .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ);
            let rp = self.device.create_render_pass(
                &vk::RenderPassCreateInfo::default()
                    .attachments(std::slice::from_ref(&att))
                    .subpasses(std::slice::from_ref(&sub))
                    .dependencies(std::slice::from_ref(&dep)),
                None,
            )?;
            self.render_passes.push(rp);
            let fb = self.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(rp)
                    .attachments(std::slice::from_ref(&view))
                    .width(w)
                    .height(h)
                    .layers(1),
                None,
            )?;
            self.framebuffers.push(fb);
            let pcr = vk::PushConstantRange::default()
                .stage_flags(vk::ShaderStageFlags::FRAGMENT)
                .offset(0)
                .size(16);
            let layout = self.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .push_constant_ranges(std::slice::from_ref(&pcr)),
                None,
            )?;
            self.layouts.push(layout);
            let stages = [
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::VERTEX)
                    .module(vs)
                    .name(c"main"),
                vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::FRAGMENT)
                    .module(fs)
                    .name(c"main"),
            ];
            let vi = vk::PipelineVertexInputStateCreateInfo::default();
            let ia = vk::PipelineInputAssemblyStateCreateInfo::default()
                .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
            let viewport = vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: w as f32,
                height: h as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            };
            let scissor = vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: w,
                    height: h,
                },
            };
            let vp = vk::PipelineViewportStateCreateInfo::default()
                .viewports(std::slice::from_ref(&viewport))
                .scissors(std::slice::from_ref(&scissor));
            let rs = vk::PipelineRasterizationStateCreateInfo::default()
                .polygon_mode(vk::PolygonMode::FILL)
                .cull_mode(vk::CullModeFlags::NONE)
                .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
                .line_width(1.0);
            let ms = vk::PipelineMultisampleStateCreateInfo::default()
                .rasterization_samples(vk::SampleCountFlags::TYPE_1);
            let cba = vk::PipelineColorBlendAttachmentState::default()
                .color_write_mask(vk::ColorComponentFlags::RGBA);
            let cb = vk::PipelineColorBlendStateCreateInfo::default()
                .attachments(std::slice::from_ref(&cba));
            let gci = vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&vi)
                .input_assembly_state(&ia)
                .viewport_state(&vp)
                .rasterization_state(&rs)
                .multisample_state(&ms)
                .color_blend_state(&cb)
                .layout(layout)
                .render_pass(rp)
                .subpass(0);
            let pipeline = self
                .device
                .create_graphics_pipelines(
                    vk::PipelineCache::null(),
                    std::slice::from_ref(&gci),
                    None,
                )
                .map_err(|(_, e)| VkError::from(e))?[0];
            self.pipelines.push(pipeline);
            let readback = self.buffer(w as u64 * h as u64 * 4, true)?;
            Ok(GraphicsTarget {
                image,
                render_pass: rp,
                framebuffer: fb,
                pipeline,
                layout,
                readback,
                w,
                h,
            })
        }
    }
}

pub struct GraphicsTarget {
    pub image: vk::Image,
    pub render_pass: vk::RenderPass,
    pub framebuffer: vk::Framebuffer,
    pub pipeline: vk::Pipeline,
    pub layout: vk::PipelineLayout,
    pub readback: usize,
    pub w: u32,
    pub h: u32,
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

impl GraphicsTarget {
    /// Record one frame: render pass with the fur shader. If `readback`, copy
    /// the image into the host buffer.
    pub fn record(
        &self,
        d: &ash::Device,
        cb: vk::CommandBuffer,
        t: f32,
        layers: u32,
        readback_buf: Option<vk::Buffer>,
    ) {
        let clear = vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 1.0],
            },
        };
        let rpbi = vk::RenderPassBeginInfo::default()
            .render_pass(self.render_pass)
            .framebuffer(self.framebuffer)
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: vk::Extent2D {
                    width: self.w,
                    height: self.h,
                },
            })
            .clear_values(std::slice::from_ref(&clear));
        let mut pc = [0u8; 16];
        pc[0..4].copy_from_slice(&t.to_le_bytes());
        pc[4..8].copy_from_slice(&layers.to_le_bytes());
        pc[8..12].copy_from_slice(&self.w.to_le_bytes());
        pc[12..16].copy_from_slice(&self.h.to_le_bytes());
        // SAFETY: recording state; handles owned by the Device.
        unsafe {
            d.cmd_begin_render_pass(cb, &rpbi, vk::SubpassContents::INLINE);
            d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            d.cmd_push_constants(cb, self.layout, vk::ShaderStageFlags::FRAGMENT, 0, &pc);
            d.cmd_draw(cb, 3, 1, 0, 0);
            d.cmd_end_render_pass(cb);
            if let Some(buf) = readback_buf {
                let region = vk::BufferImageCopy::default()
                    .image_subresource(vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: 1,
                    })
                    .image_extent(vk::Extent3D {
                        width: self.w,
                        height: self.h,
                        depth: 1,
                    });
                d.cmd_copy_image_to_buffer(
                    cb,
                    self.image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buf,
                    std::slice::from_ref(&region),
                );
                let mb = vk::MemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::HOST_READ);
                d.cmd_pipeline_barrier(
                    cb,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::HOST,
                    vk::DependencyFlags::empty(),
                    &[mb],
                    &[],
                    &[],
                );
            }
        }
    }
}

impl Drop for Device<'_> {
    fn drop(&mut self) {
        // SAFETY: wait idle, then destroy every tracked handle exactly once.
        unsafe {
            let _ = self.device.device_wait_idle();
            for p in self.pipelines.drain(..) {
                self.device.destroy_pipeline(p, None);
            }
            for f in self.framebuffers.drain(..) {
                self.device.destroy_framebuffer(f, None);
            }
            for r in self.render_passes.drain(..) {
                self.device.destroy_render_pass(r, None);
            }
            for (img, mem, view) in self.images.drain(..) {
                self.device.destroy_image_view(view, None);
                self.device.destroy_image(img, None);
                self.device.free_memory(mem, None);
            }
            for l in self.layouts.drain(..) {
                self.device.destroy_pipeline_layout(l, None);
            }
            for p in self.desc_pools.drain(..) {
                self.device.destroy_descriptor_pool(p, None);
            }
            for l in self.set_layouts.drain(..) {
                self.device.destroy_descriptor_set_layout(l, None);
            }
            for m in self.modules.drain(..) {
                self.device.destroy_shader_module(m, None);
            }
            for b in self.buffers.drain(..) {
                if b.mapped.is_some() {
                    self.device.unmap_memory(b.memory);
                }
                self.device.destroy_buffer(b.buffer, None);
                self.device.free_memory(b.memory, None);
            }
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.pool, None);
            self.device.destroy_device(None);
        }
    }
}
