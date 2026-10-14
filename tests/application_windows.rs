#![cfg(all(windows, feature = "desktop"))]

#[test]
fn desktop_executable_uses_gui_subsystem_including_debug_builds() {
    // Inspect the produced PE header without starting the application.
    let exe = std::fs::read(env!("CARGO_BIN_EXE_tiny-proxy")).unwrap();
    assert_eq!(&exe[..2], b"MZ");
    let pe = u32::from_le_bytes(exe[0x3c..0x40].try_into().unwrap()) as usize;
    assert_eq!(&exe[pe..pe + 4], b"PE\0\0");
    let optional_header = pe + 24;
    let subsystem = u16::from_le_bytes(
        exe[optional_header + 68..optional_header + 70]
            .try_into()
            .unwrap(),
    );
    assert_eq!(subsystem, 2, "the desktop EXE must not create a console");
}
