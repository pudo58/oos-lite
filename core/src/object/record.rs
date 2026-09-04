use crate::error::{OosLiteError, Result};
use super::id::ObjectId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectVersion {
    pub version: u32,
    pub manifest_id: String,
    pub created_at: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectRecord {
    pub object_id: ObjectId,
    pub latest_version: u32,
    pub versions: Vec<ObjectVersion>,
}

impl ObjectRecord {
    pub fn new(object_id: ObjectId, manifest_id: String, size_bytes: u64) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let initial_version = ObjectVersion {
            version: 1,
            manifest_id,
            created_at,
            size_bytes,
        };

        Self {
            object_id,
            latest_version: 1,
            versions: vec![initial_version],
        }
    }

    pub fn add_version(&mut self, manifest_id: String, size_bytes: u64) -> u32 {
        self.latest_version += 1;
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.versions.push(ObjectVersion {
            version: self.latest_version,
            manifest_id,
            created_at,
            size_bytes,
        });

        self.latest_version
    }

    pub fn latest(&self) -> Option<&ObjectVersion> {
        self.versions.last()
    }

    pub fn latest_manifest_id(&self) -> &str {
        self.versions
            .last()
            .map(|v| v.manifest_id.as_str())
            .unwrap_or("")
    }

    /// Binary serialization:
    /// [0..16]  : ObjectId (16 bytes)
    /// [16..20] : latest_version (u32-le)
    /// [20..24] : number of versions (u32-le)
    /// For each version:
    ///   - version: u32-le
    ///   - created_at: u64-le
    ///   - size_bytes: u64-le
    ///   - manifest_id_len: u16-le
    ///   - manifest_id bytes (UTF-8)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(self.object_id.as_bytes());
        buf.extend_from_slice(&self.latest_version.to_le_bytes());
        let num_versions = self.versions.len() as u32;
        buf.extend_from_slice(&num_versions.to_le_bytes());

        for v in &self.versions {
            buf.extend_from_slice(&v.version.to_le_bytes());
            buf.extend_from_slice(&v.created_at.to_le_bytes());
            buf.extend_from_slice(&v.size_bytes.to_le_bytes());
            let m_bytes = v.manifest_id.as_bytes();
            buf.extend_from_slice(&(m_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(m_bytes);
        }
        buf
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 24 {
            return Err(OosLiteError::Internal("ObjectRecord bytes too small".to_string()));
        }

        let mut id_bytes = [0u8; 16];
        id_bytes.copy_from_slice(&bytes[0..16]);
        let object_id = ObjectId::from_raw(id_bytes);

        let latest_version = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
        let num_versions = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;

        let mut versions = Vec::with_capacity(num_versions);
        let mut offset = 24;

        for _ in 0..num_versions {
            if offset + 22 > bytes.len() {
                return Err(OosLiteError::Internal("Malformed ObjectRecord bytes".to_string()));
            }
            let version = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            let created_at = u64::from_le_bytes(bytes[offset + 4..offset + 12].try_into().unwrap());
            let size_bytes = u64::from_le_bytes(bytes[offset + 12..offset + 20].try_into().unwrap());
            let m_len = u16::from_le_bytes(bytes[offset + 20..offset + 22].try_into().unwrap()) as usize;
            offset += 22;

            if offset + m_len > bytes.len() {
                return Err(OosLiteError::Internal("Malformed ObjectRecord manifest string".to_string()));
            }
            let manifest_id = String::from_utf8(bytes[offset..offset + m_len].to_vec())
                .map_err(|e| OosLiteError::Internal(format!("Invalid UTF-8 in manifest_id: {}", e)))?;
            offset += m_len;

            versions.push(ObjectVersion {
                version,
                manifest_id,
                created_at,
                size_bytes,
            });
        }

        Ok(Self {
            object_id,
            latest_version,
            versions,
        })
    }
}
