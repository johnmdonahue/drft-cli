mod common;
use common::drft_bin;
use std::fs;
use tempfile::TempDir;

#[test]
fn custom_rule_integration() {
    let dir = TempDir::new().unwrap();

    let scripts = dir.path().join("scripts");
    fs::create_dir(&scripts).unwrap();
    fs::write(
        scripts.join("count-nodes.sh"),
        "#!/bin/sh\necho '{\"message\": \"custom check ran\", \"node\": \"test\"}'\n",
    )
    .unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            scripts.join("count-nodes.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }

    fs::write(
        dir.path().join("drft.toml"),
        "[rules.count-nodes]\ncommand = \"./scripts/count-nodes.sh\"\nseverity = \"warn\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("index.md"), "# Hello").unwrap();

    let output = drft_bin()
        .args(["-C", dir.path().to_str().unwrap(), "check"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("count-nodes"),
        "custom rule should appear in output, got: {stdout}"
    );
    assert!(stdout.contains("custom check ran"));
    assert!(output.status.success());
}
