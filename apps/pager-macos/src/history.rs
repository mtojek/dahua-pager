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
    storage_budget_bytes: u64,
    storage_limit_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct StorageStats {
    pub used_bytes: u64,
    pub budget_bytes: u64,
    pub free_bytes: u64,
    pub occupancy_percent: f64,
    pub prune_limit_bytes: u64,
    pub bytes_until_prune: u64,
}

impl EventHistory {
    pub fn load(
        config_path: &str,
        storage_budget_bytes: u64,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (json_path, snapshot_dir) = paths(config_path);
        Self::remove_temporary_snapshots(&snapshot_dir)?;
        let entries = if json_path.exists() {
            let content = fs::read(&json_path)?;
            serde_json::from_slice::<VecDeque<StoredEvent>>(&content)?
        } else {
            VecDeque::new()
        };
        let mut history = Self::from_parts(json_path, snapshot_dir, entries, storage_budget_bytes);
        let original_len = history.entries.len();
        while history.entries.len() > HISTORY_LIMIT {
            history.remove_oldest();
        }
        let after_count_limit = history.entries.len();
        history.make_room_for(0, None)?;
        if original_len != history.entries.len() || after_count_limit != history.entries.len() {
            history.save()?;
        }
        Ok(history)
    }

    pub fn empty(config_path: &str, storage_budget_bytes: u64) -> Self {
        let (json_path, snapshot_dir) = paths(config_path);
        Self::from_parts(
            json_path,
            snapshot_dir,
            VecDeque::new(),
            storage_budget_bytes,
        )
    }

    fn from_parts(
        json_path: PathBuf,
        snapshot_dir: PathBuf,
        entries: VecDeque<StoredEvent>,
        storage_budget_bytes: u64,
    ) -> Self {
        let next_id = entries.iter().map(|entry| entry.id).max().unwrap_or(0) + 1;
        Self {
            json_path,
            snapshot_dir,
            entries,
            next_id,
            storage_budget_bytes,
            storage_limit_bytes: storage_budget_bytes.saturating_mul(90) / 100,
        }
    }

    pub fn storage_stats(&self) -> Result<StorageStats, Box<dyn std::error::Error>> {
        let used_bytes = self.storage_usage_bytes()?;
        Ok(StorageStats {
            used_bytes,
            budget_bytes: self.storage_budget_bytes,
            free_bytes: self.storage_budget_bytes.saturating_sub(used_bytes),
            occupancy_percent: if self.storage_budget_bytes == 0 {
                100.0
            } else {
                used_bytes as f64 * 100.0 / self.storage_budget_bytes as f64
            },
            prune_limit_bytes: self.storage_limit_bytes,
            bytes_until_prune: self.storage_limit_bytes.saturating_sub(used_bytes),
        })
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

    pub fn entry_ids(&self) -> Vec<u64> {
        self.entries.iter().map(|entry| entry.id).collect()
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
        match self.make_room_for(0, None) {
            Ok(true) => {}
            Ok(false) => {
                self.entries.pop_back();
                self.save()?;
                return Err("history storage budget exhausted".into());
            }
            Err(error) => {
                self.entries.pop_back();
                let _ = self.save();
                return Err(error);
            }
        }
        self.save()?;
        Ok(id)
    }

    pub fn record_snapshot(
        &mut self,
        id: u64,
        jpeg: &[u8],
    ) -> Result<String, Box<dyn std::error::Error>> {
        fs::create_dir_all(&self.snapshot_dir)?;
        let mut timestamp = chrono::Utc::now().timestamp_millis();
        let (file_name, target, temporary) = loop {
            let file_name = format!("{timestamp}.jpg");
            let target = self.snapshot_dir.join(&file_name);
            let temporary = self.snapshot_dir.join(format!("{file_name}.tmp"));
            if !target.exists() && !temporary.exists() {
                break (file_name, target, temporary);
            }
            timestamp += 1;
        };
        let entry = self
            .entries
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or("history entry expired before snapshot completed")?;
        let previous_snapshot = entry.snapshot_file.replace(file_name.clone());
        let has_room = match self.make_room_for(jpeg.len() as u64, Some(id)) {
            Ok(has_room) => has_room,
            Err(error) => {
                if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.snapshot_file = previous_snapshot;
                }
                let _ = self.save();
                return Err(error);
            }
        };
        if !has_room {
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
                entry.snapshot_file = previous_snapshot;
            }
            self.save()?;
            return Err("history storage budget exhausted".into());
        }
        if let Err(error) = fs::write(&temporary, jpeg) {
            let _ = fs::remove_file(&temporary);
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
                entry.snapshot_file = previous_snapshot;
            }
            let _ = self.save();
            return Err(error.into());
        }
        if let Err(error) = fs::rename(&temporary, &target) {
            let _ = fs::remove_file(temporary);
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
                entry.snapshot_file = previous_snapshot;
            }
            let _ = self.save();
            return Err(error.into());
        }
        if let Err(error) = self.save() {
            let _ = fs::remove_file(&target);
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
                entry.snapshot_file = previous_snapshot;
            }
            return Err(error);
        }
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

    /// Makes room for a temporary metadata rewrite plus an optional new file.
    /// Complete records are pruned oldest-first; incomplete events are kept.
    fn make_room_for(
        &mut self,
        additional_file_bytes: u64,
        protected_id: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        loop {
            let used = self.storage_usage_bytes()?;
            let metadata_bytes = serde_json::to_vec_pretty(&self.entries)?.len() as u64;
            if used
                .saturating_add(metadata_bytes)
                .saturating_add(additional_file_bytes)
                <= self.storage_limit_bytes
            {
                return Ok(true);
            }
            if !self.prune_oldest_complete(protected_id)? {
                return Ok(false);
            }
        }
    }

    fn prune_oldest_complete(
        &mut self,
        protected_id: Option<u64>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| entry.snapshot_file.is_some() && Some(entry.id) != protected_id)
        else {
            return Ok(false);
        };
        let entry = self
            .entries
            .remove(index)
            .expect("entry index came from iteration");
        if let Some(file_name) = entry.snapshot_file {
            let path = self.snapshot_path(&file_name)?;
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(true)
    }

    fn storage_usage_bytes(&self) -> Result<u64, Box<dyn std::error::Error>> {
        let mut total = match fs::metadata(&self.json_path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => return Err(error.into()),
        };
        let files = match fs::read_dir(&self.snapshot_dir) {
            Ok(files) => files,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(total),
            Err(error) => return Err(error.into()),
        };
        for file in files {
            let file = file?;
            if file.file_type()?.is_file() {
                total = total.saturating_add(file.metadata()?.len());
            }
        }
        Ok(total)
    }

    fn remove_temporary_snapshots(snapshot_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let files = match fs::read_dir(snapshot_dir) {
            Ok(files) => files,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for file in files {
            let file = file?;
            if file.file_type()?.is_file()
                && file
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "tmp")
            {
                fs::remove_file(file.path())?;
            }
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempHistory(PathBuf);

    impl TempHistory {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("pager-history-{}-{id}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }

        fn config_path(&self) -> String {
            self.0
                .join("config.local.json")
                .to_string_lossy()
                .into_owned()
        }
    }

    impl Drop for TempHistory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn storage_budget_prunes_oldest_complete_record_and_snapshot_first() {
        let temp = TempHistory::new();
        let mut history = EventHistory::empty(&temp.config_path(), 1024 * 1024);
        let first = history
            .record("t1".into(), "Backyard 1".into(), "Human".into())
            .unwrap();
        let first_file = history
            .record_snapshot(first, &vec![1; 400 * 1024])
            .unwrap();
        let second = history
            .record("t2".into(), "Backyard 2".into(), "Human".into())
            .unwrap();
        history
            .record_snapshot(second, &vec![2; 400 * 1024])
            .unwrap();
        let third = history
            .record("t3".into(), "Garage".into(), "Human".into())
            .unwrap();

        history
            .record_snapshot(third, &vec![3; 250 * 1024])
            .unwrap();

        assert_eq!(history.entry_ids(), vec![second, third]);
        assert!(!history.snapshot_path(&first_file).unwrap().exists());
        let stats = history.storage_stats().unwrap();
        assert!(stats.used_bytes <= stats.prune_limit_bytes);
    }

    #[test]
    fn snapshot_is_rejected_when_incomplete_records_cannot_be_pruned_for_space() {
        let temp = TempHistory::new();
        let mut history = EventHistory::empty(&temp.config_path(), 1024 * 1024);
        let event = history
            .record("t1".into(), "Pool".into(), "Human".into())
            .unwrap();

        let result = history.record_snapshot(event, &vec![7; 950 * 1024]);

        assert!(result.is_err());
        assert_eq!(history.entry_ids(), vec![event]);
        assert_eq!(history.entries[0].snapshot_file, None);
        assert_eq!(fs::read_dir(&history.snapshot_dir).unwrap().count(), 0);
    }
}
