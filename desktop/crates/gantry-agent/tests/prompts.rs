//! Pins the assembled system prompt per mode (docs/plan/10 §6). Set `GANTRY_UPDATE_FIXTURES=1`
//! to rewrite the fixtures after an intentional prompt change, and bump `CORE_VERSION`.

use std::path::PathBuf;

use gantry_agent::{PromptContext, SystemPromptBuilder};
use gantry_core::Mode;

#[test]
fn every_mode_matches_its_fixture() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/prompts");
    let update = std::env::var_os("GANTRY_UPDATE_FIXTURES").is_some();
    for mode in Mode::ALL {
        let prompt = SystemPromptBuilder::new(
            mode,
            PromptContext {
                platform: "linux".into(),
                app_version: "0.1.0".into(),
                workspace_roots: Vec::new(),
                project_name: None,
            },
        )
        .build();
        let path = dir.join(format!("{}.txt", mode.as_str()));
        if update {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(&path, &prompt).unwrap();
            continue;
        }
        let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{}: {e} (run with GANTRY_UPDATE_FIXTURES=1)",
                path.display()
            )
        });
        assert_eq!(
            prompt,
            expected,
            "prompt for {} drifted from its fixture",
            mode.as_str()
        );
    }
}
