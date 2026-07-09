//! QR v4 payload parsing — paste resilience and roundtrip.

use srltcp_core::crypto::identity::{normalize_qr_input, parse_qr_payload, Identity};

#[test]
fn v4_ticket_survives_multiline_paste() {
    let id = Identity::generate();
    let ticket = "iroh1testticketvalue0123456789abcdef";
    let qr = id.qr_payload_v4(ticket);
    let pasted = format!("\n  {qr}\r\n  ");
    let parsed = parse_qr_payload(&pasted).expect("parse pasted v4");
    assert_eq!(parsed.public_key, id.public_key_bytes());
    assert_eq!(parsed.iroh_ticket.as_deref(), Some(ticket));
}

#[test]
fn v4_length_prefix_matches_ticket_bytes() {
    let id = Identity::generate();
    let ticket = "a".repeat(200);
    let qr = id.qr_payload_v4(&ticket);
    let parsed = parse_qr_payload(&qr).unwrap();
    assert_eq!(parsed.iroh_ticket.as_deref(), Some(ticket.as_str()));
}

#[test]
fn invalid_base64_surfaces_clear_error() {
    let err = parse_qr_payload("not!!!valid!!!base64").unwrap_err();
    assert!(err.to_string().contains("base64") || err.to_string().contains("invalid"));
}

#[test]
fn normalize_matches_manual_strip() {
    let raw = "  ABC\nDEF\t";
    assert_eq!(normalize_qr_input(raw), "ABCDEF");
}