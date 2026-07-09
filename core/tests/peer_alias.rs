//! Peer alias map — documents stale iroh id → canonical peer id resolution.

use std::collections::HashMap;

#[test]
fn alias_resolves_stale_iroh_to_canonical() {
    let mut aliases = HashMap::new();
    let stale = "iroh:88c93b02d3265ebdd9a5bf1c647ed73ff73873106863fdb98ef745f7d45fabdb";
    let canonical = "peer:1c4e1f941bf7643ce9bf5b70e0c787072d046b22593f667bf0f9fd78e643f271";
    aliases.insert(stale.to_string(), canonical.to_string());

    let sessions: HashMap<String, u8> = [(canonical.to_string(), 1)].into();
    let resolved = aliases.get(stale).cloned().unwrap();
    assert_eq!(resolved, canonical);
    assert!(sessions.contains_key(&resolved));
}