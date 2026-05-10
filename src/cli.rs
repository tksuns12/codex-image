use std::collections::{HashMap, HashSet};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;

use crate::codex::generate_image_with_codex;
use crate::diagnostics::CliError;
use crate::output::{write_generation_output_from_files, GenerationManifest};
use crate::skill_install_ux::{
    expand_selected_targets, interactive_target_options, select_interactive_targets,
    DialoguerTargetSelector, InstallTargetSelector, InteractiveSelectionError,
    InteractiveTargetOption, SkillInstallTarget, TargetSelectionState,
};
use crate::skill_installer::{
    install_skill, uninstall_skill, SkillInstallOptions, SkillInstallPlan, SkillInstallStatus,
    SkillUninstallStatus,
};
use crate::skills::{resolve_skill_path, SkillScope, SupportedTool};
use crate::updater::{
    run_update_with_installer, BinaryInstaller, FilesystemBinaryInstaller, GitHubReleaseClient,
    UpdateOptions, UpdateResult, UpdateSource,
};

const GPT_IMAGE_MODEL: &str = "gpt-image-2";
const UPDATE_REPOSITORY: &str = "tksuns12/codex-image";

#[derive(Debug, Parser)]
#[command(name = "codex-image", version, about = "Codex Image CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum OutputMode {
    #[default]
    Human,
    Json,
}

#[derive(Args, Clone, Copy, Debug, Default)]
struct OutputArgs {
    /// Success output mode. Human-readable text by default; JSON is stable for automation.
    #[arg(long, value_enum, default_value_t = OutputMode::Human)]
    output: OutputMode,
    /// Suppress success stdout output. Errors are still emitted on stderr.
    #[arg(long, default_value_t = false)]
    quiet: bool,
}

impl OutputArgs {
    const fn effective_mode(self) -> OutputMode {
        if self.quiet {
            OutputMode::Human
        } else {
            self.output
        }
    }

    const fn should_emit_stdout(self) -> bool {
        !self.quiet
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Generate image artifacts and a manifest for the provided prompt via installed Codex.
    Generate {
        /// Prompt text passed to Codex's built-in image generation tool.
        prompt: String,
        /// Output directory where generated image files and manifest.json are written.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Update codex-image to the latest GitHub Release archive for the current platform.
    Update {
        /// Accepted for compatibility; updates replace the current binary by default.
        #[arg(long)]
        yes: bool,
        /// Resolve, download, and validate archive contents without replacing the current binary.
        #[arg(long)]
        dry_run: bool,
        /// Optional GitHub Release tag (for example: v1.2.3). Defaults to latest when omitted.
        #[arg(long = "version", value_name = "TAG", value_parser = parse_release_tag)]
        version: Option<String>,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Manage codex-image native skill installation paths.
    Skill {
        #[command(subcommand)]
        command: SkillCommands,
    },
}

#[derive(Debug, Subcommand)]
enum SkillCommands {
    /// Install the codex-image SKILL.md file for selected supported tool/scope targets.
    /// Omit flags to use interactive target selection when running in a terminal.
    Install {
        /// Tool slug to install for. May be repeated for deterministic multi-target installs.
        #[arg(long, value_enum)]
        tool: Vec<ToolArg>,
        /// Installation scope. May be repeated for deterministic multi-target installs.
        #[arg(long, value_enum)]
        scope: Vec<ScopeArg>,
        /// Required confirmation for non-interactive installs that pass --tool/--scope.
        #[arg(long)]
        yes: bool,
        /// Overwrite manual or tampered existing content.
        #[arg(long, default_value_t = false)]
        force: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
    /// Refresh managed codex-image SKILL.md files for selected supported tool/scope targets.
    /// No-ops current managed files and protects manual edits unless --force is passed.
    /// Omit flags to use interactive target selection when running in a terminal.
    Update {
        /// Tool slug to update for. May be repeated for deterministic multi-target updates.
        #[arg(long, value_enum)]
        tool: Vec<ToolArg>,
        /// Update scope. May be repeated for deterministic multi-target updates.
        #[arg(long, value_enum)]
        scope: Vec<ScopeArg>,
        /// Required confirmation for non-interactive updates that pass --tool/--scope.
        #[arg(long)]
        yes: bool,
        /// Overwrite manual or tampered existing content.
        #[arg(long, default_value_t = false)]
        force: bool,
        #[command(flatten)]
        output: OutputArgs,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ToolArg {
    Claude,
    #[value(name = "claude-code")]
    ClaudeCode,
    Codex,
    Pi,
    #[value(name = "opencode")]
    OpenCode,
}

impl From<ToolArg> for SupportedTool {
    fn from(value: ToolArg) -> Self {
        match value {
            ToolArg::Claude => SupportedTool::Claude,
            ToolArg::ClaudeCode => SupportedTool::ClaudeCode,
            ToolArg::Codex => SupportedTool::Codex,
            ToolArg::Pi => SupportedTool::Pi,
            ToolArg::OpenCode => SupportedTool::OpenCode,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ScopeArg {
    Global,
    #[value(name = "project")]
    Project,
}

impl From<ScopeArg> for SkillScope {
    fn from(value: ScopeArg) -> Self {
        match value {
            ScopeArg::Global => SkillScope::Global,
            ScopeArg::Project => SkillScope::ProjectLocal,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum SkillCommandOperation {
    Install,
    Update,
}

impl SkillCommandOperation {
    const fn slug(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Update => "update",
        }
    }

    const fn missing_confirmation_error(self) -> CliError {
        match self {
            Self::Install => CliError::MissingInstallConfirmation,
            Self::Update => CliError::MissingUpdateConfirmation,
        }
    }

    const fn partial_selection_error(self) -> CliError {
        match self {
            Self::Install => CliError::PartialInstallTargetSelection,
            Self::Update => CliError::PartialUpdateTargetSelection,
        }
    }

    const fn no_targets_non_interactive_error(self) -> CliError {
        match self {
            Self::Install => CliError::NoInstallTargetsInNonInteractiveMode,
            Self::Update => CliError::NoUpdateTargetsInNonInteractiveMode,
        }
    }

    const fn interactive_cancelled_error(self) -> CliError {
        match self {
            Self::Install => CliError::InteractiveInstallSelectionCancelled,
            Self::Update => CliError::InteractiveUpdateSelectionCancelled,
        }
    }

    const fn interactive_prompt_failed_error(self) -> CliError {
        match self {
            Self::Install => CliError::InteractiveInstallPromptFailed,
            Self::Update => CliError::InteractiveUpdatePromptFailed,
        }
    }

    const fn interactive_empty_selection_error(self) -> CliError {
        match self {
            Self::Install => CliError::InteractiveInstallSelectionEmpty,
            Self::Update => CliError::InteractiveUpdateSelectionEmpty,
        }
    }

    const fn write_failed_error(self) -> CliError {
        match self {
            Self::Install => CliError::SkillInstallWriteFailed,
            Self::Update => CliError::SkillUpdateWriteFailed,
        }
    }

    const fn blocked_manual_edit_error(self) -> CliError {
        match self {
            Self::Install => CliError::SkillInstallBlockedManualEdit,
            Self::Update => CliError::SkillUpdateBlockedManualEdit,
        }
    }

    const fn delete_failed_error(self) -> CliError {
        match self {
            Self::Install => CliError::SkillInstallDeleteFailed,
            Self::Update => CliError::SkillUpdateWriteFailed,
        }
    }

    const fn delete_blocked_manual_edit_error(self) -> CliError {
        match self {
            Self::Install => CliError::SkillInstallDeleteBlockedManualEdit,
            Self::Update => CliError::SkillUpdateBlockedManualEdit,
        }
    }
}

pub fn run() -> i32 {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let _ = err.print();
            return err.exit_code();
        }
    };

    match dispatch(cli) {
        Ok(()) => 0,
        Err(err) => {
            let envelope = err.error_envelope();
            let line = serde_json::to_string(&envelope).unwrap_or_else(|_| {
                "{\"error\":{\"code\":\"unknown\",\"message\":\"unexpected failure\",\"recoverable\":false,\"hint\":\"Re-run with supported commands or update the binary.\"}}".to_string()
            });
            eprintln!("{line}");
            err.exit_code().as_i32()
        }
    }
}

fn dispatch(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Commands::Generate {
            prompt,
            out,
            output,
        } => generate(prompt, out, output),
        Commands::Update {
            yes,
            dry_run,
            version,
            output,
        } => update(yes, dry_run, version, output),
        Commands::Skill { command } => dispatch_skill(command),
    }
}

fn generate(prompt: String, out: PathBuf, output: OutputArgs) -> Result<(), CliError> {
    let generated = generate_image_with_codex(&prompt, &out)?;
    let manifest = write_generation_output_from_files(
        &prompt,
        GPT_IMAGE_MODEL,
        &out,
        &[generated.source_path],
    )?;

    if output.should_emit_stdout() {
        match output.effective_mode() {
            OutputMode::Json => {
                let line =
                    serde_json::to_string(&manifest).map_err(|_| CliError::OutputWriteFailed)?;
                println!("{line}");
            }
            OutputMode::Human => {
                println!("{}", format_generate_result_human(&manifest, &out));
            }
        }
    }

    Ok(())
}

fn format_generate_result_human(manifest: &GenerationManifest, out_dir: &Path) -> String {
    let image_count = manifest.images.len();
    let mut lines = vec![
        format!(
            "codex-image generated {image_count} image artifact{}",
            if image_count == 1 { "" } else { "s" }
        ),
        format!("model: {}", manifest.model),
        format!("out: {}", out_dir.display()),
        format!("manifest: {}", manifest.manifest_path),
    ];

    for image in &manifest.images {
        lines.push(format!(
            "image[{index}]: {path} ({byte_count} bytes, {format})",
            index = image.index,
            path = image.path,
            byte_count = image.byte_count,
            format = image.format,
        ));
    }

    lines.join("\n")
}

fn update(
    yes: bool,
    dry_run: bool,
    version: Option<String>,
    output: OutputArgs,
) -> Result<(), CliError> {
    let client = GitHubReleaseClient::new(UPDATE_REPOSITORY)?;
    let installer = FilesystemBinaryInstaller;
    let current_executable =
        std::env::current_exe().map_err(|_| CliError::ProjectRootUnavailable)?;

    let result = execute_update_command(
        &client,
        &installer,
        current_executable,
        env!("CARGO_PKG_VERSION").to_string(),
        yes,
        dry_run,
        version,
    )?;

    print_update_result(&result, output)
}

pub fn execute_update_command<S: UpdateSource, I: BinaryInstaller>(
    source: &S,
    installer: &I,
    current_executable: PathBuf,
    current_version: String,
    yes: bool,
    dry_run: bool,
    version: Option<String>,
) -> Result<UpdateResult, CliError> {
    let options = UpdateOptions {
        current_executable,
        current_version,
        requested_version: version,
        dry_run,
        confirm: yes || !dry_run,
    };

    run_update_with_installer(source, &options, installer).map_err(Into::into)
}

fn print_update_result(result: &UpdateResult, output: OutputArgs) -> Result<(), CliError> {
    if let Some(line) = render_update_result(result, output)? {
        println!("{line}");
    }

    Ok(())
}

fn render_update_result(
    result: &UpdateResult,
    output: OutputArgs,
) -> Result<Option<String>, CliError> {
    if !output.should_emit_stdout() {
        return Ok(None);
    }

    match output.effective_mode() {
        OutputMode::Human => Ok(Some(format_update_result(result))),
        OutputMode::Json => {
            let line = serde_json::to_string(result).map_err(|_| CliError::OutputWriteFailed)?;
            Ok(Some(line))
        }
    }
}

fn format_update_result(result: &UpdateResult) -> String {
    match result.status.as_str() {
        "validated" => format!(
            "codex-image update validated {target_version} for {target}\nasset: {asset}\nbinary: {binary_path}",
            target_version = result.target_version,
            target = result.target,
            asset = result.asset,
            binary_path = result.binary_path,
        ),
        "updated" => format!(
            "codex-image updated from {current_version} to {target_version}\ntarget: {target}\nasset: {asset}\nbinary: {binary_path}",
            current_version = result.current_version,
            target_version = result.target_version,
            target = result.target,
            asset = result.asset,
            binary_path = result.binary_path,
        ),
        other => format!(
            "codex-image update {other}\ntarget version: {target_version}\ntarget: {target}\nasset: {asset}\nbinary: {binary_path}",
            target_version = result.target_version,
            target = result.target,
            asset = result.asset,
            binary_path = result.binary_path,
        ),
    }
}

fn parse_release_tag(value: &str) -> Result<String, String> {
    if !value.starts_with('v') {
        return Err("version tag must start with 'v' (example: v1.2.3)".to_string());
    }

    let mut components = value[1..].split('.');
    let valid = components.clone().count() == 3
        && components.all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()));

    if !valid {
        return Err("version tag must be semantic (example: v1.2.3)".to_string());
    }

    Ok(value.to_string())
}

fn dispatch_skill(command: SkillCommands) -> Result<(), CliError> {
    match command {
        SkillCommands::Install {
            tool,
            scope,
            yes,
            force,
            output,
        } => skill_command(
            SkillCommandOperation::Install,
            &tool,
            &scope,
            yes,
            force,
            output,
        ),
        SkillCommands::Update {
            tool,
            scope,
            yes,
            force,
            output,
        } => skill_command(
            SkillCommandOperation::Update,
            &tool,
            &scope,
            yes,
            force,
            output,
        ),
    }
}

fn skill_command(
    operation: SkillCommandOperation,
    tools: &[ToolArg],
    scopes: &[ScopeArg],
    yes: bool,
    force: bool,
    output: OutputArgs,
) -> Result<(), CliError> {
    let interactive_mode = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let selector = DialoguerTargetSelector;
    skill_command_with_selector(
        operation,
        tools,
        scopes,
        yes,
        force,
        output,
        interactive_mode,
        &selector,
    )
}

fn skill_command_with_selector(
    operation: SkillCommandOperation,
    tools: &[ToolArg],
    scopes: &[ScopeArg],
    yes: bool,
    force: bool,
    output: OutputArgs,
    interactive_mode: bool,
    selector: &dyn InstallTargetSelector,
) -> Result<(), CliError> {
    let project_root = std::env::current_dir().map_err(|_| CliError::ProjectRootUnavailable)?;
    skill_command_with_selector_and_project_root(
        operation,
        tools,
        scopes,
        yes,
        force,
        output,
        interactive_mode,
        selector,
        &project_root,
        None,
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn install_skill_command_with_selector_and_project_root(
    tools: &[ToolArg],
    scopes: &[ScopeArg],
    yes: bool,
    force: bool,
    interactive_mode: bool,
    selector: &dyn InstallTargetSelector,
    project_root: &Path,
    home_dir_override: Option<&Path>,
) -> Result<(), CliError> {
    skill_command_with_selector_and_project_root(
        SkillCommandOperation::Install,
        tools,
        scopes,
        yes,
        force,
        OutputArgs::default(),
        interactive_mode,
        selector,
        project_root,
        home_dir_override,
    )
}

#[allow(clippy::too_many_arguments)]
#[derive(Debug, Clone)]
struct SkillCommandPlan {
    install_targets: Vec<SkillInstallTarget>,
    uninstall_targets: Vec<SkillInstallTarget>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillActionOutput {
    tool: &'static str,
    scope: &'static str,
    status: &'static str,
    target_path: String,
}

#[derive(Debug, Clone, Serialize)]
struct SkillCommandOutput {
    operation: &'static str,
    results: Vec<SkillActionOutput>,
}

#[derive(Debug, Clone, Copy)]
enum SkillAction {
    InstallOrUpdate(SkillInstallTarget),
    Uninstall(SkillInstallTarget),
}

#[allow(clippy::too_many_arguments)]
fn skill_command_with_selector_and_project_root(
    operation: SkillCommandOperation,
    tools: &[ToolArg],
    scopes: &[ScopeArg],
    yes: bool,
    force: bool,
    output: OutputArgs,
    interactive_mode: bool,
    selector: &dyn InstallTargetSelector,
    project_root: &Path,
    home_dir_override: Option<&Path>,
) -> Result<(), CliError> {
    let selected_tools: Vec<SupportedTool> = tools.iter().copied().map(Into::into).collect();
    let selected_scopes: Vec<SkillScope> = scopes.iter().copied().map(Into::into).collect();

    let selection = expand_selected_targets(&selected_tools, &selected_scopes);
    if selection.state == TargetSelectionState::PartialTargets {
        return Err(operation.partial_selection_error());
    }

    let plan = match selection.state {
        TargetSelectionState::Complete => {
            if !yes {
                return Err(operation.missing_confirmation_error());
            }
            SkillCommandPlan {
                install_targets: selection.targets,
                uninstall_targets: Vec::new(),
            }
        }
        TargetSelectionState::NoTargets => {
            if !interactive_mode {
                return Err(operation.no_targets_non_interactive_error());
            }

            let home_for_options =
                effective_home_dir(home_dir_override).ok_or(CliError::HomeUnavailable)?;
            let options = interactive_target_options(&home_for_options, project_root);
            let selected_targets =
                select_interactive_targets(selector, &options).map_err(|error| match error {
                    InteractiveSelectionError::Cancelled => operation.interactive_cancelled_error(),
                    InteractiveSelectionError::PromptFailed => {
                        operation.interactive_prompt_failed_error()
                    }
                    InteractiveSelectionError::EmptySelection => {
                        operation.interactive_empty_selection_error()
                    }
                })?;

            let uninstall_targets = if matches!(operation, SkillCommandOperation::Install) {
                interactive_uninstall_targets(
                    &options,
                    &selected_targets,
                    &home_for_options,
                    project_root,
                )
            } else {
                Vec::new()
            };

            SkillCommandPlan {
                install_targets: selected_targets,
                uninstall_targets,
            }
        }
        TargetSelectionState::PartialTargets => unreachable!("partial targets already handled"),
    };

    run_skill_action_loop(
        operation,
        plan,
        force,
        output,
        project_root,
        home_dir_override,
    )
}

fn interactive_uninstall_targets(
    options: &[InteractiveTargetOption],
    selected_targets: &[SkillInstallTarget],
    home_dir: &Path,
    project_root: &Path,
) -> Vec<SkillInstallTarget> {
    let selected_paths: HashSet<PathBuf> = selected_targets
        .iter()
        .map(|target| resolve_skill_path(target.tool, target.scope, home_dir, project_root))
        .collect();

    let mut uninstall_by_path = HashMap::<PathBuf, SkillInstallTarget>::new();
    for option in options {
        if !option.install_state.is_installed() {
            continue;
        }

        if selected_paths.contains(&option.target_path) {
            continue;
        }

        uninstall_by_path
            .entry(option.target_path.clone())
            .or_insert(option.target);
    }

    let mut uninstall_targets: Vec<SkillInstallTarget> = uninstall_by_path.into_values().collect();
    uninstall_targets.sort_by_key(|target| {
        (
            SupportedTool::all()
                .iter()
                .position(|tool| tool == &target.tool)
                .unwrap_or(usize::MAX),
            SkillScope::all()
                .iter()
                .position(|scope| scope == &target.scope)
                .unwrap_or(usize::MAX),
        )
    });
    uninstall_targets
}

fn run_skill_action_loop(
    operation: SkillCommandOperation,
    plan: SkillCommandPlan,
    force: bool,
    output: OutputArgs,
    project_root: &Path,
    home_dir_override: Option<&Path>,
) -> Result<(), CliError> {
    let selected_scopes: Vec<SkillScope> = plan
        .install_targets
        .iter()
        .map(|target| target.scope)
        .chain(plan.uninstall_targets.iter().map(|target| target.scope))
        .collect();
    let home_dir = resolve_home_dir(&selected_scopes, project_root, home_dir_override)?;

    let mut actions = Vec::<SkillAction>::with_capacity(
        plan.install_targets.len() + plan.uninstall_targets.len(),
    );
    actions.extend(
        plan.install_targets
            .into_iter()
            .map(SkillAction::InstallOrUpdate),
    );
    actions.extend(
        plan.uninstall_targets
            .into_iter()
            .map(SkillAction::Uninstall),
    );

    let mut outputs = Vec::<SkillActionOutput>::with_capacity(actions.len());
    for action in actions {
        match action {
            SkillAction::InstallOrUpdate(target) => {
                let plan =
                    SkillInstallPlan::build(target.tool, target.scope, &home_dir, project_root);
                let result = install_skill(&plan, SkillInstallOptions { force })
                    .map_err(|_| operation.write_failed_error())?;

                if result.status == SkillInstallStatus::BlockedManualEdit {
                    return Err(operation.blocked_manual_edit_error());
                }

                outputs.push(SkillActionOutput {
                    tool: target.tool.slug(),
                    scope: target.scope.slug(),
                    status: result.status.slug(),
                    target_path: result.path.display().to_string(),
                });
            }
            SkillAction::Uninstall(target) => {
                let plan =
                    SkillInstallPlan::build(target.tool, target.scope, &home_dir, project_root);
                let result = uninstall_skill(&plan, SkillInstallOptions { force })
                    .map_err(|_| operation.delete_failed_error())?;

                if result.status == SkillUninstallStatus::BlockedManualEdit {
                    return Err(operation.delete_blocked_manual_edit_error());
                }

                outputs.push(SkillActionOutput {
                    tool: target.tool.slug(),
                    scope: target.scope.slug(),
                    status: result.status.slug(),
                    target_path: result.path.display().to_string(),
                });
            }
        }
    }

    if let Some(line) = render_skill_action_outputs(operation, outputs, output)? {
        println!("{line}");
    }

    Ok(())
}

fn render_skill_action_outputs(
    operation: SkillCommandOperation,
    outputs: Vec<SkillActionOutput>,
    output: OutputArgs,
) -> Result<Option<String>, CliError> {
    if !output.should_emit_stdout() {
        return Ok(None);
    }

    match output.effective_mode() {
        OutputMode::Human => Ok(Some(format_skill_action_outputs(operation, &outputs))),
        OutputMode::Json => {
            let payload = SkillCommandOutput {
                operation: operation.slug(),
                results: outputs,
            };
            let line = serde_json::to_string(&payload).map_err(|_| CliError::OutputWriteFailed)?;
            Ok(Some(line))
        }
    }
}

fn format_skill_action_outputs(
    operation: SkillCommandOperation,
    outputs: &[SkillActionOutput],
) -> String {
    let target_count = outputs.len();
    let mut lines = vec![format!(
        "codex-image skill {} completed {target_count} target{}",
        operation.slug(),
        if target_count == 1 { "" } else { "s" }
    )];

    for output in outputs {
        lines.push(format!(
            "{}: {} -> {}",
            format_skill_target(output),
            output.status.replace('_', " "),
            output.target_path
        ));
    }

    lines.join("\n")
}

fn format_skill_target(output: &SkillActionOutput) -> String {
    format!("{}/{}", output.tool, output.scope)
}

fn resolve_home_dir(
    scopes: &[SkillScope],
    project_root: &Path,
    home_dir_override: Option<&Path>,
) -> Result<PathBuf, CliError> {
    if scopes.contains(&SkillScope::Global) {
        return effective_home_dir(home_dir_override).ok_or(CliError::HomeUnavailable);
    }

    Ok(effective_home_dir(home_dir_override).unwrap_or_else(|| project_root.to_path_buf()))
}

fn effective_home_dir(home_dir_override: Option<&Path>) -> Option<PathBuf> {
    home_dir_override
        .map(|path| path.to_path_buf())
        .or_else(read_home_dir)
}

fn read_home_dir() -> Option<PathBuf> {
    let raw = std::env::var_os("HOME")?;
    if raw.is_empty() {
        return None;
    }

    let as_text = raw.to_string_lossy();
    if as_text.trim().is_empty() {
        return None;
    }

    Some(PathBuf::from(raw))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::{
        format_update_result, install_skill_command_with_selector_and_project_root,
        render_update_result, OutputArgs, OutputMode, ScopeArg, ToolArg,
    };
    use crate::diagnostics::CliError;
    use crate::skill_install_ux::{
        InstallTargetSelector, InteractiveSelectionError, InteractiveTargetOption,
        SkillInstallTarget,
    };
    use crate::skill_installer::render_managed_skill_content;
    use crate::skills::{SkillScope, SupportedTool};
    use crate::updater::UpdateResult;

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

    fn fixture_updated_result() -> UpdateResult {
        UpdateResult {
            status: "updated".to_string(),
            current_version: "0.1.0".to_string(),
            target_version: "v1.2.3".to_string(),
            target: "x86_64-unknown-linux-gnu".to_string(),
            asset: "codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz".to_string(),
            binary_path: "/tmp/codex-image".to_string(),
        }
    }

    fn fixture_validated_result() -> UpdateResult {
        UpdateResult {
            status: "validated".to_string(),
            current_version: "0.1.0".to_string(),
            target_version: "v1.2.3".to_string(),
            target: "x86_64-apple-darwin".to_string(),
            asset: "codex-image-v1.2.3-x86_64-apple-darwin.tar.gz".to_string(),
            binary_path: "/usr/local/bin/codex-image".to_string(),
        }
    }

    #[test]
    fn update_output_mode_defaults_to_human_text() {
        let rendered = render_update_result(&fixture_updated_result(), OutputArgs::default())
            .expect("human rendering should succeed")
            .expect("default mode should emit stdout");

        assert!(rendered.contains("codex-image updated from 0.1.0 to v1.2.3"));
        assert!(rendered.contains("target: x86_64-unknown-linux-gnu"));
        assert!(!rendered.trim_start().starts_with('{'));
    }

    #[test]
    fn update_output_mode_json_serializes_single_update_result_object() {
        let rendered = render_update_result(
            &fixture_updated_result(),
            OutputArgs {
                output: OutputMode::Json,
                quiet: false,
            },
        )
        .expect("json rendering should succeed")
        .expect("json mode should emit stdout");

        assert_eq!(
            rendered.lines().count(),
            1,
            "json output must be single-line"
        );

        let json: serde_json::Value =
            serde_json::from_str(&rendered).expect("rendered JSON should parse");
        assert_eq!(json["status"], "updated");
        assert_eq!(json["target_version"], "v1.2.3");
        assert_eq!(json["target"], "x86_64-unknown-linux-gnu");
    }

    #[test]
    fn update_output_mode_quiet_suppresses_validated_and_updated_stdout() {
        let quiet_human = render_update_result(
            &fixture_updated_result(),
            OutputArgs {
                output: OutputMode::Human,
                quiet: true,
            },
        )
        .expect("quiet human rendering should not error");
        assert!(
            quiet_human.is_none(),
            "quiet should suppress updated stdout"
        );

        let quiet_json = render_update_result(
            &fixture_validated_result(),
            OutputArgs {
                output: OutputMode::Json,
                quiet: true,
            },
        )
        .expect("quiet json rendering should not error");
        assert!(
            quiet_json.is_none(),
            "quiet should suppress validated stdout"
        );
    }

    #[test]
    fn update_result_renderer_uses_human_text_for_success() {
        let output = format_update_result(&fixture_updated_result());

        assert!(output.contains("codex-image updated from 0.1.0 to v1.2.3"));
        assert!(output.contains("target: x86_64-unknown-linux-gnu"));
        assert!(output.contains("asset: codex-image-v1.2.3-x86_64-unknown-linux-gnu.tar.gz"));
        assert!(output.contains("binary: /tmp/codex-image"));
        assert!(!output.trim_start().starts_with('{'));
    }

    #[test]
    fn update_result_renderer_uses_human_text_for_dry_run() {
        let output = format_update_result(&fixture_validated_result());

        assert!(output.contains("codex-image update validated v1.2.3 for x86_64-apple-darwin"));
        assert!(output.contains("asset: codex-image-v1.2.3-x86_64-apple-darwin.tar.gz"));
        assert!(output.contains("binary: /usr/local/bin/codex-image"));
        assert!(!output.trim_start().starts_with('{'));
    }

    #[test]
    fn skill_install_cli_interactive_no_flags_installs_multiple_targets_without_yes() {
        let project = tempfile::tempdir().expect("project tempdir");
        let home = tempfile::tempdir().expect("home tempdir");

        let selector = FakeSelector::from_result(Ok(vec![
            SkillInstallTarget::new(SupportedTool::Pi, SkillScope::Global),
            SkillInstallTarget::new(SupportedTool::Pi, SkillScope::ProjectLocal),
        ]));

        let result = install_skill_command_with_selector_and_project_root(
            &[],
            &[],
            false,
            false,
            true,
            &selector,
            project.path(),
            Some(home.path()),
        );

        assert!(result.is_ok());
        assert_eq!(selector.call_count(), 1);

        assert!(home
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md")
            .is_file());

        assert!(project
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md")
            .is_file());
    }

    #[test]
    fn skill_install_cli_interactive_no_flags_empty_selection_fails_without_writes() {
        let project = tempfile::tempdir().expect("project tempdir");
        let home = tempfile::tempdir().expect("home tempdir");

        let selector = FakeSelector::from_result(Err(InteractiveSelectionError::EmptySelection));

        let result = install_skill_command_with_selector_and_project_root(
            &[],
            &[],
            false,
            false,
            true,
            &selector,
            project.path(),
            Some(home.path()),
        );

        assert!(matches!(
            result,
            Err(CliError::InteractiveInstallSelectionEmpty)
        ));
        assert_eq!(selector.call_count(), 1);

        assert!(!home
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md")
            .exists());

        assert!(!project
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md")
            .exists());
    }

    #[test]
    fn skill_install_cli_interactive_no_flags_cancel_fails_without_writes() {
        let project = tempfile::tempdir().expect("project tempdir");
        let home = tempfile::tempdir().expect("home tempdir");

        let selector = FakeSelector::from_result(Err(InteractiveSelectionError::Cancelled));

        let result = install_skill_command_with_selector_and_project_root(
            &[],
            &[],
            false,
            false,
            true,
            &selector,
            project.path(),
            Some(home.path()),
        );

        assert!(matches!(
            result,
            Err(CliError::InteractiveInstallSelectionCancelled)
        ));
        assert_eq!(selector.call_count(), 1);
    }

    #[test]
    fn skill_install_cli_interactive_selection_respects_manual_edit_block() {
        let project = tempfile::tempdir().expect("project tempdir");
        let home = tempfile::tempdir().expect("home tempdir");

        let target = project
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md");
        std::fs::create_dir_all(target.parent().expect("target parent"))
            .expect("create target parent");
        let manual_content = "# custom skill\nmanual-secret\n";
        std::fs::write(&target, manual_content).expect("seed manual content");

        let selector = FakeSelector::from_result(Ok(vec![SkillInstallTarget::new(
            SupportedTool::Pi,
            SkillScope::ProjectLocal,
        )]));

        let result = install_skill_command_with_selector_and_project_root(
            &[],
            &[],
            false,
            false,
            true,
            &selector,
            project.path(),
            Some(home.path()),
        );

        assert!(matches!(
            result,
            Err(CliError::SkillInstallBlockedManualEdit)
        ));
        assert_eq!(selector.call_count(), 1);

        let preserved = std::fs::read_to_string(target).expect("manual file should stay intact");
        assert_eq!(preserved, manual_content);
    }

    #[test]
    fn skill_install_cli_interactive_unchecked_managed_target_is_deleted() {
        let project = tempfile::tempdir().expect("project tempdir");
        let home = tempfile::tempdir().expect("home tempdir");

        let global_target = home
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md");
        std::fs::create_dir_all(global_target.parent().expect("global parent"))
            .expect("create global parent");
        std::fs::write(&global_target, render_managed_skill_content())
            .expect("seed global managed skill");

        let project_target = project
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md");
        std::fs::create_dir_all(project_target.parent().expect("project parent"))
            .expect("create project parent");
        std::fs::write(&project_target, render_managed_skill_content())
            .expect("seed project managed skill");

        let selector = FakeSelector::from_result(Ok(vec![SkillInstallTarget::new(
            SupportedTool::Pi,
            SkillScope::ProjectLocal,
        )]));

        let result = install_skill_command_with_selector_and_project_root(
            &[],
            &[],
            false,
            false,
            true,
            &selector,
            project.path(),
            Some(home.path()),
        );

        assert!(result.is_ok());
        assert!(
            project_target.exists(),
            "selected target should remain installed"
        );
        assert!(
            !global_target.exists(),
            "unchecked managed target should be deleted"
        );
    }

    #[test]
    fn skill_install_cli_interactive_unchecked_manual_target_blocks_delete_without_force() {
        let project = tempfile::tempdir().expect("project tempdir");
        let home = tempfile::tempdir().expect("home tempdir");

        let global_target = home
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md");
        std::fs::create_dir_all(global_target.parent().expect("global parent"))
            .expect("create global parent");
        std::fs::write(&global_target, "# manual skill\n").expect("seed global manual skill");

        let project_target = project
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md");
        std::fs::create_dir_all(project_target.parent().expect("project parent"))
            .expect("create project parent");
        std::fs::write(&project_target, render_managed_skill_content())
            .expect("seed project managed skill");

        let selector = FakeSelector::from_result(Ok(vec![SkillInstallTarget::new(
            SupportedTool::Pi,
            SkillScope::ProjectLocal,
        )]));

        let result = install_skill_command_with_selector_and_project_root(
            &[],
            &[],
            false,
            false,
            true,
            &selector,
            project.path(),
            Some(home.path()),
        );

        assert!(matches!(
            result,
            Err(CliError::SkillInstallDeleteBlockedManualEdit)
        ));
        assert!(
            global_target.exists(),
            "manual unchecked target should remain"
        );
        assert!(
            project_target.exists(),
            "selected target should remain installed"
        );
    }

    #[test]
    fn skill_install_cli_interactive_alias_selection_does_not_delete_shared_path() {
        let project = tempfile::tempdir().expect("project tempdir");
        let home = tempfile::tempdir().expect("home tempdir");

        let shared_global_target = home
            .path()
            .join(".agents")
            .join("skills")
            .join("codex-image")
            .join("SKILL.md");
        std::fs::create_dir_all(shared_global_target.parent().expect("global parent"))
            .expect("create global parent");
        std::fs::write(&shared_global_target, render_managed_skill_content())
            .expect("seed shared managed skill");

        // Selecting Codex/global should keep the same shared path used by pi/global.
        let selector = FakeSelector::from_result(Ok(vec![SkillInstallTarget::new(
            SupportedTool::Codex,
            SkillScope::Global,
        )]));

        let result = install_skill_command_with_selector_and_project_root(
            &[],
            &[],
            false,
            false,
            true,
            &selector,
            project.path(),
            Some(home.path()),
        );

        assert!(result.is_ok());
        assert!(
            shared_global_target.exists(),
            "shared alias path should not be deleted when any alias remains selected"
        );
    }

    #[test]
    fn skill_install_cli_no_flags_non_tty_fails_fast_without_prompt() {
        let project = tempfile::tempdir().expect("project tempdir");
        let selector = FakeSelector::from_result(Ok(vec![SkillInstallTarget::new(
            SupportedTool::Pi,
            SkillScope::ProjectLocal,
        )]));

        let result = install_skill_command_with_selector_and_project_root(
            &[],
            &[],
            false,
            false,
            false,
            &selector,
            project.path(),
            None,
        );

        assert!(matches!(
            result,
            Err(CliError::NoInstallTargetsInNonInteractiveMode)
        ));
        assert_eq!(selector.call_count(), 0);
    }

    #[test]
    fn skill_install_cli_flagged_installs_still_require_yes_and_skip_selector() {
        let project = tempfile::tempdir().expect("project tempdir");
        let selector = FakeSelector::from_result(Ok(vec![SkillInstallTarget::new(
            SupportedTool::Pi,
            SkillScope::ProjectLocal,
        )]));

        let result = install_skill_command_with_selector_and_project_root(
            &[ToolArg::Pi],
            &[ScopeArg::Project],
            false,
            false,
            true,
            &selector,
            project.path(),
            None,
        );

        assert!(matches!(result, Err(CliError::MissingInstallConfirmation)));
        assert_eq!(selector.call_count(), 0);
    }
}
