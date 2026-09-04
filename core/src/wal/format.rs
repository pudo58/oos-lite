use std::io::{Cursor, Read};

use crate::chunk::ChunkId;
use crate::error::{OosLiteError, Result};
use crate::manifest::Manifest;
use crate::object::ObjectId;

pub const WAL_MAGIC: [u8; 4] = *b"OOSW";
pub const WAL_HEADER_SIZE: usize = 4 + 8 + 1 + 4 + 4; // 21 bytes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WalRecordType {
    PutObject = 1,
    Checkpoint = 2,
}

impl TryFrom<u8> for WalRecordType {
    type Error = OosLiteError;

    fn try_from(val: u8) -> Result<Self> {
        match val {
            1 => Ok(Self::PutObject),
            2 => Ok(Self::Checkpoint),
            _ => Err(OosLiteError::WalRecovery(format!(
                "Unknown WAL record type: {val}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WalPutPayload {
    pub name: String,
    pub object_id: ObjectId,
    pub version: u32,
    pub manifest: Manifest,
    pub chunks: Vec<(ChunkId, Vec<u8>)>,
}

#[derive(Debug, Clone)]
pub enum WalRecordPayload {
    PutObject(WalPutPayload),
    Checkpoint(u64), // checkpoint_lsn
}

#[derive(Debug, Clone)]
pub struct WalRecord {
    pub lsn: u64,
    pub payload: WalRecordPayload,
}

impl WalRecord {
    pub fn new_put(lsn: u64, payload: WalPutPayload) -> Self {
        Self {
            lsn,
            payload: WalRecordPayload::PutObject(payload),
        }
    }

    pub fn new_checkpoint(lsn: u64, checkpoint_lsn: u64) -> Self {
        Self {
            lsn,
            payload: WalRecordPayload::Checkpoint(checkpoint_lsn),
        }
    }

    pub fn record_type(&self) -> WalRecordType {
        match &self.payload {
            WalRecordPayload::PutObject(_) => WalRecordType::PutObject,
            WalRecordPayload::Checkpoint(_) => WalRecordType::Checkpoint,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut payload_buf = Vec::new();
        match &self.payload {
            WalRecordPayload::PutObject(put) => {
                let name_bytes = put.name.as_bytes();
                payload_buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
                payload_buf.extend_from_slice(name_bytes);

                payload_buf.extend_from_slice(put.object_id.as_bytes());
                payload_buf.extend_from_slice(&put.version.to_le_bytes());

                let manifest_bytes = put.manifest.to_bytes();
                payload_buf.extend_from_slice(&(manifest_bytes.len() as u32).to_le_bytes());
                payload_buf.extend_from_slice(&manifest_bytes);

                payload_buf.extend_from_slice(&(put.chunks.len() as u32).to_le_bytes());
                for (chunk_id, chunk_data) in &put.chunks {
                    payload_buf.extend_from_slice(chunk_id.as_bytes());
                    payload_buf.extend_from_slice(&(chunk_data.len() as u32).to_le_bytes());
                    payload_buf.extend_from_slice(chunk_data);
                }
            }
            WalRecordPayload::Checkpoint(lsn) => {
                payload_buf.extend_from_slice(&lsn.to_le_bytes());
            }
        }

        let payload_len = payload_buf.len() as u32;
        let crc = crc32fast::hash(&payload_buf);

        let mut record_buf = Vec::with_capacity(WAL_HEADER_SIZE + payload_buf.len());
        record_buf.extend_from_slice(&WAL_MAGIC);
        record_buf.extend_from_slice(&self.lsn.to_le_bytes());
        record_buf.push(self.record_type() as u8);
        record_buf.extend_from_slice(&payload_len.to_le_bytes());
        record_buf.extend_from_slice(&crc.to_le_bytes());
        record_buf.extend_from_slice(&payload_buf);

        record_buf
    }

    pub fn decode(buf: &[u8]) -> Result<(Self, usize)> {
        if buf.len() < WAL_HEADER_SIZE {
            return Err(OosLiteError::WalRecovery(
                "Buffer too short for WAL header".into(),
            ));
        }

        if buf[0..4] != WAL_MAGIC {
            return Err(OosLiteError::WalRecovery(
                "Invalid WAL magic bytes".into(),
            ));
        }

        let lsn = u64::from_le_bytes(buf[4..12].try_into().unwrap());
        let record_type = WalRecordType::try_from(buf[12])?;
        let payload_len = u32::from_le_bytes(buf[13..17].try_into().unwrap()) as usize;
        let expected_crc = u32::from_le_bytes(buf[17..21].try_into().unwrap());

        let total_size = WAL_HEADER_SIZE + payload_len;
        if buf.len() < total_size {
            return Err(OosLiteError::WalRecovery(
                "Incomplete WAL payload".into(),
            ));
        }

        let payload_slice = &buf[WAL_HEADER_SIZE..total_size];
        let actual_crc = crc32fast::hash(payload_slice);
        if actual_crc != expected_crc {
            return Err(OosLiteError::ChecksumMismatch {
                chunk_id: format!("WAL LSN {lsn}"),
                expected: expected_crc,
                actual: actual_crc,
            });
        }

        let mut cursor = Cursor::new(payload_slice);
        let payload = match record_type {
            WalRecordType::PutObject => {
                let mut name_len_buf = [0u8; 2];
                cursor.read_exact(&mut name_len_buf)?;
                let name_len = u16::from_le_bytes(name_len_buf) as usize;

                let mut name_buf = vec![0u8; name_len];
                cursor.read_exact(&mut name_buf)?;
                let name = String::from_utf8(name_buf).map_err(|e| {
                    OosLiteError::WalRecovery(format!("Invalid UTF-8 in WAL name: {e}"))
                })?;

                let mut oid_buf = [0u8; 16];
                cursor.read_exact(&mut oid_buf)?;
                let object_id = ObjectId::from_raw(oid_buf);

                let mut version_buf = [0u8; 4];
                cursor.read_exact(&mut version_buf)?;
                let version = u32::from_le_bytes(version_buf);

                let mut manifest_len_buf = [0u8; 4];
                cursor.read_exact(&mut manifest_len_buf)?;
                let manifest_len = u32::from_le_bytes(manifest_len_buf) as usize;

                let mut manifest_buf = vec![0u8; manifest_len];
                cursor.read_exact(&mut manifest_buf)?;
                let manifest = Manifest::from_bytes(&manifest_buf)?;

                let mut chunk_count_buf = [0u8; 4];
                cursor.read_exact(&mut chunk_count_buf)?;
                let chunk_count = u32::from_le_bytes(chunk_count_buf) as usize;

                let mut chunks = Vec::with_capacity(chunk_count);
                for _ in 0..chunk_count {
                    let mut cid_buf = [0u8; 32];
                    cursor.read_exact(&mut cid_buf)?;
                    let chunk_id = ChunkId::from_raw(cid_buf);

                    let mut data_len_buf = [0u8; 4];
                    cursor.read_exact(&mut data_len_buf)?;
                    let data_len = u32::from_le_bytes(data_len_buf) as usize;

                    let mut data = vec![0u8; data_len];
                    cursor.read_exact(&mut data)?;

                    chunks.push((chunk_id, data));
                }

                WalRecordPayload::PutObject(WalPutPayload {
                    name,
                    object_id,
                    version,
                    manifest,
                    chunks,
                })
            }
            WalRecordType::Checkpoint => {
                let mut lsn_buf = [0u8; 8];
                cursor.read_exact(&mut lsn_buf)?;
                WalRecordPayload::Checkpoint(u64::from_le_bytes(lsn_buf))
            }
        };

        Ok((WalRecord { lsn, payload }, total_size))
    }
}
