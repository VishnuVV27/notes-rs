use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, time::SystemTime};

const TRASH: &str = "Recently Deleted";
const ALL: &str = "All Notes";
const DEFAULT: &str = "Notes";
const PURGE_SECS: u64 = 30 * 86400;

#[derive(Serialize, Deserialize, Clone)]
struct Note {
    id: u64,
    folder: String,
    body: String,
    pinned: bool,
    #[serde(default)]
    deleted: bool,
    #[serde(default)]
    deleted_at: u64,
    #[serde(default)]
    created: u64,
    modified: u64,
}

impl Note {
    // Apple Notes: the first line is the title, the rest is the row preview.
    fn title(&self) -> &str {
        match self.body.lines().next().map(str::trim) {
            Some(t) if !t.is_empty() => t,
            _ => "New Note",
        }
    }

    fn preview(&self) -> &str {
        self.body.lines().skip(1).map(str::trim).find(|l| !l.is_empty()).unwrap_or("No additional text")
    }

    fn tags(&self) -> Vec<String> {
        self.body
            .split_whitespace()
            .filter(|w| w.len() > 1 && w.starts_with('#'))
            .map(str::to_lowercase)
            .collect()
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn ago(t: u64) -> String {
    let s = now().saturating_sub(t);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", s / 60),
        3600..=86399 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86400),
    }
}

#[derive(PartialEq, Clone, Copy, Serialize, Deserialize)]
enum Sort {
    Modified,
    Created,
    Title,
}

#[derive(Serialize, Deserialize, Default)]
struct Store {
    notes: Vec<Note>,
    #[serde(default)]
    folders: Vec<String>,
    // Notes remembers the sort type per folder.
    #[serde(default)]
    sorts: HashMap<String, Sort>,
    // The "moved to Recently Deleted" alert is only shown once.
    #[serde(default)]
    warned: bool,
}

// Accepts the old format (a bare array of notes) as well as the current one.
fn parse(s: &str) -> Store {
    serde_json::from_str(s)
        .or_else(|_| serde_json::from_str::<Vec<Note>>(s).map(|notes| Store { notes, ..Default::default() }))
        .unwrap_or_default()
}

// Recently Deleted keeps notes for 30 days, then they are purged.
fn purge(notes: &mut Vec<Note>, now: u64) {
    notes.retain(|n| !n.deleted || now.saturating_sub(n.deleted_at) < PURGE_SECS);
}

struct App {
    notes: Vec<Note>,
    folders: Vec<String>,
    sorts: HashMap<String, Sort>,
    warned: bool,
    notice: bool,
    gallery: bool,
    folder: String,
    selected: Option<u64>,
    search: String,
    new_folder: String,
    path: PathBuf,
}

impl App {
    fn load() -> Self {
        let dir = dirs::data_dir().unwrap_or_default().join("notes-app");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("notes.json");
        let mut st = std::fs::read_to_string(&path).map(|s| parse(&s)).unwrap_or_default();
        purge(&mut st.notes, now());
        let mut folders = vec![DEFAULT.to_string()];
        for f in st.folders.iter().chain(st.notes.iter().filter(|n| !n.deleted).map(|n| &n.folder)) {
            if !folders.contains(f) {
                folders.push(f.clone());
            }
        }
        Self { notes: st.notes, folders, sorts: st.sorts, warned: st.warned, notice: false, gallery: false, folder: ALL.into(), selected: None, search: String::new(), new_folder: String::new(), path }
    }

    fn persist(&self) {
        // Write to a temp file then rename so a crash can't truncate the notes.
        let tmp = self.path.with_extension("tmp");
        let st = serde_json::json!({ "notes": self.notes, "folders": self.folders, "sorts": self.sorts, "warned": self.warned });
        if std::fs::write(&tmp, st.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &self.path);
        }
    }

    fn sort(&self) -> Sort {
        self.sorts.get(&self.folder).copied().unwrap_or(Sort::Modified)
    }

    fn new_note(&mut self) {
        if self.folder == TRASH {
            return;
        }
        let id = self.notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        let t = now();
        let folder = if self.folder == ALL { DEFAULT.into() } else { self.folder.clone() };
        self.notes.push(Note { id, folder, body: String::new(), pinned: false, deleted: false, deleted_at: 0, created: t, modified: t });
        self.selected = Some(id);
        self.gallery = false;
    }

    // Deleting a folder moves its notes to Recently Deleted (ICDeleteDecision.decisionObjectsForFolder).
    fn delete_folder(&mut self, f: &str) {
        let t = now();
        for n in self.notes.iter_mut().filter(|n| n.folder == f && !n.deleted) {
            n.deleted = true;
            n.deleted_at = t;
            n.pinned = false;
        }
        self.folders.retain(|x| x != f);
        self.sorts.remove(f);
        self.folder = ALL.into();
        self.selected = None;
    }

    fn visible(&self) -> Vec<usize> {
        let q = self.search.to_lowercase();
        let in_trash = self.folder == TRASH;
        let mut v: Vec<usize> = (0..self.notes.len())
            .filter(|&i| {
                let n = &self.notes[i];
                n.deleted == in_trash
                    && (in_trash || self.folder == ALL || n.folder == self.folder)
                    && (q.is_empty() || n.body.to_lowercase().contains(&q))
            })
            .collect();
        v.sort_by(|&a, &b| {
            let (a, b) = (&self.notes[a], &self.notes[b]);
            b.pinned.cmp(&a.pinned).then_with(|| match self.sort() {
                Sort::Modified => b.modified.cmp(&a.modified),
                Sort::Created => b.created.cmp(&a.created),
                Sort::Title => a.title().to_lowercase().cmp(&b.title().to_lowercase()),
            })
        });
        v
    }

    fn all_tags(&self) -> Vec<String> {
        let mut t: Vec<String> = self.notes.iter().filter(|n| !n.deleted).flat_map(Note::tags).collect();
        t.sort();
        t.dedup();
        t
    }
}

const YELLOW: egui::Color32 = egui::Color32::from_rgb(255, 224, 102);
const SIDEBAR: egui::Color32 = egui::Color32::from_rgb(242, 242, 247);

fn row(ui: &mut egui::Ui, selected: bool, title: &str, sub: &str, height: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 8.0, YELLOW);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, egui::Color32::from_black_alpha(10));
    }
    let p = ui.painter_at(rect.shrink2(egui::vec2(4.0, 0.0)));
    let x = rect.left() + 10.0;
    if sub.is_empty() {
        p.text(egui::pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(15.0), egui::Color32::BLACK);
    } else {
        p.text(egui::pos2(x, rect.top() + 6.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(15.0), egui::Color32::BLACK);
        p.text(egui::pos2(x, rect.top() + 28.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(12.5), egui::Color32::GRAY);
    }
    resp
}

fn card(ui: &mut egui::Ui, n: &Note, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(210.0, 160.0), egui::Sense::click());
    let stroke = if selected { egui::Stroke::new(2.5, YELLOW) } else { egui::Stroke::new(1.0, egui::Color32::from_gray(220)) };
    ui.painter().rect(rect, 10.0, egui::Color32::WHITE, stroke, egui::StrokeKind::Inside);
    let p = ui.painter_at(rect.shrink(2.0));
    let x = rect.left() + 12.0;
    p.text(egui::pos2(x, rect.top() + 10.0), egui::Align2::LEFT_TOP, n.title(), egui::FontId::proportional(15.0), egui::Color32::BLACK);
    let body: Vec<&str> = n.body.lines().skip(1).filter(|l| !l.trim().is_empty()).take(5).collect();
    p.text(egui::pos2(x, rect.top() + 34.0), egui::Align2::LEFT_TOP, body.join("\n"), egui::FontId::proportional(12.5), egui::Color32::GRAY);
    p.text(egui::pos2(x, rect.bottom() - 10.0), egui::Align2::LEFT_BOTTOM, ago(n.modified), egui::FontId::proportional(11.5), egui::Color32::GRAY);
    resp
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).strong().size(12.5).color(egui::Color32::GRAY));
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut dirty = false;

        egui::Panel::left("folders")
            .exact_size(190.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(SIDEBAR).inner_margin(10.0))
            .show(ui, |ui| {
                let mut doomed = None;
                for f in [ALL.to_string()].into_iter().chain(self.folders.clone()).chain([TRASH.to_string()]) {
                    let icon = if f == TRASH { "🗑" } else if f == ALL { "🗂" } else { "📁" };
                    let count = self.notes.iter().filter(|n| (f == TRASH) == n.deleted && (f == TRASH || f == ALL || n.folder == f)).count();
                    let resp = row(ui, self.folder == f, &format!("{icon}  {f}   {count}"), "", 32.0);
                    // Notes and Recently Deleted can't be renamed or removed.
                    if f != ALL && f != TRASH && f != DEFAULT {
                        resp.context_menu(|ui| {
                            if ui.button("Delete Folder").clicked() {
                                doomed = Some(f.clone());
                                ui.close();
                            }
                        });
                    }
                    if resp.clicked() {
                        self.folder = f;
                        self.selected = None;
                    }
                }
                if let Some(f) = doomed {
                    self.delete_folder(&f);
                    dirty = true;
                }
                ui.add_space(8.0);
                let r = ui.add(egui::TextEdit::singleline(&mut self.new_folder).hint_text("＋  New Folder").frame(egui::Frame::new()));
                let name = self.new_folder.trim().to_string();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !name.is_empty() && name != TRASH && name != ALL {
                    if !self.folders.contains(&name) {
                        self.folders.push(name.clone());
                    }
                    self.folder = name;
                    self.new_folder.clear();
                    dirty = true;
                }
                let tags = self.all_tags();
                if !tags.is_empty() {
                    section(ui, "Tags");
                    for t in tags {
                        if row(ui, self.search == t, &t, "", 28.0).clicked() {
                            self.search = t;
                        }
                    }
                }
            });

        if !self.gallery {
        egui::Panel::left("list")
            .exact_size(280.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(egui::Color32::WHITE).inner_margin(10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(&self.folder);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✏").on_hover_text("New Note").clicked() {
                            self.new_note();
                            dirty = true;
                        }
                        if ui.button("▦").on_hover_text("View as Gallery").clicked() {
                            self.gallery = true;
                        }
                        let mut sort = self.sort();
                        egui::ComboBox::from_id_salt("sort")
                            .selected_text("↕")
                            .width(10.0)
                            .show_ui(ui, |ui| {
                                ui.label("Sort By");
                                ui.selectable_value(&mut sort, Sort::Modified, "Date Edited");
                                ui.selectable_value(&mut sort, Sort::Created, "Date Created");
                                ui.selectable_value(&mut sort, Sort::Title, "Title");
                            });
                        if sort != self.sort() {
                            self.sorts.insert(self.folder.clone(), sort);
                            dirty = true;
                        }
                    });
                });
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("🔍  Search").desired_width(f32::INFINITY));
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut last = None;
                    for i in self.visible() {
                        let n = &self.notes[i];
                        if self.folder != TRASH && last != Some(n.pinned) {
                            section(ui, if n.pinned { "📌 Pinned" } else { DEFAULT });
                        }
                        last = Some(n.pinned);
                        let mut sub = format!("{}   {}", ago(n.modified), n.preview());
                        let all = self.folder == ALL;
                        if all {
                            sub += &format!("\n📁 {}", n.folder);
                        }
                        if row(ui, self.selected == Some(n.id), n.title(), &sub, if all { 68.0 } else { 52.0 }).clicked() {
                            self.selected = Some(n.id);
                        }
                    }
                });
            });
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::WHITE).inner_margin(24.0)).show(ui, |ui| {
            if self.gallery {
                ui.horizontal(|ui| {
                    ui.heading(&self.folder);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("✏").on_hover_text("New Note").clicked() {
                            self.new_note();
                            dirty = true;
                        }
                        if ui.button("☰").on_hover_text("View as List").clicked() {
                            self.gallery = false;
                        }
                        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("🔍  Search"));
                    });
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for i in self.visible() {
                            if card(ui, &self.notes[i], self.selected == Some(self.notes[i].id)).clicked() {
                                self.selected = Some(self.notes[i].id);
                                self.gallery = false;
                            }
                        }
                    });
                });
                return;
            }
            let Some(i) = self.notes.iter().position(|n| Some(n.id) == self.selected) else {
                ui.centered_and_justified(|ui| ui.label(egui::RichText::new("No Note Selected").size(18.0).color(egui::Color32::GRAY)));
                return;
            };
            let deleted = self.notes[i].deleted;
            ui.horizontal(|ui| {
                if deleted {
                    if ui.button("Recover").clicked() {
                        self.notes[i].deleted = false;
                        if !self.folders.contains(&self.notes[i].folder) {
                            self.notes[i].folder = DEFAULT.into();
                        }
                        self.selected = None;
                        dirty = true;
                    } else if ui.button("Delete Forever").clicked() {
                        self.notes.remove(i);
                        self.selected = None;
                        dirty = true;
                    }
                } else {
                    if ui.button(if self.notes[i].pinned { "Unpin Note" } else { "📌 Pin Note" }).clicked() {
                        self.notes[i].pinned ^= true;
                        dirty = true;
                    }
                    if ui.button("Duplicate Note").clicked() {
                        let mut copy = self.notes[i].clone();
                        copy.id = self.notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
                        copy.pinned = false;
                        self.selected = Some(copy.id);
                        self.notes.push(copy);
                        dirty = true;
                    }
                    let cur = self.notes[i].folder.clone();
                    egui::ComboBox::from_id_salt("move").selected_text("Move to").show_ui(ui, |ui| {
                        for f in self.folders.clone() {
                            if ui.selectable_label(f == cur, &f).clicked() {
                                self.notes[i].folder = f;
                                dirty = true;
                            }
                        }
                    });
                    if ui.button("☑ Checklist").clicked() {
                        let b = &mut self.notes[i].body;
                        if !b.is_empty() && !b.ends_with('\n') {
                            b.push('\n');
                        }
                        b.push_str("☐ ");
                        dirty = true;
                    }
                    if ui.button("🗑 Delete").clicked() {
                        self.notes[i].deleted = true;
                        self.notes[i].deleted_at = now();
                        self.notes[i].pinned = false;
                        self.selected = None;
                        self.notice = !self.warned;
                        self.warned = true;
                        dirty = true;
                    }
                }
            });
            let Some(n) = self.notes.get_mut(i) else { return };
            let status = if n.deleted { "You can recover or remove from Recently Deleted within 30 days.".to_string() } else { format!("Edited: {}", ago(n.modified)) };
            ui.vertical_centered(|ui| ui.label(egui::RichText::new(status).size(12.0).color(egui::Color32::GRAY)));
            ui.add_space(8.0);
            // Notes text styles: first line is the Title; "# " Heading, "## " Subheading, rest Body.
            let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                let mut job = egui::text::LayoutJob::default();
                for (k, line) in text.as_str().split_inclusive('\n').enumerate() {
                    let size = if k == 0 { 28.0 } else if line.starts_with("## ") { 18.0 } else if line.starts_with("# ") { 22.0 } else { 16.0 };
                    job.append(line, 0.0, egui::TextFormat::simple(egui::FontId::proportional(size), egui::Color32::BLACK));
                }
                job.wrap.max_width = wrap;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let r = ui.add_enabled(
                !n.deleted,
                egui::TextEdit::multiline(&mut n.body)
                    .frame(egui::Frame::new())
                    .desired_width(f32::INFINITY)
                    .desired_rows(30)
                    .layouter(&mut layouter),
            );
            if r.changed() {
                n.modified = now();
                dirty = true;
            }
        });

        if self.notice {
            egui::Window::new("Notes").collapsible(false).resizable(false).show(ui.ctx(), |ui| {
                ui.label("Deleted notes are moved to the Recently Deleted folder.");
                ui.label("You can recover or remove them within 30 days.");
                if ui.button("OK").clicked() {
                    self.notice = false;
                }
            });
        }

        if dirty {
            self.persist();
        }
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "Notes",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1000.0, 650.0]),
            ..Default::default()
        },
        Box::new(|cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::light());
            Ok(Box::new(App::load()))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(body: &str) -> Note {
        Note { id: 1, folder: "Notes".into(), body: body.into(), pinned: false, deleted: false, deleted_at: 0, created: 0, modified: 0 }
    }

    #[test]
    fn title_preview_tags() {
        let n = note("Groceries\n\nmilk #Food and #home");
        assert_eq!(n.title(), "Groceries");
        assert_eq!(n.preview(), "milk #Food and #home");
        assert_eq!(n.tags(), vec!["#food", "#home"]);
        assert_eq!(note("").title(), "New Note");
        assert_eq!(note("Only title").preview(), "No additional text");
    }

    #[test]
    fn parses_old_and_new_formats() {
        let old = r#"[{"id":1,"folder":"Notes","body":"hi","pinned":false,"modified":5}]"#;
        assert_eq!(parse(old).notes.len(), 1);
        let new = r#"{"notes":[],"folders":["Work"],"sorts":{"Work":"Title"},"warned":true}"#;
        let st = parse(new);
        assert!(st.warned && st.folders == ["Work"] && st.sorts["Work"] == Sort::Title);
        assert!(parse("garbage").notes.is_empty());
    }

    #[test]
    fn purges_only_old_deleted_notes() {
        let mut keep = note("live");
        keep.id = 2;
        let mut fresh = note("fresh");
        fresh.deleted = true;
        fresh.deleted_at = 10_000_000;
        let mut old = note("old");
        old.deleted = true;
        old.deleted_at = 10_000_000 - PURGE_SECS;
        let mut v = vec![keep, fresh, old];
        purge(&mut v, 10_000_000);
        assert_eq!(v.len(), 2);
    }
}
