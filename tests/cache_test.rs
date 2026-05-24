use ip_tv_neon::consts::CACHE_SCHEMA_VERSION;
use ip_tv_neon::models::{AppData, CacheContainer};

#[test]
fn bincode_u32_first_field_is_4_le_bytes() {
    let c = CacheContainer {
        version: 0xDEADBEEFu32,
        data: AppData::default(),
    };
    let bytes = bincode::serialize(&c).unwrap();
    assert_eq!(
        &bytes[0..4],
        &0xDEADBEEFu32.to_le_bytes(),
        "bincode must encode u32 version field as 4 LE bytes — otherwise load_data() prefix-check is incorrect"
    );
}

#[test]
fn round_trip_valid_version() {
    let original = CacheContainer {
        version: CACHE_SCHEMA_VERSION,
        data: AppData::default(),
    };
    let bytes = bincode::serialize(&original).unwrap();
    let loaded: CacheContainer = bincode::deserialize(&bytes).unwrap();
    assert_eq!(loaded.version, CACHE_SCHEMA_VERSION);
    assert!(loaded.data.channels.is_empty());
    assert!(loaded.data.radio.is_empty());
}

#[test]
fn mismatched_version_detected_via_prefix() {
    let old = CacheContainer {
        version: 1,
        data: AppData::default(),
    };
    let bytes = bincode::serialize(&old).unwrap();
    let version_prefix = u32::from_le_bytes(bytes[..4].try_into().unwrap());
    assert_eq!(
        version_prefix, 1,
        "prefix-read must recover written version"
    );
    assert_ne!(
        version_prefix, CACHE_SCHEMA_VERSION,
        "old version must differ from current"
    );
}

#[test]
fn corrupted_bytes_fail_deserialize() {
    let garbage: Vec<u8> = (0u8..255).cycle().take(1024).collect();
    let result = bincode::deserialize::<CacheContainer>(&garbage);
    assert!(
        result.is_err(),
        "random bytes must not deserialize as valid CacheContainer"
    );
}
