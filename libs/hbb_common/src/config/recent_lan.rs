use super::*;

// One atomic file per verified device avoids lost updates between the UI,
// background host and concurrent sessions. Empty endpoints are tombstones:
// an old process writing its legacy _local cache must not resurrect deletions.
fn path(dir: &Path, fingerprint: &str) -> Result<PathBuf> {
    if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(anyhow!("Invalid device fingerprint"));
    }
    Ok(dir.join(format!("{}.toml", fingerprint.to_ascii_lowercase())))
}

pub(super) fn store(dir: &Path, recent: &RecentLanEndpoint) -> Result<()> {
    store_path(path(dir, &recent.fingerprint)?, recent)
}

pub(super) fn remove(dir: &Path, fingerprint: &str) -> Result<()> {
    store(
        dir,
        &RecentLanEndpoint {
            fingerprint: fingerprint.to_ascii_lowercase(),
            ..Default::default()
        },
    )
}

pub(super) fn load_into(dir: &Path, recent: &mut HashMap<String, RecentLanEndpoint>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
        Err(err) => {
            log::error!("Failed to read recent LAN endpoints: {err}");
            return;
        }
    };
    for entry in entries {
        let result = (|| -> Result<()> {
            let file = entry?.path();
            if file.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                return Ok(());
            }
            let value: RecentLanEndpoint = confy::load_path(&file)?;
            let fingerprint = value.fingerprint.to_ascii_lowercase();
            if path(dir, &fingerprint)? != file {
                return Err(anyhow!(
                    "Recent LAN endpoint filename does not match its fingerprint"
                ));
            }
            if value.endpoint.is_empty() {
                recent.remove(&fingerprint);
            } else {
                crate::lan::Endpoint::parse(&value.endpoint)?;
                recent.insert(fingerprint, value);
            }
            Ok(())
        })();
        if let Err(err) = result {
            log::error!("Failed to load recent LAN endpoint: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_lan_history_survives_stale_settings_writes_and_removal() {
        let dir = std::env::temp_dir().join(format!("subnetdesk-history-{}", uuid::Uuid::new_v4()));
        let history_dir = dir.join("recent_lan");
        let settings = dir.join("local.toml");
        let old_process = LocalConfig::default();
        let mut session = old_process.clone();
        for (fingerprint, endpoint) in [
            ("a".repeat(64), "192.168.1.20:21118"),
            ("b".repeat(64), "100.81.211.45:21118"),
        ] {
            let value = RecentLanEndpoint {
                endpoint: endpoint.to_owned(),
                username: "subnetrust".to_owned(),
                fingerprint,
                last_connected_at: 1,
                ..Default::default()
            };
            store(&history_dir, &value).unwrap();
            session.upsert_recent_lan_endpoint(value);
        }
        store_path(settings.clone(), &session).unwrap();
        // A background process saves the settings snapshot taken before login.
        store_path(settings.clone(), &old_process).unwrap();
        let mut fresh: LocalConfig = confy::load_path(&settings).unwrap();
        load_into(&history_dir, &mut fresh.recent_lan_endpoints);
        assert_eq!(fresh.recent_lan_endpoints.len(), 2);
        // A later stale save must not bring a deleted device back.
        remove(&history_dir, &"a".repeat(64)).unwrap();
        store_path(settings.clone(), &session).unwrap();
        let mut fresh: LocalConfig = confy::load_path(&settings).unwrap();
        load_into(&history_dir, &mut fresh.recent_lan_endpoints);
        assert_eq!(fresh.recent_lan_endpoints.len(), 1);
        assert!(fresh.recent_lan_endpoints.contains_key(&"b".repeat(64)));
        // Reconnecting the same device at a new address replaces its record.
        let changed = RecentLanEndpoint {
            endpoint: "192.168.1.99:21118".to_owned(),
            fingerprint: "a".repeat(64),
            last_connected_at: 2,
            ..Default::default()
        };
        store(&history_dir, &changed).unwrap();
        load_into(&history_dir, &mut fresh.recent_lan_endpoints);
        assert_eq!(fresh.recent_lan_endpoints.len(), 2);
        assert_eq!(
            fresh.sorted_recent_lan_endpoints()[0].endpoint,
            changed.endpoint
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
