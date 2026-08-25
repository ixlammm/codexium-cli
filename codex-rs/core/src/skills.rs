use crate::config::Config;
use crate::session::session::Session;
use crate::session::turn_context::TurnContext;
use codex_extension_api::SkillInvocationInput;
use codex_extension_api::SkillInvocationKind;
use codex_skills_extension::HostSkillsLoadInput;
use codex_skills_extension::detect_implicit_skill_invocation;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use codex_utils_plugins::PluginSkillRoot;
use std::collections::HashSet;
use tokio::sync::Mutex;

#[derive(Debug, Default)]
struct ImplicitSkillInvocations(Mutex<HashSet<String>>);

pub(crate) fn skills_load_input_from_config(
    config: &Config,
    effective_skill_roots: Vec<PluginSkillRoot>,
) -> HostSkillsLoadInput {
    HostSkillsLoadInput::new(
        config.cwd.clone(),
        effective_skill_roots,
        config.config_layer_stack.clone(),
    )
}

pub(crate) async fn maybe_emit_implicit_skill_invocation(
    sess: &Session,
    turn_context: &TurnContext,
    command: &str,
    workdir: &PathUri,
    native_workdir: Option<&AbsolutePathBuf>,
    environment_id: &str,
) {
    let Some(invocation) = detect_implicit_skill_invocation(
        turn_context.extension_data.as_ref(),
        environment_id,
        command,
        workdir,
        native_workdir,
    ) else {
        return;
    };
    let skill_resource = invocation.skill_resource.clone();
    let inserted = {
        let skill_invocations = turn_context
            .extension_data
            .get_or_init(ImplicitSkillInvocations::default);
        let mut seen_skills = skill_invocations.0.lock().await;
        seen_skills.insert(invocation.seen_key)
    };
    if !inserted {
        return;
    }

    for contributor in sess.services.extensions.skill_invocation_contributors() {
        contributor
            .on_skill_invocation(SkillInvocationInput {
                session_store: &sess.services.session_extension_data,
                thread_store: &sess.services.thread_extension_data,
                turn_store: turn_context.extension_data.as_ref(),
                turn_id: turn_context.sub_id.as_str(),
                skill_resource: skill_resource.as_str(),
                kind: SkillInvocationKind::Implicit,
            })
            .await;
    }
}
