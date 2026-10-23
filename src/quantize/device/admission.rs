//! module: device::admission; responsibility: admit GPU capabilities
//! required by the conversion pipeline. operations: Candidate::family,
//! EXTENSIONS; entities: Candidate; tests: admission_tests. Extension
//! availability alone does not establish DMA-BUF import or sync-file
//! fence export support.
use std::ffi::CStr;

use ash::vk;

pub(super) const EXTENSIONS: [&CStr; 4] = [
  ash::khr::external_memory_fd::NAME,
  ash::khr::external_fence_fd::NAME,
  ash::ext::external_memory_dma_buf::NAME,
  ash::ext::queue_family_foreign::NAME,
];

#[derive(Clone, Copy)]
pub(super) struct Candidate<'a> {
  pub properties: &'a vk::PhysicalDeviceProperties,
  pub families: &'a [vk::QueueFamilyProperties],
  pub extensions: &'a [vk::ExtensionProperties],
  pub memory: vk::ExternalMemoryFeatureFlags,
  pub fence: vk::ExternalFenceFeatureFlags,
}
impl Candidate<'_> {
  pub(super) fn family(&self) -> Option<u32> {
    if self.properties.device_type == vk::PhysicalDeviceType::CPU
      || self.properties.api_version < vk::API_VERSION_1_1
      || !self
        .memory
        .contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE)
      || !self.fence.contains(vk::ExternalFenceFeatureFlags::EXPORTABLE)
      || EXTENSIONS.iter().any(|name| {
        !self.extensions.iter().any(|extension| {
          extension
            .extension_name_as_c_str()
            .is_ok_and(|available| available == *name)
        })
      })
    {
      return None;
    }
    self
      .families
      .iter()
      .position(|family| {
        family.queue_count != 0
          && family.queue_flags.contains(vk::QueueFlags::COMPUTE)
      })
      .and_then(|index| u32::try_from(index).ok())
  }
}
