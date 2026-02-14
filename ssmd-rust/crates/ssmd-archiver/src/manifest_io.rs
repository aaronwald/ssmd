use std::collections::HashSet;
use std::path::Path;

use crate::manifest::{FileEntry, Gap, Manifest};
use crate::writer::ArchiveOutput;

/// Update manifest with completed files.
#[allow(clippy::too_many_arguments)]
pub fn update_manifest(
    base_path: &Path,
    feed: &str,
    stream_name: &str,
    date: &str,
    rotation_interval: &str,
    tickers: &HashSet<String>,
    message_types: &HashSet<String>,
    gaps: &[Gap],
    completed_files: &[FileEntry],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut manifest = Manifest::new(feed, date, rotation_interval, "jsonl");
    manifest.files = completed_files.to_vec();
    manifest.tickers = tickers.iter().cloned().collect();
    manifest.tickers.sort_unstable();
    manifest.message_types = message_types.iter().cloned().collect();
    manifest.message_types.sort_unstable();
    manifest.gaps = gaps.to_vec();
    manifest.has_gaps = !gaps.is_empty();

    let manifest_path = base_path
        .join(feed)
        .join(stream_name)
        .join(date)
        .join("manifest.json");
    if let Some(parent) = manifest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let manifest_json = serde_json::to_vec(&manifest)?;
    let tmp_manifest_path = manifest_path.with_extension("json.tmp");
    std::fs::write(&tmp_manifest_path, manifest_json)?;
    std::fs::rename(&tmp_manifest_path, &manifest_path)?;

    Ok(())
}

/// Close current writer file (if any), append resulting file entries, and write final manifest.
#[allow(clippy::too_many_arguments)]
pub fn write_manifest<W: ArchiveOutput>(
    base_path: &Path,
    feed: &str,
    stream_name: &str,
    date: &str,
    rotation_interval: &str,
    writer: &mut W,
    tickers: &HashSet<String>,
    message_types: &HashSet<String>,
    gaps: &[Gap],
    completed_files: &mut Vec<FileEntry>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    completed_files.extend(writer.close()?);

    update_manifest(
        base_path,
        feed,
        stream_name,
        date,
        rotation_interval,
        tickers,
        message_types,
        gaps,
        completed_files,
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use chrono::Utc;
    use tempfile::TempDir;

    use crate::error::ArchiverError;
    use crate::manifest::{FileEntry, Gap, Manifest};
    use crate::writer::ArchiveOutput;

    use super::{update_manifest, write_manifest};

    struct StubWriter {
        entries: Vec<FileEntry>,
    }

    impl ArchiveOutput for StubWriter {
        fn write(
            &mut self,
            _data: &[u8],
            _seq: u64,
            _now: chrono::DateTime<Utc>,
        ) -> Result<Vec<FileEntry>, ArchiverError> {
            Ok(Vec::new())
        }

        fn close(&mut self) -> Result<Vec<FileEntry>, ArchiverError> {
            Ok(std::mem::take(&mut self.entries))
        }

        fn flush(&mut self) -> Result<(), ArchiverError> {
            Ok(())
        }
    }

    #[test]
    fn test_update_manifest_writes_atomic_json() {
        let tmp = TempDir::new().unwrap();
        let date = "2026-02-14";
        let mut tickers = HashSet::new();
        tickers.insert("B".to_string());
        tickers.insert("A".to_string());
        let mut message_types = HashSet::new();
        message_types.insert("trade".to_string());
        message_types.insert("ticker".to_string());

        let files = vec![FileEntry {
            name: "1200.jsonl.gz".to_string(),
            start: Utc::now(),
            end: Utc::now(),
            records: 10,
            bytes: 100,
            raw_bytes: None,
            compression_ratio: None,
            nats_start_seq: 1,
            nats_end_seq: 10,
            records_by_type: None,
        }];

        update_manifest(
            tmp.path(),
            "kalshi",
            "politics",
            date,
            "15m",
            &tickers,
            &message_types,
            &[],
            &files,
        )
        .unwrap();

        let manifest_path = tmp
            .path()
            .join("kalshi")
            .join("politics")
            .join(date)
            .join("manifest.json");
        let content = std::fs::read_to_string(&manifest_path).unwrap();
        let manifest: Manifest = serde_json::from_str(&content).unwrap();

        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.tickers, vec!["A".to_string(), "B".to_string()]);
        assert_eq!(
            manifest.message_types,
            vec!["ticker".to_string(), "trade".to_string()]
        );
        assert!(!manifest_path.with_extension("json.tmp").exists());
    }

    #[test]
    fn test_write_manifest_closes_writer_and_appends_entries() {
        let tmp = TempDir::new().unwrap();
        let now = Utc::now();
        let mut writer = StubWriter {
            entries: vec![FileEntry {
                name: "1215.jsonl.gz".to_string(),
                start: now,
                end: now,
                records: 5,
                bytes: 50,
                raw_bytes: None,
                compression_ratio: None,
                nats_start_seq: 11,
                nats_end_seq: 15,
                records_by_type: None,
            }],
        };

        let mut completed = vec![FileEntry {
            name: "1200.jsonl.gz".to_string(),
            start: now,
            end: now,
            records: 10,
            bytes: 100,
            raw_bytes: None,
            compression_ratio: None,
            nats_start_seq: 1,
            nats_end_seq: 10,
            records_by_type: None,
        }];

        let mut tickers = HashSet::new();
        tickers.insert("KXBTC".to_string());
        let mut message_types = HashSet::new();
        message_types.insert("trade".to_string());
        let gaps = vec![Gap {
            after_seq: 9,
            missing_count: 1,
            detected_at: now,
        }];

        write_manifest(
            tmp.path(),
            "kalshi",
            "politics",
            "2026-02-14",
            "15m",
            &mut writer,
            &tickers,
            &message_types,
            &gaps,
            &mut completed,
        )
        .unwrap();

        assert_eq!(completed.len(), 2);

        let manifest_path = tmp
            .path()
            .join("kalshi")
            .join("politics")
            .join("2026-02-14")
            .join("manifest.json");
        let manifest: Manifest =
            serde_json::from_str(&std::fs::read_to_string(manifest_path).unwrap()).unwrap();
        assert_eq!(manifest.files.len(), 2);
        assert!(manifest.has_gaps);
    }
}
