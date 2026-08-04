package com.bluetoothpersonallink.core.config

import android.content.Context
import androidx.datastore.preferences.preferencesDataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.mutablePreferencesOf
import androidx.datastore.preferences.core.stringPreferencesKey
import com.bluetoothpersonallink.protocol.DeviceId
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.tasks.await
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.util.concurrent.ConcurrentHashMap

class ConfigManager(private val context: Context) {

    private val dataStore = context.preferencesDataStore("bpl_config")
    private val configCache = ConcurrentHashMap<String, String>()
    private val json = Json { ignoreUnknownKeys = true }

    private val KEY_DEVICE_ID = stringPreferencesKey("device_id")
    private val KEY_PSK = stringPreferencesKey("psk")
    private val KEY_DEVICE_NAME = stringPreferencesKey("device_name")
    private val KEY_SERVICE_UUID = stringPreferencesKey("service_uuid")
    private val KEY_AUTO_CONNECT = stringPreferencesKey("auto_connect")
    private val KEY_LOG_LEVEL = stringPreferencesKey("log_level")

    // Get or create device ID
    suspend fun getOrCreateDeviceId(): DeviceId {
        val prefs = dataStore.data.first()
        prefs[KEY_DEVICE_ID]?.let { idStr ->
            return DeviceId(idStr.decodeToByteArray())
        }

        // Generate new device ID
        val deviceId = DeviceId.generate()
        val encoded = deviceId.encodeToBase64()

        dataStore.edit {
            it[KEY_DEVICE_ID] = encoded
        }

        return deviceId
    }

    // PSK management
    suspend fun setPsk(psk: ByteArray) {
        val encoded = psk.encodeToBase64()
        dataStore.edit {
            it[KEY_PSK] = encoded
        }
        configCache[KEY_PSK.name] = encoded
    }

    suspend fun getPsk(): ByteArray? {
        val prefs = dataStore.data.first()
        return prefs[KEY_PSK]?.decodeFromBase64()
    }

    // Device name
    suspend fun setDeviceName(name: String) {
        dataStore.edit {
            it[KEY_DEVICE_NAME] = name
        }
    }

    suspend fun getDeviceName(): String {
        val prefs = dataStore.data.first()
        return prefs[KEY_DEVICE_NAME] ?: "BPL Android"
    }

    // Service UUID
    suspend fun setServiceUuid(uuid: String) {
        dataStore.edit {
            it[KEY_SERVICE_UUID] = uuid
        }
    }

    suspend fun getServiceUuid(): String {
        val prefs = dataStore.data.first()
        return prefs[KEY_SERVICE_UUID] ?: "B7E5E0F0-1A2B-4C3D-8E9F-A0B1C2D3E4F5"
    }

    // Auto connect
    suspend fun setAutoConnect(enabled: Boolean) {
        dataStore.edit {
            it[KEY_AUTO_CONNECT] = enabled.toString()
        }
    }

    suspend fun getAutoConnect(): Boolean {
        val prefs = dataStore.data.first()
        return prefs[KEY_AUTO_CONNECT]?.toBoolean() ?: true
    }

    // Log level
    suspend fun setLogLevel(level: String) {
        dataStore.edit {
            it[KEY_LOG_LEVEL] = level
        }
    }

    suspend fun getLogLevel(): String {
        val prefs = dataStore.data.first()
        return prefs[KEY_LOG_LEVEL] ?: "info"
    }

    // Generic config access
    suspend fun getString(key: String): String? {
        val prefs = dataStore.data.first()
        return prefs[stringPreferencesKey(key)]
    }

    suspend fun setString(key: String, value: String) {
        dataStore.edit {
            it[stringPreferencesKey(key)] = value
        }
    }

    suspend fun getBoolean(key: String, default: Boolean = false): Boolean {
        val prefs = dataStore.data.first()
        return prefs[stringPreferencesKey(key)]?.toBoolean() ?: default
    }

    suspend fun setBoolean(key: String, value: Boolean) {
        dataStore.edit {
            it[stringPreferencesKey(key)] = value.toString()
        }
    }

    suspend fun getInt(key: String, default: Int = 0): Int {
        val prefs = dataStore.data.first()
        return prefs[stringPreferencesKey(key)]?.toInt() ?: default
    }

    suspend fun setInt(key: String, value: Int) {
        dataStore.edit {
            it[stringPreferencesKey(key)] = value.toString()
        }
    }

    suspend fun getLong(key: String, default: Long = 0L): Long {
        val prefs = dataStore.data.first()
        return prefs[stringPreferencesKey(key)]?.toLong() ?: default
    }

    suspend fun setLong(key: String, value: Long) {
        dataStore.edit {
            it[stringPreferencesKey(key)] = value.toString()
        }
    }

    // JSON serialization for complex objects
    suspend fun <T> getObject(key: String, clazz: Class<T>): T? {
        val prefs = dataStore.data.first()
        val json = prefs[stringPreferencesKey(key)]
        return json?.let { json.decodeFromString(clazz) }
    }

    suspend fun <T> setObject(key: String, value: T) {
        val json = json.encodeToString(value)
        dataStore.edit {
            it[stringPreferencesKey(key)] = json
        }
    }

    // Clear all config
    suspend fun clear() {
        dataStore.edit {
            it.clear()
        }
        configCache.clear()
    }

    companion object {
        fun create(context: Context): ConfigManager = ConfigManager(context)
    }
}

// ByteArray extensions for Base64 encoding
import android.util.Base64

fun ByteArray.encodeToBase64(): String = Base64.encodeToString(this, Base64.NO_WRAP)

fun String.decodeToByteArray(): ByteArray = Base64.decode(this, Base64.NO_WRAP)

fun String.decodeFromBase64(): ByteArray = Base64.decode(this, Base64.NO_WRAP)