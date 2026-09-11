//! Integration test exercising `biforgeworks-core` through its public API only.

#[test]
fn public_api_exposes_expected_identity() {
    let meta = biforgeworks_core::app_metadata();

    assert_eq!(meta.name, "BI Forgeworks");
    assert_eq!(meta.identifier, "com.biforgeworks.desktop");
    assert_eq!(meta.version, "0.0.1");
}
