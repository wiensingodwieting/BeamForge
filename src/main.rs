use serde::{Deserialize, Serialize};
use slint::{ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::UNIX_EPOCH;

slint::include_modules!();

const CONFIG_FILE: &str = "config.json";
const PROFILES_DIR: &str = "profiles";
const CACHE_FILE: &str = "mods_cache.json";

#[derive(Serialize, Deserialize, Default, Clone)]
struct Config {
    storage_path: String,
    target_path: String,
    server_bin_path: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct ModMeta {
    filename: String,
    title: String,
    category: String,
    author: String,
    size_bytes: u64,
    size_str: String,
    mtime: u64,
}

fn sanitize_profile_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .collect::<String>()
        .trim()
        .to_string()
}

fn clean_mod_name(filename: &str) -> String {
    let mut s = filename.trim_end_matches(".zip");
    if s.len() > 9 {
        let (prefix, tail) = s.split_at(s.len() - 9);
        if tail.starts_with('-') && tail[1..].chars().all(|c| c.is_ascii_hexdigit()) {
            s = prefix;
        }
    }
    if s.starts_with('[') {
        if let Some(pos) = s.find(']') {
            s = s[pos + 1..].trim();
        }
    }
    s.replace('_', " ").trim().to_string()
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.0} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    }
}

fn normalize_category(cat: &str) -> String {
    match cat {
        "Авто" | "Cars" | "Car" => "Cars".to_string(),
        "Карта" | "Maps" | "Map" | "Level" | "Track" => "Maps".to_string(),
        "Колёса" | "Wheels" | "Wheel" => "Wheels".to_string(),
        _ => "Other".to_string(),
    }
}

fn parse_mod_zip(path: &Path, mtime: u64, size_bytes: u64) -> ModMeta {
    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    let size_str = format_size(size_bytes);
    let mut title = String::new();
    let mut author = String::new();
    let mut category = String::new();

    let name_lower = filename.to_lowercase();
    if name_lower.contains("[wheels]") || name_lower.contains("wheelpack") || name_lower.contains("wheels") {
        category = "Wheels".into();
    } else if name_lower.contains("[pack]") {
        category = "Other".into();
    }

    if let Ok(file) = fs::File::open(path) {
        if let Ok(mut archive) = zip::ZipArchive::new(file) {
            let mut is_level = false;
            let mut is_vehicle = false;

            for i in 0..archive.len() {
                if let Ok(file) = archive.by_index(i) {
                    if let Ok(name) = file.name() {
                        if name.starts_with("levels/") {
                            is_level = true;
                        }
                        if name.starts_with("vehicles/") {
                            is_vehicle = true;
                        }
                    }
                }
            }

            if is_level {
                category = "Maps".into();
            } else if category.is_empty() && is_vehicle {
                category = "Cars".into();
            }

            for i in 0..archive.len() {
                let is_info = {
                    if let Ok(f) = archive.by_index(i) {
                        if let Ok(name) = f.name() {
                            name.ends_with("/info.json") || name == "info.json" || name == "mod_info.json"
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                if is_info {
                    if let Ok(f) = archive.by_index(i) {
                        let mut buf = Vec::new();
                        let mut take = f.take(65536);
                        if take.read_to_end(&mut buf).is_ok() {
                            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&buf) {
                                if let Some(obj) = val.as_object() {
                                    if title.is_empty() {
                                        if let Some(t) = obj.get("title").and_then(|v| v.as_str()) {
                                            title = t.trim().to_string();
                                        } else if let Some(n) = obj.get("Name").and_then(|v| v.as_str()) {
                                            let brand = obj.get("Brand").and_then(|v| v.as_str()).unwrap_or("");
                                            title = format!("{} {}", brand, n).trim().to_string();
                                        }
                                    }
                                    if author.is_empty() {
                                        if let Some(a) = obj.get("Author").and_then(|v| v.as_str()) {
                                            author = a.trim().to_string();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if category.is_empty() {
        category = "Other".into();
    }
    if title.is_empty() {
        title = clean_mod_name(&filename);
    }

    ModMeta {
        filename,
        title,
        category: normalize_category(&category),
        author,
        size_bytes,
        size_str,
        mtime,
    }
}

struct AppState {
    config: Config,
    all_mods: Vec<ModMeta>,
    active_mods: HashSet<String>,
    presets: Vec<PresetItem>,
    current_preset: String,
    search_query: String,
    category_filter: String,
    sub_filter: String, // "all", "active", "inactive"
    show_settings: bool,
    show_new_profile_input: bool,
    server_child: Option<std::process::Child>,
}

impl AppState {
    fn new() -> Self {
        let _ = fs::create_dir_all(PROFILES_DIR);
        let config: Config = fs::read_to_string(CONFIG_FILE)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();

        let is_first_run = config.storage_path.is_empty() || config.target_path.is_empty();

        let mut state = Self {
            config,
            all_mods: Vec::new(),
            active_mods: HashSet::new(),
            presets: Vec::new(),
            current_preset: String::new(),
            search_query: String::new(),
            category_filter: "All".into(),
            sub_filter: "all".into(),
            show_settings: is_first_run || std::env::args().any(|a| a == "--test-settings"),
            show_new_profile_input: false,
            server_child: None,
        };
        state.scan_mods();
        state.scan_presets();

        if let Some(first) = state.presets.first() {
            let name = first.name.clone();
            state.load_preset(&name);
        }

        state
    }

    fn save_config(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.config) {
            let _ = fs::write(CONFIG_FILE, json);
        }
    }

    fn scan_mods(&mut self) {
        self.all_mods.clear();
        let cache: HashMap<String, ModMeta> = fs::read_to_string(CACHE_FILE)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default();

        let mut new_cache = HashMap::new();
        if let Ok(entries) = fs::read_dir(&self.config.storage_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("zip") {
                    if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                        let meta = fs::metadata(&path).ok();
                        let mtime = meta
                            .as_ref()
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        let size_bytes = meta.map(|m| m.len()).unwrap_or(0);

                        let mut mod_info = if let Some(cached) = cache.get(filename) {
                            if cached.mtime == mtime && cached.size_bytes == size_bytes {
                                cached.clone()
                            } else {
                                parse_mod_zip(&path, mtime, size_bytes)
                            }
                        } else {
                            parse_mod_zip(&path, mtime, size_bytes)
                        };

                        mod_info.category = normalize_category(&mod_info.category);
                        mod_info.size_str = format_size(size_bytes);

                        new_cache.insert(filename.to_string(), mod_info.clone());
                        self.all_mods.push(mod_info);
                    }
                }
            }
        }

        let _ = fs::write(CACHE_FILE, serde_json::to_string(&new_cache).unwrap_or_default());
        self.all_mods.sort_by_key(|a| a.filename.to_lowercase());
        println!("Found {} mods in storage (cache saved to {})", self.all_mods.len(), CACHE_FILE);
    }

    fn scan_presets(&mut self) {
        self.presets.clear();
        if let Ok(entries) = fs::read_dir(PROFILES_DIR) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        let count = fs::read_to_string(&path)
                            .ok()
                            .and_then(|d| serde_json::from_str::<Vec<String>>(&d).ok())
                            .map(|v| v.len() as i32)
                            .unwrap_or(0);
                        self.presets.push(PresetItem {
                            name: stem.into(),
                            count,
                        });
                    }
                }
            }
        }

        self.presets.sort_by(|a, b| a.name.cmp(&b.name));
    }

    fn load_preset(&mut self, name: &str) {
        let safe_name = sanitize_profile_name(name);
        if safe_name.is_empty() { return; }
        self.current_preset = safe_name.clone();
        let path = Path::new(PROFILES_DIR).join(format!("{}.json", safe_name));
        let mods: Vec<String> = fs::read_to_string(path)
            .ok()
            .and_then(|d| serde_json::from_str(&d).ok())
            .unwrap_or_default();
        self.active_mods = mods.into_iter().collect();
    }

    fn save_preset_by_name(&mut self, name: &str) -> Result<(), String> {
        let safe_name = sanitize_profile_name(name);
        if safe_name.is_empty() {
            return Err("Profile name is invalid or empty".into());
        }
        let path = Path::new(PROFILES_DIR).join(format!("{}.json", safe_name));
        let mut mods_vec: Vec<&String> = self.active_mods.iter().collect();
        mods_vec.sort();
        let json = serde_json::to_string_pretty(&mods_vec).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())?;
        self.scan_presets();
        Ok(())
    }

    fn delete_preset_by_name(&mut self, name: &str) {
        let safe_name = sanitize_profile_name(name);
        if safe_name.is_empty() { return; }
        let path = Path::new(PROFILES_DIR).join(format!("{}.json", safe_name));
        let _ = fs::remove_file(path);
        if self.current_preset == safe_name {
            self.current_preset.clear();
        }
        self.scan_presets();
    }

    fn is_server_running(&mut self) -> bool {
        if let Some(child) = self.server_child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.server_child = None;
                }
                Ok(None) => return true,
                Err(_) => {
                    self.server_child = None;
                }
            }
        }

        if let Ok(output) = Command::new("pidof").arg("BeamMP-Server").output() {
            if output.status.success() && !output.stdout.is_empty() {
                return true;
            }
        }
        false
    }

    fn stop_server(&mut self) {
        if let Some(mut child) = self.server_child.take() {
            let _ = child.kill();
        }
        let _ = Command::new("pkill").arg("-f").arg("BeamMP-Server").output();
    }

    fn activate(&self) -> Result<usize, String> {
        let storage = Path::new(&self.config.storage_path);
        let target = Path::new(&self.config.target_path);

        if !storage.is_dir() {
            return Err(format!("Storage directory not found: {}", self.config.storage_path));
        }
        if !target.is_dir() {
            return Err(format!("Target directory not found: {}", self.config.target_path));
        }

        // Safety: verify storage and target do not collide
        if let (Ok(s_canon), Ok(t_canon)) = (fs::canonicalize(storage), fs::canonicalize(target)) {
            if s_canon == t_canon || s_canon.starts_with(&t_canon) {
                return Err("Target path cannot be the same as or a parent of the Storage directory!".into());
            }
        }

        // Clean up outdated symlinks and zip archives in target directory
        let entries = fs::read_dir(target).map_err(|e| e.to_string())?;
        for entry in entries.flatten() {
            let path = entry.path();
            let is_sym = path.is_symlink();
            let is_zip = path.extension().and_then(|e| e.to_str()) == Some("zip");
            if is_sym || is_zip {
                let _ = fs::remove_file(&path);
            }
        }

        let mut count = 0;
        for mod_name in &self.active_mods {
            if !mod_name.ends_with(".zip") || mod_name.contains('/') || mod_name.contains('\\') || mod_name.contains("..") {
                continue;
            }
            let src = storage.join(mod_name);
            let dst = target.join(mod_name);

            if src.exists() {
                let abs_src = fs::canonicalize(&src).map_err(|e| e.to_string())?;
                symlink(&abs_src, &dst).map_err(|e| format!("Failed to create symlink: {}", e))?;
                count += 1;
            }
        }

        Ok(count)
    }

    fn sync_ui(&mut self, ui: &AppWindow) {
        ui.set_storage_path(self.config.storage_path.clone().into());
        ui.set_target_path(self.config.target_path.clone().into());
        ui.set_server_bin_path(self.config.server_bin_path.clone().into());
        ui.set_current_preset(self.current_preset.clone().into());
        ui.set_active_category(self.category_filter.clone().into());
        ui.set_sub_filter(self.sub_filter.clone().into());
        ui.set_show_settings(self.show_settings);
        ui.set_show_new_profile_input(self.show_new_profile_input);

        let running = self.is_server_running();
        ui.set_server_running(running);

        ui.set_presets(ModelRc::new(VecModel::from(self.presets.clone())));

        self.sync_mods_ui(ui);
    }

    fn sync_mods_ui(&self, ui: &AppWindow) {
        let query = self.search_query.to_lowercase();
        let cat = &self.category_filter;
        let sub = &self.sub_filter;

        let total = self.all_mods.len();
        let active = self.active_mods.len();
        let inactive = total.saturating_sub(active);

        let total_active_bytes: u64 = self
            .all_mods
            .iter()
            .filter(|m| self.active_mods.contains(&m.filename))
            .map(|m| m.size_bytes)
            .sum();

        ui.set_total_count(total as i32);
        ui.set_active_count(active as i32);
        ui.set_inactive_count(inactive as i32);
        ui.set_total_size_str(format_size(total_active_bytes).into());

        let filtered: Vec<ModItem> = self
            .all_mods
            .iter()
            .filter(|m| {
                let matches_cat = cat == "All" || m.category == *cat;

                let matches_query = query.is_empty()
                    || m.filename.to_lowercase().contains(&query)
                    || m.title.to_lowercase().contains(&query)
                    || m.category.to_lowercase().contains(&query)
                    || m.author.to_lowercase().contains(&query);

                let matches_sub = match sub.as_str() {
                    "active" => self.active_mods.contains(&m.filename),
                    "inactive" => !self.active_mods.contains(&m.filename),
                    _ => true,
                };

                matches_cat && matches_query && matches_sub
            })
            .map(|m| ModItem {
                name: m.filename.clone().into(),
                title: m.title.clone().into(),
                category: m.category.clone().into(),
                author: m.author.clone().into(),
                size_str: m.size_str.clone().into(),
                checked: self.active_mods.contains(&m.filename),
            })
            .collect();

        ui.set_mods(ModelRc::new(VecModel::from(filtered)));
    }
}

fn show_toast(ui_weak: &slint::Weak<AppWindow>, text: String, is_error: bool) {
    if let Some(ui) = ui_weak.upgrade() {
        ui.set_status_is_error(is_error);
        ui.set_status_text(text.clone().into());
    }
    let weak = ui_weak.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(3000), move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_status_text() == text.as_str() {
                ui.set_status_text("".into());
            }
        }
    });
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let state = Rc::new(RefCell::new(AppState::new()));

    state.borrow_mut().sync_ui(&ui);

    // Background timer to poll server process state
    let timer = slint::Timer::default();
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(1500),
            move || {
                let mut st = state.borrow_mut();
                let running = st.is_server_running();
                if let Some(ui) = ui_weak.upgrade() {
                    if ui.get_server_running() != running {
                        ui.set_server_running(running);
                    }
                }
            },
        );
    }

    // Preset selection
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_select_preset(move |name| {
            let mut st = state.borrow_mut();
            st.load_preset(&name);
            if let Some(ui) = ui_weak.upgrade() {
                st.sync_ui(&ui);
                ui.set_status_is_error(false);
                ui.set_status_text("".into());
            }
        });
    }

    // Save active preset
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_save_preset_by_name(move |name| {
            let mut st = state.borrow_mut();
            let count = st.active_mods.len();
            if let Some(ui) = ui_weak.upgrade() {
                match st.save_preset_by_name(&name) {
                    Ok(_) => {
                        st.sync_ui(&ui);
                        show_toast(&ui_weak, format!("Saved {} mods to profile '{}'", count, name), false);
                    }
                    Err(e) => {
                        show_toast(&ui_weak, format!("Error saving profile: {}", e), true);
                    }
                }
            }
        });
    }

    // Delete preset
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_delete_preset_by_name(move |name| {
            let mut st = state.borrow_mut();
            st.delete_preset_by_name(&name);
            if let Some(ui) = ui_weak.upgrade() {
                st.sync_ui(&ui);
                show_toast(&ui_weak, format!("Deleted profile '{}'", name), false);
            }
        });
    }

    // Create preset
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_create_preset(move |name| {
            let clean_name = sanitize_profile_name(&name);
            if clean_name.is_empty() { return; }
            let mut st = state.borrow_mut();
            st.current_preset = clean_name.clone();
            let _ = st.save_preset_by_name(&clean_name);
            st.show_new_profile_input = false;
            if let Some(ui) = ui_weak.upgrade() {
                st.sync_ui(&ui);
                show_toast(&ui_weak, format!("Created profile '{}'", clean_name), false);
            }
        });
    }

    // Toggle new profile input row
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_toggle_new_profile(move || {
            let mut st = state.borrow_mut();
            st.show_new_profile_input = !st.show_new_profile_input;
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_new_profile_input(st.show_new_profile_input);
            }
        });
    }

    // Search query
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_search_changed(move |query| {
            let mut st = state.borrow_mut();
            st.search_query = query.to_string();
            if let Some(ui) = ui_weak.upgrade() {
                st.sync_mods_ui(&ui);
            }
        });
    }

    // Category filter
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_filter_category(move |cat| {
            let mut st = state.borrow_mut();
            st.category_filter = cat.to_string();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_active_category(cat);
                st.sync_mods_ui(&ui);
            }
        });
    }

    // Sub filter (all / active / inactive)
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_filter_sub(move |sub| {
            let mut st = state.borrow_mut();
            st.sub_filter = sub.to_string();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_sub_filter(sub);
                st.sync_mods_ui(&ui);
            }
        });
    }

    // Toggle mod item
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_toggle_mod(move |name, checked| {
            let mut st = state.borrow_mut();
            if checked {
                st.active_mods.insert(name.to_string());
            } else {
                st.active_mods.remove(name.as_str());
            }
            if let Some(ui) = ui_weak.upgrade() {
                st.sync_mods_ui(&ui);
            }
        });
    }

    // Select all
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_select_all(move || {
            let mut st = state.borrow_mut();
            let q = st.search_query.to_lowercase();
            let cat = st.category_filter.clone();
            let to_add: Vec<String> = st
                .all_mods
                .iter()
                .filter(|m| {
                    let matches_cat = cat == "All" || m.category == cat;
                    let matches_query = q.is_empty()
                        || m.filename.to_lowercase().contains(&q)
                        || m.title.to_lowercase().contains(&q)
                        || m.category.to_lowercase().contains(&q)
                        || m.author.to_lowercase().contains(&q);
                    matches_cat && matches_query
                })
                .map(|m| m.filename.clone())
                .collect();

            for m in to_add {
                st.active_mods.insert(m);
            }
            if let Some(ui) = ui_weak.upgrade() {
                st.sync_mods_ui(&ui);
            }
        });
    }

    // Deselect all
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_deselect_all(move || {
            let mut st = state.borrow_mut();
            let q = st.search_query.to_lowercase();
            let cat = st.category_filter.clone();
            let to_remove: Vec<String> = st
                .all_mods
                .iter()
                .filter(|m| {
                    let matches_cat = cat == "All" || m.category == cat;
                    let matches_query = q.is_empty()
                        || m.filename.to_lowercase().contains(&q)
                        || m.title.to_lowercase().contains(&q)
                        || m.category.to_lowercase().contains(&q)
                        || m.author.to_lowercase().contains(&q);
                    matches_cat && matches_query
                })
                .map(|m| m.filename.clone())
                .collect();

            for m in to_remove {
                st.active_mods.remove(&m);
            }
            if let Some(ui) = ui_weak.upgrade() {
                st.sync_mods_ui(&ui);
            }
        });
    }

    // Refresh mods
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_refresh_mods(move || {
            let mut st = state.borrow_mut();
            st.scan_mods();
            if let Some(ui) = ui_weak.upgrade() {
                st.sync_mods_ui(&ui);
                show_toast(&ui_weak, format!("Refreshed storage: {} mods found", st.all_mods.len()), false);
            }
        });
    }

    // Apply active profile
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_apply_mods(move || {
            let mut st = state.borrow_mut();
            if !st.current_preset.is_empty() {
                let cur = st.current_preset.clone();
                let _ = st.save_preset_by_name(&cur);
            }
            if let Some(ui) = ui_weak.upgrade() {
                match st.activate() {
                    Ok(count) => {
                        st.sync_ui(&ui);
                        show_toast(&ui_weak, format!("Applied! Created {} symlinks in Resources/Client", count), false);
                    }
                    Err(e) => {
                        show_toast(&ui_weak, format!("Apply error: {}", e), true);
                    }
                }
            }
        });
    }

    // Server launcher / process supervisor
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_toggle_server(move || {
            let mut st = state.borrow_mut();
            let running = st.is_server_running();

            if running {
                st.stop_server();
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_server_running(false);
                    show_toast(&ui_weak, "BeamMP Server stopped".into(), false);
                }
            } else {
                if let Err(e) = st.activate() {
                    show_toast(&ui_weak, format!("Activation error before launch: {}", e), true);
                    return;
                }

                let bin = PathBuf::from(&st.config.server_bin_path);
                if !bin.is_file() {
                    show_toast(&ui_weak, "Error: Server executable / script not found!".into(), true);
                    return;
                }

                let work_dir = bin.parent().unwrap_or(Path::new("."));
                let is_sh = bin.extension().and_then(|e| e.to_str()) == Some("sh");

                let mut cmd = if is_sh {
                    let mut c = Command::new("bash");
                    c.arg(&bin);
                    c
                } else {
                    Command::new(&bin)
                };
                cmd.current_dir(work_dir);

                match cmd.spawn() {
                    Ok(child) => {
                        st.server_child = Some(child);
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_server_running(true);
                            show_toast(&ui_weak, "BeamMP Server started successfully!".into(), false);
                        }
                    }
                    Err(e) => {
                        show_toast(&ui_weak, format!("Failed to start server: {}", e), true);
                    }
                }
            }
        });
    }

    // Open ServerConfig.toml
    {
        let state = state.clone();
        ui.on_open_server_config(move || {
            let st = state.borrow();
            let bin = PathBuf::from(&st.config.server_bin_path);
            let dir = bin.parent().unwrap_or(Path::new("."));
            let cfg = dir.join("ServerConfig.toml");
            if cfg.exists() {
                let _ = Command::new("xdg-open").arg(&cfg).spawn();
            } else {
                let _ = Command::new("xdg-open").arg(dir).spawn();
            }
        });
    }

    // Reveal mod in file manager
    {
        let state = state.clone();
        ui.on_open_mod_file(move |mod_name| {
            let name_str = mod_name.as_str();
            if name_str.contains('/') || name_str.contains('\\') || name_str.contains("..") {
                return;
            }
            let st = state.borrow();
            let storage = Path::new(&st.config.storage_path);
            let file = storage.join(name_str);
            if file.exists() {
                let _ = Command::new("xdg-open").arg(&file).spawn();
            } else if storage.exists() {
                let _ = Command::new("xdg-open").arg(storage).spawn();
            }
        });
    }

    // Toggle settings modal
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_toggle_settings(move || {
            let mut st = state.borrow_mut();
            st.show_settings = !st.show_settings;
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_settings(st.show_settings);
            }
        });
    }

    // Pick storage folder
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_pick_storage(move || {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                let mut st = state.borrow_mut();
                st.config.storage_path = folder.to_string_lossy().to_string();
                st.save_config();
                st.scan_mods();
                if let Some(ui) = ui_weak.upgrade() {
                    st.sync_ui(&ui);
                }
            }
        });
    }

    // Pick server target folder
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_pick_target(move || {
            if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                let mut st = state.borrow_mut();
                st.config.target_path = folder.to_string_lossy().to_string();
                st.save_config();
                if let Some(ui) = ui_weak.upgrade() {
                    st.sync_ui(&ui);
                }
            }
        });
    }

    // Pick server executable
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_pick_server_bin(move || {
            if let Some(file) = rfd::FileDialog::new().pick_file() {
                let mut st = state.borrow_mut();
                st.config.server_bin_path = file.to_string_lossy().to_string();
                st.save_config();
                if let Some(ui) = ui_weak.upgrade() {
                    st.sync_ui(&ui);
                }
            }
        });
    }

    ui.run()
}
