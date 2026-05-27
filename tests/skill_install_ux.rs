use std::cell::RefCell;

use codex_image::skill_install_ux::{
    all_selectable_targets, expand_selected_targets, interactive_target_options,
    select_interactive_targets, InstallTargetSelector, InteractiveInstallState,
    InteractiveSelectionError, InteractiveTargetOption, SkillInstallTarget, TargetSelectionState,
};
use codex_image::skills::{SkillScope, SupportedTool};

struct FakeSelector {
    result: Result<Vec<SkillInstallTarget>, InteractiveSelectionError>,
    calls: RefCell<usize>,
}

impl FakeSelector {
    fn from_result(result: Result<Vec<SkillInstallTarget>, InteractiveSelectionError>) -> Self {
        Self {
            result,
            calls: RefCell::new(0),
        }
    }

    fn call_count(&self) -> usize {
        *self.calls.borrow()
    }
}

impl InstallTargetSelector for FakeSelector {
    fn select(
        &self,
        _options: &[InteractiveTargetOption],
    ) -> Result<Vec<SkillInstallTarget>, InteractiveSelectionError> {
        *self.calls.borrow_mut() += 1;
        self.result.clone()
    }
}

#[test]
fn skill_install_ux_all_selectable_targets_cover_full_matrix_in_canonical_order() {
    let actual = all_selectable_targets();

    let mut expected = Vec::new();
    for tool in SupportedTool::all() {
        for scope in SkillScope::all() {
            expected.push(SkillInstallTarget::new(tool, scope));
        }
    }

    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 10);
}

#[test]
fn skill_install_ux_expansion_dedupes_repeated_tool_and_scope_flags() {
    let selection = expand_selected_targets(
        &[
            SupportedTool::Pi,
            SupportedTool::Pi,
            SupportedTool::Codex,
            SupportedTool::Pi,
        ],
        &[
            SkillScope::ProjectLocal,
            SkillScope::ProjectLocal,
            SkillScope::Global,
        ],
    );

    assert_eq!(selection.state, TargetSelectionState::Complete);
    assert_eq!(
        selection.selected_tools,
        vec![SupportedTool::Codex, SupportedTool::Pi]
    );
    assert_eq!(
        selection.selected_scopes,
        vec![SkillScope::Global, SkillScope::ProjectLocal]
    );

    assert_eq!(
        selection.targets,
        vec![
            SkillInstallTarget::new(SupportedTool::Codex, SkillScope::Global),
            SkillInstallTarget::new(SupportedTool::Codex, SkillScope::ProjectLocal),
            SkillInstallTarget::new(SupportedTool::Pi, SkillScope::Global),
            SkillInstallTarget::new(SupportedTool::Pi, SkillScope::ProjectLocal),
        ]
    );
}

#[test]
fn skill_install_ux_expansion_builds_cartesian_targets_for_selected_dimensions() {
    let selection = expand_selected_targets(
        &[SupportedTool::OpenCode, SupportedTool::Claude],
        &[SkillScope::ProjectLocal, SkillScope::Global],
    );

    assert_eq!(selection.state, TargetSelectionState::Complete);
    assert_eq!(
        selection.targets,
        vec![
            SkillInstallTarget::new(SupportedTool::Claude, SkillScope::Global),
            SkillInstallTarget::new(SupportedTool::Claude, SkillScope::ProjectLocal),
            SkillInstallTarget::new(SupportedTool::OpenCode, SkillScope::Global),
            SkillInstallTarget::new(SupportedTool::OpenCode, SkillScope::ProjectLocal),
        ]
    );
}

#[test]
fn skill_install_ux_expansion_reports_no_target_and_partial_target_metadata() {
    let none_selected = expand_selected_targets(&[], &[]);
    assert_eq!(none_selected.state, TargetSelectionState::NoTargets);
    assert!(none_selected.missing_tools);
    assert!(none_selected.missing_scopes);
    assert!(none_selected.targets.is_empty());

    let missing_tools = expand_selected_targets(&[], &[SkillScope::Global]);
    assert_eq!(missing_tools.state, TargetSelectionState::PartialTargets);
    assert!(missing_tools.missing_tools);
    assert!(!missing_tools.missing_scopes);
    assert!(missing_tools.targets.is_empty());

    let missing_scopes = expand_selected_targets(&[SupportedTool::Claude], &[]);
    assert_eq!(missing_scopes.state, TargetSelectionState::PartialTargets);
    assert!(!missing_scopes.missing_tools);
    assert!(missing_scopes.missing_scopes);
    assert!(missing_scopes.targets.is_empty());
}

#[test]
fn skill_install_ux_interactive_options_include_tool_scope_and_target_path_labels() {
    let project = tempfile::tempdir().expect("project tempdir");
    let home = tempfile::tempdir().expect("home tempdir");

    let options = interactive_target_options(home.path(), project.path());
    assert_eq!(options.len(), 10);

    let first = &options[0];
    assert_eq!(first.target.tool, SupportedTool::Claude);
    assert_eq!(first.target.scope, SkillScope::Global);
    assert_eq!(first.install_state, InteractiveInstallState::NotInstalled);
    assert!(!first.default_selected);
    assert!(first.label.contains("Claude"));
    assert!(first.label.contains("(claude)"));
    assert!(first.label.contains("[global]"));
    assert!(first.label.contains("[not-installed]"));
    assert_eq!(
        first.target_path,
        home.path()
            .join(".claude")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md")
    );

    let project_local_pi = options
        .iter()
        .find(|option| {
            option.target.tool == SupportedTool::Pi
                && option.target.scope == SkillScope::ProjectLocal
        })
        .expect("pi/project option exists");
    assert!(project_local_pi.label.contains("pi"));
    assert!(project_local_pi.label.contains("[project]"));
    assert!(project_local_pi.label.contains("[not-installed]"));
    assert_eq!(
        project_local_pi.target_path,
        project
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md")
    );
}

#[test]
fn skill_install_ux_interactive_options_mark_installed_states_and_defaults() {
    let project = tempfile::tempdir().expect("project tempdir");
    let home = tempfile::tempdir().expect("home tempdir");

    let claude_global_path = home
        .path()
        .join(".claude")
        .join("skills")
        .join("codex-image")
        .join("SKILL.md");
    std::fs::create_dir_all(claude_global_path.parent().expect("parent")).expect("create parent");
    std::fs::write(
        &claude_global_path,
        codex_image::skill_installer::render_managed_skill_content(),
    )
    .expect("seed managed current");

    let codex_global_path = home
        .path()
        .join(".agents")
        .join("skills")
        .join("codex-image")
        .join("SKILL.md");
    std::fs::create_dir_all(codex_global_path.parent().expect("parent")).expect("create parent");
    let outdated_body = codex_image::skill_installer::render_skill_body().replacen(
        "Keep outputs in project-controlled directories.",
        "Keep outputs in local working directories.",
        1,
    );
    let outdated = format!(
        "{}\n{}",
        codex_image::skill_installer::managed_marker_line(&outdated_body),
        outdated_body
    );
    std::fs::write(&codex_global_path, outdated).expect("seed managed outdated");

    let claude_project_path = project
        .path()
        .join(".claude")
        .join("skills")
        .join("codex-image")
        .join("SKILL.md");
    std::fs::create_dir_all(claude_project_path.parent().expect("parent")).expect("create parent");
    std::fs::write(&claude_project_path, "# manual custom skill\n").expect("seed manual protected");

    let options = interactive_target_options(home.path(), project.path());

    let claude_global = options
        .iter()
        .find(|option| {
            option.target.tool == SupportedTool::Claude && option.target.scope == SkillScope::Global
        })
        .expect("claude global option");
    assert_eq!(
        claude_global.install_state,
        InteractiveInstallState::InstalledCurrent
    );
    assert!(claude_global.default_selected);
    assert!(claude_global.label.contains("[installed]"));

    let codex_global = options
        .iter()
        .find(|option| {
            option.target.tool == SupportedTool::Codex && option.target.scope == SkillScope::Global
        })
        .expect("codex global option");
    assert_eq!(
        codex_global.install_state,
        InteractiveInstallState::InstalledOutdated
    );
    assert!(codex_global.default_selected);
    assert!(codex_global.label.contains("[installed:outdated]"));

    let claude_project = options
        .iter()
        .find(|option| {
            option.target.tool == SupportedTool::Claude
                && option.target.scope == SkillScope::ProjectLocal
        })
        .expect("claude project option");
    assert_eq!(
        claude_project.install_state,
        InteractiveInstallState::InstalledProtected
    );
    assert!(claude_project.default_selected);
    assert!(claude_project.label.contains("[installed:protected]"));
}

#[test]
fn skill_install_ux_prompt_boundary_allows_multiple_targets_via_fake_selector() {
    let selector = FakeSelector::from_result(Ok(vec![
        SkillInstallTarget::new(SupportedTool::Pi, SkillScope::Global),
        SkillInstallTarget::new(SupportedTool::OpenCode, SkillScope::ProjectLocal),
    ]));

    let options = vec![
        InteractiveTargetOption {
            target: SkillInstallTarget::new(SupportedTool::Pi, SkillScope::Global),
            target_path: std::path::PathBuf::from("/tmp/pi-global/SKILL.md"),
            install_state: InteractiveInstallState::NotInstalled,
            default_selected: false,
            label: "pi global".to_string(),
        },
        InteractiveTargetOption {
            target: SkillInstallTarget::new(SupportedTool::OpenCode, SkillScope::ProjectLocal),
            target_path: std::path::PathBuf::from("/tmp/opencode-project/SKILL.md"),
            install_state: InteractiveInstallState::NotInstalled,
            default_selected: false,
            label: "opencode project".to_string(),
        },
    ];

    let selected = select_interactive_targets(&selector, &options).expect("selection should pass");

    assert_eq!(selector.call_count(), 1);
    assert_eq!(
        selected,
        vec![
            SkillInstallTarget::new(SupportedTool::Pi, SkillScope::Global),
            SkillInstallTarget::new(SupportedTool::OpenCode, SkillScope::ProjectLocal),
        ]
    );
}

#[test]
fn skill_install_ux_prompt_boundary_surfaces_empty_and_cancel_errors() {
    let options = vec![InteractiveTargetOption {
        target: SkillInstallTarget::new(SupportedTool::Pi, SkillScope::ProjectLocal),
        target_path: std::path::PathBuf::from("/tmp/pi-project/SKILL.md"),
        install_state: InteractiveInstallState::NotInstalled,
        default_selected: false,
        label: "pi project".to_string(),
    }];

    let empty_selector = FakeSelector::from_result(Err(InteractiveSelectionError::EmptySelection));
    let empty_result = select_interactive_targets(&empty_selector, &options);
    assert!(matches!(
        empty_result,
        Err(InteractiveSelectionError::EmptySelection)
    ));
    assert_eq!(empty_selector.call_count(), 1);

    let cancel_selector = FakeSelector::from_result(Err(InteractiveSelectionError::Cancelled));
    let cancel_result = select_interactive_targets(&cancel_selector, &options);
    assert!(matches!(
        cancel_result,
        Err(InteractiveSelectionError::Cancelled)
    ));
    assert_eq!(cancel_selector.call_count(), 1);
}
