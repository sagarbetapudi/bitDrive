package com.bluetoothpersonallink.core.db.entities

import androidx.room.Entity
import androidx.room.PrimaryKey
import com.bluetoothpersonallink.protocol.DeviceId
import kotlinx.serialization.Serializable

@Entity(tableName = "devices")
@Serializable
data class Device(
    @PrimaryKey
    val id: DeviceId,
    val name: String? = null,
    val address: String,
    val paired: Boolean = false,
    val trusted: Boolean = false,
    val psk: ByteArray? = null,
    val lastSeen: Long? = null,
    val createdAt: Long = System.currentTimeMillis(),
    val metadata: String = "{}"
)

@Entity(tableName = "sessions")
@Serializable
data class Session(
    @PrimaryKey
    val id: com.bluetoothpersonallink.protocol.SessionId,
    val deviceId: DeviceId,
    val protocolVersion: String,
    val sessionKeys: ByteArray? = null,
    val capabilities: String = "[]",
    val state: String = "closed",
    val createdAt: Long = System.currentTimeMillis(),
    val lastActivity: Long = System.currentTimeMillis(),
    val bytesSent: Long = 0,
    val bytesReceived: Long = 0
)

@Entity(tableName = "services")
@Serializable
data class Service(
    @PrimaryKey
    val id: String,
    val sessionId: com.bluetoothpersonallink.protocol.SessionId,
    val name: String,
    val version: Int,
    val channelId: Int,
    val channelType: String,
    val active: Boolean = false,
    val healthy: Boolean = true,
    val registeredAt: Long = System.currentTimeMillis(),
    val lastHeartbeat: Long? = null,
    val metadata: String = "{}"
)

@Entity(tableName = "sync_jobs")
@Serializable
data class SyncJob(
    @PrimaryKey
    val id: String,
    val name: String,
    val description: String = "",
    val direction: String,
    val localPath: String,
    val remotePath: String,
    val enabled: Boolean = true,
    val autoSync: Boolean = false,
    val scheduleType: String? = null,
    val scheduleValue: String? = null,
    val conflictStrategy: String = "last_write_wins",
    val filters: String = "{}",
    val status: String = "idle",
    val lastSync: Long? = null,
    val createdAt: Long = System.currentTimeMillis(),
    val updatedAt: Long = System.currentTimeMillis(),
    val stats: String = "{}"
)

@Entity(tableName = "sync_conflicts")
@Serializable
data class SyncConflict(
    @PrimaryKey
    val id: String,
    val jobId: String,
    val localPath: String,
    val remotePath: String,
    val localMetadata: String = "{}",
    val remoteMetadata: String = "{}",
    val localHash: String = "",
    val remoteHash: String = "",
    val detectedAt: Long = System.currentTimeMillis(),
    val resolved: Boolean = false,
    val resolutionStrategy: String? = null
)

@Entity(tableName = "photo_backup_sessions")
@Serializable
data class PhotoBackupSession(
    @PrimaryKey
    val id: String,
    val deviceId: DeviceId,
    val status: String,
    val totalPhotos: Long = 0,
    val backedUp: Long = 0,
    val skipped: Long = 0,
    val errors: Long = 0,
    val bytesTotal: Long = 0,
    val bytesTransferred: Long = 0,
    val startedAt: Long = System.currentTimeMillis(),
    val completedAt: Long? = null,
    val config: String = "{}"
)

@Entity(tableName = "photos")
@Serializable
data class Photo(
    @PrimaryKey
    val id: String,
    val backupSessionId: String? = null,
    val filename: String,
    val path: String,
    val size: Long,
    val mimeType: String? = null,
    val width: Int? = null,
    val height: Int? = null,
    val createdTime: Long = System.currentTimeMillis(),
    val modifiedTime: Long = System.currentTimeMillis(),
    val hash: String? = null,
    val exif: String = "{}",
    val locationLat: Double? = null,
    val locationLon: Double? = null,
    val albums: String = "[]",
    val favorite: Boolean = false,
    val trashed: Boolean = false
)

@Entity(tableName = "albums")
@Serializable
data class Album(
    @PrimaryKey
    val id: String,
    val name: String,
    val description: String = "",
    val photoCount: Long = 0,
    val coverPhotoId: String? = null,
    val systemAlbum: Boolean = false,
    val createdAt: Long = System.currentTimeMillis(),
    val updatedAt: Long = System.currentTimeMillis()
)

@Entity(tableName = "shell_sessions")
@Serializable
data class ShellSession(
    @PrimaryKey
    val id: String,
    val deviceId: DeviceId,
    val name: String? = null,
    val workingDirectory: String = "",
    val environment: String = "{}",
    val state: String = "created",
    val cols: Int = 80,
    val rows: Int = 24,
    val createdAt: Long = System.currentTimeMillis(),
    val lastActivity: Long = System.currentTimeMillis()
)

@Entity(tableName = "shell_history")
@Serializable
data class ShellHistory(
    @PrimaryKey(autoGenerate = true)
    val id: Long = 0,
    val sessionId: String,
    val command: String,
    val exitCode: Int? = null,
    val executedAt: Long = System.currentTimeMillis()
)

@Entity(tableName = "config")
@Serializable
data class ConfigEntry(
    @PrimaryKey
    val key: String,
    val value: String,
    val description: String? = null,
    val readOnly: Boolean = false,
    val secret: Boolean = false,
    val updatedAt: Long = System.currentTimeMillis()
)

@Entity(tableName = "trusted_devices")
@Serializable
data class TrustedDevice(
    @PrimaryKey
    val deviceId: String, // base64 encoded DeviceId
    val name: String,
    val pairedAt: String,
    val lastSeen: String,
    val trusted: Boolean = true,
    val psk: String? = null // base64 encoded PSK
)