package com.bluetoothpersonallink.protocol

import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.channels.ReceiveChannel
import kotlinx.coroutines.channels.SendChannel
import kotlinx.serialization.decodeFromByteArray
import kotlinx.serialization.encodeToByteArray
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonConfiguration

class FrameCodec(
    private val maxPayloadSize: Int = ProtocolConstants.DEFAULT_MAX_FRAME_SIZE
) {
    private var rxSequence: Long = 0
    private var txSequence: Long = 0
    private val json = Json(JsonConfiguration.Stable)

    fun encode(frame: Frame): ByteArray {
        frame.header.sequence = txSequence++
        frame.header.payloadLength = frame.payload.size
        frame.header.headerCrc = calculateHeaderCrc(frame.header)
        return encodeToByteArray(frame)
    }

    fun decode(data: ByteArray): Frame? {
        if (data.size < 30 + 16) return null // min header + auth tag

        // Check magic number
        val magic = UInt.fromInts(data[0], data[1], data[2], data[3])
        if (magic != ProtocolConstants.MAGIC_NUMBER) {
            throw CodecException("Invalid magic number: ${magic.toString()}")
        }

        val frame = decodeFromByteArray<Frame>(data)

        // Verify header CRC
        val expectedCrc = calculateHeaderCrc(frame.header.copy(headerCrc = 0))
        if (frame.header.headerCrc != expectedCrc) {
            throw CodecException("Header CRC mismatch")
        }

        // Verify sequence (replay protection)
        if (frame.header.sequence <= rxSequence) {
            throw CodecException("Replay detected or out of order sequence")
        }
        rxSequence = frame.header.sequence

        return frame
    }

    fun nextTxSequence(): Long {
        return txSequence++
    }

    fun rxSequence(): Long = rxSequence

    fun resetSequences() {
        rxSequence = 0
        txSequence = 0
    }

    private fun calculateHeaderCrc(header: FrameHeader): UInt {
        // CRC32C of header without the CRC field itself
        val buffer = encodeToByteArray(header.copy(headerCrc = 0))
        return crc32c(buffer)
    }

    private fun crc32c(data: ByteArray): UInt {
        // Simplified CRC32C - in production use a proper implementation
        var crc = 0xFFFFFFFFu
        for (b in data) {
            crc = crc xor (b.toUInt() shl 24)
            repeat(8) {
                if ((crc and 0x80000000u) != 0u) {
                    crc = (crc shl 1) xor 0x1EDC6F41u
                } else {
                    crc = crc shl 1
                }
            }
        }
        return ~crc
    }

    // Fragment handling
    private val fragmentBuffers = mutableMapOf<Long, FragmentBuffer>()

    fun addFragment(frame: Frame): Frame? {
        val flags = frame.header.flags
        if (!flags.isFragment) return frame

        val key = (frame.header.channelId.value.toLong() shl 32) or frame.header.sequence
        val buffer = fragmentBuffers.getOrPut(key) { FragmentBuffer(frame.header.payloadLength) }

        buffer.addFragment(frame.payload, flags.isLastFragment)

        if (flags.isLastFragment) {
            fragmentBuffers.remove(key)
            val reassembled = Frame(
                header = frame.header.copy(
                    payloadLength = buffer.totalSize,
                    flags = FrameFlags()
                ),
                payload = buffer.getData(),
                authTag = frame.authTag
            )
            return reassembled
        }
        return null
    }

    private class FragmentBuffer(expectedSize: Int) {
        private val chunks = mutableListOf<ByteArray>()
        private var received = 0
        private var lastFragment = false

        fun addFragment(data: ByteArray, isLast: Boolean) {
            chunks.add(data)
            received += data.size
            if (isLast) lastFragment = true
        }

        val totalSize: Int get() = received

        fun getData(): ByteArray {
            val result = ByteArray(received)
            var pos = 0
            for (chunk in chunks) {
                result.copyInto(chunk, pos)
                pos += chunk.size
            }
            return result
        }
    }
}

class CodecException(message: String) : Exception(message)

class ProtocolCodec(
    private val frameCodec: FrameCodec = FrameCodec()
) {
    fun encodeSessionOpenRequest(request: SessionOpenRequest): ByteArray {
        return frameCodec.encode(buildControlFrame(FrameType.SESSION_OPEN, request))
    }

    fun encodeSessionOpenResponse(response: SessionOpenResponse): ByteArray {
        return frameCodec.encode(buildControlFrame(FrameType.SESSION_OPEN_ACK, response))
    }

    fun encodeKeepalive(): ByteArray {
        val keepalive = KeepAlive(System.currentTimeMillis(), frameCodec.nextTxSequence())
        return frameCodec.encode(buildControlFrame(FrameType.KEEPALIVE, keepalive))
    }

    fun encodeCapabilityNegotiate(request: CapabilityNegotiateRequest): ByteArray {
        return frameCodec.encode(buildControlFrame(FrameType.CAPABILITY_NEGOTIATE, request))
    }

    fun encodeAuthChallenge(challenge: AuthChallenge): ByteArray {
        return frameCodec.encode(buildControlFrame(FrameType.AUTH_CHALLENGE, challenge))
    }

    fun encodeAuthResponse(response: AuthResponse): ByteArray {
        return frameCodec.encode(buildControlFrame(FrameType.AUTH_RESPONSE, response))
    }

    fun encodeChannelOpenRequest(request: ChannelOpenRequest): ByteArray {
        return frameCodec.encode(buildControlFrame(FrameType.CHANNEL_OPEN, request))
    }

    fun encodeFlowControlUpdate(update: FlowControlUpdate): ByteArray {
        return frameCodec.encode(buildControlFrame(FrameType.FLOW_CONTROL, update))
    }

    private fun <T> buildControlFrame(type: FrameType, payload: T): Frame {
        val payloadBytes = kotlinx.serialization.encodeToByteArray(payload)
        return Frame(
            header = FrameHeader(
                type = type,
                flags = FrameFlags(ackRequired = true),
                channelId = ChannelId(ProtocolConstants.CONTROL_CHANNEL_ID),
                sequence = frameCodec.nextTxSequence(),
                payloadLength = payloadBytes.size
            ),
            payload = payloadBytes
        )
    }
}

// Session, Capability, Auth message types
@kotlinx.serialization.Serializable
data class SessionOpenRequest(
    val clientVersion: ProtocolVersion,
    val clientDeviceId: DeviceId,
    val clientName: String,
    val capabilities: List<ServiceCapability>,
    val keepaliveConfig: KeepAliveConfig,
    val maxChannels: Int,
    val maxFrameSize: Int,
    val clientNonce: ByteArray
)

@kotlinx.serialization.Serializable
data class SessionOpenResponse(
    val result: Result,
    val negotiatedVersion: ProtocolVersion,
    val serverDeviceId: DeviceId,
    val sessionId: SessionId,
    val capabilities: List<ServiceCapability>,
    val keepaliveConfig: KeepAliveConfig,
    val maxChannels: Int,
    val maxFrameSize: Int,
    val serverNonce: ByteArray,
    val sessionKeySeed: ByteArray
)

@kotlinx.serialization.Serializable
data class SessionCloseRequest(
    val reason: ResultCode,
    val message: String
)

@kotlinx.serialization.Serializable
data class KeepAlive(
    val timestamp: Long,
    val sequence: Long
)

@kotlinx.serialization.Serializable
data class CapabilityNegotiateRequest(
    val clientCapabilities: CapabilitySet,
    val requireAll: Boolean
)

@kotlinx.serialization.Serializable
data class CapabilityNegotiateResponse(
    val result: Result,
    val negotiatedCapabilities: List<NegotiatedCapability>,
    val remoteCapabilities: CapabilitySet
)

@kotlinx.serialization.Serializable
data class CapabilitySet(
    val capabilities: List<ServiceCapability>,
    val protocolVersion: ProtocolVersion,
    val softwareVersion: String,
    val deviceName: String,
    val deviceId: DeviceId
)

@kotlinx.serialization.Serializable
data class NegotiatedCapability(
    val serviceId: String,
    val negotiatedVersion: Int,
    val negotiatedFeatures: Map<String, String>,
    val available: Boolean,
    val unavailableReason: String
)

@kotlinx.serialization.Serializable
data class AuthChallenge(
    val challenge: ByteArray,
    val method: String,
    val salt: ByteArray,
    val iterations: Int,
    val serverPublicKey: ByteArray
)

@kotlinx.serialization.Serializable
data class AuthResponse(
    val response: ByteArray,
    val clientNonce: ByteArray,
    val publicKey: ByteArray
)

@kotlinx.serialization.Serializable
data class AuthSuccess(
    val keyConfirmation: ByteArray,
    val sessionKeysEncrypted: ByteArray,
    val sessionKeys: SessionKeySet
)

@kotlinx.serialization.Serializable
data class AuthFailure(
    val reason: ResultCode,
    val message: String
)

@kotlinx.serialization.Serializable
data class SessionKeySet(
    val masterKey: ByteArray,
    val encryptionKey: ByteArray,
    val authenticationKey: ByteArray,
    val ivSalt: ByteArray,
    val channelKeys: Map<Int, ChannelKeys>
)

@kotlinx.serialization.Serializable
data class ChannelKeys(
    val encryptionKey: ByteArray,
    val authKey: ByteArray,
    val iv: ByteArray
)

@kotlinx.serialization.Serializable
data class ChannelOpenRequest(
    val channelId: ChannelId,
    val serviceId: String,
    val windowSize: Int,
    val maxFrameSize: Int
)

@kotlinx.serialization.Serializable
data class ChannelOpenAck(
    val channelId: ChannelId,
    val result: Result,
    val windowSize: Int,
    val maxFrameSize: Int
)

@kotlinx.serialization.Serializable
data class ChannelCloseRequest(
    val channelId: ChannelId,
    val reason: ResultCode,
    val message: String
)

@kotlinx.serialization.Serializable
data class ProtocolVersion(
    val major: Int = 1,
    val minor: Int = 0,
    val patch: Int = 0
)