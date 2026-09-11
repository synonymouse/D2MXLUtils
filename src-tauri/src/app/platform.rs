//! Environment and privilege preparation before desktop initialization.

#[cfg(target_os = "windows")]
use crate::logger::error as log_error;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{BOOL, HANDLE};
#[cfg(target_os = "windows")]
use windows::Win32::Security::{
    AdjustTokenPrivileges, GetTokenInformation, LookupPrivilegeValueW, TokenElevationType,
    TokenLinkedToken, LUID_AND_ATTRIBUTES, SE_DEBUG_NAME, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_ELEVATION_TYPE, TOKEN_LINKED_TOKEN, TOKEN_PRIVILEGES,
    TOKEN_QUERY,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Com::CoTaskMemFree;
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

pub(crate) fn prepare_environment() {
    // Force XWayland instead of native Wayland for GTK/webkit2gtk. Native
    // Wayland gives client apps no control over top-level window position
    // at all (no X11-RANDR-style settable (x, y) — only the compositor
    // decides), which breaks placing the overlay in a screen corner and,
    // on unusual multi-monitor layouts, can land the default position on a
    // monitor the user isn't even looking at. XWayland restores normal
    // X11 positioning semantics; must be set before GTK initializes.
    #[cfg(target_os = "linux")]
    if std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }

    // NVIDIA's proprietary driver has long had incomplete/buggy DMA-BUF
    // export support, which is what WebKitGTK's hardware compositing path
    // relies on. On affected setups (confirmed: NVIDIA + Wayland session,
    // even with GDK_BACKEND forced to x11 above) this doesn't crash or log
    // anything from WebKit — the window just paints its background and
    // never draws page content, i.e. a silent white screen. This is a
    // lightweight overlay UI, not a GPU-heavy page, so there's no real
    // cost to disabling the DMA-BUF renderer unconditionally.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

/// Enable SeDebugPrivilege on the current process token, if possible.
///
/// This matches what many memory tools (including the original AutoIt-based D2Stats)
/// do before calling OpenProcess on game processes. Without this privilege, some
/// Windows configurations may return ACCESS_DENIED even for the same user.
#[cfg(target_os = "windows")]
pub(crate) fn enable_debug_privilege() {
    use std::mem::size_of;
    use windows::Win32::Foundation::{CloseHandle, LUID};

    unsafe {
        let mut token_handle = HANDLE::default();
        // We need both QUERY and ADJUST_PRIVILEGES to toggle SeDebugPrivilege.
        let desired_access = TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY;
        if let Err(e) = OpenProcessToken(GetCurrentProcess(), desired_access, &mut token_handle) {
            log_error(&format!("SeDebugPrivilege: OpenProcessToken failed: {}", e));
            return;
        }

        // Resolve the LUID for SeDebugPrivilege.
        let mut luid = LUID::default();
        if let Err(e) = LookupPrivilegeValueW(None, SE_DEBUG_NAME, &mut luid) {
            log_error(&format!(
                "SeDebugPrivilege: LookupPrivilegeValueW failed: {}",
                e
            ));
            let _ = CloseHandle(token_handle);
            return;
        }

        let mut tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };

        // Enable SeDebugPrivilege on this token.
        let result = AdjustTokenPrivileges(
            token_handle,
            BOOL(0),
            Some(&tp as *const TOKEN_PRIVILEGES),
            size_of::<TOKEN_PRIVILEGES>() as u32,
            None,
            None,
        );

        let _ = CloseHandle(token_handle);

        if let Err(e) = result {
            log_error(&format!(
                "SeDebugPrivilege: AdjustTokenPrivileges failed: {}",
                e
            ));
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn enable_debug_privilege() {
    // No-op on non-Windows platforms.
}

/// Configure WebView2 user data folder for elevated processes.
///
/// When running with administrator privileges (elevated), WebView2 may fail
/// to access the user's LocalAppData because the elevated process runs under
/// a different user context. This function detects elevation and sets
/// WEBVIEW2_USER_DATA_FOLDER to the non-elevated user's LocalAppData path.
#[cfg(target_os = "windows")]
pub(crate) fn setup_webview2_for_elevation() {
    use std::mem::size_of;

    unsafe {
        // Get the current process token
        let mut token_handle = HANDLE::default();
        if let Err(e) = OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle) {
            log_error(&format!(
                "WebView2 setup: OpenProcessToken failed, skipping elevation check: {}",
                e
            ));
            return;
        }

        // Check elevation type
        let mut elevation_type = TOKEN_ELEVATION_TYPE::default();
        let mut return_length = 0u32;

        let result = GetTokenInformation(
            token_handle,
            TokenElevationType,
            Some(&mut elevation_type as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION_TYPE>() as u32,
            &mut return_length,
        );

        if let Err(e) = result {
            log_error(&format!(
                "WebView2 setup: GetTokenInformation(TokenElevationType) failed: {}",
                e
            ));
            let _ = windows::Win32::Foundation::CloseHandle(token_handle);
            return;
        }

        // TokenElevationTypeFull (2) means the process is elevated via UAC
        // We need to get the linked token (non-elevated user token) to find correct AppData
        if elevation_type.0 != 2 {
            // Not elevated via UAC, no need to adjust WebView2 path
            let _ = windows::Win32::Foundation::CloseHandle(token_handle);
            return;
        }

        // Get the linked token (the non-elevated user token)
        let mut linked_token = TOKEN_LINKED_TOKEN::default();
        let mut return_length = 0u32;

        let result = GetTokenInformation(
            token_handle,
            TokenLinkedToken,
            Some(&mut linked_token as *mut _ as *mut _),
            size_of::<TOKEN_LINKED_TOKEN>() as u32,
            &mut return_length,
        );

        let _ = windows::Win32::Foundation::CloseHandle(token_handle);

        if let Err(e) = result {
            log_error(&format!(
                "WebView2 setup: GetTokenInformation(TokenLinkedToken) failed: {}",
                e
            ));
            return;
        }

        // Get LocalAppData path using the linked (non-elevated) token
        let path_ptr = SHGetKnownFolderPath(
            &FOLDERID_LocalAppData,
            KF_FLAG_DEFAULT,
            linked_token.LinkedToken,
        );

        let _ = windows::Win32::Foundation::CloseHandle(linked_token.LinkedToken);

        match path_ptr {
            Ok(ptr) => {
                // Convert PWSTR to Rust String
                let path_str = ptr.to_string().unwrap_or_default();
                CoTaskMemFree(Some(ptr.as_ptr() as *const _));

                if !path_str.is_empty() {
                    // Construct WebView2 data folder path
                    let webview2_path = format!("{}\\D2MXLUtils\\WebView2", path_str);
                    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview2_path);
                }
            }
            Err(e) => {
                log_error(&format!(
                    "WebView2 setup: SHGetKnownFolderPath(LocalAppData) failed: {:?}",
                    e
                ));
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn setup_webview2_for_elevation() {
    // No-op on non-Windows platforms
}
