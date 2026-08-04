//! Codec module for encoding/decoding protocol messages
//!
//! Provides high-level encoding/decoding for the BPL protocol frames.

use bytes::{Bytes, BytesMut};
use prost::Message;
use tracing::trace;

use crate::{
    pb::*,
    error::{ProtocolError, Result},
    frame::{Frame, FrameCodec},
    MAGIC_NUMBER, DEFAULT_MAX_FRAME_SIZE,
};

/// Protocol codec for encoding/decoding complete protocol messages
pub struct ProtocolCodec {
    frame_codec: FrameCodec,
    rx_buffer: BytesMut,
}

impl ProtocolCodec {
    /// Create a new protocol codec
    pub fn new() -> Self {
        Self {
            frame_codec: FrameCodec::new(DEFAULT_MAX_FRAME_SIZE as usize),
            rx_buffer: BytesMut::new(),
        }
    }

    /// Encode a session open request
    pub fn encode_session_open_request(
        &mut self,
        request: &SessionOpenRequest,
    ) -> Result<Bytes> {
        let sequence = self.frame_codec.next_tx_sequence();
        let frame = crate::frame::build_session_open_request(request, sequence)?;
        self.frame_codec.encode(frame)
    }

    /// Encode a session open response
    pub fn encode_session_open_response(
        &mut self,
        response: &SessionOpenResponse,
    ) -> Result<Bytes> {
        let sequence = self.frame_codec.next_tx_sequence();
        let frame = crate::frame::build_session_open_response(response, sequence)?;
        self.frame_codec.encode(frame)
    }

    /// Encode a keepalive
    pub fn encode_keepalive(&mut self, timestamp: u64) -> Result<Bytes> {
        let sequence = self.frame_codec.next_tx_sequence();
        let frame = crate::frame::build_keepalive(sequence, timestamp)?;
        self.frame_codec.encode(frame)
    }

    /// Encode a capability negotiation request
    pub fn encode_capability_negotiate(
        &mut self,
        request: &CapabilityNegotiateRequest,
    ) -> Result<Bytes> {
        let sequence = self.frame_codec.next_tx_sequence();
        let frame = crate::frame::build_capability_negotiate(request, sequence)?;
        self.frame_codec.encode(frame)
    }

    /// Encode an authentication challenge
    pub fn encode_auth_challenge(
        &mut self,
        challenge: &AuthChallenge,
    ) -> Result<Bytes> {
        let sequence = self.frame_codec.next_tx_sequence();
        let frame = crate::frame::build_auth_challenge(challenge, sequence)?;
        self.frame_codec.encode(frame)
    }

    /// Encode an authentication response
    pub fn encode_auth_response(
        &mut self,
        response: &AuthResponse,
    ) -> Result<Bytes> {
        let sequence = self.frame_codec.next_tx_sequence();
        let frame = crate::frame::build_auth_response(response, sequence)?;
        self.frame_codec.encode(frame)
    }

    /// Encode a channel open request
    pub fn encode_channel_open_request(
        &mut self,
        request: &ChannelOpenRequest,
    ) -> Result<Bytes> {
        let sequence = self.frame_codec.next_tx_sequence();
        let frame = crate::frame::build_channel_open_request(request, sequence)?;
        self.frame_codec.encode(frame)
    }

    /// Encode a flow control update
    pub fn encode_flow_control_update(
        &mut self,
        update: &FlowControlUpdate,
    ) -> Result<Bytes> {
        let sequence = self.frame_codec.next_tx_sequence();
        let frame = crate::frame::build_flow_control_update(update, sequence)?;
        self.frame_codec.encode(frame)
    }

    /// Encode a generic control frame
    pub fn encode_control_frame<T: Message>(
        &mut self,
        frame_type: FrameType,
        payload: &T,
        channel_id: u32,
    ) -> Result<Bytes> {
        let sequence = self.frame_codec.next_tx_sequence();
        let frame = crate::frame::build_control_frame(frame_type, payload, channel_id, sequence)?;
        self.frame_codec.encode(frame)
    }

    /// Decode frames from incoming data
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<Frame>> {
        self.rx_buffer.extend_from_slice(data);
        let mut frames = Vec::new();

        loop {
            match self.frame_codec.decode(&mut self.rx_buffer)? {
                Some(frame) => {
                    trace!("Decoded frame: type={:?}, channel={}, seq={}, payload={} bytes",
                        frame.header.r#type,
                        frame.header.channel_id.as_ref().map(|c| c.value).unwrap_or(0),
                        frame.header.sequence.as_ref().map(|s| s.value).unwrap_or(0),
                        frame.payload.len());

                    // Try to reassemble fragments
                    if let Some(reassembled) = self.frame_codec.reassemble_fragment(frame)? {
                        frames.push(reassembled);
                    }
                }
                None => break,
            }
        }

        Ok(frames)
    }

    /// Get the frame codec for advanced usage
    pub fn frame_codec(&mut self) -> &mut FrameCodec {
        &mut self.frame_codec
    }

    /// Reset the codec state (for reconnection)
    pub fn reset(&mut self) {
        self.frame_codec.reset_sequences();
        self.rx_buffer.clear();
    }

    /// Get current RX sequence number
    pub fn rx_sequence(&self) -> u64 {
        self.frame_codec.rx_sequence()
    }

    /// Set RX sequence number (for reconnection)
    pub fn set_rx_sequence(&mut self, seq: u64) {
        self.frame_codec.rx_sequence = seq; // Note: direct access for reconnection
    }
}

impl Default for ProtocolCodec {
    fn default() -> Self {
        Self::new()
    }
}

/// Decode a frame payload as a specific message type
pub fn decode_frame_payload<T: Message + Default>(frame: &Frame) -> Result<T> {
    T::decode(frame.payload.as_ref())
        .map_err(|e| ProtocolError::Deserialization(e.to_string()))
}

/// Encode a message as frame payload
pub fn encode_frame_payload<T: Message>(msg: &T) -> Result<Bytes> {
    let mut buf = BytesMut::new();
    msg.encode(&mut buf)
        .map_err(|e| ProtocolError::Serialization(e.to_string()))?;
    Ok(buf.freeze())
}

/// Frame type utilities
pub mod frame_types {
    use crate::pb::FrameType;

    pub fn is_control_frame(frame_type: FrameType) -> bool {
        matches!(frame_type,
            FrameType::SessionOpen | FrameType::SessionOpenAck |
            FrameType::SessionClose | FrameType::SessionCloseAck |
            FrameType::Keepalive | FrameType::KeepaliveAck |
            FrameType::CapabilityNegotiate | FrameType::CapabilityAck |
            FrameType::AuthChallenge | FrameType::AuthResponse |
            FrameType::AuthSuccess | FrameType::AuthFailure | FrameType::AuthKeyConfirm |
            FrameType::ChannelOpen | FrameType::ChannelOpenAck |
            FrameType::ChannelClose | FrameType::ChannelCloseAck |
            FrameType::FlowControl |
            FrameType::ServiceRegister | FrameType::ServiceUnregister |
            FrameType::ServiceDiscover | FrameType::ServiceDiscoverResp |
            FrameType::Error
        )
    }

    pub fn is_data_frame(frame_type: FrameType) -> bool {
        matches!(frame_type,
            FrameType::Data | FrameType::DataFragment | FrameType::DataEnd
        )
    }

    pub fn requires_ack(frame_type: FrameType) -> bool {
        is_control_frame(frame_type) || frame_type == FrameType::Data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_roundtrip() {
        let mut codec = ProtocolCodec::new();

        let request = SessionOpenRequest {
            protocol_version: Some(ProtocolVersion { major: 1, minor: 0, patch: 0 }),
            software_version: "1.0.0".to_string(),
            device_name: "Test Client".to_string(),
            device_id: Some(DeviceId { value: vec![1,2,3,4,5,6] }),
            device_type: "desktop".to_string(),
            keepalive: None,
            max_channels: 16,
            max_frame_size: 16384,
            client_nonce: vec![0u8; 32],
            metadata: Default::default(),
        };

        let encoded = codec.encode_session_open_request(&request).unwrap();
        assert!(!encoded.is_empty());
    }
}