//! 128-bit Hybrid Logical Object ID and object records.

pub mod id;
pub mod record;

pub use id::ObjectId;
pub use record::{ObjectRecord, ObjectVersion};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_id_generate_and_parse() {
        let id1 = ObjectId::generate();
        let id2 = ObjectId::generate();
        assert_ne!(id1, id2);
        assert_eq!(id1.to_hex().len(), 32);

        let parsed: ObjectId = id1.to_hex().parse().expect("parse ObjectId failed");
        assert_eq!(id1, parsed);
    }

    #[test]
    fn test_object_record_versioning_and_serde() {
        let id = ObjectId::generate();
        let mut record = ObjectRecord::new(id, "manifest_v1".to_string(), 1024);
        assert_eq!(record.latest_version, 1);
        assert_eq!(record.versions.len(), 1);

        let v2 = record.add_version("manifest_v2".to_string(), 2048);
        assert_eq!(v2, 2);
        assert_eq!(record.latest_version, 2);
        assert_eq!(record.versions.len(), 2);
        assert_eq!(record.latest_manifest_id(), "manifest_v2");

        let bytes = record.to_bytes();
        let deserialized = ObjectRecord::from_bytes(&bytes).expect("deserialization failed");
        assert_eq!(record, deserialized);
    }
}
