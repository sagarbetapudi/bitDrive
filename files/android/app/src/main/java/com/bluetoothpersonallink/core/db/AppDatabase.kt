package com.bluetoothpersonallink.core.db

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.TypeConverters
import com.bluetoothpersonallink.core.db.dao.*
import com.bluetoothpersonallink.core.db.entities.*
import com.bluetoothpersonallink.protocol.DeviceId

@Database(
    entities = [
        Device::class,
        Session::class,
        Service::class,
        SyncJob::class,
        SyncConflict::class,
        PhotoBackupSession::class,
        Photo::class,
        Album::class,
        ShellSession::class,
        ShellHistory::class,
        ConfigEntry::class,
        TrustedDevice::class
    ],
    version = 7,
    exportSchema = false
)
@TypeConverters(Converters::class)
abstract class AppDatabase : RoomDatabase() {

    abstract fun deviceDao(): DeviceDao
    abstract fun sessionDao(): SessionDao
    abstract fun serviceDao(): ServiceDao
    abstract fun syncJobDao(): SyncJobDao
    abstract fun syncConflictDao(): SyncConflictDao
    abstract fun photoBackupSessionDao(): PhotoBackupSessionDao
    abstract fun photoDao(): PhotoDao
    abstract fun albumDao(): AlbumDao
    abstract fun shellSessionDao(): ShellSessionDao
    abstract fun shellHistoryDao(): ShellHistoryDao
    abstract fun configDao(): ConfigDao
    abstract fun trustedDeviceDao(): TrustedDeviceDao

    companion object {
        @Volatile
        private var INSTANCE: AppDatabase? = null

        fun getDatabase(context: Context): AppDatabase {
            return INSTANCE ?: synchronized(this) {
                val instance = Room.databaseBuilder(
                    context.applicationContext,
                    AppDatabase::class.java, "bpl.db"
                ).fallbackToDestructiveMigration()
                    .build()
                INSTANCE = instance
                instance
            }
        }
    }
}

// Converters for complex types
package com.bluetoothpersonallink.core.db

import androidx.room.TypeConverter
import com.bluetoothpersonallink.protocol.DeviceId
import com.bluetoothpersonallink.protocol.SessionId
import com.bluetoothpersonallink.protocol.ChannelId
import kotlinx.serialization.json.Json

class Converters {

    private val json = Json { ignoreUnknownKeys = true }

    @TypeConverter
    fun deviceIdToString(deviceId: DeviceId?): String? {
        return deviceId?.encodeToBase64()
    }

    @TypeConverter
    fun stringToDeviceId(str: String?): DeviceId? {
        return str?.decodeToDeviceId()
    }

    @TypeConverter
    fun sessionIdToString(sessionId: SessionId?): String? {
        return sessionId?.encodeToBase64()
    }

    @TypeConverter
    fun stringToSessionId(str: String?): SessionId? {
        return str?.decodeToSessionId()
    }

    @TypeConverter
    fun channelIdToInt(channelId: ChannelId?): Int {
        return channelId?.value ?: 0
    }

    @TypeConverter
    fun intToChannelId(value: Int): ChannelId {
        return ChannelId(value)
    }

    @TypeConverter
    fun mapToString(map: Map<String, String>?): String? {
        return map?.let { json.encodeToString(it) }
    }

    @TypeConverter
    fun stringToMap(str: String?): Map<String, String>? {
        return str?.let { json.decodeFromString(it) }
    }

    @TypeConverter
    fun listToString(list: List<String>?): String? {
        return list?.let { json.encodeToString(it) }
    }

    @TypeConverter
    fun stringToList(str: String?): List<String>? {
        return str?.let { json.decodeFromString(it) }
    }

    @TypeConverter
    fun byteArrayToString(bytes: ByteArray?): String? {
        return bytes?.encodeToBase64()
    }

    @TypeConverter
    fun stringToByteArray(str: String?): ByteArray? {
        return str?.decodeToByteArray()
    }
}

// Extension functions for DeviceId
package com.bluetoothpersonallink.protocol

import android.util.Base64
import kotlinx.serialization.KSerializer
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

fun DeviceId.encodeToBase64(): String = Base64.encodeToString(value, Base64.NO_WRAP)

fun String.decodeToDeviceId(): DeviceId = DeviceId(Base64.decode(this, Base64.NO_WRAP))

fun SessionId.encodeToBase64(): String = Base64.encodeToString(value, Base64.NO_WRAP)

fun String.decodeToSessionId(): SessionId = SessionId(Base64.decode(this, Base64.NO_WRAP))