package com.bluetoothpersonallink.core.db.dao

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import androidx.room.Update
import com.bluetoothpersonallink.core.db.entities.Device
import com.bluetoothpersonallink.protocol.DeviceId
import kotlinx.coroutines.flow.Flow

@Dao
interface DeviceDao {
    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(device: Device): Long

    @Update
    suspend fun update(device: Device): Int

    @Query("SELECT * FROM devices WHERE id = :id")
    suspend fun getById(id: DeviceId): Device?

    @Query("SELECT * FROM devices WHERE address = :address")
    suspend fun getByAddress(address: String): Device?

    @Query("SELECT * FROM devices WHERE paired = 1")
    suspend fun getPairedDevices(): List<Device>

    @Query("SELECT * FROM devices WHERE trusted = 1")
    suspend fun getTrustedDevices(): List<Device>

    @Query("SELECT * FROM devices")
    suspend fun getAll(): List<Device>

    @Query("SELECT * FROM devices")
    fun getAllFlow(): Flow<List<Device>>

    @Query("SELECT * FROM devices WHERE paired = 1")
    fun getPairedDevicesFlow(): Flow<List<Device>>

    @Query("DELETE FROM devices WHERE id = :id")
    suspend fun delete(id: DeviceId): Int
}