use super::*;

#[test]
fn ascii_and_raw_forms_resolve_to_the_same_sfn_bytes() {
    let ascii = FatShortName::ascii("GAME.COM");
    let raw = FatShortName::Raw {
        raw_sfn_hex: "47414d4520202020434f4d".to_owned(),
    };

    assert_eq!(
        ascii.raw_bytes("ASCII fixture").unwrap(),
        raw.raw_bytes("raw fixture").unwrap()
    );
}

#[test]
fn cp932_sfn_is_preserved_as_exact_directory_bytes() {
    let name = FatShortName::Raw {
        raw_sfn_hex: "93b9919088d995b7444154".to_owned(),
    };

    assert_eq!(
        name.raw_bytes("Docho data").unwrap(),
        [
            0x93, 0xb9, 0x91, 0x90, 0x88, 0xd9, 0x95, 0xb7, b'D', b'A', b'T'
        ]
    );
}

#[test]
fn raw_sfn_rejects_reserved_markers_and_noncanonical_hex() {
    for raw_sfn_hex in [
        "00b9919088d995b7444154",
        "e5b9919088d995b7444154",
        "2e20202020202020444154",
        "93B9919088D995B7444154",
    ] {
        assert!(
            FatShortName::Raw {
                raw_sfn_hex: raw_sfn_hex.to_owned(),
            }
            .validate("raw fixture")
            .is_err()
        );
    }
}

#[test]
fn raw_name_objects_reject_unknown_fields() {
    let raw_sfn = r#"{
        "raw_sfn_hex": "93b9919088d995b7444154",
        "ignored": true
    }"#;
    let raw_lha = r#"{
        "raw_name_hex": "93b9919088d995b72e444154",
        "ignored": true
    }"#;

    assert!(serde_json::from_str::<FatShortName>(raw_sfn).is_err());
    assert!(serde_json::from_str::<LhaMemberName>(raw_lha).is_err());
}
