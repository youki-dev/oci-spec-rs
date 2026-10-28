use oci_spec::distribution::Reference;
#[test]
fn reference_tags_use_ascii_and_keep_their_length_boundary() {
    for tag in [
        "A".to_owned(),
        "_".into(),
        "tag.with-dash_".into(),
        "x".repeat(128),
    ] {
        let reference: Reference = format!("registry.test/image:{tag}").parse().unwrap();
        assert_eq!(reference.tag(), Some(tag.as_str()));
    }
    for tag in [
        "-tag".to_owned(),
        ".tag".into(),
        "тег".into(),
        "標籤".into(),
        "x".repeat(129),
    ] {
        assert!(format!("registry.test/image:{tag}")
            .parse::<Reference>()
            .is_err());
    }
}
