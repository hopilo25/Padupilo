//! Pekerjaan berat (deteksi adegan, transkripsi, ekspor, unduh) berjalan di thread terpisah.

use padupilo::transcribe::Word;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub enum JobResult {
    Scenes(Vec<f64>),
    Words(Vec<Option<Vec<Word>>>),
    Exported(std::path::PathBuf),
    Downloaded,
    Failed(String),
}

#[derive(Default)]
pub struct Status {
    pub label: String,
    pub progress: f32,
    pub result: Option<JobResult>,
}

pub struct Job {
    pub title: &'static str,
    pub status: Arc<Mutex<Status>>,
    pub cancel: Arc<AtomicBool>,
}

impl Job {
    pub fn spawn<F>(title: &'static str, ctx: eframe::egui::Context, work: F) -> Job
    where
        F: FnOnce(&dyn Fn(&str, f32), &AtomicBool) -> JobResult + Send + 'static,
    {
        let status = Arc::new(Mutex::new(Status { label: title.to_string(), ..Default::default() }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (st, cn, cx) = (status.clone(), cancel.clone(), ctx.clone());
        std::thread::spawn(move || {
            let report = |label: &str, p: f32| {
                if let Ok(mut s) = st.lock() {
                    s.label = label.to_string();
                    s.progress = p;
                }
                cx.request_repaint();
            };
            let r = work(&report, &cn);
            if let Ok(mut s) = st.lock() {
                s.result = Some(r);
            }
            cx.request_repaint();
        });
        Job { title, status, cancel }
    }

    pub fn take_result(&self) -> Option<JobResult> {
        self.status.lock().ok().and_then(|mut s| s.result.take())
    }

    pub fn snapshot(&self) -> (String, f32) {
        self.status.lock().map(|s| (s.label.clone(), s.progress)).unwrap_or_default()
    }
}
