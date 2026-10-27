//! Golden-file round trips for `PromptPlan`, through JSON and TrongoScript.
//!
//! Set `COSMOS_BLESS=1` to regenerate the `.trongo` file from the JSON one.

use cosmos_llm_context::{CacheHint, PromptPlan, PromptSection, SectionKind};

fn expected() -> PromptPlan {
    PromptPlan::new()
        .with_section(PromptSection::text(
            SectionKind::Base,
            "You are a coding agent.",
        ))
        .with_section(PromptSection::text(
            SectionKind::ToolGuidance,
            "Prefer read before edit.",
        ))
        .with_section(
            PromptSection::text(
                SectionKind::Instructions,
                "Instructions from /repo/AGENTS.md:\n\nRun cargo test.",
            )
            .with_source("/repo/AGENTS.md")
            .with_cache(CacheHint::Ephemeral),
        )
        .with_section(PromptSection::text(
            SectionKind::Environment,
            "<environment>\nWorking directory: /repo\n</environment>",
        ))
}

#[test]
fn prompt_plan_json_golden() {
    let ir: PromptPlan = serde_json::from_str(include_str!("golden/prompt_plan.json")).unwrap();
    assert_eq!(ir, expected());
    let back: PromptPlan = serde_json::from_str(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(back, ir);
    assert_eq!(ir.cache_breakpoints(), vec![2]);
}

#[cfg(feature = "trongo")]
#[test]
fn prompt_plan_trongo_golden() {
    if std::env::var_os("COSMOS_BLESS").is_some() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/golden/prompt_plan.trongo"
        );
        std::fs::write(path, expected().to_trongo().unwrap() + "\n").unwrap();
    }
    let ir = PromptPlan::from_trongo(include_str!("golden/prompt_plan.trongo")).unwrap();
    assert_eq!(ir, expected());
    assert_eq!(
        PromptPlan::from_trongo(&ir.to_trongo().unwrap()).unwrap(),
        ir
    );
    let j: PromptPlan = serde_json::from_str(include_str!("golden/prompt_plan.json")).unwrap();
    assert_eq!(j, ir);
}
