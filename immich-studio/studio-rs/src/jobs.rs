//! Background jobs (long work like "label all photos") with progress the page can poll.

use std::{collections::HashMap, future::Future, sync::{Arc, Mutex}};

use serde_json::{json, Value};

#[derive(Clone, Default)]
pub struct Jobs(Arc<Mutex<HashMap<String, Job>>>);

#[derive(Clone)]
pub struct Job {
    pub kind: String,
    pub status: &'static str,
    pub progress: u32,
    pub result: Value,
    pub error: Option<String>,
    pub started: String,
    /// a finished file to download: (bytes, media type, file name)
    pub file: Option<(bytes::Bytes, String, String)>,
}

/// Handed to the work so it can report progress and attach a result file.
#[derive(Clone)]
pub struct Progress {
    jobs: Jobs,
    id: String,
}

impl Progress {
    pub fn set(&self, percent: u32) {
        if let Some(j) = self.jobs.0.lock().unwrap().get_mut(&self.id) {
            j.progress = percent.min(100);
        }
    }

    pub fn file(&self, data: bytes::Bytes, media: &str, name: &str) {
        if let Some(j) = self.jobs.0.lock().unwrap().get_mut(&self.id) {
            j.file = Some((data, media.into(), name.into()));
        }
    }
}

impl Jobs {
    pub fn start<F, Fut>(&self, kind: &str, work: F) -> String
    where
        F: FnOnce(Progress) -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<Value>> + Send + 'static,
    {
        let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
        self.0.lock().unwrap().insert(id.clone(), Job {
            kind: kind.into(),
            status: "running",
            progress: 0,
            result: Value::Null,
            error: None,
            started: chrono::Utc::now().to_rfc3339(),
            file: None,
        });
        let progress = Progress { jobs: self.clone(), id: id.clone() };
        let jobs = self.clone();
        let job_id = id.clone();
        tokio::spawn(async move {
            let outcome = work(progress).await;
            if let Some(j) = jobs.0.lock().unwrap().get_mut(&job_id) {
                match outcome {
                    Ok(v) => { j.status = "done"; j.progress = 100; j.result = v; }
                    Err(e) => { j.status = "error"; j.error = Some(format!("{e:#}")); }
                }
            }
        });
        id
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.0.lock().unwrap().get(id).cloned()
    }

    pub fn describe(&self, id: &str) -> Option<Value> {
        let j = self.get(id)?;
        let mut v = json!({ "id": id, "kind": j.kind, "status": j.status, "progress": j.progress,
                            "result": j.result, "started": j.started });
        if let Some(e) = j.error { v["error"] = e.into(); }
        if j.file.is_some() { v["file_url"] = format!("api/jobs/{id}/file").into(); }
        Some(v)
    }
}
