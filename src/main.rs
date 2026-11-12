use serde::{Deserialize, Serialize};
use slint::{ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::Receiver;
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
    #[serde(default)]
    map_path: String,
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
    let mut map_path = String::new();

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
                            if map_path.is_empty() {
                                if name.ends_with("/info.json") && name.matches('/').count() == 2 {
                                    map_path = format!("/{}", name);
                                } else {
                                    let parts: Vec<&str> = name.split('/').collect();
                                    if parts.len() >= 2 && !parts[1].is_empty() && parts[1] != "info.json" {
                                        map_path = format!("/levels/{}/info.json", parts[1]);
                                    }
                                }
                            }
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
                            if title.is_empty() {
                                let text = String::from_utf8_lossy(&buf);
                                if let Some(pos) = text.find("\"title\"") {
                                    if let Some(colon) = text[pos..].find(':') {
                                        let after = &text[pos + colon + 1..];
                                        if let Some(q1) = after.find('"') {
                                            if let Some(q2) = after[q1 + 1..].find('"') {
                                                let t = after[q1 + 1..q1 + 1 + q2].trim();
                                                if !t.is_empty() && !t.contains('\n') {
                                                    title = t.to_string();
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
        map_path,
    }
}

#[derive(Clone, Default, Debug)]
struct ServerConfigData {
    name: String,
    description: String,
    map: String,
    max_players: String,
    max_cars: String,
    port: String,
    private: bool,
    auth_key: String,
}

fn load_server_config_toml(path: &Path) -> ServerConfigData {
    let mut data = ServerConfigData {
        name: "BeamMP Server".into(),
        description: "BeamMP Server".into(),
        map: "/levels/west_coast_usa/info.json".into(),
        max_players: "8".into(),
        max_cars: "3".into(),
        port: "30814".into(),
        private: true,
        auth_key: String::new(),
    };

    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let key = k.trim();
                let val = v.trim().trim_matches('"');
                match key {
                    "Name" => data.name = val.to_string(),
                    "Description" => data.description = val.to_string(),
                    "Map" => data.map = val.to_string(),
                    "MaxPlayers" => data.max_players = val.to_string(),
                    "MaxCars" => data.max_cars = val.to_string(),
                    "Port" => data.port = val.to_string(),
                    "Private" => data.private = val.parse::<bool>().unwrap_or(true),
                    "AuthKey" => data.auth_key = val.to_string(),
                    _ => {}
                }
            }
        }
    }
    data
}

fn sanitize_toml_str(val: &str) -> String {
    val.replace('\r', "")
       .replace('\n', " ")
       .replace('\\', "\\\\")
       .replace('"', "\\\"")
}

fn save_server_config_toml(
    path: &Path,
    name: &str,
    description: &str,
    map: &str,
    max_players: &str,
    max_cars: &str,
    port: &str,
    private: bool,
    auth_key: &str,
) -> Result<(), String> {
    let s_name = sanitize_toml_str(name);
    let s_desc = sanitize_toml_str(description);
    let s_map = sanitize_toml_str(map);
    let s_auth = sanitize_toml_str(auth_key.trim());
    let u_players = max_players.trim().parse::<u32>().unwrap_or(8);
    let u_cars = max_cars.trim().parse::<u32>().unwrap_or(3);
    let u_port = port.trim().parse::<u32>().unwrap_or(30814);

    let content = fs::read_to_string(path).unwrap_or_default();
    let mut new_lines = Vec::new();
    let mut found_name = false;
    let mut found_desc = false;
    let mut found_map = false;
    let mut found_max_players = false;
    let mut found_max_cars = false;
    let mut found_port = false;
    let mut found_private = false;
    let mut found_auth = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Name ") || trimmed.starts_with("Name=") {
            new_lines.push(format!("Name = \"{}\"", s_name));
            found_name = true;
        } else if trimmed.starts_with("Description ") || trimmed.starts_with("Description=") {
            new_lines.push(format!("Description = \"{}\"", s_desc));
            found_desc = true;
        } else if trimmed.starts_with("Map ") || trimmed.starts_with("Map=") {
            new_lines.push(format!("Map = \"{}\"", s_map));
            found_map = true;
        } else if trimmed.starts_with("MaxPlayers ") || trimmed.starts_with("MaxPlayers=") {
            new_lines.push(format!("MaxPlayers = {}", u_players));
            found_max_players = true;
        } else if trimmed.starts_with("MaxCars ") || trimmed.starts_with("MaxCars=") {
            new_lines.push(format!("MaxCars = {}", u_cars));
            found_max_cars = true;
        } else if trimmed.starts_with("Port ") || trimmed.starts_with("Port=") {
            new_lines.push(format!("Port = {}", u_port));
            found_port = true;
        } else if trimmed.starts_with("Private ") || trimmed.starts_with("Private=") {
            new_lines.push(format!("Private = {}", private));
            found_private = true;
        } else if trimmed.starts_with("AuthKey ") || trimmed.starts_with("AuthKey=") {
            new_lines.push(format!("AuthKey = \"{}\"", s_auth));
            found_auth = true;
        } else {
            new_lines.push(line.to_string());
        }
    }

    if !found_name { new_lines.push(format!("Name = \"{}\"", s_name)); }
    if !found_desc { new_lines.push(format!("Description = \"{}\"", s_desc)); }
    if !found_map { new_lines.push(format!("Map = \"{}\"", s_map)); }
    if !found_max_players { new_lines.push(format!("MaxPlayers = {}", u_players)); }
    if !found_max_cars { new_lines.push(format!("MaxCars = {}", u_cars)); }
    if !found_port { new_lines.push(format!("Port = {}", u_port)); }
    if !found_private { new_lines.push(format!("Private = {}", private)); }
    if !found_auth { new_lines.push(format!("AuthKey = \"{}\"", s_auth)); }

    fs::write(path, new_lines.join("\n") + "\n").map_err(|e| e.to_string())
}

const VANILLA_MAPS: &[(&str, &str, &str, &[u8])] = &[
    ("West Coast, USA", "/levels/west_coast_usa/info.json", "west_coast_usa.jpg", include_bytes!("../ui/maps/west_coast_usa.jpg")),
    ("Gridmap v2", "/levels/gridmap_v2/info.json", "gridmap_v2.jpg", include_bytes!("../ui/maps/gridmap_v2.jpg")),
    ("Utah, USA", "/levels/utah/info.json", "utah.jpg", include_bytes!("../ui/maps/utah.jpg")),
    ("East Coast, USA", "/levels/east_coast_usa/info.json", "east_coast_usa.jpg", include_bytes!("../ui/maps/east_coast_usa.jpg")),
    ("Italy", "/levels/italy/info.json", "italy.jpg", include_bytes!("../ui/maps/italy.jpg")),
    ("Jungle Rock Island", "/levels/jungle_rock_island/info.json", "jungle_rock_island.jpg", include_bytes!("../ui/maps/jungle_rock_island.jpg")),
    ("Industrial Site", "/levels/industrial/info.json", "industrial.jpg", include_bytes!("../ui/maps/industrial.jpg")),
    ("Small Island, USA", "/levels/small_island/info.json", "small_island.jpg", include_bytes!("../ui/maps/small_island.jpg")),
    ("Johnson Valley", "/levels/johnson_valley/info.json", "johnson_valley.jpg", include_bytes!("../ui/maps/johnson_valley.jpg")),
    ("Automation Test Track", "/levels/automation_test_track/info.json", "automation_test_track.jpg", include_bytes!("../ui/maps/automation_test_track.jpg")),
    ("Hirochi Raceway", "/levels/hirochi_raceway/info.json", "hirochi_raceway.jpg", include_bytes!("../ui/maps/hirochi_raceway.jpg")),
    ("Derby Arena", "/levels/derby/info.json", "derby.jpg", include_bytes!("../ui/maps/derby.jpg")),
];

fn extract_map_path_from_zip(zip_path: &Path) -> Option<String> {
    let file = fs::File::open(zip_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if let Ok(entry_name) = entry.name() {
                if entry_name.starts_with("levels/") {
                    if entry_name.ends_with("/info.json") && entry_name.matches('/').count() == 2 {
                        return Some(format!("/{}", entry_name));
                    }
                    let parts: Vec<&str> = entry_name.split('/').collect();
                    if parts.len() >= 2 && !parts[1].is_empty() && parts[1] != "info.json" {
                        return Some(format!("/levels/{}/info.json", parts[1]));
                    }
                }
            }
        }
    }
    None
}

fn extract_zip_preview(zip_path: &Path, out_base: &Path) -> Option<PathBuf> {
    let file = fs::File::open(zip_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    let mut best_index = None;
    let mut best_priority = 0;
    let mut best_ext = "jpg";

    for i in 0..archive.len() {
        if let Ok(entry) = archive.by_index(i) {
            if let Ok(entry_name) = entry.name() {
                let name = entry_name.to_lowercase();
                let ext = if name.ends_with(".png") {
                    "png"
                } else if name.ends_with(".jpg") || name.ends_with(".jpeg") {
                    "jpg"
                } else {
                    continue;
                };

                if name.contains("preview") && best_priority < 3 {
                    best_index = Some(i);
                    best_priority = 3;
                    best_ext = ext;
                } else if name.contains("quickrace") && best_priority < 2 {
                    best_index = Some(i);
                    best_priority = 2;
                    best_ext = ext;
                } else if name.starts_with("levels/") && best_priority < 1 {
                    best_index = Some(i);
                    best_priority = 1;
                    best_ext = ext;
                }
            }
        }
    }

    if let Some(idx) = best_index {
        if let Ok(entry) = archive.by_index(idx) {
            let mut buf = Vec::new();
            let mut take = entry.take(4 * 1024 * 1024);
            if take.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
                let target = out_base.with_extension(best_ext);
                if fs::write(&target, &buf).is_ok() {
                    return Some(target);
                }
            }
        }
    }
    None
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
    server_stdin: Option<std::process::ChildStdin>,
    server_logs: Vec<String>,
    log_receiver: Option<Receiver<String>>,
    show_console: bool,
    show_server_config: bool,
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
            server_stdin: None,
            server_logs: Vec::new(),
            log_receiver: None,
            show_console: false,
            show_server_config: false,
        };
        state.scan_mods();
        state.scan_presets();

        if let Some(first) = state.presets.first() {
            let name = first.name.clone();
            state.load_preset(&name);
        }

        state
    }

    fn get_map_items(&self) -> Vec<MapItem> {
        let preview_dir = PathBuf::from("/tmp/beamforge_maps");
        let _ = fs::create_dir_all(&preview_dir);

        let mut items = Vec::new();
        let mut added_paths = HashSet::new();

        // 1. Vanilla maps
        for (title, path, filename, bytes) in VANILLA_MAPS {
            let target_file = preview_dir.join(filename);
            if !target_file.exists() {
                let _ = fs::write(&target_file, bytes);
            }
            let img = slint::Image::load_from_path(&target_file).unwrap_or_default();
            items.push(MapItem {
                title: (*title).into(),
                path: (*path).into(),
                badge: "VANILLA".into(),
                preview: img,
            });
            added_paths.insert((*path).to_string());
        }

        // 2. Mod maps
        let fallback_img_path = preview_dir.join("gridmap_v2.jpg");
        for m in &self.all_mods {
            if m.category == "Maps" {
                let mut map_path = m.map_path.clone();
                let zip_path = Path::new(&self.config.storage_path).join(&m.filename);

                if map_path.is_empty() && zip_path.exists() {
                    map_path = extract_map_path_from_zip(&zip_path).unwrap_or_default();
                }

                if map_path.is_empty() || added_paths.contains(&map_path) {
                    continue;
                }

                let safe_name = m.filename.replace(|c: char| !c.is_alphanumeric() && c != '.', "_");
                let out_base = preview_dir.join(&safe_name);

                let jpg_target = out_base.with_extension("jpg");
                let png_target = out_base.with_extension("png");

                let preview_file = if jpg_target.exists() {
                    Some(jpg_target)
                } else if png_target.exists() {
                    Some(png_target)
                } else if zip_path.exists() {
                    extract_zip_preview(&zip_path, &out_base)
                } else {
                    None
                };

                let img = if let Some(ref pf) = preview_file {
                    slint::Image::load_from_path(pf).unwrap_or_default()
                } else if fallback_img_path.exists() {
                    slint::Image::load_from_path(&fallback_img_path).unwrap_or_default()
                } else {
                    slint::Image::default()
                };

                items.push(MapItem {
                    title: m.title.clone().into(),
                    path: map_path.clone().into(),
                    badge: "MOD".into(),
                    preview: img,
                });
                added_paths.insert(map_path);
            }
        }

        items
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
                            if cached.mtime == mtime && cached.size_bytes == size_bytes && (!cached.map_path.is_empty() || cached.category != "Maps") {
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
        self.server_stdin = None;
        let _ = Command::new("pkill").arg("-f").arg("BeamMP-Server").output();
        self.server_logs.push("--- BeamMP Server stopped ---".into());
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
        ui.set_show_console(self.show_console);
        ui.set_console_text(self.server_logs.join("\n").into());
        ui.set_show_server_config(self.show_server_config);

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

    // Background timer to poll server process state and stream logs
    let timer = slint::Timer::default();
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(250),
            move || {
                let mut st = state.borrow_mut();
                let running = st.is_server_running();
                let mut logs_updated = false;
                let mut new_lines = Vec::new();
                if let Some(rx) = st.log_receiver.as_ref() {
                    while let Ok(line) = rx.try_recv() {
                        new_lines.push(line);
                    }
                }
                if !new_lines.is_empty() {
                    logs_updated = true;
                    st.server_logs.extend(new_lines);
                    if st.server_logs.len() > 1000 {
                        let excess = st.server_logs.len() - 1000;
                        st.server_logs.drain(0..excess);
                    }
                }

                if let Some(ui) = ui_weak.upgrade() {
                    if ui.get_server_running() != running {
                        ui.set_server_running(running);
                    }
                    if logs_updated && st.show_console {
                        ui.set_console_text(st.server_logs.join("\n").into());
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
                cmd.stdin(std::process::Stdio::piped());
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());

                match cmd.spawn() {
                    Ok(mut child) => {
                        let stdin = child.stdin.take();
                        let stdout = child.stdout.take();
                        let stderr = child.stderr.take();

                        let (tx, rx) = std::sync::mpsc::channel();
                        st.log_receiver = Some(rx);
                        st.server_stdin = stdin;
                        st.server_child = Some(child);

                        if let Some(out) = stdout {
                            let tx_out = tx.clone();
                            std::thread::spawn(move || {
                                let reader = BufReader::new(out);
                                for line in reader.lines().flatten() {
                                    let _ = tx_out.send(line);
                                }
                            });
                        }

                        if let Some(err) = stderr {
                            let tx_err = tx;
                            std::thread::spawn(move || {
                                let reader = BufReader::new(err);
                                for line in reader.lines().flatten() {
                                    let _ = tx_err.send(line);
                                }
                            });
                        }

                        st.server_logs.push("--- BeamMP Server started ---".into());
                        if let Some(ui) = ui_weak.upgrade() {
                            ui.set_server_running(true);
                            ui.set_console_text(st.server_logs.join("\n").into());
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

    // Toggle live console modal
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_toggle_console(move || {
            let mut st = state.borrow_mut();
            st.show_console = !st.show_console;
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_console(st.show_console);
                ui.set_console_text(st.server_logs.join("\n").into());
            }
        });
    }

    // Clear console output
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_clear_console(move || {
            let mut st = state.borrow_mut();
            st.server_logs.clear();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_console_text("".into());
            }
        });
    }

    // Send command to server stdin
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_send_console_command(move |cmd_str| {
            let mut st = state.borrow_mut();
            let cmd = cmd_str.trim().to_string();
            if cmd.is_empty() { return; }
            st.server_logs.push(format!("> {}", cmd));
            if let Some(stdin) = st.server_stdin.as_mut() {
                let _ = writeln!(stdin, "{}", cmd);
                let _ = stdin.flush();
            }
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_console_text(st.server_logs.join("\n").into());
            }
        });
    }

    // Toggle server config editor modal
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_toggle_server_config(move || {
            let mut st = state.borrow_mut();
            st.show_server_config = !st.show_server_config;
            if st.show_server_config {
                let bin = PathBuf::from(&st.config.server_bin_path);
                let dir = bin.parent().unwrap_or(Path::new("."));
                let cfg_file = dir.join("ServerConfig.toml");
                let data = load_server_config_toml(&cfg_file);
                let map_items = st.get_map_items();

                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_cfg_name(data.name.into());
                    ui.set_cfg_desc(data.description.into());
                    ui.set_cfg_map(data.map.clone().into());
                    ui.set_cfg_max_players(data.max_players.into());
                    ui.set_cfg_max_cars(data.max_cars.into());
                    ui.set_cfg_port(data.port.into());
                    ui.set_cfg_private(data.private);
                    ui.set_cfg_auth_key(data.auth_key.into());

                    if let Some(m) = map_items.iter().find(|item| item.path == data.map) {
                        ui.set_preview_map_img(m.preview.clone());
                        ui.set_preview_map_title(m.title.clone());
                        ui.set_preview_map_badge(m.badge.clone());
                        ui.set_preview_map_path(m.path.clone());
                    } else if let Some(m) = map_items.first() {
                        ui.set_preview_map_img(m.preview.clone());
                        ui.set_preview_map_title(m.title.clone());
                        ui.set_preview_map_badge(m.badge.clone());
                        ui.set_preview_map_path(m.path.clone());
                    }

                    ui.set_map_items(ModelRc::new(VecModel::from(map_items)));
                    ui.set_show_server_config(true);
                }
            } else if let Some(ui) = ui_weak.upgrade() {
                ui.set_show_server_config(false);
            }
        });
    }

    // Select map from suggestions
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_select_map(move |map| {
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_cfg_map(map.clone());
                let st = state.borrow();
                let map_items = st.get_map_items();
                if let Some(m) = map_items.iter().find(|item| item.path == map.as_str()) {
                    ui.set_preview_map_img(m.preview.clone());
                    ui.set_preview_map_title(m.title.clone());
                    ui.set_preview_map_badge(m.badge.clone());
                    ui.set_preview_map_path(m.path.clone());
                }
            }
        });
    }

    // Save server configuration
    {
        let state = state.clone();
        let ui_weak = ui.as_weak();
        ui.on_save_server_config(move |name, desc, map, max_p, max_c, port, priv_srv, auth| {
            let st = state.borrow();
            let bin = PathBuf::from(&st.config.server_bin_path);
            let dir = bin.parent().unwrap_or(Path::new("."));
            let cfg_file = dir.join("ServerConfig.toml");

            match save_server_config_toml(&cfg_file, &name, &desc, &map, &max_p, &max_c, &port, priv_srv, &auth) {
                Ok(_) => {
                    if let Some(ui) = ui_weak.upgrade() {
                        ui.set_show_server_config(false);
                    }
                    show_toast(&ui_weak, "ServerConfig.toml saved successfully!".into(), false);
                }
                Err(e) => {
                    show_toast(&ui_weak, format!("Failed to save config: {}", e), true);
                }
            }
        });
    }

    // Open ServerConfig.toml in external editor
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
