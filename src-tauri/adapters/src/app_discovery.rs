//! Application discovery for open context capture.
//!
//! This module discovers running user-facing applications and their open project
//! folders. It uses platform-specific APIs to enumerate visible windows/processes
//! without reading browser history, cookies, credentials, or profile databases.
//!
//! The discovery is opt-in and privacy-preserving: only the applications and
//! folders the user explicitly selects are carried into the manifest. Unknown
//! applications are labeled "manual" and restore gracefully reports unavailable
//! apps without blocking other actions.

use crate::traits::{
    ApplicationCategory, ApplicationDiscoveryRequest, ApplicationDiscoveryResult,
    DiscoveredApplication, DiscoveredFolder,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::warn;
use workspace_clone_core::Result;

/// CDP tab structure
#[derive(Deserialize)]
struct CdpTab {
    #[serde(rename = "type")]
    tab_type: String,
    url: String,
    title: Option<String>,
}

#[derive(Deserialize)]
struct CdpVersion {
    #[serde(rename = "Browser")]
    browser: String,
}

#[derive(Deserialize)]
struct SafariTab {
    title: String,
    url: String,
}

const SAFARI_TABS_SCRIPT: &str = r#"
const Safari = Application("Safari");
const tabs = [];
Safari.windows().forEach(function(window) {
  window.tabs().forEach(function(tab) {
    tabs.push({ title: tab.name(), url: tab.url() });
  });
});
JSON.stringify(tabs);
"#;

/// Application discovery adapter - discovers running applications
pub struct AppDiscoveryAdapter;

impl AppDiscoveryAdapter {
    pub fn new() -> Self {
        Self
    }

    /// Discover running applications and their open folders
    pub async fn discover(
        &self,
        request: &ApplicationDiscoveryRequest,
    ) -> Result<ApplicationDiscoveryResult> {
        let mut applications = Vec::new();
        let mut warnings = Vec::new();

        // Discover running applications via platform-specific window enumeration
        let running_apps = self.enumerate_running_applications().await;
        applications.extend(running_apps);

        // The opt-in list represents browsers with tabs we actually read, not
        // browsers that happen to be installed or share a debugging port.
        if request.include_browser_tabs {
            applications.retain(|app| app.category != ApplicationCategory::Browser);
            let (browsers, browser_warnings) =
                self.discover_browser_tabs(&request.browser_ids).await;
            applications.extend(browsers);
            warnings.extend(browser_warnings);
        }

        // Sort by category then name for consistent UI
        applications.sort_by(|a, b| {
            (a.category as u8)
                .cmp(&(b.category as u8))
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(ApplicationDiscoveryResult {
            applications,
            partial: Vec::new(),
            warnings,
        })
    }

    /// Platform-specific running application enumeration
    async fn enumerate_running_applications(&self) -> Vec<DiscoveredApplication> {
        #[cfg(target_os = "windows")]
        {
            self.enumerate_windows_windows().await
        }
        #[cfg(target_os = "macos")]
        {
            self.enumerate_macos_windows().await
        }
        #[cfg(target_os = "linux")]
        {
            self.enumerate_linux_windows().await
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            Vec::new()
        }
    }

    /// Windows: enumerate visible windows and their processes
    #[cfg(target_os = "windows")]
    async fn enumerate_windows_windows(&self) -> Vec<DiscoveredApplication> {
        use windows::Win32::Foundation::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::Win32::System::Threading::*;
        use windows::Win32::System::ProcessStatus::*;
        use windows::Win32::System::LibraryLoader::*;

        let mut apps = HashMap::new();

        unsafe {
            let mut windows = Vec::new();
            EnumWindows(Some(enum_windows_callback), LPARAM(&mut windows as *mut _ as isize)).ok();

            for (hwnd, pid) in windows {
                if IsWindowVisible(hwnd).as_bool() {
                    let mut title = [0u16; 512];
                    let len = GetWindowTextW(hwnd, &mut title);
                    if len > 0 {
                        let title = String::from_utf16_lossy(&title[..len as usize]);
                        if !title.trim().is_empty() {
                            if let Some(process_name) = get_process_name(pid) {
                                let key = process_name.clone();
                                let entry = apps.entry(key).or_insert_with(|| DiscoveredApplication {
                                    id: format!("app-{}", sanitize_id(&process_name)),
                                    name: process_name.clone(),
                                    category: categorize_app(&process_name),
                                    executable_path: get_process_path(pid),
                                    open_folders: Vec::new(),
                                    has_adapter: has_adapter(&process_name),
                                    adapter_id: get_adapter_id(&process_name),
                                });
                                // Try to extract folder from window title
                                if let Some(folder) = extract_folder_from_title(&title, &process_name) {
                                    if folder.exists() {
                                        entry.open_folders.push(Self::analyze_folder_static(&folder));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        apps.into_values().collect()
    }

    /// macOS: enumerate visible windows via CGWindowListCopyWindowInfo
    #[cfg(target_os = "macos")]
    async fn enumerate_macos_windows(&self) -> Vec<DiscoveredApplication> {
        use core_foundation::array::CFArray;
        use core_foundation::base::TCFType;
        use core_foundation::dictionary::CFDictionary;
        use core_foundation::number::CFNumber;
        use core_foundation::string::CFString;
        use core_graphics::window::{
            kCGWindowListOptionOnScreenOnly, kCGNullWindowID, CGWindowListCopyWindowInfo,
        };
        use core_foundation_sys::dictionary::CFDictionaryGetValue;
        use core_foundation_sys::string::CFStringRef;

        let mut apps = HashMap::new();

        unsafe {
            let window_list = CGWindowListCopyWindowInfo(
                kCGWindowListOptionOnScreenOnly,
                kCGNullWindowID,
            );

            if !window_list.is_null() {
                let cf_array: CFArray<CFDictionary> = CFArray::wrap_under_create_rule(window_list);
                let count = cf_array.len();

                for i in 0..count {
                    if let Some(window_info) = cf_array.get(i) {
                        // window_info is a CFDictionaryRef
                        let dict: CFDictionary<CFString, CFString> = CFDictionary::wrap_under_get_rule(window_info.as_concrete_TypeRef());

                        // Get owner PID using raw C API
                        let owner_pid_key = CFString::new("kCGWindowOwnerPID");
                        let owner_pid: i32 = unsafe {
                            let key_ptr = owner_pid_key.as_concrete_TypeRef() as *const std::ffi::c_void;
                            let dict_ptr = dict.as_concrete_TypeRef();
                            let val_ptr = CFDictionaryGetValue(dict_ptr, key_ptr);
                            if !val_ptr.is_null() {
                                let num = CFNumber::wrap_under_get_rule(val_ptr as *const _);
                                num.to_i32().unwrap_or(0)
                            } else { 0 }
                        };

                        // Get window title using raw C API
                        let title_key = CFString::new("kCGWindowName");
                        let window_title: String = unsafe {
                            let key_ptr = title_key.as_concrete_TypeRef() as *const std::ffi::c_void;
                            let dict_ptr = dict.as_concrete_TypeRef();
                            let val_ptr = CFDictionaryGetValue(dict_ptr, key_ptr);
                            if !val_ptr.is_null() {
                                let s = CFString::wrap_under_get_rule(val_ptr as CFStringRef);
                                s.to_string()
                            } else { String::new() }
                        };

                        // Get owner name using raw C API
                        let owner_name_key = CFString::new("kCGWindowOwnerName");
                        let owner_name: String = unsafe {
                            let key_ptr = owner_name_key.as_concrete_TypeRef() as *const std::ffi::c_void;
                            let dict_ptr = dict.as_concrete_TypeRef();
                            let val_ptr = CFDictionaryGetValue(dict_ptr, key_ptr);
                            if !val_ptr.is_null() {
                                let s = CFString::wrap_under_get_rule(val_ptr as CFStringRef);
                                s.to_string()
                            } else { String::new() }
                        };

                        if owner_pid > 0
                            && !owner_name.is_empty()
                            && !window_title.trim().is_empty()
                            && !is_macos_system_process(&owner_name)
                        {
                            let key = owner_name.clone();
                            let entry = apps.entry(key).or_insert_with(|| DiscoveredApplication {
                                id: format!("app-{}", sanitize_id(&owner_name)),
                                name: owner_name.clone(),
                                category: categorize_app(&owner_name),
                                executable_path: get_executable_path_for_pid(owner_pid),
                                open_folders: Vec::new(),
                                has_adapter: has_adapter(&owner_name),
                                adapter_id: get_adapter_id(&owner_name),
                            });
                            // Try to extract folder from window title
                            if let Some(folder) = extract_folder_from_title(&window_title, &owner_name) {
                                if folder.exists() {
                                    entry.open_folders.push(Self::analyze_folder_static(&folder));
                                }
                            }
                        }
                    }
                }
            }
        }

        apps.into_values().collect()
    }

    /// Linux: enumerate visible windows via wmctrl
    #[cfg(target_os = "linux")]
    async fn enumerate_linux_windows(&self) -> Vec<DiscoveredApplication> {
        let mut apps = HashMap::new();

        // Try wmctrl first
        if let Ok(output) = Command::new("wmctrl").args(["-l", "-p"]).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    // Format: 0x03000003  0 12345 hostname window-title
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let pid_str = parts[2];
                        let title = parts[3..].join(" ");
                        if let Ok(pid) = pid_str.parse::<u32>() {
                            if let Some(process_name) = get_linux_process_name(pid) {
                                let key = process_name.clone();
                                let entry = apps.entry(key).or_insert_with(|| DiscoveredApplication {
                                    id: format!("app-{}", sanitize_id(&process_name)),
                                    name: process_name.clone(),
                                    category: categorize_app(&process_name),
                                    executable_path: get_linux_process_exe(pid),
                                    open_folders: Vec::new(),
                                    has_adapter: has_adapter(&process_name),
                                    adapter_id: get_adapter_id(&process_name),
                                });
                                if let Some(folder) = extract_folder_from_title(&title, &process_name) {
                                    if folder.exists() {
                                        entry.open_folders.push(Self::analyze_folder_static(&folder));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        apps.into_values().collect()
    }

    /// Discover browser tabs from Safari or an explicitly enabled Chromium CDP endpoint.
    async fn discover_browser_tabs(
        &self,
        browser_ids: &[String],
    ) -> (Vec<DiscoveredApplication>, Vec<String>) {
        let mut apps = Vec::new();
        let mut warnings = Vec::new();

        let target_browsers: Vec<_> = if browser_ids.is_empty() {
            vec!["safari", "chrome", "edge", "brave", "vivaldi", "opera"]
        } else {
            browser_ids.iter().map(|s| s.as_str()).collect()
        };

        for browser_id in target_browsers {
            let (tabs, warning) = if browser_id == "safari" {
                self.enumerate_safari_tabs().await
            } else {
                (self.enumerate_browser_tabs_cdp(browser_id).await, None)
            };

            if let Some(warning) = warning {
                warnings.push(warning);
            }
            if !tabs.is_empty() {
                apps.push(DiscoveredApplication {
                    id: format!("browser-{browser_id}"),
                    name: Self::browser_display_name(browser_id),
                    category: ApplicationCategory::Browser,
                    executable_path: None,
                    open_folders: tabs,
                    has_adapter: true,
                    adapter_id: Some("browser".to_string()),
                });
            }
        }

        if apps.is_empty() && warnings.is_empty() {
            warnings.push(
                "No open browser tabs were found. Safari must be running and allowed under System Settings > Privacy & Security > Automation. Chromium browsers need remote debugging enabled."
                    .to_string(),
            );
        }

        (apps, warnings)
    }

    #[cfg(target_os = "macos")]
    async fn enumerate_safari_tabs(&self) -> (Vec<DiscoveredFolder>, Option<String>) {
        use tokio::process::Command as TokioCommand;

        let running = TokioCommand::new("/usr/bin/pgrep")
            .args(["-x", "Safari"])
            .output()
            .await
            .is_ok_and(|output| output.status.success());
        if !running {
            return (Vec::new(), None);
        }

        let output = match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            TokioCommand::new("/usr/bin/osascript")
                .args(["-l", "JavaScript", "-e", SAFARI_TABS_SCRIPT])
                .output(),
        )
        .await
        {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                return (
                    Vec::new(),
                    Some(format!("Could not query Safari tabs: {error}")),
                )
            }
            Err(_) => {
                return (
                    Vec::new(),
                    Some("Safari tab discovery timed out.".to_string()),
                )
            }
        };

        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            warn!("Safari tab discovery failed: {detail}");
            return (
                Vec::new(),
                Some(
                    "Safari tabs could not be read. Allow StackHandoff to control Safari in System Settings > Privacy & Security > Automation, then try again."
                        .to_string(),
                ),
            );
        }

        let tabs: Vec<SafariTab> = match serde_json::from_slice(&output.stdout) {
            Ok(tabs) => tabs,
            Err(error) => {
                warn!("Safari returned invalid tab data: {error}");
                return (
                    Vec::new(),
                    Some("Safari returned tab data that StackHandoff could not read.".to_string()),
                );
            }
        };

        (safe_browser_tabs(tabs), None)
    }

    #[cfg(not(target_os = "macos"))]
    async fn enumerate_safari_tabs(&self) -> (Vec<DiscoveredFolder>, Option<String>) {
        (Vec::new(), None)
    }

    /// Enumerate browser tabs via CDP (Chromium-based browsers)
    async fn enumerate_browser_tabs_cdp(&self, browser_id: &str) -> Vec<DiscoveredFolder> {
        let client = reqwest::Client::new();
        let debug_port = find_cdp_port(browser_id);
        if debug_port == 0 {
            return Vec::new();
        }

        let version_url = format!("http://127.0.0.1:{debug_port}/json/version");
        let version: CdpVersion = match client
            .get(&version_url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
            Ok(response) => match response.json().await {
                Ok(version) => version,
                Err(_) => return Vec::new(),
            },
            Err(_) => return Vec::new(),
        };
        if !cdp_browser_matches(browser_id, &version.browser) {
            return Vec::new();
        }

        let tabs_url = format!("http://127.0.0.1:{debug_port}/json/list");
        let tabs: Vec<CdpTab> = match client
            .get(&tabs_url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => match response.json().await {
                Ok(tabs) => tabs,
                Err(_) => return Vec::new(),
            },
            Err(_) => return Vec::new(),
        };

        safe_browser_tabs(
            tabs.into_iter()
            .filter(|t| t.tab_type == "page" && !t.url.is_empty())
            .map(|t| SafariTab {
                title: t.title.unwrap_or_else(|| "Tab".to_string()),
                url: t.url,
            })
            .collect(),
        )
    }

    fn browser_display_name(id: &str) -> String {
        match id {
            "chrome" => "Google Chrome",
            "firefox" => "Mozilla Firefox",
            "edge" => "Microsoft Edge",
            "safari" => "Safari",
            "brave" => "Brave",
            "vivaldi" => "Vivaldi",
            "opera" => "Opera",
            _ => id,
        }
        .to_string()
    }

    /// Check if a URL is safe (http/https only)
    fn is_safe_url(url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else { return false; };
        matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some()
    }

    /// Analyze a folder for git info (static version)
    fn analyze_folder_static(path: &Path) -> DiscoveredFolder {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let is_git_repo = path.join(".git").exists();
        let (git_branch, git_dirty) = if is_git_repo {
            let branch = Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(path)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != "HEAD");

            let dirty = Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(path)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| !o.stdout.is_empty())
                .unwrap_or(false);

            (branch, dirty)
        } else {
            (None, false)
        };

        DiscoveredFolder {
            path: path.to_string_lossy().to_string(),
            name,
            is_git_repo,
            git_branch,
            git_dirty,
        }
    }
}

/// Find CDP debug port for a browser
fn find_cdp_port(browser_id: &str) -> u16 {
    // Common debug ports
    let ports = match browser_id {
        "chrome" => vec![9222, 9223, 9224, 9225, 9226, 9227, 9228, 9229],
        "edge" => vec![9222, 9223, 9224],
        "brave" => vec![9222, 9223],
        "vivaldi" => vec![9222],
        "opera" => vec![9222],
        _ => return 0,
    };

    for port in ports {
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            return port;
        }
    }
    0
}

/// Windows callback for EnumWindows
#[cfg(target_os = "windows")]
unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let windows = &mut *(lparam.0 as *mut Vec<(HWND, u32)>);
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid > 0 {
        windows.push((hwnd, pid));
    }
    TRUE
}

/// Get process name from PID (Windows)
#[cfg(target_os = "windows")]
fn get_process_name(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::*;
    use windows::Win32::Foundation::*;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid).ok()?;
        let mut name = [0u16; 260];
        let mut size = 260;
        if GetModuleFileNameExW(handle, HMODULE(0), &mut name, size).0 > 0 {
            let path = String::from_utf16_lossy(&name);
            Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        } else {
            None
        }
    }
}

/// Get process path from PID (Windows)
#[cfg(target_os = "windows")]
fn get_process_path(pid: u32) -> Option<String> {
    use windows::Win32::System::Threading::*;
    use windows::Win32::Foundation::*;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid).ok()?;
        let mut name = [0u16; 260];
        let mut size = 260;
        if GetModuleFileNameExW(handle, HMODULE(0), &mut name, size).0 > 0 {
            Some(String::from_utf16_lossy(&name))
        } else {
            None
        }
    }
}

/// Get executable path for PID (macOS) - simplified version
#[cfg(target_os = "macos")]
fn get_executable_path_for_pid(pid: i32) -> Option<String> {
    // Use proc_pidpath
    let mut path: Vec<std::os::raw::c_char> = vec![0; 1024];
    let result = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr() as *mut _, path.len() as u32) };
    if result > 0 {
        // Convert i8 to u8 for String::from_utf8_lossy
        let u8_path: Vec<u8> = path[..result as usize].iter().map(|&c| c as u8).collect();
        Some(String::from_utf8_lossy(&u8_path).to_string())
    } else {
        None
    }
}

/// Get Linux process name from PID
#[cfg(target_os = "linux")]
fn get_linux_process_name(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/comm", pid);
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Get Linux process exe path
#[cfg(target_os = "linux")]
fn get_linux_process_exe(pid: u32) -> Option<String> {
    let path = format!("/proc/{}/exe", pid);
    std::fs::read_link(path).ok().map(|p| p.to_string_lossy().to_string())
}

/// Sanitize string for use as ID
fn sanitize_id(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn is_macos_system_process(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "control centre"
            | "control center"
            | "controlcenter"
            | "dock"
            | "finder"
            | "notification centre"
            | "notification center"
            | "notificationcenter"
            | "systemuiserver"
            | "windowserver"
            | "loginwindow"
            | "stackhandoff"
    )
}

fn cdp_browser_matches(browser_id: &str, browser_product: &str) -> bool {
    let product = browser_product.to_ascii_lowercase();
    match browser_id {
        "chrome" => product.contains("chrome") && !product.contains("edg"),
        "edge" => product.contains("edge") || product.contains("edg/"),
        "brave" => product.contains("brave"),
        "vivaldi" => product.contains("vivaldi"),
        "opera" => product.contains("opera"),
        _ => false,
    }
}

fn safe_browser_tabs(tabs: Vec<SafariTab>) -> Vec<DiscoveredFolder> {
    tabs.into_iter()
        .filter(|tab| AppDiscoveryAdapter::is_safe_url(&tab.url))
        .map(|tab| DiscoveredFolder {
            path: tab.url,
            name: if tab.title.trim().is_empty() {
                "Tab".to_string()
            } else {
                tab.title
            },
            is_git_repo: false,
            git_branch: None,
            git_dirty: false,
        })
        .collect()
}

/// Categorize application by name
fn categorize_app(name: &str) -> ApplicationCategory {
    let name_lower = name.to_lowercase();
    if name_lower.contains("code") || name_lower.contains("cursor") || name_lower.contains("vim") || name_lower.contains("sublime") || name_lower.contains("zed") {
        ApplicationCategory::Editor
    } else if name_lower.contains("intellij") || name_lower.contains("pycharm") || name_lower.contains("webstorm") || name_lower.contains("phpstorm") || name_lower.contains("goland") || name_lower.contains("rustrover") || name_lower.contains("datagrip") || name_lower.contains("rubymine") || name_lower.contains("clion") || name_lower.contains("rider") || name_lower.contains("android studio") {
        ApplicationCategory::Ide
    } else if name_lower.contains("terminal") || name_lower.contains("iterm") || name_lower.contains("powershell") || name_lower.contains("cmd") || name_lower.contains("wt.exe") || name_lower.contains("alacritty") || name_lower.contains("kitty") || name_lower.contains("wezterm") {
        ApplicationCategory::Terminal
    } else if name_lower.contains("chrome") || name_lower.contains("firefox") || name_lower.contains("safari") || name_lower.contains("edge") || name_lower.contains("brave") || name_lower.contains("vivaldi") || name_lower.contains("opera") {
        ApplicationCategory::Browser
    } else if name_lower.contains("tableplus") || name_lower.contains("dbeaver") || name_lower.contains("postico") || name_lower.contains("datagrip") {
        ApplicationCategory::Database
    } else if name_lower.contains("figma") || name_lower.contains("sketch") || name_lower.contains("adobe") {
        ApplicationCategory::Design
    } else {
        ApplicationCategory::Other
    }
}

/// Check if app has a dedicated adapter
fn has_adapter(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    name_lower.contains("code") || name_lower.contains("cursor") || 
    name_lower.contains("chrome") || name_lower.contains("firefox") || 
    name_lower.contains("edge") || name_lower.contains("brave") || 
    name_lower.contains("vivaldi") || name_lower.contains("opera") ||
    name_lower.contains("safari")
}

/// Get adapter ID for known apps
fn get_adapter_id(name: &str) -> Option<String> {
    let name_lower = name.to_lowercase();
    if name_lower.contains("code") || name_lower.contains("cursor") {
        Some("vscode".to_string())
    } else if name_lower.contains("chrome") || name_lower.contains("firefox") || 
              name_lower.contains("edge") || name_lower.contains("brave") || 
              name_lower.contains("vivaldi") || name_lower.contains("opera") ||
              name_lower.contains("safari") {
        Some("browser".to_string())
    } else {
        None
    }
}

/// Extract folder path from window title
fn extract_folder_from_title(title: &str, _app_name: &str) -> Option<PathBuf> {
    // Common patterns for extracting paths from window titles
    // VS Code: "folder-name — project-name — Visual Studio Code"
    // JetBrains: "project-name — file-name — IDE-name"
    // Terminal: "user@host: /path/to/folder"
    // Browser: "page-title — domain"
    
    // Try to find a path-like pattern
    let patterns = [
        r#"([A-Za-z]:[\\/][^\\/:*?"<>|]+(?:[\\/][^\\/:*?"<>|]+)*)"#,  // Windows paths
        r#"(/[^/\s:]+(?:/[^/\s:]+)+)"#,  // Unix paths
        r#"(~/[^/\s:]+(?:/[^/\s:]+)*)"#,  // Home-relative paths
    ];

    for pattern in &patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            if let Some(cap) = re.captures(title) {
                if let Some(path_str) = cap.get(1) {
                    let path = PathBuf::from(path_str.as_str());
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }
    }

    None
}

impl AppDiscoveryAdapter {
    /// Analyze a folder for git info (instance method)
    async fn analyze_folder(&self, path: &Path) -> DiscoveredFolder {
        Self::analyze_folder_static(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discover_returns_structured_result() {
        let adapter = AppDiscoveryAdapter::new();
        let result = adapter
            .discover(&ApplicationDiscoveryRequest::default())
            .await
            .unwrap();

        let _ = result.applications.len();
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn is_safe_url_accepts_http_https() {
        assert!(AppDiscoveryAdapter::is_safe_url("http://localhost:3000"));
        assert!(AppDiscoveryAdapter::is_safe_url("https://github.com/acme/api"));
        assert!(AppDiscoveryAdapter::is_safe_url("https://example.com/path?query=1#frag"));
    }

    #[test]
    fn is_safe_url_rejects_dangerous_schemes() {
        assert!(!AppDiscoveryAdapter::is_safe_url("file:///etc/passwd"));
        assert!(!AppDiscoveryAdapter::is_safe_url("javascript:alert(1)"));
        assert!(!AppDiscoveryAdapter::is_safe_url("data:text/html,<script>"));
        assert!(!AppDiscoveryAdapter::is_safe_url("vnd.ms-word:ofe|u|file:///etc/passwd"));
        assert!(!AppDiscoveryAdapter::is_safe_url("smb://evil/share"));
    }

    #[test]
    fn is_safe_url_rejects_malformed() {
        for url in ["", "not a url", "http://", "://missing-scheme", "/just/a/path"] {
            assert!(!AppDiscoveryAdapter::is_safe_url(url), "{url} must be rejected");
        }
    }

    #[test]
    fn sanitize_id_works() {
        assert_eq!(sanitize_id("Visual Studio Code"), "visual-studio-code");
        assert_eq!(sanitize_id("Google Chrome"), "google-chrome");
        assert_eq!(sanitize_id("IntelliJ IDEA"), "intellij-idea");
    }

    #[test]
    fn categorize_app_works() {
        assert_eq!(categorize_app("Visual Studio Code"), ApplicationCategory::Editor);
        assert_eq!(categorize_app("IntelliJ IDEA"), ApplicationCategory::Ide);
        assert_eq!(categorize_app("iTerm2"), ApplicationCategory::Terminal);
        assert_eq!(categorize_app("Google Chrome"), ApplicationCategory::Browser);
        assert_eq!(categorize_app("TablePlus"), ApplicationCategory::Database);
        assert_eq!(categorize_app("Figma"), ApplicationCategory::Design);
        assert_eq!(categorize_app("UnknownApp"), ApplicationCategory::Other);
    }

    #[test]
    fn extract_folder_from_title_vscode() {
        let title = "src — myproject — Visual Studio Code";
        // This test would need a real path to work
        // Just testing the function doesn't panic
        let _ = extract_folder_from_title(title, "Visual Studio Code");
    }

    #[test]
    fn find_cdp_port_returns_zero_when_unavailable() {
        // Port 0 is invalid, so this should return 0
        assert_eq!(find_cdp_port("chrome"), 0);
    }

    #[test]
    fn cdp_tabs_are_only_attributed_to_the_browser_that_serves_them() {
        assert!(cdp_browser_matches("chrome", "Google Chrome/130.0.0.0"));
        assert!(!cdp_browser_matches("chrome", "Microsoft Edge/130.0.0.0"));
        assert!(cdp_browser_matches("edge", "Microsoft Edge/130.0.0.0"));
        assert!(cdp_browser_matches("brave", "Brave/130.0.0.0"));
        assert!(!cdp_browser_matches("safari", "Google Chrome/130.0.0.0"));
    }

    #[test]
    fn browser_tab_results_keep_only_safe_urls_and_use_tab_titles() {
        let tabs = safe_browser_tabs(vec![
            SafariTab {
                title: "Project dashboard".into(),
                url: "https://example.com/dashboard".into(),
            },
            SafariTab {
                title: "Local development".into(),
                url: "http://localhost:3000".into(),
            },
            SafariTab {
                title: "Private file".into(),
                url: "file:///Users/me/private".into(),
            },
        ]);

        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs[0].name, "Project dashboard");
        assert_eq!(tabs[0].path, "https://example.com/dashboard");
        assert_eq!(tabs[1].path, "http://localhost:3000");
    }

    #[test]
    fn macos_system_ui_processes_are_not_reported_as_open_apps() {
        for name in [
            "Control Centre",
            "Dock",
            "Finder",
            "Notification Centre",
            "StackHandoff",
        ] {
            assert!(is_macos_system_process(name), "{name} should be filtered");
        }
        assert!(!is_macos_system_process("Safari"));
        assert!(!is_macos_system_process("Notes"));
    }
}