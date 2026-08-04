package com.bluetoothpersonallink.protocol

import kotlinx.serialization.KSerializer
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

// Common types
@kotlinx.serialization.Serializable
data class DeviceId(val value: ByteArray) {
    fun encodeToBase64(): String = android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)
    companion object {
        fun decodeFromBase64(str: String): DeviceId = DeviceId(android.util.Base64.decode(str, android.util.Base64.NO_WRAP))
        fun generate(): DeviceId {
            val bytes = ByteArray(16)
            java.security.SecureRandom().nextBytes(bytes)
            return DeviceId(bytes)
        }
    }
}

@kotlinx.serialization.Serializable
data class SessionId(val value: ByteArray) {
    fun encodeToBase64(): String = android.util.Base64.encodeToString(value, android.util.Base64.NO_WRAP)
    companion object {
        fun decodeFromBase64(str: String): SessionId = SessionId(android.util.Base64.decode(str, android.util.Base64.NO_WRAP))
        fun generate(): SessionId {
            val bytes = ByteArray(16)
            java.security.SecureRandom().nextBytes(bytes)
            return SessionId(bytes)
        }
    }
}

@kotlinx.serialization.Serializable
data class ChannelId(val value: Int)

// Protocol constants
object ProtocolConstants {
    const val PROTOCOL_VERSION: UInt = 0x00010000u
    const val MAGIC_NUMBER: UInt = 0x42504C01u // "BPL\x01"
    const val MAX_FRAME_SIZE: Int = 65535
    const val CONTROL_CHANNEL_ID: Int = 0
    const val DEFAULT_KEEPALIVE_INTERVAL_MS: Long = 30000
    const val DEFAULT_SESSION_TIMEOUT_MS: Long = 90000
    const val MAX_CHANNELS: Int = 16
    const val DEFAULT_WINDOW_SIZE: Int = 65536
    const val DEFAULT_MAX_FRAME_SIZE: Int = 16384
}

// Frame types
@kotlinx.serialization.Serializable
enum class FrameType(val value: Byte) {
    // Data frames
    DATA(0),
    DATA_FRAGMENT(1),
    DATA_END(2),

    // Control frames
    CONTROL(10),

    // Session management
    SESSION_OPEN(20),
    SESSION_OPEN_ACK(21),
    SESSION_CLOSE(22),
    SESSION_CLOSE_ACK(23),

    // Keepalive
    KEEPALIVE(30),
    KEEPALIVE_ACK(31),

    // Capability negotiation
    CAPABILITY_NEGOTIATE(40),
    CAPABILITY_ACK(41),

    // Authentication
    AUTH_CHALLENGE(50),
    AUTH_RESPONSE(51),
    AUTH_SUCCESS(52),
    AUTH_FAILURE(53),
    AUTH_KEY_CONFIRM(54),

    // Channel management
    CHANNEL_OPEN(60),
    CHANNEL_OPEN_ACK(61),
    CHANNEL_CLOSE(62),
    CHANNEL_CLOSE_ACK(63),

    // Flow control
    FLOW_CONTROL(70),

    // Service registry
    SERVICE_REGISTER(80),
    SERVICE_UNREGISTER(81),
    SERVICE_DISCOVER(82),
    SERVICE_DISCOVER_RESP(83),

    // Error
    ERROR(255)
}

// Frame flags
@kotlinx.serialization.Serializable
data class FrameFlags(
    var isFragment: Boolean = false,
    var isLastFragment: Boolean = false,
    var encrypted: Boolean = false,
    var authenticated: Boolean = false,
    var compressed: Boolean = false,
    var priority: Boolean = false,
    var ackRequired: Boolean = false,
    var reserved: Boolean = false
)

// Frame header
@kotlinx.serialization.Serializable
data class FrameHeader(
    val magic: UInt = ProtocolConstants.MAGIC_NUMBER,
    val version: UInt = ProtocolConstants.PROTOCOL_VERSION,
    val type: FrameType = FrameType.DATA,
    val flags: FrameFlags = FrameFlags(),
    val channelId: ChannelId = ChannelId(0),
    val sequence: Long = 0,
    val payloadLength: Int = 0,
    val headerCrc: UInt = 0
)

// Frame
@kotlinx.serialization.Serializable
data class Frame(
    val header: FrameHeader,
    val payload: ByteArray,
    val authTag: ByteArray = ByteArray(0)
)

// Result codes
@kotlinx.serialization.Serializable
enum class ResultCode(val value: Int) {
    SUCCESS(0),
    ERROR_GENERIC(1),
    ERROR_INVALID_ARGUMENT(2),
    ERROR_NOT_FOUND(3),
    ERROR_PERMISSION_DENIED(4),
    ERROR_NOT_CONNECTED(5),
    ERROR_SESSION_CLOSED(6),
    ERROR_CHANNEL_CLOSED(7),
    ERROR_AUTH_FAILED(8),
    ERROR_CAPABILITY_MISMATCH(9),
    ERROR_PROTOCOL_VERSION(10),
    ERROR_SEQUENCE_MISMATCH(11),
    ERROR_REPLAY_DETECTED(12),
    ERROR_DECRYPTION_FAILED(13),
    ERROR_BUFFER_TOO_SMALL(14),
    ERROR_TIMEOUT(15),
    ERROR_CANCELLED(16),
    ERROR_BUSY(17),
    ERROR_UNSUPPORTED(18),
    ERROR_CONFLICT(19),
    ERROR_QUOTA_EXCEEDED(20),
    ERROR_IO(21)
}

// Result
@kotlinx.serialization.Serializable
data class Result(
    val code: ResultCode = ResultCode.SUCCESS,
    val message: String = "",
    val details: ByteArray = ByteArray(0)
)

// File metadata
@kotlinx.serialization.Serializable
data class FileMetadata(
    val path: String = "",
    val name: String = "",
    val size: Long = 0,
    val isDirectory: Boolean = false,
    val modifiedTime: Long = 0,
    val createdTime: Long = 0,
    val mimeType: String = "",
    val hash: String = "",
    val permissions: Int = 0,
    val owner: String = "",
    val group: String = "",
    val isSymlink: Boolean = false,
    val symlinkTarget: String = "",
    val extendedAttributes: Map<String, String> = emptyMap()
)

// Directory entry
@kotlinx.serialization.Serializable
data class DirectoryEntry(
    val metadata: FileMetadata
)

// Byte range
@kotlinx.serialization.Serializable
data class ByteRange(
    val offset: Long = 0,
    val length: Long = 0
)

// Service capability
@kotlinx.serialization.Serializable
data class ServiceCapability(
    val serviceId: String = "",
    val version: Int = 1,
    val name: String = "",
    val description: String = "",
    val required: Boolean = false,
    val metadata: Map<String, String> = emptyMap()
)

// Error detail
@kotlinx.serialization.Serializable
data class ErrorDetail(
    val code: ResultCode = ResultCode.ERROR_GENERIC,
    val message: String = "",
    val serviceId: String = "",
    val operation: String = "",
    val context: Map<String, String> = emptyMap()
)

// Keepalive config
@kotlinx.serialization.Serializable
data class KeepAliveConfig(
    val intervalMs: Long = ProtocolConstants.DEFAULT_KEEPALIVE_INTERVAL_MS,
    val timeoutMs: Long = 10000,
    val maxMissed: Int = 3
)

// Window update
@kotlinx.serialization.Serializable
data class WindowUpdate(
    val channelId: ChannelId,
    val windowSize: Int
)