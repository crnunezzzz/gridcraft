#![forbid(unsafe_code)]
use crate::platform::Platform;
use egui::{Context, Ui};
use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

pub struct Editor {
    platform: Platform,
    app: SheetApp,
    pictures: Rc<RefCell<HashMap<u64, Inbox>>>,
    export_path: PathBuf,
}
impl Editor {
    pub const NAME: &'static str = "GridCraft";
    /// The editor has no momentum of its own: the host coasts after a flick.
    pub const OWN_MOMENTUM: bool = false;
    pub fn new(dir: &Path, platform: Platform, _ctx: &Context) -> Self {
        let open = platform.clone();
        let save = platform.clone();
        let picture = platform.clone();
        let pictures: Rc<RefCell<HashMap<u64, Inbox>>> = Default::default();
        let picture_queue = pictures.clone();
        let serial = Rc::new(RefCell::new(100u64));
        let clipboard = platform.clone();
        let html_clipboard = platform.clone();
        let copy_clipboard = platform.clone();
        let links = platform.clone();
        let print = platform.clone();
        let print_path = dir.join("print.pdf");
        let services = Services {
            host_toolbar: Some(Default::default()),
            paste_html: Some(Box::new(move || html_clipboard.clipboard("html"))),
            copy_html: Some(Box::new(move |html, text| copy_clipboard.copy_html(html, text))),
            open_url: Some(Box::new(move |url| {
                let _ = links.exchange(json!({"op":"open_url","url":url}));
            })),
            paste_text: Some(Box::new(move || clipboard.exchange(json!({"op":"clipboard_get"})).ok()?.get("text")?.as_str().map(str::to_owned))),
            print: Some(Box::new(move |bytes| {
                if std::fs::write(&print_path, bytes).is_ok() {
                    let _ = print.exchange(json!({"op":"print","path":print_path}));
                }
            })),
            open_async: Some(Box::new(move || {
                let _ = open.exchange(json!({"op":"open","ticket":1,"purpose":"document"}));
            })),
            pick_save: Some(Box::new(move |name| {
                save.exchange(json!({"op":"local_save","name":name,"export_after_write":true})).ok()?.get("path")?.as_str().map(str::to_owned)
            })),
            pick_picture_async: Some(Box::new(move |inbox| {
                let mut id = serial.borrow_mut();
                *id = id.saturating_add(1);
                picture_queue.borrow_mut().insert(*id, inbox);
                let _ = picture.exchange(json!({"op":"open","ticket":*id,"purpose":"picture"}));
            })),
            ..Default::default()
        };
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let recovered = dir.join("session.xlsx");
        if recovered.is_file() {
            let _ = session.run("file.open", json!({"path":recovered}));
            if let Some(doc) = session.active_mut() {
                doc.path = None;
            }
        }
        let mut app = SheetApp::new(session, services);
        if let Ok(bytes) = std::fs::read(dir.join("editor.json"))
            && let Ok(prefs) = serde_json::from_slice(&bytes)
        {
            app.ui = prefs;
        }
        Self { app, pictures, platform, export_path: dir.join("export.xlsx") }
    }
    pub fn logic(&mut self, ctx: &Context) {
        self.app.logic(ctx);
    }
    pub fn raw_input(&mut self, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }
    pub fn ui(&mut self, ui: &mut Ui) {
        self.app.ui(ui);
    }
    pub fn host_state(&mut self, open: bool, busy: bool, status: &str) {
        if let Some(host) = &mut self.app.services.host_toolbar {
            host.assistant_open = open;
            host.busy = busy;
            host.status = status.into();
        }
    }
    /// The interface size as a zoom over the system's display scale (1.0: as Android draws it).
    pub fn interface_scale(&self, _native: f32) -> f32 {
        self.app.ui.zoom_factor()
    }
    pub fn set_interface_scale(&mut self, _native: f32, scale: f32) {
        self.app.ui.interface_scale = gridcraft_ui_egui::clamp_interface_scale(f64::from(scale) * 100.0).unwrap_or(100);
    }
    pub fn host_action(&mut self) -> Option<&'static str> {
        self.app.services.host_toolbar.as_mut()?.action.take()
    }
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.app.run(id, params)
    }
    pub fn agent_save(&mut self) -> Result<Value, String> {
        let path = if let Some(path) = self.app.session.active().and_then(|d| d.path.clone()) {
            path
        } else {
            self.platform.exchange(json!({"op":"local_save","name":"workbook.xlsx"}))?["path"]
                .as_str()
                .ok_or("No se pudo crear el archivo local")?
                .to_owned()
        };
        self.app.run("file.save", json!({"path":path}))?;
        if self.app.session.active().is_none_or(|doc| doc.is_dirty()) || !Path::new(&path).is_file() {
            return Err("El guardado necesita completarse en el editor".into());
        }
        Ok(json!({"saved":true,"scope":"local_copy","name":Path::new(&path).file_name().and_then(|n| n.to_str())}))
    }
    pub fn catalog(&self) -> Value {
        serde_json::to_value(self.app.session.commands()).unwrap_or(json!([]))
    }
    pub fn context(&mut self) -> Value {
        self.app.session.run("document.inspect", json!({})).unwrap_or_else(|e| json!({"error":e}))
    }
    pub fn file_event(&mut self, event: &Value) {
        let ticket = event["ticket"].as_u64().unwrap_or(0);
        if let Some(inbox) = self.pictures.borrow_mut().remove(&ticket) {
            if let Some(path) = event["path"].as_str()
                && let Ok(bytes) = std::fs::read(path)
            {
                inbox.lock().unwrap_or_else(|e| e.into_inner()).push((event["name"].as_str().unwrap_or("picture").into(), bytes));
            }
        } else if let Some(path) = event["path"].as_str() {
            self.app.open_path(path);
        }
    }
    pub fn persist(&self, dir: &Path) {
        if let Ok(bytes) = serde_json::to_vec(&self.app.ui) {
            let _ = std::fs::write(dir.join("editor.json"), bytes);
        }
    }
    fn snapshot(&mut self, path: &Path) -> Result<(), String> {
        let result = self.app.session.run("file.saveBytes", json!({"format":"xlsx"}))?;
        let bytes = gridcraft_engine::io::base64_decode(result["base64"].as_str().ok_or("No se pudo codificar el archivo")?)
            .ok_or("Datos de archivo inválidos")?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(tmp, path)).map_err(|e| e.to_string())
    }
    pub fn recover(&mut self, dir: &Path) {
        let _ = self.snapshot(&dir.join("session.xlsx"));
    }
    pub fn export(&mut self, platform: &Platform) -> Result<(), String> {
        self.snapshot(&self.export_path.clone())?;
        platform.exchange(json!({"op":"export","path":self.export_path,"name":"workbook.xlsx","ticket":9000}))?;
        Ok(())
    }
}
