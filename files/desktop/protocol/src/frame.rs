//! Frame encoding/decoding for the BPL protocol
//!
//! This module implements the wire format for protocol frames including
//! framing, CRC32C checksums, and fragmentation support.

use bytes::{Buf, BufMut, Bytes, BytesMut};
use crc32fast::Hasher;
use prost::Message;
use thiserror::Error;

use crate::{
    pb::{
        FrameHeader, FrameType, FrameFlags, ChannelId, SequenceNumber,
        SessionOpenRequest, SessionOpenResponse, SessionCloseRequest,
        KeepAlive, CapabilityNegotiateRequest, CapabilityNegotiateResponse,
        AuthChallenge, AuthResponse, AuthSuccess, AuthFailure,
        ChannelOpenRequest, ChannelOpenAck, ChannelCloseRequest,
        FlowControlUpdate, ServiceRegisterRequest, ServiceUnregisterRequest,
        ServiceDiscoverRequest, ServiceDiscoverResponse, ErrorFrame,
    },
    error::{ProtocolError, Result},
    MAGIC_NUMBER, DEFAULT_MAX_FRAME_SIZE, CONTROL_CHANNEL_ID, MAX_FRAME_PAYLOAD_SIZE,
};

/// Frame with header and payload
#[derive(Debug, Clone)]
pub struct Frame {
    pub header: FrameHeader,
    pub payload: Bytes,
    pub auth_tag: Bytes, // AEAD authentication tag (16 bytes for AES-GCM)
}

/// Frame codec for encoding/decoding frames
pub struct FrameCodec {
    pub max_payload_size: usize,
    pub rx_sequence: u64,
    pub tx_sequence: u64,
    rx_fragments: Vec<FrameFragment>,
    tx_fragments: Vec<FrameFragment>,
}

/// Fragment for handling large payloads
#[derive(Debug, Clone)]
struct FrameFragment {
    header: FrameHeader,
    payload: BytesMut,
    fragment_index: u32,
    total_fragments: u32,
}

impl FrameCodec {
    /// Create a new frame codec
    pub fn new(max_payload_size: usize) -> Self {
        Self {
            max_payload_size: max_payload_size.min(MAX_FRAME_PAYLOAD_SIZE),
            rx_sequence: 0,
            tx_sequence: 0,
            rx_fragments: Vec::new(),
            tx_fragments: Vec::new(),
        }
    }

    /// Create with default settings
    pub fn default() -> Self {
        Self::new(DEFAULT_MAX_FRAME_SIZE as usize)
    }

    /// Encode a frame
    pub fn encode(&mut self, mut frame: Frame) -> Result<Bytes> {
        // Update sequence number
        frame.header.sequence = Some(SequenceNumber { value: self.tx_sequence });
        self.tx_sequence = self.tx_sequence.wrapping_add(1);

        // Calculate payload length
        frame.header.payload_length = frame.payload.len() as u32;

        // Calculate header CRC
        frame.header.header_crc = self.calculate_header_crc(&frame.header)?;

        // Encode to bytes
        let mut buf = BytesMut::with_capacity(
            4 + 4 + 1 + 1 + 4 + 8 + 4 + 4 + frame.payload.len() + frame.auth_tag.len()
        );

        // Magic (4 bytes)
        buf.put_u32_le(MAGIC_NUMBER);

        // Version (4 bytes)
        buf.put_u32_le(frame.header.version);

        // Frame type (1 byte)
        buf.put_u8(frame.header.r#type as u8);

        // Flags (1 byte - packed)
        buf.put_u8(self.pack_flags(frame.header.flags.as_ref().unwrap()));

        // Channel ID (4 bytes)
        buf.put_u32_le(frame.header.channel_id.as_ref().map_or(0, |c| c.value));

        // Sequence (8 bytes)
        buf.put_u64_le(frame.header.sequence.as_ref().map_or(0, |s| s.value));

        // Payload length (4 bytes)
        buf.put_u32_le(frame.header.payload_length);

        // Header CRC (4 bytes)
        buf.put_u32_le(frame.header.header_crc);

        // Payload
        buf.extend_from_slice(&frame.payload);

        // Auth tag
        buf.extend_from_slice(&frame.auth_tag);

        Ok(buf.freeze())
    }

    /// Decode a frame from bytes
    pub fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Frame>> {
        // Need at least header size (30 bytes) + auth tag (16 bytes)
        const MIN_FRAME_SIZE: usize = 30 + 16;

        if buf.len() < MIN_FRAME_SIZE {
            return Ok(None);
        }

        // Peek at magic number
        let magic = u32::from_le_bytes([
            buf[0], buf[1], buf[2], buf[3],
        ]);

        if magic != MAGIC_NUMBER {
            return Err(ProtocolError::InvalidMagic {
                expected: MAGIC_NUMBER,
                actual: magic,
            });
        }

        // Read header fields to determine payload length
        let version = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        let frame_type = FrameType::try_from(buf[8] as i32).map_err(|_| {
            ProtocolError::UnknownFrameType { r#type: buf[8] }
        })?;
        let flags_byte = buf[9];
        let channel_id = u32::from_le_bytes([buf[10], buf[11], buf[12], buf[13]]);
        let sequence = u64::from_le_bytes([
            buf[14], buf[15], buf[16], buf[17],
            buf[18], buf[19], buf[20], buf[21],
        ]);
        let payload_length = u32::from_le_bytes([buf[22], buf[23], buf[24], buf[25]]);
        let header_crc = u32::from_le_bytes([buf[26], buf[27], buf[28], buf[29]]);

        // Check total frame size
        let total_size = 30 + payload_length as usize + 16; // header + payload + auth_tag
        if buf.len() < total_size {
            return Ok(None);
        }

        // Verify header CRC
        let calc_crc = self.calculate_header_crc_raw(&buf[..30])?;
        if calc_crc != header_crc {
            return Err(ProtocolError::ChecksumMismatch {
                expected: header_crc,
                actual: calc_crc,
            });
        }

        // Extract payload and auth tag
        let payload_start = 30;
        let payload_end = payload_start + payload_length as usize;
        let payload = buf[payload_start..payload_end].to_vec().into();
        let auth_tag = buf[payload_end..payload_end + 16].to_vec().into();

        // Advance buffer
        let _ = buf.split_to(total_size);

        // Verify sequence number (replay protection)
        if sequence < self.rx_sequence {
            return Err(ProtocolError::ReplayDetected { sequence });
        }
        self.rx_sequence = sequence;

        // Parse flags
        let flags = self.unpack_flags(flags_byte);

        // Build header
        let header = FrameHeader {
            magic,
            version,
            r#type: frame_type as i32,
            flags: Some(flags),
            channel_id: Some(ChannelId { value: channel_id }),
            sequence: Some(SequenceNumber { value: sequence }),
            payload_length,
            header_crc,
        };

        Ok(Some(Frame {
            header,
            payload,
            auth_tag,
        }))
    }

    /// Encode a frame fragment for large payloads
    pub fn encode_fragment(
        &mut self,
        frame_type: FrameType,
        channel_id: u32,
        payload: &[u8],
        flags: FrameFlags,
    ) -> Result<Vec<Frame>> {
        let mut frames = Vec::new();

        if payload.len() <= self.max_payload_size {
            // Single frame
            let frame = Frame {
                header: FrameHeader {
                    magic: MAGIC_NUMBER,
                    version: 0x00010000,
                    r#type: frame_type as i32,
                    flags: Some(flags),
                    channel_id: Some(ChannelId { value: channel_id }),
                    sequence: Some(SequenceNumber { value: self.tx_sequence }),
                    payload_length: payload.len() as u32,
                    header_crc: 0,
                },
                payload: Bytes::copy_from_slice(payload),
                auth_tag: Bytes::new(), // Will be filled by encryption layer
            };
            let _encoded = self.encode(frame.clone())?;
            frames.push(frame);
        } else {
            // Fragment
            let total_fragments = (payload.len() + self.max_payload_size - 1) / self.max_payload_size;
            for (i, chunk) in payload.chunks(self.max_payload_size).enumerate() {
                let mut frag_flags = flags.clone();
                frag_flags.is_fragment = true;
                frag_flags.is_last_fragment = i == total_fragments - 1;

                let frame = Frame {
                    header: FrameHeader {
                        magic: MAGIC_NUMBER,
                        version: 0x00010000,
                        r#type: frame_type as i32,
                        flags: Some(frag_flags),
                        channel_id: Some(ChannelId { value: channel_id }),
                        sequence: Some(SequenceNumber { value: self.tx_sequence }),
                        payload_length: chunk.len() as u32,
                        header_crc: 0,
                    },
                    payload: Bytes::copy_from_slice(chunk),
                    auth_tag: Bytes::new(),
                };
                frames.push(frame);
                self.tx_sequence = self.tx_sequence.wrapping_add(1);
            }
        }

        Ok(frames)
    }

    /// Try to reassemble a fragmented frame
    pub fn reassemble_fragment(&mut self, frame: Frame) -> Result<Option<Frame>> {
        let flags = frame.header.flags.as_ref().unwrap();

        if !flags.is_fragment {
            return Ok(Some(frame));
        }

        // Find or create fragment buffer
        let fragment_key = (frame.header.channel_id.as_ref().unwrap().value, frame.header.sequence.as_ref().unwrap().value);

        if let Some(idx) = self.rx_fragments.iter().position(|f| {
            f.header.channel_id.as_ref().unwrap().value == fragment_key.0
        }) {
            let fragment = &mut self.rx_fragments[idx];
            fragment.payload.extend_from_slice(&frame.payload);

            if flags.is_last_fragment {
                let mut complete = fragment.header.clone();
                complete.payload_length = fragment.payload.len() as u32;
                complete.flags = Some(FrameFlags {
                    is_fragment: false,
                    is_last_fragment: false,
                    ..Default::default()
                });

                let result = Frame {
                    header: complete,
                    payload: fragment.payload.clone().freeze(),
                    auth_tag: frame.auth_tag,
                };

                self.rx_fragments.remove(idx);
                Ok(Some(result))
            } else {
                Ok(None)
            }
        } else {
            // New fragment sequence
            self.rx_fragments.push(FrameFragment {
                header: frame.header.clone(),
                payload: BytesMut::from(&frame.payload[..]),
                fragment_index: 0,
                total_fragments: 0,
            });
            Ok(None)
        }
    }

    /// Pack FrameFlags into a single byte
    fn pack_flags(&self, flags: &FrameFlags) -> u8 {
        let mut byte = 0u8;
        if flags.is_fragment { byte |= 1 << 0; }
        if flags.is_last_fragment { byte |= 1 << 1; }
        if flags.encrypted { byte |= 1 << 2; }
        if flags.authenticated { byte |= 1 << 3; }
        if flags.compressed { byte |= 1 << 4; }
        if flags.priority { byte |= 1 << 5; }
        if flags.ack_required { byte |= 1 << 6; }
        if flags.reset_sequence { byte |= 1 << 7; }
        byte
    }

    /// Unpack flags byte into FrameFlags
    fn unpack_flags(&self, byte: u8) -> FrameFlags {
        FrameFlags {
            is_fragment: (byte & (1 << 0)) != 0,
            is_last_fragment: (byte & (1 << 1)) != 0,
            encrypted: (byte & (1 << 2)) != 0,
            authenticated: (byte & (1 << 3)) != 0,
            compressed: (byte & (1 << 4)) != 0,
            priority: (byte & (1 << 5)) != 0,
            ack_required: (byte & (1 << 6)) != 0,
            reset_sequence: (byte & (1 << 7)) != 0,
            retransmission: false,
        }
    }

    /// Calculate CRC32C of frame header
    fn calculate_header_crc(&self, header: &FrameHeader) -> Result<u32> {
        let mut buf = BytesMut::new();
        buf.put_u32_le(header.magic);
        buf.put_u32_le(header.version);
        buf.put_u8(header.r#type as u8);
        buf.put_u8(self.pack_flags(header.flags.as_ref().unwrap()));
        buf.put_u32_le(header.channel_id.as_ref().unwrap().value);
        buf.put_u64_le(header.sequence.as_ref().unwrap().value);
        buf.put_u32_le(header.payload_length);
        let mut hasher = Hasher::new();
        hasher.update(&buf);
        Ok(hasher.finalize())
    }

    /// Calculate CRC32C of raw header bytes (excluding CRC field)
    fn calculate_header_crc_raw(&self, header_bytes: &[u8]) -> Result<u32> {
        let crc_data = &header_bytes[..26];
        let mut hasher = Hasher::new();
        hasher.update(crc_data);
        Ok(hasher.finalize())
    }

    /// Get next TX sequence number
    pub fn next_tx_sequence(&mut self) -> u64 {
        let seq = self.tx_sequence;
        self.tx_sequence = self.tx_sequence.wrapping_add(1);
        seq
    }

    /// Get current RX sequence number
    pub fn rx_sequence(&self) -> u64 {
        self.rx_sequence
    }

    /// Reset sequence numbers (for reconnection)
    pub fn reset_sequences(&mut self) {
        self.rx_sequence = 0;
        self.tx_sequence = 0;
    }
}

/// Build a control frame
pub fn build_control_frame(
    frame_type: FrameType,
    payload: &impl Message,
    channel_id: u32,
    sequence: u64,
) -> Result<Frame> {
    let mut buf = BytesMut::new();
    payload.encode(&mut buf)?;

    Ok(Frame {
        header: FrameHeader {
            magic: MAGIC_NUMBER,
            version: 0x00010000,
            r#type: frame_type as i32,
            flags: Some(FrameFlags {
                is_fragment: false,
                is_last_fragment: false,
                encrypted: false,
                authenticated: true,
                compressed: false,
                priority: false,
                ack_required: true,
                reset_sequence: false,
                retransmission: false,
            }),
            channel_id: Some(ChannelId { value: channel_id }),
            sequence: Some(SequenceNumber { value: sequence }),
            payload_length: buf.len() as u32,
            header_crc: 0, // Will be calculated by codec
        },
        payload: buf.freeze(),
        auth_tag: Bytes::new(),
    })
}

/// Build a session open request frame
pub fn build_session_open_request(
    request: &SessionOpenRequest,
    sequence: u64,
) -> Result<Frame> {
    build_control_frame(FrameType::SessionOpen, request, CONTROL_CHANNEL_ID, sequence)
}

/// Build a session open response frame
pub fn build_session_open_response(
    response: &SessionOpenResponse,
    sequence: u64,
) -> Result<Frame> {
    build_control_frame(FrameType::SessionOpenAck, response, CONTROL_CHANNEL_ID, sequence)
}

/// Build a keepalive frame
pub fn build_keepalive(sequence: u64, timestamp: u64) -> Result<Frame> {
    let keepalive = KeepAlive {
        timestamp: Some(crate::pb::Timestamp { value: timestamp as i64 }),
        sequence: sequence as u32,
    };
    build_control_frame(FrameType::Keepalive, &keepalive, CONTROL_CHANNEL_ID, sequence)
}

/// Build a capability negotiation frame
pub fn build_capability_negotiate(
    request: &CapabilityNegotiateRequest,
    sequence: u64,
) -> Result<Frame> {
    build_control_frame(FrameType::CapabilityNegotiate, request, CONTROL_CHANNEL_ID, sequence)
}

/// Build an authentication challenge frame
pub fn build_auth_challenge(
    challenge: &AuthChallenge,
    sequence: u64,
) -> Result<Frame> {
    build_control_frame(FrameType::AuthChallenge, challenge, CONTROL_CHANNEL_ID, sequence)
}

/// Build an authentication response frame
pub fn build_auth_response(
    response: &AuthResponse,
    sequence: u64,
) -> Result<Frame> {
    build_control_frame(FrameType::AuthResponse, response, CONTROL_CHANNEL_ID, sequence)
}

/// Build a channel open request frame
pub fn build_channel_open_request(
    request: &ChannelOpenRequest,
    sequence: u64,
) -> Result<Frame> {
    build_control_frame(FrameType::ChannelOpen, request, CONTROL_CHANNEL_ID, sequence)
}

/// Build a flow control update frame
pub fn build_flow_control_update(
    update: &FlowControlUpdate,
    sequence: u64,
) -> Result<Frame> {
    build_control_frame(FrameType::FlowControl, update, CONTROL_CHANNEL_ID, sequence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_codec_roundtrip() {
        let mut codec = FrameCodec::default();

        let frame = Frame {
            header: FrameHeader {
                magic: MAGIC_NUMBER,
                version: 0x00010000,
                r#type: FrameType::Data as i32,
                flags: Some(FrameFlags {
                    is_fragment: false,
                    is_last_fragment: false,
                    encrypted: false,
                    authenticated: true,
                    compressed: false,
                    priority: false,
                    ack_required: false,
                    reset_sequence: false,
                    retransmission: false,
                }),
                channel_id: Some(ChannelId { value: 1 }),
                sequence: Some(SequenceNumber { value: 1 }),
                payload_length: 4,
                header_crc: 0,
            },
            payload: Bytes::from_static(b"test"),
            auth_tag: Bytes::from_static(&[0u8; 16]),
        };

        let encoded = codec.encode(frame.clone()).unwrap();
        let mut buf = BytesMut::from(&encoded[..]);
        let decoded = codec.decode(&mut buf).unwrap().unwrap();

        assert_eq!(decoded.payload, frame.payload);
        assert_eq!(decoded.header.channel_id, frame.header.channel_id);
        assert_eq!(decoded.header.r#type, frame.header.r#type);
    }

    #[test]
    fn test_fragment_reassembly() {
        let mut codec = FrameCodec::new(10); // Small fragment size

        let payload = b"Hello, World! This is a test message for fragmentation.";
        let frames = codec.encode_fragment(
            FrameType::Data,
            1,
            payload,
            FrameFlags::default(),
        ).unwrap();

        assert!(frames.len() > 1);

        // Reassemble
        let mut reassembled = None;
        for frame in frames {
            if let Some(frame) = codec.reassemble_fragment(frame).unwrap() {
                reassembled = Some(frame);
            }
        }

        assert!(reassembled.is_some());
        assert_eq!(&reassembled.unwrap().payload[..], payload);
    }
}