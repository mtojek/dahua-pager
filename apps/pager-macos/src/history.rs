use pager_ui::HistoryEntry;
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};

const HISTORY_LIMIT: usize = 1000;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredEvent {
    pub id: u64,
    pub timestamp: String,
    pub camera_name: String,
    pub description: String,
    pub snapshot_file: Option<String>,
}

pub struct EventHistory {
    json_path: PathBuf,
    snapshot_dir: PathBuf,
    entries: VecDeque<StoredEvent>,
    next_id: u64,
}

impl EventHistory {
    pub fn load(config_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let (json_path, snapshot_dir) = paths(config_path);
        let entries = if json_path.exists() {
            let content = fs::read(&json_path)?;
            serde_json::from_slice::<VecDeque<StoredEvent>>(&content)?
        } else {
            VecDeque::new()
        };
        let mut history = Self::from_parts(json_path, snapshot_dir, entries);
        while history.entries.len() > HISTORY_LIMIT {
            history.remove_oldest();
        }
        Ok(history)
    }

    pub fn empty(config_path: &str) -> Self {
        let (json_path, snapshot_dir) = paths(config_path);
        Self::from_parts(json_path, snapshot_dir, VecDeque::new())
    }

    fn from_parts(
        json_path: PathBuf,
        snapshot_dir: PathBuf,
        entries: VecDeque<StoredEvent>,
    ) -> Self {
        let next_id = entries.iter().map(|entry| entry.id).max().unwrap_or(0) + 1;
        Self {
            json_path,
            snapshot_dir,
            entries,
            next_id,
        }
    }

    pub fn ui_entries(&self) -> Vec<HistoryEntry> {
        self.entries
            .iter()
            .map(|entry| HistoryEntry {
                id: entry.id,
                camera_name: entry.camera_name.clone(),
                timestamp: entry.timestamp.clone(),
                description: entry.description.clone(),
                snapshot_file: entry.snapshot_file.clone(),
            })
            .collect()
    }

    pub fn record(
        &mut self,
        timestamp: String,
        camera_name: String,
        description: String,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push_back(StoredEvent {
            id,
            timestamp,
            camera_name,
            description,
            snapshot_file: None,
        });
        while self.entries.len() > HISTORY_LIMIT {
            self.remove_oldest();
        }
        self.save()?;
        Ok(id)
    }

    pub fn record_snapshot(
        &mut self,
        id: u64,
        jpeg: &[u8],
    ) -> Result<String, Box<dyn std::error::Error>> {
        let file_name = format!("event-{id}.jpg");
        fs::create_dir_all(&self.snapshot_dir)?;
        let target = self.snapshot_dir.join(&file_name);
        let temporary = self.snapshot_dir.join(format!("event-{id}.jpg.tmp"));
        fs::write(&temporary, jpeg)?;
        fs::rename(temporary, target)?;
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or("history entry expired before snapshot completed")?;
        entry.snapshot_file = Some(file_name.clone());
        self.save()?;
        Ok(file_name)
    }

    pub fn snapshot_path(&self, file_name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if Path::new(file_name)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(file_name)
        {
            return Err("invalid history snapshot filename".into());
        }
        Ok(self.snapshot_dir.join(file_name))
    }

    fn remove_oldest(&mut self) {
        if let Some(entry) = self.entries.pop_front()
            && let Some(file_name) = entry.snapshot_file
        {
            let _ = fs::remove_file(self.snapshot_dir.join(file_name));
        }
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = self
            .json_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.json_path.with_extension("history.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&self.entries)?)?;
        fs::rename(temporary, &self.json_path)?;
        Ok(())
    }
}

fn paths(config_path: &str) -> (PathBuf, PathBuf) {
    let config_path = Path::new(config_path);
    (
        config_path.with_extension("history.json"),
        config_path.with_extension("history"),
    )
}
