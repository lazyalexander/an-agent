use super::*;

#[test]
fn links_ignore_examples_and_resolve_repository_paths() {
    let links = targets("[a](../README.md#x)\n```sh\n[x](missing)\n```\n[b]: ./other.md\n");
    assert_eq!(links, ["../README.md#x", "./other.md"]);
    assert_eq!(
        local_target(Path::new("doc/a.md"), &links[0]).unwrap(),
        Some(PathBuf::from("README.md"))
    );
    assert!(local_target(Path::new("README.md"), "../outside.md").is_err());
    assert!(
        local_target(Path::new("doc/a.md"), "https://example.invalid/a")
            .unwrap()
            .is_none()
    );
}
