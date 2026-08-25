use codex_exec_server::LOCAL_ENVIRONMENT_ID;
use codex_extension_api::ExtensionData;
use codex_skills::ImplicitSkillAccess;
use codex_skills::detect_implicit_skill_invocation_for_command;
use codex_skills::implicit_skill_accesses_for_command;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;

use crate::HostSkillsSnapshot;
use crate::catalog::SkillSourceKind;
use crate::state::ExecutorSkillsStepState;

/// Output of [`detect_implicit_skill_invocation`].
#[derive(Debug)]
pub struct ImplicitSkillInvocation {
    /// Stable identifier the caller uses to deduplicate the notification.
    pub seen_key: String,
    /// Path or opaque resource id surfaced to contributors.
    pub skill_resource: String,
}

/// Identifies the executor-owned or host-owned skill referenced by a command.
pub fn detect_implicit_skill_invocation(
    turn_store: &ExtensionData,
    environment_id: &str,
    command: &str,
    workdir: &PathUri,
    native_workdir: Option<&AbsolutePathBuf>,
) -> Option<ImplicitSkillInvocation> {
    if environment_id == LOCAL_ENVIRONMENT_ID
        && let Some(native_workdir) = native_workdir
        && let Some(host_snapshot) = turn_store.get::<HostSkillsSnapshot>()
        && let Some(skill) = detect_implicit_skill_invocation_for_command(
            host_snapshot.outcome(),
            command,
            native_workdir,
        )
    {
        return Some(ImplicitSkillInvocation {
            skill_resource: skill.path_to_skills_md.to_string_lossy().into_owned(),
            seen_key: format!("host:{}", skill.path_to_skills_md.to_string_lossy()),
        });
    }

    let catalog = turn_store.get::<ExecutorSkillsStepState>()?;
    for access in implicit_skill_accesses_for_command(command, workdir) {
        for entry in &catalog.0.entries {
            let Some((skill_environment_id, skill_path)) = entry.main_prompt.environment_path()
            else {
                continue;
            };
            if !entry.enabled
                || entry.authority.kind != SkillSourceKind::Executor
                || skill_environment_id != environment_id
            {
                continue;
            }

            let matches = match &access {
                ImplicitSkillAccess::Document(path) => path == skill_path,
                ImplicitSkillAccess::Script(path) => skill_path
                    .parent()
                    .and_then(|skill_dir| skill_dir.join("scripts").ok())
                    .is_some_and(|scripts_dir| path.starts_with(&scripts_dir)),
            };
            if matches {
                let id = entry.main_prompt.as_str().to_owned();
                return Some(ImplicitSkillInvocation {
                    skill_resource: id.clone(),
                    seen_key: format!("resource:{id}"),
                });
            }
        }
    }

    None
}

#[cfg(test)]
#[path = "invocation_tests.rs"]
mod tests;
