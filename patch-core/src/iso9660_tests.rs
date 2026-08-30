use super::*;

#[test]
fn builds_and_reextracts_one_deterministic_iso_directory() {
    let files = vec![
        IsoFile {
            name: "456.BAT".to_owned(),
            bytes: b"MADO456\r\n".to_vec(),
        },
        IsoFile {
            name: "MADO456.COM".to_owned(),
            bytes: vec![0x45; 4_111],
        },
    ];
    let first = build_single_directory_iso("MADOU456_KO", "MADOU456", &files).unwrap();
    let second = build_single_directory_iso("MADOU456_KO", "MADOU456", &files).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        extract_logical_directory(&first, "MADOU456_KO", "MADOU456").unwrap(),
        files
    );
}

#[test]
fn rejects_names_outside_the_level_one_ascii_contract() {
    let error = build_single_directory_iso(
        "MADOU456_KO",
        "MADOU456",
        &[IsoFile {
            name: "too-long-name.dat".to_owned(),
            bytes: vec![1],
        }],
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("ISO filename"));
}
