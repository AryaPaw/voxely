//! Verify the linked executable and its real activation context, not source XML alone.
use windows::core::{s, w, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    FindResourceW, GetModuleHandleW, GetProcAddress, LoadLibraryW, LoadResource, LockResource,
    SizeofResource,
};
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, GetDpiAwarenessContextForProcess,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{CREATEPROCESS_MANIFEST_RESOURCE_ID, RT_MANIFEST};

#[test]
fn process_manifest_declares_common_controls_v6_and_per_monitor_v2() {
    unsafe {
        let executable = GetModuleHandleW(PCWSTR::null()).expect("current executable module");
        let resource = FindResourceW(
            executable,
            PCWSTR(CREATEPROCESS_MANIFEST_RESOURCE_ID as usize as *const u16),
            RT_MANIFEST,
        );
        assert!(
            !resource.is_invalid(),
            "executable has no process activation manifest"
        );
        let length = SizeofResource(executable, resource) as usize;
        assert!(
            (1..=64 * 1024).contains(&length),
            "invalid manifest resource size"
        );
        let loaded = LoadResource(executable, resource).expect("manifest resource bytes");
        let bytes = LockResource(loaded) as *const u8;
        assert!(!bytes.is_null(), "manifest resource wasn't mapped");
        let xml = std::str::from_utf8(std::slice::from_raw_parts(bytes, length))
            .expect("manifest resource is UTF-8");
        for required in [
            "name=\"Voxely.Desktop.Application\"",
            "version=\"1.0.0.0\"",
            "Microsoft.Windows.Common-Controls",
            "version=\"6.0.0.0\"",
            "level=\"asInvoker\"",
            "uiAccess=\"false\"",
            ">PerMonitorV2<",
            ">true/pm<",
            ">SegmentHeap<",
        ] {
            assert!(
                xml.contains(required),
                "embedded manifest is missing {required}"
            );
        }
        assert!(
            AreDpiAwarenessContextsEqual(
                GetDpiAwarenessContextForProcess(GetCurrentProcess()),
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            )
            .as_bool(),
            "process activation didn't enable per-monitor v2 DPI awareness"
        );
    }
}

struct LoadedModule(HMODULE);
impl Drop for LoadedModule {
    fn drop(&mut self) {
        unsafe {
            let _ = FreeLibrary(self.0);
        }
    }
}

#[repr(C)]
#[derive(Default)]
struct DllVersionInfo {
    size: u32,
    major: u32,
    minor: u32,
    build: u32,
    platform: u32,
}

#[test]
fn common_controls_v6_is_active_and_task_dialog_entry_point_exists() {
    unsafe {
        // Resolve by the normal activation context, without a WinSxS path or a
        // temporary activation override that could hide a broken EXE manifest.
        let controls =
            LoadedModule(LoadLibraryW(w!("comctl32.dll")).expect("load common controls"));
        let get_version = GetProcAddress(controls.0, s!("DllGetVersion"))
            .expect("common controls version entry point");
        let get_version: unsafe extern "system" fn(*mut DllVersionInfo) -> i32 =
            std::mem::transmute(get_version);
        let mut version = DllVersionInfo {
            size: std::mem::size_of::<DllVersionInfo>() as u32,
            ..Default::default()
        };
        assert_eq!(get_version(&mut version), 0, "DllGetVersion failed");
        assert_eq!(version.major, 6, "process loaded legacy common controls");
        assert!(
            GetProcAddress(controls.0, s!("TaskDialogIndirect")).is_some(),
            "activated common controls lacks TaskDialogIndirect"
        );
    }
}
