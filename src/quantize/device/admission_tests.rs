//! module: device::admission_tests; responsibility: define usable GPU
//! admission. operations: Rust test cases; entities: none; tests:
//! required capabilities, complete extension sets, live compute queues
//! and candidate fallthrough.
use ash::vk;

use super::admission::{Candidate, EXTENSIONS};

fn extensions() -> Vec<vk::ExtensionProperties> {
  EXTENSIONS
    .iter()
    .map(|name| {
      let mut extension = vk::ExtensionProperties::default();
      for (to, from) in extension
        .extension_name
        .iter_mut()
        .zip(name.to_bytes_with_nul())
      {
        *to = *from as std::ffi::c_char;
      }
      extension
    })
    .collect()
}

#[test]
fn test_admit_requires_hardware_vulkan11_dma_import_and_sync_export() {
  let properties = vk::PhysicalDeviceProperties {
    api_version: vk::API_VERSION_1_1,
    device_type: vk::PhysicalDeviceType::INTEGRATED_GPU,
    ..Default::default()
  };
  let families = [vk::QueueFamilyProperties {
    queue_count: 1,
    queue_flags: vk::QueueFlags::COMPUTE,
    ..Default::default()
  }];
  let extensions = extensions();
  let candidate = Candidate {
    properties: &properties,
    families: &families,
    extensions: &extensions,
    memory: vk::ExternalMemoryFeatureFlags::IMPORTABLE,
    fence: vk::ExternalFenceFeatureFlags::EXPORTABLE,
  };
  assert_eq!(candidate.family(), Some(0));
  for properties in [
    vk::PhysicalDeviceProperties {
      device_type: vk::PhysicalDeviceType::CPU,
      ..properties
    },
    vk::PhysicalDeviceProperties {
      api_version: vk::API_VERSION_1_0,
      ..properties
    },
  ] {
    assert_eq!(
      Candidate { properties: &properties, ..candidate }.family(),
      None
    );
  }
  assert_eq!(
    Candidate {
      memory: vk::ExternalMemoryFeatureFlags::EXPORTABLE,
      ..candidate
    }
    .family(),
    None
  );
  assert_eq!(
    Candidate {
      fence: vk::ExternalFenceFeatureFlags::IMPORTABLE,
      ..candidate
    }
    .family(),
    None
  );
  for missing in 0..extensions.len() {
    let mut incomplete = extensions.clone();
    assert_eq!(
      incomplete.remove(missing).extension_name,
      extensions[missing].extension_name
    );
    assert_eq!(
      Candidate { extensions: &incomplete, ..candidate }.family(),
      None
    );
  }
  let families = [
    vk::QueueFamilyProperties { queue_count: 0, ..families[0] },
    vk::QueueFamilyProperties {
      queue_flags: vk::QueueFlags::TRANSFER,
      ..families[0]
    },
    families[0],
  ];
  assert_eq!(
    Candidate { families: &families[..2], ..candidate }.family(),
    None
  );
  assert_eq!(
    Candidate { families: &families, ..candidate }.family(),
    Some(2)
  );
  assert_eq!(Candidate { families: &[], ..candidate }.family(), None);
  let candidates =
    [Candidate { extensions: &[], ..candidate }, candidate];
  assert_eq!(candidates.iter().find_map(Candidate::family), Some(0));
}
