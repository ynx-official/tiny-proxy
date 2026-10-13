#![cfg(all(windows, feature = "desktop"))]

use std::{os::windows::ffi::OsStrExt, ptr};
use windows_sys::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{
            FindResourceW, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
            LoadLibraryExW, LoadResource, LockResource, SizeofResource,
        },
        UI::WindowsAndMessaging::{
            IMAGE_ICON, LR_DEFAULTSIZE, LR_SHARED, LoadImageW, RT_GROUP_ICON, RT_ICON,
        },
    },
    core::PCWSTR,
};

struct ResourceModule(HMODULE);

impl ResourceModule {
    fn resource(&self, id: u16, kind: PCWSTR) -> &[u8] {
        // Integer resource identifiers are Win32 pseudo-pointers; the mapped
        // bytes stay valid until this module is released.
        unsafe {
            let resource = FindResourceW(self.0, id as usize as PCWSTR, kind);
            assert!(!resource.is_null(), "missing Windows resource {id}");
            let len = SizeofResource(self.0, resource) as usize;
            assert!(len > 0, "empty Windows resource {id}");
            let data = LockResource(LoadResource(self.0, resource));
            assert!(!data.is_null(), "could not read Windows resource {id}");
            std::slice::from_raw_parts(data.cast(), len)
        }
    }
}

impl Drop for ResourceModule {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0) };
    }
}

#[test]
fn executable_embeds_all_icon_frames_and_loads_without_sidecar_assets() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("tiny-proxy.exe");
    std::fs::copy(env!("CARGO_BIN_EXE_tiny-proxy"), &executable).unwrap();
    let path: Vec<_> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // Map resources only: this does not run the app or open its database.
    let module = ResourceModule(unsafe {
        LoadLibraryExW(
            path.as_ptr(),
            ptr::null_mut(),
            LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE,
        )
    });
    assert!(!module.0.is_null(), "could not map the built executable");

    // GPUI's Windows backend loads the main icon from resource ID 1.
    let group = module.resource(1, RT_GROUP_ICON);
    let icon = include_bytes!("../assets/icons/tiny-proxy.ico");
    assert_eq!(&group[..6], &icon[..6]);
    let count = u16::from_le_bytes(icon[4..6].try_into().unwrap()) as usize;
    assert_eq!(group.len(), 6 + count * 14);
    for index in 0..count {
        let entry = &group[6 + index * 14..6 + (index + 1) * 14];
        let original = &icon[6 + index * 16..6 + (index + 1) * 16];
        assert_eq!(&entry[..12], &original[..12]);
        let id = u16::from_le_bytes(entry[12..14].try_into().unwrap());
        let len = u32::from_le_bytes(original[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(original[12..16].try_into().unwrap()) as usize;
        assert_eq!(module.resource(id, RT_ICON), &icon[offset..offset + len]);
    }
    let loaded = unsafe {
        LoadImageW(
            module.0,
            1usize as PCWSTR,
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        )
    };
    assert!(!loaded.is_null(), "GPUI's icon loading call must succeed");
}
