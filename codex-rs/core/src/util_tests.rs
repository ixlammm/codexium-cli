use super::*;
use pretty_assertions::assert_eq;

#[test]
fn normalize_thread_name_trims_and_rejects_empty() {
    assert_eq!(normalize_thread_name("   "), None);
    assert_eq!(
        normalize_thread_name("  my thread  "),
        Some("my thread".to_string())
    );
}
