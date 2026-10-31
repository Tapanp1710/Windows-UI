//! Everything installed, the way the Start menu sees it.
//!
//! Enumerating `shell:AppsFolder` rather than walking Start Menu folders for `.lnk`
//! files: the AppsFolder is the shell's own list, so it includes Store and packaged
//! apps, which have no shortcut on disk, and it gives each one a parsing name that
//! `ShellExecuteW` can launch directly - the same path the dock already uses.
//!
//! Icons are taken from each `IShellItem` as it is enumerated, rather than looked up
//! again later by name: the parsing names the AppsFolder hands out do not all
//! round-trip through `SHCreateItemFromParsingName`, and fetching them that way left
//! most apps with a blank page icon. The whole enumeration runs once, on a background
//! thread at startup, so the second it costs is spent long before the Win key is
//! pressed.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub name: String,
    /// `shell:AppsFolder\<AUMID>` or a path; whatever the shell wants back.
    pub parsing_name: String,
    pub icon: Option<String>,
}

/// Icon edge length for launcher entries.
///
/// Smaller than the dock's 256: a couple of hundred of these cross the IPC boundary
/// in one message, and they are drawn at 32 CSS pixels.
const LAUNCHER_ICON_PX: i32 = 96;

/// Every installed app, sorted by name, case-insensitively.
pub fn all() -> Vec<App> {
    let mut apps = read().unwrap_or_default();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.parsing_name == b.parsing_name);
    apps
}

fn read() -> windows::core::Result<Vec<App>> {
    use windows::core::w;
    use windows::Win32::UI::Shell::{
        IEnumShellItems, IShellItem, SHCreateItemFromParsingName, BHID_EnumItems,
        SIGDN_NORMALDISPLAY, SIGDN_PARENTRELATIVEPARSING,
    };

    crate::win::init_com();

    let mut out = Vec::new();
    unsafe {
        let folder: IShellItem = SHCreateItemFromParsingName(w!("shell:AppsFolder"), None)?;
        let items: IEnumShellItems = folder.BindToHandler(None, &BHID_EnumItems)?;

        loop {
            let mut batch: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            // Next returns S_FALSE at the end, which is "ok" but fetches nothing.
            if items.Next(&mut batch, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = batch[0].take() else { break };

            let Ok(name) = item.GetDisplayName(SIGDN_NORMALDISPLAY) else {
                continue;
            };
            // PARENTRELATIVE gives the bare AUMID, which prefixed with
            // `shell:AppsFolder\` is the form both ShellExecuteW and the icon
            // extractor understand. The DESKTOPABSOLUTE form comes back as a raw
            // `::{CLSID}\AUMID` path that the icon extractor could not resolve -
            // which is why almost every app came up with a blank page icon.
            let Ok(parsing) = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING) else {
                continue;
            };

            let name = name.to_string().unwrap_or_default();
            let aumid = parsing.to_string().unwrap_or_default();
            let parsing_name = if aumid.is_empty() {
                String::new()
            } else {
                ["shell:AppsFolder\\", &aumid].concat()
            };
            if name.is_empty() || parsing_name.is_empty() {
                continue;
            }
            let icon = crate::win::icons::icon_data_url_from_item(&item, LAUNCHER_ICON_PX);
            out.push(App {
                name,
                parsing_name,
                icon,
            });
        }
    }
    Ok(out)
}

/// One entry in a folder listing, for the launcher's sidebar.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

/// The well-known folders the sidebar offers, in the order the brief lists them.
pub fn shell_folders() -> Vec<(String, String)> {
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    ["Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos"]
        .iter()
        .map(|n| (n.to_string(), format!("{home}\\{n}")))
        .filter(|(_, p)| std::path::Path::new(p).is_dir())
        .collect()
}

/// List a folder, directories first then files, both alphabetical.
///
/// Hidden and system entries are left out, which is what Explorer shows by default.
pub fn list_dir(path: &str) -> Vec<FileEntry> {
    let Ok(rd) = std::fs::read_dir(path) else {
        return Vec::new();
    };

    let mut out: Vec<FileEntry> = rd
        .flatten()
        .filter(|e| !is_hidden(e))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            Some(FileEntry {
                is_dir: e.file_type().ok()?.is_dir(),
                path: e.path().to_string_lossy().to_string(),
                name,
            })
        })
        .collect();

    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out.truncate(500);
    out
}

fn is_hidden(e: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    e.metadata()
        .map(|m| m.file_attributes() & (HIDDEN | SYSTEM) != 0)
        .unwrap_or(false)
}

/// Search the user's folders for files whose name contains `needle`.
///
/// A bounded breadth-first walk rather than a Windows Search query: the search
/// index is reached through OLE DB, which is a great deal of surface area for
/// "find files whose name contains this". The walk is capped by depth, by result
/// count and by a time budget, so a deep Downloads folder cannot make the launcher
/// feel slow.
pub fn search_files(needle: &str, limit: usize) -> Vec<FileEntry> {
    use std::collections::VecDeque;
    use std::time::{Duration, Instant};

    let needle = needle.to_lowercase();
    if needle.len() < 2 {
        return Vec::new();
    }

    let deadline = Instant::now() + Duration::from_millis(350);
    let mut queue: VecDeque<(std::path::PathBuf, u32)> = shell_folders()
        .into_iter()
        .map(|(_, p)| (std::path::PathBuf::from(p), 0u32))
        .collect();

    let mut out = Vec::new();
    while let Some((dir, depth)) = queue.pop_front() {
        if out.len() >= limit || Instant::now() > deadline {
            break;
        }
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            if is_hidden(&e) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if name.to_lowercase().contains(&needle) {
                out.push(FileEntry {
                    name,
                    path: e.path().to_string_lossy().to_string(),
                    is_dir,
                });
                if out.len() >= limit {
                    break;
                }
            } else if is_dir && depth < 3 {
                queue.push_back((e.path(), depth + 1));
            }
        }
    }
    out
}
