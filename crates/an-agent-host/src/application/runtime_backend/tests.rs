use super::*;
use serde_json::{Value, json};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn fixture() -> TestResult<(Manifest, BTreeMap<String, Vec<u8>>)> {
    let mut payloads = BTreeMap::new();
    let mut components = BTreeMap::new();
    let mut files = BTreeMap::new();
    let mut provenance = BTreeMap::new();
    for (index, key) in COMPONENTS.iter().enumerate() {
        let path = format!("payload/{key}");
        let bytes = key.as_bytes().to_vec();
        let hash = format!("{:x}", Sha256::digest(&bytes));
        provenance.insert(path.clone(), hash.clone());
        components.insert(key.to_string(), path.clone());
        files.insert(
            path.clone(),
            super::super::manifest::File {
                sha256: hash,
                bytes: bytes.len() as u64,
                executable: index < 5,
            },
        );
        payloads.insert(path, bytes);
    }
    let declaration = json!({"schema":"an-container-backend-v1","backend":"apple-container",
        "image":format!("local/documents@sha256:{}", "a".repeat(64)),"cpus":2,"memory_mib":1024,"storage_bytes":1073741824});
    let source = json!({"schema":"an-container-provenance-v1","repository":"https://github.com/lazyalexander/container.git",
        "commit":"a15b46097aa4c2325da13c1fb5192b9274aab083",
        "swift_lock_sha256":files[&components["container_swift_lock"]].sha256,"files":provenance});
    for (key, value) in [("backend", declaration), ("container_provenance", source)] {
        let path = format!("payload/{key}.json");
        let bytes = serde_json::to_vec(&value)?;
        files.insert(
            path.clone(),
            super::super::manifest::File {
                sha256: format!("{:x}", Sha256::digest(&bytes)),
                bytes: bytes.len() as u64,
                executable: false,
            },
        );
        components.insert(key.to_string(), path.clone());
        payloads.insert(path, bytes);
    }
    let license = components["container_swift_lock"].clone();
    Ok((
        Manifest {
            schema: "an-offline-runtime-v1".into(),
            name: "fixture".into(),
            version: "1".into(),
            platform: "macos-aarch64".into(),
            licenses: vec![license],
            components,
            files,
        },
        payloads,
    ))
}

fn verify(
    manifest: &Manifest,
    payloads: &BTreeMap<String, Vec<u8>>,
) -> Result<Option<Declaration>, String> {
    manifest.validate()?;
    validate(manifest, |path| {
        payloads
            .get(path)
            .cloned()
            .ok_or("missing test payload".into())
    })
}

fn replace(
    manifest: &mut Manifest,
    payloads: &mut BTreeMap<String, Vec<u8>>,
    key: &str,
    transform: impl FnOnce(&mut Value),
) -> TestResult {
    let path = &manifest.components[key];
    let mut value: Value = serde_json::from_slice(&payloads[path])?;
    transform(&mut value);
    let bytes = serde_json::to_vec(&value)?;
    let file = manifest.files.get_mut(path).ok_or("fixture file")?;
    file.sha256 = format!("{:x}", Sha256::digest(&bytes));
    file.bytes = bytes.len() as u64;
    payloads.insert(path.clone(), bytes);
    Ok(())
}

#[test]
fn digest_bound_offline_payload_is_data_and_legacy_remains_compatible() -> TestResult {
    let (manifest, payloads) = fixture()?;
    assert_eq!(
        verify(&manifest, &payloads)?
            .ok_or("backend declaration")?
            .backend,
        "apple-container"
    );
    let mut legacy = manifest.clone();
    legacy.components.clear();
    legacy
        .components
        .insert("domain".into(), legacy.licenses[0].clone());
    assert!(verify(&legacy, &payloads)?.is_none());
    let mut orphan = manifest.clone();
    orphan.components.remove("backend");
    assert!(verify(&orphan, &payloads).is_err());
    Ok(())
}

#[test]
fn changed_declarations_modes_sources_and_resource_requests_are_rejected() -> TestResult {
    for (field, value) in [
        ("image", json!("local/documents:latest")),
        ("backend", json!("shell")),
        ("cpus", json!(0)),
        ("memory_mib", json!(0)),
        ("storage_bytes", json!(0)),
        ("network", json!(true)),
        ("cli", json!("/arbitrary/cli")),
    ] {
        let (mut manifest, mut payloads) = fixture()?;
        replace(&mut manifest, &mut payloads, "backend", |data| {
            data[field] = value
        })?;
        assert!(verify(&manifest, &payloads).is_err(), "{field}");
    }
    for key in COMPONENTS {
        let (mut manifest, payloads) = fixture()?;
        manifest.components.remove(key);
        assert!(verify(&manifest, &payloads).is_err(), "{key}");
    }
    let (mut manifest, payloads) = fixture()?;
    manifest
        .files
        .get_mut(&manifest.components["backend"])
        .ok_or("fixture metadata")?
        .executable = true;
    assert!(verify(&manifest, &payloads).is_err());
    let (mut manifest, payloads) = fixture()?;
    manifest
        .components
        .insert("cocoon".into(), manifest.components["container"].clone());
    assert!(verify(&manifest, &payloads).is_err());
    let (mut manifest, mut payloads) = fixture()?;
    replace(
        &mut manifest,
        &mut payloads,
        "container_provenance",
        |data| data["swift_lock_sha256"] = json!("b".repeat(64)),
    )?;
    assert!(verify(&manifest, &payloads).is_err());
    let (manifest, mut payloads) = fixture()?;
    payloads
        .get_mut(&manifest.components["backend"])
        .ok_or("fixture metadata")?
        .push(b' ');
    assert!(verify(&manifest, &payloads).is_err());
    Ok(())
}
