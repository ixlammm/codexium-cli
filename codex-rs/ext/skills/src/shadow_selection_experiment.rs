// This shadow-selection experiment is temporary and should be removed after evaluation.

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use crate::HostSkillsSnapshot;
use codex_protocol::user_input::UserInput;

use crate::catalog::SkillCatalog;
use crate::catalog::SkillCatalogEntry;
use crate::catalog::SkillSourceKind;
use crate::dynamic_skill_selector::CharacterNgramSkillSelector;
use crate::dynamic_skill_selector::CharacterRoutingCardSkillSelector;
use crate::dynamic_skill_selector::CheapSkillSelection;
use crate::dynamic_skill_selector::CheapSkillSelector;
use crate::dynamic_skill_selector::FieldedBm25SkillSelector;
use crate::dynamic_skill_selector::LruPlusLexicalSkillSelector;
use crate::dynamic_skill_selector::LruSkillSelector;
use crate::dynamic_skill_selector::MultiQueryLexicalSkillSelector;
use crate::dynamic_skill_selector::RoutingCardLexicalSkillSelector;
use crate::dynamic_skill_selector::RrfLexicalCharSkillSelector;
use crate::dynamic_skill_selector::SkillSelectionDocument;
use crate::dynamic_skill_selector::WeightedLexicalSkillSelector;

const MAX_SHADOW_QUERY_BYTES: usize = 16 * 1024;
const MAX_SHADOW_RESULTS: usize = 50;

pub(crate) struct ShadowSelectionExperiment {
    selectors: Vec<Box<dyn CheapSkillSelector>>,
}

impl ShadowSelectionExperiment {
    pub(crate) fn new() -> Self {
        Self {
            selectors: vec![
                Box::new(WeightedLexicalSkillSelector),
                Box::new(FieldedBm25SkillSelector),
                Box::new(CharacterNgramSkillSelector),
                Box::new(MultiQueryLexicalSkillSelector),
                Box::new(RrfLexicalCharSkillSelector),
                Box::new(RoutingCardLexicalSkillSelector),
            ],
        }
    }

    pub(crate) fn run(
        &self,
        inputs: &[UserInput],
        catalog: &SkillCatalog,
        explicitly_selected: &[SkillCatalogEntry],
        host_snapshot: Option<&HostSkillsSnapshot>,
        recent_skill_invocations: Arc<RecentSkillInvocations>,
    ) -> ShadowSelectionTurnState {
        let query = build_shadow_query(inputs);
        let explicitly_selected_skill_resources = explicitly_selected
            .iter()
            .map(|entry| normalize_skill_resource(entry.main_prompt.as_str()))
            .collect::<HashSet<_>>();
        let documents = catalog
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.is_model_visible()
                    // Invocation observation currently exists only for host shell use and
                    // orchestrator reads. Keep the candidate set aligned with that universe.
                    && matches!(
                        &entry.authority.kind,
                        SkillSourceKind::Host | SkillSourceKind::Orchestrator
                    )
                    && !explicitly_selected_skill_resources
                        .contains(&normalize_skill_resource(entry.main_prompt.as_str()))
            })
            .map(|(id, entry)| SkillSelectionDocument {
                id,
                name: entry.name.as_str(),
                short_description: entry.short_description.as_deref(),
                description: entry.description.as_str(),
                dependencies: entry.dependencies.as_ref(),
            })
            .collect::<Vec<_>>();
        let eligible_ids = documents
            .iter()
            .map(|document| document.id)
            .collect::<HashSet<_>>();
        let eligible_skill_ids_by_resource = documents
            .iter()
            .map(|document| {
                (
                    normalize_skill_resource(catalog.entries[document.id].main_prompt.as_str()),
                    document.id,
                )
            })
            .collect::<HashMap<_, _>>();
        let eligible_skill_resources = eligible_skill_ids_by_resource
            .keys()
            .cloned()
            .collect::<HashSet<_>>();
        let recent_skill_ids = recent_skill_invocations
            .snapshot()
            .iter()
            .filter_map(|resource| eligible_skill_ids_by_resource.get(resource).copied())
            .collect();
        let routing_selector = CharacterRoutingCardSkillSelector::new(catalog, host_snapshot);
        let lru_selector = LruSkillSelector::new(recent_skill_ids);
        let lru_plus_lexical_selector = LruPlusLexicalSkillSelector::new(lru_selector.clone());

        for selector in self
            .selectors
            .iter()
            .map(std::convert::AsRef::as_ref)
            .chain([
                &routing_selector as &dyn CheapSkillSelector,
                &lru_selector as &dyn CheapSkillSelector,
                &lru_plus_lexical_selector as &dyn CheapSkillSelector,
            ])
        {
            let selection =
                selector.select(&query.text, &documents, /*limit*/ MAX_SHADOW_RESULTS);
            let selected_ids = sanitize_selected_ids(&selection, &eligible_ids);
            tracing::debug!(
                method = selector.method(),
                catalog_entries = documents.len(),
                selected_entries = selected_ids.len(),
                query_terms = selection.query_term_count,
                query_truncated = query.truncated || selection.query_truncated,
                candidate_set_truncated = selection.candidate_set_truncated,
                "ran shadow skill selection"
            );
        }

        ShadowSelectionTurnState {
            eligible_skill_resources,
            seen_skill_resources: Mutex::new(HashSet::new()),
            recent_skill_invocations,
        }
    }

    pub(crate) fn record_invocation(&self, state: &ShadowSelectionTurnState, skill_resource: &str) {
        let skill_resource = normalize_skill_resource(skill_resource);
        if !state.eligible_skill_resources.contains(&skill_resource) {
            return;
        }
        if !state
            .seen_skill_resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(skill_resource.clone())
        {
            return;
        }
        state.recent_skill_invocations.record(&skill_resource);
    }
}

pub(crate) struct ShadowSelectionTurnState {
    eligible_skill_resources: HashSet<String>,
    seen_skill_resources: Mutex<HashSet<String>>,
    recent_skill_invocations: Arc<RecentSkillInvocations>,
}

#[derive(Default)]
pub(crate) struct RecentSkillInvocations {
    skill_resources: Mutex<VecDeque<String>>,
}

impl RecentSkillInvocations {
    fn snapshot(&self) -> Vec<String> {
        self.skill_resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    fn record(&self, skill_resource: &str) {
        let mut skill_resources = self
            .skill_resources
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(index) = skill_resources
            .iter()
            .position(|resource| resource == skill_resource)
        {
            skill_resources.remove(index);
        }
        skill_resources.push_front(skill_resource.to_string());
        skill_resources.truncate(MAX_SHADOW_RESULTS);
    }
}

fn sanitize_selected_ids(
    selection: &CheapSkillSelection,
    eligible_ids: &HashSet<usize>,
) -> Vec<usize> {
    let mut seen = HashSet::new();
    selection
        .candidate_ids
        .iter()
        .copied()
        .filter(|id| eligible_ids.contains(id) && seen.insert(*id))
        .take(MAX_SHADOW_RESULTS)
        .collect()
}

fn normalize_skill_resource(skill_resource: &str) -> String {
    skill_resource.replace('\\', "/")
}

struct ShadowQuery {
    text: String,
    truncated: bool,
}

fn build_shadow_query(inputs: &[UserInput]) -> ShadowQuery {
    let mut text = String::new();
    let mut truncated = false;
    for input in inputs {
        let part = match input {
            UserInput::Text { text, .. } => text.as_str(),
            UserInput::Skill { name, .. } | UserInput::Mention { name, .. } => name.as_str(),
            _ => continue,
        };
        if part.is_empty() {
            continue;
        }
        if !text.is_empty() && !push_bounded(&mut text, " ") {
            truncated = true;
            break;
        }
        if !push_bounded(&mut text, part) {
            truncated = true;
            break;
        }
    }
    ShadowQuery { text, truncated }
}

fn push_bounded(destination: &mut String, value: &str) -> bool {
    let remaining = MAX_SHADOW_QUERY_BYTES.saturating_sub(destination.len());
    if value.len() <= remaining {
        destination.push_str(value);
        return true;
    }
    let mut end = remaining;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    destination.push_str(&value[..end]);
    false
}

#[cfg(test)]
#[path = "shadow_selection_experiment_tests.rs"]
mod tests;
